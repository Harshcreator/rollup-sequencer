use std::collections::HashMap;
use std::time::Instant;

use thiserror::Error;
use types::{Block, BlockId, Hash, Transaction, TxId};
use metrics as sequencer_metrics;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("not found")]
    NotFound,
    #[error("backend error: {0}")]
    Backend(String),
}

pub trait BlockStore {
    fn put_block(&mut self, block: Block) -> Result<(), StorageError>;
    fn get_block(&self, id: BlockId) -> Result<Block, StorageError>;
    fn get_block_by_height(&self, height: u64) -> Result<Block, StorageError>;
}

pub trait TxStore {
    fn put_tx(&mut self, tx: Transaction) -> Result<TxId, StorageError>;
    fn get_tx(&self, id: TxId) -> Result<Transaction, StorageError>;
}

pub trait StateStore {
    fn put_state_root(&mut self, height: u64, root: Hash) -> Result<(), StorageError>;
    fn latest_state_root(&self) -> Result<(u64, Hash), StorageError>;
}

/// A simple in-memory storage implementation used for testing and as a
/// reference for the sled-backed implementation.
#[derive(Default)]
pub struct InMemoryStorage {
    blocks_by_id: HashMap<BlockId, Block>,
    blocks_by_height: HashMap<u64, BlockId>,
    txs: HashMap<TxId, Transaction>,
    state_roots: HashMap<u64, Hash>,
}

impl BlockStore for InMemoryStorage {
    fn put_block(&mut self, block: Block) -> Result<(), StorageError> {
        let id = block.header.id();
        let height = block.header.height;
        self.blocks_by_height.insert(height, id);
        self.blocks_by_id.insert(id, block);
        Ok(())
    }

    fn get_block(&self, id: BlockId) -> Result<Block, StorageError> {
        self.blocks_by_id
            .get(&id)
            .cloned()
            .ok_or(StorageError::NotFound)
    }

    fn get_block_by_height(&self, height: u64) -> Result<Block, StorageError> {
        let id = self
            .blocks_by_height
            .get(&height)
            .copied()
            .ok_or(StorageError::NotFound)?;
        self.get_block(id)
    }
}

impl TxStore for InMemoryStorage {
    fn put_tx(&mut self, tx: Transaction) -> Result<TxId, StorageError> {
        let id = tx.id();
        self.txs.insert(id, tx);
        Ok(id)
    }

    fn get_tx(&self, id: TxId) -> Result<Transaction, StorageError> {
        self.txs.get(&id).cloned().ok_or(StorageError::NotFound)
    }
}

impl StateStore for InMemoryStorage {
    fn put_state_root(&mut self, height: u64, root: Hash) -> Result<(), StorageError> {
        self.state_roots.insert(height, root);
        Ok(())
    }

    fn latest_state_root(&self) -> Result<(u64, Hash), StorageError> {
        self.state_roots
            .iter()
            .max_by_key(|(h, _)| *h)
            .map(|(h, r)| (*h, *r))
            .ok_or(StorageError::NotFound)
    }
}

/// Sled-backed storage implementation intended for production use.
pub struct SledStorage {
    db: sled::Db,
    blocks: sled::Tree,
    blocks_by_height: sled::Tree,
    txs: sled::Tree,
    state_roots: sled::Tree,
}

impl SledStorage {
    pub fn open(path: &std::path::Path) -> Result<Self, StorageError> {
        let db = sled::open(path).map_err(|e| StorageError::Backend(e.to_string()))?;
        let blocks = db
            .open_tree("blocks")
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let blocks_by_height = db
            .open_tree("blocks_by_height")
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let txs = db
            .open_tree("txs")
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let state_roots = db
            .open_tree("state_roots")
            .map_err(|e| StorageError::Backend(e.to_string()))?;

        Ok(Self {
            db,
            blocks,
            blocks_by_height,
            txs,
            state_roots,
        })
    }
}

impl BlockStore for SledStorage {
    fn put_block(&mut self, block: Block) -> Result<(), StorageError> {
        let start = Instant::now();
        let id = block.header.id();
        let height = block.header.height;
        let key_id = id.0 .0;
        let key_height = height.to_be_bytes();
        let value = bincode::serialize(&block).map_err(|e| StorageError::Backend(e.to_string()))?;

        self.blocks
            .insert(key_id, value)
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        self.blocks_by_height
            .insert(key_height, &id.0 .0)
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        sequencer_metrics::record_storage_op_duration_ms("sled_put_block", elapsed);
        Ok(())
    }

    fn get_block(&self, id: BlockId) -> Result<Block, StorageError> {
        let start = Instant::now();
        let key_id = id.0 .0;
        let Some(bytes) = self
            .blocks
            .get(key_id)
            .map_err(|e| StorageError::Backend(e.to_string()))? else {
            return Err(StorageError::NotFound);
        };
        let block: Block = bincode::deserialize(&bytes)
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        sequencer_metrics::record_storage_op_duration_ms("sled_get_block", elapsed);
        Ok(block)
    }

    fn get_block_by_height(&self, height: u64) -> Result<Block, StorageError> {
        let start = Instant::now();
        let key_height = height.to_be_bytes();
        let Some(id_bytes) = self
            .blocks_by_height
            .get(key_height)
            .map_err(|e| StorageError::Backend(e.to_string()))? else {
            return Err(StorageError::NotFound);
        };
        let mut id_arr = [0u8; 32];
        id_arr.copy_from_slice(&id_bytes);
        let id = BlockId(Hash(id_arr));
        let block = self.get_block(id)?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        sequencer_metrics::record_storage_op_duration_ms("sled_get_block_by_height", elapsed);
        Ok(block)
    }
}

impl TxStore for SledStorage {
    fn put_tx(&mut self, tx: Transaction) -> Result<TxId, StorageError> {
        let start = Instant::now();
        let id = tx.id();
        let key_id = id.0 .0;
        let value = bincode::serialize(&tx).map_err(|e| StorageError::Backend(e.to_string()))?;
        self.txs
            .insert(key_id, value)
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        sequencer_metrics::record_storage_op_duration_ms("sled_put_tx", elapsed);
        Ok(id)
    }

    fn get_tx(&self, id: TxId) -> Result<Transaction, StorageError> {
        let start = Instant::now();
        let key_id = id.0 .0;
        let Some(bytes) = self
            .txs
            .get(key_id)
            .map_err(|e| StorageError::Backend(e.to_string()))? else {
            return Err(StorageError::NotFound);
        };
        let tx: Transaction = bincode::deserialize(&bytes)
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        sequencer_metrics::record_storage_op_duration_ms("sled_get_tx", elapsed);
        Ok(tx)
    }
}

impl StateStore for SledStorage {
    fn put_state_root(&mut self, height: u64, root: Hash) -> Result<(), StorageError> {
        let start = Instant::now();
        let key_height = height.to_be_bytes();
        self.state_roots
            .insert(key_height, &root.0)
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        sequencer_metrics::record_storage_op_duration_ms("sled_put_state_root", elapsed);
        Ok(())
    }

    fn latest_state_root(&self) -> Result<(u64, Hash), StorageError> {
        let start = Instant::now();
        let mut latest: Option<(u64, Hash)> = None;
        for res in self.state_roots.iter() {
            let (k, v) = res.map_err(|e| StorageError::Backend(e.to_string()))?;
            let mut height_bytes = [0u8; 8];
            height_bytes.copy_from_slice(&k);
            let height = u64::from_be_bytes(height_bytes);
            let mut root_bytes = [0u8; 32];
            root_bytes.copy_from_slice(&v);
            let candidate = (height, Hash(root_bytes));
            if let Some((best_h, _)) = latest {
                if height > best_h {
                    latest = Some(candidate);
                }
            } else {
                latest = Some(candidate);
            }
        }
        let result = latest.ok_or(StorageError::NotFound);
        if result.is_ok() {
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            sequencer_metrics::record_storage_op_duration_ms("sled_latest_state_root", elapsed);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{BlockHeader, NamespaceId, Transaction};
    use proptest::prelude::*;

    fn make_block(height: u64) -> Block {
        let header = BlockHeader {
            height,
            parent: None,
            tx_root: Hash([0u8; 32]),
            state_root: Hash([0u8; 32]),
            timestamp_ms: 0,
            proposer: [0u8; 32],
        };
        Block {
            header,
            txs: Vec::new(),
        }
    }

    fn make_tx(nonce: u64) -> Transaction {
        Transaction {
            namespace: NamespaceId(1),
            gas_price: 1,
            nonce,
            payload: vec![],
            signature: vec![],
        }
    }

    proptest! {
        #[test]
        fn in_memory_tx_roundtrip_holds(nonces in proptest::collection::vec(0u64..1000, 0..32)) {
            let mut store = InMemoryStorage::default();
            let mut ids = Vec::new();
            for nonce in nonces {
                let tx = make_tx(nonce);
                let id = store.put_tx(tx.clone()).unwrap();
                ids.push((id, tx));
            }

            for (id, original) in ids {
                let loaded = store.get_tx(id).unwrap();
                prop_assert_eq!(loaded.id(), original.id());
            }
        }
    }

    #[test]
    fn block_roundtrip_by_id_and_height() {
        let mut store = InMemoryStorage::default();
        let block = make_block(1);
        let id = block.header.id();

        BlockStore::put_block(&mut store, block.clone()).unwrap();

        let fetched_by_id = BlockStore::get_block(&store, id).unwrap();
        let fetched_by_height = BlockStore::get_block_by_height(&store, 1).unwrap();

        assert_eq!(fetched_by_id.header.height, 1);
        assert_eq!(fetched_by_height.header.height, 1);
        assert_eq!(fetched_by_id.header.id(), id);
    }

    #[test]
    fn tx_roundtrip() {
        let mut store = InMemoryStorage::default();
        let tx = make_tx(1);
        let id = TxStore::put_tx(&mut store, tx.clone()).unwrap();

        let fetched = TxStore::get_tx(&store, id).unwrap();
        assert_eq!(fetched.nonce, tx.nonce);
    }

    #[test]
    fn state_root_latest_tracks_highest_height() {
        let mut store = InMemoryStorage::default();
        StateStore::put_state_root(&mut store, 1, Hash([1u8; 32])).unwrap();
        StateStore::put_state_root(&mut store, 5, Hash([5u8; 32])).unwrap();

        let (height, root) = StateStore::latest_state_root(&store).unwrap();
        assert_eq!(height, 5);
        assert_eq!(root, Hash([5u8; 32]));
    }

    #[test]
    fn sled_block_tx_and_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let mut store = SledStorage::open(path).unwrap();

        // Block roundtrip
        let block = make_block(7);
        let block_id = block.header.id();
        BlockStore::put_block(&mut store, block.clone()).unwrap();
        let fetched_block = BlockStore::get_block(&store, block_id).unwrap();
        assert_eq!(fetched_block.header.height, 7);

        // Tx roundtrip
        let tx = make_tx(42);
        let tx_id = TxStore::put_tx(&mut store, tx.clone()).unwrap();
        let fetched_tx = TxStore::get_tx(&store, tx_id).unwrap();
        assert_eq!(fetched_tx.nonce, tx.nonce);

        // State root roundtrip
        StateStore::put_state_root(&mut store, 3, Hash([3u8; 32])).unwrap();
        let (h, root) = StateStore::latest_state_root(&store).unwrap();
        assert_eq!(h, 3);
        assert_eq!(root, Hash([3u8; 32]));
    }
}
