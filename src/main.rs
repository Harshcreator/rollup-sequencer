use std::net::SocketAddr;
use std::sync::Arc;

use consensus::{ConsensusEngine, FinalityEvent, SingleNodeConsensus};
use mempool::SimpleMempool;
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

    let db_path = std::path::Path::new("./data");
    let storage = SledStorage::open(db_path)?;
    let mempool = SimpleMempool::default();

    let engine = SingleNodeConsensus::new(mempool, storage);
    let shared_engine = Arc::new(Mutex::new(engine));

    // Spawn RPC server.
    let rpc_state: RpcState<_> = Arc::clone(&shared_engine);
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    tokio::spawn(async move {
        if let Err(e) = run_rpc_server(rpc_state, addr).await {
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
