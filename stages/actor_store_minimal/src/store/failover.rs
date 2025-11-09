use std::sync::Arc;
use crate::store::coordinator::{ClusterCoordinator, CoordinatorEvent};
use crate::store::election::ElectionManager;
use crate::store::replication::{ReplicationEngine, NodeRole, NodeId};
use crate::store::sync::NodeSyncManager;
use crate::store::store::ConcurrentActorStore;

pub struct FailoverManager {
    node_id: NodeId,
    coordinator: Arc<ClusterCoordinator>,
    election: Arc<ElectionManager>,
    replication: Arc<ReplicationEngine>,
    sync: Arc<NodeSyncManager>,
    store: Arc<ConcurrentActorStore>,
}

impl FailoverManager {
    pub fn new(
        node_id: NodeId,
        coordinator: Arc<ClusterCoordinator>,
        election: Arc<ElectionManager>,
        replication: Arc<ReplicationEngine>,
        sync: Arc<NodeSyncManager>,
        store: Arc<ConcurrentActorStore>,
    ) -> Self {
        FailoverManager {
            node_id,
            coordinator,
            election,
            replication,
            sync,
            store,
        }
    }

    pub async fn handle_leader_failure(&self) -> Result<(), String> {
        // Detected leader failure, start election
        println!("[Failover] Leader failure detected, starting election...");

        let cluster_view = self.coordinator.get_cluster_view().await;
        let cluster_size = cluster_view.len();

        let (term, _vote_requests) = self.election.start_election(cluster_size).await;

        println!("[Failover] Started election for term {}", term);

        // In a real implementation, we'd send vote requests and wait for responses
        // For now, simulate becoming leader if we have quorum
        if self.election.check_quorum(term, cluster_size).await {
            self.become_leader(term).await?;
        }

        Ok(())
    }

    async fn become_leader(&self, term: u64) -> Result<(), String> {
        println!("[Failover] Node {} became leader for term {}", self.node_id, term);

        // Promote to leader role
        self.replication.promote_to_leader().await;

        // Update coordinator
        self.coordinator.set_leader(self.node_id.clone(), term).await;

        Ok(())
    }

    pub async fn handle_follower_reconnect(&self, leader_addr: String) -> Result<(), String> {
        println!("[Failover] Reconnecting to new leader at {}", leader_addr);

        // Check if we have a term mismatch (we were partitioned leader)
        let local_term = self.store.get_term();
        let coordinator_term = self.coordinator.get_term().await;

        if local_term < coordinator_term {
            println!("[Failover] Term mismatch detected (local={}, coordinator={})", local_term, coordinator_term);

            // Rollback uncommitted writes from stale term
            let committed_seq = self.store.change_log().get_committed_sequence();
            self.store.rollback_uncommitted_writes(committed_seq);

            // Update our term
            self.store.set_term(coordinator_term);
        }

        // Demote to follower
        self.replication.demote_to_follower(leader_addr.clone()).await;
        self.store.set_role(NodeRole::Follower);

        // Start sync process
        self.sync.start_sync().await;

        println!("[Failover] Sync started with new leader");

        Ok(())
    }

    pub async fn detect_failures(&self) -> Vec<NodeId> {
        let events = self.coordinator.check_health().await;
        let mut failed_nodes = Vec::new();

        for event in events {
            if let CoordinatorEvent::NodeFailed { node_id } = event {
                println!("[Failover] Detected failure of node: {}", node_id);
                failed_nodes.push(node_id);
            }
        }

        failed_nodes
    }

    pub async fn recover_from_partition(&self) -> Result<(), String> {
        println!("[Failover] Recovering from network partition...");

        // Check if we're still part of the cluster
        let leader = self.coordinator.get_leader().await;

        if leader.is_none() {
            // No leader, trigger election
            self.handle_leader_failure().await?;
        } else {
            // Leader exists, verify we can reach it
            let current_role = self.replication.get_role().await;

            if current_role == NodeRole::Follower {
                // Re-sync with leader
                println!("[Failover] Re-syncing with leader after partition");
                self.sync.start_sync().await;
            }
        }

        Ok(())
    }
}
