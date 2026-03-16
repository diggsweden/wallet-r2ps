use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

use super::dto::WsResponseMsg;

/// Broadcast capacity per client channel.
const BROADCAST_CAPACITY: usize = 256;

/// In-memory registry of connected WebSocket clients.
///
/// Maps `client_id` to a broadcast sender. Multiple WebSocket connections
/// for the same `client_id` share the same broadcast channel.
///
/// This is a per-pod data structure — each pod has its own registry.
/// The WS Kafka subscriber uses this to route responses to locally
/// connected clients.
#[derive(Debug, Clone)]
pub struct ClientConnectionRegistry {
    channels: Arc<DashMap<String, broadcast::Sender<WsResponseMsg>>>,
}

impl ClientConnectionRegistry {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(DashMap::new()),
        }
    }

    /// Register a client and obtain a broadcast receiver for pushed messages.
    ///
    /// If the client already has a channel (e.g., another WS connection),
    /// a new receiver is subscribed to the existing sender.
    pub fn register(&self, client_id: &str) -> broadcast::Receiver<WsResponseMsg> {
        let entry = self
            .channels
            .entry(client_id.to_string())
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
                tx
            });
        entry.value().subscribe()
    }

    /// Unregister a client. Removes the channel if no more receivers exist.
    ///
    /// Called when the last WebSocket connection for a client_id disconnects.
    pub fn unregister(&self, client_id: &str) {
        // Only remove if the sender has no active receivers
        if let Some(entry) = self.channels.get(client_id)
            && entry.receiver_count() == 0
        {
            drop(entry);
            self.channels.remove(client_id);
        }
    }

    /// Send a response message to all connected WebSocket sessions for a client_id.
    ///
    /// Returns `true` if the message was sent to at least one receiver.
    pub fn send_to_client(&self, client_id: &str, message: WsResponseMsg) -> bool {
        if let Some(sender) = self.channels.get(client_id) {
            match sender.send(message) {
                Ok(count) => count > 0,
                Err(_) => {
                    // All receivers have been dropped — clean up
                    drop(sender);
                    self.channels.remove(client_id);
                    false
                }
            }
        } else {
            false
        }
    }

    /// Check if a client has any active connections on this pod.
    pub fn is_connected(&self, client_id: &str) -> bool {
        self.channels
            .get(client_id)
            .is_some_and(|sender| sender.receiver_count() > 0)
    }
}

impl Default for ClientConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}
