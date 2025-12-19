//! Notification broadcasting for MCP servers
//!
//! This module provides a broadcast channel for file change events
//! that can be shared between file watchers and multiple MCP server instances.

use std::path::PathBuf;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub enum FileChangeEvent {
    FileReindexed { path: PathBuf },
    FileCreated { path: PathBuf },
    FileDeleted { path: PathBuf },
    IndexReloaded, // Entire index was reloaded from disk
}

/// Manages notification broadcasting to multiple MCP server instances
#[derive(Clone)]
pub struct NotificationBroadcaster {
    sender: broadcast::Sender<FileChangeEvent>,
}

impl NotificationBroadcaster {
    /// Create a new broadcaster with specified channel capacity
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Send a file change event to all subscribers
    pub fn send(&self, event: FileChangeEvent) {
        match self.sender.send(event.clone()) {
            Ok(count) => {
                crate::debug_event!("broadcast", "sent", "{event:?} to {count} subscribers");
            }
            Err(_) => {
                // No receivers, this is fine
                crate::debug_event!("broadcast", "dropped", "no subscribers for {event:?}");
            }
        }
    }

    /// Subscribe to receive notifications
    pub fn subscribe(&self) -> broadcast::Receiver<FileChangeEvent> {
        self.sender.subscribe()
    }
}

/// Extension trait for MCP server to handle notifications
impl super::CodeIntelligenceServer {
    /// Start listening for broadcast notifications and forward them via MCP
    pub async fn start_notification_listener(
        &self,
        mut receiver: broadcast::Receiver<FileChangeEvent>,
    ) {
        use rmcp::model::{
            LoggingLevel, LoggingMessageNotificationParam, ResourceUpdatedNotificationParam,
        };

        crate::debug_event!("mcp-notify", "listening");

        loop {
            match receiver.recv().await {
                Ok(event) => {
                    crate::debug_event!("mcp-notify", "received", "{event:?}");

                    let peer_guard = self.peer.lock().await;
                    if let Some(peer) = peer_guard.as_ref() {
                        match event {
                            FileChangeEvent::FileReindexed { path } => {
                                let path_str = path.display().to_string();

                                // Send resource updated notification
                                let _ = peer
                                    .notify_resource_updated(ResourceUpdatedNotificationParam {
                                        uri: format!("file://{path_str}"),
                                    })
                                    .await;

                                // Send logging message
                                let _ = peer
                                    .notify_logging_message(LoggingMessageNotificationParam {
                                        level: LoggingLevel::Info,
                                        logger: Some("codanna".to_string()),
                                        data: serde_json::json!({
                                            "action": "re-indexed",
                                            "file": path_str
                                        }),
                                    })
                                    .await;

                                crate::debug_event!(
                                    "mcp-notify",
                                    "sent",
                                    "FileReindexed {path_str}"
                                );
                            }
                            FileChangeEvent::FileCreated { path } => {
                                let _ = peer.notify_resource_list_changed().await;
                                crate::debug_event!(
                                    "mcp-notify",
                                    "sent",
                                    "FileCreated {}",
                                    path.display()
                                );
                            }
                            FileChangeEvent::FileDeleted { path } => {
                                let _ = peer.notify_resource_list_changed().await;
                                crate::debug_event!(
                                    "mcp-notify",
                                    "sent",
                                    "FileDeleted {}",
                                    path.display()
                                );
                            }
                            FileChangeEvent::IndexReloaded => {
                                let _ = peer.notify_resource_list_changed().await;
                                crate::debug_event!("mcp-notify", "sent", "IndexReloaded");
                            }
                        }
                    } else {
                        crate::debug_event!("mcp-notify", "dropped", "no peer");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("[mcp-notify] lagged by {n} messages");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    crate::debug_event!("mcp-notify", "channel closed");
                    break;
                }
            }
        }
    }
}
