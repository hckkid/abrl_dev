use std::collections::{VecDeque, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub sequence: u64,
    pub term: u64,  // Leadership epoch when this event was created
    pub operation: Operation,
    pub key: String,
    pub prev_value: Option<Value>,
    pub new_value: Option<Value>,
    pub prev_version: Option<u32>,
    pub new_version: Option<u32>,
    pub timestamp: DateTime<Utc>,
}

pub type SubscriberId = u64;

#[derive(Debug, Clone, Copy)]
pub enum CdcConsistency {
    Majority,  // N/2 + 1 replicas acknowledged
    All,       // All replicas acknowledged
}

#[derive(Debug, Clone)]
pub enum StartPosition {
    Beginning,      // Start from checkpoint (oldest in buffer)
    Sequence(u64),  // Start from specific sequence
    Now,            // Start from next sequence (only new events)
}

struct Subscriber {
    id: SubscriberId,
    last_seen: u64,
}

pub struct ChangeLog {
    buffer: parking_lot::RwLock<VecDeque<ChangeEvent>>,
    next_sequence: AtomicU64,
    buffer_size: usize,
    subscribers: parking_lot::RwLock<HashMap<SubscriberId, Subscriber>>,
    next_subscriber_id: AtomicU64,
    committed_sequence: parking_lot::RwLock<u64>,
    consistency_level: CdcConsistency,
    cluster_size: parking_lot::RwLock<usize>,
}

impl ChangeLog {
    pub fn new(buffer_size: usize) -> Self {
        Self::new_with_consistency(buffer_size, CdcConsistency::Majority, 1)
    }

    pub fn new_with_consistency(buffer_size: usize, consistency: CdcConsistency, cluster_size: usize) -> Self {
        ChangeLog {
            buffer: parking_lot::RwLock::new(VecDeque::with_capacity(buffer_size)),
            next_sequence: AtomicU64::new(1),
            buffer_size,
            subscribers: parking_lot::RwLock::new(HashMap::new()),
            next_subscriber_id: AtomicU64::new(1),
            committed_sequence: parking_lot::RwLock::new(0),
            consistency_level: consistency,
            cluster_size: parking_lot::RwLock::new(cluster_size),
        }
    }

    pub fn append(&self, mut event: ChangeEvent) -> u64 {
        let sequence = self.next_sequence.fetch_add(1, Ordering::SeqCst);
        event.sequence = sequence;

        let mut buffer = self.buffer.write();
        if buffer.len() >= self.buffer_size {
            buffer.pop_front();
        }
        buffer.push_back(event);

        sequence
    }

    pub fn get_checkpoint(&self) -> u64 {
        let buffer = self.buffer.read();
        buffer.front().map(|e| e.sequence).unwrap_or(0)
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence.load(Ordering::SeqCst)
    }

    pub fn subscribe(&self, start_position: StartPosition) -> Result<SubscriberId, String> {
        let last_seen = match start_position {
            StartPosition::Beginning => {
                let checkpoint = self.get_checkpoint();
                if checkpoint == 0 {
                    0
                } else {
                    checkpoint - 1
                }
            }
            StartPosition::Sequence(seq) => {
                let checkpoint = self.get_checkpoint();
                let next_seq = self.next_sequence();

                if seq < checkpoint {
                    return Err(format!(
                        "Sequence {} is before checkpoint {}. Buffer has overwritten that data.",
                        seq, checkpoint
                    ));
                }
                if seq >= next_seq {
                    return Err(format!(
                        "Sequence {} is beyond current sequence {}",
                        seq, next_seq
                    ));
                }
                seq - 1 // Start from one before so we include this sequence
            }
            StartPosition::Now => self.next_sequence() - 1,
        };

        let id = self.next_subscriber_id.fetch_add(1, Ordering::SeqCst);
        let subscriber = Subscriber { id, last_seen };

        self.subscribers.write().insert(id, subscriber);

        Ok(id)
    }

    pub fn poll(&self, subscriber_id: SubscriberId, batch_size: usize) -> Result<Vec<ChangeEvent>, String> {
        let mut subscribers = self.subscribers.write();
        let subscriber = subscribers.get_mut(&subscriber_id)
            .ok_or_else(|| format!("Subscriber {} not found", subscriber_id))?;

        let buffer = self.buffer.read();
        let committed = *self.committed_sequence.read();
        let mut events = Vec::new();

        for event in buffer.iter() {
            // Only return events that are:
            // 1. After last seen by subscriber
            // 2. Committed (replicated to required quorum)
            if event.sequence > subscriber.last_seen && event.sequence <= committed {
                events.push(event.clone());
                if events.len() >= batch_size {
                    break;
                }
            }
        }

        if let Some(last_event) = events.last() {
            subscriber.last_seen = last_event.sequence;
        }

        Ok(events)
    }

    pub fn unsubscribe(&self, subscriber_id: SubscriberId) -> Result<(), String> {
        self.subscribers.write().remove(&subscriber_id)
            .ok_or_else(|| format!("Subscriber {} not found", subscriber_id))?;
        Ok(())
    }

    pub fn update_committed_sequence(&self, follower_acks: &HashMap<String, u64>) {
        let cluster_size = *self.cluster_size.read();
        if cluster_size == 0 {
            return;
        }

        let mut acks: Vec<u64> = follower_acks.values().copied().collect();
        acks.sort();

        let new_committed = match self.consistency_level {
            CdcConsistency::Majority => {
                let quorum_index = (cluster_size / 2).saturating_sub(1);
                if acks.len() > quorum_index {
                    acks[quorum_index]
                } else {
                    0
                }
            }
            CdcConsistency::All => {
                if acks.len() == cluster_size - 1 {  // -1 because leader doesn't ack itself
                    *acks.iter().min().unwrap_or(&0)
                } else {
                    0
                }
            }
        };

        let mut committed = self.committed_sequence.write();
        if new_committed > *committed {
            *committed = new_committed;
        }
    }

    pub fn get_committed_sequence(&self) -> u64 {
        *self.committed_sequence.read()
    }

    pub fn set_cluster_size(&self, size: usize) {
        *self.cluster_size.write() = size;
    }

    pub fn rollback_to(&self, sequence: u64) {
        let mut buffer = self.buffer.write();
        buffer.retain(|event| event.sequence <= sequence);
    }
}
