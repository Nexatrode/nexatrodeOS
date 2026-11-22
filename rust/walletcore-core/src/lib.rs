//! Microkit-facing NexaWal core.
//!
//! Built on vendored `monero-wallet` (`default-features = false`) — no Tokio,
//! rayon, ureq, or ZMQ. Host adapters (vault / net / scan workers) sit outside.
#![no_std]

extern crate alloc;

mod scan_rpc;
mod block_range;
mod ownership;
#[cfg(feature = "worker-profile")]
pub use ownership::execute_profiled_scan_job;
pub use ownership::{decode_scan_result, encode_scan_job, execute_scan_job, scan_verified_block, OwnershipError, ScannedBlock, MAX_SCAN_JOB, MAX_SCAN_RESULT};
pub use block_range::{decode_block_range, encode_blocks_request, RangeBlock, RangeBody, RangeError, MAX_RANGE_BLOCKS, MAX_RANGE_BODY};
mod output_distribution;
mod decoy_collector;
pub use output_distribution::{DistributionDownload, OutputDistribution};
pub use decoy_collector::{DecoyRingCollector, DecoyRingSession};

use alloc::string::String;
use alloc::string::ToString as _;
pub use monero_wallet::address::Network;
use monero_wallet::ed25519::{Point, Scalar};
pub use monero_wallet::WalletOutput;
use zeroize::Zeroizing;

pub use monero_wallet::address::MoneroAddress;
pub use monero_wallet::block::Block;
pub use monero_wallet::transaction::{Pruned, Timelock, Transaction};
pub use monero_wallet::{Scanner, ViewPair, ViewPairError};
pub use monero_wallet::interface::FeeRate;
pub use scan_rpc::{
    SCAN_BLOCKS, TX_BATCH, STRICT_COMPACT_BATCH, strict_tx_batch_range, strict_tx_request_range, http_response_status, next_tls_stall_polls, scan_request_delay_ns, transient_scan_network_error, transient_daemon_status, https_rpc_response_complete, build_get_block_body, build_get_outs_body,
    build_get_output_histogram_body, build_get_transactions_body, build_get_pruned_transactions_body, build_http_post,
    decode_block_blob, json_output_index_lists, json_string_array, json_string_value,
    outputs_balance, parse_get_outs_key_mask, parse_get_outs_ok, parse_histogram_total,
    parse_send_raw_ok, parse_spent_statuses, parse_tx_batch, parse_tx_batch_verified, scan_assembled, scan_assembled_strict,
    build_fee_estimate_body, parse_normal_fee_rate, build_spent_status_body, build_output_distribution_body, parse_output_distribution_chunk, build_get_outs_batch_body, parse_get_outs_batch,
};

#[cfg(feature = "compile-time-generators")]
use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
#[cfg(not(feature = "compile-time-generators"))]
use curve25519_dalek::constants::ED25519_BASEPOINT_POINT as ED25519_BASEPOINT_TABLE;

/// Plain HTTP daemon endpoint. TLS URLs, credentials, paths and IPv6 are
/// rejected until the guest transport implements them explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeEndpoint {
    host: [u8; 120],
    host_len: u8,
    authority: [u8; 126],
    authority_len: u8,
    pub port: u16,
}

impl NodeEndpoint {
    pub fn parse(input: &str) -> Option<Self> {
        let input = input.strip_prefix("http://").unwrap_or(input);
        let (host, port) = input.rsplit_once(':')?;
        if host.is_empty() || host.len() > 119 || port.is_empty()
            || !port.bytes().all(|b| b.is_ascii_digit()) { return None; }
        let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
        if !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            || host.split('.').any(|part| part.is_empty() || part.len() > 63
                || part.starts_with('-') || part.ends_with('-')) { return None; }
        // Numeric dotted hosts must be valid IPv4, not malformed DNS names.
        if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') && parse_ipv4(host).is_none() {
            return None;
        }
        if input.len() > 125 { return None; }
        let mut value = Self { host: [0; 120], host_len: host.len() as u8,
            authority: [0; 126], authority_len: input.len() as u8, port };
        value.host[..host.len()].copy_from_slice(host.as_bytes());
        value.authority[..input.len()].copy_from_slice(input.as_bytes());
        Some(value)
    }

    pub fn host(&self) -> &str {
        core::str::from_utf8(&self.host[..self.host_len as usize]).unwrap_or("")
    }
    pub fn authority(&self) -> &str {
        core::str::from_utf8(&self.authority[..self.authority_len as usize]).unwrap_or("")
    }
    pub fn ipv4(&self) -> Option<[u8; 4]> { parse_ipv4(self.host()) }
}

fn parse_ipv4(input: &str) -> Option<[u8; 4]> {
    let mut out = [0; 4];
    let mut parts = input.split('.');
    for byte in &mut out {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *byte = part.parse().ok()?;
    }
    if parts.next().is_some() { return None; }
    Some(out)
}

/// Require the daemon's get_info response to identify the selected chain.
pub fn daemon_network_matches(json: &[u8], network: Network) -> bool {
    let Ok(response) = serde_json::from_slice::<serde_json::Value>(json) else { return false; };
    let info = response.get("result").unwrap_or(&response);
    let expected = match network {
        Network::Mainnet => "mainnet", Network::Stagenet => "stagenet", Network::Testnet => "testnet",
    };
    info.get("status").and_then(|value| value.as_str()) == Some("OK")
        && info.get("nettype").and_then(|value| value.as_str()) == Some(expected)
}

/// Encrypted vault payload for a wallet identity.
pub struct WalletIdentity {
    spend: Zeroizing<[u8; 32]>,
    view: Zeroizing<[u8; 32]>,
    pub restore_height: u64,
    pub network: Network,
}

impl WalletIdentity {
    pub const ENCODED_SIZE: usize = 74;

    /// Generate a Monero mnemonic using the caller's cryptographic entropy source.
    /// The caller should present the backup before persisting the returned identity.
    pub fn generate<R: rand_core::RngCore + rand_core::CryptoRng>(
        rng: &mut R,
        restore_height: u64,
        network: Network,
    ) -> Result<(Self, Zeroizing<String>), OpenError> {
        let seed = monero_seed::Seed::new(rng, monero_seed::Language::English);
        let words = seed.to_string();
        let identity = Self::from_mnemonic(words.clone(), restore_height, network)?;
        Ok((identity, words))
    }

    /// Generate from 64 bytes obtained from the platform entropy service.
    pub fn generate_from_entropy(
        entropy: Zeroizing<[u8; 64]>,
        restore_height: u64,
        network: Network,
    ) -> Result<(Self, Zeroizing<String>), OpenError> {
        let mut spend = curve25519_dalek::Scalar::from_bytes_mod_order_wide(&entropy);
        if spend == curve25519_dalek::Scalar::ZERO {
            return Err(OpenError::BadKey);
        }
        let bytes = Zeroizing::new(spend.to_bytes());
        use zeroize::Zeroize;
        spend.zeroize();
        let seed = monero_seed::Seed::from_entropy(monero_seed::Language::English, bytes)
            .ok_or(OpenError::BadKey)?;
        let words = seed.to_string();
        let identity = Self::from_mnemonic(words.clone(), restore_height, network)?;
        Ok((identity, words))
    }

    pub fn from_mnemonic(
        words: Zeroizing<String>,
        restore_height: u64,
        network: Network,
    ) -> Result<Self, OpenError> {
        let seed = monero_seed::Seed::from_string(monero_seed::Language::English, words)
            .map_err(|_| OpenError::BadKey)?;
        let spend = seed.entropy();
        let view = Zeroizing::new(<[u8; 32]>::from(Scalar::hash(spend.as_ref())));
        Ok(Self { spend, view, restore_height, network })
    }

    pub fn encode(&self) -> Zeroizing<[u8; Self::ENCODED_SIZE]> {
        let mut out = Zeroizing::new([0; Self::ENCODED_SIZE]);
        out[0] = 1;
        out[1] = match self.network {
            Network::Mainnet => 0,
            Network::Stagenet => 1,
            Network::Testnet => 2,
        };
        out[2..34].copy_from_slice(&self.spend[..]);
        out[34..66].copy_from_slice(&self.view[..]);
        out[66..74].copy_from_slice(&self.restore_height.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, OpenError> {
        if bytes.len() != Self::ENCODED_SIZE || bytes[0] != 1 {
            return Err(OpenError::BadKey);
        }
        let network = match bytes[1] {
            0 => Network::Mainnet,
            1 => Network::Stagenet,
            2 => Network::Testnet,
            _ => return Err(OpenError::BadKey),
        };
        let mut spend = Zeroizing::new([0; 32]);
        let mut view = Zeroizing::new([0; 32]);
        spend.copy_from_slice(&bytes[2..34]);
        view.copy_from_slice(&bytes[34..66]);
        let restore_height = u64::from_le_bytes(bytes[66..74].try_into().map_err(|_| OpenError::BadKey)?);
        let derived = Zeroizing::new(<[u8; 32]>::from(Scalar::hash(spend.as_ref())));
        if *derived != *view {
            return Err(OpenError::BadKey);
        }
        // Validate canonical scalar encodings before retaining the identity.
        WalletState::open_from_key_bytes_on_network("restored", &spend, &view, restore_height, network)?;
        Ok(Self { spend, view, restore_height, network })
    }

    pub fn open(&self) -> Result<WalletState, OpenError> {
        WalletState::open_from_key_bytes_on_network(
            "restored", &self.spend, &self.view, self.restore_height, self.network,
        )
    }
}

/// A signed transaction and the amounts shown at fee confirmation.
#[derive(Clone)]
pub struct PendingTransaction {
    pub wallet_address: String,
    pub tx_hex: Zeroizing<String>,
    pub fee: u64,
    pub amount: u64,
}

impl PendingTransaction {
    const MAGIC: [u8; 8] = *b"NXTX0001";
    const HEADER_SIZE: usize = 30;
    pub const MAX_HEX_LENGTH: usize = 12_000;

    /// Only the wallet that created a record may resume its relay.
    pub fn belongs_to(&self, wallet: &WalletState) -> bool {
        wallet.open && wallet.address() == self.wallet_address
    }

    /// Compute the Monero transaction ID from the complete signed transaction.
    fn parse_transaction(&self) -> Option<Transaction> {
        let hex = self.tx_hex.as_bytes();
        if hex.is_empty() || hex.len() > Self::MAX_HEX_LENGTH || hex.len() % 2 != 0 {
            return None;
        }
        let mut bytes = Zeroizing::new(alloc::vec::Vec::with_capacity(hex.len() / 2));
        for pair in hex.chunks_exact(2) {
            let digit = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                _ => None,
            };
            bytes.push((digit(pair[0])? << 4) | digit(pair[1])?);
        }
        let mut remaining: &[u8] = &bytes;
        let transaction = Transaction::read(&mut remaining).ok()?;
        if !remaining.is_empty() {
            return None;
        }
        Some(transaction)
    }

    pub fn transaction_id(&self) -> Option<[u8; 32]> {
        Some(self.parse_transaction()?.hash())
    }

    /// Check the saved metadata against the transaction shape produced by
    /// this wallet. Cryptographic acceptance is still checked by the node.
    pub fn signed_payment_id(&self) -> Option<[u8; 32]> {
        use monero_wallet::transaction::Input;
        let tx = self.parse_transaction()?;
        let Transaction::V2 { prefix, proofs: Some(proofs) } = &tx else { return None; };
        if self.amount == 0 || self.fee == 0 || proofs.base.fee != self.fee || prefix.inputs.is_empty()
            || prefix.inputs.iter().any(|input| !matches!(input, Input::ToKey { amount: None, key_offsets, .. } if key_offsets.len() == usize::from(RING_LEN))) {
            return None;
        }
        Some(tx.hash())
    }

    pub fn encode(&self) -> Option<Zeroizing<alloc::vec::Vec<u8>>> {
        let hex = self.tx_hex.as_bytes();
        let address = self.wallet_address.as_bytes();
        if address.is_empty() || address.len() > 128
            || MoneroAddress::from_str_with_unchecked_network(&self.wallet_address).is_err()
            || hex.is_empty() || hex.len() > Self::MAX_HEX_LENGTH || hex.len() % 2 != 0
            || !hex.iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return None;
        }
        let mut out = Zeroizing::new(alloc::vec::Vec::with_capacity(Self::HEADER_SIZE + address.len() + hex.len()));
        out.extend_from_slice(&Self::MAGIC);
        out.extend_from_slice(&(hex.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.fee.to_le_bytes());
        out.extend_from_slice(&self.amount.to_le_bytes());
        out.extend_from_slice(&(address.len() as u16).to_le_bytes());
        out.extend_from_slice(address);
        out.extend_from_slice(hex);
        Some(out)
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::HEADER_SIZE || bytes[..8] != Self::MAGIC {
            return None;
        }
        let length = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
        let address_length = u16::from_le_bytes(bytes[28..30].try_into().ok()?) as usize;
        if address_length == 0 || address_length > 128
            || length == 0 || length > Self::MAX_HEX_LENGTH || length % 2 != 0
            || bytes.len() != Self::HEADER_SIZE + address_length + length
        {
            return None;
        }
        let address = core::str::from_utf8(&bytes[Self::HEADER_SIZE..Self::HEADER_SIZE + address_length]).ok()?;
        MoneroAddress::from_str_with_unchecked_network(address).ok()?;
        let hex = &bytes[Self::HEADER_SIZE + address_length..];
        if !hex.iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)) {
            return None;
        }
        Some(Self {
            wallet_address: String::from(address),
            tx_hex: Zeroizing::new(String::from(core::str::from_utf8(hex).ok()?)),
            fee: u64::from_le_bytes(bytes[12..20].try_into().ok()?),
            amount: u64::from_le_bytes(bytes[20..28].try_into().ok()?),
        })
    }
}

/// A sent transaction recorded only after an anchored scan found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmedSend {
    pub txid: [u8; 32],
    pub block_hash: [u8; 32],
    pub block_height: u64,
    pub amount: u64,
    pub fee: u64,
    /// Retained signed bytes allow a reorganized payment to be retried safely.
    /// Older records may not contain them and cannot be automatically reopened.
    pub tx_hex: Option<Zeroizing<String>>,
}

/// One atomic vault record contains both the pending payment and sent history.
/// The on-disk size is bounded; callers must not discard history to make room.
#[derive(Clone)]
pub struct WalletJournal {
    pub wallet_address: String,
    pub pending: Option<PendingTransaction>,
    pub sent: alloc::vec::Vec<ConfirmedSend>,
    /// A checkpoint is valid only for a prefix with no owned outputs or sends.
    pub empty_scan_checkpoint: Option<(u64, [u8; 32])>,
}

impl WalletJournal {
    const MAGIC: [u8; 8] = *b"NXJR0003";
    const V2_MAGIC: [u8; 8] = *b"NXJR0002";
    const LEGACY_MAGIC: [u8; 8] = *b"NXJR0001";
    const HEADER: usize = 16;
    const ENTRY: usize = 88;
    const ENTRY_V2: usize = 92;
    pub const MAX_BYTES: usize = 4 * 3072;

    /// A checkpoint with no stored outputs cannot represent funded state or
    /// transaction observations, so those wallets must scan from the start.
    pub fn permits_empty_scan_checkpoint(&self, owned_outputs: usize, metadata_outputs: usize) -> bool {
        owned_outputs == 0 && metadata_outputs == 0 && self.pending.is_none() && self.sent.is_empty()
    }

    /// Reserve enough space for the largest permitted signed transaction in
    /// either pending or confirmed form before creating new signed bytes.
    pub fn can_record_new_transaction(&self) -> bool {
        if self.pending.is_some() { return false; }
        self.encode().is_some_and(|bytes| bytes.len().checked_add(Self::ENTRY_V2 + PendingTransaction::MAX_HEX_LENGTH / 2)
            .is_some_and(|needed| needed <= Self::MAX_BYTES))
    }

    pub fn empty(wallet_address: &str) -> Option<Self> {
        MoneroAddress::from_str_with_unchecked_network(wallet_address).ok()?;
        if wallet_address.len() > 128 { return None; }
        Some(Self { wallet_address: String::from(wallet_address), pending: None, sent: alloc::vec::Vec::new(), empty_scan_checkpoint: None })
    }

    pub fn encode(&self) -> Option<Zeroizing<alloc::vec::Vec<u8>>> {
        MoneroAddress::from_str_with_unchecked_network(&self.wallet_address).ok()?;
        let address = self.wallet_address.as_bytes();
        if address.is_empty() || address.len() > 128 || self.sent.len() > u16::MAX as usize { return None; }
        let tx = if let Some(pending) = &self.pending {
            if pending.wallet_address != self.wallet_address || pending.tx_hex.is_empty()
                || pending.tx_hex.len() > PendingTransaction::MAX_HEX_LENGTH || pending.tx_hex.len() % 2 != 0 { return None; }
            let mut bytes = Zeroizing::new(alloc::vec::Vec::with_capacity(pending.tx_hex.len() / 2));
            for pair in pending.tx_hex.as_bytes().chunks_exact(2) {
                let digit = |byte| match byte { b'0'..=b'9' => Some(byte - b'0'), b'a'..=b'f' => Some(byte - b'a' + 10), _ => None };
                bytes.push((digit(pair[0])? << 4) | digit(pair[1])?);
            }
            Some(bytes)
        } else { None };
        for (i, item) in self.sent.iter().enumerate() {
            if self.sent[..i].iter().any(|old| old.txid == item.txid) { return None; }
        }
        let tx_len = tx.as_ref().map_or(0, |tx| tx.len());
        let mut history_bytes = 0usize;
        for entry in &self.sent {
            let len = if let Some(hex) = entry.tx_hex.as_ref() {
                let record = PendingTransaction { wallet_address: self.wallet_address.clone(), tx_hex: hex.clone(), fee: entry.fee, amount: entry.amount };
                if record.transaction_id()? != entry.txid { return None; }
                hex.len() / 2
            } else { 0 };
            history_bytes = history_bytes.checked_add(Self::ENTRY_V2)?.checked_add(len)?;
        }
        let needed = Self::HEADER.checked_add(address.len())?.checked_add(41)?
            .checked_add(if tx.is_some() { 16 + tx_len } else { 0 })?
            .checked_add(history_bytes)?;
        if needed > Self::MAX_BYTES { return None; }
        let mut out = Zeroizing::new(alloc::vec::Vec::with_capacity(needed));
        out.extend_from_slice(&Self::MAGIC);
        out.extend_from_slice(&(address.len() as u16).to_le_bytes());
        out.extend_from_slice(&(tx_len as u32).to_le_bytes());
        out.extend_from_slice(&(self.sent.len() as u16).to_le_bytes());
        out.extend_from_slice(address);
        if let (Some(pending), Some(bytes)) = (&self.pending, tx) {
            out.extend_from_slice(&pending.fee.to_le_bytes());
            out.extend_from_slice(&pending.amount.to_le_bytes());
            out.extend_from_slice(&bytes);
        }
        for item in &self.sent {
            out.extend_from_slice(&item.txid);
            out.extend_from_slice(&item.block_hash);
            out.extend_from_slice(&item.block_height.to_le_bytes());
            out.extend_from_slice(&item.amount.to_le_bytes());
            out.extend_from_slice(&item.fee.to_le_bytes());
            let hex = item.tx_hex.as_ref().map(|v| v.as_str()).unwrap_or("");
            out.extend_from_slice(&((hex.len() / 2) as u32).to_le_bytes());
            for pair in hex.as_bytes().chunks_exact(2) {
                let digit = |byte| match byte { b'0'..=b'9' => Some(byte - b'0'), b'a'..=b'f' => Some(byte - b'a' + 10), _ => None };
                out.push((digit(pair[0])? << 4) | digit(pair[1])?);
            }
        }
        match self.empty_scan_checkpoint {
            Some((next, hash)) if next > 0 => { out.push(1); out.extend_from_slice(&next.to_le_bytes()); out.extend_from_slice(&hash); }
            None => { out.push(0); out.extend_from_slice(&[0; 40]); }
            _ => return None,
        }
        Some(out)
    }

    /// Build, but do not apply, an atomic pending-to-history transition.
    /// The caller must persist the candidate before replacing in-memory state.
    pub fn confirmed_candidate(&self, evidence: &PendingChainEvidence, chain: &RestoreChain,
        chain_height: u64, minimum_confirmations: u64) -> Option<Self> {
        let pending = self.pending.as_ref()?;
        let txid = pending.transaction_id()?;
        if pending.wallet_address != self.wallet_address || !evidence.matches(&txid) { return None; }
        let (block_height, block_hash) = evidence.block(chain, chain_height)?;
        if minimum_confirmations == 0 || chain_height.checked_sub(block_height)? < minimum_confirmations { return None; }
        let entry = ConfirmedSend { txid, block_hash, block_height, amount: pending.amount, fee: pending.fee, tx_hex: Some(pending.tx_hex.clone()) };
        let mut candidate = self.clone();
        if let Some(existing) = candidate.sent.iter().find(|old| old.txid == txid) {
            if existing != &entry { return None; }
        } else {
            candidate.sent.push(entry);
        }
        candidate.pending = None;
        candidate.encode()?;
        Some(candidate)
    }

    /// Prepare a reorg recovery without changing the live journal. Persist the
    /// returned candidate before treating the transaction as pending again.
    pub fn orphaned_candidate(&self, txid: [u8; 32]) -> Option<Self> {
        if self.pending.is_some() { return None; }
        let index = self.sent.iter().position(|entry| entry.txid == txid)?;
        let entry = &self.sent[index];
        let record = PendingTransaction { wallet_address: self.wallet_address.clone(),
            tx_hex: entry.tx_hex.as_ref()?.clone(), amount: entry.amount, fee: entry.fee };
        if record.transaction_id()? != txid { return None; }
        let mut candidate = self.clone();
        candidate.sent.remove(index);
        candidate.pending = Some(record);
        candidate.encode()?;
        Some(candidate)
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        // Existing installs store a bare pending transaction in the same banks.
        if bytes.starts_with(&PendingTransaction::MAGIC) {
            let pending = PendingTransaction::decode(bytes)?;
            let mut journal = Self::empty(&pending.wallet_address)?;
            journal.pending = Some(pending);
            return Some(journal);
        }
        if bytes.len() < Self::HEADER || bytes.len() > Self::MAX_BYTES { return None; }
        let legacy = bytes.starts_with(&Self::LEGACY_MAGIC);
        let has_checkpoint = bytes.starts_with(&Self::MAGIC);
        if !legacy && !has_checkpoint && !bytes.starts_with(&Self::V2_MAGIC) { return None; }
        let address_len = u16::from_le_bytes(bytes[8..10].try_into().ok()?) as usize;
        let tx_len = u32::from_le_bytes(bytes[10..14].try_into().ok()?) as usize;
        let count = u16::from_le_bytes(bytes[14..16].try_into().ok()?) as usize;
        if address_len == 0 || address_len > 128 || tx_len > PendingTransaction::MAX_HEX_LENGTH / 2 { return None; }
        let minimum = Self::HEADER.checked_add(address_len)?
            .checked_add(if tx_len != 0 { 16 + tx_len } else { 0 })?
            .checked_add(count.checked_mul(if legacy { Self::ENTRY } else { Self::ENTRY_V2 })?)?;
        if minimum > bytes.len() || (legacy && minimum != bytes.len()) { return None; }
        let mut offset = Self::HEADER;
        let address = core::str::from_utf8(&bytes[offset..offset + address_len]).ok()?;
        let mut journal = Self::empty(address)?;
        offset += address_len;
        if tx_len != 0 {
            let fee = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
            let amount = u64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().ok()?);
            offset += 16;
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut hex = Zeroizing::new(String::with_capacity(tx_len * 2));
            for byte in &bytes[offset..offset + tx_len] {
                hex.push(HEX[(byte >> 4) as usize] as char);
                hex.push(HEX[(byte & 15) as usize] as char);
            }
            offset += tx_len;
            journal.pending = Some(PendingTransaction { wallet_address: journal.wallet_address.clone(), tx_hex: hex, fee, amount });
        }
        for _ in 0..count {
            if bytes.len().checked_sub(offset)? < if legacy { Self::ENTRY } else { Self::ENTRY_V2 } { return None; }
            let mut item = ConfirmedSend {
                txid: bytes[offset..offset + 32].try_into().ok()?,
                block_hash: bytes[offset + 32..offset + 64].try_into().ok()?,
                block_height: u64::from_le_bytes(bytes[offset + 64..offset + 72].try_into().ok()?),
                amount: u64::from_le_bytes(bytes[offset + 72..offset + 80].try_into().ok()?),
                fee: u64::from_le_bytes(bytes[offset + 80..offset + 88].try_into().ok()?),
                tx_hex: None,
            };
            offset += Self::ENTRY;
            if !legacy {
                let raw_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
                offset += 4;
                if raw_len > PendingTransaction::MAX_HEX_LENGTH / 2 || raw_len > bytes.len().checked_sub(offset)? { return None; }
                if raw_len > 0 {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    let mut hex = Zeroizing::new(String::with_capacity(raw_len * 2));
                    for byte in &bytes[offset..offset + raw_len] {
                        hex.push(HEX[(byte >> 4) as usize] as char);
                        hex.push(HEX[(byte & 15) as usize] as char);
                    }
                    let record = PendingTransaction { wallet_address: journal.wallet_address.clone(), tx_hex: hex.clone(), fee: item.fee, amount: item.amount };
                    if record.transaction_id()? != item.txid { return None; }
                    item.tx_hex = Some(hex);
                    offset += raw_len;
                }
            }
            if journal.sent.iter().any(|old| old.txid == item.txid) { return None; }
            journal.sent.push(item);
        }
        if has_checkpoint {
            if bytes.len().checked_sub(offset)? != 41 { return None; }
            match bytes[offset] {
                0 if bytes[offset + 1..].iter().all(|&b| b == 0) => (),
                1 => {
                    let next = u64::from_le_bytes(bytes[offset + 1..offset + 9].try_into().ok()?);
                    if next == 0 { return None; }
                    journal.empty_scan_checkpoint = Some((next, bytes[offset + 9..offset + 41].try_into().ok()?));
                }
                _ => return None,
            }
            offset += 41;
        }
        if offset != bytes.len() { return None; }
        Some(journal)
    }
}

/// What a completed, anchored scan observed at a saved history entry's height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryObservation {
    Unchecked,
    Block { hash: [u8; 32], contains_transaction: bool },
}

/// A candidate is only a proposal: replace the live journal after its encoded
/// bytes have been committed and read back from durable storage.
pub enum JournalPlan {
    NoChange,
    Confirm(WalletJournal),
    Reopen(WalletJournal),
    RecoveryRequired,
}

pub fn plan_journal_reconciliation(
    journal: &WalletJournal,
    observations: &[HistoryObservation],
    evidence: Option<&PendingChainEvidence>,
    chain: &RestoreChain,
    chain_height: u64,
    minimum_confirmations: u64,
) -> Option<JournalPlan> {
    if observations.len() != journal.sent.len() || chain.next() != chain_height || !chain.is_verified() || chain.anchor().is_none() {
        return None;
    }
    let mut orphan = None;
    for (entry, observed) in journal.sent.iter().zip(observations) {
        match observed {
            HistoryObservation::Unchecked => return None,
            HistoryObservation::Block { hash, contains_transaction } => {
                if hash != &entry.block_hash || !contains_transaction {
                    // There is room for only one recoverable pending record.
                    if orphan.replace(entry.txid).is_some() { return Some(JournalPlan::RecoveryRequired); }
                }
            }
        }
    }
    if let Some(txid) = orphan {
        return Some(match journal.orphaned_candidate(txid) {
            Some(candidate) => JournalPlan::Reopen(candidate),
            None => JournalPlan::RecoveryRequired,
        });
    }
    if let Some(evidence) = evidence {
        if let Some(candidate) = journal.confirmed_candidate(evidence, chain, chain_height, minimum_confirmations) {
            return Some(JournalPlan::Confirm(candidate));
        }
    }
    Some(JournalPlan::NoChange)
}

/// A node's report about the exact saved transaction. This is not an
/// independently verified chain inclusion proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingObservation {
    Missing,
    InPool,
    InBlock(u64),
}

pub fn observe_pending_transaction(record: &PendingTransaction, json: &[u8]) -> Option<PendingObservation> {
    let expected = record.transaction_id()?;
    let body: serde_json::Value = serde_json::from_slice(json).ok()?;
    if body.get("status")?.as_str()? != "OK" { return None; }
    let txs = body.get("txs")?.as_array()?;
    let missed = match body.get("missed_tx") {
        Some(value) => Some(value.as_array()?),
        None => None,
    };
    if txs.is_empty() {
        let missed = missed?;
        return (missed.len() == 1 && missed[0].as_str().and_then(|s| decode_hex32(s.as_bytes())) == Some(expected))
            .then_some(PendingObservation::Missing);
    }
    if txs.len() != 1 || missed.is_some_and(|items| !items.is_empty()) { return None; }
    let tx = txs[0].as_object()?;
    let observed = PendingTransaction {
        wallet_address: String::new(),
        tx_hex: Zeroizing::new(String::from(tx.get("as_hex")?.as_str()?)),
        fee: 0,
        amount: 0,
    };
    if observed.transaction_id()? != expected { return None; }
    if tx.get("in_pool")?.as_bool()? { Some(PendingObservation::InPool) }
    else { Some(PendingObservation::InBlock(tx.get("block_height")?.as_u64()?)) }
}

/// Tracks the last fully scanned block. A caller must check the next block's
/// height and parent before scanning it, then commit only after it succeeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestoreChain {
    start: u64,
    next: u64,
    last: Option<[u8; 32]>,
    verified: bool,
}

impl RestoreChain {
    pub fn new(start: u64) -> Self { Self { start, next: start, last: None, verified: false } }
    pub fn next(&self) -> u64 { self.next }
    pub fn resume_empty(start: u64, next: u64, anchor: [u8; 32]) -> Option<Self> {
        if next <= start { return None; }
        Some(Self { start, next, last: Some(anchor), verified: false })
    }
    pub fn reset(&mut self) { *self = Self::new(self.start); }
    /// Anchor an empty scan at the block immediately before its start. No
    /// outputs are credited by this operation; the anchor must be rechecked.
    pub fn seed_preceding_anchor(&mut self, height: u64, hash: [u8; 32]) -> bool {
        if self.last.is_some() || self.next != self.start || height.checked_add(1) != Some(self.start) { return false; }
        self.last = Some(hash);
        self.verified = false;
        true
    }
    pub fn anchor(&self) -> Option<(u64, [u8; 32])> {
        self.last.map(|hash| (self.next - 1, hash))
    }
    pub fn accepts(&self, height: u64, previous: [u8; 32]) -> bool {
        height == self.next && self.last.is_none_or(|hash| hash == previous)
    }
    pub fn commit(&mut self, height: u64, previous: [u8; 32], hash: [u8; 32]) -> bool {
        if !self.accepts(height, previous) { return false; }
        let Some(next) = height.checked_add(1) else { return false; };
        self.next = next;
        self.last = Some(hash);
        self.verified = false;
        true
    }
    pub fn invalidate(&mut self) { self.verified = false; }
    pub fn verify_tip(&mut self, height: u64, hash: [u8; 32], chain_height: u64) -> bool {
        self.verified = self.next == chain_height && self.anchor_matches(height, hash);
        self.verified
    }
    pub fn is_verified(&self) -> bool { self.verified }
    pub fn anchor_matches(&self, height: u64, hash: [u8; 32]) -> bool {
        self.anchor() == Some((height, hash))
    }
}

/// Evidence that a pending transaction appeared in a fully scanned block.
/// This is relative to the selected node, not an independent proof of work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingChainEvidence {
    txid: [u8; 32],
    found: Option<(u64, [u8; 32])>,
    verified_tip: Option<(u64, [u8; 32])>,
}

impl PendingChainEvidence {
    pub fn new(txid: [u8; 32]) -> Self { Self { txid, found: None, verified_tip: None } }
    pub fn matches(&self, txid: &[u8; 32]) -> bool { &self.txid == txid }
    pub fn invalidate(&mut self) { self.verified_tip = None; }
    pub fn reset(&mut self) { self.found = None; self.invalidate(); }
    /// Call only after all block transactions were checked and committed.
    pub fn record_block(&mut self, chain: &RestoreChain, height: u64, hash: [u8; 32], ids: &[[u8; 32]]) -> Result<(), ()> {
        self.invalidate();
        if chain.anchor() != Some((height, hash)) { return Err(()); }
        if ids.iter().any(|id| *id == self.txid) {
            if self.found.is_some_and(|previous| previous != (height, hash)) { return Err(()); }
            self.found = Some((height, hash));
        }
        Ok(())
    }
    /// Call only after the scan has reached the advertised tip and its anchor
    /// has been rechecked after the spent-status requests.
    pub fn finish(&mut self, chain: &RestoreChain, chain_height: u64) -> bool {
        self.invalidate();
        if chain.next() != chain_height || !chain.is_verified() { return false; }
        let Some(anchor) = chain.anchor() else { return false; };
        self.verified_tip = Some(anchor);
        true
    }
    pub fn block(&self, chain: &RestoreChain, chain_height: u64) -> Option<(u64, [u8; 32])> {
        if chain.next() != chain_height || !chain.is_verified() || self.verified_tip.is_none() || self.verified_tip != chain.anchor() { return None; }
        self.found
    }
    pub fn location(&self, chain: &RestoreChain, chain_height: u64) -> Option<u64> {
        self.block(chain, chain_height).map(|(height, _)| height)
    }
}

/// Scan metadata for one owned output. `spent_status` is the daemon's
/// 0 (unspent), 1 (confirmed spend), or 2 (pool spend); None is unchecked.
#[derive(Clone, Copy, Debug)]
pub struct RestoredOutput {
    pub amount: u64,
    pub height: u64,
    pub coinbase: bool,
    pub timelock: Timelock,
    pub spent_status: Option<u8>,
}

fn restored_output_unlocked(output: &RestoredOutput, chain_height: u64) -> bool {
    let lock = if output.coinbase { 60 } else { 10 };
    let age_ok = output.height.checked_add(lock).is_some_and(|height| height <= chain_height);
    let additional_ok = match output.timelock {
        Timelock::None => true,
        Timelock::Block(height) => (height as u64) <= chain_height,
        Timelock::Time(_) => false,
    };
    age_ok && additional_ok
}

/// Select only checked, unspent, mature outputs. An unchecked output makes the
/// entire selection invalid; callers must not sign against a partial scan.
/// The caller must include fees in `required` and obtain a separate ring for
/// every selected input. `None` also covers insufficient unlocked funds.
pub fn select_restored_inputs(outputs: &[RestoredOutput], chain_height: u64, required: u64) -> Option<alloc::vec::Vec<usize>> {
    if required == 0 { return None; }
    restored_balances(outputs, chain_height)?;
    let mut indices: alloc::vec::Vec<usize> = outputs.iter().enumerate()
        .filter(|(_, output)| output.spent_status == Some(0) && restored_output_unlocked(output, chain_height))
        .map(|(index, _)| index).collect();
    indices.sort_unstable_by(|a, b| outputs[*b].amount.cmp(&outputs[*a].amount).then(a.cmp(b)));
    let mut total = 0u64;
    for (count, index) in indices.iter().enumerate() {
        total = total.checked_add(outputs[*index].amount)?;
        if total >= required { indices.truncate(count + 1); return Some(indices); }
    }
    None
}

/// Return confirmed available and mature amounts only after every output has
/// a valid spent status. Time-locked outputs stay locked without trusted time.
pub fn restored_balances(outputs: &[RestoredOutput], chain_height: u64) -> Option<(u64, u64)> {
    let (mut total, mut unlocked) = (0u64, 0u64);
    for output in outputs {
        if output.height >= chain_height { return None; }
        let status = output.spent_status?;
        if status > 2 { return None; }
        if status != 0 { continue; }
        total = total.checked_add(output.amount)?;
        if restored_output_unlocked(output, chain_height) { unlocked = unlocked.checked_add(output.amount)?; }
    }
    Some((total, unlocked))
}

/// One owned output discovered during the current scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestoredActivityOutput {
    pub txid: [u8; 32],
    pub height: u64,
    pub amount: u64,
}

/// Reconstruct recent incoming and outgoing activity, aggregating outputs of
/// the same transaction and excluding change from known outgoing transactions.
pub fn restored_activity(outputs: &[RestoredActivityOutput], journal: &WalletJournal) -> Option<alloc::vec::Vec<(bool, u64)>> {
    let pending_id = journal.pending.as_ref().and_then(PendingTransaction::transaction_id);
    let mut incoming: alloc::vec::Vec<([u8; 32], u64, u64)> = alloc::vec::Vec::new();
    for output in outputs {
        if pending_id == Some(output.txid) || journal.sent.iter().any(|entry| entry.txid == output.txid) { continue; }
        if let Some(existing) = incoming.iter_mut().find(|entry| entry.0 == output.txid) {
            if existing.1 != output.height { return None; }
            existing.2 = existing.2.checked_add(output.amount)?;
        } else { incoming.push((output.txid, output.height, output.amount)); }
    }
    let mut events: alloc::vec::Vec<(u64, [u8; 32], bool, u64)> = incoming.into_iter()
        .map(|(txid, height, amount)| (height, txid, true, amount)).collect();
    events.extend(journal.sent.iter().map(|entry| (entry.block_height, entry.txid, false, entry.amount)));
    events.sort_unstable_by_key(|event| (event.0, event.1));
    let skip = events.len().saturating_sub(5);
    Some(events.into_iter().skip(skip).map(|(_, _, received, amount)| (received, amount)).collect())
}

/// Open wallet backed by real monero-oxide view/spend material.
pub struct WalletState {
    pub open: bool,
    pub network: Network,
    pub height: u64,
    pub balance_atomic: u64,
    pub unlocked_atomic: u64,
    /// True after at least one daemon ownership scan pass completed.
    pub scanned: bool,
    pub history_count: u32,
    /// Owned outputs from the last successful scan (spend path / history).
    pub outputs: alloc::vec::Vec<WalletOutput>,
    /// Hex-encoded signed tx waiting for `send_raw_transaction` (empty if none).
    pub pending_tx_hex: alloc::string::String,
    /// Fee of `pending_tx_hex`, in atomic units.
    pub pending_fee_atomic: u64,
    /// Amount of `pending_tx_hex` that leaves the wallet, in atomic units.
    pub pending_send_amount: u64,
    /// Sent amounts from this boot, oldest first, capped at 5.
    pub recent_sent: alloc::vec::Vec<u64>,
    /// Received (+) and spent (−) amounts that survive a spent-output clear.
    /// Oldest first, capped at 5. Dust (1 piconero) is not recorded.
    activity: [(i8, u64); 5],
    activity_len: u8,
    view: Option<ViewPair>,
    /// Private spend scalar — required for `SignableTransaction::sign`.
    spend_secret: Option<Zeroizing<Scalar>>,
    address: String,
    label: String,
}

impl WalletState {
    pub fn closed() -> Self {
        Self {
            open: false,
            network: Network::Mainnet,
            height: 0,
            balance_atomic: 0,
            unlocked_atomic: 0,
            scanned: false,
            history_count: 0,
            outputs: alloc::vec::Vec::new(),
            pending_tx_hex: alloc::string::String::new(),
            pending_fee_atomic: 0,
            pending_send_amount: 0,
            recent_sent: alloc::vec::Vec::new(),
            activity: [(0, 0); 5],
            activity_len: 0,
            view: None,
            spend_secret: None,
            address: String::new(),
            label: String::from("closed"),
        }
    }

    /// Derive spend/view from `seed` via Keccak hash (demo restore path).
    /// Production will take mnemonic / imported keys; crypto path is the same stack.
    pub fn open_from_seed(label: &str, seed: &[u8]) -> Result<Self, OpenError> {
        let spend_scalar = Zeroizing::new(Scalar::hash([b"nexawal-spend:", seed].concat()));
        let view_scalar = Zeroizing::new(Scalar::hash([b"nexawal-view:", seed].concat()));
        Self::open_from_scalars(label, spend_scalar, view_scalar, 0, Network::Mainnet)
    }

    /// Restore the English 25-word mnemonic used by the mobile wallets.
    pub fn open_from_mnemonic(
        words: Zeroizing<String>,
        restore_height: u64,
    ) -> Result<Self, OpenError> {
        Self::open_from_mnemonic_on_network(words, restore_height, Network::Mainnet)
    }

    pub fn open_from_mnemonic_on_network(
        words: Zeroizing<String>,
        restore_height: u64,
        network: Network,
    ) -> Result<Self, OpenError> {
        WalletIdentity::from_mnemonic(words, restore_height, network)?.open()
    }

    /// Open from canonical 32-byte spend/view secret keys (Monero CLI hex order).
    pub fn open_from_key_bytes(
        label: &str,
        spend_secret: &[u8; 32],
        view_secret: &[u8; 32],
        restore_height: u64,
    ) -> Result<Self, OpenError> {
        Self::open_from_key_bytes_on_network(label, spend_secret, view_secret, restore_height, Network::Mainnet)
    }

    pub fn open_from_key_bytes_on_network(
        label: &str,
        spend_secret: &[u8; 32],
        view_secret: &[u8; 32],
        restore_height: u64,
        network: Network,
    ) -> Result<Self, OpenError> {
        let mut spend_slice: &[u8] = spend_secret.as_slice();
        let mut view_slice: &[u8] = view_secret.as_slice();
        let spend_scalar = Zeroizing::new(
            Scalar::read(&mut spend_slice).map_err(|_| OpenError::BadKey)?,
        );
        let view_scalar =
            Zeroizing::new(Scalar::read(&mut view_slice).map_err(|_| OpenError::BadKey)?);
        Self::open_from_scalars(label, spend_scalar, view_scalar, restore_height, network)
    }

    fn open_from_scalars(
        label: &str,
        spend_scalar: Zeroizing<Scalar>,
        view_scalar: Zeroizing<Scalar>,
        restore_height: u64,
        network: Network,
    ) -> Result<Self, OpenError> {
        let spend_pub = Point::from(&(*spend_scalar).into() * ED25519_BASEPOINT_TABLE);
        let view = ViewPair::new(spend_pub, view_scalar).map_err(OpenError::ViewPair)?;
        let address = view.legacy_address(network).to_string();
        Ok(Self {
            open: true,
            network,
            height: restore_height,
            balance_atomic: 0,
            unlocked_atomic: 0,
            scanned: false,
            history_count: 0,
            outputs: alloc::vec::Vec::new(),
            pending_tx_hex: alloc::string::String::new(),
            pending_fee_atomic: 0,
            pending_send_amount: 0,
            recent_sent: alloc::vec::Vec::new(),
            activity: [(0, 0); 5],
            activity_len: 0,
            view: Some(view),
            spend_secret: Some(spend_scalar),
            address,
            label: String::from(label),
        })
    }

    pub fn view_pair(&self) -> Option<&ViewPair> {
        self.view.as_ref()
    }

    pub fn spend_secret(&self) -> Option<&Zeroizing<Scalar>> {
        self.spend_secret.as_ref()
    }

    /// Rebuild after a bump-heap reset: keys/output were cloned off-heap first.
    pub fn reopen_with_output(
        label: &str,
        spend_secret: Zeroizing<Scalar>,
        view: ViewPair,
        restore_height: u64,
        output: WalletOutput,
        network: Network,
    ) -> Self {
        let address = view.legacy_address(network).to_string();
        let mut state = Self {
            open: true,
            network,
            height: restore_height,
            balance_atomic: 0,
            unlocked_atomic: 0,
            scanned: false,
            history_count: 0,
            outputs: alloc::vec::Vec::new(),
            pending_tx_hex: alloc::string::String::new(),
            pending_fee_atomic: 0,
            pending_send_amount: 0,
            recent_sent: alloc::vec::Vec::new(),
            activity: [(0, 0); 5],
            activity_len: 0,
            view: Some(view),
            spend_secret: Some(spend_secret),
            address,
            label: String::from(label),
        };
        state.apply_scan_outputs(core::slice::from_ref(&output));
        state
    }

    /// Record scan outputs without publishing partial balances or history.
    pub fn append_restore_outputs(&mut self, outputs: &[WalletOutput]) {
        self.outputs.extend_from_slice(outputs);
        self.history_count = self.outputs.len() as u32;
        self.scanned = false;
        self.balance_atomic = 0;
        self.unlocked_atomic = 0;
    }

    pub fn set_recent_activity(&mut self, events: &[(bool, u64)]) {
        self.activity = [(0, 0); 5];
        let skip = events.len().saturating_sub(5);
        self.activity_len = 0;
        for (received, amount) in events.iter().skip(skip) {
            self.activity[self.activity_len as usize] = (if *received { 1 } else { -1 }, *amount);
            self.activity_len += 1;
        }
    }

    pub fn apply_scan_outputs(&mut self, outputs: &[WalletOutput]) {
        self.outputs.extend_from_slice(outputs);
        let bal = outputs_balance(&self.outputs);
        self.balance_atomic = bal;
        self.unlocked_atomic = bal;
        self.history_count = self.outputs.len() as u32;
        for output in outputs {
            self.note_activity(true, output.commitment().amount);
        }
        self.scanned = true;
    }

    pub fn clear_scan_results(&mut self) {
        self.outputs.clear();
        self.balance_atomic = 0;
        self.unlocked_atomic = 0;
        self.history_count = 0;
        self.scanned = false;
    }

    /// Discard results from a reorganized chain without losing a pending send.
    pub fn clear_restore_history(&mut self) {
        self.clear_scan_results();
        self.activity = [(0, 0); 5];
        self.activity_len = 0;
    }

    /// Tip hang finished with no outs (SCAN 0) — still counts as a completed scan
    /// so prepare returns INSUFFICIENT instead of NEED SCAN → refresh thrash.
    pub fn mark_scanned_empty(&mut self) {
        self.outputs.clear();
        self.balance_atomic = 0;
        self.unlocked_atomic = 0;
        self.history_count = 0;
        self.scanned = true;
    }

    pub fn address(&self) -> &str {
        self.address.as_str()
    }

    pub fn label(&self) -> &str {
        self.label.as_str()
    }

    /// Build a scanner for the open view pair (clone of keys).
    pub fn scanner(&self) -> Option<Scanner> {
        self.view.as_ref().map(|view| Scanner::new(view.clone()))
    }

    pub fn format_status(&self, out: &mut [u8]) -> Option<usize> {
        if !self.open {
            return write_bytes(out, b"NEXAWAL | CLOSED");
        }
        let mut n = write_bytes(out, b"NEXAWAL | OPEN ")?;
        n += write_bytes(&mut out[n..], self.label.as_bytes())?;
        // Full primary address for Receive/copy (standard NexaWal face).
        n += write_bytes(&mut out[n..], b" | ADDR ")?;
        n += write_bytes(&mut out[n..], self.address.as_bytes())?;
        n += write_bytes(&mut out[n..], b" | H")?;
        n += write_u64(&mut out[n..], self.height)?;
        n += write_bytes(&mut out[n..], b" | BAL ")?;
        n += write_u64(&mut out[n..], self.balance_atomic)?;
        n += write_bytes(&mut out[n..], b" | UNLOCKED ")?;
        n += write_u64(&mut out[n..], self.unlocked_atomic)?;
        if self.scanned {
            n += write_bytes(&mut out[n..], b" | SCAN ")?;
            n += write_u64(&mut out[n..], self.history_count as u64)?;
        }
        // Confirmed journal activity is shown after a complete scan even while
        // signing remains disabled.
        if self.activity_len > 0 {
                n += write_bytes(&mut out[n..], b" | HIST ")?;
                for i in 0..self.activity_len as usize {
                    if i > 0 {
                        n += write_bytes(&mut out[n..], b";")?;
                    }
                    let (sign, amount) = self.activity[i];
                    n += write_bytes(&mut out[n..], if sign < 0 { b"-" } else { b"+" })?;
                    n += write_u64(&mut out[n..], amount)?;
                }
            } else if self.scanned && (!self.outputs.is_empty() || !self.recent_sent.is_empty()) {
                n += write_bytes(&mut out[n..], b" | HIST ")?;
                let mut first = true;
                let take = self.outputs.len().min(5);
                for o in self.outputs.iter().rev().take(take) {
                    if !first {
                        n += write_bytes(&mut out[n..], b";")?;
                    }
                    first = false;
                    n += write_bytes(&mut out[n..], b"+")?;
                    n += write_u64(&mut out[n..], o.commitment().amount)?;
                }
                for amount in &self.recent_sent {
                    if !first {
                        n += write_bytes(&mut out[n..], b";")?;
                    }
                    first = false;
                    n += write_bytes(&mut out[n..], b"-")?;
                    n += write_u64(&mut out[n..], *amount)?;
                }
            }
        Some(n)
    }

    pub fn record_sent(&mut self, amount: u64) {
        if amount == 0 {
            return;
        }
        if self.recent_sent.len() >= 5 {
            self.recent_sent.remove(0);
        }
        self.recent_sent.push(amount);
        self.note_activity(false, amount);
    }

    /// Remember a real output amount. `received` is false when its key image is spent
    /// or this boot broadcast it. Repeats of the same sign and amount are ignored.
    pub fn note_activity(&mut self, received: bool, amount: u64) {
        if amount <= 1 {
            return;
        }
        let sign: i8 = if received { 1 } else { -1 };
        let n = self.activity_len as usize;
        if self.activity[..n].contains(&(sign, amount)) {
            return;
        }
        if n == self.activity.len() {
            self.activity.copy_within(1.., 0);
            self.activity_len -= 1;
        }
        let i = self.activity_len as usize;
        self.activity[i] = (sign, amount);
        self.activity_len += 1;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    ViewPair(ViewPairError),
    BadKey,
}

/// Decode 64 hex chars into 32 bytes (no_std).
pub fn decode_hex32(hex: &[u8]) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_nibble(hex[i * 2])?;
        let lo = hex_nibble(hex[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn write_bytes(out: &mut [u8], bytes: &[u8]) -> Option<usize> {
    if bytes.len() > out.len() {
        return None;
    }
    out[..bytes.len()].copy_from_slice(bytes);
    Some(bytes.len())
}

fn write_u64(out: &mut [u8], mut value: u64) -> Option<usize> {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    if value == 0 {
        i -= 1;
        digits[i] = b'0';
    } else {
        while value > 0 {
            i -= 1;
            digits[i] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    write_bytes(out, &digits[i..])
}

/// Durable wallet blob store (vault-backed on NexatrodeOS).
pub trait WalletStore {
    fn load(&mut self, out: &mut [u8]) -> Result<usize, StoreError>;
    fn save(&mut self, blob: &[u8]) -> Result<(), StoreError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    Missing,
    Corrupt,
    Denied,
    Full,
}

/// Daemon JSON/bin RPC (net/TLS PD on NexatrodeOS).
pub trait DaemonRpc {
    fn get_height(&mut self) -> Result<u64, RpcError>;
    fn fetch_blocks(&mut self, start: u64, count: u64, out: &mut [u8]) -> Result<usize, RpcError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpcError {
    Offline,
    BadResponse,
    Truncated,
}

/// Parallel ownership scan (single PD loop now; scan_worker PDs later).
pub trait ParallelScan {
    fn scan_range(&mut self, start: u64, end: u64) -> Result<u64, ScanError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanError {
    NotReady,
    Range,
}

pub fn open_wallet(label: &str) -> Result<WalletState, OpenError> {
    WalletState::open_from_seed(label, label.as_bytes())
}

pub fn refresh_stub(state: &mut WalletState, tip: u64) {
    if state.open && tip > state.height {
        state.height = tip;
    }
}

pub fn balance_atomic(state: &WalletState) -> u64 {
    state.balance_atomic
}

/// Read the height only from a complete daemon JSON result.
pub fn parse_daemon_height(json: &[u8]) -> Option<u64> {
    let response: serde_json::Value = serde_json::from_slice(json).ok()?;
    response.get("result").unwrap_or(&response).get("height")?.as_u64()
}

/// JSON-RPC body for a tip-height probe (HTTP POST /json_rpc).
pub fn get_info_request_body() -> &'static [u8] {
    br#"{"jsonrpc":"2.0","id":"0","method":"get_info","params":{}}"#
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareError {
    Closed,
    BadAddress,
    /// A previously signed payment must be reconciled before another is built.
    Pending,
    Insufficient,
    /// Tip sync only so far — no owned outputs to spend.
    NeedScan,
    /// Have outputs but decoy/fee selection RPC is not wired yet.
    NeedDecoys,
    /// SignableTransaction / CLSAG failed.
    SignFail,
}

/// Current mainnet CLSAG ring size (Bulletproof+).
pub const RING_LEN: u8 = 16;

/// Absolute-index ring member: (index, key, commitment) compressed points.
pub type RingMember = (u64, [u8; 32], [u8; 32]);

/// Match the reference selector's subgroup and identity checks for node outputs.
fn valid_ring_points(key: monero_wallet::ed25519::Point, commitment: monero_wallet::ed25519::Point) -> bool {
    use curve25519_dalek::{edwards::EdwardsPoint, traits::IsIdentity};
    let key: EdwardsPoint = key.into();
    let commitment: EdwardsPoint = commitment.into();
    key.is_torsion_free() && commitment.is_torsion_free() && !key.is_identity()
}

/// Validate dest + amount against an open wallet. Full tx build needs scanned outputs
/// plus decoy selection (landed after prepare gates prove out on-device).
pub fn prepare_send(state: &WalletState, dest: &str, amount: u64) -> Result<(), PrepareError> {
    prepare_gates(state, dest, amount)?;
    // Outputs exist — SignableTransaction still needs OutputWithDecoys from daemon.
    Err(PrepareError::NeedDecoys)
}

fn prepare_gates(state: &WalletState, dest: &str, amount: u64) -> Result<(), PrepareError> {
    if !state.pending_tx_hex.is_empty() { return Err(PrepareError::Pending); }
    if !state.open {
        return Err(PrepareError::Closed);
    }
    if MoneroAddress::from_str(state.network, dest).is_err() {
        return Err(PrepareError::BadAddress);
    }
    if amount == 0 {
        return Err(PrepareError::Insufficient);
    }
    if !state.scanned {
        return Err(PrepareError::NeedScan);
    }
    if state.balance_atomic == 0 || state.outputs.is_empty() {
        return Err(PrepareError::Insufficient);
    }
    if state.balance_atomic < amount {
        return Err(PrepareError::Insufficient);
    }
    Ok(())
}

/// Prefer the largest owned out (skip 1-piconero dust siblings in the same tx).
pub fn primary_spend_output(state: &WalletState) -> Option<&WalletOutput> {
    state
        .outputs
        .iter()
        .max_by_key(|o| o.commitment().amount)
}

/// Key image for the primary (largest) owned output.
pub fn first_output_key_image(state: &WalletState) -> Option<[u8; 32]> {
    let output = primary_spend_output(state)?;
    output_key_image(state, output)
}

/// Derive the key image of a specific owned output for spent reconciliation.
pub fn output_key_image(state: &WalletState, output: &WalletOutput) -> Option<[u8; 32]> {
    use core::ops::Deref;
    use monero_wallet::ed25519::Point;
    let spend = state.spend_secret()?;
    let spend_d = Zeroizing::new((**spend).into());
    let input_key = Zeroizing::new(spend_d.deref() + output.key_offset().into());
    let ki = Point::from(
        input_key.deref() * Point::biased_hash(output.key().compress().to_bytes()).into(),
    );
    Some(ki.compress().to_bytes())
}

/// Build CLSAG decoys from absolute-index ring members (must include the real spend).
pub fn build_decoys_for_output(
    output: &WalletOutput,
    members: &[RingMember],
) -> Option<monero_wallet::ringct::clsag::Decoys> {
    use monero_wallet::ed25519::CompressedPoint;
    use monero_wallet::ringct::clsag::Decoys;

    if members.len() != usize::from(RING_LEN) {
        return None;
    }
    let real = output.index_on_blockchain();
    let mut ring: alloc::vec::Vec<(u64, [monero_wallet::ed25519::Point; 2])> =
        alloc::vec::Vec::with_capacity(members.len());
    for &(idx, key_b, mask_b) in members {
        let key = CompressedPoint::from(key_b).decompress()?;
        let commit = CompressedPoint::from(mask_b).decompress()?;
        if !valid_ring_points(key, commit) { return None; }
        ring.push((idx, [key, commit]));
    }
    // Refuse a node response that disagrees with the scanned output rather
    // than silently replacing it with local data.
    let local = [output.key(), output.commitment().commit()];
    if !ring.iter().any(|(i, points)| *i == real && *points == local) { return None; }
    ring.sort_by_key(|(i, _)| *i);
    ring.dedup_by_key(|(i, _)| *i);
    if ring.len() != usize::from(RING_LEN) {
        return None;
    }
    let mut offsets = alloc::vec::Vec::with_capacity(ring.len());
    offsets.push(ring[0].0);
    for m in 1..ring.len() {
        offsets.push(ring[m].0.saturating_sub(ring[m - 1].0));
    }
    let signer = ring.partition_point(|(i, _)| *i < real) as u8;
    Decoys::new(
        offsets,
        signer,
        ring.into_iter().map(|(_, p)| p).collect(),
    )
}

/// Build a transaction from previously checked inputs and independently
/// selected rings. This remains gated by `state.scanned`; the service must not
/// set that flag until its chain, spent checks and ring selection are ready.
pub fn prepare_restored_transaction(
    state: &mut WalletState, metadata: &[RestoredOutput], chain_height: u64,
    inputs: &[(usize, alloc::vec::Vec<RingMember>)], dest: &str, amount: u64,
    send_max: bool, fee_rate: monero_wallet::interface::FeeRate, seed: Zeroizing<[u8; 32]>,
) -> Result<(), PrepareError> {
    build_restored_transaction(state, metadata, chain_height, inputs, dest, amount, send_max, fee_rate, seed, false, false).map(|_| ())
}

/// Estimate a payment without signing it or changing pending state. Callers
/// must revalidate the inputs, fee rate, and destination before later signing.
pub fn preview_restored_transaction(
    state: &mut WalletState, metadata: &[RestoredOutput], chain_height: u64,
    inputs: &[(usize, alloc::vec::Vec<RingMember>)], dest: &str, amount: u64,
    send_max: bool, fee_rate: monero_wallet::interface::FeeRate, seed: Zeroizing<[u8; 32]>,
) -> Result<(u64, u64), PrepareError> {
    build_restored_transaction(state, metadata, chain_height, inputs, dest, amount, send_max, fee_rate, seed, true, false)
}

/// Preview against the service's fully checked scan without enabling signing.
pub fn preview_verified_restored_transaction(
    state: &mut WalletState, metadata: &[RestoredOutput], chain: &RestoreChain, chain_height: u64,
    inputs: &[(usize, alloc::vec::Vec<RingMember>)], dest: &str, amount: u64,
    send_max: bool, fee_rate: monero_wallet::interface::FeeRate, seed: Zeroizing<[u8; 32]>,
) -> Result<(u64, u64), PrepareError> {
    if !chain.is_verified() || chain.next() != chain_height || chain.anchor().is_none() { return Err(PrepareError::NeedScan); }
    build_restored_transaction(state, metadata, chain_height, inputs, dest, amount, send_max, fee_rate, seed, true, true)
}

/// Immutable terms displayed to the user. The wallet and chain anchor bind a
/// confirmation to the scan that produced its estimate.
pub struct PaymentPreview {
    wallet_address: String,
    destination: String,
    requested_amount: u64,
    send_max: bool,
    amount: u64,
    fee: u64,
    chain_height: u64,
    anchor: (u64, [u8; 32]),
}

impl PaymentPreview {
    pub fn destination(&self) -> &str { &self.destination }
    pub fn amount(&self) -> u64 { self.amount }
    pub fn fee(&self) -> u64 { self.fee }
}

pub fn create_payment_preview(
    state: &mut WalletState, metadata: &[RestoredOutput], chain: &RestoreChain, chain_height: u64,
    inputs: &[(usize, alloc::vec::Vec<RingMember>)], dest: &str, amount: u64,
    send_max: bool, fee_rate: monero_wallet::interface::FeeRate,
) -> Result<PaymentPreview, PrepareError> {
    let (pay, fee) = preview_verified_restored_transaction(state, metadata, chain, chain_height,
        inputs, dest, amount, send_max, fee_rate, Zeroizing::new([0; 32]))?;
    Ok(PaymentPreview { wallet_address: String::from(state.address()), destination: String::from(dest),
        requested_amount: amount, send_max, amount: pay, fee, chain_height,
        anchor: chain.anchor().ok_or(PrepareError::NeedScan)? })
}

fn sign_confirmed_preview(
    state: &mut WalletState, metadata: &[RestoredOutput], chain: &RestoreChain,
    inputs: &[(usize, alloc::vec::Vec<RingMember>)], preview: &PaymentPreview,
    fee_rate: monero_wallet::interface::FeeRate, seed: Zeroizing<[u8; 32]>,
) -> Result<(), PrepareError> {
    if state.address() != preview.wallet_address || chain.anchor() != Some(preview.anchor)
        || !chain.is_verified() || chain.next() != preview.chain_height { return Err(PrepareError::NeedScan); }
    sign_verified_restored_transaction(state, metadata, chain, preview.chain_height, inputs,
        &preview.destination, preview.requested_amount, preview.send_max, fee_rate,
        (preview.amount, preview.fee), seed)
}

/// Ordered evidence gathered from the selected node immediately before signing.
/// Any mismatch consumes the current attempt; callers start again after sync.
pub struct PaymentRevalidation {
    wallet: String,
    destination: String,
    amount: u64,
    fee: u64,
    height: u64,
    anchor: (u64, [u8; 32]),
    images: alloc::vec::Vec<[u8; 32]>,
    phase: u8,
}

pub struct PaymentRevalidationProof {
    validation: PaymentRevalidation,
    fee_rate: monero_wallet::interface::FeeRate,
}

impl PaymentRevalidation {
    pub fn new(state: &WalletState, inputs: &[(usize, alloc::vec::Vec<RingMember>)], preview: &PaymentPreview) -> Option<Self> {
        if !state.open || state.address() != preview.wallet_address || inputs.is_empty() { return None; }
        let mut images = alloc::vec::Vec::with_capacity(inputs.len());
        for (index, _) in inputs {
            let image = output_key_image(state, state.outputs.get(*index)?)?;
            if images.contains(&image) { return None; }
            images.push(image);
        }
        Some(Self { wallet: preview.wallet_address.clone(), destination: preview.destination.clone(),
            amount: preview.amount, fee: preview.fee, height: preview.chain_height, anchor: preview.anchor,
            images, phase: 0 })
    }

    pub fn key_images(&self) -> &[[u8; 32]] { &self.images }

    pub fn observe_info_response(&mut self, json: &[u8], network: Network) -> bool {
        let valid = daemon_network_matches(json, network);
        self.observe_height(valid, parse_daemon_height(json).unwrap_or(u64::MAX))
    }

    pub fn observe_anchor_response(&mut self, json: &[u8]) -> bool {
        let block = serde_json::from_slice::<serde_json::Value>(json).ok()
            .and_then(|response| {
                let result = response.get("result")?;
                if result.get("status")?.as_str()? != "OK" { return None; }
                decode_block_blob(result.get("blob")?.as_str()?.as_bytes())
            });
        let Some(block) = block else { self.phase = u8::MAX; return false; };
        let Some(height) = u64::try_from(block.number()).ok() else { self.phase = u8::MAX; return false; };
        self.observe_anchor(height, block.hash())
    }

    pub fn observe_height(&mut self, network_matches: bool, height: u64) -> bool {
        if self.phase != 0 || !network_matches || height != self.height { self.phase = u8::MAX; return false; }
        self.phase = 1; true
    }

    pub fn observe_anchor(&mut self, height: u64, hash: [u8; 32]) -> bool {
        if !matches!(self.phase, 1 | 3) || self.anchor != (height, hash) { self.phase = u8::MAX; return false; }
        self.phase += 1; true
    }

    pub fn observe_spent(&mut self, statuses: &[u8]) -> bool {
        if self.phase != 2 || statuses.len() != self.images.len() || statuses.iter().any(|status| *status != 0) {
            self.phase = u8::MAX; return false;
        }
        self.phase = 3; true
    }

    pub fn finish(self, fee_rate: monero_wallet::interface::FeeRate) -> Option<PaymentRevalidationProof> {
        (self.phase == 4).then_some(PaymentRevalidationProof { validation: self, fee_rate })
    }

    pub fn finish_fee_response(self, json: &[u8]) -> Option<PaymentRevalidationProof> {
        self.finish(parse_normal_fee_rate(json)?)
    }
}

pub fn sign_revalidated_preview(
    state: &mut WalletState, metadata: &[RestoredOutput], chain: &RestoreChain,
    inputs: &[(usize, alloc::vec::Vec<RingMember>)], preview: &PaymentPreview,
    proof: PaymentRevalidationProof, seed: Zeroizing<[u8; 32]>,
) -> Result<(), PrepareError> {
    let validation = proof.validation;
    if validation.wallet != preview.wallet_address || validation.destination != preview.destination
        || validation.amount != preview.amount || validation.fee != preview.fee
        || validation.height != preview.chain_height || validation.anchor != preview.anchor
        || inputs.len() != validation.images.len() { return Err(PrepareError::NeedScan); }
    for ((index, _), expected) in inputs.iter().zip(&validation.images) {
        if state.outputs.get(*index).and_then(|output| output_key_image(state, output)) != Some(*expected) {
            return Err(PrepareError::NeedScan);
        }
    }
    sign_confirmed_preview(state, metadata, chain, inputs, preview, proof.fee_rate, seed)
}

/// Sign only if a fresh calculation matches the amount and fee confirmed by
/// the user. Callers must bind `dest` to the confirmed preview and persist the
/// resulting bytes before any relay.
pub fn sign_verified_restored_transaction(
    state: &mut WalletState, metadata: &[RestoredOutput], chain: &RestoreChain, chain_height: u64,
    inputs: &[(usize, alloc::vec::Vec<RingMember>)], dest: &str, amount: u64,
    send_max: bool, fee_rate: monero_wallet::interface::FeeRate, expected: (u64, u64), seed: Zeroizing<[u8; 32]>,
) -> Result<(), PrepareError> {
    if !chain.is_verified() || chain.next() != chain_height || chain.anchor().is_none() { return Err(PrepareError::NeedScan); }
    let preview = build_restored_transaction(state, metadata, chain_height, inputs, dest, amount, send_max, fee_rate, Zeroizing::new(*seed), true, true)?;
    if preview != expected { return Err(PrepareError::SignFail); }
    let signed = build_restored_transaction(state, metadata, chain_height, inputs, dest, amount, send_max, fee_rate, seed, false, true)?;
    if signed != expected {
        state.pending_tx_hex.clear();
        state.pending_fee_atomic = 0;
        state.pending_send_amount = 0;
        return Err(PrepareError::SignFail);
    }
    Ok(())
}

fn build_restored_transaction(
    state: &mut WalletState, metadata: &[RestoredOutput], chain_height: u64,
    inputs: &[(usize, alloc::vec::Vec<RingMember>)], dest: &str, amount: u64,
    send_max: bool, fee_rate: monero_wallet::interface::FeeRate, seed: Zeroizing<[u8; 32]>, preview: bool, verified_scan: bool,
) -> Result<(u64, u64), PrepareError> {
    use monero_wallet::{OutputWithDecoys, ringct::RctType, send::{Change, SendError, SignableTransaction}};
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;
    if !state.pending_tx_hex.is_empty() { return Err(PrepareError::Pending); }
    if !state.open { return Err(PrepareError::Closed); }
    let address = MoneroAddress::from_str(state.network, dest).map_err(|_| PrepareError::BadAddress)?;
    if !(state.scanned || verified_scan) || state.outputs.len() != metadata.len() { return Err(PrepareError::NeedScan); }
    restored_balances(metadata, chain_height).ok_or(PrepareError::NeedScan)?;
    if inputs.is_empty() { return Err(PrepareError::Insufficient); }
    let mut selected = alloc::vec::Vec::with_capacity(inputs.len());
    let mut sum = 0u64;
    for (position, (index, ring)) in inputs.iter().enumerate() {
        if inputs[..position].iter().any(|(previous, _)| previous == index) { return Err(PrepareError::NeedScan); }
        let meta = metadata.get(*index).ok_or(PrepareError::NeedScan)?;
        let output = state.outputs.get(*index).ok_or(PrepareError::NeedScan)?;
        if meta.amount != output.commitment().amount || meta.spent_status != Some(0)
            || !restored_output_unlocked(meta, chain_height) { return Err(PrepareError::NeedScan); }
        sum = sum.checked_add(meta.amount).ok_or(PrepareError::Insufficient)?;
        let decoys = build_decoys_for_output(output, ring).ok_or(PrepareError::NeedDecoys)?;
        selected.push(OutputWithDecoys::from_wallet_output(output, decoys));
    }
    if send_max && metadata.iter().enumerate().any(|(index, meta)|
        meta.spent_status == Some(0) && restored_output_unlocked(meta, chain_height)
            && !inputs.iter().any(|(selected, _)| *selected == index)) { return Err(PrepareError::NeedScan); }
    let mut pay = if send_max { sum } else { amount };
    if pay == 0 || pay > sum { return Err(PrepareError::Insufficient); }
    for _ in 0..4 {
        let view = state.view_pair().cloned().ok_or(PrepareError::Closed)?;
        let intent = match SignableTransaction::new(RctType::ClsagBulletproofPlus, Zeroizing::new(*seed),
            selected.clone(), alloc::vec![(address, pay)], Change::new(view, None), alloc::vec::Vec::new(), fee_rate) {
            Ok(intent) => intent,
            Err(SendError::NotEnoughFunds { necessary_fee: Some(fee), .. }) if send_max && fee < sum => {
                let next = sum - fee;
                if next == pay { return Err(PrepareError::SignFail); }
                pay = next; continue;
            }
            Err(SendError::NotEnoughFunds { .. }) => return Err(PrepareError::Insufficient),
            Err(_) => return Err(PrepareError::SignFail),
        };
        if preview { return Ok((pay, intent.necessary_fee())); }
        let mut rng = ChaCha20Rng::from_seed(*seed);
        let signed = intent.sign(&mut rng, state.spend_secret().ok_or(PrepareError::Closed)?).map_err(|_| PrepareError::SignFail)?;
        let fee = match &signed { Transaction::V2 { proofs: Some(proofs), .. } => proofs.base.fee, _ => return Err(PrepareError::SignFail) };
        let bytes = signed.serialize();
        if bytes.len() > PendingTransaction::MAX_HEX_LENGTH / 2 { return Err(PrepareError::SignFail); }
        let mut hex = alloc::string::String::with_capacity(bytes.len() * 2);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for b in bytes { hex.push(HEX[(b >> 4) as usize] as char); hex.push(HEX[(b & 15) as usize] as char); }
        state.pending_tx_hex = hex;
        state.pending_fee_atomic = fee;
        state.pending_send_amount = pay;
        return Ok((pay, fee));
    }
    Err(PrepareError::SignFail)
}

/// Build + sign a self-change tx; stores hex in `state.pending_tx_hex`.
/// `send_max` pays the primary output minus the necessary fee.
pub fn prepare_and_sign(
    state: &mut WalletState,
    dest: &str,
    amount: u64,
    ring_members: &[RingMember],
    send_max: bool,
    seed: Zeroizing<[u8; 32]>,
) -> Result<(), PrepareError> {
    use monero_wallet::OutputWithDecoys;
    use monero_wallet::interface::FeeRate;
    use monero_wallet::ringct::RctType;
    use monero_wallet::send::{Change, SendError, SignableTransaction};
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    if !state.pending_tx_hex.is_empty() { return Err(PrepareError::Pending); }
    if send_max {
        if !state.open {
            return Err(PrepareError::Closed);
        }
        if MoneroAddress::from_str(state.network, dest).is_err() {
            return Err(PrepareError::BadAddress);
        }
        if !state.scanned {
            return Err(PrepareError::NeedScan);
        }
        if state.balance_atomic == 0 || state.outputs.is_empty() {
            return Err(PrepareError::Insufficient);
        }
    } else {
        prepare_gates(state, dest, amount)?;
    }
    let output_amount = primary_spend_output(state)
        .ok_or(PrepareError::Insufficient)?
        .commitment()
        .amount;
    let mut pay = if send_max { output_amount } else { amount };
    if pay == 0 {
        return Err(PrepareError::Insufficient);
    }
    let fee_rate = FeeRate::new(20_000, 10_000).ok_or(PrepareError::SignFail)?;
    let mut signed_hex = None;
    for attempt in 0..2 {
        let spend = state.spend_secret().ok_or(PrepareError::Closed)?;
        let view = state.view_pair().cloned().ok_or(PrepareError::Closed)?;
        let output = primary_spend_output(state).ok_or(PrepareError::Insufficient)?;
        let decoys =
            build_decoys_for_output(output, ring_members).ok_or(PrepareError::NeedDecoys)?;
        let input = OutputWithDecoys::from_wallet_output(output, decoys);
        let addr = MoneroAddress::from_str(state.network, dest)
            .map_err(|_| PrepareError::BadAddress)?;
        let outgoing = Zeroizing::new(*seed);
        let mut rng = ChaCha20Rng::from_seed(*seed);
        let tx = match SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            outgoing,
            alloc::vec![input],
            alloc::vec![(addr, pay)],
            Change::new(view, None),
            alloc::vec::Vec::new(),
            fee_rate,
        ) {
            Ok(tx) => tx,
            Err(SendError::NotEnoughFunds {
                necessary_fee: Some(fee),
                ..
            }) if send_max && attempt == 0 && fee > 0 && fee < output_amount => {
                pay = output_amount - fee;
                continue;
            }
            Err(_) => return Err(PrepareError::SignFail),
        };
        let signed = tx.sign(&mut rng, spend).map_err(|_| PrepareError::SignFail)?;
        let fee = match &signed {
            Transaction::V2 {
                proofs: Some(proofs),
                ..
            } => proofs.base.fee,
            _ => 0,
        };
        let bytes = signed.serialize();
        if bytes.len() > PendingTransaction::MAX_HEX_LENGTH / 2 { return Err(PrepareError::SignFail); }
        let mut hex = alloc::string::String::with_capacity(bytes.len() * 2);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for b in bytes {
            hex.push(HEX[(b >> 4) as usize] as char);
            hex.push(HEX[(b & 0xf) as usize] as char);
        }
        signed_hex = Some((hex, fee, pay));
        break;
    }
    let Some((hex, fee, pay)) = signed_hex else {
        return Err(PrepareError::SignFail);
    };
    state.pending_tx_hex = hex;
    state.pending_fee_atomic = fee;
    state.pending_send_amount = pay;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    Closed,
    NeedScan,
    Insufficient,
    NeedPrepare,
}

/// Gate broadcast: requires a pending signed hex from prepare (decoy path).
pub fn send_prepared(state: &WalletState) -> Result<&str, SendError> {
    if !state.open {
        return Err(SendError::Closed);
    }
    // Once a transaction has been signed, relay must use those exact bytes.
    // A reboot may leave the scan incomplete or the inputs already spent by
    // this very transaction; neither condition justifies creating a new one.
    if state.pending_tx_hex.is_empty() {
        return Err(SendError::NeedPrepare);
    }
    Ok(state.pending_tx_hex.as_str())
}

/// JSON body for monerod `/send_raw_transaction`.
pub fn build_send_raw_body(tx_hex: &str, out: &mut [u8]) -> Option<usize> {
    let mut n = 0usize;
    macro_rules! push {
        ($b:expr) => {{
            let b: &[u8] = $b;
            if n + b.len() > out.len() {
                return None;
            }
            out[n..n + b.len()].copy_from_slice(b);
            n += b.len();
        }};
    }
    push!(br#"{"tx_as_hex":""#);
    push!(tx_hex.as_bytes());
    push!(br#"","do_sanity_checks":false}"#);
    Some(n)
}


#[cfg(test)]
mod mnemonic_tests {
    use super::*;

    #[test]
    fn pending_transaction_round_trips_and_rejects_truncation() {
        let pending = PendingTransaction {
            wallet_address: String::from("48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN"),
            tx_hex: Zeroizing::new("a1b2".into()),
            fee: 120,
            amount: 900,
        };
        let encoded = pending.encode().unwrap();
        let restored = PendingTransaction::decode(&encoded).unwrap();
        assert_eq!(restored.tx_hex.as_str(), "a1b2");
        assert_eq!((restored.fee, restored.amount), (120, 900));
        assert!(!restored.belongs_to(&WalletState::closed()));
        assert!(restored.transaction_id().is_none());
        let tx = PendingTransaction {
            wallet_address: pending.wallet_address.clone(),
            tx_hex: Zeroizing::new(String::from("02f78dae0101ffbb8dae0101e0b2d2b9c21103e6854544fbb66d55fc3546f4d3e69f8234257b69fa2237712af3b058a5f01ba14a340173f263b8a4bbc46dfb6f29e0584adbfffdf7a47c929d77c2d0c142afea2b05300211000000f7eeeb3f0e00000000000000000000")),
            fee: 0,
            amount: 0,
        };
        assert_eq!(tx.transaction_id(), Some([
            0x37, 0x3a, 0x2a, 0xce, 0x62, 0x7d, 0xeb, 0xaf,
            0x8b, 0xfd, 0x49, 0x31, 0x55, 0xfd, 0x3c, 0x00,
            0xc5, 0xc2, 0xfc, 0x16, 0x44, 0x00, 0xec, 0x22,
            0xe7, 0x9e, 0xe7, 0x9a, 0x1a, 0xc4, 0x87, 0xc4,
        ]));
        assert!(tx.signed_payment_id().is_none()); // A miner transaction is not a retryable payment.
        assert!(PendingTransaction::decode(&encoded[..encoded.len() - 1]).is_none());
        let mut altered = encoded.clone();
        let last = altered.len() - 1;
        altered[last] = b'X';
        assert!(PendingTransaction::decode(&altered).is_none());
    }

    #[test]
    fn node_endpoint_and_network_validation() {
        let node = NodeEndpoint::parse("http://node.example.org:38081").unwrap();
        assert_eq!(node.host(), "node.example.org");
        assert_eq!(node.port, 38081);
        assert_eq!(node.authority(), "node.example.org:38081");
        assert_eq!(NodeEndpoint::parse("127.0.0.1:18081").unwrap().ipv4(), Some([127, 0, 0, 1]));
        for bad in ["https://node.example:443", "node.example", "1.2.3.999:80", "host:0", "host:65536", "user@host:80", "host:80/path", "host\r\nX:80"] {
            assert!(NodeEndpoint::parse(bad).is_none(), "accepted {bad}");
        }
        let response = br#"{"status":"OK","nettype":"stagenet","height":42}"#;
        assert!(daemon_network_matches(response, Network::Stagenet));
        assert!(!daemon_network_matches(response, Network::Mainnet));
        assert!(!daemon_network_matches(br#"{"status":"OK"}"#, Network::Mainnet));
        let wrapped = br#"{"jsonrpc":"2.0","result":{"status":"OK","nettype":"stagenet","height":42}}"#;
        assert!(daemon_network_matches(wrapped, Network::Stagenet));
        assert_eq!(parse_daemon_height(wrapped), Some(42));
        assert!(!daemon_network_matches(br#"{"result":{"status":"Failed","nested":{"status":"OK","nettype":"stagenet"}}}"#, Network::Stagenet));
        assert_eq!(parse_daemon_height(br#"{"result":{"nested":{"height":42}}}"#), None);
        assert_eq!(parse_daemon_height(br#"{"result":{"height":42}"#), None);
    }

    #[test]
    fn wallet_journal_preserves_pending_history_and_legacy_records() {
        let addr = "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN";
        let pending = PendingTransaction { wallet_address: addr.into(), tx_hex: Zeroizing::new("a1b2".into()), fee: 5, amount: 42 };
        let legacy = pending.encode().unwrap();
        let mut journal = WalletJournal::decode(&legacy).unwrap();
        assert_eq!(journal.pending.as_ref().unwrap().tx_hex.as_str(), "a1b2");
        let entry = ConfirmedSend { txid: [4; 32], block_hash: [5; 32], block_height: 10, amount: 123, fee: 7, tx_hex: None };
        journal.sent.push(entry.clone());
        let encoded = journal.encode().unwrap();
        let decoded = WalletJournal::decode(&encoded).unwrap();
        assert_eq!(decoded.wallet_address, addr);
        assert_eq!(decoded.pending.unwrap().tx_hex.as_str(), "a1b2");
        assert_eq!(decoded.sent, alloc::vec![entry.clone()]);
        // Version 1 journal entries had no transaction-length field.
        let mut v2 = encoded[..encoded.len() - 41].to_vec();
        v2[..8].copy_from_slice(b"NXJR0002");
        assert!(WalletJournal::decode(&v2).is_some());
        let mut v1 = v2[..v2.len() - 4].to_vec();
        v1[..8].copy_from_slice(b"NXJR0001");
        let migrated = WalletJournal::decode(&v1).unwrap();
        assert_eq!(migrated.sent, alloc::vec![entry.clone()]);
        assert_eq!(migrated.pending.unwrap().tx_hex.as_str(), "a1b2");
        assert!(WalletJournal::decode(&encoded[..encoded.len() - 1]).is_none());
        journal.sent.push(entry);
        assert!(journal.encode().is_none());
        journal.sent.clear();
        journal.pending = None;
        assert!(WalletJournal::decode(&journal.encode().unwrap()).unwrap().pending.is_none());
        for index in 0u16..140 {
            let mut id = [0u8; 32];
            id[..2].copy_from_slice(&index.to_le_bytes());
            journal.sent.push(ConfirmedSend { txid: id, block_hash: [5; 32], block_height: 10, amount: 1, fee: 1, tx_hex: None });
        }
        assert!(journal.encode().is_none());
        assert_eq!(journal.sent.len(), 140); // Capacity errors never drop history.

    }

    #[test]
    fn empty_scan_checkpoint_is_bounded_and_reorg_resets_to_original_start() {
        let addr = "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN";
        let mut journal = WalletJournal::empty(addr).unwrap();
        assert!(journal.permits_empty_scan_checkpoint(0, 0));
        assert!(!journal.permits_empty_scan_checkpoint(1, 0));
        assert!(!journal.permits_empty_scan_checkpoint(0, 1));
        let mut pending = WalletJournal::empty(addr).unwrap();
        pending.pending = Some(PendingTransaction { wallet_address: addr.into(), tx_hex: Zeroizing::new("aa".into()), fee: 1, amount: 1 });
        assert!(!pending.permits_empty_scan_checkpoint(0, 0));
        let mut sent = WalletJournal::empty(addr).unwrap();
        sent.sent.push(ConfirmedSend { txid: [1; 32], block_hash: [2; 32], block_height: 1, amount: 1, fee: 1, tx_hex: None });
        assert!(!sent.permits_empty_scan_checkpoint(0, 0));
        journal.empty_scan_checkpoint = Some((120, [7; 32]));
        let encoded = journal.encode().unwrap();
        let restored = WalletJournal::decode(&encoded).unwrap();
        assert_eq!(restored.empty_scan_checkpoint, Some((120, [7; 32])));
        let mut chain = RestoreChain::resume_empty(100, 120, [7; 32]).unwrap();
        assert_eq!(chain.anchor(), Some((119, [7; 32])));
        assert!(!chain.is_verified());
        assert!(!chain.anchor_matches(119, [8; 32]));
        chain.reset();
        assert_eq!(chain.next(), 100);
        assert!(chain.anchor().is_none());
        assert!(RestoreChain::resume_empty(100, 100, [7; 32]).is_none());
        let mut malformed = encoded.to_vec();
        let flag = malformed.len() - 41;
        malformed[flag] = 2;
        assert!(WalletJournal::decode(&malformed).is_none());
    }

    #[test]
    fn pending_observation_requires_matching_transaction_and_explicit_state() {
        let tx = "02f78dae0101ffbb8dae0101e0b2d2b9c21103e6854544fbb66d55fc3546f4d3e69f8234257b69fa2237712af3b058a5f01ba14a340173f263b8a4bbc46dfb6f29e0584adbfffdf7a47c929d77c2d0c142afea2b05300211000000f7eeeb3f0e00000000000000000000";
        let hash = "373a2ace627debaf8bfd493155fd3c00c5c2fc164400ec22e79ee79a1ac487c4";
        let record = PendingTransaction { wallet_address: String::new(), tx_hex: Zeroizing::new(tx.into()), fee: 0, amount: 0 };
        let pool = alloc::format!(r#"{{"status":"OK","txs":[{{"as_hex":"{}","in_pool":true}}],"missed_tx":[]}}"#, tx);
        assert_eq!(observe_pending_transaction(&record, pool.as_bytes()), Some(PendingObservation::InPool));
        let block = alloc::format!(r#"{{"status":"OK","txs":[{{"as_hex":"{}","in_pool":false,"block_height":42}}]}}"#, tx);
        assert_eq!(observe_pending_transaction(&record, block.as_bytes()), Some(PendingObservation::InBlock(42)));
        let missing = alloc::format!(r#"{{"status":"OK","txs":[],"missed_tx":["{}"]}}"#, hash);
        assert_eq!(observe_pending_transaction(&record, missing.as_bytes()), Some(PendingObservation::Missing));
        assert_eq!(observe_pending_transaction(&record, block.replace(tx, "00").as_bytes()), None);
        assert_eq!(observe_pending_transaction(&record, block.replace(",\"block_height\":42", "").as_bytes()), None);
        assert_eq!(observe_pending_transaction(&record, br#"{"status":"OK","txs":[],"missed_tx":[]}"#), None);
        assert_eq!(observe_pending_transaction(&record, pool.replace("\"missed_tx\":[]", "\"missed_tx\":null").as_bytes()), None);
        assert_eq!(observe_pending_transaction(&record, br#"{"status":"Failed","txs":[]}"#), None);
        assert_eq!(observe_pending_transaction(&record, b"{"), None);
    }

    #[test]
    fn journal_confirmation_requires_anchored_depth_and_preserves_original() {
        let addr = "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN";
        let tx = "02f78dae0101ffbb8dae0101e0b2d2b9c21103e6854544fbb66d55fc3546f4d3e69f8234257b69fa2237712af3b058a5f01ba14a340173f263b8a4bbc46dfb6f29e0584adbfffdf7a47c929d77c2d0c142afea2b05300211000000f7eeeb3f0e00000000000000000000";
        let mut journal = WalletJournal::empty(addr).unwrap();
        journal.pending = Some(PendingTransaction { wallet_address: addr.into(), tx_hex: Zeroizing::new(tx.into()), fee: 5, amount: 42 });
        let txid = journal.pending.as_ref().unwrap().transaction_id().unwrap();
        let mut evidence = PendingChainEvidence::new(txid);
        let mut chain = RestoreChain::new(10);
        assert!(chain.commit(10, [0; 32], [1; 32]));
        assert!(evidence.record_block(&chain, 10, [1; 32], &[txid]).is_ok());
        assert!(journal.confirmed_candidate(&evidence, &chain, 11, 1).is_none());
        assert!(chain.verify_tip(10, [1; 32], 11));
        assert!(evidence.finish(&chain, 11));
        assert!(journal.confirmed_candidate(&evidence, &chain, 11, 10).is_none());
        let candidate = journal.confirmed_candidate(&evidence, &chain, 11, 1).unwrap();
        assert!(journal.pending.is_some());
        assert!(candidate.pending.is_none());
        assert_eq!(candidate.sent[0].txid, txid);
        assert_eq!(candidate.sent[0].block_hash, [1; 32]);
        let restored = WalletJournal::decode(&candidate.encode().unwrap()).unwrap();
        assert_eq!(restored.sent, candidate.sent);
        let orphaned = restored.orphaned_candidate(txid).unwrap();
        assert_eq!(orphaned.pending.as_ref().unwrap().tx_hex.as_str(), tx);
        assert!(orphaned.sent.is_empty());
        assert!(restored.pending.is_none());
        assert!(orphaned.orphaned_candidate(txid).is_none());
        let mut already_pending = restored.clone();
        already_pending.pending = journal.pending.clone();
        assert!(already_pending.orphaned_candidate(txid).is_none());
        let mut corrupted = candidate.encode().unwrap();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 1;
        assert!(WalletJournal::decode(&corrupted).is_none());
        evidence.reset();
        assert!(journal.confirmed_candidate(&evidence, &chain, 11, 1).is_none());
    }

    #[test]
    fn restored_activity_groups_receipts_and_excludes_change() {
        let addr = "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN";
        let mut journal = WalletJournal::empty(addr).unwrap();
        journal.sent.push(ConfirmedSend { txid: [2; 32], block_hash: [8; 32], block_height: 12, amount: 9, fee: 1, tx_hex: None });
        let outputs = [
            RestoredActivityOutput { txid: [1; 32], height: 10, amount: 4 },
            RestoredActivityOutput { txid: [1; 32], height: 10, amount: 3 },
            RestoredActivityOutput { txid: [2; 32], height: 12, amount: 100 },
            RestoredActivityOutput { txid: [3; 32], height: 13, amount: 9 },
        ];
        assert_eq!(restored_activity(&outputs, &journal), Some(alloc::vec![(true, 7), (false, 9), (true, 9)]));
        let mut conflicting = outputs;
        conflicting[1].height = 11;
        assert!(restored_activity(&conflicting, &journal).is_none());
        let mut state = WalletState::closed();
        state.open = true;
        state.set_recent_activity(&[(false, 9), (false, 9)]);
        let mut buf = [0u8; 512];
        let n = state.format_status(&mut buf).unwrap();
        assert!(core::str::from_utf8(&buf[..n]).unwrap().contains("| HIST -9;-9"));
    }

    #[test]
    fn journal_plan_confirms_only_after_ten_verified_blocks() {
        let addr = "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN";
        let tx = "02f78dae0101ffbb8dae0101e0b2d2b9c21103e6854544fbb66d55fc3546f4d3e69f8234257b69fa2237712af3b058a5f01ba14a340173f263b8a4bbc46dfb6f29e0584adbfffdf7a47c929d77c2d0c142afea2b05300211000000f7eeeb3f0e00000000000000000000";
        let mut journal = WalletJournal::empty(addr).unwrap();
        journal.pending = Some(PendingTransaction { wallet_address: addr.into(), tx_hex: Zeroizing::new(tx.into()), fee: 5, amount: 42 });
        let txid = journal.pending.as_ref().unwrap().transaction_id().unwrap();
        let mut evidence = PendingChainEvidence::new(txid);
        let mut chain = RestoreChain::new(10);
        let mut previous = [0; 32];
        for height in 10u64..20 {
            let hash = [height as u8; 32];
            assert!(chain.commit(height, previous, hash));
            let ids = if height == 10 { alloc::vec![txid] } else { alloc::vec![] };
            assert!(evidence.record_block(&chain, height, hash, &ids).is_ok());
            previous = hash;
        }
        assert!(plan_journal_reconciliation(&journal, &[], Some(&evidence), &chain, 20, 10).is_none());
        assert!(chain.verify_tip(19, [19; 32], 20));
        assert!(evidence.finish(&chain, 20));
        let Some(JournalPlan::Confirm(candidate)) = plan_journal_reconciliation(&journal, &[], Some(&evidence), &chain, 20, 10) else { panic!("expected confirmation"); };
        assert!(journal.pending.is_some());
        assert!(candidate.pending.is_none());
        assert_eq!(candidate.sent.len(), 1);
        assert_eq!(candidate.sent[0].tx_hex.as_ref().unwrap().as_str(), tx);
        let restored = WalletJournal::decode(&candidate.encode().unwrap()).unwrap();
        assert_eq!(restored.orphaned_candidate(txid).unwrap().pending.unwrap().transaction_id(), Some(txid));
    }

    #[test]
    fn journal_plan_reopens_orphan_before_clearing_any_pending_payment() {
        let addr = "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN";
        let tx = "02f78dae0101ffbb8dae0101e0b2d2b9c21103e6854544fbb66d55fc3546f4d3e69f8234257b69fa2237712af3b058a5f01ba14a340173f263b8a4bbc46dfb6f29e0584adbfffdf7a47c929d77c2d0c142afea2b05300211000000f7eeeb3f0e00000000000000000000";
        let pending = PendingTransaction { wallet_address: addr.into(), tx_hex: Zeroizing::new(tx.into()), fee: 5, amount: 42 };
        let txid = pending.transaction_id().unwrap();
        let mut journal = WalletJournal::empty(addr).unwrap();
        journal.sent.push(ConfirmedSend { txid, block_hash: [1; 32], block_height: 10, amount: 42, fee: 5, tx_hex: Some(pending.tx_hex.clone()) });
        let mut chain = RestoreChain::new(10);
        assert!(chain.commit(10, [0; 32], [2; 32]));
        let orphan = [HistoryObservation::Block { hash: [2; 32], contains_transaction: false }];
        assert!(plan_journal_reconciliation(&journal, &orphan, None, &chain, 11, 10).is_none());
        assert!(chain.verify_tip(10, [2; 32], 11));
        let Some(JournalPlan::Reopen(candidate)) = plan_journal_reconciliation(&journal, &orphan, None, &chain, 11, 10) else { panic!("orphan must reopen"); };
        assert_eq!(candidate.pending.unwrap().transaction_id(), Some(txid));
        assert!(journal.pending.is_none());
        assert_eq!(journal.sent.len(), 1);
        journal.pending = Some(pending);
        assert!(matches!(plan_journal_reconciliation(&journal, &orphan, None, &chain, 11, 10), Some(JournalPlan::RecoveryRequired)));
        assert!(plan_journal_reconciliation(&journal, &[HistoryObservation::Unchecked], None, &chain, 11, 10).is_none());
        assert!(plan_journal_reconciliation(&journal, &orphan, None, &chain, 12, 10).is_none());
    }

    #[test]
    fn pending_chain_evidence_requires_a_complete_anchored_scan() {
        let txid = [7; 32];
        let mut evidence = PendingChainEvidence::new(txid);
        let mut chain = RestoreChain::new(10);
        assert!(chain.commit(10, [0; 32], [1; 32]));
        assert!(evidence.record_block(&chain, 10, [1; 32], &[[9; 32], txid]).is_ok());
        assert_eq!(evidence.location(&chain, 11), None);
        assert!(!evidence.finish(&chain, 12));
        assert!(!evidence.finish(&chain, 11));
        assert!(chain.verify_tip(10, [1; 32], 11));
        assert!(evidence.finish(&chain, 11));
        assert_eq!(evidence.location(&chain, 11), Some(10));
        evidence.invalidate();
        assert_eq!(evidence.location(&chain, 11), None);
        assert!(chain.commit(11, [1; 32], [2; 32]));
        assert!(evidence.record_block(&chain, 11, [2; 32], &[]).is_ok());
        assert!(chain.verify_tip(11, [2; 32], 12));
        assert!(evidence.finish(&chain, 12));
        assert_eq!(evidence.location(&chain, 12), Some(10));
        evidence.reset();
        chain.reset();
        assert_eq!(evidence.location(&chain, 10), None);
    }

    #[test]
    fn restore_chain_rejects_gaps_and_reorganizations() {
        let mut chain = RestoreChain::new(10);
        let first = [1; 32];
        let second = [2; 32];
        assert!(!chain.accepts(11, [0; 32]));
        assert!(chain.commit(10, [0; 32], first));
        assert!(!chain.accepts(11, [9; 32]));
        assert!(!chain.commit(12, first, second));
        assert_eq!(chain.next(), 11);
        assert!(chain.anchor_matches(10, first));
        assert!(!chain.anchor_matches(10, second));
        assert!(chain.commit(11, first, second));
        chain.reset();
        assert_eq!(chain.next(), 10);
        assert_eq!(chain.anchor(), None);
        let mut last = RestoreChain::new(u64::MAX);
        assert!(!last.commit(u64::MAX, [0; 32], first));
    }

    #[test]
    fn empty_restore_range_requires_rechecked_preceding_anchor() {
        let mut chain = RestoreChain::new(100);
        assert!(!chain.seed_preceding_anchor(98, [1; 32]));
        assert!(chain.seed_preceding_anchor(99, [1; 32]));
        assert!(!chain.is_verified());
        assert!(!chain.verify_tip(99, [2; 32], 100));
        assert!(chain.verify_tip(99, [1; 32], 100));
        assert!(chain.commit(100, [1; 32], [3; 32]));
        assert!(!chain.is_verified());
        chain.reset();
        assert_eq!(chain.anchor(), None);
        assert!(!RestoreChain::new(0).seed_preceding_anchor(u64::MAX, [0; 32]));
    }

    #[test]
    fn balances_require_spent_checks_and_maturity() {
        let mut outputs = [
            RestoredOutput { amount: 4, height: 90, coinbase: false, timelock: Timelock::None, spent_status: Some(0) },
            RestoredOutput { amount: 8, height: 50, coinbase: true, timelock: Timelock::None, spent_status: Some(0) },
            RestoredOutput { amount: 16, height: 0, coinbase: false, timelock: Timelock::Block(101), spent_status: Some(0) },
            RestoredOutput { amount: 32, height: 0, coinbase: false, timelock: Timelock::Time(1), spent_status: Some(0) },
            RestoredOutput { amount: 64, height: 0, coinbase: false, timelock: Timelock::None, spent_status: Some(1) },
            RestoredOutput { amount: 128, height: 0, coinbase: false, timelock: Timelock::None, spent_status: Some(2) },
        ];
        assert_eq!(restored_balances(&outputs, 100), Some((60, 4)));
        assert_eq!(restored_balances(&outputs, 110), Some((60, 28)));
        assert_eq!(restored_balances(&outputs, 90), None);
        outputs[0].spent_status = None;
        assert_eq!(restored_balances(&outputs, 110), None);
        outputs[0].spent_status = Some(3);
        assert_eq!(restored_balances(&outputs, 110), None);
    }

    #[test]
    fn input_selection_excludes_spent_and_locked_outputs_and_rejects_partial_scans() {
        let mut outputs = [
            RestoredOutput { amount: 7, height: 80, coinbase: false, timelock: Timelock::None, spent_status: Some(0) },
            RestoredOutput { amount: 5, height: 80, coinbase: false, timelock: Timelock::None, spent_status: Some(0) },
            RestoredOutput { amount: 100, height: 80, coinbase: false, timelock: Timelock::None, spent_status: Some(1) },
            RestoredOutput { amount: 200, height: 95, coinbase: false, timelock: Timelock::None, spent_status: Some(0) },
            RestoredOutput { amount: 300, height: 0, coinbase: false, timelock: Timelock::Time(1), spent_status: Some(0) },
        ];
        assert_eq!(select_restored_inputs(&outputs, 100, 6), Some(alloc::vec![0]));
        assert_eq!(select_restored_inputs(&outputs, 100, 12), Some(alloc::vec![0, 1]));
        assert_eq!(select_restored_inputs(&outputs, 100, 13), None);
        assert_eq!(select_restored_inputs(&outputs, 100, 0), None);
        outputs[2].spent_status = None;
        assert_eq!(select_restored_inputs(&outputs, 100, 1), None);
        outputs[2].spent_status = Some(3);
        assert_eq!(select_restored_inputs(&outputs, 100, 1), None);
    }

    #[test]
    fn ring_points_reject_identity_and_torsion() {
        use curve25519_dalek::{constants::{ED25519_BASEPOINT_POINT, EIGHT_TORSION}, traits::Identity};
        use monero_wallet::ed25519::Point;
        let base = Point::from(ED25519_BASEPOINT_POINT);
        let identity = Point::from(curve25519_dalek::EdwardsPoint::identity());
        let torsion = Point::from(EIGHT_TORSION[1]);
        assert!(valid_ring_points(base, base));
        assert!(!valid_ring_points(identity, base));
        assert!(!valid_ring_points(torsion, base));
        assert!(!valid_ring_points(base, torsion));
    }

    #[test]
    fn restored_signing_rejects_unverified_or_pending_wallet() {
        let mut state = WalletState::closed();
        state.open = true;
        let address = "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN";
        let fee = monero_wallet::interface::FeeRate::new(20_000, 10_000).unwrap();
        assert_eq!(prepare_restored_transaction(&mut state, &[], 100, &[], address, 1, false, fee, Zeroizing::new([0; 32])), Err(PrepareError::NeedScan));
        assert!(state.pending_tx_hex.is_empty());
        state.pending_tx_hex = "abcd".into();
        assert_eq!(prepare_restored_transaction(&mut state, &[], 100, &[], address, 1, false, fee, Zeroizing::new([0; 32])), Err(PrepareError::Pending));
        assert_eq!(state.pending_tx_hex, "abcd");
    }

    #[test]
    fn journal_reserves_space_before_signing() {
        let address = "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN";
        let mut journal = WalletJournal::empty(address).unwrap();
        assert!(journal.can_record_new_transaction());
        for i in 0..80u8 {
            journal.sent.push(ConfirmedSend { txid: [i; 32], block_hash: [2; 32], block_height: i.into(), amount: 1, fee: 1, tx_hex: None });
        }
        assert!(journal.encode().is_some());
        assert!(!journal.can_record_new_transaction());
    }

    #[test]
    fn restored_builder_signs_two_synthetic_inputs() {
        use curve25519_dalek::{constants::ED25519_BASEPOINT_POINT, Scalar as DalekScalar};
        use monero_wallet::ed25519::{Commitment, Scalar as MoneroScalar};
        let spend = DalekScalar::from(7u64);
        let view = DalekScalar::from(11u64);
        let mut state = WalletState::open_from_key_bytes("test", &spend.to_bytes(), &view.to_bytes(), 0).unwrap();
        let mut rings = alloc::vec::Vec::new();
        for (position, offset) in [13u64, 17].iter().enumerate() {
            let amount = 10_000_000_000u64;
            let index = 100 + position as u64;
            let key = (ED25519_BASEPOINT_POINT * (spend + DalekScalar::from(*offset))).compress().to_bytes();
            let mask = DalekScalar::from(19 + position as u64);
            let commitment = Commitment::new(MoneroScalar::from(mask), amount);
            let mut bytes = alloc::vec::Vec::new();
            bytes.extend_from_slice(&[position as u8; 32]);
            bytes.extend_from_slice(&0u64.to_le_bytes());
            bytes.extend_from_slice(&index.to_le_bytes());
            bytes.extend_from_slice(&key);
            bytes.extend_from_slice(&DalekScalar::from(*offset).to_bytes());
            bytes.extend_from_slice(&mask.to_bytes());
            bytes.extend_from_slice(&amount.to_le_bytes());
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            let output = WalletOutput::read(&mut bytes.as_slice()).unwrap();
            let mut ring = alloc::vec::Vec::new();
            for i in 0..15u64 {
                let point = (ED25519_BASEPOINT_POINT * DalekScalar::from(30 + i + 20 * position as u64)).compress().to_bytes();
                ring.push((1000 + i + 100 * position as u64, point, point));
            }
            ring.push((index, key, commitment.commit().compress().to_bytes()));
            state.outputs.push(output);
            rings.push((position, ring));
        }
        state.scanned = true;
        let meta = alloc::vec![RestoredOutput { amount: 10_000_000_000, height: 10, coinbase: false, timelock: Timelock::None, spent_status: Some(0) }; 2];
        let dest = String::from(state.address());
        let rate = monero_wallet::interface::FeeRate::new(20_000, 10_000).unwrap();
        let mut verified = RestoreChain::new(99);
        assert!(verified.commit(99, [0; 32], [1; 32]));
        assert!(verified.verify_tip(99, [1; 32], 100));
        state.scanned = false;
        assert_eq!(preview_verified_restored_transaction(&mut state, &meta, &verified, 100, &rings[..1], &dest, 10_000_000_000, false, rate, Zeroizing::new([7; 32])), Err(PrepareError::Insufficient));
        assert_eq!(select_restored_inputs(&meta, 100, 10_000_000_001), Some(alloc::vec![0, 1]));
        assert!(preview_verified_restored_transaction(&mut state, &meta, &verified, 100, &rings, &dest, 10_000_000_000, false, rate, Zeroizing::new([7; 32])).is_ok());
        assert!(preview_verified_restored_transaction(&mut state, &meta, &verified, 100, &rings, &dest, 15_000_000_000, false, rate, Zeroizing::new([7; 32])).is_ok());
        assert!(!state.scanned && state.pending_tx_hex.is_empty());
        let confirmed = create_payment_preview(&mut state, &meta, &verified, 100, &rings, &dest, 15_000_000_000, false, rate).unwrap();
        assert_eq!(confirmed.destination(), dest);
        let mut out_of_order = PaymentRevalidation::new(&state, &rings, &confirmed).unwrap();
        assert!(!out_of_order.observe_spent(&[0, 0]));
        assert!(!out_of_order.observe_height(true, 100));
        assert!(out_of_order.finish(rate).is_none());
        let mut wrong_network = PaymentRevalidation::new(&state, &rings, &confirmed).unwrap();
        assert!(!wrong_network.observe_info_response(br#"{"result":{"status":"OK","nettype":"stagenet","height":100}}"#, Network::Mainnet));
        assert!(wrong_network.finish(rate).is_none());
        let mut bad_anchor = PaymentRevalidation::new(&state, &rings, &confirmed).unwrap();
        assert!(bad_anchor.observe_info_response(br#"{"result":{"status":"OK","nettype":"mainnet","height":100}}"#, Network::Mainnet));
        assert!(!bad_anchor.observe_anchor_response(br#"{"result":{"status":"OK","blob":"00"}}"#));
        assert!(bad_anchor.finish(rate).is_none());
        let mut spent = PaymentRevalidation::new(&state, &rings, &confirmed).unwrap();
        assert!(spent.observe_height(true, 100));
        assert!(spent.observe_anchor(99, [1; 32]));
        assert!(!spent.observe_spent(&[0, 2]));
        assert!(spent.finish(rate).is_none());
        let mut checked = PaymentRevalidation::new(&state, &rings, &confirmed).unwrap();
        assert_eq!(checked.key_images().len(), 2);
        assert!(checked.observe_height(true, 100));
        assert!(checked.observe_anchor(99, [1; 32]));
        assert!(checked.observe_spent(&[0, 0]));
        assert!(checked.observe_anchor(99, [1; 32]));
        let proof = checked.finish(rate).unwrap();
        sign_revalidated_preview(&mut state, &meta, &verified, &rings, &confirmed, proof, Zeroizing::new([7; 32])).unwrap();
        assert!(!state.pending_tx_hex.is_empty());
        state.pending_tx_hex.clear();
        let mut changed_anchor = verified;
        changed_anchor.reset();
        assert!(changed_anchor.commit(99, [0; 32], [9; 32]));
        assert!(changed_anchor.verify_tip(99, [9; 32], 100));
        assert_eq!(sign_confirmed_preview(&mut state, &meta, &changed_anchor, &rings, &confirmed, rate, Zeroizing::new([7; 32])), Err(PrepareError::NeedScan));
        let different_rate = monero_wallet::interface::FeeRate::new(40_000, 10_000).unwrap();
        assert_eq!(sign_confirmed_preview(&mut state, &meta, &verified, &rings, &confirmed, different_rate, Zeroizing::new([7; 32])), Err(PrepareError::SignFail));
        assert!(state.pending_tx_hex.is_empty());
        sign_confirmed_preview(&mut state, &meta, &verified, &rings, &confirmed, rate, Zeroizing::new([7; 32])).unwrap();
        assert!(!state.pending_tx_hex.is_empty());
        state.pending_tx_hex.clear();
        state.scanned = true;
        let (preview_amount, preview_fee) = preview_restored_transaction(&mut state, &meta, 100, &rings, &dest, 15_000_000_000, false, rate, Zeroizing::new([7; 32])).unwrap();
        assert!(state.pending_tx_hex.is_empty());
        assert_eq!(preview_amount, 15_000_000_000);
        state.scanned = false;
        assert_eq!(sign_verified_restored_transaction(&mut state, &meta, &verified, 100, &rings, &dest, 15_000_000_000, false, rate, (preview_amount, preview_fee + 1), Zeroizing::new([7; 32])), Err(PrepareError::SignFail));
        assert!(state.pending_tx_hex.is_empty());
        let mut stale = verified;
        stale.invalidate();
        assert_eq!(sign_verified_restored_transaction(&mut state, &meta, &stale, 100, &rings, &dest, 15_000_000_000, false, rate, (preview_amount, preview_fee), Zeroizing::new([7; 32])), Err(PrepareError::NeedScan));
        sign_verified_restored_transaction(&mut state, &meta, &verified, 100, &rings, &dest, 15_000_000_000, false, rate, (preview_amount, preview_fee), Zeroizing::new([7; 32])).unwrap();
        assert!(!state.pending_tx_hex.is_empty());
        state.pending_tx_hex.clear();
        state.scanned = true;
        prepare_restored_transaction(&mut state, &meta, 100, &rings, &dest, 15_000_000_000, false, rate, Zeroizing::new([7; 32])).unwrap();
        assert!(!state.pending_tx_hex.is_empty());
        assert_eq!(state.pending_send_amount, 15_000_000_000);
        assert!(state.pending_fee_atomic > 0);
        assert_eq!(state.pending_fee_atomic, preview_fee);
        let mut saved = PendingTransaction { wallet_address: dest.clone(), tx_hex: Zeroizing::new(state.pending_tx_hex.clone()), fee: state.pending_fee_atomic, amount: state.pending_send_amount };
        assert!(saved.signed_payment_id().is_some());
        saved.fee += 1;
        assert!(saved.signed_payment_id().is_none());
        state.pending_tx_hex.clear();
        let (max_amount, max_fee) = preview_restored_transaction(&mut state, &meta, 100, &rings, &dest, 0, true, rate, Zeroizing::new([8; 32])).unwrap();
        assert!(state.pending_tx_hex.is_empty());
        prepare_restored_transaction(&mut state, &meta, 100, &rings, &dest, 0, true, rate, Zeroizing::new([8; 32])).unwrap();
        assert_eq!((state.pending_send_amount, state.pending_fee_atomic), (max_amount, max_fee));
        assert_eq!(state.pending_send_amount + state.pending_fee_atomic, 20_000_000_000);
    }

    #[test]
    fn wallet_status_keeps_unlocked_separate_from_total() {
        let mut state = WalletState::closed();
        state.open = true;
        state.balance_atomic = 100;
        state.unlocked_atomic = 40;
        let mut buf = [0u8; 512];
        let length = state.format_status(&mut buf).unwrap();
        let status = core::str::from_utf8(&buf[..length]).unwrap();
        assert!(status.contains("| BAL 100 | UNLOCKED 40"));
    }

    #[test]
    fn pending_relay_uses_same_bytes_without_a_fresh_scan() {
        let mut state = WalletState::closed();
        assert_eq!(send_prepared(&state), Err(SendError::Closed));
        state.open = true;
        assert_eq!(send_prepared(&state), Err(SendError::NeedPrepare));
        state.pending_tx_hex = "a1b2".into();
        assert_eq!(send_prepared(&state), Ok("a1b2"));
        assert_eq!(prepare_send(&state, "invalid", 1), Err(PrepareError::Pending));
        assert_eq!(prepare_and_sign(&mut state, "invalid", 1, &[], true, Zeroizing::new([0; 32])), Err(PrepareError::Pending));
        state.clear_scan_results();
        assert_eq!(send_prepared(&state), Ok("a1b2"));
        assert_eq!(send_prepared(&state), Ok("a1b2"));
    }

    #[test]
    fn scan_updates_do_not_discard_pending_transaction() {
        let mut state = WalletState::closed();
        state.pending_tx_hex = String::from("a1b2");
        state.pending_fee_atomic = 23;
        state.pending_send_amount = 456;
        state.apply_scan_outputs(&[]);
        state.clear_scan_results();
        state.mark_scanned_empty();
        state.note_activity(true, 456);
        state.clear_restore_history();
        assert_eq!(state.activity_len, 0);
        assert_eq!(state.pending_tx_hex, "a1b2");
        assert_eq!(state.pending_fee_atomic, 23);
        assert_eq!(state.pending_send_amount, 456);
    }

    #[test]
    fn mobile_wallet_vector_restores_address_and_height() {
        let words = "ability pockets lordship tomorrow gypsy match neutral uncle avatar             betting bicycle junk unzip pyramid lynx mammal edgy empty uneven knowledge juvenile wiring             paradise psychic betting";
        let state = WalletState::open_from_mnemonic(Zeroizing::new(String::from(words)), 12345)
            .expect("reference mnemonic must restore");
        assert_eq!(state.address(), "48tLyQXpcwt8w6uKHyb5Zs3vdnoDWAEKFQr1c198o7aX9dBzXP3BTSMVsDiuH3ozDCNqwojb4vNeQZf7xg6URimDLaNtGSN");
        assert_eq!(state.height, 12345);
        let testnet = WalletState::open_from_mnemonic_on_network(
            Zeroizing::new(String::from(words)), 12345, Network::Testnet,
        ).expect("reference mnemonic must restore on testnet");
        assert_ne!(state.address(), testnet.address());
        assert_eq!(testnet.network, Network::Testnet);
        let identity = WalletIdentity::from_mnemonic(
            Zeroizing::new(String::from(words)), 12345, Network::Testnet,
        ).expect("identity");
        let mut encoded = identity.encode();
        assert_eq!(WalletIdentity::decode(&encoded[..]).unwrap().open().unwrap().address(), testnet.address());
        encoded[40] ^= 1;
        assert!(WalletIdentity::decode(&encoded[..]).is_err());

        use rand_core::SeedableRng;
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([42; 32]);
        let (created, backup) = WalletIdentity::generate(&mut rng, 456, Network::Stagenet)
            .expect("generate wallet");
        let restored = WalletIdentity::from_mnemonic(backup, 456, Network::Stagenet)
            .expect("restore generated backup");
        assert_eq!(created.open().unwrap().address(), restored.open().unwrap().address());
        assert_eq!(restored.restore_height, 456);
        let (created, backup) = WalletIdentity::generate_from_entropy(
            Zeroizing::new([0x93; 64]), 789, Network::Testnet,
        ).expect("generate from platform entropy");
        let restored = WalletIdentity::from_mnemonic(backup, 789, Network::Testnet)
            .expect("restore platform-generated backup");
        assert_eq!(created.open().unwrap().address(), restored.open().unwrap().address());
        assert!(matches!(prepare_send(&testnet, state.address(), 1), Err(PrepareError::BadAddress)));
        assert!(matches!(prepare_send(&state, testnet.address(), 1), Err(PrepareError::BadAddress)));
        let bad = words.replace("psychic betting", "psychic pockets");
        assert!(WalletState::open_from_mnemonic(Zeroizing::new(bad), 0).is_err());
    }
}
