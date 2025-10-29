# Actor Store - Complete Specification

> **Purpose**: This specification serves as a comprehensive reference for implementing the Actor Store through LLM-assisted code generation. It combines functional requirements, architectural design, acceptance criteria, test cases, and chaos testing scenarios.

---

## Table of Contents

1. [Overview](#overview)
2. [Data Model](#data-model)
3. [API Specification](#api-specification)
4. [Implementation Phases](#implementation-phases)
5. [Acceptance Scenarios](#acceptance-scenarios)
6. [Test Cases](#test-cases)
7. [Chaos Tests](#chaos-tests)
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
- **Consistency**: Strong consistency through leader-based replication
- **Durability**: Write-ahead logging with configurable persistence
- **Scalability**: Horizontal scaling through consistent hashing
- **Observable**: Complete change stream for reactive processing

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
    /// Starts at 1 on insert, increments on every update
    pub version: u64,
}
```

### Key Constraints

**Key (id field)**:
- Type: `String` or `ObjectID` (ObjectID will be implemented later)
- Must be non-empty
- Must be valid UTF-8
- Recommended max length: 512 bytes
- Similar to Redis keys (not UUID-based ObjectID)
- Keys must implement `Eq`, `Hash` traits for HashMap/DashMap based implementation.
- Cannot be changed post creation of Actor for now.

**Value (value field)**:
- Type: `serde_json::Value`
- Can be any valid JSON: object, array, string, number, boolean, null
- No schema enforcement (schema-less storage)
- Recommended max size: 10 MB per value
- Stored as serialized bytes internally

**Version (version field)**:
- Type: `u64`
- Starts at 0 on insert
- Increments by 1 on every update
- Never decreases
- Used for optimistic concurrency control

### Example ActorData Instances

```json
{
  "id": "user:12345",
  "value": {
    "name": "Alice",
    "email": "alice@example.com",
    "balance": 1000.50
  },
  "version": 1
}
```

```json
{
  "id": "order:order-abc-123",
  "value": {
    "items": ["item1", "item2"],
    "total": 299.99,
    "status": "pending"
  },
  "version": 5
}
```

---

## API Specification

### Core CRUD Operations

#### 1. Insert

Creates a new ActorData entry with auto-generated version 1.

```rust
fn insert(&mut self, id: String, value: serde_json::Value) -> Result<ActorData, InsertError>
```

**Parameters**:
- `id`: Unique string identifier for the actor
- `value`: JSON value to store

**Returns**:
- `Ok(ActorData)`: The created entry with `version = 1`
- `Err(InsertError::KeyAlreadyExists)`: If key already exists
- `Err(InsertError::InvalidKey)`: If key is empty or invalid
- `Err(InsertError::ValueTooLarge)`: If value exceeds size limit

**Behavior**:
- Fails if key already exists (use update to modify existing)
- Automatically sets version to 1
- Generates CDC event with operation type "INSERT"

**Example**:
```rust
let data = store.insert(
    "user:alice".to_string(),
    json!({"name": "Alice", "balance": 100})
)?;
assert_eq!(data.version, 1);
```

---

#### 2. Get

Retrieves an ActorData by its key.

```rust
fn get(&self, id: &str) -> Option<ActorData>
```

**Parameters**:
- `id`: String key to lookup

**Returns**:
- `Some(ActorData)`: If key exists
- `None`: If key does not exist

**Behavior**:
- Read-only operation (no side effects)
- Returns a clone of the stored data
- O(1) lookup time

**Example**:
```rust
if let Some(data) = store.get("user:alice") {
    println!("Found: {} at version {}", data.id, data.version);
}
```

---

#### 3. Update

Updates an existing ActorData, incrementing its version.

```rust
fn update(&mut self, id: &str, value: serde_json::Value) -> Result<ActorData, UpdateError>
```

**Parameters**:
- `id`: Key of the actor to update
- `value`: New JSON value

**Returns**:
- `Ok(ActorData)`: Updated entry with incremented version
- `Err(UpdateError::NotFound)`: If key does not exist

**Behavior**:
- Fails if key doesn't exist (use insert to create)
- Increments version by 1
- Replaces entire value (not partial update)
- Generates CDC event with operation type "UPDATE"

**Example**:
```rust
let updated = store.update(
    "user:alice",
    json!({"name": "Alice", "balance": 150})
)?;
assert_eq!(updated.version, 2); // Incremented from 1
```

---

#### 4. Update Versioned (Compare-and-Swap)

Updates only if the current version matches the expected version.

```rust
fn update_versioned(
    &mut self,
    id: &str,
    value: serde_json::Value,
    expected_version: u64
) -> Result<ActorData, UpdateVersionedError>
```

**Parameters**:
- `id`: Key of the actor to update
- `value`: New JSON value
- `expected_version`: Version number expected to be current

**Returns**:
- `Ok(ActorData)`: Updated entry with incremented version
- `Err(UpdateVersionedError::NotFound)`: If key does not exist
- `Err(UpdateVersionedError::VersionMismatch { expected, actual })`: If version doesn't match

**Behavior**:
- Atomic compare-and-swap operation
- Only succeeds if current version == expected_version
- Prevents lost updates in concurrent scenarios
- Increments version by 1 on success
- Generates CDC event with operation type "UPDATE"

**Example**:
```rust
// Read current version
let current = store.get("user:alice").unwrap();
assert_eq!(current.version, 5);

// Update only if version is still 5
let updated = store.update_versioned(
    "user:alice",
    json!({"name": "Alice", "balance": 200}),
    5
)?;
assert_eq!(updated.version, 6);

// This would fail if another client updated between read and write
```

---

#### 5. Delete

Removes an ActorData entry.

```rust
fn delete(&mut self, id: &str) -> Option<ActorData>
```

**Parameters**:
- `id`: Key of the actor to delete

**Returns**:
- `Some(ActorData)`: The deleted entry (for audit/rollback)
- `None`: If key did not exist

**Behavior**:
- Returns the deleted entry before removal
- Generates CDC event with operation type "DELETE"
- Idempotent (deleting non-existent key returns None)

**Example**:
```rust
if let Some(deleted) = store.delete("user:alice") {
    println!("Deleted user at version {}", deleted.version);
}
```

---

#### 6. Get All

Retrieves all ActorData entries in the store.

```rust
fn get_all(&self) -> Vec<ActorData>
```

**Returns**:
- Vector of all ActorData entries (unordered)

**Behavior**:
- Returns clones of all entries
- Order is not guaranteed
- Can be expensive for large stores
- Use for debugging, backups, or small datasets

**Example**:
```rust
let all_actors = store.get_all();
println!("Total actors: {}", all_actors.len());
```

---

### Change Data Capture (CDC)

#### Change Event Structure

```rust
pub struct ChangeEvent {
    /// Sequence number (monotonically increasing)
    pub sequence: u64,

    /// Type of operation
    pub operation: OperationType,

    /// Actor key
    pub actor_id: String,

    /// Previous value (None for INSERT)
    pub previous_value: Option<serde_json::Value>,

    /// Previous version (None for INSERT)
    pub previous_version: Option<u64>,

    /// New value (None for DELETE)
    pub new_value: Option<serde_json::Value>,

    /// New version (None for DELETE)
    pub new_version: Option<u64>,

    /// Timestamp of the operation
    pub timestamp: DateTime<Utc>,
}

pub enum OperationType {
    INSERT,
    UPDATE,
    DELETE,
}
```

#### Subscribe to Changes

```rust
fn subscribe(&self, options: SubscriptionOptions) -> ChangeStreamReceiver
```

**SubscriptionOptions**:

```rust
pub struct SubscriptionOptions {
    /// Starting position for the subscription
    pub start_position: StartPosition,

    /// Optional filter by key prefix
    pub key_prefix: Option<String>,
}

pub enum StartPosition {
    /// Start from the beginning of the change log
    Beginning,

    /// Start from a specific sequence number (resume token)
    SequenceNumber(u64),

    /// Start from current position (only future changes)
    Now,

    /// Start from a specific timestamp (Phase 2+)
    Timestamp(DateTime<Utc>),
}
```

**Returns**:
- `ChangeStreamReceiver`: A channel/stream receiver for change events

**Behavior**:
- Returns all events matching the subscription criteria
- Events are ordered by sequence number
- Subscriber can resume from last processed sequence
- Events are retained based on retention policy

**Example**:
```rust
// Subscribe from beginning
let receiver = store.subscribe(SubscriptionOptions {
    start_position: StartPosition::Beginning,
    key_prefix: None,
});

// Process events
while let Some(event) = receiver.recv().await {
    match event.operation {
        OperationType::INSERT => println!("New actor: {}", event.actor_id),
        OperationType::UPDATE => println!("Updated actor: {}", event.actor_id),
        OperationType::DELETE => println!("Deleted actor: {}", event.actor_id),
    }
}
```

```rust
// Resume from last checkpoint
let last_sequence = load_checkpoint();
let receiver = store.subscribe(SubscriptionOptions {
    start_position: StartPosition::SequenceNumber(last_sequence),
    key_prefix: Some("user:".to_string()),
});
```

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

**Storage Engine**:
```rust
use dashmap::DashMap;
use std::sync::Arc;

pub struct InMemoryStore {
    // Concurrent hash map for actor storage
    data: DashMap<String, ActorData>,

    // Change event log
    change_log: Arc<RwLock<Vec<ChangeEvent>>>,

    // Sequence counter for CDC
    sequence_counter: AtomicU64,

    // Event broadcast channel
    event_sender: broadcast::Sender<ChangeEvent>,
}
```

**Key Decisions**:
- Use `DashMap` for lock-free concurrent access
- Use `broadcast` channel for event distribution to subscribers
- Store change log in memory (bounded by retention settings)
- No persistence (data lost on restart)

**Deliverables**:
- `src/store/mod.rs`: Main store trait
- `src/store/memory.rs`: In-memory implementation
- `src/types.rs`: Data types (ActorData, ChangeEvent, etc.)
- `src/error.rs`: Error types
- `tests/memory_store_tests.rs`: Basic functionality tests

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
    fn insert(&mut self, id: String, value: Value) -> Result<ActorData>;
    fn get(&self, id: &str) -> Option<ActorData>;
    fn update(&mut self, id: &str, value: Value) -> Result<ActorData>;
    fn delete(&mut self, id: &str) -> Option<ActorData>;
    fn get_all(&self) -> Vec<ActorData>;
    fn snapshot(&self) -> Result<Snapshot>;
    fn restore(&mut self, snapshot: Snapshot) -> Result<()>;
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

## Acceptance Scenarios

### Scenario 1: Basic CRUD Operations

**Given**: An empty Actor Store

**When**:
```rust
// Insert a new actor
let actor = store.insert("user:1", json!({"name": "Alice"}))?;

// Read it back
let retrieved = store.get("user:1").unwrap();

// Update it
let updated = store.update("user:1", json!({"name": "Alice", "age": 30}))?;

// Delete it
let deleted = store.delete("user:1").unwrap();

// Verify deletion
let not_found = store.get("user:1");
```

**Then**:
- Insert returns ActorData with version = 1
- Get returns same data
- Update returns ActorData with version = 2
- Delete returns the last state before deletion
- Final get returns None

---

### Scenario 2: Version Conflict Detection

**Given**: An actor exists with version 5

**When**:
```rust
// Client A reads actor
let actor_a = store.get("user:1").unwrap();
assert_eq!(actor_a.version, 5);

// Client B reads actor
let actor_b = store.get("user:1").unwrap();
assert_eq!(actor_b.version, 5);

// Client A updates
store.update_versioned("user:1", json!({"balance": 100}), 5)?;

// Client B tries to update with stale version
let result = store.update_versioned("user:1", json!({"balance": 200}), 5);
```

**Then**:
- Client A's update succeeds, version becomes 6
- Client B's update fails with VersionMismatch error
- Client B must retry: read latest version, then update

---

### Scenario 3: Change Stream Subscription

**Given**: A store with 3 existing actors

**When**:
```rust
// Subscribe from beginning
let mut receiver = store.subscribe(SubscriptionOptions {
    start_position: StartPosition::Beginning,
    key_prefix: None,
});

// Should receive 3 INSERT events for existing actors
let event1 = receiver.recv().await.unwrap();
let event2 = receiver.recv().await.unwrap();
let event3 = receiver.recv().await.unwrap();

// Insert a new actor
store.insert("user:4", json!({"name": "Bob"}))?;

// Should receive INSERT event for new actor
let event4 = receiver.recv().await.unwrap();
```

**Then**:
- First 3 events are INSERTs with sequences 1, 2, 3
- Fourth event is INSERT with sequence 4
- All events arrive in order
- Subscriber continues receiving future changes

---

### Scenario 4: Resume from Checkpoint

**Given**: A subscriber processed events up to sequence 100

**When**:
```rust
// Save checkpoint
save_checkpoint(100);

// Subscriber restarts and resumes
let receiver = store.subscribe(SubscriptionOptions {
    start_position: StartPosition::SequenceNumber(100),
    key_prefix: None,
});

// Should receive events starting from sequence 101
let event = receiver.recv().await.unwrap();
assert_eq!(event.sequence, 101);
```

**Then**:
- No events are skipped
- No events are duplicated
- Processing resumes exactly where it left off

---

### Scenario 5: Filtered Subscription

**Given**: Store contains actors with keys "user:*" and "order:*"

**When**:
```rust
// Subscribe only to user changes
let receiver = store.subscribe(SubscriptionOptions {
    start_position: StartPosition::Now,
    key_prefix: Some("user:".to_string()),
});

// Insert user and order
store.insert("user:5", json!({"name": "Charlie"}))?;
store.insert("order:100", json!({"total": 50.0}))?;

// Receive events
let event = receiver.recv().await.unwrap();
```

**Then**:
- Only receive event for "user:5"
- "order:100" event is filtered out
- Reduces unnecessary data transfer

---

### Scenario 6: Get All Actors

**Given**: Store contains 5 actors

**When**:
```rust
let all = store.get_all();
```

**Then**:
- Returns vector with 5 ActorData entries
- All entries are distinct
- Order is unspecified but consistent

---

### Scenario 7: Concurrent Updates (Phase 1)

**Given**: Multiple threads accessing the store

**When**:
```rust
// Insert initial actor
store.insert("counter", json!({"count": 0}))?;

// Spawn 100 threads, each incrementing counter
let handles: Vec<_> = (0..100)
    .map(|_| {
        let store = store.clone();
        thread::spawn(move || {
            loop {
                let current = store.get("counter").unwrap();
                let new_count = current.value["count"].as_i64().unwrap() + 1;

                match store.update_versioned(
                    "counter",
                    json!({"count": new_count}),
                    current.version
                ) {
                    Ok(_) => break,
                    Err(_) => continue, // Retry on version conflict
                }
            }
        })
    })
    .collect();

// Wait for all threads
for handle in handles {
    handle.join().unwrap();
}

// Check final count
let final_count = store.get("counter").unwrap();
```

**Then**:
- Final count is exactly 100
- No updates are lost
- Version conflicts are handled correctly

---

### Scenario 8: Persistence and Recovery (Phase 2)

**Given**: A persistent store with data

**When**:
```rust
// Create store with RocksDB backend
let store = ActorStore::new_persistent("/tmp/store")?;

// Insert data
store.insert("user:1", json!({"name": "Alice"}))?;
store.insert("user:2", json!({"name": "Bob"}))?;

// Simulate crash (drop store)
drop(store);

// Restart store
let store = ActorStore::new_persistent("/tmp/store")?;

// Read data
let user1 = store.get("user:1");
let user2 = store.get("user:2");
```

**Then**:
- Both actors are recovered
- Versions are preserved
- No data loss occurred

---

### Scenario 9: Snapshot and Restore (Phase 2)

**Given**: A store with 1000 actors

**When**:
```rust
// Create snapshot
let snapshot = store.create_snapshot()?;

// Modify store
store.insert("temp:1", json!({"foo": "bar"}))?;

// Restore from snapshot
store.restore_snapshot(snapshot)?;

// Check state
let temp = store.get("temp:1");
```

**Then**:
- Snapshot contains all 1000 original actors
- "temp:1" does not exist after restore
- Store state matches snapshot exactly

---

### Scenario 10: Leader Failover (Phase 3)

**Given**: A 3-node cluster with node A as leader

**When**:
```rust
// Node A is leader
assert!(cluster.node_a.is_leader());

// Client writes to leader
cluster.node_a.insert("user:1", json!({"name": "Alice"}))?;

// Kill leader node A
cluster.kill_node_a();

// Wait for new leader election
cluster.wait_for_leader_election(Duration::from_secs(2));

// Client writes to new leader (node B or C)
let leader = cluster.current_leader();
leader.insert("user:2", json!({"name": "Bob"}))?;

// Verify data on all nodes
let user1_b = cluster.node_b.get("user:1");
let user2_b = cluster.node_b.get("user:1");
```

**Then**:
- New leader is elected within 2 seconds
- "user:1" is preserved (was replicated before failure)
- "user:2" is successfully written to new leader
- All alive nodes have consistent data

---

## Test Cases

### Unit Tests: In-Memory Store

#### Test: Insert New Actor

```rust
#[test]
fn test_insert_new_actor() {
    let store = InMemoryStore::new();

    let result = store.insert("user:1".to_string(), json!({"name": "Alice"}));

    assert!(result.is_ok());
    let actor = result.unwrap();
    assert_eq!(actor.id, "user:1");
    assert_eq!(actor.version, 1);
    assert_eq!(actor.value["name"], "Alice");
}
```

#### Test: Insert Duplicate Key Fails

```rust
#[test]
fn test_insert_duplicate_fails() {
    let mut store = InMemoryStore::new();

    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let result = store.insert("user:1".to_string(), json!({"name": "Bob"}));

    assert!(matches!(result, Err(InsertError::KeyAlreadyExists)));
}
```

#### Test: Get Existing Actor

```rust
#[test]
fn test_get_existing() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let result = store.get("user:1");

    assert!(result.is_some());
    let actor = result.unwrap();
    assert_eq!(actor.id, "user:1");
    assert_eq!(actor.value["name"], "Alice");
}
```

#### Test: Get Non-Existent Actor

```rust
#[test]
fn test_get_nonexistent() {
    let store = InMemoryStore::new();

    let result = store.get("user:1");

    assert!(result.is_none());
}
```

#### Test: Update Increments Version

```rust
#[test]
fn test_update_increments_version() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let updated = store.update("user:1", json!({"name": "Alice", "age": 30})).unwrap();

    assert_eq!(updated.version, 2);
    assert_eq!(updated.value["age"], 30);
}
```

#### Test: Update Non-Existent Fails

```rust
#[test]
fn test_update_nonexistent_fails() {
    let mut store = InMemoryStore::new();

    let result = store.update("user:1", json!({"name": "Alice"}));

    assert!(matches!(result, Err(UpdateError::NotFound)));
}
```

#### Test: Update Versioned Success

```rust
#[test]
fn test_update_versioned_success() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"count": 0})).unwrap();

    let result = store.update_versioned("user:1", json!({"count": 1}), 1);

    assert!(result.is_ok());
    let actor = result.unwrap();
    assert_eq!(actor.version, 2);
    assert_eq!(actor.value["count"], 1);
}
```

#### Test: Update Versioned Conflict

```rust
#[test]
fn test_update_versioned_conflict() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"count": 0})).unwrap();
    store.update("user:1", json!({"count": 1})).unwrap(); // Now version is 2

    let result = store.update_versioned("user:1", json!({"count": 2}), 1);

    assert!(matches!(result, Err(UpdateVersionedError::VersionMismatch { .. })));
}
```

#### Test: Delete Existing Actor

```rust
#[test]
fn test_delete_existing() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let deleted = store.delete("user:1");

    assert!(deleted.is_some());
    assert_eq!(deleted.unwrap().id, "user:1");

    // Verify actually deleted
    assert!(store.get("user:1").is_none());
}
```

#### Test: Delete Non-Existent

```rust
#[test]
fn test_delete_nonexistent() {
    let mut store = InMemoryStore::new();

    let result = store.delete("user:1");

    assert!(result.is_none());
}
```

#### Test: Get All Empty Store

```rust
#[test]
fn test_get_all_empty() {
    let store = InMemoryStore::new();

    let all = store.get_all();

    assert_eq!(all.len(), 0);
}
```

#### Test: Get All With Data

```rust
#[test]
fn test_get_all_with_data() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();
    store.insert("user:2".to_string(), json!({"name": "Bob"})).unwrap();
    store.insert("user:3".to_string(), json!({"name": "Charlie"})).unwrap();

    let all = store.get_all();

    assert_eq!(all.len(), 3);
    let ids: Vec<String> = all.iter().map(|a| a.id.clone()).collect();
    assert!(ids.contains(&"user:1".to_string()));
    assert!(ids.contains(&"user:2".to_string()));
    assert!(ids.contains(&"user:3".to_string()));
}
```

---

### Unit Tests: Change Data Capture

#### Test: CDC Event on Insert

```rust
#[test]
fn test_cdc_insert_event() {
    let mut store = InMemoryStore::new();
    let mut receiver = store.subscribe(SubscriptionOptions {
        start_position: StartPosition::Now,
        key_prefix: None,
    });

    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let event = receiver.try_recv().unwrap();
    assert_eq!(event.operation, OperationType::INSERT);
    assert_eq!(event.actor_id, "user:1");
    assert_eq!(event.new_version, Some(1));
    assert!(event.previous_value.is_none());
}
```

#### Test: CDC Event on Update

```rust
#[test]
fn test_cdc_update_event() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let mut receiver = store.subscribe(SubscriptionOptions {
        start_position: StartPosition::Now,
        key_prefix: None,
    });

    store.update("user:1", json!({"name": "Alice Updated"})).unwrap();

    let event = receiver.try_recv().unwrap();
    assert_eq!(event.operation, OperationType::UPDATE);
    assert_eq!(event.actor_id, "user:1");
    assert_eq!(event.previous_version, Some(1));
    assert_eq!(event.new_version, Some(2));
    assert_eq!(event.previous_value.unwrap()["name"], "Alice");
    assert_eq!(event.new_value.unwrap()["name"], "Alice Updated");
}
```

#### Test: CDC Event on Delete

```rust
#[test]
fn test_cdc_delete_event() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let mut receiver = store.subscribe(SubscriptionOptions {
        start_position: StartPosition::Now,
        key_prefix: None,
    });

    store.delete("user:1").unwrap();

    let event = receiver.try_recv().unwrap();
    assert_eq!(event.operation, OperationType::DELETE);
    assert_eq!(event.actor_id, "user:1");
    assert_eq!(event.previous_version, Some(1));
    assert!(event.new_value.is_none());
    assert!(event.new_version.is_none());
}
```

#### Test: Subscribe from Beginning

```rust
#[test]
fn test_subscribe_from_beginning() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();
    store.insert("user:2".to_string(), json!({"name": "Bob"})).unwrap();

    let mut receiver = store.subscribe(SubscriptionOptions {
        start_position: StartPosition::Beginning,
        key_prefix: None,
    });

    let event1 = receiver.recv().await.unwrap();
    let event2 = receiver.recv().await.unwrap();

    assert_eq!(event1.sequence, 1);
    assert_eq!(event2.sequence, 2);
}
```

#### Test: Subscribe from Sequence Number

```rust
#[test]
fn test_subscribe_from_sequence() {
    let mut store = InMemoryStore::new();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap(); // seq 1
    store.insert("user:2".to_string(), json!({"name": "Bob"})).unwrap();   // seq 2
    store.insert("user:3".to_string(), json!({"name": "Charlie"})).unwrap(); // seq 3

    let mut receiver = store.subscribe(SubscriptionOptions {
        start_position: StartPosition::SequenceNumber(2),
        key_prefix: None,
    });

    let event = receiver.recv().await.unwrap();

    // Should start from sequence 3 (next after 2)
    assert_eq!(event.sequence, 3);
    assert_eq!(event.actor_id, "user:3");
}
```

#### Test: Filtered Subscription

```rust
#[test]
fn test_filtered_subscription() {
    let mut store = InMemoryStore::new();

    let mut receiver = store.subscribe(SubscriptionOptions {
        start_position: StartPosition::Now,
        key_prefix: Some("user:".to_string()),
    });

    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();
    store.insert("order:100".to_string(), json!({"total": 50.0})).unwrap();
    store.insert("user:2".to_string(), json!({"name": "Bob"})).unwrap();

    let event1 = receiver.recv().await.unwrap();
    let event2 = receiver.recv().await.unwrap();

    assert_eq!(event1.actor_id, "user:1");
    assert_eq!(event2.actor_id, "user:2");

    // Should not receive order:100 event
    assert!(receiver.try_recv().is_err());
}
```

#### Test: Multiple Subscribers

```rust
#[test]
fn test_multiple_subscribers() {
    let mut store = InMemoryStore::new();

    let mut receiver1 = store.subscribe(SubscriptionOptions::default());
    let mut receiver2 = store.subscribe(SubscriptionOptions::default());

    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let event1 = receiver1.recv().await.unwrap();
    let event2 = receiver2.recv().await.unwrap();

    assert_eq!(event1.sequence, event2.sequence);
    assert_eq!(event1.actor_id, event2.actor_id);
}
```

---

### Integration Tests: Concurrent Access

#### Test: Concurrent Inserts

```rust
#[tokio::test]
async fn test_concurrent_inserts() {
    let store = Arc::new(InMemoryStore::new());
    let mut handles = vec![];

    for i in 0..100 {
        let store_clone = store.clone();
        handles.push(tokio::spawn(async move {
            store_clone.insert(
                format!("user:{}", i),
                json!({"id": i})
            ).unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(store.get_all().len(), 100);
}
```

#### Test: Concurrent Update Versioned (Optimistic Locking)

```rust
#[tokio::test]
async fn test_concurrent_update_versioned() {
    let store = Arc::new(InMemoryStore::new());
    store.insert("counter".to_string(), json!({"count": 0})).unwrap();

    let mut handles = vec![];
    let mut success_count = Arc::new(AtomicUsize::new(0));

    for _ in 0..100 {
        let store_clone = store.clone();
        let success_clone = success_count.clone();

        handles.push(tokio::spawn(async move {
            for _ in 0..10 {
                let current = store_clone.get("counter").unwrap();
                let new_count = current.value["count"].as_i64().unwrap() + 1;

                match store_clone.update_versioned(
                    "counter",
                    json!({"count": new_count}),
                    current.version
                ) {
                    Ok(_) => {
                        success_clone.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                    Err(_) => continue, // Retry
                }
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let final_count = store.get("counter").unwrap();
    assert_eq!(final_count.value["count"], 100);
}
```

#### Test: Read While Writing

```rust
#[tokio::test]
async fn test_read_while_writing() {
    let store = Arc::new(InMemoryStore::new());
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();

    let store_clone = store.clone();
    let writer = tokio::spawn(async move {
        for i in 0..1000 {
            store_clone.update("user:1", json!({"name": "Alice", "count": i})).unwrap();
        }
    });

    let store_clone = store.clone();
    let reader = tokio::spawn(async move {
        for _ in 0..1000 {
            let actor = store_clone.get("user:1").unwrap();
            assert_eq!(actor.id, "user:1");
        }
    });

    writer.await.unwrap();
    reader.await.unwrap();
}
```

---

### Integration Tests: Persistence (Phase 2)

#### Test: Recover After Crash

```rust
#[test]
fn test_recover_after_crash() {
    let temp_dir = tempdir().unwrap();
    let path = temp_dir.path();

    // Create store and insert data
    {
        let mut store = ActorStore::new_persistent(path).unwrap();
        store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();
        store.insert("user:2".to_string(), json!({"name": "Bob"})).unwrap();
    } // Store dropped, simulating crash

    // Reopen store
    let store = ActorStore::new_persistent(path).unwrap();

    let user1 = store.get("user:1").unwrap();
    let user2 = store.get("user:2").unwrap();

    assert_eq!(user1.value["name"], "Alice");
    assert_eq!(user2.value["name"], "Bob");
}
```

#### Test: WAL Replay

```rust
#[test]
fn test_wal_replay() {
    let temp_dir = tempdir().unwrap();
    let path = temp_dir.path();

    // Write operations
    {
        let mut store = ActorStore::new_persistent(path).unwrap();
        store.insert("user:1".to_string(), json!({"count": 0})).unwrap();
        store.update("user:1", json!({"count": 1})).unwrap();
        store.update("user:1", json!({"count": 2})).unwrap();
        store.update("user:1", json!({"count": 3})).unwrap();
        // Crash before checkpoint
    }

    // Reopen and replay WAL
    let store = ActorStore::new_persistent(path).unwrap();

    let user1 = store.get("user:1").unwrap();
    assert_eq!(user1.value["count"], 3);
    assert_eq!(user1.version, 4); // Insert = v1, 3 updates = v4
}
```

#### Test: Snapshot Creation

```rust
#[test]
fn test_snapshot_creation() {
    let temp_dir = tempdir().unwrap();
    let path = temp_dir.path();

    let mut store = ActorStore::new_persistent(path).unwrap();
    store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();
    store.insert("user:2".to_string(), json!({"name": "Bob"})).unwrap();

    let snapshot = store.create_snapshot().unwrap();

    assert_eq!(snapshot.actor_count, 2);
    assert!(snapshot.data.contains_key("user:1"));
    assert!(snapshot.data.contains_key("user:2"));
}
```

#### Test: Restore from Snapshot

```rust
#[test]
fn test_restore_from_snapshot() {
    let temp_dir = tempdir().unwrap();
    let path = temp_dir.path();

    let snapshot;
    {
        let mut store = ActorStore::new_persistent(path).unwrap();
        store.insert("user:1".to_string(), json!({"name": "Alice"})).unwrap();
        snapshot = store.create_snapshot().unwrap();
    }

    // Create new store and restore
    let mut store = ActorStore::new_persistent(temp_dir.path().join("restored")).unwrap();
    store.restore_snapshot(snapshot).unwrap();

    let user1 = store.get("user:1").unwrap();
    assert_eq!(user1.value["name"], "Alice");
}
```

---

## Chaos Tests

### Chaos Test 1: Random Operations

**Objective**: Verify correctness under random mixed operations

```rust
#[test]
fn chaos_random_operations() {
    let store = Arc::new(InMemoryStore::new());
    let mut rng = rand::thread_rng();

    // Pre-populate with actors
    for i in 0..100 {
        store.insert(format!("key:{}", i), json!({"value": i})).unwrap();
    }

    let operations = vec![
        "insert", "get", "update", "update_versioned", "delete", "get_all"
    ];

    for _ in 0..10000 {
        let op = operations.choose(&mut rng).unwrap();
        let key = format!("key:{}", rng.gen_range(0..200));

        match *op {
            "insert" => {
                let _ = store.insert(key, json!({"value": rng.gen::<i32>()}));
            }
            "get" => {
                let _ = store.get(&key);
            }
            "update" => {
                let _ = store.update(&key, json!({"value": rng.gen::<i32>()}));
            }
            "update_versioned" => {
                if let Some(current) = store.get(&key) {
                    let _ = store.update_versioned(
                        &key,
                        json!({"value": rng.gen::<i32>()}),
                        current.version
                    );
                }
            }
            "delete" => {
                let _ = store.delete(&key);
            }
            "get_all" => {
                let _ = store.get_all();
            }
            _ => unreachable!()
        }
    }

    // Invariants check
    let all = store.get_all();
    for actor in all {
        // Re-fetch to ensure consistency
        let refetched = store.get(&actor.id).unwrap();
        assert_eq!(actor.version, refetched.version);
        assert_eq!(actor.value, refetched.value);
    }
}
```

---

### Chaos Test 2: High Concurrency Stress

**Objective**: Verify correctness under extreme concurrent load

```rust
#[tokio::test]
async fn chaos_high_concurrency() {
    let store = Arc::new(InMemoryStore::new());
    let num_threads = 100;
    let operations_per_thread = 1000;

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let store_clone = store.clone();

        handles.push(tokio::spawn(async move {
            for i in 0..operations_per_thread {
                let key = format!("thread:{}:key:{}", thread_id, i % 10);

                // Try insert
                let _ = store_clone.insert(
                    key.clone(),
                    json!({"thread": thread_id, "iter": i})
                );

                // Read
                if let Some(actor) = store_clone.get(&key) {
                    // Update with version check
                    let _ = store_clone.update_versioned(
                        &key,
                        json!({"thread": thread_id, "iter": i + 1}),
                        actor.version
                    );
                }
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify no corruption
    let all = store.get_all();
    for actor in all {
        assert!(actor.version >= 1);
        assert!(actor.value.is_object());
    }
}
```

---

### Chaos Test 3: Subscriber Lag and Catchup

**Objective**: Verify slow subscribers don't lose events

```rust
#[tokio::test]
async fn chaos_slow_subscriber() {
    let store = Arc::new(InMemoryStore::new());

    let receiver = store.subscribe(SubscriptionOptions {
        start_position: StartPosition::Now,
        key_prefix: None,
    });

    // Fast writer
    let store_clone = store.clone();
    let writer = tokio::spawn(async move {
        for i in 0..1000 {
            store_clone.insert(
                format!("key:{}", i),
                json!({"value": i})
            ).unwrap();
        }
    });

    // Slow subscriber
    let mut events_received = 0;
    let reader = tokio::spawn(async move {
        let mut receiver = receiver;
        while let Ok(event) = receiver.recv().await {
            events_received += 1;
            tokio::time::sleep(Duration::from_micros(100)).await; // Simulate slow processing

            if events_received >= 1000 {
                break;
            }
        }
        events_received
    });

    writer.await.unwrap();
    let count = reader.await.unwrap();

    assert_eq!(count, 1000, "Slow subscriber should receive all events");
}
```

---

### Chaos Test 4: Crash During Write (Phase 2)

**Objective**: Verify no partial writes after crash

```rust
#[test]
fn chaos_crash_during_write() {
    let temp_dir = tempdir().unwrap();
    let path = temp_dir.path();

    // Write data and crash randomly
    for attempt in 0..10 {
        let store = ActorStore::new_persistent(path).unwrap();

        let should_crash_at = rand::thread_rng().gen_range(0..100);

        for i in 0..100 {
            if i == should_crash_at {
                // Simulate crash by dropping without cleanup
                drop(store);
                break;
            }

            let _ = store.insert(
                format!("key:{}", i),
                json!({"attempt": attempt, "index": i})
            );
        }
    }

    // Final recovery
    let store = ActorStore::new_persistent(path).unwrap();

    // Verify all recovered actors are consistent
    let all = store.get_all();
    for actor in all {
        assert!(actor.version >= 1);
        assert!(actor.value["index"].is_number());
    }
}
```

---

### Chaos Test 5: Network Partition (Phase 3)

**Objective**: Verify split-brain prevention and consistency

```rust
#[tokio::test]
async fn chaos_network_partition() {
    // Create 5-node cluster
    let cluster = TestCluster::new(5).await;

    // Write some data
    cluster.leader().insert("key:1", json!({"value": 1})).unwrap();
    cluster.wait_for_replication().await;

    // Partition: [Node1, Node2] vs [Node3, Node4, Node5]
    cluster.partition(vec![0, 1], vec![2, 3, 4]).await;

    // Majority partition should elect leader and accept writes
    let majority_leader = cluster.wait_for_leader_in_partition(vec![2, 3, 4]).await;
    assert!(majority_leader.is_some());

    majority_leader.unwrap().insert("key:2", json!({"value": 2})).unwrap();

    // Minority partition should reject writes
    let result = cluster.node(0).insert("key:3", json!({"value": 3}));
    assert!(result.is_err()); // No quorum

    // Heal partition
    cluster.heal_partition().await;
    cluster.wait_for_convergence().await;

    // Verify consistency
    for node in cluster.nodes() {
        assert!(node.get("key:1").is_some());
        assert!(node.get("key:2").is_some());
        assert!(node.get("key:3").is_none()); // Never committed
    }
}
```

---

### Chaos Test 6: Leader Flapping (Phase 3)

**Objective**: Verify stability under rapid leader changes

```rust
#[tokio::test]
async fn chaos_leader_flapping() {
    let cluster = TestCluster::new(5).await;
    let kill_interval = Duration::from_millis(500);

    let cluster_clone = cluster.clone();
    let chaos_handle = tokio::spawn(async move {
        for _ in 0..20 {
            tokio::time::sleep(kill_interval).await;

            // Kill current leader
            if let Some(leader_id) = cluster_clone.current_leader() {
                cluster_clone.kill_node(leader_id).await;
            }

            // Wait for new leader
            cluster_clone.wait_for_leader_election(Duration::from_secs(2)).await;

            // Restart killed node
            cluster_clone.restart_all_dead_nodes().await;
        }
    });

    // Meanwhile, continuously write data
    let cluster_clone = cluster.clone();
    let writer_handle = tokio::spawn(async move {
        for i in 0..100 {
            loop {
                if let Some(leader) = cluster_clone.current_leader_node() {
                    match leader.insert(format!("key:{}", i), json!({"value": i})) {
                        Ok(_) => break,
                        Err(_) => {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                    }
                } else {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });

    chaos_handle.await.unwrap();
    writer_handle.await.unwrap();

    // Verify all writes succeeded
    cluster.wait_for_convergence().await;
    for i in 0..100 {
        let key = format!("key:{}", i);
        assert!(cluster.node(0).get(&key).is_some());
    }
}
```

---

### Chaos Test 7: Slow Follower (Phase 3)

**Objective**: Verify leader doesn't wait for slow followers

```rust
#[tokio::test]
async fn chaos_slow_follower() {
    let mut cluster = TestCluster::new(5).await;

    // Make node 4 artificially slow
    cluster.add_latency(4, Duration::from_millis(500));

    // Write 100 operations
    let leader = cluster.leader();
    for i in 0..100 {
        leader.insert(format!("key:{}", i), json!({"value": i})).unwrap();
    }

    // Leader should have committed all (doesn't wait for slow node)
    assert_eq!(leader.get_all().len(), 100);

    // Fast followers should catch up quickly
    cluster.wait_for_replication_except(vec![4], Duration::from_secs(2)).await;

    for i in 0..4 {
        assert_eq!(cluster.node(i).get_all().len(), 100);
    }

    // Slow follower eventually catches up
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(cluster.node(4).get_all().len(), 100);
}
```

---

### Chaos Test 8: Disk Corruption (Phase 2)

**Objective**: Verify graceful handling of corrupted data files

```rust
#[test]
fn chaos_disk_corruption() {
    let temp_dir = tempdir().unwrap();
    let path = temp_dir.path();

    // Create store with data
    {
        let mut store = ActorStore::new_persistent(path).unwrap();
        for i in 0..100 {
            store.insert(format!("key:{}", i), json!({"value": i})).unwrap();
        }
    }

    // Corrupt some data files
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension() == Some(std::ffi::OsStr::new("sst")) {
            // Randomly corrupt some SST files
            if rand::random::<bool>() {
                std::fs::write(entry.path(), b"corrupted data").unwrap();
                break;
            }
        }
    }

    // Try to open store
    let result = ActorStore::new_persistent(path);

    // Should either:
    // 1. Detect corruption and return error
    // 2. Recover using WAL/backups
    // 3. Panic is NOT acceptable
    match result {
        Ok(store) => {
            // If opened successfully, verify consistency
            let all = store.get_all();
            for actor in all {
                assert!(actor.version >= 1);
            }
        }
        Err(e) => {
            // Corruption detected gracefully
            assert!(e.to_string().contains("corrupt") || e.to_string().contains("checksum"));
        }
    }
}
```

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
```

### Store Trait

```rust
pub trait ActorStore: Send + Sync {
    fn insert(&mut self, id: String, value: Value) -> Result<ActorData, InsertError>;
    fn get(&self, id: &str) -> Option<ActorData>;
    fn update(&mut self, id: &str, value: Value) -> Result<ActorData, UpdateError>;
    fn update_versioned(&mut self, id: &str, value: Value, expected_version: u64)
        -> Result<ActorData, UpdateVersionedError>;
    fn delete(&mut self, id: &str) -> Option<ActorData>;
    fn get_all(&self) -> Vec<ActorData>;
    fn subscribe(&self, options: SubscriptionOptions) -> ChangeStreamReceiver;
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
