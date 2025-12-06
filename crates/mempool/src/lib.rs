use std::collections::{HashMap, VecDeque};
use thiserror::Error;
use types::{NamespaceId, Transaction, TxId};

#[derive(Clone, Debug)]
pub struct MempoolConfig {
    pub max_tx: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self { max_tx: 10_000 }
    }
}

#[derive(Debug, Error)]
pub enum MempoolError {
    #[error("mempool is full")]
    Full,
}

/// Basic mempool interface. 
/// Intentional TODO: add async support later, when integrating with the rest of the system.
pub trait Mempool {
    fn insert(&mut self, tx: Transaction) -> Result<TxId, MempoolError>;
    fn get_batch(&self, max: usize) -> Vec<(TxId, Transaction)>;
    fn remove_committed(&mut self, ids: &[TxId]);
    fn len(&self) -> usize;
}

/// A naive in-memory mempool that preserves insertion order and tracks
/// transactions per namespace for a priority-based, gas-aware implementation.
#[derive(Debug)]
pub struct SimpleMempool {
    config: MempoolConfig,
    queue: VecDeque<TxId>,
    txs: HashMap<TxId, Transaction>,
    by_namespace: HashMap<NamespaceId, Vec<TxId>>,
}

impl SimpleMempool {
    pub fn new(config: MempoolConfig) -> Self {
        Self {
            config,
            queue: VecDeque::new(),
            txs: HashMap::new(),
            by_namespace: HashMap::new(),
        }
    }
}

impl Default for SimpleMempool {
    fn default() -> Self {
        Self::new(MempoolConfig::default())
    }
}

impl Mempool for SimpleMempool {
    fn insert(&mut self, tx: Transaction) -> Result<TxId, MempoolError> {
        if self.txs.len() >= self.config.max_tx {
            return Err(MempoolError::Full);
        }

        let id = tx.id();
        if self.txs.contains_key(&id) {
            return Ok(id);
        }

        self.queue.push_back(id);
        self.by_namespace
            .entry(tx.namespace)
            .or_insert_with(Vec::new)
            .push(id);
        self.txs.insert(id, tx);

        Ok(id)
    }

    fn get_batch(&self, max: usize) -> Vec<(TxId, Transaction)> {
        self.queue
            .iter()
            .take(max)
            .filter_map(|id| self.txs.get(id).cloned().map(|tx| (*id, tx)))
            .collect()
    }

    fn remove_committed(&mut self, ids: &[TxId]) {
        for id in ids {
            if let Some(tx) = self.txs.remove(id) {
                if let Some(list) = self.by_namespace.get_mut(&tx.namespace) {
                    list.retain(|tid| tid != id);
                }
            }
        }
        self.queue.retain(|id| !ids.contains(id));
    }

    fn len(&self) -> usize {
        self.txs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx(namespace: u64, nonce: u64) -> Transaction {
        Transaction {
            namespace: NamespaceId(namespace),
            gas_price: 1,
            nonce,
            payload: vec![],
            signature: vec![],
        }
    }

    #[test]
    fn insert_and_get_batch_preserves_order() {
        let mut mp = SimpleMempool::default();

        let tx1 = make_tx(1, 1);
        let tx2 = make_tx(1, 2);
        let id1 = mp.insert(tx1.clone()).unwrap();
        let id2 = mp.insert(tx2.clone()).unwrap();

        let batch = mp.get_batch(10);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].0, id1);
        assert_eq!(batch[1].0, id2);
    }

    #[test]
    fn remove_committed_evicts_from_mempool() {
        let mut mp = SimpleMempool::default();
        let tx1 = make_tx(1, 1);
        let tx2 = make_tx(2, 1);
        let id1 = mp.insert(tx1).unwrap();
        let id2 = mp.insert(tx2).unwrap();

        mp.remove_committed(&[id1]);
        assert_eq!(mp.len(), 1);

        let remaining: Vec<_> = mp.get_batch(10).into_iter().map(|(id, _)| id).collect();
        assert_eq!(remaining, vec![id2]);
    }

    #[test]
    fn mempool_respects_capacity_limit() {
        let mut mp = SimpleMempool::new(MempoolConfig { max_tx: 1 });
        mp.insert(make_tx(1, 1)).unwrap();
        let res = mp.insert(make_tx(1, 2));
        assert!(matches!(res, Err(MempoolError::Full)));
    }
}
