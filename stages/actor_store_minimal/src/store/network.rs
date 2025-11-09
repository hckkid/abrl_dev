use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use crate::store::replication::{ReplicationMessage, NodeId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub from: NodeId,
    pub to: NodeId,
    pub payload: ReplicationMessage,
}

pub struct NetworkLayer {
    node_id: NodeId,
    listen_addr: String,
}

impl NetworkLayer {
    pub fn new(node_id: NodeId, listen_addr: String) -> Self {
        NetworkLayer {
            node_id,
            listen_addr,
        }
    }

    pub async fn start_server<F>(&self, handler: F) -> Result<(), String>
    where
        F: Fn(Message) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind(&self.listen_addr)
            .await
            .map_err(|e| format!("Failed to bind to {}: {}", self.listen_addr, e))?;

        let handler = Arc::new(handler);

        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    let handler = handler.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(socket, handler).await {
                            eprintln!("Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Accept error: {}", e);
                }
            }
        }
    }

    async fn handle_connection<F>(mut socket: TcpStream, handler: Arc<F>) -> Result<(), String>
    where
        F: Fn(Message) + Send + Sync,
    {
        let mut len_buf = [0u8; 4];
        socket.read_exact(&mut len_buf).await
            .map_err(|e| format!("Failed to read length: {}", e))?;

        let len = u32::from_be_bytes(len_buf) as usize;
        let mut data_buf = vec![0u8; len];

        socket.read_exact(&mut data_buf).await
            .map_err(|e| format!("Failed to read data: {}", e))?;

        let msg: Message = bincode::deserialize(&data_buf)
            .map_err(|e| format!("Failed to deserialize: {}", e))?;

        handler(msg);
        Ok(())
    }

    pub async fn send_message(&self, to_addr: &str, msg: Message) -> Result<(), String> {
        let mut stream = TcpStream::connect(to_addr)
            .await
            .map_err(|e| format!("Failed to connect to {}: {}", to_addr, e))?;

        let data = bincode::serialize(&msg)
            .map_err(|e| format!("Failed to serialize: {}", e))?;

        let len = data.len() as u32;
        stream.write_all(&len.to_be_bytes()).await
            .map_err(|e| format!("Failed to write length: {}", e))?;

        stream.write_all(&data).await
            .map_err(|e| format!("Failed to write data: {}", e))?;

        stream.flush().await
            .map_err(|e| format!("Failed to flush: {}", e))?;

        Ok(())
    }
}
