//! Sequencer metrics and Prometheus exporter wiring.

use metrics::{counter, gauge};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use once_cell::sync::OnceCell;

static PROM_HANDLE: OnceCell<PrometheusHandle> = OnceCell::new();

/// Install the global metrics recorder.
///
/// Call this once at startup before recording metrics.
pub fn init_metrics() -> Result<(), Box<dyn std::error::Error>> {
	let builder = PrometheusBuilder::new();
	let handle = builder.install_recorder()?;
	PROM_HANDLE
		.set(handle)
		.map_err(|_| "prometheus handle already initialized".to_string())?;
	Ok(())
}

/// Render all metrics in Prometheus text format.
pub fn render_metrics() -> String {
	PROM_HANDLE
		.get()
		.map(|h| h.render())
		.unwrap_or_else(|| "".to_string())
}

/// Record that a transaction was submitted into the mempool.
pub fn record_tx_submitted() {
	counter!("sequencer_tx_submitted").increment(1);
}

/// Update the mempool size gauge.
pub fn record_mempool_size(len: usize) {
	gauge!("sequencer_mempool_size").set(len as f64);
}

/// Record that a block was committed, along with its transaction count.
pub fn record_block_committed(tx_count: usize) {
	counter!("sequencer_blocks_committed").increment(1);
	counter!("sequencer_txs_committed").increment(tx_count as u64);
}
