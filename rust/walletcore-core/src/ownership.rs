//! Pure ownership work on blocks already authenticated by the range decoder.
//! The wallet retains chain authority, transaction evidence, balances and persistence.
//! Mailbox generations and release/acquire publication belong to the caller.
use alloc::{collections::BTreeMap, vec::Vec};
use monero_wallet::{
    ed25519::{CompressedPoint, Scalar},
    primitives::keccak256,
};
use zeroize::{Zeroize, Zeroizing};

use crate::{
    RangeBlock, RangeError, Transaction, ViewPair, WalletOutput,
    block_range::{Budget, MAX_BLOB, decode_block, read_pruned},
    scan_assembled_strict,
};

pub const MAX_SCAN_JOB: usize = 2 * 1024 * 1024;
pub const MAX_SCAN_RESULT: usize = 1024 * 1024;
// Four-byte versioned kind and Keccak-256 of the remaining body. This detects
// transport corruption; the reader must still check the expected ticket and block.
const HEADER: usize = 36;
type Hash = [u8; 32];
type R<T> = Result<T, OwnershipError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnershipError {
    Limit,
    Encoding,
    InvalidBlock,
    Scan,
}
impl From<RangeError> for OwnershipError {
    fn from(err: RangeError) -> Self {
        if err.is_limit() {
            Self::Limit
        } else {
            Self::InvalidBlock
        }
    }
}

pub struct ScannedBlock {
    pub height: u64,
    pub hash: Hash,
    pub previous: Hash,
    pub miner_hash: Hash,
    /// Non-miner transaction hashes in block order for pending/history evidence.
    pub transactions: Vec<Hash>,
    pub outputs: Vec<WalletOutput>,
}

fn validate(b: &RangeBlock) -> R<()> {
    if b.txs.len() != b.block.transactions.len() { return Err(OwnershipError::InvalidBlock); }
    let miner = Transaction::<crate::Pruned>::from(b.block.miner_transaction().clone());
    let txs = core::iter::once(&miner).chain(&b.txs);
    let legacy = txs.clone().any(|tx| tx.version() == 1 && !tx.prefix().outputs.is_empty());
    if legacy != !b.output_indices.is_empty()
        || (legacy && b.output_indices.len() != b.txs.len() + 1) {
        return Err(OwnershipError::InvalidBlock);
    }
    let mut buckets = BTreeMap::new();
    let mut n = 0u64;
    let mut first_zero = None;
    let mut next_zero = None;
    for (t, tx) in txs.enumerate() {
        let row = b.output_indices.get(t);
        if row.is_some_and(|r| r.len() != tx.prefix().outputs.len()) { return Err(OwnershipError::InvalidBlock); }
        for (o, output) in tx.prefix().outputs.iter().enumerate() {
            let amount = if tx.version() == 2 { 0 } else { output.amount.ok_or(OwnershipError::InvalidBlock)? };
            if amount == 0 { n = n.checked_add(1).ok_or(OwnershipError::InvalidBlock)?; }
            if let Some(row) = row {
                let index = row[o];
                let next = index.checked_add(1).ok_or(OwnershipError::InvalidBlock)?;
                if buckets.insert(amount, next).is_some_and(|expected| expected != index) { return Err(OwnershipError::InvalidBlock); }
                if buckets.len() > 4096 { return Err(OwnershipError::Limit); }
                if amount == 0 { first_zero.get_or_insert(index); next_zero = Some(next); }
            }
        }
    }
    if legacy && (first_zero != b.first_rct || next_zero != b.next_rct) { return Err(OwnershipError::InvalidBlock); }
    match (b.first_rct, b.next_rct, n) {
        (None, None, 0) => Ok(()),
        (Some(first), Some(next), n) if n > 0 && first.checked_add(n) == Some(next) => Ok(()),
        _ => Err(OwnershipError::InvalidBlock),
    }
}

/// Check worker receipts against the authenticated source shapes and indices.
/// Ownership derivations remain the worker's job; it has no commit authority.
pub fn validate_scanned_block(result: &ScannedBlock, b: &RangeBlock) -> bool {
    if result.height != b.block.number() as u64 || result.hash != b.block.hash()
        || result.previous != b.block.header.previous || result.miner_hash != b.block.miner_transaction().hash()
        || result.transactions != b.block.transactions { return false; }
    let miner = Transaction::<crate::Pruned>::from(b.block.miner_transaction().clone());
    let mut txs = core::iter::once(&miner).chain(&b.txs).enumerate();
    let mut current = txs.next();
    let mut base = b.first_rct.unwrap_or(0);
    for output in &result.outputs {
        loop {
            let Some((t, tx)) = current else { return false; };
            let hash = if t == 0 { result.miner_hash } else { result.transactions[t - 1] };
            if hash == output.transaction() { break; }
            if tx.version() == 2 { base = match base.checked_add(tx.prefix().outputs.len() as u64) { Some(i) => i, None => return false }; }
            current = txs.next();
        }
        let (t, tx) = current.unwrap();
        let Ok(local) = usize::try_from(output.index_in_transaction()) else { return false; };
        let Some(source) = tx.prefix().outputs.get(local) else { return false; };
        let index = if let Some(row) = b.output_indices.get(t) { row.get(local).copied() } else { base.checked_add(local as u64) };
        if source.key != output.key().compress() || index != Some(output.index_on_blockchain())
            || output.is_legacy() != (tx.version() == 1)
            || output.amount_bucket() != if tx.version() == 1 { source.amount.unwrap_or(u64::MAX) } else { 0 }
            || source.amount.is_some_and(|amount| amount != output.commitment().amount) { return false; }
    }
    true
}

pub fn scan_verified_block(view: &ViewPair, item: RangeBlock) -> R<ScannedBlock> {
    validate(&item)?;
    let height = item.block.number() as u64;
    let hash = item.block.hash();
    let previous = item.block.header.previous;
    let miner_hash = item.block.miner_transaction().hash();
    let transactions = item.block.transactions.clone();
    let outputs = if item.output_indices.is_empty() {
        scan_assembled_strict(view, item.block, item.txs, item.first_rct)
    } else {
        let block = monero_wallet::interface::ScannableBlock {
            block: item.block, transactions: item.txs,
            output_index_for_first_ringct_output: item.first_rct,
        };
        crate::Scanner::new(view.clone()).scan_with_output_indices(block, &item.output_indices)
            .map(|outputs| outputs.ignore_additional_timelock()).map_err(|_| ())
    }.map_err(|_| OwnershipError::Scan)?;
    Ok(ScannedBlock {
        height,
        hash,
        previous,
        miner_hash,
        transactions,
        outputs,
    })
}

struct Writer<'a> {
    data: &'a mut [u8],
    pos: usize,
}
impl Writer<'_> {
    fn put(&mut self, value: &[u8]) -> R<()> {
        let end = self
            .pos
            .checked_add(value.len())
            .ok_or(OwnershipError::Limit)?;
        self.data
            .get_mut(self.pos..end)
            .ok_or(OwnershipError::Limit)?
            .copy_from_slice(value);
        self.pos = end;
        Ok(())
    }
    fn count(&mut self, size: usize) -> R<()> {
        self.put(
            &u32::try_from(size)
                .map_err(|_| OwnershipError::Limit)?
                .to_le_bytes(),
        )
    }
    fn record(&mut self, bytes: &[u8], max: usize) -> R<()> {
        if bytes.len() > max {
            return Err(OwnershipError::Limit);
        }
        self.count(bytes.len())?;
        self.put(bytes)
    }
    fn index(&mut self, i: Option<u64>) -> R<()> {
        self.put(&[u8::from(i.is_some())])?;
        if let Some(i) = i {
            self.put(&i.to_le_bytes())?;
        }
        Ok(())
    }
}

fn pack(
    dest: &mut [u8],
    magic: &[u8; 4],
    max: usize,
    f: impl FnOnce(&mut Writer<'_>) -> R<()>,
) -> R<usize> {
    let available = dest.len().min(max);
    if available < HEADER {
        dest.zeroize();
        return Err(OwnershipError::Limit);
    }
    let mut w = Writer {
        data: &mut dest[..available],
        pos: HEADER,
    };
    w.data[..HEADER].fill(0);
    if let Err(err) = f(&mut w) {
        w.data[..w.pos].zeroize();
        return Err(err);
    }
    let sum = keccak256(&w.data[HEADER..w.pos]);
    w.data[4..HEADER].copy_from_slice(&sum);
    w.data[..4].copy_from_slice(magic);
    Ok(w.pos)
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn open(bytes: &'a [u8], magic: &[u8; 4], max: usize) -> R<Self> {
        if bytes.len() > max {
            return Err(OwnershipError::Limit);
        }
        let (header, body) = bytes
            .split_at_checked(HEADER)
            .ok_or(OwnershipError::Encoding)?;
        if header[..4] != magic[..] || header[4..] != keccak256(body) {
            return Err(OwnershipError::Encoding);
        }
        Ok(Self(body))
    }
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        let (value, rest) = self.0.split_at_checked(n).ok_or(OwnershipError::Encoding)?;
        self.0 = rest;
        Ok(value)
    }
    fn fixed<const N: usize>(&mut self) -> R<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn count(&mut self) -> R<usize> {
        Ok(u32::from_le_bytes(self.fixed()?) as usize)
    }
    fn blob(&mut self, limit: usize) -> R<&'a [u8]> {
        let n = self.count()?;
        if n > limit {
            return Err(OwnershipError::Limit);
        }
        self.take(n)
    }
    fn index(&mut self) -> R<Option<u64>> {
        match self.fixed::<1>()? {
            [0] => Ok(None),
            [1] => Ok(Some(u64::from_le_bytes(self.fixed()?))),
            _ => Err(OwnershipError::Encoding),
        }
    }
    fn finish(self) -> R<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(OwnershipError::Encoding)
        }
    }
}

/// Encode a view-only scan job. Treat the returned bytes as secret and clear
/// the shared mailbox once the worker has consumed it.
pub fn encode_scan_job(view: &ViewPair, item: &RangeBlock, dest: &mut [u8]) -> R<usize> {
    let header = HEADER.min(dest.len());
    dest[..header].zeroize();
    validate(item)?;
    pack(dest, b"NXJ2", MAX_SCAN_JOB, |w| {
        w.put(&view.spend().compress().to_bytes())?;
        w.put(view.private_view_key_bytes().as_ref())?;
        w.index(item.first_rct)?;
        w.index(item.next_rct)?;
        w.record(&item.block.serialize(), MAX_BLOB)?;
        // The block itself already gives the exact transaction count.
        for tx in &item.txs {
            w.record(&tx.serialize(), MAX_BLOB)?;
        }
        w.count(item.output_indices.len())?;
        for row in &item.output_indices {
            w.count(row.len())?;
            for index in row { w.put(&index.to_le_bytes())?; }
        }
        Ok(())
    })
}

fn decode_job(job: &[u8]) -> R<(ViewPair, RangeBlock)> {
    let mut r = Reader::open(job, b"NXJ2", MAX_SCAN_JOB)?;
    let spend = CompressedPoint::from(r.fixed::<32>()?)
        .decompress()
        .ok_or(OwnershipError::Encoding)?;
    let scalar =
        Zeroizing::new(Scalar::read(&mut r.take(32)?).map_err(|_| OwnershipError::Encoding)?);
    if *scalar == Scalar::ZERO {
        return Err(OwnershipError::Encoding);
    }
    let view = ViewPair::new(spend, scalar).map_err(|_| OwnershipError::Encoding)?;
    let first_rct = r.index()?;
    let next_rct = r.index()?;
    let mut budget = Budget::new();
    let block = decode_block(r.blob(MAX_BLOB)?, &mut budget)?;
    let count = block.transactions.len();
    budget.charge(count * core::mem::size_of::<Transaction<crate::Pruned>>())?;
    let mut txs = Vec::new();
    txs.try_reserve_exact(count)
        .map_err(|_| OwnershipError::Limit)?;
    for _ in &block.transactions {
        let (tx, rest) = read_pruned(r.blob(MAX_BLOB)?, &mut budget)?;
        if !rest.is_empty() {
            return Err(OwnershipError::Encoding);
        }
        txs.push(tx);
    }
    let rows = r.count()?;
    if rows != 0 && rows != count + 1 { return Err(OwnershipError::InvalidBlock); }
    budget.charge(rows * core::mem::size_of::<Vec<u64>>())?;
    let mut output_indices = Vec::new();
    output_indices.try_reserve_exact(rows).map_err(|_| OwnershipError::Limit)?;
    for t in 0..rows {
        let n = r.count()?;
        let expected = if t == 0 { block.miner_transaction().prefix().outputs.len() } else { txs[t - 1].prefix().outputs.len() };
        if n != expected { return Err(OwnershipError::InvalidBlock); }
        budget.charge(n * core::mem::size_of::<u64>())?;
        let mut row = Vec::new();
        row.try_reserve_exact(n).map_err(|_| OwnershipError::Limit)?;
        for _ in 0..n { row.push(u64::from_le_bytes(r.fixed()?)); }
        output_indices.push(row);
    }
    r.finish()?;
    Ok((
        view,
        RangeBlock {
            block,
            txs,
            first_rct,
            next_rct,
            output_indices,
        },
    ))
}

/// Execute a complete job into a separate result mailbox. No wallet state
/// is changed. Receipt bytes contain wallet metadata and need the same clearing
/// discipline as the view-only job.
pub fn execute_scan_job(job: &[u8], dest: &mut [u8]) -> R<usize> {
    execute_job(job, dest, || {})
}

/// Mark decode completion and ownership-scan completion on the worker's clock.
#[cfg(feature = "worker-profile")]
pub fn execute_profiled_scan_job(job: &[u8], dest: &mut [u8], mark: impl FnMut()) -> R<usize> {
    execute_job(job, dest, mark)
}

#[inline(always)]
fn execute_job(job: &[u8], dest: &mut [u8], mut mark: impl FnMut()) -> R<usize> {
    let header = HEADER.min(dest.len());
    dest[..header].zeroize();
    let (view, item) = decode_job(job)?;
    // The scanner copies arbitrary extra data into each owned receipt. Bound
    // that temporary allocation even if every output belongs to this view.
    let mut work = 0usize;
    let mut charge = |prefix: &monero_wallet::transaction::TransactionPrefix| -> R<()> {
        let unit = 512usize.saturating_add(prefix.extra.len().saturating_mul(16));
        work = work.saturating_add(prefix.outputs.len().saturating_mul(unit));
        if work > 8 * 1024 * 1024 {
            return Err(OwnershipError::Limit);
        }
        Ok(())
    };
    charge(item.block.miner_transaction().prefix())?;
    for tx in &item.txs {
        charge(tx.prefix())?;
    }
    mark();
    let result = scan_verified_block(&view, item)?;
    mark();
    pack(dest, b"NXR1", MAX_SCAN_RESULT, |w| {
        w.put(&result.height.to_le_bytes())?;
        w.put(&result.hash)?;
        w.put(&result.previous)?;
        w.put(&result.miner_hash)?;
        w.count(result.transactions.len())?;
        for id in &result.transactions {
            w.put(id)?;
        }
        w.count(result.outputs.len())?;
        for output in &result.outputs {
            w.record(&Zeroizing::new(output.serialize()), MAX_SCAN_RESULT)?;
        }
        Ok(())
    })
}

pub fn decode_scan_result(src: &[u8]) -> R<ScannedBlock> {
    let mut r = Reader::open(src, b"NXR1", MAX_SCAN_RESULT)?;
    let height = u64::from_le_bytes(r.fixed()?);
    let hash = r.fixed()?;
    let previous = r.fixed()?;
    let miner_hash = r.fixed()?;
    let n = r.count()?;
    if n > MAX_BLOB / 32 {
        return Err(OwnershipError::Limit);
    }
    let transactions: Vec<Hash> = r
        .take(n * 32)?
        .chunks_exact(32)
        .map(|h| h.try_into().unwrap())
        .collect();
    let n = r.count()?;
    // A serialized receipt always includes a 32-byte transaction ID and key.
    if n > r.0.len() / 64 {
        return Err(OwnershipError::Encoding);
    }
    let mut extra_budget = (4 * MAX_SCAN_RESULT)
        .checked_sub(n * core::mem::size_of::<WalletOutput>())
        .ok_or(OwnershipError::Limit)?;
    let mut outputs = Vec::new();
    outputs
        .try_reserve_exact(n)
        .map_err(|_| OwnershipError::Limit)?;
    let mut expected_txs = core::iter::once(&miner_hash).chain(&transactions);
    let mut current = expected_txs.next();
    let mut previous_indices = BTreeMap::new();
    let mut previous_local = None;
    for _ in 0..n {
        let bytes = r.blob(MAX_SCAN_RESULT)?;
        let mut input = bytes;
        let output = WalletOutput::read(&mut input).map_err(|_| OwnershipError::Encoding)?;
        if !input.is_empty() || Zeroizing::new(output.serialize()).as_slice() != bytes {
            return Err(OwnershipError::Encoding);
        }
        // Empty arbitrary-data chunks still occupy Vec headers in the decoded receipt.
        let data = output.arbitrary_data();
        let overhead = data.len() * 2 * core::mem::size_of::<Vec<u8>>();
        extra_budget = extra_budget
            .checked_sub(overhead + data.iter().map(Vec::capacity).sum::<usize>())
            .ok_or(OwnershipError::Limit)?;
        let txid = output.transaction();
        let local = output.index_in_transaction();
        if previous_local.is_some_and(|(id, i)| id == txid && i >= local) {
            return Err(OwnershipError::InvalidBlock);
        }
        previous_local = Some((txid, local));
        while current.is_some_and(|id| id != &txid) {
            current = expected_txs.next();
        }
        let index = output.index_on_blockchain();
        if current.is_none() || previous_indices.get(&output.amount_bucket()).is_some_and(|i| *i >= index) {
            return Err(OwnershipError::InvalidBlock);
        }
        previous_indices.insert(output.amount_bucket(), index);
        outputs.push(output);
    }
    r.finish()?;
    Ok(ScannedBlock {
        height,
        hash,
        previous,
        miner_hash,
        transactions,
        outputs,
    })
}
