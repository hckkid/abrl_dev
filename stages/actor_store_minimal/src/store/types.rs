use serde::{Serialize, Deserialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorData {
    pub id: String,
    pub value: Value,
    pub version: u32
}

pub enum CollectionCommand {
    Create(ActorData),
    Update {
        id: String,
        value: Value,
    },
    Delete {
        id: String
    },
    GetAll,
}

pub trait ActorStoreCommandExecutor {
    fn execute_command(&self, collection_name: String ,cmd: CollectionCommand) -> Result<Option<ActorData>, String>;
    fn create_collection(&self, collection_name: String) -> Result<(), String>;
}