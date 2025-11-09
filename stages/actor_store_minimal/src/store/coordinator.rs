use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc, Duration};
use crate::store::replication::{NodeId, NodeRole};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub address: String,
    pub role: NodeRole,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    Healthy,
    Suspected,
    Failed,
}

#[derive(Debug, Clone)]
struct NodeHealth {
    last_heartbeat: DateTime<Utc>,
    status: NodeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinatorEvent {
    MembershipChange { added: Option<NodeId>, removed: Option<NodeId> },
    LeaderElected { node_id: NodeId, term: u64 },
    NodeFailed { node_id: NodeId },
}

pub struct ClusterCoordinator {
    membership: Arc<RwLock<HashMap<NodeId, NodeInfo>>>,
    term: Arc<RwLock<u64>>,
    leader: Arc<RwLock<Option<NodeId>>>,
    node_health: Arc<RwLock<HashMap<NodeId, NodeHealth>>>,
    heartbeat_timeout: Duration,
}

impl ClusterCoordinator {
    pub fn new() -> Self {
        ClusterCoordinator {
            membership: Arc::new(RwLock::new(HashMap::new())),
            term: Arc::new(RwLock::new(0)),
            leader: Arc::new(RwLock::new(None)),
            node_health: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_timeout: Duration::seconds(5),
        }
    }

    pub async fn join(&self, node_id: NodeId, address: String, role: NodeRole) -> Result<CoordinatorEvent, String> {
        let node_info = NodeInfo {
            node_id: node_id.clone(),
            address,
            role,
            joined_at: Utc::now(),
        };

        self.membership.write().await.insert(node_id.clone(), node_info);

        self.node_health.write().await.insert(
            node_id.clone(),
            NodeHealth {
                last_heartbeat: Utc::now(),
                status: NodeStatus::Healthy,
            },
        );

        Ok(CoordinatorEvent::MembershipChange {
            added: Some(node_id),
            removed: None,
        })
    }

    pub async fn leave(&self, node_id: NodeId) -> Result<CoordinatorEvent, String> {
        self.membership.write().await.remove(&node_id);
        self.node_health.write().await.remove(&node_id);

        // If leaving node was leader, trigger re-election
        let leader = self.leader.read().await;
        if leader.as_ref() == Some(&node_id) {
            drop(leader);
            *self.leader.write().await = None;
        }

        Ok(CoordinatorEvent::MembershipChange {
            added: None,
            removed: Some(node_id),
        })
    }

    pub async fn heartbeat(&self, node_id: NodeId) -> Result<(), String> {
        let mut health = self.node_health.write().await;
        if let Some(node_health) = health.get_mut(&node_id) {
            node_health.last_heartbeat = Utc::now();
            node_health.status = NodeStatus::Healthy;
            Ok(())
        } else {
            Err(format!("Node {} not found", node_id))
        }
    }

    pub async fn check_health(&self) -> Vec<CoordinatorEvent> {
        let mut events = Vec::new();
        let now = Utc::now();
        let mut health = self.node_health.write().await;

        for (node_id, node_health) in health.iter_mut() {
            let elapsed = now.signed_duration_since(node_health.last_heartbeat);

            if elapsed > self.heartbeat_timeout {
                if node_health.status == NodeStatus::Healthy {
                    node_health.status = NodeStatus::Suspected;
                } else if node_health.status == NodeStatus::Suspected {
                    node_health.status = NodeStatus::Failed;
                    events.push(CoordinatorEvent::NodeFailed { node_id: node_id.clone() });
                }
            }
        }

        events
    }

    pub async fn get_cluster_view(&self) -> Vec<NodeInfo> {
        self.membership.read().await.values().cloned().collect()
    }

    pub async fn get_leader(&self) -> Option<NodeId> {
        self.leader.read().await.clone()
    }

    pub async fn set_leader(&self, node_id: NodeId, term: u64) -> CoordinatorEvent {
        *self.leader.write().await = Some(node_id.clone());
        *self.term.write().await = term;
        CoordinatorEvent::LeaderElected { node_id, term }
    }

    pub async fn get_term(&self) -> u64 {
        *self.term.read().await
    }

    pub async fn increment_term(&self) -> u64 {
        let mut term = self.term.write().await;
        *term += 1;
        *term
    }
}
