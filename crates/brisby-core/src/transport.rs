//! Transport abstraction for Nym mixnet communication
//!
//! This module provides a trait-based abstraction over the Nym mixnet,
//! allowing for mock implementations during testing and the real Nym
//! client in production.

use crate::{Error, Result};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

/// A Nym network address
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct NymAddress(String);

impl NymAddress {
    /// Create a new NymAddress from a string
    pub fn new(address: impl Into<String>) -> Self {
        Self(address.into())
    }

    /// Get the address as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NymAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for NymAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NymAddress({})", self.0)
    }
}

impl FromStr for NymAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        // Basic validation - Nym addresses are base58 encoded
        if s.is_empty() {
            return Err(Error::InvalidAddress("empty address".to_string()));
        }
        Ok(Self(s.to_string()))
    }
}

impl From<String> for NymAddress {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for NymAddress {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// An anonymous sender tag for replying without knowing the sender's address
///
/// When a message is received with SURBs (Single Use Reply Blocks), the sender
/// tag can be used to send a reply without knowing the sender's actual address.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SenderTag(Vec<u8>);

impl SenderTag {
    /// Create a new SenderTag from bytes
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Get the tag as bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convert to owned bytes
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl fmt::Debug for SenderTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SenderTag({} bytes)", self.0.len())
    }
}

/// A message received from the mixnet
#[derive(Clone, Debug)]
pub struct ReceivedMessage {
    /// The message payload
    pub data: Vec<u8>,
    /// Optional sender tag for anonymous replies
    pub sender_tag: Option<SenderTag>,
}

impl ReceivedMessage {
    /// Create a new received message
    pub fn new(data: Vec<u8>, sender_tag: Option<SenderTag>) -> Self {
        Self { data, sender_tag }
    }
}

/// Configuration for the transport layer
#[derive(Clone, Debug)]
pub struct TransportConfig {
    /// Path for persistent storage (None for ephemeral)
    pub storage_path: Option<std::path::PathBuf>,
    /// Number of SURBs to include with outgoing messages
    pub surbs_per_message: u32,
    /// Whether to use testnet instead of mainnet
    pub use_testnet: bool,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            storage_path: None,
            surbs_per_message: 5,
            use_testnet: false,
        }
    }
}

/// Transport trait for mixnet communication
///
/// This trait abstracts over the Nym mixnet client, allowing for:
/// - Real Nym integration in production
/// - Mock implementations for testing
/// - Future alternative transports
#[allow(async_fn_in_trait)]
pub trait Transport: Send + Sync {
    /// Connect to the mixnet
    async fn connect(&mut self) -> Result<()>;

    /// Disconnect from the mixnet
    async fn disconnect(&mut self) -> Result<()>;

    /// Get our own address on the network
    fn our_address(&self) -> Option<&NymAddress>;

    /// Check if we're connected
    fn is_connected(&self) -> bool;

    /// Send a message to a specific address
    async fn send(&self, recipient: &NymAddress, data: Vec<u8>) -> Result<()>;

    /// Send an anonymous reply using a sender tag
    async fn send_reply(&self, sender_tag: &SenderTag, data: Vec<u8>) -> Result<()>;

    /// Receive the next message (blocking)
    async fn receive(&self) -> Result<ReceivedMessage>;

    /// Try to receive a message with a timeout
    async fn receive_timeout(&self, timeout: std::time::Duration) -> Result<Option<ReceivedMessage>>;
}

/// A shareable transport handle
pub type TransportHandle = Arc<dyn Transport>;

pub mod mock {
    //! Mock transport for testing and development

    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// A mock transport for testing
    pub struct MockTransport {
        address: Option<NymAddress>,
        connected: bool,
        /// Messages to deliver on receive()
        incoming: Mutex<VecDeque<ReceivedMessage>>,
        /// Messages that were sent
        outgoing: Mutex<Vec<(NymAddress, Vec<u8>)>>,
        /// Replies that were sent
        replies: Mutex<Vec<(SenderTag, Vec<u8>)>>,
    }

    impl MockTransport {
        /// Create a new mock transport
        pub fn new() -> Self {
            Self {
                address: None,
                connected: false,
                incoming: Mutex::new(VecDeque::new()),
                outgoing: Mutex::new(Vec::new()),
                replies: Mutex::new(Vec::new()),
            }
        }

        /// Queue a message to be received
        pub fn queue_message(&self, msg: ReceivedMessage) {
            self.incoming.lock().unwrap().push_back(msg);
        }

        /// Get all sent messages
        pub fn get_sent_messages(&self) -> Vec<(NymAddress, Vec<u8>)> {
            self.outgoing.lock().unwrap().clone()
        }

        /// Get all sent replies
        pub fn get_sent_replies(&self) -> Vec<(SenderTag, Vec<u8>)> {
            self.replies.lock().unwrap().clone()
        }
    }

    impl Default for MockTransport {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Transport for MockTransport {
        async fn connect(&mut self) -> Result<()> {
            self.address = Some(NymAddress::new("mock-address-12345.mock"));
            self.connected = true;
            Ok(())
        }

        async fn disconnect(&mut self) -> Result<()> {
            self.connected = false;
            Ok(())
        }

        fn our_address(&self) -> Option<&NymAddress> {
            self.address.as_ref()
        }

        fn is_connected(&self) -> bool {
            self.connected
        }

        async fn send(&self, recipient: &NymAddress, data: Vec<u8>) -> Result<()> {
            if !self.connected {
                return Err(Error::SendFailed("not connected".to_string()));
            }
            self.outgoing.lock().unwrap().push((recipient.clone(), data));
            Ok(())
        }

        async fn send_reply(&self, sender_tag: &SenderTag, data: Vec<u8>) -> Result<()> {
            if !self.connected {
                return Err(Error::SendFailed("not connected".to_string()));
            }
            self.replies.lock().unwrap().push((sender_tag.clone(), data));
            Ok(())
        }

        async fn receive(&self) -> Result<ReceivedMessage> {
            loop {
                if let Some(msg) = self.incoming.lock().unwrap().pop_front() {
                    return Ok(msg);
                }
                // In a real implementation, this would block
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }

        async fn receive_timeout(
            &self,
            timeout: std::time::Duration,
        ) -> Result<Option<ReceivedMessage>> {
            let start = std::time::Instant::now();
            while start.elapsed() < timeout {
                if let Some(msg) = self.incoming.lock().unwrap().pop_front() {
                    return Ok(Some(msg));
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Ok(None)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn test_mock_transport_connect() {
            let mut transport = MockTransport::new();
            assert!(!transport.is_connected());
            assert!(transport.our_address().is_none());

            transport.connect().await.unwrap();
            assert!(transport.is_connected());
            assert!(transport.our_address().is_some());
        }

        #[tokio::test]
        async fn test_mock_transport_send_receive() {
            let mut transport = MockTransport::new();
            transport.connect().await.unwrap();

            // Queue a message to receive
            let msg = ReceivedMessage::new(b"hello".to_vec(), None);
            transport.queue_message(msg);

            // Send a message
            let recipient = NymAddress::new("recipient-address");
            transport.send(&recipient, b"world".to_vec()).await.unwrap();

            // Verify sent message
            let sent = transport.get_sent_messages();
            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].1, b"world");

            // Receive the queued message
            let received = transport
                .receive_timeout(std::time::Duration::from_millis(100))
                .await
                .unwrap();
            assert!(received.is_some());
            assert_eq!(received.unwrap().data, b"hello");
        }

        #[tokio::test]
        async fn test_mock_transport_reply() {
            let mut transport = MockTransport::new();
            transport.connect().await.unwrap();

            let tag = SenderTag::new(vec![1, 2, 3, 4]);
            transport.send_reply(&tag, b"reply data".to_vec()).await.unwrap();

            let replies = transport.get_sent_replies();
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0].1, b"reply data");
        }
    }
}
