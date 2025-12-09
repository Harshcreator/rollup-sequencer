# Two-Node Demo

This project ships with a minimal two-node demo that shows how
transactions submitted to one node are gossiped to another node and
included in committed blocks.

Under the hood, both nodes run the same binary with different
configuration derived from the `NODE_ID` environment variable.

- `NODE_ID=1` (default):
  - Gossip listen address: `127.0.0.1:9001`
  - Gossip peer(s): `127.0.0.1:9002`
  - RPC address: `127.0.0.1:8080`
  - Data directory: `./data_1`
- `NODE_ID=2`:
  - Gossip listen address: `127.0.0.1:9002`
  - Gossip peer(s): `127.0.0.1:9001`
  - RPC address: `127.0.0.1:8081`
  - Data directory: `./data_2`

## Prerequisites

- Rust toolchain installed (`cargo` on your PATH)
- This repository checked out locally

From the repo root:

```powershell
cd c:\Projects\rollup-sequencer\rollup-sequencer
cargo build
```

## Running Node 1

```powershell
# In Terminal 1
$env:NODE_ID = "1"
cargo run
```

Node 1 will:

- Listen for gossip on `127.0.0.1:9001`
- Expose its HTTP RPC API on `http://127.0.0.1:8080`
- Store data in `./data_1`

You should see logs indicating the node has started and is periodically
running consensus steps.

## Running Node 2

Open a second terminal window in the same project directory and run:

```powershell
# In Terminal 2
$env:NODE_ID = "2"
cargo run
```

Node 2 will:

- Listen for gossip on `127.0.0.1:9002`
- Expose its HTTP RPC API on `http://127.0.0.1:8081`
- Store data in `./data_2`

At this point, both nodes are connected over UDP gossip and are ready
to exchange transactions.

## Submitting Transactions

Submit transactions via the RPC `/tx` endpoint. For example, to send a
transaction to **Node 1** from a third terminal:

```powershell
$body = '{
  "namespace_id": 0,
  "gas_price": 1,
  "data": "SGVsbG8sIHNlcXVlbmNlciEgKHR4IGZyb20gTm9kZSAxKSI=")
}'

Invoke-WebRequest `
  -Uri "http://127.0.0.1:8080/tx" `
  -Method POST `
  -ContentType "application/json" `
  -Body $body
```

This issues a transaction to Node 1. Node 1:

1. Accepts the transaction via its RPC endpoint.
2. Inserts it into its local mempool.
3. Gossips it to Node 2 over UDP.

Both nodes periodically run consensus steps (every 500 ms) that seal
blocks from their local mempools. You should see logs on one or both
nodes indicating that blocks with non-zero transactions are being
committed.

> Note: The transaction payload is a base64-encoded byte string. You
> can change `data` to any valid base64 string; the sequencer treats it
> as opaque bytes.

## Observing Metrics and Health

Each node exposes a small HTTP API documented in `docs/api.md`.

- Health:
  - Node 1: `http://127.0.0.1:8080/health`
  - Node 2: `http://127.0.0.1:8081/health`
- Prometheus metrics:
  - Node 1: `http://127.0.0.1:8080/metrics`
  - Node 2: `http://127.0.0.1:8081/metrics`

In PowerShell, you can query these endpoints with:

```powershell
Invoke-WebRequest -Uri "http://127.0.0.1:8080/health"
Invoke-WebRequest -Uri "http://127.0.0.1:8080/metrics" | Select-Object -ExpandProperty Content
```

Repeat with port `8081` to inspect Node 2.

## Cleaning Up

Each node stores its data in a separate directory. To reset the demo:

```powershell
Remove-Item -Recurse -Force .\data_1, .\data_2
```

This will delete the local sled databases so that subsequent runs start
from a clean state.
