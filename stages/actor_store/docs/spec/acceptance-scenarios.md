
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
