//! Pruned Monero daemon block ranges. Results share one continuous output-index sequence
//! per amount bucket. The caller still anchors the first block to the saved chain.
use crate::{Block, Pruned, Transaction};
use alloc::{collections::BTreeMap, vec::Vec};
use core::{cell::Cell, mem::size_of};
use monero_epee::{Array, BytesLike, Epee, EpeeEntry, EpeeError, Type};
use monero_wallet::{
    block::BlockHeader,
    io::VarInt,
    primitives::keccak256,
    transaction::{Input, Output, TransactionPrefix},
};

// UTM's 32 MiB coordinator can accept a larger body for dense historical ranges.
const UTM_RANGE: bool = cfg!(feature = "utm-range-74");
pub const MAX_RANGE_BLOCKS: usize = if UTM_RANGE { 74 } else { 75 };
pub const MAX_RANGE_BODY: usize = (if UTM_RANGE { 4 } else { 2 }) * 1024 * 1024;
// Reserve space in the guest for two HTTP bodies, the scanner and the current decode.
// Bound each individual parse before calling the Monero deserializer, then charge actual
// Vec capacities against the aggregate retained budget.
const MAX_RETAINED: usize = 8 * 1024 * 1024;
pub(super) const MAX_BLOB: usize = 256 * 1024;
const MAX_INPUTS_PER_TX: usize = 2048;
const MAX_AMOUNT_BUCKETS: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeError {
    Limit,
    Encoding,
    Daemon,
    Busy,
    Height,
    Link,
    Block,
    Transaction,
    Hash,
    Indices,
    MissingProof,
}

impl RangeError {
    pub fn is_busy(self) -> bool {
        self == Self::Busy
    }
    pub fn is_limit(self) -> bool {
        self == Self::Limit
    }
    pub fn needs_full_transactions(self) -> bool {
        self == Self::MissingProof
    }
}
impl From<EpeeError> for RangeError {
    fn from(_: EpeeError) -> Self {
        Self::Encoding
    }
}

#[derive(Debug)]
pub struct RangeBlock {
    pub block: Block,
    pub txs: Vec<Transaction<Pruned>>,
    pub first_rct: Option<u64>,
    /// Exclusive end of this block's RingCT outputs (None when the block has none).
    pub next_rct: Option<u64>,
    /// Exact amount-bucket indices, miner first. Retained only for blocks with
    /// transparent (V1) outputs; modern-only blocks use the compact RingCT span.
    pub output_indices: Vec<Vec<u64>>,
}

/// Encode a positive, bounded range. Invalid ranges yield an empty body so they
/// cannot be sent as Monero's unbounded (zero-count) request.
pub fn encode_blocks_request(start: u64, count: usize) -> Vec<u8> {
    if start == 0
        || count == 0
        || count > MAX_RANGE_BLOCKS
        || start.checked_add(count as u64).is_none()
    {
        return Vec::new();
    }
    let mut body = Vec::with_capacity(65);
    body.extend(monero_epee::HEADER);
    body.push(monero_epee::VERSION);
    body.push(3 << 2);
    body.extend(b"\x05prune");
    body.extend([Type::Bool as u8, 1]);
    body.extend(b"\x0cstart_height");
    body.push(Type::Uint64 as u8);
    body.extend(start.to_le_bytes());
    body.extend(b"\x0fmax_block_count");
    body.push(Type::Uint64 as u8);
    body.extend((count as u64).to_le_bytes());
    body
}

// The vendored decoder accepts any BytesLike source. Keep the remaining wire bytes after
// each key to distinguish a scalar from a one-element array, and count root bytes for EOF.
struct Bytes<'a> {
    slice: &'a [u8],
    after: &'a [u8],
    consumed: Option<&'a Cell<usize>>,
}

impl<'a> BytesLike<'a> for Bytes<'a> {
    type ExternallyTrackedLength = ();
    fn len(&self, (): ()) -> usize {
        self.slice.len()
    }
    fn read_bytes(&mut self, n: usize) -> Result<((), Self), EpeeError> {
        let (value, rest) = self.slice.split_at_checked(n).ok_or(EpeeError::Short(n))?;
        self.slice = rest;
        if let Some(count) = self.consumed {
            count.set(count.get() + n);
        }
        Ok((
            (),
            Self {
                slice: value,
                after: rest,
                consumed: None,
            },
        ))
    }
    fn read_into_slice(&mut self, out: &mut [u8]) -> Result<(), EpeeError> {
        out.copy_from_slice(self.read_bytes(out.len())?.1.slice);
        Ok(())
    }
}

type Entry<'a, 'p> = EpeeEntry<'a, 'p, Bytes<'a>>;
type R<T> = Result<T, RangeError>;

struct Field<'a, 'p> {
    entry: Entry<'a, 'p>,
    wire: &'a [u8],
}
impl<'a> Field<'a, '_> {
    fn typed(&self, kind: Type, is_array: bool) -> R<()> {
        let flag = if is_array { Array::Array as u8 } else { 0 };
        if self.wire.first() != Some(&((kind as u8) | flag)) {
            return Err(RangeError::Encoding);
        }
        Ok(())
    }
    fn string(self) -> R<&'a [u8]> {
        self.typed(Type::String, false)?;
        Ok(self.entry.to_str()?.consume().slice)
    }
    fn uint(self) -> R<u64> {
        self.typed(Type::Uint64, false)?;
        Ok(self.entry.to_u64()?)
    }
    fn boolean(self) -> R<bool> {
        self.typed(Type::Bool, false)?;
        if !matches!(self.wire.get(1), Some(0 | 1)) {
            return Err(RangeError::Encoding);
        }
        Ok(self.entry.to_bool()?)
    }
    fn objects(
        self,
        limit: usize,
        mut visit: impl FnMut(usize, Entry<'a, '_>) -> R<()>,
    ) -> R<usize> {
        self.typed(Type::Object, true)?;
        let n = self.entry.len();
        if n > limit {
            return Err(RangeError::Limit);
        }
        let mut iter = self.entry.iterate()?;
        for i in 0..n {
            visit(i, iter.next().ok_or(RangeError::Encoding)??)?;
        }
        Ok(n)
    }
}

fn fields<'a>(
    entry: Entry<'a, '_>,
    mut visit: impl FnMut(&'a [u8], Field<'a, '_>) -> R<()>,
) -> R<()> {
    let mut fields = entry.fields()?;
    let mut names: [&[u8]; 32] = [&[]; 32];
    let mut count = 0;
    while let Some(pair) = fields.next() {
        let (key, entry) = pair?;
        let key = key.consume();
        if count == names.len() {
            return Err(RangeError::Limit);
        }
        if names[..count].contains(&key.slice) {
            return Err(RangeError::Encoding);
        }
        names[count] = key.slice;
        count += 1;
        visit(
            key.slice,
            Field {
                entry,
                wire: key.after,
            },
        )?;
    }
    Ok(())
}

fn root(body: &[u8], visit: impl FnMut(&[u8], Field<'_, '_>) -> R<()>) -> R<()> {
    root_counted(body, &Cell::new(0), visit)
}

fn root_counted(
    body: &[u8],
    count: &Cell<usize>,
    visit: impl FnMut(&[u8], Field<'_, '_>) -> R<()>,
) -> R<()> {
    let bytes = Bytes {
        slice: body,
        after: &[],
        consumed: Some(count),
    };
    let mut epee = Epee::new(bytes)?;
    fields(epee.entry()?, visit)?;
    if count.get() != body.len() {
        return Err(RangeError::Encoding);
    }
    Ok(())
}

pub(super) struct Budget(usize);
impl Budget {
    pub(super) fn new() -> Self {
        Self(MAX_RETAINED)
    }
    pub(super) fn charge(&mut self, amount: usize) -> R<()> {
        self.0 = self.0.checked_sub(amount).ok_or(RangeError::Limit)?;
        Ok(())
    }
    fn vector<T>(&mut self, v: &Vec<T>) -> R<()> {
        self.charge(v.capacity() * size_of::<T>() + 32)
    }
    fn prefix(&mut self, prefix: &TransactionPrefix) -> R<()> {
        self.vector(&prefix.inputs)?;
        self.vector(&prefix.outputs)?;
        self.vector(&prefix.extra)?;
        for input in &prefix.inputs {
            if let Input::ToKey { key_offsets, .. } = input {
                self.vector(key_offsets)?;
            }
        }
        Ok(())
    }
    fn transaction(&mut self, tx: &Transaction<Pruned>) -> R<()> {
        self.prefix(tx.prefix())?;
        if let Transaction::V2 {
            proofs: Some(p), ..
        } = tx
        {
            self.vector(&p.base.pseudo_outs)?;
            self.vector(&p.base.encrypted_amounts)?;
            self.vector(&p.base.commitments)?;
        }
        Ok(())
    }
}

fn check_blob(bytes: &[u8]) -> R<()> {
    if bytes.len() > MAX_BLOB {
        return Err(RangeError::Limit);
    }
    Ok(())
}

fn check_inputs(mut bytes: &[u8], miner: bool) -> R<()> {
    let _: u64 = VarInt::read(&mut bytes).map_err(|_| RangeError::Transaction)?;
    let _: u64 = VarInt::read(&mut bytes).map_err(|_| RangeError::Transaction)?;
    let count: usize = VarInt::read(&mut bytes).map_err(|_| RangeError::Transaction)?;
    if count > MAX_INPUTS_PER_TX {
        return Err(RangeError::Limit);
    }
    if count == 0 || (miner && (count != 1 || bytes.first() != Some(&255))) {
        return Err(RangeError::Transaction);
    }
    Ok(())
}

pub(super) fn decode_block(bytes: &[u8], budget: &mut Budget) -> R<Block> {
    check_blob(bytes)?;
    let mut input = bytes;
    let header = BlockHeader::read(&mut input).map_err(|_| RangeError::Block)?;
    check_inputs(input, true)?;
    // Miner transactions have neither signatures nor a prunable RingCT component.
    let miner = Transaction::<Pruned>::read(&mut input).map_err(|_| RangeError::Block)?;
    budget.transaction(&miner)?;
    let miner = match miner {
        Transaction::V1 { prefix, .. } => Transaction::V1 {
            prefix,
            signatures: Vec::new(),
        },
        Transaction::V2 {
            prefix,
            proofs: None,
        } => Transaction::V2 {
            prefix,
            proofs: None,
        },
        _ => return Err(RangeError::Block),
    };
    let count: usize = VarInt::read(&mut input).map_err(|_| RangeError::Block)?;
    if count.checked_mul(32) != Some(input.len()) || count >= Block::MAX_TRANSACTIONS {
        return Err(RangeError::Block);
    }
    let hashes = input
        .chunks_exact(32)
        .map(|h| h.try_into().unwrap())
        .collect();
    budget.vector(&hashes)?;
    Block::new(header, miner, hashes).ok_or(RangeError::Block)
}

pub(super) fn read_pruned<'a>(
    blob: &'a [u8],
    budget: &mut Budget,
) -> R<(Transaction<Pruned>, &'a [u8])> {
    check_blob(blob)?;
    check_inputs(blob, false)?;
    let mut input = blob;
    let tx = Transaction::<Pruned>::read(&mut input).map_err(|_| RangeError::Transaction)?;
    budget.transaction(&tx)?;
    if tx
        .prefix()
        .inputs
        .iter()
        .any(|i| matches!(i, Input::Gen(_)))
    {
        return Err(RangeError::Transaction);
    }
    Ok((tx, input))
}

fn transaction(entry: Entry<'_, '_>, budget: &mut Budget) -> R<(Transaction<Pruned>, [u8; 32])> {
    let (mut blob, mut proof) = (None, None);
    fields(entry, |key, field| {
        match key {
            b"blob" => blob = Some(field.string()?),
            b"prunable_hash" => {
                proof = Some(<[u8; 32]>::try_from(field.string()?).map_err(|_| RangeError::Hash)?)
            }
            _ => {}
        }
        Ok(())
    })?;
    let blob = blob.ok_or(RangeError::Transaction)?;
    let (tx, input) = read_pruned(blob, budget)?;
    let hash = match &tx {
        Transaction::V1 { prefix, .. } => {
            if proof.is_some_and(|hash| hash != [0; 32]) {
                return Err(RangeError::Hash);
            }
            // V1 hashes cover the complete serialization. A pruned V1 prefix by itself
            // cannot authenticate the output keys, so obtain a full transaction instead.
            let signatures: usize = prefix
                .inputs
                .iter()
                .map(|i| match i {
                    Input::ToKey { key_offsets, .. } => key_offsets.len(),
                    Input::Gen(_) => 0,
                })
                .sum();
            if input.is_empty() && signatures != 0 {
                return Err(RangeError::MissingProof);
            }
            if signatures == 0 || signatures.checked_mul(64) != Some(input.len()) {
                return Err(RangeError::Transaction);
            }
            keccak256(blob)
        }
        Transaction::V2 {
            proofs: Some(_), ..
        } => {
            if !input.is_empty() {
                return Err(RangeError::Transaction);
            }
            let proof = proof
                .filter(|h| *h != [0; 32])
                .ok_or(RangeError::MissingProof)?;
            tx.hash_with_prunable_hash(proof).ok_or(RangeError::Hash)?
        }
        _ => return Err(RangeError::Transaction),
    };
    Ok((tx, hash))
}

fn range_block(entry: Entry<'_, '_>, budget: &mut Budget) -> R<RangeBlock> {
    let (mut pruned, mut block) = (false, None);
    let (mut txs, mut hashes) = (Vec::new(), Vec::new());
    fields(entry, |key, field| {
        match key {
            b"pruned" => pruned = field.boolean()?,
            b"block" => block = Some(decode_block(field.string()?, budget)?),
            b"txs" => {
                field.typed(Type::Object, true)?;
                let count = field.entry.len();
                // Each transaction must also have a distinct hash in a bounded block blob.
                if count > MAX_BLOB / 32 {
                    return Err(RangeError::Limit);
                }
                budget.charge(count * size_of::<Transaction<Pruned>>() + 32)?;
                txs.try_reserve_exact(count)
                    .map_err(|_| RangeError::Limit)?;
                field.objects(count, |_, item| {
                    let (tx, hash) = transaction(item, budget)?;
                    txs.push(tx);
                    hashes.push(hash);
                    Ok(())
                })?;
            }
            _ => {}
        }
        Ok(())
    })?;
    let block = block.ok_or(RangeError::Block)?;
    if !pruned {
        return Err(RangeError::MissingProof);
    }
    if block.transactions != hashes {
        return Err(RangeError::Hash);
    }
    Ok(RangeBlock {
        block,
        txs,
        first_rct: None,
        next_rct: None,
        output_indices: Vec::new(),
    })
}

fn indices(
    entry: Entry<'_, '_>,
    block: &mut RangeBlock,
    buckets: &mut BTreeMap<u64, u64>,
    budget: &mut Budget,
) -> R<()> {
    let expected = block.txs.len() + 1;
    let retain = (block.block.miner_transaction().version() == 1
        && !block.block.miner_transaction().prefix().outputs.is_empty())
        || block.txs.iter().any(|tx| tx.version() == 1 && !tx.prefix().outputs.is_empty());
    if retain {
        budget.charge(expected * size_of::<Vec<u64>>())?;
        block.output_indices.try_reserve_exact(expected).map_err(|_| RangeError::Limit)?;
    }
    let mut seen = None;
    fields(entry, |key, field| {
        if key != b"indices" {
            return Ok(());
        }
        if field.entry.len() != expected {
            return Err(RangeError::Indices);
        }
        seen = Some(field.objects(expected, |n, item| {
            let (rct, outputs) = if n == 0 {
                let tx = block.block.miner_transaction();
                (tx.version() == 2, &tx.prefix().outputs)
            } else {
                let tx = &block.txs[n - 1];
                (tx.version() == 2, &tx.prefix().outputs)
            };
            let mut indexed = false;
            let mut retained = Vec::new();
            if retain {
                budget.charge(outputs.len() * size_of::<u64>())?;
                retained.try_reserve_exact(outputs.len()).map_err(|_| RangeError::Limit)?;
            }
            fields(item, |key, field| {
                if key != b"indices" {
                    return Ok(());
                }
                field.typed(Type::Uint64, true)?;
                if field.entry.len() != outputs.len() {
                    return Err(RangeError::Indices);
                }
                let mut indexes = field.entry.iterate()?;
                for Output { amount, .. } in outputs {
                    let i = indexes.next().ok_or(RangeError::Indices)??.to_u64()?;
                    let amount = if rct {
                        0
                    } else {
                        amount.ok_or(RangeError::Indices)?
                    };
                    let next = i.checked_add(1).ok_or(RangeError::Indices)?;
                    if let Some(expected) = buckets.insert(amount, next) {
                        if i != expected {
                            return Err(RangeError::Indices);
                        }
                    }
                    if buckets.len() > MAX_AMOUNT_BUCKETS {
                        return Err(RangeError::Limit);
                    }
                    if retain { retained.push(i); }
                    if amount == 0 {
                        block.first_rct.get_or_insert(i);
                        block.next_rct = Some(next);
                    }
                }
                indexed = true;
                Ok(())
            })?;
            if !indexed && !outputs.is_empty() {
                return Err(RangeError::Indices);
            }
            if retain { block.output_indices.push(retained); }
            Ok(())
        })?);
        Ok(())
    })?;
    if seen != Some(expected) {
        return Err(RangeError::Indices);
    }
    Ok(())
}

struct DecodedBlocks {
    blocks: Vec<RangeBlock>,
    budget: Budget,
    next_parent: Option<[u8; 32]>,
}

impl DecodedBlocks {
    fn new(count: usize) -> R<Self> {
        let mut blocks = Vec::new();
        blocks
            .try_reserve_exact(count)
            .map_err(|_| RangeError::Limit)?;
        let mut budget = Budget::new();
        budget.vector(&blocks)?;
        Ok(Self {
            blocks,
            budget,
            next_parent: None,
        })
    }

    fn push(&mut self, entry: Entry<'_, '_>, start: u64) -> R<()> {
        let block = range_block(entry, &mut self.budget)?;
        if start.checked_add(self.blocks.len() as u64) != Some(block.block.number() as u64) {
            return Err(RangeError::Height);
        }
        if self
            .next_parent
            .is_some_and(|hash| hash != block.block.header.previous)
        {
            return Err(RangeError::Link);
        }
        self.next_parent = Some(block.block.hash());
        self.blocks.push(block);
        Ok(())
    }
}

pub fn decode_block_range(body: &[u8], start: u64, count: usize) -> R<Vec<RangeBlock>> {
    finish_range(body, start, count, None, &[])
}

fn finish_range(
    body: &[u8],
    start: u64,
    count: usize,
    cached: Option<DecodedBlocks>,
    ends: &[usize],
) -> R<Vec<RangeBlock>> {
    if count == 0 || count > MAX_RANGE_BLOCKS || body.len() > MAX_RANGE_BODY {
        return Err(RangeError::Limit);
    }
    if start == 0 {
        return Err(RangeError::Height);
    }
    // Finish checking the entire envelope before honoring a daemon retry hint.
    // BUSY carries no range data. An omitted false untrusted flag is valid EPEE.
    let (mut status, mut has_data) = (Err(RangeError::Daemon), false);
    let (mut first, mut tip) = (None, None);
    root(body, |key, field| {
        match key {
            b"status" => {
                status = match field.string()? {
                    b"OK" => Ok(()),
                    b"BUSY" => Err(RangeError::Busy),
                    _ => Err(RangeError::Daemon),
                }
            }
            b"start_height" => first = Some(field.uint()?),
            b"current_height" => tip = Some(field.uint()?),
            b"untrusted" => {
                if field.boolean()? {
                    return Err(RangeError::Daemon);
                }
            }
            b"blocks" | b"output_indices" => {
                field.typed(Type::Object, true)?;
                has_data |= field.entry.len() != 0;
            }
            _ => {}
        }
        Ok(())
    })
    .map_err(|e| {
        if e == RangeError::Limit {
            RangeError::Encoding
        } else {
            e
        }
    })?;
    if status == Err(RangeError::Busy) && has_data {
        return Err(RangeError::Daemon);
    }
    status?;
    let mut decoded = match cached {
        Some(decoded) => decoded,
        None => DecodedBlocks::new(count)?,
    };
    let position = Cell::new(0);
    root_counted(body, &position, |key, field| {
        match key {
            b"blocks" => {
                field.objects(count, |i, entry| {
                    if let Some(end) = ends.get(i) {
                        // Both cursors must agree about every cached object's
                        // entire span, including any unrecognized nested fields.
                        drop(entry);
                        if position.get() != *end {
                            return Err(RangeError::Encoding);
                        }
                    } else {
                        decoded.push(entry, start)?;
                    }
                    Ok(())
                })?;
            }
            _ => {}
        }
        Ok(())
    })?;
    let mut result = decoded.blocks;
    let end = start
        .checked_add(result.len() as u64)
        .ok_or(RangeError::Height)?;
    if first != Some(start) || tip.is_none_or(|height| height < end) || result.is_empty() {
        return Err(RangeError::Height);
    }
    // EPEE objects are unordered. Make a zero-copy pass to validate the index
    // vectors directly against decoded output shapes without keeping another nested Vec.
    let mut buckets = BTreeMap::new();
    let mut checked = false;
    root(body, |key, field| {
        if key == b"output_indices" {
            if field.entry.len() != result.len() {
                return Err(RangeError::Indices);
            }
            field.objects(result.len(), |i, item| {
                indices(item, &mut result[i], &mut buckets, &mut decoded.budget)
            })?;
            checked = true;
        }
        Ok(())
    })?;
    if !checked {
        return Err(RangeError::Indices);
    }
    Ok(result)
}

#[derive(Clone, Copy)]
enum Prefix {
    Header,
    Blocks {
        offset: usize,
        count: usize,
        attempt_at: usize,
    },
    Stopped,
}

/// An append-only, bounded HTTP response. Work done on a prefix belongs to these
/// exact bytes. Only `finish` returns blocks, after checking the complete envelope
/// and index tables. The caller supplies body fragments after TLS and HTTP framing.
pub struct RangeBody {
    bytes: Vec<u8>,
    start: u64,
    count: usize,
    prefix: Prefix,
    decoded: Option<DecodedBlocks>,
    ends: Vec<usize>,
}

impl RangeBody {
    pub fn new(start: u64, count: usize) -> Self {
        Self {
            bytes: Vec::new(),
            start,
            count,
            prefix: Prefix::Header,
            decoded: None,
            ends: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    /// Reserve for a fixed response or an incoming chunk, with capped geometric
    /// growth for small chunks. Both capacity and allocation errors return `Limit`.
    pub fn reserve(&mut self, additional: usize) -> R<()> {
        let required = self
            .len()
            .checked_add(additional)
            .filter(|n| *n <= MAX_RANGE_BODY)
            .ok_or(RangeError::Limit)?;
        if required > self.capacity() {
            let target = required
                .max(self.capacity().saturating_mul(2))
                .min(MAX_RANGE_BODY);
            self.bytes
                .try_reserve_exact(target - self.len())
                .map_err(|_| RangeError::Limit)?;
        }
        Ok(())
    }

    pub fn append(&mut self, fragment: &[u8]) -> R<()> {
        self.reserve(fragment.len())?;
        self.bytes.extend_from_slice(fragment);
        Ok(())
    }

    pub fn validated_blocks(&self) -> usize {
        self.decoded
            .as_ref()
            .map_or(0, |decoded| decoded.blocks.len())
    }

    /// Decode and hash-check at most one available block. A short object waits
    /// for more bytes. Other field orders and invalid prefixes defer to the
    /// ordinary decoder so the complete envelope determines retry/error policy.
    pub fn validate_next(&mut self) -> bool {
        if let Prefix::Header = self.prefix {
            // 9-byte header, two varints of at most 8 bytes, 7-byte key, 1-byte type.
            const HEADER_WINDOW: usize = 33;
            let Some(header) = self.bytes.get(..HEADER_WINDOW) else {
                return false;
            };
            self.prefix = Prefix::Stopped;
            if self.start == 0 || !(1..=MAX_RANGE_BLOCKS).contains(&self.count) {
                return false;
            }
            let Some((offset, count)) = first_blocks_field(header) else {
                return false;
            };
            if count == 0 || count > self.count {
                return false;
            }
            if self.ends.try_reserve_exact(count).is_err() {
                return false;
            }
            let Ok(decoded) = DecodedBlocks::new(self.count) else {
                return false;
            };
            self.decoded = Some(decoded);
            self.prefix = Prefix::Blocks {
                offset,
                count,
                attempt_at: offset,
            };
        }

        let Prefix::Blocks {
            offset,
            count,
            attempt_at,
        } = self.prefix
        else {
            return false;
        };
        if self.len() < attempt_at {
            return false;
        }
        let available = &self.bytes[offset..];
        let consumed = Cell::new(0);
        let mut object = Epee::from_object_payload(Bytes {
            slice: available,
            after: &[],
            consumed: Some(&consumed),
        });
        // EPEE skips transaction blobs in place. Check the complete object
        // extent before invoking the more expensive Monero deserializer.
        if object
            .entry()
            .map_err(RangeError::from)
            .and_then(|entry| fields(entry, |_, _| Ok(())))
            .is_err()
        {
            // Bound rescanning even if the peer sends one byte per fragment.
            self.prefix = Prefix::Blocks {
                offset,
                count,
                attempt_at: offset + available.len().saturating_mul(2).max(64),
            };
            return false;
        }
        let end = offset + consumed.get();
        let mut object = Epee::from_object_payload(Bytes {
            slice: &self.bytes[offset..end],
            after: &[],
            consumed: None,
        });
        let decoded = self.decoded.as_mut().unwrap();
        if object
            .entry()
            .map_err(RangeError::from)
            .and_then(|entry| decoded.push(entry, self.start))
            .is_err()
        {
            // The envelope must be checked before reporting e.g. MissingProof
            // or Limit. Release the partial decode and retry once at EOF.
            self.decoded = None;
            self.ends.clear();
            self.prefix = Prefix::Stopped;
            return false;
        }
        self.ends.push(end);
        self.prefix = if decoded.blocks.len() == count {
            Prefix::Stopped
        } else {
            Prefix::Blocks {
                offset: end,
                count,
                attempt_at: end,
            }
        };
        true
    }

    /// Consume the authenticated, completely framed response; cached blocks
    /// have no public route out of this object before all checks succeed.
    pub fn finish(self) -> R<Vec<RangeBlock>> {
        finish_range(
            &self.bytes,
            self.start,
            self.count,
            self.decoded,
            &self.ends,
        )
    }
}

// Capture the real parser position immediately after the first array count.
// Limit this inspection to the header window: EPEE drains entries when dropped.
fn first_blocks_field(header: &[u8]) -> Option<(usize, usize)> {
    let consumed = Cell::new(0);
    let bytes = Bytes {
        slice: header,
        after: &[],
        consumed: Some(&consumed),
    };
    let mut epee = Epee::new(bytes).ok()?;
    let mut fields = epee.entry().ok()?.fields().ok()?;
    let (key, entry) = fields.next()?.ok()?;
    let key = key.consume();
    if key.slice != b"blocks"
        || key.after.first() != Some(&((Type::Object as u8) | (Array::Array as u8)))
    {
        return None;
    }
    Some((consumed.get(), entry.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_object_offsets_must_match_the_full_document_walk() {
        let bytes = include_bytes!("../tests/fixtures/mainnet-3806161-range2.bin");
        let mut body = RangeBody::new(3_806_161, 2);
        body.append(bytes).unwrap();
        assert!(body.validate_next());
        assert!(body.validate_next());
        body.ends[0] -= 1;
        assert_eq!(body.finish().unwrap_err(), RangeError::Encoding);
    }
}
