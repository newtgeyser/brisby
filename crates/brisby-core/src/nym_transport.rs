//! Real Nym mixnet transport implementation
//!
//! This module is only compiled when the "nym" feature is enabled.

#![cfg(feature = "nym")]

use crate::transport::{NymAddress, ReceivedMessage, SenderTag, Transport, TransportConfig};
use crate::{Error, Result};
use nym_sdk::mixnet::{self, IncludedSurbs, MixnetClient, MixnetMessageSender, ReconstructedMessage};
use std::convert::TryInto;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Size of an AnonymousSenderTag in bytes
const SENDER_TAG_SIZE: usize = 16;

/// Real Nym mixnet transport
pub struct NymTransport {
    config: TransportConfig,
    client: Option<Arc<Mutex<MixnetClient>>>,
    address: Option<NymAddress>,
}

impl NymTransport {
    /// Create a new Nym transport with the given configuration
    pub fn new(config: TransportConfig) -> Self {
        Self {
            config,
            client: None,
            address: None,
        }
    }

    /// Create a new Nym transport with default configuration
    pub fn with_defaults() -> Self {
        Self::new(TransportConfig::default())
    }

    /// Create a new Nym transport with persistent storage
    pub fn with_storage(path: PathBuf) -> Self {
        Self::new(TransportConfig {
            storage_path: Some(path),
            ..Default::default()
        })
    }

    fn convert_message(msg: ReconstructedMessage) -> ReceivedMessage {
        let sender_tag = msg.sender_tag.map(|tag| {
            // Convert Nym's AnonymousSenderTag to our SenderTag
            SenderTag::new(tag.to_bytes().to_vec())
        });
        ReceivedMessage::new(msg.message, sender_tag)
    }
}

impl Transport for NymTransport {
    async fn connect(&mut self) -> Result<()> {
        let client = if let Some(ref storage_path) = self.config.storage_path {
            // Use persistent storage
            let storage_paths = mixnet::StoragePaths::new_from_dir(storage_path)
                .map_err(|e| Error::ConnectionFailed(e.to_string()))?;

            mixnet::MixnetClientBuilder::new_with_default_storage(storage_paths)
                .await
                .map_err(|e| Error::ConnectionFailed(e.to_string()))?
                .build()
                .map_err(|e| Error::ConnectionFailed(e.to_string()))?
                .connect_to_mixnet()
                .await
                .map_err(|e| Error::ConnectionFailed(e.to_string()))?
        } else {
            // Ephemeral session
            mixnet::MixnetClient::connect_new()
                .await
                .map_err(|e| Error::ConnectionFailed(e.to_string()))?
        };

        let addr = client.nym_address();
        self.address = Some(NymAddress::new(addr.to_string()));
        self.client = Some(Arc::new(Mutex::new(client)));

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(client) = self.client.take() {
            let client = Arc::try_unwrap(client)
                .map_err(|_| Error::Transport("client still in use".to_string()))?
                .into_inner();
            // disconnect() returns () in this SDK version
            client.disconnect().await;
        }
        self.address = None;
        Ok(())
    }

    fn our_address(&self) -> Option<&NymAddress> {
        self.address.as_ref()
    }

    fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    async fn send(&self, recipient: &NymAddress, data: Vec<u8>) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| Error::SendFailed("not connected".to_string()))?;

        let recipient_addr: mixnet::Recipient = recipient
            .as_str()
            .parse()
            .map_err(|e: mixnet::RecipientFormattingError| Error::InvalidAddress(e.to_string()))?;

        // Always include at least one SURB so the receiver can reply
        let surbs = IncludedSurbs::new(self.config.surbs_per_message.max(1));

        client
            .lock()
            .await
            .send_message(recipient_addr, data, surbs)
            .await
            .map_err(|e| Error::SendFailed(e.to_string()))?;

        Ok(())
    }

    async fn send_reply(&self, sender_tag: &SenderTag, data: Vec<u8>) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| Error::SendFailed("not connected".to_string()))?;

        // Convert our SenderTag back to Nym's AnonymousSenderTag
        let tag_bytes: [u8; SENDER_TAG_SIZE] = sender_tag
            .as_bytes()
            .try_into()
            .map_err(|_| Error::SendFailed("invalid sender tag size".to_string()))?;
        let anon_tag = mixnet::AnonymousSenderTag::from_bytes(tag_bytes);

        client
            .lock()
            .await
            .send_reply(anon_tag, data)
            .await
            .map_err(|e| Error::SendFailed(e.to_string()))?;

        Ok(())
    }

    async fn receive(&self) -> Result<ReceivedMessage> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| Error::ReceiveFailed("not connected".to_string()))?;

        loop {
            // wait_for_messages() returns Option<Vec<ReconstructedMessage>>
            if let Some(mut messages) = client.lock().await.wait_for_messages().await {
                if let Some(msg) = messages.pop() {
                    return Ok(Self::convert_message(msg));
                }
            }
            // Brief sleep to avoid busy-waiting
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    async fn receive_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<ReceivedMessage>> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| Error::ReceiveFailed("not connected".to_string()))?;

        // Use tokio timeout
        match tokio::time::timeout(timeout, async {
            loop {
                if let Some(mut messages) = client.lock().await.wait_for_messages().await {
                    if let Some(msg) = messages.pop() {
                        return Ok::<_, Error>(Some(Self::convert_message(msg)));
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        {
            Ok(result) => result,
            Err(_) => Ok(None), // Timeout
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests require a running Nym network, so they're integration tests
    // See tests/nym_integration.rs
}
