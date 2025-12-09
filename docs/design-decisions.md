# Design Decisions

This document records key design choices made in this implementation of a rollup sequencer and the rationale behind them.

## Language and Ecosystem

- **Rust** was chosen for its strong safety guarantees, performance, and ecosystem maturity around async IO (Tokio), networking, and observability.
- The workspace is split into multiple small crates (`types`, `mempool`, `storage`, `consensus`, `rpc`, `metrics`, `networking`) to enforce clear boundaries and keep compile times manageable.

## Storage Backend: sled

- **Choice**: sled was selected as the initial embedded database:
	- Simple API, good for prototyping.
	- No external service to run; everything is embedded.
- **Abstraction**: storage is accessed only through the traits `BlockStore`, `TxStore`, and `StateStore`.
	- This allows swapping sled for another backend (e.g., RocksDB) without touching consensus or RPC logic.
- **Data model**:
	- Blocks keyed by `BlockId` and by height.
	- Transactions keyed by `TxId`.
	- State roots keyed by height.

## Consensus: Single-Node First

- **Goal**: provide a clear, testable baseline rather than an incomplete, complex BFT algorithm.
- **Current state**:
	- Single-node, step-based consensus (`SingleNodeConsensus`).
	- Safety invariants (no forks at the same height, monotonic heights/views) tested and documented.
- **Future work**:
	- Extend into a leader-based multi-node protocol that wraps this engine.
	- Introduce explicit votes, quorum certificates, and a proper view-change mechanism.

## Networking: UDP Gossip Instead of libp2p

- **Choice**: a minimal UDP-based gossip protocol for transactions and blocks, instead of immediately adopting libp2p.
	- Keeps the implementation small and understandable for a CV/demo.
	- Avoids the complexity of negotiation, multiplexing, and NAT traversal while still demonstrating multi-node behavior.
- **Envelope**:
	- JSON-encoded `GossipMessage::{Tx, Block}`.
	- Easy to inspect on the wire and debug.

## Observability: metrics + tracing

- **Metrics**:
	- The `metrics` crate and `metrics-exporter-prometheus` provide a lightweight, in-process metrics pipeline.
	- Key counters, gauges, and histograms are exposed via a `/metrics` endpoint for Prometheus.
	- Storage and consensus are instrumented to understand performance characteristics.
- **Tracing**:
	- `tracing` spans wrap consensus `step()` and RPC handlers to provide structured logs and execution traces.

## Error Handling and RPC Shape

- **Error shape**:
	- RPC endpoints return JSON error objects rather than panicking on internal failures.
	- This is a step towards more robust, client-friendly APIs.
- **Validation**:
	- The current system performs only basic shape validation via Serde.
	- Future work: enforce rate-limits and domain-specific invariant checks (e.g., nonce monotonicity, maximum payload size).

## Testing Strategy

- **Unit tests**:
	- Cover hashing, transaction/block IDs, Merkle proofs, mempool ordering, storage behavior, and consensus invariants.
- **Property tests (proptest)**:
	- Merkle proofs validated over random transaction sets (`types` crate).
	- In-memory storage roundtrip for transactions over random nonce sets (`storage` crate).
- **Integration demo**:
	- Two-node UDP gossip + RPC setup serves as a practical system-level test.
	- L1 settlement integration tests build `L1BatchCommitment` values
	  from real `FinalityEvent::BlockCommitted` blocks and feed the
	  resulting commitment hashes into a mock L1 sink, demonstrating how
	  the sequencer would connect to an on-chain settlement contract.
