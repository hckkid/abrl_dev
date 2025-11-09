use serde::{Serialize, Deserialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncPhase {
    Idle,
    RequestingSnapshot,
    ApplyingSnapshot,
    StreamingChanges,
    Active,
}

pub struct SyncProgress {
    pub applied: u64,
    pub remaining: Option<u64>,
}

pub struct NodeSyncManager {
    phase: Arc<RwLock<SyncPhase>>,
    snapshot_sequence: Arc<RwLock<Option<u64>>>,
    applied_until: Arc<RwLock<u64>>,
    target_sequence: Arc<RwLock<Option<u64>>>,
}

impl NodeSyncManager {
    pub fn new() -> Self {
        NodeSyncManager {
            phase: Arc::new(RwLock::new(SyncPhase::Idle)),
            snapshot_sequence: Arc::new(RwLock::new(None)),
            applied_until: Arc::new(RwLock::new(0)),
            target_sequence: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get_phase(&self) -> SyncPhase {
        *self.phase.read().await
    }

    pub async fn start_sync(&self) {
        *self.phase.write().await = SyncPhase::RequestingSnapshot;
    }

    pub async fn snapshot_received(&self, as_of_sequence: u64) {
        *self.snapshot_sequence.write().await = Some(as_of_sequence);
        *self.phase.write().await = SyncPhase::ApplyingSnapshot;
    }

    pub async fn snapshot_applied(&self) {
        let snapshot_seq = self.snapshot_sequence.read().await.unwrap_or(0);
        *self.applied_until.write().await = snapshot_seq;
        *self.phase.write().await = SyncPhase::StreamingChanges;
    }

    pub async fn apply_change(&self, sequence: u64) {
        let mut applied = self.applied_until.write().await;
        if sequence > *applied {
            *applied = sequence;
        }
    }

    pub async fn set_target_sequence(&self, target: u64) {
        *self.target_sequence.write().await = Some(target);
    }

    pub async fn check_if_caught_up(&self) -> bool {
        let applied = *self.applied_until.read().await;
        let target = *self.target_sequence.read().await;

        if let Some(target_seq) = target {
            if applied >= target_seq {
                *self.phase.write().await = SyncPhase::Active;
                return true;
            }
        }

        false
    }

    pub async fn mark_active(&self) {
        *self.phase.write().await = SyncPhase::Active;
    }

    pub async fn get_progress(&self) -> SyncProgress {
        let applied = *self.applied_until.read().await;
        let target = *self.target_sequence.read().await;

        let remaining = target.map(|t| if t > applied { t - applied } else { 0 });

        SyncProgress {
            applied,
            remaining,
        }
    }

    pub async fn get_last_applied(&self) -> u64 {
        *self.applied_until.read().await
    }
}
