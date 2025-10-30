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
- **Atomicity**: Single-operation atomicity; multi-entity transactions in future phases
- **Isolation**: Per-key serializability; leader reads are linearizable
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

## Detailed Component Design

### Component 1: ActorStoreClient (Public API)

```rust
use serde_json::Value;
use std::sync::Arc;

/// Public-facing client for Actor Store operations
#[derive(Clone)]
pub struct ActorStoreClient {
    store: Arc<ActorStore>,
    cdc_log: Arc<CdcLog>,
}

impl ActorStoreClient {
    /// Create a new client with default configuration
    pub fn new() -> Self {
        Self::with_config(ActorStoreConfig::default())
    }

    /// Create a new client with custom configuration
    pub fn with_config(config: ActorStoreConfig) -> Self {
        let (cdc_writer, cdc_log) = create_cdc_log(config.cdc_config);

        let store = Arc::new(ActorStore {
            data: DashMap::new(),
            cdc_writer,
        });

        Self {
            store,
            cdc_log: Arc::new(cdc_log),
        }
    }

    /// Insert a new actor
    pub fn insert(
        &self,
        actor_type: impl Into<String>,
        id: impl Into<String>,
        value: Value,
    ) -> Result<ActorData, InsertError> {
        self.store.insert(actor_type.into(), id.into(), value)
    }

    /// Get an actor by ID
    pub fn get(
        &self,
        actor_type: impl Into<String>,
        id: &str,
    ) -> Option<ActorData> {
        self.store.get(actor_type.into(), id)
    }

    /// Update an existing actor
    pub fn update(
        &self,
        actor_type: impl Into<String>,
        id: &str,
        value: Value,
    ) -> Result<ActorData, UpdateError> {
        self.store.update(actor_type.into(), id, value)
    }

    /// Update with version check (compare-and-swap)
    pub fn update_versioned(
        &self,
        actor_type: impl Into<String>,
        id: &str,
        value: Value,
        expected_version: u64,
    ) -> Result<ActorData, UpdateVersionedError> {
        self.store.update_versioned(actor_type.into(), id, value, expected_version)
    }

    /// Delete an actor
    pub fn delete(
        &self,
        actor_type: impl Into<String>,
        id: &str,
    ) -> Option<ActorData> {
        self.store.delete(actor_type.into(), id)
    }

    /// Get all actors of a specific type
    pub fn get_all(
        &self,
        actor_type: impl Into<String>,
    ) -> Vec<ActorData> {
        self.store.get_all(actor_type.into())
    }

    /// Subscribe to change events
    pub fn subscribe(
        &self,
        options: SubscriptionOptions,
    ) -> Result<CdcSubscription, CdcError> {
        self.cdc_log.subscribe(options)
    }

    /// Get CDC statistics
    pub fn cdc_stats(&self) -> CdcStats {
        self.cdc_log.stats()
    }
}

pub struct ActorStoreConfig {
    pub cdc_config: CdcConfig,
    // Future: persistence config, cluster config, etc.
}
```

---

### Component 2: ActorStore (Server/Handler)

```rust
use dashmap::DashMap;

/// Internal storage handler with nested DashMap architecture
pub struct ActorStore {
    // actor_type -> (actor_id -> ActorData)
    data: DashMap<String, DashMap<String, ActorData>>,

    // CDC writer (lock-free, cloneable)
    cdc_writer: CdcWriter,
}

impl ActorStore {
    /// Insert operation implementation
    pub fn insert(
        &self,
        actor_type: String,
        id: String,
        value: Value,
    ) -> Result<ActorData, InsertError> {
        // Validate inputs
        if id.is_empty() {
            return Err(InsertError::InvalidKey);
        }

        // Create ActorData
        let actor = ActorData {
            id: id.clone(),
            value: value.clone(),
            version: 0,
        };

        // Get or create inner DashMap for actor type
        let type_store = self.data
            .entry(actor_type.clone())
            .or_insert_with(DashMap::new);

        // Attempt insert (fails if key exists)
        if type_store.insert(id.clone(), actor.clone()).is_some() {
            return Err(InsertError::KeyAlreadyExists);
        }

        // Publish CDC event (lock-free, returns sequence number)
        let seq = self.cdc_writer.write(UnsequencedEvent {
            operation: OperationType::INSERT,
            actor_type,
            actor_id: id,
            previous_value: None,
            previous_version: None,
            new_value: Some(value),
            new_version: Some(0),
            timestamp: Utc::now(),
        })?;

        Ok(actor)
    }

    /// Get operation implementation
    pub fn get(
        &self,
        actor_type: String,
        id: &str,
    ) -> Option<ActorData> {
        self.data
            .get(&actor_type)?
            .get(id)
            .map(|entry| entry.value().clone())
    }

    /// Update operation implementation
    pub fn update(
        &self,
        actor_type: String,
        id: &str,
        value: Value,
    ) -> Result<ActorData, UpdateError> {
        let type_store = self.data
            .get(&actor_type)
            .ok_or(UpdateError::NotFound)?;

        let mut entry = type_store
            .get_mut(id)
            .ok_or(UpdateError::NotFound)?;

        let old_value = entry.value.clone();
        let old_version = entry.version;

        // Update in place
        entry.value = value.clone();
        entry.version += 1;

        let updated = entry.clone();
        drop(entry); // Release lock

        // Publish CDC event
        let seq = self.cdc_writer.write(UnsequencedEvent {
            operation: OperationType::UPDATE,
            actor_type,
            actor_id: id.to_string(),
            previous_value: Some(old_value),
            previous_version: Some(old_version),
            new_value: Some(value),
            new_version: Some(updated.version),
            timestamp: Utc::now(),
        })?;

        Ok(updated)
    }

    /// Update with version check implementation
    pub fn update_versioned(
        &self,
        actor_type: String,
        id: &str,
        value: Value,
        expected_version: u64,
    ) -> Result<ActorData, UpdateVersionedError> {
        let type_store = self.data
            .get(&actor_type)
            .ok_or(UpdateVersionedError::NotFound)?;

        let mut entry = type_store
            .get_mut(id)
            .ok_or(UpdateVersionedError::NotFound)?;

        // Version check
        if entry.version != expected_version {
            return Err(UpdateVersionedError::VersionMismatch {
                expected: expected_version,
                actual: entry.version,
            });
        }

        let old_value = entry.value.clone();
        let old_version = entry.version;

        // Update in place
        entry.value = value.clone();
        entry.version += 1;

        let updated = entry.clone();
        drop(entry); // Release lock

        // Publish CDC event
        let seq = self.cdc_writer.write(UnsequencedEvent {
            operation: OperationType::UPDATE,
            actor_type,
            actor_id: id.to_string(),
            previous_value: Some(old_value),
            previous_version: Some(old_version),
            new_value: Some(value),
            new_version: Some(updated.version),
            timestamp: Utc::now(),
        })?;

        Ok(updated)
    }

    /// Delete operation implementation
    pub fn delete(
        &self,
        actor_type: String,
        id: &str,
    ) -> Option<ActorData> {
        let type_store = self.data.get(&actor_type)?;
        let (_, deleted) = type_store.remove(id)?;

        // Publish CDC event
        let _ = self.cdc_writer.write(UnsequencedEvent {
            operation: OperationType::DELETE,
            actor_type,
            actor_id: id.to_string(),
            previous_value: Some(deleted.value.clone()),
            previous_version: Some(deleted.version),
            new_value: None,
            new_version: None,
            timestamp: Utc::now(),
        });

        Some(deleted)
    }

    /// Get all actors of a specific type
    pub fn get_all(
        &self,
        actor_type: String,
    ) -> Vec<ActorData> {
        self.data
            .get(&actor_type)
            .map(|type_store| {
                type_store.iter()
                    .map(|entry| entry.value().clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}
```

---

### Component 3: CDC Module

```rust
use crossbeam::channel::{bounded, Sender, Receiver};
use crossbeam::queue::ArrayQueue;
use tokio::sync::broadcast;

/// Configuration for CDC
pub struct CdcConfig {
    pub replay_buffer_size: usize,      // e.g., 10_000
    pub channel_capacity: usize,        // e.g., 10_000
    pub subscriber_capacity: usize,     // e.g., 1_000
    pub persistence_path: Option<PathBuf>,  // Phase 2
}

/// Creates a CDC log system (similar to mpsc::channel pattern)
pub fn create_cdc_log(config: CdcConfig) -> (CdcWriter, CdcLog) {
    let (sender, receiver) = bounded(config.channel_capacity);
    let replay_buffer = ArrayQueue::new(config.replay_buffer_size);
    let (broadcast_tx, _) = broadcast::channel(config.subscriber_capacity);

    let sequence = Arc::new(AtomicU64::new(0));

    let inner = Arc::new(CdcLogInner {
        sequence: sequence.clone(),
        receiver,
        replay_buffer,
        broadcast_tx: broadcast_tx.clone(),
        config,
        stats: Arc::new(AtomicStats::default()),
    });

    // Spawn background processor
    let inner_clone = inner.clone();
    let processor_handle = std::thread::Builder::new()
        .name("cdc-processor".to_string())
        .spawn(move || cdc_processor_loop(inner_clone))
        .expect("Failed to spawn CDC processor");

    let writer = CdcWriter {
        sender,
        sequence,
    };

    let cdc_log = CdcLog {
        inner,
        processor_handle: Some(processor_handle),
    };

    (writer, cdc_log)
}

/// CdcWriter - Lightweight, cloneable handle for publishing events
#[derive(Clone)]
pub struct CdcWriter {
    sender: Sender<ChangeEvent>,
    sequence: Arc<AtomicU64>,
}

impl CdcWriter {
    /// Write an event (returns actual sequence number)
    pub fn write(&self, event: UnsequencedEvent) -> Result<u64, CdcError> {
        // Pre-allocate sequence number (lock-free, instant)
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst);

        // Create sequenced event
        let sequenced = ChangeEvent {
            sequence: seq,
            operation: event.operation,
            actor_type: event.actor_type,
            actor_id: event.actor_id,
            previous_value: event.previous_value,
            previous_version: event.previous_version,
            new_value: event.new_value,
            new_version: event.new_version,
            timestamp: event.timestamp,
        };

        // Enqueue for background processing
        self.sender.send(sequenced)
            .map_err(|_| CdcError::Disconnected)?;

        Ok(seq)
    }
}

/// Event before sequencing (created by ActorStore)
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

/// Sequenced event (after CDC processing)
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

/// CdcLog - Manages subscriptions and event log
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
    stats: Arc<AtomicStats>,
}

impl CdcLog {
    /// Subscribe to change events
    pub fn subscribe(
        &self,
        options: SubscriptionOptions,
    ) -> Result<CdcSubscription, CdcError> {
        let receiver = self.inner.broadcast_tx.subscribe();

        // TODO: Handle replay for Beginning/SequenceNumber positions

        self.inner.stats.active_subscriptions
            .fetch_add(1, Ordering::Relaxed);

        Ok(CdcSubscription {
            receiver,
            filter: SubscriptionFilter {
                actor_type: options.actor_type_filter,
                key_prefix: options.key_prefix_filter,
            },
        })
    }

    /// Get current sequence number
    pub fn current_sequence(&self) -> u64 {
        self.inner.sequence.load(Ordering::SeqCst)
    }

    /// Get CDC statistics
    pub fn stats(&self) -> CdcStats {
        CdcStats {
            total_events: self.inner.stats.total_events.load(Ordering::Relaxed),
            current_sequence: self.current_sequence(),
            replay_buffer_size: self.inner.replay_buffer.len(),
            active_subscriptions: self.inner.stats.active_subscriptions.load(Ordering::Relaxed),
            events_dropped: self.inner.stats.events_dropped.load(Ordering::Relaxed),
        }
    }
}

/// Background processor thread
fn cdc_processor_loop(inner: Arc<CdcLogInner>) {
    while let Ok(event) = inner.receiver.recv() {
        // Add to replay buffer (lock-free)
        if inner.replay_buffer.force_push(event.clone()).is_err() {
            // Oldest event evicted
            inner.stats.events_dropped.fetch_add(1, Ordering::Relaxed);
        }

        // Broadcast to subscribers
        let _ = inner.broadcast_tx.send(event);

        // Update stats
        inner.stats.total_events.fetch_add(1, Ordering::Relaxed);

        // Phase 2: Write to WAL
        // if let Some(ref wal) = inner.wal_writer {
        //     wal.append(&event)?;
        // }
    }
}

/// Subscription handle
pub struct CdcSubscription {
    receiver: broadcast::Receiver<ChangeEvent>,
    filter: SubscriptionFilter,
}

impl CdcSubscription {
    /// Receive next event (blocking, with filtering)
    pub async fn recv(&mut self) -> Result<ChangeEvent, CdcError> {
        loop {
            let event = self.receiver.recv().await
                .map_err(|_| CdcError::SubscriptionLagged)?;

            // Apply filters
            if let Some(ref actor_type) = self.filter.actor_type {
                if &event.actor_type != actor_type {
                    continue;
                }
            }

            if let Some(ref prefix) = self.filter.key_prefix {
                if !event.actor_id.starts_with(prefix) {
                    continue;
                }
            }

            return Ok(event);
        }
    }
}

struct SubscriptionFilter {
    actor_type: Option<String>,
    key_prefix: Option<String>,
}

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

struct AtomicStats {
    total_events: AtomicU64,
    events_dropped: AtomicU64,
    active_subscriptions: AtomicUsize,
}
```

---

## API Specification

### Core CRUD Operations

All operations are exposed through `ActorStoreClient` and require an `actor_type` parameter (equivalent to collection/table name in traditional databases).

#### 1. Insert

Creates a new ActorData entry with auto-generated version 0.

```rust
pub fn insert(
    &self,
    actor_type: impl Into<String>,
    id: impl Into<String>,
    value: Value,
) -> Result<ActorData, InsertError>
```

**Parameters**:
- `actor_type`: The actor type namespace (e.g., "users", "accounts")
- `id`: Unique string identifier for the actor within its type
- `value`: JSON value to store

**Returns**:
- `Ok(ActorData)`: The created entry with `version = 0`
- `Err(InsertError::KeyAlreadyExists)`: If key already exists in the actor type
- `Err(InsertError::InvalidKey)`: If key is empty or invalid
- `Err(InsertError::ValueTooLarge)`: If value exceeds size limit

**Behavior**:
- Fails if key already exists within the actor type (use update to modify existing)
- Automatically sets version to 0
- Generates CDC event with operation type "INSERT"
- Thread-safe, can be called concurrently from multiple threads

**Example**:
```rust
let client = ActorStoreClient::new(store);
let data = client.insert(
    "users",
    "user:alice",
    json!({"name": "Alice", "balance": 100})
)?;
assert_eq!(data.version, 0);
```

---

#### 2. Get

Retrieves an ActorData by its type and key.

```rust
pub fn get(
    &self,
    actor_type: impl Into<String>,
    id: impl AsRef<str>,
) -> Option<ActorData>
```

**Parameters**:
- `actor_type`: The actor type namespace
- `id`: String key to lookup within the actor type

**Returns**:
- `Some(ActorData)`: If key exists in the specified actor type
- `None`: If key does not exist

**Behavior**:
- Read-only operation (no side effects)
- Returns a clone of the stored data
- O(1) lookup time within actor type
- Thread-safe, linearizable reads

**Example**:
```rust
if let Some(data) = client.get("users", "user:alice") {
    println!("Found: {} at version {}", data.id, data.version);
}
```

---

#### 3. Update

Updates an existing ActorData, incrementing its version.

```rust
pub fn update(
    &self,
    actor_type: impl Into<String>,
    id: impl AsRef<str>,
    value: Value,
) -> Result<ActorData, UpdateError>
```

**Parameters**:
- `actor_type`: The actor type namespace
- `id`: Key of the actor to update within its type
- `value`: New JSON value

**Returns**:
- `Ok(ActorData)`: Updated entry with incremented version
- `Err(UpdateError::NotFound)`: If key does not exist in the actor type

**Behavior**:
- Fails if key doesn't exist (use insert to create)
- Increments version by 1
- Replaces entire value (not partial update)
- Generates CDC event with operation type "UPDATE"
- Thread-safe via interior mutability

**Example**:
```rust
let updated = client.update(
    "users",
    "user:alice",
    json!({"name": "Alice", "balance": 150})
)?;
assert_eq!(updated.version, 1); // Incremented from 0
```

---

#### 4. Update Versioned (Compare-and-Swap)

Updates only if the current version matches the expected version.

```rust
pub fn update_versioned(
    &self,
    actor_type: impl Into<String>,
    id: impl AsRef<str>,
    value: Value,
    expected_version: u64,
) -> Result<ActorData, UpdateVersionedError>
```

**Parameters**:
- `actor_type`: The actor type namespace
- `id`: Key of the actor to update within its type
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
let current = client.get("users", "user:alice").unwrap();
assert_eq!(current.version, 5);

// Update only if version is still 5
let updated = client.update_versioned(
    "users",
    "user:alice",
    json!({"name": "Alice", "balance": 200}),
    5
)?;
assert_eq!(updated.version, 6);

// This would fail if another client updated between read and write
```

---

#### 5. Delete

Removes an ActorData entry from the specified actor type.

```rust
pub fn delete(
    &self,
    actor_type: impl Into<String>,
    id: impl AsRef<str>,
) -> Option<ActorData>
```

**Parameters**:
- `actor_type`: The actor type namespace
- `id`: Key of the actor to delete within its type

**Returns**:
- `Some(ActorData)`: The deleted entry (for audit/rollback)
- `None`: If key did not exist

**Behavior**:
- Returns the deleted entry before removal
- Generates CDC event with operation type "DELETE"
- Idempotent (deleting non-existent key returns None)
- Thread-safe

**Example**:
```rust
if let Some(deleted) = client.delete("users", "user:alice") {
    println!("Deleted user at version {}", deleted.version);
}
```

---

#### 6. Get All

Retrieves all ActorData entries for a specific actor type.

```rust
pub fn get_all(&self, actor_type: impl Into<String>) -> Vec<ActorData>
```

**Parameters**:
- `actor_type`: The actor type namespace to query

**Returns**:
- Vector of all ActorData entries for the specified type (unordered)

**Behavior**:
- Returns clones of all entries in the actor type
- Order is not guaranteed
- Can be expensive for large actor types
- Use for debugging, backups, or small datasets

**Example**:
```rust
let all_users = client.get_all("users");
println!("Total users: {}", all_users.len());
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

## Acceptance Scenarios

### Scenario 1: Basic CRUD Operations

**Given**: An empty Actor Store

**When**:
```rust
// Insert a new actor
let actor = client.insert("users", "user:1", json!({"name": "Alice"}))?;

// Read it back
let retrieved = client.get("users", "user:1").unwrap();

// Update it
let updated = client.update("users", "user:1", json!({"name": "Alice", "age": 30}))?;

// Delete it
let deleted = client.delete("users", "user:1").unwrap();

// Verify deletion
let not_found = client.get("users", "user:1");
```

**Then**:
- Insert returns ActorData with version = 0
- Get returns same data
- Update returns ActorData with version = 1
- Delete returns the last state before deletion
- Final get returns None

---

### Scenario 2: Version Conflict Detection

**Given**: An actor exists with version 5

**When**:
```rust
// Client A reads actor
let actor_a = client.get("users", "user:1").unwrap();
assert_eq!(actor_a.version, 5);

// Client B reads actor
let actor_b = client.get("users", "user:1").unwrap();
assert_eq!(actor_b.version, 5);

// Client A updates
client.update_versioned("users", "user:1", json!({"balance": 100}), 5)?;

// Client B tries to update with stale version
let result = client.update_versioned("users", "user:1", json!({"balance": 200}), 5);
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
let mut receiver = client.subscribe(SubscriptionOptions {
    start_position: StartPosition::Beginning,
    actor_type_filter: None,
    key_prefix_filter: None,
});

// Should receive 3 INSERT events for existing actors
let event1 = receiver.recv().await.unwrap();
let event2 = receiver.recv().await.unwrap();
let event3 = receiver.recv().await.unwrap();

// Insert a new actor
client.insert("users", "user:4", json!({"name": "Bob"}))?;

// Should receive INSERT event for new actor
let event4 = receiver.recv().await.unwrap();
```

**Then**:
- First 3 events are INSERTs with sequences 0, 1, 2
- Fourth event is INSERT with sequence 3
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
let receiver = client.subscribe(SubscriptionOptions {
    start_position: StartPosition::SequenceNumber(100),
    actor_type_filter: None,
    key_prefix_filter: None,
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
let receiver = client.subscribe(SubscriptionOptions {
    start_position: StartPosition::Now,
    actor_type_filter: None,
    key_prefix_filter: Some("user:".to_string()),
});

// Insert user and order
client.insert("users", "user:5", json!({"name": "Charlie"}))?;
client.insert("orders", "order:100", json!({"total": 50.0}))?;

// Receive events
let event = receiver.recv().await.unwrap();
```

**Then**:
- Only receive event for "user:5"
- "order:100" event is filtered out
- Reduces unnecessary data transfer

---

### Scenario 6: Get All Actors

**Given**: Store contains 5 users

**When**:
```rust
let all = client.get_all("users");
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
client.insert("counters", "counter", json!({"count": 0}))?;

// Spawn 100 threads, each incrementing counter
let handles: Vec<_> = (0..100)
    .map(|_| {
        let client = client.clone();
        thread::spawn(move || {
            loop {
                let current = client.get("counters", "counter").unwrap();
                let new_count = current.value["count"].as_i64().unwrap() + 1;

                match client.update_versioned(
                    "counters",
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
let final_count = client.get("counters", "counter").unwrap();
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
