use serde::{Serialize, Deserialize};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use std::collections::HashMap;
use crate::store::cdc::ChangeEvent;
use crate::store::snapshot::Snapshot;

pub type NodeId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    Leader,
    Follower,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicationMessage {
    // Leader -> Follower: Push change events
    PushChanges { events: Vec<ChangeEvent> },

    // Follower -> Leader: Ack receipt of sequences
    Ack { node_id: NodeId, up_to_sequence: u64 },

    // Follower -> Leader: Request snapshot
    RequestSnapshot,

    // Leader -> Follower: Send snapshot
    SendSnapshot { snapshot: Snapshot },

    // Follower -> Leader: Pull changes from sequence
    PullChanges { from_sequence: u64, batch_size: usize },

    // Leader -> Follower: Response to pull
    ChangesBatch { events: Vec<ChangeEvent> },
}

pub struct ReplicationEngine {
    role: Arc<RwLock<NodeRole>>,
    node_id: NodeId,
    leader_endpoint: Arc<RwLock<Option<String>>>,
    follower_positions: Arc<RwLock<HashMap<NodeId, u64>>>,
    outbound_tx: mpsc::UnboundedSender<(NodeId, ReplicationMessage)>,
    outbound_rx: Arc<RwLock<mpsc::UnboundedReceiver<(NodeId, ReplicationMessage)>>>,
}

impl ReplicationEngine {
    pub fn new(node_id: NodeId, role: NodeRole) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        ReplicationEngine {
            role: Arc::new(RwLock::new(role)),
            node_id,
            leader_endpoint: Arc::new(RwLock::new(None)),
            follower_positions: Arc::new(RwLock::new(HashMap::new())),
            outbound_tx: tx,
            outbound_rx: Arc::new(RwLock::new(rx)),
        }
    }

    pub async fn get_role(&self) -> NodeRole {
        *self.role.read().await
    }

    pub async fn promote_to_leader(&self) {
        *self.role.write().await = NodeRole::Leader;
        *self.leader_endpoint.write().await = None;
        self.follower_positions.write().await.clear();
    }

    pub async fn demote_to_follower(&self, leader_addr: String) {
        *self.role.write().await = NodeRole::Follower;
        *self.leader_endpoint.write().await = Some(leader_addr);
    }

    pub async fn push(&self, event: ChangeEvent, followers: Vec<NodeId>) {
        if self.get_role().await != NodeRole::Leader {
            return;
        }

        let msg = ReplicationMessage::PushChanges { events: vec![event] };
        for follower_id in followers {
            let _ = self.outbound_tx.send((follower_id, msg.clone()));
        }
    }

    pub async fn ack(&self, node_id: NodeId, up_to_sequence: u64) {
        let mut positions = self.follower_positions.write().await;
        positions.insert(node_id, up_to_sequence);
    }

    pub async fn get_follower_positions(&self) -> HashMap<NodeId, u64> {
        self.follower_positions.read().await.clone()
    }

    pub async fn get_follower_lag(&self, node_id: &NodeId) -> Option<u64> {
        let positions = self.follower_positions.read().await;
        positions.get(node_id).copied()
    }

    pub fn send_message(&self, to: NodeId, msg: ReplicationMessage) -> Result<(), String> {
        self.outbound_tx.send((to, msg))
            .map_err(|e| format!("Failed to send message: {}", e))
    }

    pub async fn recv_message(&self) -> Option<(NodeId, ReplicationMessage)> {
        self.outbound_rx.write().await.recv().await
    }
}
