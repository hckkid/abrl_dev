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
