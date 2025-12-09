use std::time::{SystemTime, UNIX_EPOCH};

use mempool::{Mempool, SimpleMempool};
use storage::{BlockStore, InMemoryStorage, StateStore, TxStore};
use thiserror::Error;
use types::{merkle_root, Block, BlockHeader, BlockId, Hash, Transaction, TxId};

use metrics as sequencer_metrics;

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

    fn step(&mut self) -> Result<Option<FinalityEvent>, ConsensusError> {
        self.view.0 += 1;

        let Some(block) = self.build_block()? else {
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
}
