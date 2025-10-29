# Distributed Actor Store - Architecture and Implementation Roadmap

## Executive Summary

This document outlines the architecture, key design decisions, and implementation roadmap for a distributed in-memory Actor Store that serves as the foundation for the Distributed Reactive Actors runtime. The store provides ACID guarantees, fast leader failover, ordered change data capture, and pluggable backend support. The initial implementation will be built in Rust as a standalone service exposing CRUD and streaming APIs, allowing independent development and testing before integration with the language runtime.

---

## Part 1: Architecture Overview

### 1.1 System Architecture

The Actor Store is designed as a three-tier architecture comprising storage, consensus, and API layers.

**Storage Layer** manages actor data persistence and replication across the cluster. Each actor is stored as a triple consisting of an ObjectID key, a JSON value, and a version number. The storage layer maintains N replicas of each actor using consistent hashing for placement. Write operations propagate to all replicas with quorum acknowledgment ensuring durability. The layer employs write-ahead logging for crash recovery and supports both in-memory and disk-backed persistence modes.

**Consensus Layer** coordinates distributed operations and maintains cluster consistency. This layer implements a leader-based replication protocol using Raft consensus for leader election and log replication. The consensus layer ensures that all replicas agree on the order of operations, preventing split-brain scenarios during network partitions. It provides sub-second leader failover through heartbeat monitoring and automated leader election when failures occur.

**API Layer** exposes CRUD operations and change stream subscriptions through both gRPC and HTTP interfaces. The API layer handles request routing, directing writes to the current leader and distributing reads across replicas based on consistency requirements. It provides exactly-once semantics through operation deduplication and supports batch operations for improved throughput.

### 1.2 Core Components

**Cluster Manager** maintains the membership view of all nodes in the cluster and monitors node health through periodic heartbeats. When the cluster manager detects node failures, it triggers replica rebalancing and leader election as needed. The manager also handles graceful node additions and removals by coordinating data migration across the cluster.

**Replication Engine** implements the leader-based replication protocol. The engine on the leader node receives write requests, appends them to the write-ahead log, and replicates log entries to follower nodes. Once a quorum of followers acknowledges a log entry, the engine commits the entry and makes it visible to readers. The replication engine ensures that followers eventually catch up even after temporary disconnections through log replay mechanisms.

**Storage Engine** provides the physical storage layer for actor data. The engine supports both in-memory storage using concurrent hash maps and disk-based persistence using embedded key-value stores like RocksDB. The storage engine maintains indexes on actor IDs for fast lookups and supports range scans for batch operations. It implements copy-on-write semantics for in-memory operations to enable lock-free reads during write operations.

**Change Data Capture** streams all data modifications to subscribers in order. The CDC component maintains a durable event log of all create, update, and delete operations. Subscribers can resume from any position in the log using sequence numbers, enabling fault-tolerant stream processing. The CDC system guarantees ordering within each actor but allows concurrent delivery of changes to different actors for improved throughput.

**Operation Log** provides exactly-once semantics by tracking operation identifiers and their completion status. Before executing any operation, the system checks the operation log to detect duplicate requests. This mechanism prevents repeated execution of the same operation during retries or failover scenarios.

### 1.3 Data Model

Each actor in the store is represented as an immutable triple with three reserved fields.

The **id field** contains a string-typed unique identifier that never changes after actor creation. This identifier serves as both the primary key for storage and the routing key for consistent hashing. The system generates unique IDs automatically if not provided during creation, using a combination of timestamp, node identifier, and random component to ensure global uniqueness.

The **value field** holds arbitrary JSON data representing the actor state. The system imposes no schema on this field, allowing applications complete flexibility in structuring their data. The value is stored as a serialized byte array internally but exposed as parsed JSON through the API. Updates to the value field always replace the entire JSON document atomically.

The **version field** contains a monotonically increasing counter that increments automatically on each update operation. The version enables optimistic concurrency control through compare-and-swap operations and serves as a logical timestamp for ordering events in the change stream. Each replica maintains the same version number for a given actor state, ensuring consistency across the cluster.

The store organizes actors into namespaces based on their type names. Each namespace operates as an independent collection with its own consistent hash ring and replication settings, allowing different actor types to have different replication factors or storage backends. This organization mirrors the concept of collections in MongoDB or databases in Redis.

---

## Part 2: Replication Protocol

### 2.1 Leader-Based Replication

The system employs a leader-based replication model where one node serves as the leader for each namespace. All write operations for actors in a namespace must flow through that namespace's leader, while read operations can be served by any replica depending on the desired consistency level.

When a client submits a write operation, the system routes the request to the leader node responsible for the target actor's namespace. The leader validates the operation, assigns it a sequence number, and appends it to its local write-ahead log. The leader then replicates the log entry to all follower nodes in parallel. Each follower writes the entry to its own WAL and acknowledges receipt to the leader. Once a quorum of nodes has acknowledged the entry, the leader commits it by applying the operation to its in-memory state and responding to the client. The leader continues propagating committed entries to slower followers asynchronously.

This protocol ensures that committed operations are durable across node failures while maintaining high performance for the common case where most nodes respond quickly. The quorum requirement allows the system to tolerate up to floor of N divided by two node failures while continuing to accept writes.

### 2.2 Raft Integration

The store integrates Raft consensus to provide leader election, log replication, and cluster membership management. Raft operates at the namespace level, with each namespace running an independent Raft group. This design allows different namespaces to have different leaders, distributing the write load across the cluster.

The Raft implementation handles several critical scenarios. During normal operation, the leader sends periodic heartbeats to followers to maintain its leadership. If followers do not receive a heartbeat within the election timeout, they initiate a new election by incrementing the term number and requesting votes from other nodes. A candidate becomes leader if it receives votes from a majority of nodes. The newly elected leader then synchronizes its log with all followers before accepting new write operations.

When a follower falls behind the leader, the Raft protocol enables it to catch up through log replay. The leader maintains the log index of each follower and sends appropriate log entries to bring lagging followers up to date. If a follower's log diverges from the leader's due to a previous leader failure, Raft's consistency check detects the divergence and forces the follower to truncate its log and replay entries from the leader.

Network partitions represent a critical challenge for distributed systems. Under partition, the Raft protocol ensures that at most one partition can form a quorum and elect a leader. The minority partition will fail to elect a leader and enter a read-only mode. When the partition heals, nodes in the minority partition recognize the higher term number from the majority partition, step down from any leadership role, and synchronize their logs with the current leader.

### 2.3 Write Path Details

The complete write path involves multiple stages to ensure atomicity and durability. When a client issues a write request, the API layer first verifies the request format and extracts the target actor ID. The system then uses consistent hashing to determine which namespace owns this actor and routes the request to the current leader of that namespace.

The leader receives the write request and performs validation. For update operations, the leader checks whether the specified version matches the current version in storage, rejecting mismatched versions with a conflict error. For create operations, the leader verifies that no actor with the same ID already exists. If validation succeeds, the leader generates a unique operation ID by combining the actor ID, operation type, client ID, and timestamp.

Before executing the operation, the leader checks the operation log to detect duplicate requests. If the operation ID already exists with a completed status, the leader returns the cached result without re-executing the operation. This deduplication enables safe retries from clients without risking duplicate side effects.

For new operations, the leader appends an entry to its write-ahead log containing the operation details. The entry includes the operation ID, actor ID, operation type, new value, timestamp, and any preconditions. The leader then flushes this entry to durable storage before proceeding.

After logging locally, the leader sends replicate requests to all follower nodes. Each follower appends the entry to its own WAL and sends an acknowledgment back to the leader. The leader maintains a count of acknowledgments and proceeds to the commit phase once it receives responses from a quorum of nodes.

Committing the operation involves multiple atomic steps. The leader applies the operation to its in-memory actor store, updating the actor's value and incrementing its version. Simultaneously, the leader writes the operation to the change data capture log with an assigned sequence number. The leader also updates the operation log to mark this operation ID as completed. Finally, the leader responds to the client with the updated actor state.

After responding to the client, the leader continues replicating to any followers that have not yet acknowledged. These lagging followers eventually receive and apply the committed entry, bringing them back into sync with the leader.

### 2.4 Read Path Details

The read path design balances consistency requirements against performance needs. The system supports two consistency levels for read operations.

Strong consistency reads always fetch data from the current leader node. When a client requests a strong read, the API layer routes the request to the leader for the relevant namespace. Before returning data, the leader confirms that it is still the leader by checking whether it has received acknowledgments from a quorum of nodes within the heartbeat interval. This check prevents a partitioned former leader from serving stale data. Strong consistency reads guarantee that clients see their own writes and all committed writes from other clients, providing linearizable semantics.

Eventual consistency reads can be served by any replica, including followers. These reads do not require communication with the leader, offering lower latency and higher throughput. However, eventual reads may return slightly stale data if the local replica has not yet received the latest committed entries from the leader. The system includes version numbers in read responses, allowing clients to detect staleness and retry with strong consistency if needed.

For batch read operations, the system optimizes the read path by parallel fetching from multiple replicas. The API layer partitions the requested actor IDs by their assigned replicas and issues concurrent read requests. This parallelization significantly improves throughput for large batch reads while maintaining the requested consistency level for each individual read.

---

## Part 3: Change Data Capture

### 3.1 Event Stream Architecture

The change data capture system provides an ordered stream of all modifications to actors in the store. Every create, update, and delete operation generates an event that is appended to a durable event log. Clients subscribe to this log to receive notifications of changes, enabling reactive processing patterns.

The event log is implemented as an append-only sequence where each event receives a monotonically increasing sequence number. The sequence numbers establish a total ordering of events across all actors in a namespace. Within this total ordering, events for the same actor maintain their causal order since all operations on an actor flow through the same leader.

The CDC system persists events to durable storage before making them visible to subscribers. This durability guarantee ensures that subscribers can reliably process all changes without data loss. Each event includes comprehensive metadata including the operation type, actor ID, previous value, new value, version numbers, timestamp, and sequence number.

### 3.2 Subscription Model

Clients subscribe to the event stream by specifying a starting position and optional filters. The starting position can be either the beginning of the log, the current end of the log, or a specific sequence number. Starting from the beginning enables new subscribers to replay the entire history of changes and build a complete view of current state. Starting from the current end enables subscribers to process only future changes. Starting from a specific sequence number enables subscribers to resume after a failure without missing or duplicating events.

The system supports filtering subscriptions by actor type to reduce unnecessary data transfer. A subscriber interested only in changes to User actors can register a filter that excludes events for other actor types. The CDC system applies these filters at the source before transmitting events over the network.

Subscribers receive events through a streaming RPC connection that pushes new events as they occur. The connection uses backpressure mechanisms to prevent overwhelming slow subscribers. If a subscriber cannot keep up with the event rate, the system buffers events up to a configurable limit. When the buffer fills, the system can either drop the connection or block new events depending on configuration.

### 3.3 Ordering Guarantees

The CDC system provides strong ordering guarantees that enable correct reactive processing. Events for the same actor are delivered in causal order, meaning that if operation A happens before operation B on actor X, then the event for A will be delivered before the event for B to all subscribers. This per-actor ordering enables subscribers to maintain consistent derived state.

Events for different actors may be delivered concurrently or in any order. The system does not guarantee any ordering relationship between operations on different actors unless the application explicitly establishes causality through version dependencies. This relaxed ordering for independent actors allows the CDC system to achieve high throughput through parallel event delivery.

The sequence numbers in the event log establish a global ordering that can be used to reason about concurrent operations. If two operations on different actors have sequence numbers S1 and S2 where S1 is less than S2, then operation 1 was committed before operation 2. Applications can use these sequence numbers to implement distributed snapshots or cross-actor consistency checks.

### 3.4 Subscriber Resume and Fault Tolerance

Subscribers must be able to recover from failures and resume processing without losing events or processing duplicates. The CDC system enables this through durable sequence numbers and exactly-once delivery semantics.

Each subscriber maintains a checkpoint representing the sequence number of the last successfully processed event. Before processing an event, the subscriber performs its application logic and updates any derived state. After successfully completing this work, the subscriber advances its checkpoint to the current event's sequence number and persists this checkpoint to durable storage. If the subscriber crashes before updating the checkpoint, it will resume from the previous checkpoint and reprocess the event.

To handle reprocessing safely, subscribers must implement idempotent event handlers. The system provides each event with a unique event ID derived from the operation ID. Subscribers can track processed event IDs to detect and skip duplicates during resume scenarios.

The CDC system retains events in the log for a configurable retention period. This retention window must be longer than the maximum expected downtime for any subscriber to ensure that subscribers can always resume without missing events. Once events age beyond the retention period, the system removes them from the log to reclaim storage space.

---

## Part 4: Failure Scenarios and Handling

### 4.1 Leader Node Failure

Leader failure represents the most critical failure scenario since all writes depend on the leader. The system must detect leader failures quickly and elect a new leader with minimal downtime.

Detection occurs through the Raft heartbeat mechanism. Followers expect to receive heartbeats from the leader at regular intervals. If a follower does not receive a heartbeat within the election timeout, it assumes the leader has failed and initiates a leader election. The election timeout is typically set to 150 to 300 milliseconds, allowing detection of leader failure within a few hundred milliseconds.

Upon detecting a potential leader failure, a follower transitions to candidate state, increments the term number, and requests votes from all other nodes. The candidate votes for itself and waits for responses from other nodes. A node grants its vote to a candidate if the candidate's log is at least as up-to-date as its own log and if it has not already voted for another candidate in this term. A candidate becomes the new leader upon receiving votes from a majority of nodes.

The newly elected leader takes several steps to establish its authority. First, it replicates a no-op entry to its log to commit any entries from previous terms. This ensures that the new leader's log contains all committed entries. Second, it begins sending heartbeats to all followers to assert its leadership and prevent new elections. Third, it processes any pending write requests from clients. Finally, it synchronizes its log with all followers by identifying and replicating any missing entries.

The entire leader election process typically completes in less than one second, resulting in brief unavailability for write operations. Read operations can continue being served by followers during the election using eventual consistency semantics. Once a new leader is elected, normal operation resumes and write operations can proceed.

### 4.2 Follower Node Failure

Follower failures have less impact than leader failures since followers do not handle write requests directly. When a follower fails, the leader continues operating normally using the remaining followers to form a quorum.

The leader detects follower failure through missing acknowledgments for replicate requests. If a follower does not respond to several consecutive replicate attempts, the leader marks it as failed and removes it from the active quorum calculation. Write operations continue to proceed as long as a majority of nodes remains available.

When the failed follower recovers, it reconnects to the leader and begins catching up on missed log entries. The follower sends its last known log index to the leader. The leader identifies the divergence point between its log and the follower's log and begins sending all entries from that point forward. The follower applies these entries to its local state until it catches up with the leader's current position. Once synchronized, the follower rejoins the active quorum and resumes normal replication.

During the catch-up process, the follower does not participate in quorum calculations for new writes. This prevents a stale follower from affecting the availability of the system. The leader only includes the follower in quorum calculations after verifying that its log matches the leader's log up to the current commit index.

### 4.3 Network Partition

Network partitions split the cluster into multiple disjoint sets of nodes that cannot communicate with each other. The Raft consensus protocol ensures correctness during partitions by requiring that only one partition can make progress on writes.

Consider a partition that splits a five-node cluster into groups of three nodes and two nodes. The three-node partition holds a majority and can elect a leader who continues accepting writes. The two-node partition cannot form a majority and therefore cannot elect a leader. Nodes in the minority partition detect that they cannot reach a quorum and reject any write requests with an error indicating unavailability. Read requests in the minority partition can still be served using eventual consistency semantics but will return increasingly stale data as time passes.

When the partition heals and the two groups can communicate again, the nodes in the minority partition recognize the higher term number from the majority partition. These nodes update their logs to match the current leader's log, discarding any conflicting entries that were never committed. The cluster then returns to normal operation with all nodes participating in replication.

Applications must handle partition scenarios gracefully by implementing appropriate retry logic and degraded functionality modes. The store provides clear error responses during partitions to help applications detect and respond to reduced availability.

### 4.4 Simultaneous Multi-Node Failures

If multiple nodes fail simultaneously and the cluster loses quorum, the system becomes unavailable for writes but can continue serving stale reads. Consider a five-node cluster that loses three nodes to a datacenter power failure. The remaining two nodes recognize that they cannot form a quorum and transition to read-only mode.

Recovery from multi-node failure requires manual intervention to restore quorum. Operators must repair or replace the failed nodes and ensure they rejoin the cluster. If the failed nodes had committed data that was not yet replicated to the surviving nodes, the system must run a reconciliation process to recover this data from write-ahead logs or backups.

To prevent complete data loss during catastrophic failures, the system should be configured with asynchronous cross-datacenter replication. A separate cluster in a different datacenter maintains a replica of all data with some replication lag. If the primary datacenter becomes completely unavailable, operators can promote the disaster recovery cluster to serve traffic while the primary is restored.

### 4.5 Split-Brain Prevention

The quorum-based approach inherently prevents split-brain scenarios where multiple leaders accept conflicting writes. Since becoming a leader requires votes from a majority of nodes, at most one leader can exist for any term. Even if network delays cause a node to believe it is still the leader after a new leader has been elected, the old leader will fail to achieve quorum for new writes and will soon recognize the new leader's higher term number.

The system includes additional safety mechanisms to handle edge cases. Leaders include their term number in all replicate requests. Followers reject replicate requests from leaders with stale term numbers. Clients include timestamps in their requests, and the system rejects requests with timestamps too far in the past or future to prevent replay attacks.

---

## Part 5: Implementation Plan

### 5.1 Technology Stack

The Actor Store will be implemented in Rust to leverage its performance, memory safety, and strong concurrency primitives. Rust provides the necessary low-level control for building high-performance distributed systems while preventing entire classes of bugs through its ownership model and type system.

The core dependencies include several proven libraries. The Raft consensus implementation will use the openraft crate, which provides a well-tested implementation of the Raft protocol with active maintenance and good documentation. For RPC communication, the system will use tonic for gRPC and hyper for HTTP, providing both protocol options with minimal overhead. The storage layer will initially use dashmap for concurrent in-memory storage and rocksdb for disk-backed persistence. For serialization, the system will use serde_json for JSON handling and bincode for efficient binary encoding of internal data structures.

### 5.2 Module Architecture

The codebase will be organized into several independent modules with clear interfaces between them.

The **cluster module** manages node membership, health monitoring, and cluster topology. This module maintains the list of active nodes, handles join and leave operations, and distributes the consistent hash ring to all nodes. It exposes interfaces for querying the current cluster state and subscribing to membership changes.

The **consensus module** wraps the Raft implementation and provides a simpler interface for the rest of the system. This module handles leader election, log replication, and ensures that all state transitions go through consensus. It exposes operations for proposing new entries, reading committed entries, and querying current leadership status.

The **storage module** provides the physical storage layer with support for multiple backends. The module defines a Storage trait with methods for CRUD operations, range scans, and snapshots. Concrete implementations include MemoryStorage for in-memory operation and RocksStorage for disk-backed persistence. Applications can implement custom storage backends by implementing the Storage trait.

The **replication module** coordinates data replication across the cluster. This module implements the write path by receiving operations from the API layer, proposing them through consensus, and applying committed operations to storage. It also handles synchronization of new or recovering nodes.

The **cdc module** implements the change data capture system. This module maintains the event log, handles subscription requests, and streams events to subscribers. It ensures proper ordering and delivery guarantees for all events.

The **api module** exposes the public interfaces for interacting with the store. This module implements both gRPC and HTTP servers that handle client requests, perform request validation, route requests to the appropriate handlers, and format responses.

### 5.3 Development Phases

The implementation will proceed through four phases, each delivering a working system with incrementally added capabilities.

**Phase One: Single-Node Core** implements the basic functionality on a single node. This phase includes the storage engine with in-memory and RocksDB backends, CRUD operations with version management, the operation log for deduplication, and the CDC event stream. The deliverable from this phase is a single-node store that can handle all operations with durability and change streaming. This phase establishes the core data model and validates the API design before adding distribution complexity.

**Phase Two: Consensus and Replication** adds clustering and distributed coordination. This phase integrates the Raft consensus module, implements leader election and failover, adds the replication protocol with quorum writes, and implements log synchronization for recovering nodes. The deliverable from this phase is a distributed store that maintains consistency across multiple nodes and handles leader failures automatically. This phase validates the replication protocol and failure handling mechanisms.

**Phase Three: Production Readiness** adds features necessary for production deployment. This phase implements monitoring and observability with metrics and distributed tracing, adds configurable consistency levels for reads, implements disk-based WAL persistence, adds snapshot and restore capabilities, and creates operational tooling for cluster management. The deliverable from this phase is a production-ready distributed store suitable for real workloads with proper observability and operational controls.

**Phase Four: Optimization and Scaling** focuses on performance improvements and advanced features. This phase implements batch operations for improved throughput, adds pipelining for reduced latency, optimizes memory usage and garbage collection, implements compression for storage and network efficiency, and adds advanced CDC features like filtered subscriptions. The deliverable from this phase is a highly optimized store capable of handling demanding production workloads.

### 5.4 API Design

The store exposes both gRPC and HTTP APIs for maximum flexibility. The gRPC API provides higher performance and streaming capabilities, while the HTTP API offers easier integration and debugging.

The core CRUD operations include create, read, update, delete, and compare-and-swap. The create operation takes an actor ID, value, and optional metadata, returning the created actor with version one. The read operation takes an actor ID and consistency level, returning the current actor state and version. The update operation takes an actor ID, new value, and optional expected version, returning the updated actor with incremented version. The delete operation takes an actor ID and removes the actor from the store. The compare-and-swap operation atomically updates an actor only if its current version matches an expected version.

Batch operations allow processing multiple actors in a single request. The batch read operation takes a list of actor IDs and returns all found actors. The batch write operation takes a list of operations and applies them atomically when possible or reports partial failures.

The CDC subscription API allows clients to stream changes. The subscribe operation takes a starting position, optional filters, and a callback handler. The system invokes the callback for each matching event and handles backpressure automatically. Clients can cancel subscriptions or adjust filters dynamically.

Administrative operations provide cluster management capabilities. The list nodes operation returns the current cluster membership. The rebalance operation initiates data migration to balance load across nodes. The snapshot operation creates a consistent backup of all data. The restore operation loads data from a previous snapshot.

### 5.5 Testing Strategy

The implementation requires comprehensive testing at multiple levels to ensure correctness under normal operation and failure scenarios.

Unit tests will cover individual components in isolation. These tests validate the storage engine operations, the operation log deduplication logic, the CDC event generation, and the consistent hashing distribution. Unit tests use mocked dependencies and run quickly in continuous integration.

Integration tests will verify interactions between components. These tests validate the complete CRUD workflow through the API, the replication flow from leader to followers, the CDC subscription and delivery mechanism, and the operation deduplication across retries. Integration tests use real instances of all components but run in controlled environments.

Chaos tests will validate behavior under failure conditions. These tests inject network partitions during write operations, kill leader nodes during active replication, simulate slow followers to verify catch-up logic, introduce disk failures to test persistence handling, and generate concurrent conflicting writes to validate resolution. Chaos tests use fault injection frameworks to create realistic failure scenarios.

Performance tests will establish throughput and latency characteristics. These tests measure write latency with varying cluster sizes, read throughput for strong and eventual consistency, CDC stream delivery rate, and system behavior under sustained high load. Performance tests provide benchmarks for regression detection and capacity planning.

---

## Part 6: Deployment Considerations

### 6.1 Cluster Sizing

The optimal cluster size depends on workload characteristics and availability requirements. A minimum cluster size of three nodes provides fault tolerance for a single node failure while maintaining quorum. Five nodes can tolerate two simultaneous failures and provide better read scalability. Larger clusters offer diminishing returns for fault tolerance but can improve read throughput through additional replicas.

The replication factor determines how many copies of each actor exist across the cluster. A replication factor of three provides good durability while keeping storage overhead reasonable. Higher replication factors increase storage costs but improve fault tolerance and read availability. The replication factor should be odd to simplify quorum calculations.

### 6.2 Hardware Requirements

Each node should have sufficient resources to handle its share of the workload. CPU requirements depend primarily on request rate and complexity of operations. Modern multi-core processors can handle thousands of requests per second per node. Memory requirements depend on the working set size and whether using in-memory or disk-backed storage. In-memory deployments require enough RAM to hold all actor data plus overhead for operating system and runtime. Disk-backed deployments need less memory but require fast SSDs to maintain acceptable latency.

Network bandwidth and latency significantly impact distributed system performance. Nodes should be connected via low-latency networks with sufficient bandwidth to handle replication traffic. Data center deployments with sub-millisecond latency between nodes provide the best performance. Cloud deployments should use regional clusters rather than cross-region clusters when possible to minimize replication latency.

### 6.3 Monitoring and Operations

Production deployments require comprehensive monitoring to detect issues before they impact users. Key metrics include request rates and latencies for each operation type, replication lag between leader and followers, event log size and growth rate, memory and disk utilization, and cluster health status.

Operational procedures should cover common maintenance tasks. Adding new nodes to increase capacity requires updating the cluster membership and waiting for data rebalancing to complete. Removing nodes requires draining their data to other nodes before shutdown. Upgrading software versions requires rolling upgrades where nodes are updated one at a time while maintaining quorum. Creating backups requires coordinating snapshots across nodes to ensure consistency.

---

## Part 7: Future Extensions

### 7.1 Pluggable Backends

The initial implementation focuses on the in-memory distributed store, but the architecture supports pluggable backends for different use cases. Future backends might include MongoDB for durable storage with existing operational tooling, PostgreSQL for transactional capabilities and SQL access, and Kafka for event sourcing patterns. Each backend would implement the Storage trait and provide appropriate configuration options.

### 7.2 Cross-Datacenter Replication

For global applications, the system could support asynchronous replication across datacenter boundaries. This would involve maintaining separate Raft clusters in each datacenter with asynchronous event streaming between them. Cross-datacenter replication provides disaster recovery capabilities and enables serving users from geographically closer locations.

### 7.3 Secondary Indexes

The current design uses actor ID as the only index. Secondary indexes would enable efficient queries by other attributes. For example, indexing users by email address would allow quick lookups without knowing the actor ID. Maintaining secondary indexes in a distributed setting requires careful coordination to ensure consistency.

### 7.4 Transactions

Multi-actor transactions would enable atomic updates across multiple actors. This could be implemented using two-phase commit coordinated by a transaction manager. Transactions would provide stronger consistency guarantees but at the cost of additional coordination overhead and reduced concurrency.

---

## Summary

This architecture provides a solid foundation for building a distributed Actor Store that meets the requirements of the Distributed Reactive Actors runtime. The leader-based replication with Raft consensus ensures strong consistency and fast failover. The change data capture system enables reactive processing patterns with ordering guarantees. The phased implementation plan allows incremental development with working systems at each stage.

The decision to implement the store as a standalone service in Rust allows focused development of this critical component before integrating it with the language runtime. The clean API boundaries enable testing in isolation and provide flexibility for future enhancements like pluggable backends and cross-datacenter replication.
