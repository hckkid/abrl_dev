use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use crate::store::types::{ActorData, ActorStoreCommandExecutor, CollectionCommand};
use crate::store::cdc::{ChangeLog, ChangeEvent, Operation};
use crate::store::snapshot::Snapshot;
use crate::store::replication::NodeRole;
use chrono::Utc;

pub struct ConcurrentActorStore {
    store: DashMap<String, DashMap<String, ActorData>>,
    change_log: ChangeLog,
    current_term: Arc<RwLock<u64>>,
    current_role: Arc<RwLock<NodeRole>>,
}

impl ConcurrentActorStore {
    pub fn new() -> ConcurrentActorStore {
        ConcurrentActorStore {
            store: DashMap::new(),
            change_log: ChangeLog::new(10000), // 10k events buffer
            current_term: Arc::new(RwLock::new(0)),
            current_role: Arc::new(RwLock::new(NodeRole::Follower)),
        }
    }

    pub fn change_log(&self) -> &ChangeLog {
        &self.change_log
    }

    pub fn set_term(&self, term: u64) {
        *self.current_term.write() = term;
    }

    pub fn get_term(&self) -> u64 {
        *self.current_term.read()
    }

    pub fn set_role(&self, role: NodeRole) {
        *self.current_role.write() = role;
    }

    pub fn get_role(&self) -> NodeRole {
        *self.current_role.read()
    }

    pub fn is_current_leader(&self) -> bool {
        *self.current_role.read() == NodeRole::Leader
    }

    pub fn get_snapshot(&self) -> Snapshot {
        let mut collections = HashMap::new();

        for entry in self.store.iter() {
            let collection_name = entry.key().clone();
            let collection_data = entry.value();

            let mut actors = HashMap::new();
            for actor_entry in collection_data.iter() {
                actors.insert(actor_entry.key().clone(), actor_entry.value().clone());
            }

            collections.insert(collection_name, actors);
        }

        let as_of_sequence = self.change_log.next_sequence() - 1;
        Snapshot::new(collections, as_of_sequence)
    }

    pub fn apply_snapshot(&self, snapshot: Snapshot) -> Result<(), String> {
        // Clear existing data
        self.store.clear();

        // Apply snapshot data
        for (collection_name, actors) in snapshot.collections {
            let collection = DashMap::new();
            for (id, actor_data) in actors {
                collection.insert(id, actor_data);
            }
            self.store.insert(collection_name, collection);
        }

        Ok(())
    }

    pub fn rollback_uncommitted_writes(&self, committed_sequence: u64) {
        println!("[Store] Rolling back uncommitted writes beyond sequence {}", committed_sequence);

        // Rollback change log
        self.change_log.rollback_to(committed_sequence);

        // Note: Full store rollback would require replaying from snapshot or from committed events
        // For now, we rely on sync from leader to fix the store state
        // A production implementation would need to reverse-apply uncommitted changes
    }
}

impl ActorStoreCommandExecutor for ConcurrentActorStore {
    fn execute_command(&self, collection_name: String, cmd: CollectionCommand) -> Result<Option<ActorData>, String> {
        // Validate leadership for write operations
        match &cmd {
            CollectionCommand::Create(_) | CollectionCommand::Update { .. } | CollectionCommand::Delete { .. } => {
                if !self.is_current_leader() {
                    return Err("Not current leader, cannot accept writes".to_string());
                }
            }
            _ => {} // GetAll doesn't need leadership check
        }

        let coll = self.store.get(&collection_name);
        if coll.is_none() {
            return Err(format!("Collection {} not found", collection_name));
        }
        let coll = coll.unwrap();
        let current_term = self.get_term();

        match cmd {
            CollectionCommand::Create(mut actor_data) => {
                actor_data.version = 0;
                let id = actor_data.id.clone();
                let value = actor_data.value.clone();

                coll.value().entry(id.clone()).or_insert_with(|| {
                    // Emit CDC event with current term
                    let event = ChangeEvent {
                        sequence: 0, // will be set by append
                        term: current_term,
                        operation: Operation::Insert,
                        key: id.clone(),
                        prev_value: None,
                        new_value: Some(value.clone()),
                        prev_version: None,
                        new_version: Some(0),
                        timestamp: Utc::now(),
                    };
                    self.change_log.append(event);
                    actor_data.clone()
                });
                Ok(None)
            }
            CollectionCommand::Update { id, value } => {
                use dashmap::mapref::entry::Entry;
                match coll.value().entry(id.clone()) {
                    Entry::Occupied(mut occupied) => {
                        let old_data = occupied.get().clone();
                        let existing = occupied.get_mut();
                        existing.value = value.clone();
                        existing.version += 1;

                        // Emit CDC event with current term
                        let event = ChangeEvent {
                            sequence: 0, // will be set by append
                            term: current_term,
                            operation: Operation::Update,
                            key: id,
                            prev_value: Some(old_data.value.clone()),
                            new_value: Some(value),
                            prev_version: Some(old_data.version),
                            new_version: Some(existing.version),
                            timestamp: Utc::now(),
                        };
                        self.change_log.append(event);

                        Ok(Some(old_data))
                    }
                    Entry::Vacant(_) => {
                        Err(format!("Actor {} not found", id))
                    }
                }
            }
            CollectionCommand::Delete { id } => {
                match coll.value().remove(&id) {
                    Some((_, old_data)) => {
                        // Emit CDC event with current term
                        let event = ChangeEvent {
                            sequence: 0, // will be set by append
                            term: current_term,
                            operation: Operation::Delete,
                            key: id,
                            prev_value: Some(old_data.value.clone()),
                            new_value: None,
                            prev_version: Some(old_data.version),
                            new_version: None,
                            timestamp: Utc::now(),
                        };
                        self.change_log.append(event);

                        Ok(Some(old_data))
                    }
                    None => Err(format!("Actor {} not found", id))
                }
            }
            CollectionCommand::GetAll => {
                // GetAll doesn't make sense at collection level, use get_snapshot() instead
                Err("GetAll not supported via execute_command, use get_snapshot()".to_string())
            }
        }
    }

    fn create_collection(&self, collection_name: String) -> Result<(), String> {
        self.store.entry(collection_name).or_insert_with(|| DashMap::new());
        Ok(())
    }
}