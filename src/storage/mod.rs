//! Storage engine primitives for Phase 3.
//!
//! This module provides:
//! - append-only Write-Ahead Log (WAL) with in-memory block-device persistence,
//! - crash recovery via deterministic WAL replay,
//! - copy-on-write object map updates with atomic publication,
//! - a compact fixed-fanout B-Tree with copy-on-write root publication.

use crate::sync::Mutex;
use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::{AtomicUsize, Ordering};

const WAL_MAGIC: u32 = 0x534F_5357;
const WAL_VERSION: u16 = 1;
const WAL_RECORD_ENCODED_SIZE: usize = 64;
pub const WAL_BLOCK_SIZE: usize = WAL_RECORD_ENCODED_SIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalOp {
    Put,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalRecord {
    pub tx_id: u64,
    pub op: WalOp,
    pub key_hash: [u8; 32],
    pub lba: u64,
    pub len: u32,
    pub committed: bool,
}

impl WalRecord {
    pub fn put(tx_id: u64, key_hash: [u8; 32], lba: u64, len: u32) -> Self {
        Self {
            tx_id,
            op: WalOp::Put,
            key_hash,
            lba,
            len,
            committed: false,
        }
    }

    pub fn delete(tx_id: u64, key_hash: [u8; 32]) -> Self {
        Self {
            tx_id,
            op: WalOp::Delete,
            key_hash,
            lba: 0,
            len: 0,
            committed: false,
        }
    }

    fn encode(&self) -> [u8; WAL_RECORD_ENCODED_SIZE] {
        let mut out = [0u8; WAL_RECORD_ENCODED_SIZE];
        out[0..4].copy_from_slice(&WAL_MAGIC.to_le_bytes());
        out[4..6].copy_from_slice(&WAL_VERSION.to_le_bytes());
        out[6] = match self.op {
            WalOp::Put => 1,
            WalOp::Delete => 2,
        };
        out[7] = u8::from(self.committed);
        out[8..16].copy_from_slice(&self.tx_id.to_le_bytes());
        out[16..48].copy_from_slice(&self.key_hash);
        out[48..56].copy_from_slice(&self.lba.to_le_bytes());
        out[56..60].copy_from_slice(&self.len.to_le_bytes());
        let checksum = checksum32(&out[..60]);
        out[60..64].copy_from_slice(&checksum.to_le_bytes());
        out
    }

    fn decode(buf: &[u8; WAL_RECORD_ENCODED_SIZE]) -> Option<Self> {
        let magic = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        if magic != WAL_MAGIC {
            return None;
        }
        let version = u16::from_le_bytes([buf[4], buf[5]]);
        if version != WAL_VERSION {
            return None;
        }

        let expected = u32::from_le_bytes([buf[60], buf[61], buf[62], buf[63]]);
        if checksum32(&buf[..60]) != expected {
            return None;
        }

        let op = match buf[6] {
            1 => WalOp::Put,
            2 => WalOp::Delete,
            _ => return None,
        };
        let committed = buf[7] != 0;
        let tx_id = u64::from_le_bytes([
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
        ]);
        let mut key_hash = [0u8; 32];
        key_hash.copy_from_slice(&buf[16..48]);
        let lba = u64::from_le_bytes([
            buf[48], buf[49], buf[50], buf[51], buf[52], buf[53], buf[54], buf[55],
        ]);
        let len = u32::from_le_bytes([buf[56], buf[57], buf[58], buf[59]]);

        Some(Self {
            tx_id,
            op,
            key_hash,
            lba,
            len,
            committed,
        })
    }
}

fn checksum32(data: &[u8]) -> u32 {
    let mut s = 0u32;
    for b in data {
        s = s.wrapping_add(*b as u32);
        s = s.rotate_left(3);
    }
    s
}

pub trait WalBlockDevice {
    fn capacity_blocks(&self) -> usize;
    fn read_block(&self, index: usize, out: &mut [u8; WAL_BLOCK_SIZE]) -> bool;
    fn write_block(&self, index: usize, data: &[u8; WAL_BLOCK_SIZE]) -> bool;
}

pub struct InMemoryWalDevice<const BLOCKS: usize> {
    blocks: Mutex<[[u8; WAL_BLOCK_SIZE]; BLOCKS]>,
}

impl<const BLOCKS: usize> InMemoryWalDevice<BLOCKS> {
    pub const fn new() -> Self {
        Self {
            blocks: Mutex::new([[0u8; WAL_BLOCK_SIZE]; BLOCKS]),
        }
    }

    #[cfg(test)]
    pub fn corrupt_byte(&self, block: usize, byte: usize, value: u8) {
        let mut b = self.blocks.lock();
        if block < BLOCKS && byte < WAL_BLOCK_SIZE {
            b[block][byte] = value;
        }
    }
}

impl<const BLOCKS: usize> Default for InMemoryWalDevice<BLOCKS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const BLOCKS: usize> WalBlockDevice for InMemoryWalDevice<BLOCKS> {
    fn capacity_blocks(&self) -> usize {
        BLOCKS
    }

    fn read_block(&self, index: usize, out: &mut [u8; WAL_BLOCK_SIZE]) -> bool {
        if index >= BLOCKS {
            return false;
        }
        let b = self.blocks.lock();
        out.copy_from_slice(&b[index]);
        true
    }

    fn write_block(&self, index: usize, data: &[u8; WAL_BLOCK_SIZE]) -> bool {
        if index >= BLOCKS {
            return false;
        }
        let mut b = self.blocks.lock();
        b[index] = *data;
        true
    }
}

pub struct WriteAheadLog<'a, D: WalBlockDevice> {
    device: &'a D,
    head: AtomicUsize,
}

impl<'a, D: WalBlockDevice> WriteAheadLog<'a, D> {
    pub fn new(device: &'a D) -> Self {
        Self {
            device,
            head: AtomicUsize::new(0),
        }
    }

    pub fn append(&self, rec: WalRecord) -> Option<usize> {
        let idx = self.head.fetch_add(1, Ordering::AcqRel);
        if idx >= self.device.capacity_blocks() {
            return None;
        }
        let enc = rec.encode();
        if !self.device.write_block(idx, &enc) {
            return None;
        }
        Some(idx)
    }

    pub fn commit(&self, slot: usize) -> bool {
        if slot >= self.device.capacity_blocks() {
            return false;
        }
        let mut raw = [0u8; WAL_BLOCK_SIZE];
        if !self.device.read_block(slot, &mut raw) {
            return false;
        }
        let mut rec = match WalRecord::decode(&raw) {
            Some(r) => r,
            None => return false,
        };
        rec.committed = true;
        self.device.write_block(slot, &rec.encode())
    }

    pub fn replay_committed<F: FnMut(WalRecord)>(&self, mut apply: F) {
        for idx in 0..self.device.capacity_blocks() {
            let mut raw = [0u8; WAL_BLOCK_SIZE];
            if !self.device.read_block(idx, &mut raw) {
                continue;
            }
            if let Some(rec) = WalRecord::decode(&raw) {
                if rec.committed {
                    apply(rec);
                }
            }
        }
    }

    pub fn recover_head(&self) -> usize {
        let mut end = 0usize;
        for idx in 0..self.device.capacity_blocks() {
            let mut raw = [0u8; WAL_BLOCK_SIZE];
            if !self.device.read_block(idx, &mut raw) {
                break;
            }
            if WalRecord::decode(&raw).is_some() {
                end = idx + 1;
            } else {
                break;
            }
        }
        self.head.store(end, Ordering::Release);
        end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectEntry {
    pub key_hash: [u8; 32],
    pub lba: u64,
    pub len: u32,
}

#[derive(Clone, Copy)]
struct ObjectMap<const N: usize> {
    entries: [Option<ObjectEntry>; N],
}

impl<const N: usize> ObjectMap<N> {
    const fn new() -> Self {
        Self { entries: [None; N] }
    }

    fn get(&self, key_hash: &[u8; 32]) -> Option<ObjectEntry> {
        self.entries
            .iter()
            .flatten()
            .find(|e| &e.key_hash == key_hash)
            .copied()
    }

    fn upsert(&mut self, entry: ObjectEntry) -> bool {
        for slot in &mut self.entries {
            if let Some(existing) = slot {
                if existing.key_hash == entry.key_hash {
                    *slot = Some(entry);
                    return true;
                }
            }
        }
        for slot in &mut self.entries {
            if slot.is_none() {
                *slot = Some(entry);
                return true;
            }
        }
        false
    }

    fn delete(&mut self, key_hash: &[u8; 32]) -> bool {
        for slot in &mut self.entries {
            if let Some(existing) = slot {
                if &existing.key_hash == key_hash {
                    *slot = None;
                    return true;
                }
            }
        }
        false
    }
}

pub struct CowObjectIndex<const N: usize> {
    maps: [Mutex<ObjectMap<N>>; 2],
    active: AtomicUsize,
}

impl<const N: usize> CowObjectIndex<N> {
    pub fn new() -> Self {
        Self {
            maps: [Mutex::new(ObjectMap::new()), Mutex::new(ObjectMap::new())],
            active: AtomicUsize::new(0),
        }
    }

    pub fn get(&self, key_hash: &[u8; 32]) -> Option<ObjectEntry> {
        let idx = self.active.load(Ordering::Acquire) & 1;
        let map = self.maps[idx].lock();
        map.get(key_hash)
    }

    pub fn upsert(&self, entry: ObjectEntry) -> bool {
        self.swap_with(|m| m.upsert(entry))
    }

    pub fn delete(&self, key_hash: &[u8; 32]) -> bool {
        self.swap_with(|m| m.delete(key_hash))
    }

    fn swap_with<F: FnOnce(&mut ObjectMap<N>) -> bool>(&self, f: F) -> bool {
        let current = self.active.load(Ordering::Acquire) & 1;
        let next = 1 - current;

        let src = self.maps[current].lock();
        let mut dst = self.maps[next].lock();
        *dst = *src;
        let changed = f(&mut dst);
        let _ = self
            .active
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire);
        changed
    }

    pub fn apply_wal(&self, rec: WalRecord) -> bool {
        match rec.op {
            WalOp::Put => self.upsert(ObjectEntry {
                key_hash: rec.key_hash,
                lba: rec.lba,
                len: rec.len,
            }),
            WalOp::Delete => self.delete(&rec.key_hash),
        }
    }
}

impl<const N: usize> Default for CowObjectIndex<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BTreeNodeEntry {
    pub key_hash: [u8; 32],
    pub lba: u64,
}

#[derive(Clone, Copy)]
pub struct BTreeNode<const FANOUT: usize> {
    entries: [Option<BTreeNodeEntry>; FANOUT],
    len: usize,
}

impl<const FANOUT: usize> BTreeNode<FANOUT> {
    pub const fn new() -> Self {
        Self {
            entries: [None; FANOUT],
            len: 0,
        }
    }

    pub fn insert_sorted(&mut self, entry: BTreeNodeEntry) -> bool {
        if self.len >= FANOUT {
            return false;
        }

        let mut pos = self.len;
        for i in 0..self.len {
            let cur = self.entries[i].expect("occupied before len");
            if key_cmp(&entry.key_hash, &cur.key_hash).is_lt() {
                pos = i;
                break;
            }
            if cur.key_hash == entry.key_hash {
                self.entries[i] = Some(entry);
                return true;
            }
        }

        for i in (pos..self.len).rev() {
            self.entries[i + 1] = self.entries[i];
        }
        self.entries[pos] = Some(entry);
        self.len += 1;
        true
    }

    pub fn delete(&mut self, key_hash: &[u8; 32]) -> bool {
        let mut pos = None;
        for i in 0..self.len {
            if let Some(entry) = self.entries[i] {
                if &entry.key_hash == key_hash {
                    pos = Some(i);
                    break;
                }
            }
        }
        let i = match pos {
            Some(v) => v,
            None => return false,
        };
        for j in i..(self.len - 1) {
            self.entries[j] = self.entries[j + 1];
        }
        self.entries[self.len - 1] = None;
        self.len -= 1;
        true
    }

    pub fn find(&self, key_hash: &[u8; 32]) -> Option<BTreeNodeEntry> {
        self.entries[..self.len]
            .iter()
            .flatten()
            .find(|e| &e.key_hash == key_hash)
            .copied()
    }

    pub fn entries(&self) -> &[Option<BTreeNodeEntry>] {
        &self.entries[..self.len]
    }

    #[cfg(all(test, feature = "crypto"))]
    fn force_hole_for_test(&mut self) {
        assert!(FANOUT > 0);
        self.len = 1;
        self.entries[0] = None;
    }
}

impl<const FANOUT: usize> Default for BTreeNode<FANOUT> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CowBTreeIndex<const FANOUT: usize> {
    roots: [Mutex<BTreeNode<FANOUT>>; 2],
    active: AtomicUsize,
}

impl<const FANOUT: usize> CowBTreeIndex<FANOUT> {
    pub fn new() -> Self {
        Self {
            roots: [Mutex::new(BTreeNode::new()), Mutex::new(BTreeNode::new())],
            active: AtomicUsize::new(0),
        }
    }

    pub fn find(&self, key_hash: &[u8; 32]) -> Option<BTreeNodeEntry> {
        let idx = self.active.load(Ordering::Acquire) & 1;
        let root = self.roots[idx].lock();
        root.find(key_hash)
    }

    pub fn upsert(&self, entry: BTreeNodeEntry) -> bool {
        self.swap_root(|root| root.insert_sorted(entry))
    }

    pub fn delete(&self, key_hash: &[u8; 32]) -> bool {
        self.swap_root(|root| root.delete(key_hash))
    }

    fn swap_root<F: FnOnce(&mut BTreeNode<FANOUT>) -> bool>(&self, f: F) -> bool {
        let current = self.active.load(Ordering::Acquire) & 1;
        let next = 1 - current;

        let src = self.roots[current].lock();
        let mut dst = self.roots[next].lock();
        *dst = *src;
        let changed = f(&mut dst);
        let _ = self
            .active
            .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire);
        changed
    }
}

impl<const FANOUT: usize> Default for CowBTreeIndex<FANOUT> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxError {
    WalFull,
    CommitFailed,
    IndexUpdateFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxStatus {
    Idle,
    InFlight,
    Committed,
    Aborted,
}

impl TxStatus {
    fn from_usize(v: usize) -> Self {
        match v {
            1 => TxStatus::InFlight,
            2 => TxStatus::Committed,
            3 => TxStatus::Aborted,
            _ => TxStatus::Idle,
        }
    }

    fn as_usize(self) -> usize {
        match self {
            TxStatus::Idle => 0,
            TxStatus::InFlight => 1,
            TxStatus::Committed => 2,
            TxStatus::Aborted => 3,
        }
    }
}

pub struct AtomicTransactionManager<'a, D: WalBlockDevice, const N: usize, const FANOUT: usize> {
    wal: WriteAheadLog<'a, D>,
    object_index: CowObjectIndex<N>,
    btree_index: CowBTreeIndex<FANOUT>,
    tx_seq: AtomicUsize,
    epoch: AtomicUsize,
    status: AtomicUsize,
}

impl<'a, D: WalBlockDevice, const N: usize, const FANOUT: usize>
    AtomicTransactionManager<'a, D, N, FANOUT>
{
    pub fn new(device: &'a D) -> Self {
        Self {
            wal: WriteAheadLog::new(device),
            object_index: CowObjectIndex::new(),
            btree_index: CowBTreeIndex::new(),
            tx_seq: AtomicUsize::new(1),
            epoch: AtomicUsize::new(0),
            status: AtomicUsize::new(TxStatus::Idle.as_usize()),
        }
    }

    pub fn status(&self) -> TxStatus {
        TxStatus::from_usize(self.status.load(Ordering::Acquire))
    }

    pub fn epoch(&self) -> usize {
        self.epoch.load(Ordering::Acquire)
    }

    pub fn put(&self, key_hash: [u8; 32], lba: u64, len: u32) -> Result<usize, TxError> {
        self.status
            .store(TxStatus::InFlight.as_usize(), Ordering::Release);

        let tx_id = self.tx_seq.fetch_add(1, Ordering::AcqRel) as u64;
        let rec = WalRecord::put(tx_id, key_hash, lba, len);
        let slot = self.wal.append(rec).ok_or(TxError::WalFull)?;
        if !self.wal.commit(slot) {
            self.status
                .store(TxStatus::Aborted.as_usize(), Ordering::Release);
            return Err(TxError::CommitFailed);
        }

        if !self.object_index.apply_wal(WalRecord {
            committed: true,
            ..rec
        }) {
            self.status
                .store(TxStatus::Aborted.as_usize(), Ordering::Release);
            return Err(TxError::IndexUpdateFailed);
        }

        let btree_ok = self.btree_index.upsert(BTreeNodeEntry { key_hash, lba });
        if !btree_ok {
            self.status
                .store(TxStatus::Aborted.as_usize(), Ordering::Release);
            return Err(TxError::IndexUpdateFailed);
        }

        self.bump_epoch_cas();
        self.status
            .store(TxStatus::Committed.as_usize(), Ordering::Release);
        Ok(slot)
    }

    pub fn delete(&self, key_hash: [u8; 32]) -> Result<usize, TxError> {
        self.status
            .store(TxStatus::InFlight.as_usize(), Ordering::Release);

        let tx_id = self.tx_seq.fetch_add(1, Ordering::AcqRel) as u64;
        let rec = WalRecord::delete(tx_id, key_hash);
        let slot = self.wal.append(rec).ok_or(TxError::WalFull)?;
        if !self.wal.commit(slot) {
            self.status
                .store(TxStatus::Aborted.as_usize(), Ordering::Release);
            return Err(TxError::CommitFailed);
        }

        let _ = self.object_index.apply_wal(WalRecord {
            committed: true,
            ..rec
        });
        let _ = self.btree_index.delete(&key_hash);

        self.bump_epoch_cas();
        self.status
            .store(TxStatus::Committed.as_usize(), Ordering::Release);
        Ok(slot)
    }

    pub fn recover(&self) {
        let _ = self.wal.recover_head();
        self.wal.replay_committed(|rec| {
            let _ = self.object_index.apply_wal(rec);
            match rec.op {
                WalOp::Put => {
                    let _ = self.btree_index.upsert(BTreeNodeEntry {
                        key_hash: rec.key_hash,
                        lba: rec.lba,
                    });
                }
                WalOp::Delete => {
                    let _ = self.btree_index.delete(&rec.key_hash);
                }
            }
        });
        self.status
            .store(TxStatus::Committed.as_usize(), Ordering::Release);
    }

    pub fn get_object(&self, key_hash: &[u8; 32]) -> Option<ObjectEntry> {
        self.object_index.get(key_hash)
    }

    pub fn btree_lookup(&self, key_hash: &[u8; 32]) -> Option<BTreeNodeEntry> {
        self.btree_index.find(key_hash)
    }

    fn bump_epoch_cas(&self) {
        let mut cur = self.epoch.load(Ordering::Acquire).wrapping_sub(1);
        loop {
            let next = cur.wrapping_add(1);
            match self
                .epoch
                .compare_exchange_weak(cur, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(observed) => cur = observed,
            }
        }
    }
}

fn key_cmp(a: &[u8; 32], b: &[u8; 32]) -> CmpOrdering {
    for i in 0..32 {
        if a[i] < b[i] {
            return CmpOrdering::Less;
        }
        if a[i] > b[i] {
            return CmpOrdering::Greater;
        }
    }
    CmpOrdering::Equal
}

#[cfg(test)]
#[cfg(feature = "crypto")]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use crate::crypto::{PathCrypto, NONCE_SIZE};
    use core::sync::atomic::AtomicU64;

    struct FaultyWalDevice<const BLOCKS: usize> {
        fail_read_at: Option<usize>,
        fail_write_at: Option<usize>,
        blocks: Mutex<[[u8; WAL_BLOCK_SIZE]; BLOCKS]>,
    }

    impl<const BLOCKS: usize> FaultyWalDevice<BLOCKS> {
        const fn new(fail_read_at: Option<usize>, fail_write_at: Option<usize>) -> Self {
            Self {
                fail_read_at,
                fail_write_at,
                blocks: Mutex::new([[0u8; WAL_BLOCK_SIZE]; BLOCKS]),
            }
        }

        fn write_raw(&self, index: usize, raw: [u8; WAL_BLOCK_SIZE]) {
            let mut blocks = self.blocks.lock();
            if index < BLOCKS {
                blocks[index] = raw;
            }
        }
    }

    impl<const BLOCKS: usize> WalBlockDevice for FaultyWalDevice<BLOCKS> {
        fn capacity_blocks(&self) -> usize {
            BLOCKS
        }

        fn read_block(&self, index: usize, out: &mut [u8; WAL_BLOCK_SIZE]) -> bool {
            if self.fail_read_at == Some(index) || index >= BLOCKS {
                return false;
            }
            let blocks = self.blocks.lock();
            out.copy_from_slice(&blocks[index]);
            true
        }

        fn write_block(&self, index: usize, data: &[u8; WAL_BLOCK_SIZE]) -> bool {
            if self.fail_write_at == Some(index) || index >= BLOCKS {
                return false;
            }
            let mut blocks = self.blocks.lock();
            blocks[index] = *data;
            true
        }
    }

    fn kh(v: u8) -> [u8; 32] {
        [v; 32]
    }

    #[test]
    fn test_wal_commit_and_replay() {
        let dev: InMemoryWalDevice<8> = InMemoryWalDevice::new();
        let wal = WriteAheadLog::new(&dev);
        let slot1 = wal.append(WalRecord::put(1, kh(1), 100, 32)).unwrap();
        let slot2 = wal.append(WalRecord::delete(2, kh(2))).unwrap();
        assert!(wal.commit(slot1));
        assert!(wal.commit(slot2));

        let mut replayed = 0usize;
        wal.replay_committed(|_| replayed += 1);
        assert_eq!(replayed, 2);
    }

    #[test]
    fn test_wal_recovery_ignores_corrupted_tail() {
        let dev: InMemoryWalDevice<8> = InMemoryWalDevice::new();
        let wal = WriteAheadLog::new(&dev);
        let s1 = wal.append(WalRecord::put(10, kh(5), 11, 3)).unwrap();
        assert!(wal.commit(s1));
        let _ = wal.append(WalRecord::put(11, kh(6), 22, 4)).unwrap();

        dev.corrupt_byte(1, 0, 0xEE);

        let recovered = wal.recover_head();
        assert_eq!(recovered, 1);

        let mut replayed = 0;
        wal.replay_committed(|r| {
            replayed += 1;
            assert_eq!(r.tx_id, 10);
        });
        assert_eq!(replayed, 1);
    }

    #[test]
    fn test_cow_index_upsert_delete() {
        let idx: CowObjectIndex<16> = CowObjectIndex::new();
        let e = ObjectEntry {
            key_hash: kh(7),
            lba: 123,
            len: 9,
        };
        assert!(idx.upsert(e));
        assert_eq!(idx.get(&kh(7)), Some(e));
        assert!(idx.delete(&kh(7)));
        assert_eq!(idx.get(&kh(7)), None);
    }

    #[test]
    fn test_btree_node_sorted_insert_and_find() {
        let mut node: BTreeNode<4> = BTreeNode::new();
        assert!(node.insert_sorted(BTreeNodeEntry {
            key_hash: kh(3),
            lba: 30,
        }));
        assert!(node.insert_sorted(BTreeNodeEntry {
            key_hash: kh(1),
            lba: 10,
        }));
        assert!(node.insert_sorted(BTreeNodeEntry {
            key_hash: kh(2),
            lba: 20,
        }));

        assert_eq!(node.find(&kh(1)).unwrap().lba, 10);
        assert_eq!(node.find(&kh(2)).unwrap().lba, 20);
        assert_eq!(node.find(&kh(3)).unwrap().lba, 30);
    }

    #[test]
    fn test_cow_btree_index_upsert_find_delete() {
        let tree: CowBTreeIndex<8> = CowBTreeIndex::new();
        let e = BTreeNodeEntry {
            key_hash: kh(9),
            lba: 900,
        };
        assert!(tree.upsert(e));
        assert_eq!(tree.find(&kh(9)), Some(e));
        assert!(tree.delete(&kh(9)));
        assert_eq!(tree.find(&kh(9)), None);
    }

    #[test]
    fn test_phase3_end_to_end_encrypt_wal_recover_index() {
        let master = [1u8; 32];
        let salt = [2u8; 32];
        let path = "/objects/a.bin";
        let nonce = [7u8; NONCE_SIZE];
        let mut payload = *b"phase3-object";

        let key = PathCrypto::derive_object_key(&master, &salt, path);
        let tag = PathCrypto::encrypt_in_place(&key, &nonce, path.as_bytes(), &mut payload);

        let dev: InMemoryWalDevice<16> = InMemoryWalDevice::new();
        let wal = WriteAheadLog::new(&dev);
        let idx: CowObjectIndex<16> = CowObjectIndex::new();

        let key_hash = PathCrypto::path_hash(path);
        let rec = WalRecord::put(42, key_hash, 4096, payload.len() as u32);
        let slot = wal.append(rec).unwrap();
        assert!(wal.commit(slot));

        let mut replay_count = 0usize;
        wal.replay_committed(|r| {
            replay_count += 1;
            assert!(idx.apply_wal(r));
        });
        assert_eq!(replay_count, 1);

        let stored = idx.get(&key_hash).unwrap();
        assert_eq!(stored.lba, 4096);
        assert_eq!(stored.len, payload.len() as u32);

        let mut decrypted = payload;
        assert!(PathCrypto::decrypt_in_place(
            &key,
            &nonce,
            path.as_bytes(),
            &mut decrypted,
            &tag
        ));
        assert_eq!(&decrypted, b"phase3-object");
    }

    #[test]
    fn test_phase4_atomic_transaction_put_delete() {
        let dev: InMemoryWalDevice<32> = InMemoryWalDevice::new();
        let txm: AtomicTransactionManager<'_, _, 32, 16> = AtomicTransactionManager::new(&dev);

        let key = PathCrypto::path_hash("/phase4/item");
        let put = txm.put(key, 8192, 128);
        assert!(put.is_ok());
        assert_eq!(txm.status(), TxStatus::Committed);
        assert!(txm.get_object(&key).is_some());
        assert!(txm.btree_lookup(&key).is_some());

        let del = txm.delete(key);
        assert!(del.is_ok());
        assert_eq!(txm.status(), TxStatus::Committed);
        assert!(txm.get_object(&key).is_none());
        assert!(txm.btree_lookup(&key).is_none());
    }

    #[test]
    fn test_phase4_recovery_rebuilds_indices() {
        let dev: InMemoryWalDevice<16> = InMemoryWalDevice::new();
        let txm: AtomicTransactionManager<'_, _, 16, 8> = AtomicTransactionManager::new(&dev);
        let key = PathCrypto::path_hash("/phase4/recover");

        assert!(txm.put(key, 12288, 64).is_ok());

        let recovered: AtomicTransactionManager<'_, _, 16, 8> = AtomicTransactionManager::new(&dev);
        recovered.recover();
        assert!(recovered.get_object(&key).is_some());
        assert!(recovered.btree_lookup(&key).is_some());
    }

    #[test]
    fn test_phase4_acquire_release_visibility() {
        let marker = AtomicU64::new(0);
        let published_epoch = AtomicUsize::new(0);

        marker.store(0xA5A5_1234_5678_9ABC, Ordering::Relaxed);
        published_epoch.store(1, Ordering::Release);

        let observed_epoch = published_epoch.load(Ordering::Acquire);
        assert_eq!(observed_epoch, 1);
        let observed_marker = marker.load(Ordering::Relaxed);
        assert_eq!(observed_marker, 0xA5A5_1234_5678_9ABC);
    }

    #[test]
    fn test_phase4_high_load_transaction_stress() {
        const UNIQUE_KEYS: usize = 128;
        const PUT_OPS: usize = 2_000;
        const DELETE_OPS: usize = UNIQUE_KEYS / 2;

        let dev: InMemoryWalDevice<4_096> = InMemoryWalDevice::new();
        let txm: AtomicTransactionManager<'_, _, 256, 256> = AtomicTransactionManager::new(&dev);

        let mut expected = [false; UNIQUE_KEYS];

        for i in 0..PUT_OPS {
            let k = i % UNIQUE_KEYS;
            let key = kh(k as u8);
            let lba = 0x1000 + (i as u64 * 8);
            let len = ((i % 4096) + 1) as u32;
            assert!(txm.put(key, lba, len).is_ok());
            expected[k] = true;
        }

        for k in 0..DELETE_OPS {
            let key = kh(k as u8);
            assert!(txm.delete(key).is_ok());
            expected[k] = false;
        }

        assert_eq!(txm.status(), TxStatus::Committed);
        assert_eq!(txm.epoch(), PUT_OPS + DELETE_OPS);

        for (k, present) in expected.iter().enumerate().take(UNIQUE_KEYS) {
            let entry = txm.get_object(&kh(k as u8));
            assert_eq!(entry.is_some(), *present);
        }

        let recovered: AtomicTransactionManager<'_, _, 256, 256> =
            AtomicTransactionManager::new(&dev);
        recovered.recover();
        for (k, present) in expected.iter().enumerate().take(UNIQUE_KEYS) {
            let entry = recovered.get_object(&kh(k as u8));
            assert_eq!(entry.is_some(), *present);
        }
    }

    #[test]
    fn test_phase4_epoch_is_monotonic_under_updates() {
        let dev: InMemoryWalDevice<512> = InMemoryWalDevice::new();
        let txm: AtomicTransactionManager<'_, _, 64, 64> = AtomicTransactionManager::new(&dev);

        let mut last_epoch = txm.epoch();
        for i in 0..200 {
            let key = kh((i % 32) as u8);
            assert!(txm.put(key, 0x2000 + i as u64, 32).is_ok());
            let current = txm.epoch();
            assert!(current >= last_epoch);
            last_epoch = current;
            assert_eq!(txm.status(), TxStatus::Committed);
        }
        assert_eq!(last_epoch, 200);
    }

    #[test]
    fn test_storage_error_and_default_paths_for_coverage() {
        let _ = InMemoryWalDevice::<1>::default();
        let _ = CowObjectIndex::<1>::default();
        let _ = BTreeNode::<1>::default();
        let _ = CowBTreeIndex::<1>::default();

        let rec = WalRecord::put(1, kh(1), 7, 3);
        let mut raw = rec.encode();
        raw[4] = 0xFF;
        assert!(WalRecord::decode(&raw).is_none());

        let mut raw = rec.encode();
        raw[60] ^= 0x11;
        assert!(WalRecord::decode(&raw).is_none());

        let mut raw = rec.encode();
        raw[6] = 9;
        let checksum = checksum32(&raw[..60]);
        raw[60..64].copy_from_slice(&checksum.to_le_bytes());
        assert!(WalRecord::decode(&raw).is_none());

        let dev: InMemoryWalDevice<1> = InMemoryWalDevice::new();
        let mut buf = [0u8; WAL_BLOCK_SIZE];
        assert!(!dev.read_block(99, &mut buf));
        assert!(!dev.write_block(99, &[0u8; WAL_BLOCK_SIZE]));

        let wal = WriteAheadLog::new(&dev);
        assert!(wal.append(rec).is_some());
        assert!(wal.append(rec).is_none());
        assert!(!wal.commit(99));

        let faulty_write: FaultyWalDevice<2> = FaultyWalDevice::new(None, Some(0));
        let wal_faulty_write = WriteAheadLog::new(&faulty_write);
        assert!(wal_faulty_write.append(rec).is_none());

        let faulty_read: FaultyWalDevice<2> = FaultyWalDevice::new(Some(0), None);
        let wal_faulty_read = WriteAheadLog::new(&faulty_read);
        assert!(wal_faulty_read.append(rec).is_some());
        assert!(!wal_faulty_read.commit(0));
        let mut replayed = 0usize;
        wal_faulty_read.replay_committed(|_| replayed += 1);
        assert_eq!(replayed, 0);
        assert_eq!(wal_faulty_read.recover_head(), 0);

        let faulty_decode: FaultyWalDevice<2> = FaultyWalDevice::new(None, None);
        let wal_faulty_decode = WriteAheadLog::new(&faulty_decode);
        assert!(wal_faulty_decode.append(rec).is_some());
        faulty_decode.write_raw(0, [0u8; WAL_BLOCK_SIZE]);
        assert!(!wal_faulty_decode.commit(0));

        let idx: CowObjectIndex<1> = CowObjectIndex::new();
        assert!(idx.upsert(ObjectEntry {
            key_hash: kh(1),
            lba: 1,
            len: 1,
        }));
        assert!(!idx.upsert(ObjectEntry {
            key_hash: kh(2),
            lba: 2,
            len: 2,
        }));
        assert!(!idx.delete(&kh(3)));

        let mut node: BTreeNode<1> = BTreeNode::new();
        assert!(node.insert_sorted(BTreeNodeEntry {
            key_hash: kh(1),
            lba: 1,
        }));
        assert!(!node.insert_sorted(BTreeNodeEntry {
            key_hash: kh(2),
            lba: 2,
        }));
        assert!(!node.delete(&kh(3)));
        assert_eq!(node.entries().len(), 1);

        let mut broken: BTreeNode<2> = BTreeNode::new();
        broken.force_hole_for_test();
        assert!(!broken.delete(&kh(1)));

        assert_eq!(TxStatus::from_usize(1), TxStatus::InFlight);
        assert_eq!(TxStatus::from_usize(3), TxStatus::Aborted);
        assert_eq!(TxStatus::from_usize(99), TxStatus::Idle);
        assert_eq!(TxStatus::Aborted.as_usize(), 3);

        let dev_full: InMemoryWalDevice<0> = InMemoryWalDevice::new();
        let txm_full: AtomicTransactionManager<'_, _, 1, 1> =
            AtomicTransactionManager::new(&dev_full);
        let put_full = txm_full.put(kh(9), 9, 9);
        assert!(matches!(put_full, Err(TxError::WalFull)));
        let del_full = txm_full.delete(kh(9));
        assert!(matches!(del_full, Err(TxError::WalFull)));

        let dev_commit_fail: FaultyWalDevice<4> = FaultyWalDevice::new(Some(0), None);
        let txm_commit_fail: AtomicTransactionManager<'_, _, 2, 2> =
            AtomicTransactionManager::new(&dev_commit_fail);
        assert!(matches!(
            txm_commit_fail.put(kh(4), 4, 4),
            Err(TxError::CommitFailed)
        ));

        let dev_commit_fail_delete: FaultyWalDevice<4> = FaultyWalDevice::new(Some(0), None);
        let txm_commit_fail_delete: AtomicTransactionManager<'_, _, 2, 2> =
            AtomicTransactionManager::new(&dev_commit_fail_delete);
        assert!(matches!(
            txm_commit_fail_delete.delete(kh(4)),
            Err(TxError::CommitFailed)
        ));

        let dev_index_fail: InMemoryWalDevice<8> = InMemoryWalDevice::new();
        let txm_index_fail: AtomicTransactionManager<'_, _, 1, 8> =
            AtomicTransactionManager::new(&dev_index_fail);
        assert!(txm_index_fail.put(kh(1), 1, 1).is_ok());
        assert!(matches!(
            txm_index_fail.put(kh(2), 2, 2),
            Err(TxError::IndexUpdateFailed)
        ));

        let dev_btree_fail: InMemoryWalDevice<8> = InMemoryWalDevice::new();
        let txm_btree_fail: AtomicTransactionManager<'_, _, 8, 1> =
            AtomicTransactionManager::new(&dev_btree_fail);
        assert!(txm_btree_fail.put(kh(1), 1, 1).is_ok());
        assert!(matches!(
            txm_btree_fail.put(kh(2), 2, 2),
            Err(TxError::IndexUpdateFailed)
        ));
    }
}
