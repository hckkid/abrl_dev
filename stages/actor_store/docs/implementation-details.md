# Actor Store - Implementation Details

> **Purpose**: This document provides detailed implementation guidance for the Actor Store components. It complements the [main specification](spec.md) with concrete code examples and implementation patterns.

---

## Table of Contents

1. [Component Implementations](#component-implementations)
   - [ActorStoreClient Implementation](#actorstorecllient-implementation)
   - [ActorStore Implementation](#actorstore-implementation)
   - [CDC Module Implementation](#cdc-module-implementation)
2. [Phase-Specific Implementations](#phase-specific-implementations)
   - [Phase 1: In-Memory](#phase-1-in-memory-implementation)
   - [Phase 2: Persistent Storage](#phase-2-persistent-storage-implementation)
   - [Phase 3: Distributed](#phase-3-distributed-implementation)

---

## Component Implementations

### ActorStoreClient Implementation

**Purpose**: Provides the public-facing API for applications to interact with the Actor Store.

**Full Implementation**:

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
    /// Returns ActorData with version = 0 on success
    pub fn insert(
        &self,
        actor_type: impl Into<String>,
        id: impl Into<String>,
        value: Value,
    ) -> Result<ActorData, InsertError> {
        self.store.insert(actor_type.into(), id.into(), value)
    }

    /// Get an actor by ID
    /// Returns None if actor doesn't exist
    pub fn get(
        &self,
        actor_type: impl Into<String>,
        id: &str,
    ) -> Option<ActorData> {
        self.store.get(actor_type.into(), id)
    }

    /// Update an existing actor
    /// Increments version by 1 on success
    pub fn update(
        &self,
        actor_type: impl Into<String>,
        id: &str,
        value: Value,
    ) -> Result<ActorData, UpdateError> {
        self.store.update(actor_type.into(), id, value)
    }

    /// Update with version check (compare-and-swap)
    /// Only succeeds if current version matches expected_version
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
    /// Returns the deleted ActorData if it existed
    pub fn delete(
        &self,
        actor_type: impl Into<String>,
        id: &str,
    ) -> Option<ActorData> {
        self.store.delete(actor_type.into(), id)
    }

    /// Get all actors of a specific type
    /// Returns vector in arbitrary order
    pub fn get_all(
        &self,
        actor_type: impl Into<String>,
    ) -> Vec<ActorData> {
        self.store.get_all(actor_type.into())
    }

    /// Subscribe to change events
    /// Returns a receiver that can be used to consume CDC events
    pub fn subscribe(
        &self,
        options: SubscriptionOptions,
    ) -> Result<CdcSubscription, CdcError> {
        self.cdc_log.subscribe(options)
    }

    /// Get CDC statistics
    /// Returns current CDC metrics
    pub fn cdc_stats(&self) -> CdcStats {
        self.cdc_log.stats()
    }
}

pub struct ActorStoreConfig {
    pub cdc_config: CdcConfig,
    // Future: persistence config, cluster config, etc.
}
```

**Key Implementation Notes**:
- Uses `Arc` for thread-safe sharing of `ActorStore` and `CdcLog`
- All methods use `&self` (interior mutability via DashMap)
- Generic `Into<String>` parameters for ergonomic API usage
- Delegates operations to internal `ActorStore` and `CdcLog` components

---

### ActorStore Implementation

**Purpose**: Internal storage engine that manages data and coordinates with CDC.

**Full Implementation**:

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

        // Create ActorData with version 0
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
        self.cdc_writer.write(UnsequencedEvent {
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
        self.cdc_writer.write(UnsequencedEvent {
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

        // Version check (compare-and-swap)
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
        self.cdc_writer.write(UnsequencedEvent {
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

        // Publish CDC event (ignore errors for delete)
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

**Key Implementation Notes**:
- **Nested DashMap**: Outer map for actor types, inner map for actor IDs
- **Lock-free CDC**: Events published immediately after data mutation
- **Version management**: Starts at 0, increments on each update
- **Error handling**: Proper propagation of errors from DashMap operations
- **Memory efficiency**: Uses `or_insert_with` to avoid unnecessary allocations

---

### CDC Module Implementation

**Purpose**: Separate module for change data capture with lock-free event sequencing.

**Full Implementation**:

```rust
use crossbeam::channel::{bounded, Sender, Receiver};
use crossbeam::queue::ArrayQueue;
use tokio::sync::broadcast;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

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
    /// Pre-allocates sequence number atomically, then enqueues for processing
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
    /// Returns a receiver that can filter events based on options
    pub fn subscribe(
        &self,
        options: SubscriptionOptions,
    ) -> Result<CdcSubscription, CdcError> {
        let receiver = self.inner.broadcast_tx.subscribe();

        // TODO: Handle replay for Beginning/SequenceNumber positions
        // This will require reading from replay_buffer

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
/// Processes events from channel, adds to replay buffer, and broadcasts to subscribers
fn cdc_processor_loop(inner: Arc<CdcLogInner>) {
    while let Ok(event) = inner.receiver.recv() {
        // Add to replay buffer (lock-free, evicts oldest if full)
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
    /// Receive next event (async, with filtering)
    /// Continues looping until an event passes the filter
    pub async fn recv(&mut self) -> Result<ChangeEvent, CdcError> {
        loop {
            let event = self.receiver.recv().await
                .map_err(|_| CdcError::SubscriptionLagged)?;

            // Apply actor_type filter
            if let Some(ref actor_type) = self.filter.actor_type {
                if &event.actor_type != actor_type {
                    continue;
                }
            }

            // Apply key_prefix filter
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

#[derive(Default)]
struct AtomicStats {
    total_events: AtomicU64,
    events_dropped: AtomicU64,
    active_subscriptions: AtomicUsize,
}
```

**Key Implementation Notes**:
- **Lock-free sequencing**: Uses `AtomicU64::fetch_add` for O(1) sequence allocation
- **Decoupled processing**: Background thread handles replay buffer and broadcasting
- **Pull-based consumption**: Subscribers can consume at their own pace (Kafka-style)
- **Filtering**: Applied on subscriber side to reduce network traffic
- **Replay buffer**: Circular buffer using `ArrayQueue` for efficient memory usage

---

## Phase-Specific Implementations

### Phase 1: In-Memory Implementation

**Data Structures**:

```rust
use dashmap::DashMap;
use std::sync::Arc;

// Component 1: Client (Public API)
pub struct ActorStoreClient {
    store: Arc<ActorStore>,
    cdc_log: Arc<CdcLog>,
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

**Implementation Notes**:
- All data stored in memory (lost on restart)
- CDC log stored in circular buffer (limited retention)
- No disk I/O, maximum performance
- Suitable for caching, session storage, temporary data

---

### Phase 2: Persistent Storage Implementation

**Storage Backend Trait**:

```rust
pub trait StorageBackend: Send + Sync {
    /// Insert a new actor with version 0
    fn insert(&self, actor_type: String, id: String, value: Value)
        -> Result<ActorData>;

    /// Get an actor by type and ID
    fn get(&self, actor_type: &str, id: &str)
        -> Option<ActorData>;

    /// Update an existing actor (increments version)
    fn update(&self, actor_type: &str, id: &str, value: Value)
        -> Result<ActorData>;

    /// Delete an actor
    fn delete(&self, actor_type: &str, id: &str)
        -> Option<ActorData>;

    /// Get all actors of a specific type
    fn get_all(&self, actor_type: &str)
        -> Vec<ActorData>;

    /// Create a snapshot of all data
    fn snapshot(&self)
        -> Result<Snapshot>;

    /// Restore from a snapshot
    fn restore(&self, snapshot: Snapshot)
        -> Result<()>;
}
```

**RocksDB Implementation**:

```rust
pub struct RocksDBBackend {
    db: rocksdb::DB,
    wal: WriteAheadLog,
}

impl StorageBackend for RocksDBBackend {
    fn insert(&self, actor_type: String, id: String, value: Value)
        -> Result<ActorData> {
        // 1. Create key: "{actor_type}:{id}"
        let db_key = format!("{}:{}", actor_type, id);

        // 2. Create ActorData with version 0
        let actor = ActorData { id, value, version: 0 };

        // 3. Serialize to bytes
        let bytes = bincode::serialize(&actor)?;

        // 4. Write to RocksDB
        self.db.put(db_key.as_bytes(), &bytes)?;

        // 5. Append to WAL for crash recovery
        self.wal.append(&Operation::Insert {
            actor_type,
            actor: actor.clone()
        })?;

        Ok(actor)
    }

    // ... other methods follow similar pattern
}
```

**Write-Ahead Log (WAL)**:

```rust
pub struct WriteAheadLog {
    file: std::fs::File,
    path: PathBuf,
}

impl WriteAheadLog {
    /// Append an operation to the WAL
    /// Format: [length: u32][operation: bytes]
    pub fn append(&mut self, op: &Operation) -> Result<()> {
        let bytes = bincode::serialize(op)?;
        let len = bytes.len() as u32;

        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&bytes)?;
        self.file.sync_all()?; // fsync for durability

        Ok(())
    }

    /// Replay all operations from WAL
    /// Used during recovery after crash
    pub fn replay<F>(&mut self, mut apply: F) -> Result<()>
    where
        F: FnMut(Operation) -> Result<()>
    {
        let mut file = std::fs::File::open(&self.path)?;

        loop {
            // Read length
            let mut len_bytes = [0u8; 4];
            if file.read_exact(&mut len_bytes).is_err() {
                break; // EOF
            }
            let len = u32::from_le_bytes(len_bytes);

            // Read operation
            let mut op_bytes = vec![0u8; len as usize];
            file.read_exact(&mut op_bytes)?;

            let op: Operation = bincode::deserialize(&op_bytes)?;
            apply(op)?;
        }

        Ok(())
    }
}
```

**Implementation Notes**:
- RocksDB provides LSM-tree storage (optimized for writes)
- WAL enables crash recovery (replay operations)
- Snapshot creates point-in-time backup
- CDC log also persisted to disk (separate WAL)

---

### Phase 3: Distributed Implementation

**Raft Integration** (not implemented in Phase 1, but architecture prepared):

```rust
pub struct DistributedActorStore {
    // Local storage
    local_store: Arc<ActorStore>,

    // Raft consensus
    raft: Arc<Raft<TypeConfig>>,

    // Cluster state
    node_id: NodeId,
    peers: Arc<RwLock<HashMap<NodeId, PeerInfo>>>,
}

impl DistributedActorStore {
    /// Insert with Raft consensus
    pub async fn insert(
        &self,
        actor_type: String,
        id: String,
        value: Value,
    ) -> Result<ActorData> {
        // 1. Check if leader
        if !self.raft.is_leader() {
            return Err(Error::NotLeader);
        }

        // 2. Propose to Raft
        let proposal = Proposal::Insert { actor_type, id, value };
        let result = self.raft.propose(proposal).await?;

        // 3. Wait for quorum (W = N/2 + 1)
        result.await
    }
}
```

**Implementation Notes**:
- Leader-based replication (writes go to leader)
- Raft ensures strong consistency
- Automatic failover (new leader election)
- Parallel follower replication for performance

---

## Summary

This document provides detailed implementation guidance for:
- **Component structure**: How ActorStoreClient, ActorStore, and CDC interact
- **Data flow**: From API call → storage → CDC → subscribers
- **Concurrency patterns**: Lock-free CDC, interior mutability, atomic operations
- **Phase evolution**: From in-memory → persistent → distributed

For high-level architecture and API contracts, see the [main specification](spec.md).