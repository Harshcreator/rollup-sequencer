use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use consensus::{ConsensusEngine, FinalityEvent, SingleNodeConsensus};
use mempool::SimpleMempool;
use metrics as sequencer_metrics;
use networking::{start_network, GossipMessage, NetworkConfig};
use rpc::{run_rpc_server, RpcState};
use storage::SledStorage;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{info, Level};
// No direct use of types here; RPC constructs transactions.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    // Install global metrics recorder; metrics are exposed via the RPC server.
    sequencer_metrics::init_metrics()?;

    // Very simple two-node demo configuration based on NODE_ID env var.
    let node_id = env::var("NODE_ID").unwrap_or_else(|_| "1".to_string());
    let (listen_addr, peers, rpc_addr): (SocketAddr, Vec<SocketAddr>, SocketAddr) =
        if node_id == "1" {
            (
                "127.0.0.1:9001".parse().unwrap(),
                vec!["127.0.0.1:9002".parse().unwrap()],
                "127.0.0.1:8080".parse().unwrap(),
            )
        } else {
            (
                "127.0.0.1:9002".parse().unwrap(),
                vec!["127.0.0.1:9001".parse().unwrap()],
                "127.0.0.1:8081".parse().unwrap(),
            )
        };

    // Use a per-node sled database directory to avoid file locks when
    // running multiple nodes on the same machine.
    let data_dir = format!("./data_{}", node_id);
    let storage = SledStorage::open(std::path::Path::new(&data_dir))?;
    let mempool = SimpleMempool::default();

    let engine = SingleNodeConsensus::new(mempool, storage);
    let shared_engine = Arc::new(Mutex::new(engine));

    // Start networking: gossip transactions into the local mempool and
    // committed blocks into local storage via the consensus engine.
    let net_engine = Arc::clone(&shared_engine);
    let net_config = NetworkConfig { listen_addr, peers };
    let net_handle = start_network(net_config, move |msg| {
        let net_engine = Arc::clone(&net_engine);
        match msg {
            GossipMessage::Tx(tx) => {
                // Best-effort: insert into mempool via consensus engine.
                info!("received gossiped tx; inserting into local mempool");
                tokio::spawn(async move {
                    let mut guard = net_engine.lock().await;
                    let _ = guard.submit_tx(tx);
                });
            }
            GossipMessage::Block(_block) => {
                // In a fuller implementation, we would verify and import
                // the block. For now, we log receipt only.
                tracing::info!("received gossiped block (ignored in demo)");
            }
        }
    })
    .await;

    // Spawn RPC server, giving it access to both the engine and network
    // so it can gossip submitted transactions.
    let rpc_state: RpcState<_> = Arc::new(rpc::RpcInnerState {
        engine: Arc::clone(&shared_engine),
        network: Some(net_handle),
    });
    tokio::spawn(async move {
        if let Err(e) = run_rpc_server(rpc_state, rpc_addr).await {
            eprintln!("RPC server error: {e}");
        }
    });

    // Simple consensus loop that periodically seals blocks from the mempool.
    loop {
        {
            let mut engine_guard = shared_engine.lock().await;
            if let Some(FinalityEvent::BlockCommitted { block, .. }) = engine_guard.step()? {
                info!(
                    height = block.header.height,
                    tx_count = block.txs.len(),
                    "committed block"
                );
            }
        }

        sleep(Duration::from_millis(500)).await;
    }
}
