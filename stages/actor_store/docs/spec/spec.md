# Actor Store - Complete Specification

> **Purpose**: This specification serves as a comprehensive reference for implementing the Actor Store through LLM-assisted code generation. It combines functional requirements, architectural design, acceptance criteria, test cases, and chaos testing scenarios.

---

## Table of Contents

1. [Overview](#overview)
2. [Data Model](#data-model)
3. [API Specification](api-specification.md#api-specification)
4. [Implementation Phases](#implementation-phases)
5. [Acceptance Scenarios](acceptance-scenarios.md#acceptance-scenarios)
6. [Test Cases](test-cases.md#test-cases)
7. [Chaos Tests](test-cases.md#chaos-tests)
8. [Performance Requirements](#performance-requirements)
9. [Architecture References](#architecture-references)

---

## Overview

### What is Actor Store?

Actor Store is a distributed, Redis-like key-value store specifically designed for storing Actor objects with versioning and change data capture capabilities. It provides ACID guarantees, fast failover, and ordered change streams.

### Core Capabilities

- **CRUD Operations**: Insert, read, update, delete operations with version management
- **Optimistic Concurrency**: Compare-and-swap operations using version numbers
- **Change Data Capture**: Subscribe to all changes with resume capability
- **In-Memory & Persistent**: Start with in-memory, scale to disk-based storage
- **Distributed**: Raft-based consensus for high availability (Phase 3)

### Key Characteristics

- **Storage Model**: Key-Value with JSON values
- **Atomicity**: Single-operation atomicity; multi-entity transactions in future phases
- **Isolation**: Per-key serializability; leader reads are linearizable
- **Consistency**: Strong consistency through leader-based replication
- **Durability**: Write-ahead logging with configurable persistence
- **Scalability**: Horizontal scaling through consistent hashing
- **Observable**: Complete change stream for reactive processing

### Component Map

- Consensus Layer (Paxos Like algo, similar to zookeeper)
- Actor Stores (Replicated, 1 primary)
- CDC (Replicated, distributed, but kafka like, subscribable)

---

## Data Model

### ActorData Structure

Each stored entity is represented as an **ActorData** triple:

```rust
pub struct ActorData {
    /// Unique identifier (String type, similar to Redis keys)
    pub id: String,

    /// JSON value (arbitrary structure)
    pub value: serde_json::Value,

    /// Monotonically increasing version counter
    /// Starts at 0 on insert, increments on every update
    pub version: u64,
}
```

### Key Constraints

**Key (id field)**:
- Type: `String` (to be generalised to arbitrary types implementing `Eq` and `Hash` in future, this type can vary per Actor in future)
- Must be non-empty
- Must be valid UTF-8
- Recommended max length: 512 bytes
- Similar to Redis keys (not UUID-based ObjectID)
- Keys must implement `Eq`, `Hash` traits for HashMap/DashMap based implementation.
- **Immutability**: Cannot be changed post creation of Actor for now.

**Value (value field)**:
- Type: `serde_json::Value`
- Can be any valid JSON: object, array, string, number, boolean, null
- No schema enforcement (schema-less storage)
- Recommended max size: 10 MB per value
- Stored in compact(non-pretty/uglified) JSON format for storage efficiency when writing to disk for persistence.

**Version (version field)**:
- Type: `u64`
- Starts at 0 on insert
- Increments by 1 on every update
- Never decreases
- Used for optimistic concurrency control

### Example ActorData Instances

```json
// New Entry
{
  "id": "user-12345",
  "value": {
    "name": "Alice",
    "email": "alice@example.com",
    "balance": 1000.50
  },
  "version": 0
}
```

```json
{
  "id": "order-abc-123",
  "value": {
    "items": ["item1", "item2"],
    "total": 299.99,
    "status": "pending"
  },
  "version": 5
}
```

---

## Architecture Components

The Actor Store is composed of three main components that work together:

### 1. ActorStoreClient (Public API)

The client provides the public-facing API for applications. Similar to MongoDB client or Redis client, it abstracts the internal storage mechanism and provides a clean interface for CRUD operations.

**Key Characteristics:**
- Thread-safe (can be cloned and shared across threads)
- Requires `actor_type` parameter (similar to collection/table name)
- Client should work with either self(if Copy Trait is implemented) or read reference(&T) for public API.
- Returns results immediately

### 2. ActorStore (Server/Handler)

The internal storage engine that processes commands and manages data.

**Architecture:**
```rust
pub struct ActorStore {
    // Nested DashMap: actor_type -> (actor_id -> ActorData)
    data: DashMap<String, DashMap<String, ActorData>>,

    // CDC writer handle (lock-free)
    cdc_writer: CdcWriter,
}
```

**Design Benefits:**
- **Lock-free concurrency**: Different actor types have zero contention
- **Per-key isolation**: Operations on different keys within same type are concurrent
- **Interior mutability**: DashMap provides lock-free read/write access
- **Minimal contention**: Only serialization point is per-key within DashMap

### 3. CDC Module (Change Data Capture)

Separate module responsible for event sequencing, storage, and distribution.

**Components:**
- **CdcWriter**: Lightweight handle for publishing events (used by ActorStore)
- **CdcLog**: Manages event log, replay buffer, and subscriptions
- **Background Processor**: Sequences events, maintains replay buffer, broadcasts to subscribers
  - We might make it pull based similar to kafka, a consumer can ask for offset, and it may ask for batch of events from an offset.

**Design Benefits:**
- **Separation of concerns**: CDC logic independent of storage
- **Lock-free publishing**: Writers use lock-free channel
- **Resume capability**: Replay buffer supports subscription resume
- **Phase 2 ready**: Easy to add disk persistence (WAL)

---

## Component API Signatures

> **Note**: For detailed implementation code and examples, see [implementation-details.md](../implementation-details.md).

### Component 1: ActorStoreClient (Public API)

**Purpose**: Thread-safe client for application-level Actor Store operations.

**Type Definition**:
```rust
#[derive(Clone)]
pub struct ActorStoreClient {
    store: Arc<ActorStore>,
    cdc_log: Arc<CdcLog>,
}
```

**Constructor Methods**:

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `pub fn new() -> Self` | Creates client with default configuration |
| `with_config` | `pub fn with_config(config: ActorStoreConfig) -> Self` | Creates client with custom CDC and storage configuration |

**CRUD Operations**:

| Method | Signature | Description |
|--------|-----------|-------------|
| `insert` | `pub fn insert(&self, actor_type: impl Into<String>, id: impl Into<String>, value: Value) -> Result<ActorData, InsertError>` | Inserts new actor with version 0. Fails if key already exists. |
| `get` | `pub fn get(&self, actor_type: impl Into<String>, id: &str) -> Option<ActorData>` | Retrieves actor by type and ID. Returns None if not found. |
| `update` | `pub fn update(&self, actor_type: impl Into<String>, id: &str, value: Value) -> Result<ActorData, UpdateError>` | Updates existing actor, incrementing version by 1. Fails if not found. |
| `update_versioned` | `pub fn update_versioned(&self, actor_type: impl Into<String>, id: &str, value: Value, expected_version: u64) -> Result<ActorData, UpdateVersionedError>` | Compare-and-swap update. Only succeeds if current version matches expected. |
| `delete` | `pub fn delete(&self, actor_type: impl Into<String>, id: &str) -> Option<ActorData>` | Removes actor. Returns deleted ActorData if existed. |
| `get_all` | `pub fn get_all(&self, actor_type: impl Into<String>) -> Vec<ActorData>` | Returns all actors of specified type in arbitrary order. |

**CDC Operations**:

| Method | Signature | Description |
|--------|-----------|-------------|
| `subscribe` | `pub fn subscribe(&self, options: SubscriptionOptions) -> Result<CdcSubscription, CdcError>` | Creates subscription to change events with optional filtering. |
| `cdc_stats` | `pub fn cdc_stats(&self) -> CdcStats` | Returns current CDC metrics (sequence, buffer size, subscribers). |

**Configuration**:
```rust
pub struct ActorStoreConfig {
    pub cdc_config: CdcConfig,
    // Future: persistence config, cluster config, etc.
}
```

**See also**: [ActorStoreClient Implementation](../implementation-details.md#actorstorecllient-implementation)

---

### Component 2: ActorStore (Internal Storage Handler)

**Purpose**: Internal storage engine with nested DashMap architecture and integrated CDC publishing.

**Type Definition**:
```rust
pub struct ActorStore {
    // actor_type -> (actor_id -> ActorData)
    data: DashMap<String, DashMap<String, ActorData>>,

    // CDC writer (lock-free, cloneable)
    cdc_writer: CdcWriter,
}
```

**Storage Operations**:

| Method | Signature | Description |
|--------|-----------|-------------|
| `insert` | `pub fn insert(&self, actor_type: String, id: String, value: Value) -> Result<ActorData, InsertError>` | Creates actor with version 0, publishes INSERT CDC event. Gets or creates inner DashMap for actor_type. Fails if key already exists. |
| `get` | `pub fn get(&self, actor_type: String, id: &str) -> Option<ActorData>` | Retrieves actor from nested DashMap. Returns None if actor_type or actor_id not found. |
| `update` | `pub fn update(&self, actor_type: String, id: &str, value: Value) -> Result<ActorData, UpdateError>` | Updates actor in-place, increments version by 1, publishes UPDATE CDC event with previous and new values. Acquires DashMap entry lock, modifies, then releases. |
| `update_versioned` | `pub fn update_versioned(&self, actor_type: String, id: &str, value: Value, expected_version: u64) -> Result<ActorData, UpdateVersionedError>` | Compare-and-swap update. Checks current version matches expected before updating. Provides optimistic concurrency control for conflict detection. |
| `delete` | `pub fn delete(&self, actor_type: String, id: &str) -> Option<ActorData>` | Removes actor from nested DashMap, publishes DELETE CDC event with previous value. Returns deleted ActorData if existed. |
| `get_all` | `pub fn get_all(&self, actor_type: String) -> Vec<ActorData>` | Collects all actors of specified type. Iterates inner DashMap and clones all entries. Returns empty vector if actor_type not found. |

**See also**: [ActorStore Implementation](../implementation-details.md#actorstore-implementation)

---

### Component 3: CDC Module (Change Data Capture)

**Purpose**: Separate module for event sequencing, storage, and distribution with lock-free publishing and subscription management.

**Configuration**:
```rust
pub struct CdcConfig {
    pub replay_buffer_size: usize,      // e.g., 10_000
    pub channel_capacity: usize,        // e.g., 10_000
    pub subscriber_capacity: usize,     // e.g., 1_000
    pub persistence_path: Option<PathBuf>,  // Phase 2
}
```

**Factory Function**:

| Function | Signature | Description |
|----------|-----------|-------------|
| `create_cdc_log` | `pub fn create_cdc_log(config: CdcConfig) -> (CdcWriter, CdcLog)` | Creates CDC system similar to mpsc::channel pattern. Returns writer handle and log manager. Spawns background processor thread for event sequencing. |

**CdcWriter (Lock-Free Publisher)**:

Type definition:
```rust
#[derive(Clone)]
pub struct CdcWriter {
    sender: Sender<ChangeEvent>,
    sequence: Arc<AtomicU64>,
}
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `write` | `pub fn write(&self, event: UnsequencedEvent) -> Result<u64, CdcError>` | Pre-allocates sequence number using atomic fetch_add (lock-free). Creates sequenced event and enqueues for background processing. Returns assigned sequence number. |

**CdcLog (Event Log Manager)**:

Type definition:
```rust
pub struct CdcLog {
    inner: Arc<CdcLogInner>,
    processor_handle: Option<JoinHandle<()>>,
}
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `subscribe` | `pub fn subscribe(&self, options: SubscriptionOptions) -> Result<CdcSubscription, CdcError>` | Creates subscription with optional filtering (actor_type, key_prefix) and start position (Beginning, SequenceNumber, Now). Returns subscription handle for receiving events. |
| `current_sequence` | `pub fn current_sequence(&self) -> u64` | Returns latest sequence number assigned. Thread-safe atomic read. |
| `stats` | `pub fn stats(&self) -> CdcStats` | Returns CDC metrics: total events, current sequence, replay buffer size, active subscriptions, events dropped. |

**CdcSubscription (Event Consumer)**:

Type definition:
```rust
pub struct CdcSubscription {
    receiver: broadcast::Receiver<ChangeEvent>,
    filter: SubscriptionFilter,
}
```

| Method | Signature | Description |
|--------|-----------|-------------|
| `recv` | `pub async fn recv(&mut self) -> Result<ChangeEvent, CdcError>` | Receives next event matching subscription filters. Async, blocking until event available. Applies actor_type and key_prefix filters before returning. |

**Event Types**:

```rust
// Event before sequencing (created by ActorStore)
pub struct UnsequencedEvent {
    pub operation: OperationType,
    pub actor_type: String,
    pub actor_id: String,
    pub previous_value: Option<Value>,
    pub previous_version: Option<u64>,
    pub new_value: Option<Value>,
    pub new_version: Option<u64>,
    pub timestamp: DateTime<Utc>,
}

// Sequenced event (after CDC processing)
pub struct ChangeEvent {
    pub sequence: u64,  // Assigned before enqueue
    pub operation: OperationType,
    pub actor_type: String,
    pub actor_id: String,
    pub previous_value: Option<Value>,
    pub previous_version: Option<u64>,
    pub new_value: Option<Value>,
    pub new_version: Option<u64>,
    pub timestamp: DateTime<Utc>,
}
```

**Supporting Types**:

```rust
pub struct SubscriptionOptions {
    pub start_position: StartPosition,
    pub actor_type_filter: Option<String>,
    pub key_prefix_filter: Option<String>,
}

pub enum StartPosition {
    Beginning,
    SequenceNumber(u64),
    Now,
    Timestamp(DateTime<Utc>),  // Phase 2
}

pub struct CdcStats {
    pub total_events: u64,
    pub current_sequence: u64,
    pub replay_buffer_size: usize,
    pub active_subscriptions: usize,
    pub events_dropped: u64,
}
```

**See also**: [CDC Module Implementation](../implementation-details.md#cdc-module-implementation)

---

## API Specification

> **For detailed API documentation including full signatures, parameters, return types, behavior descriptions, and code examples, see [api-specification.md](api-specification.md).**

The Actor Store exposes its functionality through three main API surfaces:

1. **Core CRUD Operations** (via `ActorStoreClient`):
   - `insert()`: Create new actors with version 0
   - `get()`: Retrieve actors by type and ID
   - `update()`: Update existing actors, incrementing version
   - `update_versioned()`: Optimistic concurrency with compare-and-swap
   - `delete()`: Remove actors and return previous state
   - `get_all()`: Retrieve all actors of a given type

2. **Change Data Capture**:
   - `subscribe()`: Subscribe to change events with filtering and resume capability
   - Event stream with sequence numbers for ordered processing
   - Support for different start positions (Beginning, SequenceNumber, Now, Timestamp)

3. **Configuration and Monitoring**:
   - `ActorStoreConfig`: CDC configuration, buffer sizes, persistence settings
   - `cdc_stats()`: Metrics for monitoring CDC performance

All operations support the `actor_type` parameter to namespace actors (similar to collections/tables in traditional databases).

See [api-specification.md](api-specification.md) for comprehensive API documentation.

---

## Implementation Phases

### Phase 1: Minimal In-Memory Implementation

**Goal**: Build a single-node, in-memory Actor Store with all core APIs.

#### Scope

**Included**:
- In-memory storage using `DashMap<String, ActorData>` or `RwLock<HashMap<String, ActorData>>`
- All 6 CRUD operations (insert, get, update, update_versioned, delete, get_all)
- Change data capture with in-memory event log
- Subscription support (Beginning, SequenceNumber, Now)
- Basic error handling

**Excluded**:
- Disk persistence
- Replication
- Leader election
- Network communication
- Authentication/authorization

#### Implementation Details

**Three-Component Architecture**:

```rust
use dashmap::DashMap;
use std::sync::Arc;

// Component 1: Client (Public API)
pub struct ActorStoreClient {
    store: Arc<ActorStore>,
}

// Component 2: Storage Handler
pub struct ActorStore {
    // Nested DashMap: actor_type -> (actor_id -> ActorData)
    data: DashMap<String, DashMap<String, ActorData>>,

    // CDC writer handle (lock-free, cloneable)
    cdc_writer: CdcWriter,
}

// Component 3: CDC Module
pub struct CdcWriter {
    sender: Sender<ChangeEvent>,
    sequence: Arc<AtomicU64>,
}

pub struct CdcLog {
    inner: Arc<CdcLogInner>,
    processor_handle: Option<JoinHandle<()>>,
}

struct CdcLogInner {
    sequence: Arc<AtomicU64>,
    receiver: Receiver<ChangeEvent>,
    replay_buffer: ArrayQueue<ChangeEvent>,
    broadcast_tx: broadcast::Sender<ChangeEvent>,
    config: CdcConfig,
}
```

**Key Decisions**:
- **Nested DashMap**: Outer map for actor types, inner map for actor IDs - enables zero-contention concurrent access across different actor types
- **Lock-Free CDC**: Pre-allocated sequence numbers using `AtomicU64::fetch_add` with background processing
- **Interior Mutability**: Client uses `&self` (not `&mut self`), relying on DashMap's internal locking
- **mpsc-like CDC Pattern**: CdcWriter sends to channel, CdcLog processes events in background thread
- **Cloneable Writer**: Multiple components can hold CdcWriter handles without Arc wrapping
- In-memory only (data and CDC log lost on restart)

**Deliverables**:
- `src/client.rs`: ActorStoreClient (public API)
- `src/store.rs`: ActorStore (internal handler)
- `src/cdc/mod.rs`: CDC module (writer, log, subscriptions)
- `src/types.rs`: Data types (ActorData, ChangeEvent, UnsequencedEvent, etc.)
- `src/error.rs`: Error types (InsertError, UpdateError, CdcError, etc.)
- `tests/client_tests.rs`: Client API tests
- `tests/cdc_tests.rs`: Change data capture tests

---

### Phase 2: Disk-Based Persistence

**Goal**: Add durability through disk-based storage and write-ahead logging.

#### Scope

**Added**:
- RocksDB backend for persistent storage
- Write-ahead log (WAL) for crash recovery
- Snapshot creation and restoration
- CDC log persistence with retention policies
- Timestamp-based subscriptions

**Maintained**:
- All Phase 1 APIs (backward compatible)
- Single-node operation

#### Implementation Details

**Storage Backends**:
```rust
pub trait StorageBackend: Send + Sync {
    fn insert(&self, actor_type: String, id: String, value: Value) -> Result<ActorData>;
    fn get(&self, actor_type: &str, id: &str) -> Option<ActorData>;
    fn update(&self, actor_type: &str, id: &str, value: Value) -> Result<ActorData>;
    fn delete(&self, actor_type: &str, id: &str) -> Option<ActorData>;
    fn get_all(&self, actor_type: &str) -> Vec<ActorData>;
    fn snapshot(&self) -> Result<Snapshot>;
    fn restore(&self, snapshot: Snapshot) -> Result<()>;
}

pub struct RocksDBBackend {
    db: rocksdb::DB,
    wal: WriteAheadLog,
}
```

**Write-Ahead Log**:
- Every mutation is logged before applying
- Enables crash recovery by replaying log
- Checkpointing to compact log periodically

**Deliverables**:
- `src/store/rocksdb.rs`: RocksDB implementation
- `src/wal.rs`: Write-ahead log
- `src/snapshot.rs`: Snapshot/restore logic
- `tests/persistence_tests.rs`: Crash recovery tests

---

### Phase 3: Distributed Clustering

**Goal**: Transform into a distributed store with replication and high availability.

#### Scope

**Added**:
- Raft consensus integration (using `openraft` crate)
- Leader-based replication
- Node discovery and cluster formation
- Node roles (PRIMARY, SECONDARY, READ_ONLY)
- Automatic failover
- Strong and eventual consistency reads
- Distributed CDC with global ordering

**Architecture**:
- Refer to `actor-store-architecture.md` for full distributed architecture
- Refer to `actor-store-cluster-management.md` for cluster management

#### Key Components

**Consensus Layer**:
- Raft log replication
- Leader election
- Membership changes

**Replication**:
- Quorum writes (W = N/2 + 1)
- Parallel follower replication
- Catch-up mechanism for lagging followers

**API Changes**:
- Add consistency level options to reads
- Redirect writes to leader
- Add cluster management endpoints

**Deliverables**:
- `src/consensus/mod.rs`: Raft integration
- `src/replication.rs`: Replication protocol
- `src/cluster/mod.rs`: Cluster management
- `src/discovery.rs`: Node discovery
- `tests/distributed_tests.rs`: Multi-node tests
- `tests/failover_tests.rs`: Leader election tests

---

## Performance Requirements

### Throughput Targets

**Phase 1 (In-Memory)**:
- Single-threaded: 100,000 ops/sec
- Multi-threaded: 500,000 ops/sec (16 threads)

**Phase 2 (Disk-Based)**:
- Single-threaded: 10,000 ops/sec
- Multi-threaded: 50,000 ops/sec (16 threads)

**Phase 3 (Distributed 3-node)**:
- Write throughput: 10,000 ops/sec
- Read throughput: 100,000 ops/sec (across all nodes)

### Latency Targets

**Phase 1 (In-Memory)**:
- p50: < 10 µs
- p99: < 100 µs
- p99.9: < 1 ms

**Phase 2 (Disk-Based)**:
- p50: < 1 ms
- p99: < 10 ms
- p99.9: < 50 ms

**Phase 3 (Distributed)**:
- Write p50: < 5 ms
- Write p99: < 50 ms
- Read p50: < 1 ms (eventual consistency)
- Read p99: < 10 ms

### Scalability Targets

**Phase 1**:
- Support up to 10 million actors in memory
- Support up to 100 concurrent subscribers

**Phase 2**:
- Support up to 100 million actors on disk
- Support database size up to 100 GB

**Phase 3**:
- Support clusters up to 50 nodes
- Support replication factor up to 5
- Support multi-datacenter deployments

---

## Architecture References

This specification builds upon the detailed architectural documents:

### Related Documents

1. **actor-store-architecture.md**:
   - Complete distributed architecture design
   - Raft consensus integration
   - Replication protocol details
   - Change Data Capture implementation
   - Failure scenarios and handling
   - Implementation phases and technology stack

2. **actor-store-cluster-management.md**:
   - Node identity and addressing
   - Cluster discovery mechanisms
   - Node roles (PRIMARY, SECONDARY, READ_ONLY)
   - Cluster formation and membership
   - Health monitoring and lifecycle management
   - Configuration examples

### Architecture Alignment

**Key Design Decisions from Architecture Docs**:

1. **String Keys**: This spec uses `String` keys (not `ObjectID`) for Redis-like simplicity and DashMap/HashMap compatibility

2. **Leader-Based Replication**: Phase 3 will implement leader election using Raft with quorum writes (W = N/2 + 1)

3. **Storage Backends**:
   - Phase 1: DashMap (concurrent in-memory)
   - Phase 2: RocksDB (embedded key-value store)
   - Phase 3+: Pluggable backends (MongoDB, PostgreSQL, etc.)

4. **CDC Ordering**:
   - Per-actor causal ordering guaranteed
   - Global sequence numbers for cross-actor ordering
   - Resume tokens via sequence numbers

5. **Version Management**:
   - u64 monotonic counters
   - Compare-and-swap for optimistic concurrency
   - Replicated versions across all nodes

---

## Appendix: Type Signatures (Rust)

### Core Types

```rust
pub struct ActorData {
    pub id: String,
    pub value: serde_json::Value,
    pub version: u64,
}

pub struct ChangeEvent {
    pub sequence: u64,
    pub operation: OperationType,
    pub actor_id: String,
    pub previous_value: Option<Value>,
    pub previous_version: Option<u64>,
    pub new_value: Option<Value>,
    pub new_version: Option<u64>,
    pub timestamp: DateTime<Utc>,
}

pub enum OperationType {
    INSERT,
    UPDATE,
    DELETE,
}

pub struct SubscriptionOptions {
    pub start_position: StartPosition,
    pub key_prefix: Option<String>,
}

pub enum StartPosition {
    Beginning,
    SequenceNumber(u64),
    Now,
    Timestamp(DateTime<Utc>), // Phase 2+
}
```

### Error Types

```rust
pub enum InsertError {
    KeyAlreadyExists,
    InvalidKey,
    ValueTooLarge,
}

pub enum UpdateError {
    NotFound,
    ValueTooLarge,
}

pub enum UpdateVersionedError {
    NotFound,
    VersionMismatch { expected: u64, actual: u64 },
    ValueTooLarge,
}

pub enum CdcError {
    // Unit enum - specific error variants will be added during implementation
    // Potential future variants:
    // - ChannelDisconnected
    // - SubscriptionLagged
    // - BufferOverflow
    // - InvalidSequenceNumber
}
```

### Store Trait

```rust
pub trait ActorStore: Send + Sync {
    fn insert(&self, actor_type: String, id: String, value: Value) -> Result<ActorData, InsertError>;
    fn get(&self, actor_type: String, id: &str) -> Option<ActorData>;
    fn update(&self, actor_type: String, id: &str, value: Value) -> Result<ActorData, UpdateError>;
    fn update_versioned(&self, actor_type: String, id: &str, value: Value, expected_version: u64)
        -> Result<ActorData, UpdateVersionedError>;
    fn delete(&self, actor_type: String, id: &str) -> Option<ActorData>;
    fn get_all(&self, actor_type: String) -> Vec<ActorData>;
}
```

---

## Summary

This specification provides a complete blueprint for implementing the Actor Store through three progressive phases:

1. **Phase 1**: Minimal in-memory store with full CRUD and CDC
2. **Phase 2**: Disk persistence with snapshots and WAL
3. **Phase 3**: Distributed clustering with Raft consensus

Each phase includes:
- ✅ Functional requirements
- ✅ API contracts
- ✅ Acceptance scenarios
- ✅ Comprehensive test cases
- ✅ Chaos test scenarios
- ✅ Performance targets
- ✅ Architecture alignment

This document serves as a complete reference for LLM-assisted code generation, ensuring consistency between requirements, implementation, and testing.
