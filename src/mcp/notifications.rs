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

/// The resource URI for a change-event path: an emitted relative path
/// on the MCP wire, portable-form per the emission contract. Clients
/// subscribe by URI and rmcp filters by exact membership — the URI
/// must byte-match the subscription on every platform. Non-Normal
/// path shapes fall back to display text.
pub fn resource_uri(path: &std::path::Path) -> String {
    let portable = crate::parsing::paths::portable_join(path).unwrap_or_else(|| {
        crate::parsing::paths::render_absolute_path(path)
            .display()
            .to_string()
    });
    format!("file://{portable}")
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
        // Logging types are deprecated by SEP-2577; keep emitting them for client
        // compatibility until rmcp removes the API.
        #[allow(deprecated)]
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
                                // Portable-form: the wire path and URI must
                                // byte-match subscriptions on every platform
                                let path_str = crate::parsing::paths::portable_join(&path)
                                    .unwrap_or_else(|| {
                                        crate::parsing::paths::render_absolute_path(&path)
                                            .display()
                                            .to_string()
                                    });

                                // Send standard MCP resource updated notification (backwards compatible)
                                let _ = peer
                                    .notify_resource_updated(ResourceUpdatedNotificationParam::new(
                                        format!("file://{path_str}"),
                                    ))
                                    .await;

                                // Send logging message (backwards compatible)
                                #[allow(deprecated)]
                                let _ = peer
                                    .notify_logging_message(
                                        LoggingMessageNotificationParam::new(
                                            LoggingLevel::Info,
                                            serde_json::json!({
                                                "action": "re-indexed",
                                                "file": path_str
                                            }),
                                        )
                                        .with_logger("codanna"),
                                    )
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
                                    crate::parsing::paths::render_absolute_path(&path).display()
                                );
                            }
                            FileChangeEvent::FileDeleted { path } => {
                                let _ = peer.notify_resource_list_changed().await;

                                crate::debug_event!(
                                    "mcp-notify",
                                    "sent",
                                    "FileDeleted {}",
                                    crate::parsing::paths::render_absolute_path(&path).display()
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

#[cfg(test)]
mod tests {
    use super::*;

    // The notification URI is an emitted relative path on the MCP wire:
    // portable-form on every platform. rmcp filters resource-updated
    // notifications by exact URI membership, so a native-separator URI
    // never matches the client's subscription.
    #[test]
    fn resource_uri_is_portable_form_on_every_platform() {
        let path = std::path::Path::new("src").join("alpha.rs");
        assert_eq!(resource_uri(&path), "file://src/alpha.rs");
    }
}
