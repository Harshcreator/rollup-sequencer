use consensus::{build_l1_batch_commitment, ConsensusEngine, FinalityEvent, SingleNodeConsensus};
use mempool::SimpleMempool;
use storage::{InMemoryStorage, TxStore};
use types::{NamespaceId, Transaction};

fn make_tx(nonce: u64) -> Transaction {
    Transaction {
        namespace: NamespaceId(1),
        gas_price: 1,
        nonce,
        payload: vec![],
        signature: vec![],
    }
}

/// This integration test showcases how a component can subscribe to
/// finality events from the consensus engine, build an L1 batch
/// commitment from the committed blocks, and "post" it to a mock L1
/// sink. In a real deployment the sink would be an on-chain
/// settlement contract.
#[test]
fn l1_batch_can_be_built_from_finality_stream() {
    let mempool = SimpleMempool::default();
    let storage = InMemoryStorage::default();
    let mut engine = SingleNodeConsensus::new(mempool, storage);

    // Seed enough transactions so we are guaranteed at least one
    // committed block when we drive the engine.
    for i in 0..10 {
        let tx = make_tx(i);
        let _tx_id = engine.submit_tx(tx).expect("submit_tx should succeed");
    }

    // Drive the engine for a few steps and collect committed blocks.
    let mut committed_blocks = Vec::new();
    for _ in 0..5 {
        if let Some(FinalityEvent::BlockCommitted { block, .. }) = engine.step().unwrap() {
            committed_blocks.push(block);
        }
    }

    assert!(!committed_blocks.is_empty(), "expected at least one committed block");

    // Build a batch commitment as would be posted to L1.
    let batch = build_l1_batch_commitment(42, &committed_blocks);

    // Mock L1 sink that stores commitment hashes.
    let mut mock_l1_contract: Vec<types::Hash> = Vec::new();
    mock_l1_contract.push(batch.hash());

    assert_eq!(mock_l1_contract.len(), 1);
}
