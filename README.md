
# Rollup Sequencer Core

A production-grade transaction ordering and finality engine for rollups, inspired by Espresso Systems.

## Quick Start
```bash
cargo build
cargo test
cargo run
```

## Documentation

The core documentation for this project lives in the `docs/` directory:

- [Architecture](docs/architecture.md)  High-level and low-level design, components, and data flow.
- [API](docs/api.md)  HTTP endpoints, request/response formats, and usage examples.
- [Design Decisions](docs/design-decisions.md)  Rationale behind major trade-offs and technology choices.
 - [Two-Node Demo](docs/demo.md)  Step-by-step guide to running and observing the gossip-based two-node setup.
