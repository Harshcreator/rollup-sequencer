# API Documentation

This document describes the external HTTP APIs exposed by the sequencer node. All endpoints are served by the `rpc` crate using Axum, and are available on the node's configured RPC address (e.g. `127.0.0.1:8080` for Node 1, `127.0.0.1:8081` for Node 2).

## Conventions

- Base URL: `http://<host>:<port>` (default dev values shown below).
- Request/response bodies are JSON unless otherwise stated.
- Error responses use the following shape:

```json
{
	"error": "human-readable message"
}
```

---

## Health

### `GET /health`

Simple liveness check.

- **Request**: no body.
- **Responses**:
	- `200 OK` with plain text body:

		```text
		ok
		```

---

## Transactions

### `POST /tx`

Submit a transaction to the sequencer. The transaction is inserted into the local mempool and gossiped to peers.

- **Request**: `application/json`

	```json
	{
		"namespace": 1,
		"gas_price": 10,
		"nonce": 1,
		"payload": "base64-or-utf8-string"
	}
	```

	- `namespace` (`u64`): logical rollup / namespace identifier.
	- `gas_price` (`u64`): relative priority indicator; higher values are scheduled first.
	- `nonce` (`u64`): monotonically increasing per namespace/sender in typical deployments.
	- `payload` (`string`): opaque transaction payload; interpreted by the rollup execution layer.

- **Successful response**: `200 OK`, JSON

	```json
	{
		"tx_id": "<64-hex-char transaction id>"
	}
	```

- **Error responses**:
	- `500 Internal Server Error`:

		```json
		{
			"error": "submit_tx failed: <details>"
		}
		```

		This indicates an internal failure (e.g. mempool capacity issues). Future extensions can refine this into validation errors (400) vs. internal errors (500).

**Side effects**:

- Increments `sequencer_tx_submitted`.
- Updates `sequencer_mempool_size`.
- Sends a `GossipMessage::Tx` over UDP to configured peers.

---

## Metrics

### `GET /metrics`

Expose internal metrics in Prometheus text format.

- **Request**: no body.
- **Response**: `200 OK`, `text/plain; version=0.0.4` body containing metrics, for example:

	```text
	# TYPE sequencer_tx_submitted counter
	sequencer_tx_submitted 42

	# TYPE sequencer_mempool_size gauge
	sequencer_mempool_size 3

	# TYPE sequencer_blocks_committed counter
	sequencer_blocks_committed 10

	# TYPE sequencer_txs_committed counter
	sequencer_txs_committed 100

	# TYPE sequencer_consensus_step_ms histogram
	# TYPE sequencer_storage_op_ms histogram
	```

These metrics are intended to be scraped by Prometheus and visualized via Grafana.

---

## Future APIs

The current API surface is intentionally minimal. Planned additions include:

- Block and transaction query endpoints (by height/ID).
- WebSocket or SSE endpoints for streaming new blocks and transactions.
- Admin endpoints for node status and configuration.
