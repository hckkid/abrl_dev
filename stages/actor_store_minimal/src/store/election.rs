use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::store::replication::NodeId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ElectionMessage {
    RequestVote { candidate_id: NodeId, term: u64 },
    GrantVote { voter_id: NodeId, term: u64, candidate_id: NodeId, granted: bool },
}

pub struct ElectionManager {
    node_id: NodeId,
    current_term: Arc<RwLock<u64>>,
    voted_for: Arc<RwLock<Option<NodeId>>>,
    pending_votes: Arc<RwLock<HashMap<u64, HashSet<NodeId>>>>,
}

impl ElectionManager {
    pub fn new(node_id: NodeId) -> Self {
        ElectionManager {
            node_id,
            current_term: Arc::new(RwLock::new(0)),
            voted_for: Arc::new(RwLock::new(None)),
            pending_votes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_election(&self, cluster_size: usize) -> (u64, Vec<ElectionMessage>) {
        let mut term = self.current_term.write().await;
        *term += 1;
        let new_term = *term;

        *self.voted_for.write().await = Some(self.node_id.clone());

        self.pending_votes.write().await.insert(new_term, HashSet::from([self.node_id.clone()]));

        // Create vote requests for all other nodes
        let msg = ElectionMessage::RequestVote {
            candidate_id: self.node_id.clone(),
            term: new_term,
        };

        // Return messages to broadcast
        (new_term, vec![msg; cluster_size - 1])
    }

    pub async fn handle_vote_request(
        &self,
        candidate_id: NodeId,
        term: u64,
    ) -> ElectionMessage {
        let mut current_term = self.current_term.write().await;
        let mut voted_for = self.voted_for.write().await;

        let granted = if term > *current_term {
            // New term, reset state
            *current_term = term;
            *voted_for = Some(candidate_id.clone());
            true
        } else if term == *current_term {
            // Same term, grant if we haven't voted yet
            if voted_for.is_none() {
                *voted_for = Some(candidate_id.clone());
                true
            } else {
                voted_for.as_ref() == Some(&candidate_id)
            }
        } else {
            // Old term, reject
            false
        };

        ElectionMessage::GrantVote {
            voter_id: self.node_id.clone(),
            term,
            candidate_id,
            granted,
        }
    }

    pub async fn handle_vote_response(
        &self,
        voter_id: NodeId,
        term: u64,
        granted: bool,
    ) -> Option<bool> {
        if !granted {
            return None;
        }

        let mut votes = self.pending_votes.write().await;
        let vote_set = votes.entry(term).or_insert_with(HashSet::new);
        vote_set.insert(voter_id);

        None // Caller will check if quorum reached
    }

    pub async fn check_quorum(&self, term: u64, cluster_size: usize) -> bool {
        let votes = self.pending_votes.read().await;
        if let Some(vote_set) = votes.get(&term) {
            let quorum = (cluster_size / 2) + 1;
            vote_set.len() >= quorum
        } else {
            false
        }
    }

    pub async fn get_current_term(&self) -> u64 {
        *self.current_term.read().await
    }

    pub async fn update_term(&self, new_term: u64) {
        let mut term = self.current_term.write().await;
        if new_term > *term {
            *term = new_term;
            *self.voted_for.write().await = None;
        }
    }
}
