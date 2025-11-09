use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use crate::store::types::ActorData;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub collections: HashMap<String, HashMap<String, ActorData>>,
    pub as_of_sequence: u64,
}

impl Snapshot {
    pub fn new(collections: HashMap<String, HashMap<String, ActorData>>, as_of_sequence: u64) -> Self {
        Snapshot {
            collections,
            as_of_sequence,
        }
    }
}
