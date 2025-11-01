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
