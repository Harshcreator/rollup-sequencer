//! Minimal P2P-style gossip for transactions and blocks.
//!
//! This is **not** a full libp2p implementation, but it provides a
//! simple UDP-based gossip channel that allows two (or more) nodes to
//! exchange transactions and committed blocks.

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use types::{Block, Transaction};

/// Messages exchanged between peers.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum GossipMessage {
	Tx(Transaction),
	Block(Block),
}

/// Simple networking configuration for a node.
#[derive(Clone, Debug)]
pub struct NetworkConfig {
	pub listen_addr: SocketAddr,
	pub peers: Vec<SocketAddr>,
}

/// Handle for sending gossip messages to peers.
#[derive(Clone)]
pub struct NetworkHandle {
	tx: mpsc::Sender<GossipMessage>,
}

impl NetworkHandle {
	pub async fn broadcast_tx(&self, tx_obj: Transaction) {
		let _ = self.tx.send(GossipMessage::Tx(tx_obj)).await;
	}

	pub async fn broadcast_block(&self, block: Block) {
		let _ = self.tx.send(GossipMessage::Block(block)).await;
	}
}

/// Start a UDP gossip loop.
///
/// - Binds to `config.listen_addr`.
/// - Broadcasts any outgoing messages to all configured peers.
/// - For every incoming message, calls `on_message`.
pub async fn start_network<F>(
	config: NetworkConfig,
	on_message: F,
) -> NetworkHandle
where
	F: Fn(GossipMessage) + Send + Sync + 'static,
{
	let socket = UdpSocket::bind(config.listen_addr)
		.await
		.expect("failed to bind UDP gossip socket");
	let (tx, mut rx) = mpsc::channel::<GossipMessage>(1024);
 
	let socket = std::sync::Arc::new(socket);
	let on_message = std::sync::Arc::new(on_message);
	let recv_socket = std::sync::Arc::clone(&socket);
	let peers = config.peers.clone();

	// Receiver loop.
	tokio::spawn(async move {
		let mut buf = vec![0u8; 64 * 1024];
		loop {
			match recv_socket.recv_from(&mut buf).await {
				Ok((len, _addr)) => {
					if let Ok(msg) = serde_json::from_slice::<GossipMessage>(&buf[..len]) {
						let handler = on_message.clone();
						tokio::spawn(async move { handler(msg) });
					}
				}
				Err(_e) => {
					// Back off briefly on error.
					sleep(Duration::from_millis(100)).await;
				}
			}
		}
	});

	// Sender loop.
	let send_socket = socket;
	tokio::spawn(async move {
		while let Some(msg) = rx.recv().await {
			if let Ok(bytes) = serde_json::to_vec(&msg) {
				for peer in &peers {
					let _ = send_socket.send_to(&bytes, peer).await;
				}
			}
		}
	});

	NetworkHandle { tx }
}
