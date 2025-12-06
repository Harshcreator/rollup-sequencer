use blake3::Hasher;
use serde::{Deserialize, Serialize};

/// Fixed-size hash used across the sequencer
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash(#[serde(with = "serde_bytes_array")] pub [u8; 32]);

/// Transaction identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxId(pub Hash);

/// Block identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub Hash);

/// Logical namespace / rollup identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NamespaceId(pub u64);

/// Basic transaction status for RPC and storage
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransactionStatus {
    Pending,
    Included { block: BlockId, index: u32 },
    Rejected,
}

/// Core transaction type used by the sequencer
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub namespace: NamespaceId,
    pub gas_price: u64,
    pub nonce: u64,
    #[serde(with = "serde_bytes_vec")]
    pub payload: Vec<u8>,
    #[serde(with = "serde_bytes_vec")]
    pub signature: Vec<u8>,
}

impl Transaction {
    pub fn id(&self) -> TxId {
        let encoded = bincode::serialize(self).expect("transaction should serialize");
        TxId(hash_bytes(&encoded))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    pub height: u64,
    pub parent: Option<BlockId>,
    pub tx_root: Hash,
    pub state_root: Hash,
    pub timestamp_ms: u64,
    #[serde(with = "serde_bytes_array")]
    pub proposer: [u8; 32],
}

impl BlockHeader {
    pub fn id(&self) -> BlockId {
        let encoded = bincode::serialize(self).expect("header should serialize");
        BlockId(hash_bytes(&encoded))
    }
}

/// Block consisting of a header and list of transaction IDs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub txs: Vec<TxId>,
}

/// Merkle proof for a transaction's inclusion in a block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleProof {
    pub index: u32,
    pub siblings: Vec<Hash>,
}

/// Compute a Merkle root from a list of transaction IDs.
/// Empty input yields a zero hash.
pub fn merkle_root(txs: &[TxId]) -> Hash {
    if txs.is_empty() {
        return Hash([0u8; 32]);
    }

    let mut layer: Vec<Hash> = txs.iter().map(|TxId(h)| *h).collect();

    while layer.len() > 1 {
        let mut next = Vec::with_capacity((layer.len() + 1) / 2);
        for chunk in layer.chunks(2) {
            let combined = if chunk.len() == 2 {
                let mut data = Vec::with_capacity(64);
                data.extend_from_slice(&chunk[0].0);
                data.extend_from_slice(&chunk[1].0);
                data
            } else {
                let mut data = Vec::with_capacity(64);
                data.extend_from_slice(&chunk[0].0);
                data.extend_from_slice(&chunk[0].0);
                data
            };
            next.push(hash_bytes(&combined));
        }
        layer = next;
    }

    layer[0]
}

/// Build a Merkle proof for the leaf at `index`.
pub fn merkle_proof(txs: &[TxId], index: usize) -> Option<MerkleProof> {
    if txs.is_empty() || index >= txs.len() {
        return None;
    }

    let mut idx = index;
    let mut layer: Vec<Hash> = txs.iter().map(|TxId(h)| *h).collect();
    let mut siblings = Vec::new();

    while layer.len() > 1 {
        let is_right = idx % 2 == 1;
        let sibling_idx = if is_right { idx - 1 } else { idx + 1 };

        let sibling_hash = if sibling_idx < layer.len() {
            layer[sibling_idx]
        } else {
            layer[idx]
        };
        siblings.push(sibling_hash);

        idx /= 2;

        let mut next = Vec::with_capacity((layer.len() + 1) / 2);
        for chunk in layer.chunks(2) {
            let combined = if chunk.len() == 2 {
                let mut data = Vec::with_capacity(64);
                data.extend_from_slice(&chunk[0].0);
                data.extend_from_slice(&chunk[1].0);
                data
            } else {
                let mut data = Vec::with_capacity(64);
                data.extend_from_slice(&chunk[0].0);
                data.extend_from_slice(&chunk[0].0);
                data
            };
            next.push(hash_bytes(&combined));
        }
        layer = next;
    }

    Some(MerkleProof {
        index: index as u32,
        siblings,
    })
}

/// Verify that a transaction ID is included in a tree with the given root.
pub fn verify_merkle_proof(root: Hash, leaf: TxId, proof: &MerkleProof) -> bool {
    let mut hash = leaf.0;
    let mut idx = proof.index as usize;

    for sibling in &proof.siblings {
        let mut data = Vec::with_capacity(64);
        if idx % 2 == 0 {
            data.extend_from_slice(&hash.0);
            data.extend_from_slice(&sibling.0);
        } else {
            data.extend_from_slice(&sibling.0);
            data.extend_from_slice(&hash.0);
        }
        hash = hash_bytes(&data);
        idx /= 2;
    }

    hash == root
}

pub fn hash_bytes(data: &[u8]) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(hasher.finalize().as_bytes());
    Hash(out)
}

mod serde_bytes_array {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = [u8; 32];

            fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                write!(f, "a 32-byte hash")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if v.len() != 32 {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let mut out = [0u8; 32];
                out.copy_from_slice(v);
                Ok(out)
            }
        }

        deserializer.deserialize_bytes(Visitor)
    }
}

mod serde_bytes_vec {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Vec<u8>;

            fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
                write!(f, "a byte vector")
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v.to_vec())
            }
        }

        deserializer.deserialize_bytes(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_bytes_is_deterministic() {
        let data = b"hello world";
        let h1 = hash_bytes(data);
        let h2 = hash_bytes(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_bytes_is_sensitive_to_input() {
        let h1 = hash_bytes(b"hello world");
        let h2 = hash_bytes(b"hello world!");
        assert_ne!(h1, h2);
    }

    #[test]
    fn transaction_id_stable_for_same_content() {
        let tx1 = Transaction {
            namespace: NamespaceId(1),
            gas_price: 10,
            nonce: 1,
            payload: b"abc".to_vec(),
            signature: vec![],
        };
        let tx2 = Transaction { ..tx1.clone() };
        assert_eq!(tx1.id(), tx2.id());
    }

    #[test]
    fn block_header_id_changes_with_height() {
        let header1 = BlockHeader {
            height: 1,
            parent: None,
            tx_root: hash_bytes(b"tx_root"),
            state_root: hash_bytes(b"state_root"),
            timestamp_ms: 0,
            proposer: [0u8; 32],
        };

        let mut header2 = header1.clone();
        header2.height = 2;

        assert_ne!(header1.id(), header2.id());
    }

    #[test]
    fn merkle_root_empty_is_zero() {
        let root = merkle_root(&[]);
        assert_eq!(root, Hash([0u8; 32]));
    }

    #[test]
    fn merkle_proof_roundtrip() {
        let txs: Vec<_> = (0u8..4)
            .map(|i| {
                let tx = Transaction {
                    namespace: NamespaceId(1),
                    gas_price: 1,
                    nonce: i as u64,
                    payload: vec![i],
                    signature: vec![],
                };
                tx.id()
            })
            .collect();

        let root = merkle_root(&txs);
        for (idx, tx_id) in txs.iter().enumerate() {
            let proof = merkle_proof(&txs, idx).expect("proof exists");
            assert!(verify_merkle_proof(root, *tx_id, &proof));
        }
    }
}
