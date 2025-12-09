use std::sync::Arc;

use axum::{extract::State, response::IntoResponse, routing::get, routing::post, Json, Router};
use consensus::ConsensusEngine;
use networking::NetworkHandle;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::info;
use types::{NamespaceId, Transaction};

pub struct RpcInnerState<E> {
    pub engine: Arc<Mutex<E>>,
    pub network: Option<NetworkHandle>,
}

pub type RpcState<E> = Arc<RpcInnerState<E>>;

#[derive(Deserialize)]
pub struct SubmitTxRequest {
    pub namespace: u64,
    pub gas_price: u64,
    pub nonce: u64,
    pub payload: String,
}

#[derive(Serialize)]
pub struct SubmitTxResponse {
    pub tx_id: String,
}

#[derive(Serialize)]
pub struct TxStatusResponse {
    pub found: bool,
}

type AppState<E> = RpcState<E>;

async fn submit_tx_handler<E: ConsensusEngine + Send + Sync + 'static>(
    State(state): State<AppState<E>>,
    Json(req): Json<SubmitTxRequest>,
) -> Json<SubmitTxResponse> {
    let tx = Transaction {
        namespace: NamespaceId(req.namespace),
        gas_price: req.gas_price,
        nonce: req.nonce,
        payload: req.payload.into_bytes(),
        signature: vec![],
    };

    let tx_clone = tx.clone();
    let mut engine = state.engine.lock().await;
    let tx_id = engine
        .submit_tx(tx)
        .expect("submit_tx should not fail in RPC handler");
    drop(engine);

    if let Some(net) = &state.network {
        // Fire-and-forget gossip; if the channel is full, we just drop.
        net.broadcast_tx(tx_clone).await;
    }

    Json(SubmitTxResponse {
        tx_id: hex::encode(tx_id.0 .0),
    })
}

async fn health_handler() -> &'static str {
    "ok"
}

async fn metrics_handler() -> impl IntoResponse {
    let body = metrics::render_metrics();
    ([("Content-Type", "text/plain; version=0.0.4")], body)
}

pub fn router<E>(state: RpcState<E>) -> Router
where
    E: ConsensusEngine + Send + Sync + 'static,
{
    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/tx", post(submit_tx_handler::<E>))
        .with_state(state)
}

/// Helper to spawn the Axum server on the given address.
pub async fn run_rpc_server<E>(
    state: RpcState<E>,
    addr: std::net::SocketAddr,
) -> Result<(), std::convert::Infallible>
where
    E: ConsensusEngine + Send + Sync + 'static,
{
    let app = router(state);
    info!(%addr, "starting RPC server");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind RPC listener");
    axum::serve(listener, app).await.expect("RPC server failed");
    Ok(())
}

#[cfg(test)]
mod tests {
    // RPC tests can be added later when we wire a test engine.
}
