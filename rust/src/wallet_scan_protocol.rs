//! Wallet ↔ scan_worker PD wire (shared page + Call opcodes).
#![allow(dead_code)]

use sel4_microkit::{Channel, MessageLabel};

/// Wallet-local channel to the scan_worker PD.
pub const WALLET_TO_WORKER_CHANNEL: Channel = Channel::new(3);
/// Worker-local channel from wallet.
pub const WORKER_FROM_WALLET_CHANNEL: Channel = Channel::new(0);

pub const REQUEST_WORKER_PING: MessageLabel = 0x601;
/// Arm an async tip-block fetch (worker advances in notified()).
pub const REQUEST_WORKER_ARM_BLOCK: MessageLabel = 0x602;

pub const REPLY_WORKER_OK: MessageLabel = 0x600;
pub const REPLY_WORKER_BAD: MessageLabel = 0x603;

pub const SCAN_JOB_MAGIC: u32 = u32::from_le_bytes(*b"NXSC");

#[repr(C)]
pub struct ScanJobPage {
    pub magic: u32,
    pub height: u64,
    pub status: u32,
    /// For DONE: number of peer tx hashes in the fetched block.
    pub outs: u32,
}

impl ScanJobPage {
    pub const STATUS_IDLE: u32 = 0;
    pub const STATUS_DONE: u32 = 1;
    pub const STATUS_FAIL: u32 = 2;
    pub const STATUS_ARMED: u32 = 3;

    pub fn clear(&mut self) {
        self.magic = SCAN_JOB_MAGIC;
        self.height = 0;
        self.status = Self::STATUS_IDLE;
        self.outs = 0;
    }
}
