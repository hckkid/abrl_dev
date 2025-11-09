use std::sync::Arc;
use crate::store::{
    store::ConcurrentActorStore,
    replication::{ReplicationEngine, NodeRole, NodeId},
    coordinator::ClusterCoordinator,
    election::ElectionManager,
    sync::NodeSyncManager,
    failover::FailoverManager,
    network::NetworkLayer,
};

pub struct ActorStoreNode {
    pub node_id: NodeId,
    pub store: Arc<ConcurrentActorStore>,
    pub replication: Arc<ReplicationEngine>,
    pub coordinator: Arc<ClusterCoordinator>,
    pub election: Arc<ElectionManager>,
    pub sync: Arc<NodeSyncManager>,
    pub failover: Arc<FailoverManager>,
    pub network: Arc<NetworkLayer>,
}

impl ActorStoreNode {
    pub fn new(node_id: NodeId, listen_addr: String, role: NodeRole) -> Self {
        let store = Arc::new(ConcurrentActorStore::new());
        let replication = Arc::new(ReplicationEngine::new(node_id.clone(), role));
        let coordinator = Arc::new(ClusterCoordinator::new());
        let election = Arc::new(ElectionManager::new(node_id.clone()));
        let sync = Arc::new(NodeSyncManager::new());
        let network = Arc::new(NetworkLayer::new(node_id.clone(), listen_addr));

        let failover = Arc::new(FailoverManager::new(
            node_id.clone(),
            coordinator.clone(),
            election.clone(),
            replication.clone(),
            sync.clone(),
            store.clone(),
        ));

        ActorStoreNode {
            node_id,
            store,
            replication,
            coordinator,
            election,
            sync,
            failover,
            network,
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        // In a full implementation, start network listeners and background tasks
        println!("[Node {}] Starting...", self.node_id);
        Ok(())
    }

    pub async fn join_cluster(&self, address: String) -> Result<(), String> {
        let role = self.replication.get_role().await;
        self.coordinator.join(self.node_id.clone(), address, role).await?;
        Ok(())
    }

    pub async fn send_heartbeat(&self) -> Result<(), String> {
        self.coordinator.heartbeat(self.node_id.clone()).await
    }
}
