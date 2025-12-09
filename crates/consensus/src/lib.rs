use std::time::{Instant, SystemTime, UNIX_EPOCH};

use mempool::{Mempool, SimpleMempool};
use storage::{BlockStore, InMemoryStorage, StateStore, TxStore};
use thiserror::Error;
use types::{merkle_root, Block, BlockHeader, BlockId, Hash, L1BatchCommitment, Transaction, TxId};

use metrics as sequencer_metrics;
use tracing::instrument;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ViewNumber(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ValidatorId(pub [u8; 32]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuorumCertificate {
    pub view: ViewNumber,
    pub block_id: BlockId,
}

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("mempool error: {0}")]
    Mempool(String),
    #[error("storage error: {0}")]
    Storage(String),
}

impl From<storage::StorageError> for ConsensusError {
    fn from(e: storage::StorageError) -> Self {
        Self::Storage(e.to_string())
    }
}

/// Events emitted by the consensus engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FinalityEvent {
    BlockCommitted { block: Block, qc: QuorumCertificate },
}

/// Basic consensus engine interface for a single-node, step-driven engine.
pub trait ConsensusEngine {
    fn submit_tx(&mut self, tx: Transaction) -> Result<TxId, ConsensusError>;
    fn step(&mut self) -> Result<Option<FinalityEvent>, ConsensusError>;
}

/// Build an L1 batch commitment for a set of committed L2 blocks.
///
/// In a real deployment, a component subscribing to `FinalityEvent`s
/// would gather blocks into batches and call this function to obtain
/// a value that is then posted to an L1 settlement contract.
pub fn build_l1_batch_commitment(batch_number: u64, blocks: &[Block]) -> L1BatchCommitment {
    let block_ids = blocks.iter().map(|b| b.header.id()).collect();
    L1BatchCommitment {
        batch_number,
        block_ids,
    }
}

/// A single-node consensus engine that periodically pulls transactions from
/// the mempool, builds blocks, and commits them to storage. QCs are
/// synthetic: the single validator implicitly forms a quorum.
pub struct SingleNodeConsensus<M, S>
where
    M: Mempool,
    S: BlockStore + StateStore + TxStore,
{
    view: ViewNumber,
    validator: ValidatorId,
    mempool: M,
    storage: S,
    last_block_id: Option<BlockId>,
    last_height: u64,
}

impl Default for SingleNodeConsensus<SimpleMempool, InMemoryStorage> {
    fn default() -> Self {
        Self::new(SimpleMempool::default(), InMemoryStorage::default())
    }
}

impl<M, S> SingleNodeConsensus<M, S>
where
    M: Mempool,
    S: BlockStore + StateStore + TxStore,
{
    pub fn new(mempool: M, storage: S) -> Self {
        Self {
            view: ViewNumber(0),
            validator: ValidatorId([0u8; 32]),
            mempool,
            storage,
            last_block_id: None,
            last_height: 0,
        }
    }

    fn build_block(&mut self) -> Result<Option<Block>, ConsensusError> {
        // For now, pull a small fixed batch.
        let batch = self.mempool.get_batch(100);
        if batch.is_empty() {
            return Ok(None);
        }

        let tx_ids: Vec<TxId> = batch.iter().map(|(id, _)| *id).collect();
        let tx_root = merkle_root(&tx_ids);

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let header = BlockHeader {
            height: self.last_height + 1,
            parent: self.last_block_id,
            tx_root,
            // Placeholder: real state root will come from execution.
            state_root: Hash([0u8; 32]),
            timestamp_ms: now_ms,
            proposer: self.validator.0,
        };

        let block = Block {
            header,
            txs: tx_ids,
        };

        Ok(Some(block))
    }
}

impl<M, S> ConsensusEngine for SingleNodeConsensus<M, S>
where
    M: Mempool,
    S: BlockStore + StateStore + TxStore,
{
    fn submit_tx(&mut self, tx: Transaction) -> Result<TxId, ConsensusError> {
        self
            .mempool
            .insert(tx)
            .map_err(|e| ConsensusError::Mempool(e.to_string()))
    }

    #[instrument(skip(self))]
    fn step(&mut self) -> Result<Option<FinalityEvent>, ConsensusError> {
        let start = Instant::now();
        self.view.0 += 1;

        let Some(block) = self.build_block()? else {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            sequencer_metrics::record_consensus_step_duration_ms(elapsed);
            return Ok(None);
        };

        let block_id = block.header.id();
        let height = block.header.height;

        // Persist block and txs.
        self.storage.put_block(block.clone())?;
        for tx_id in &block.txs {
            // We don't store full txs here because they should already
            // be present from earlier, but for now keep it simple by
            // ignoring this step. Future work can link tx bodies.
            let _ = tx_id;
        }

        let qc = QuorumCertificate {
            view: self.view,
            block_id,
        };

        self.last_block_id = Some(block_id);
        self.last_height = height;
        sequencer_metrics::record_block_committed(block.txs.len());
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        sequencer_metrics::record_consensus_step_duration_ms(elapsed);

        Ok(Some(FinalityEvent::BlockCommitted { block, qc }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn single_node_commits_blocks_from_mempool() {
        let mempool = SimpleMempool::default();
        let storage = InMemoryStorage::default();
        let mut engine = SingleNodeConsensus::new(mempool, storage);

        // Submit a few transactions.
        for i in 0..3 {
            engine.submit_tx(make_tx(i)).unwrap();
        }

        // One step should commit at least one block.
        let event = engine.step().unwrap();
        match event {
            Some(FinalityEvent::BlockCommitted { block, qc }) => {
                assert_eq!(block.header.height, 1);
                assert_eq!(qc.block_id, block.header.id());
            }
            _ => panic!("expected committed block"),
        }
    }

    #[test]
    fn committed_block_heights_are_strictly_increasing() {
        let mempool = SimpleMempool::default();
        let storage = InMemoryStorage::default();
        let mut engine = SingleNodeConsensus::new(mempool, storage);

        // Submit several transactions so multiple blocks can be produced.
        for i in 0..5 {
            engine.submit_tx(make_tx(i)).unwrap();
        }

        let mut last_height = 0u64;
        for _ in 0..5 {
            if let Some(FinalityEvent::BlockCommitted { block, .. }) = engine.step().unwrap() {
                assert!(block.header.height > last_height);
                last_height = block.header.height;
            }
        }
    }

    #[test]
    fn no_two_distinct_blocks_at_same_height() {
        let mempool = SimpleMempool::default();
        let storage = InMemoryStorage::default();
        let mut engine = SingleNodeConsensus::new(mempool, storage);

        // Pre-fill enough transactions for several blocks.
        for i in 0..10 {
            engine.submit_tx(make_tx(i)).unwrap();
        }

        use std::collections::HashMap;
        let mut by_height: HashMap<u64, types::BlockId> = HashMap::new();

        for _ in 0..10 {
            if let Some(FinalityEvent::BlockCommitted { block, .. }) = engine.step().unwrap() {
                let h = block.header.height;
                let id = block.header.id();
                if let Some(existing) = by_height.get(&h) {
                    assert_eq!(*existing, id, "two distinct blocks at same height");
                } else {
                    by_height.insert(h, id);
                }
            }
        }
    }

    #[test]
    fn l1_batch_commitment_covers_committed_blocks() {
        let mempool = SimpleMempool::default();
        let storage = InMemoryStorage::default();
        let mut engine = SingleNodeConsensus::new(mempool, storage);

        // Submit a few transactions so at least one block is produced.
        for i in 0..3 {
            engine.submit_tx(make_tx(i)).unwrap();
        }

        let mut committed_blocks = Vec::new();
        if let Some(FinalityEvent::BlockCommitted { block, .. }) = engine.step().unwrap() {
            committed_blocks.push(block);
        }

        assert!(!committed_blocks.is_empty());

        let batch = build_l1_batch_commitment(0, &committed_blocks);
        assert_eq!(batch.batch_number, 0);
        assert_eq!(batch.block_ids.len(), committed_blocks.len());
        assert_eq!(batch.block_ids[0], committed_blocks[0].header.id());

        // The batch hash is deterministic.
        let h1 = batch.hash();
        let h2 = batch.hash();
        assert_eq!(h1, h2);
    }
}
