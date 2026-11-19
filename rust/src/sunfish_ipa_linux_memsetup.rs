//! Exact first IPA v4.2 memory transaction from the pinned Linux reference.
//!
//! This is an offline encoder, not a DMA authorization or a publication path.
//! It describes ipa_mem_setup()'s four immediate commands in the existing
//! private pool. A later native owner must check the retained domain, pool,
//! authenticated firmware, GSI/endpoint/START owners, IRQs and faults before
//! publishing anything, and retain all resources on an uncertain result.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_buffers::{self as buffers, Kind};
use crate::sunfish_ipa_linux_memory as memory;

pub const COUNT: usize = 4;
pub const PAYLOAD_BYTES: usize = 32;
pub const TRE_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Geometry,
    Layout,
    Bounds,
    EventPointer,
    EventChannel,
    TransferPointer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Command {
    opcode: u16,
    local_offset: usize,
    size: usize,
    payload_offset: usize,
    payload_iova: u64,
    tre_offset: usize,
    tre_iova: u64,
    payload: [u8; PAYLOAD_BYTES],
    tre: [u8; TRE_BYTES],
}
impl Command {
    pub const fn opcode(self) -> u16 {
        self.opcode
    }
    pub const fn local_offset(self) -> usize {
        self.local_offset
    }
    pub const fn size(self) -> usize {
        self.size
    }
    pub const fn payload_offset(self) -> usize {
        self.payload_offset
    }
    pub const fn payload_iova(self) -> u64 {
        self.payload_iova
    }
    pub const fn tre_offset(self) -> usize {
        self.tre_offset
    }
    pub const fn tre_iova(self) -> u64 {
        self.tre_iova
    }
    pub const fn payload(&self) -> &[u8; PAYLOAD_BYTES] {
        &self.payload
    }
    pub const fn tre(&self) -> &[u8; TRE_BYTES] {
        &self.tre
    }
}

/// Addresses and bytes are diagnostic planning data. This type neither
/// borrows a live pool nor supplies any register or memory write capability.
pub struct Plan {
    commands: [Command; COUNT],
    event_iova: u64,
    channel_doorbell: u32,
}
impl Plan {
    pub fn new(shared_mem_size: u32) -> Result<Self, Error> {
        let geometry = memory::geometry(shared_mem_size).map_err(|_| Error::Geometry)?;
        if geometry.offset != 0 || geometry.usable_bytes != memory::SHARED_BYTES {
            return Err(Error::Geometry);
        }
        let pool = buffers::Plan::new().map_err(|_| Error::Layout)?;
        let zero = pool.find(Kind::Zero).ok_or(Error::Layout)?;
        let payload = pool.find(Kind::CommandPayload).ok_or(Error::Layout)?;
        let tre = pool.find(Kind::Tre(1)).ok_or(Error::Layout)?;
        let event = pool.find(Kind::Event(1)).ok_or(Error::Layout)?;
        if zero.requested() != 0x2000
            || payload.requested() < COUNT * PAYLOAD_BYTES
            || tre.requested() != 4096
            || event.requested() != 4096
            || buffers::COMMAND_ELEMENT_BYTES != PAYLOAD_BYTES
            || buffers::RING_ELEMENT_BYTES != TRE_BYTES
        {
            return Err(Error::Layout);
        }
        let empty = Command {
            opcode: 0,
            local_offset: 0,
            size: 0,
            payload_offset: 0,
            payload_iova: 0,
            tre_offset: 0,
            tre_iova: 0,
            payload: [0; PAYLOAD_BYTES],
            tre: [0; TRE_BYTES],
        };
        let mut commands = [empty; COUNT];
        // No AP_HEADER exists in v4.2. The optional region must not be
        // invented or substituted for the adjacent canary/processing areas.
        for (i, name) in ["MODEM_HEADER", "MODEM_PROC_CTX", "AP_PROC_CTX", "MODEM"]
            .into_iter()
            .enumerate()
        {
            let region = memory::REGIONS
                .iter()
                .find(|r| r.name == name)
                .ok_or(Error::Layout)?;
            if region.size == 0
                || region.size > zero.requested()
                || region
                    .offset
                    .checked_add(region.size)
                    .ok_or(Error::Bounds)?
                    > geometry.usable_bytes
                || region.offset > u16::MAX as usize
                || region.size > u16::MAX as usize
            {
                return Err(Error::Bounds);
            }
            let c = &mut commands[i];
            c.opcode = if i == 0 { 0x09 } else { 0x13 };
            c.local_offset = region.offset;
            c.size = region.size;
            c.payload_offset = payload.offset() + i * PAYLOAD_BYTES;
            c.payload_iova = payload.iova() + (i * PAYLOAD_BYTES) as u64;
            c.tre_offset = tre.offset() + i * TRE_BYTES;
            c.tre_iova = tre.iova() + (i * TRE_BYTES) as u64;
            if i == 0 {
                if region.size > 0xfff {
                    return Err(Error::Bounds);
                }
                c.payload[..8].copy_from_slice(&zero.iova().to_le_bytes());
                let flags = (region.offset as u32) << 12 | region.size as u32;
                c.payload[8..12].copy_from_slice(&flags.to_le_bytes());
            } else {
                // Direction 0: write to IPA. No clear-after-read or
                // pipeline-skip options; all reserved bytes remain zero.
                c.payload[2..4].copy_from_slice(&(region.size as u16).to_le_bytes());
                c.payload[4..6].copy_from_slice(&(region.offset as u16).to_le_bytes());
                c.payload[8..16].copy_from_slice(&zero.iova().to_le_bytes());
            }
            c.tre[..8].copy_from_slice(&c.payload_iova.to_le_bytes());
            c.tre[8..10].copy_from_slice(&c.opcode.to_le_bytes());
            // Immediate command (3); all but the last CHAIN. The last
            // requests IEOT and uses Linux's toward-IPA BEI setting.
            let flags = 0x00030000u32 | if i + 1 == COUNT { 0x600 } else { 1 };
            c.tre[12..16].copy_from_slice(&flags.to_le_bytes());
        }
        let channel_doorbell =
            u32::try_from(tre.iova() + (COUNT * TRE_BYTES) as u64).map_err(|_| Error::Bounds)?;
        Ok(Self {
            commands,
            event_iova: event.iova(),
            channel_doorbell,
        })
    }
    pub const fn commands(&self) -> &[Command; COUNT] {
        &self.commands
    }
    /// Linux rings with the first unfilled TRE, not the last filled TRE.
    pub const fn channel_doorbell(&self) -> (usize, u32) {
        (0x1e008, self.channel_doorbell)
    }
    /// After one transaction event, publish the last consumed event entry.
    pub const fn event_doorbell(&self) -> (usize, u32) {
        (0x1e100, self.event_iova as u32)
    }
    /// This direct IPA register write belongs after transaction completion.
    pub const fn processing_context(&self) -> (usize, u32, u32) {
        (0x1e8, 0x5f0, 0x1ffff)
    }
    pub const fn first_unfilled_event(&self) -> u32 {
        (self.event_iova + TRE_BYTES as u64) as u32
    }
    pub const fn event_iova(&self) -> u64 {
        self.event_iova
    }

    /// Check only the single event associated with this first transaction.
    /// Linux's TX completion path uses chid/xfer_ptr and ignores len/code/type;
    /// keep those latter fields as raw diagnostics instead of inventing an ABI.
    /// This parser alone does not prove IRQ delivery, ordering, DMA or readiness.
    pub fn completion(&self, first_unfilled: u32, bytes: &[u8; TRE_BYTES]) -> Result<Event, Error> {
        if first_unfilled != self.first_unfilled_event() {
            return Err(Error::EventPointer);
        }
        let event = Event {
            transfer: u64::from_le_bytes(bytes[..8].try_into().map_err(|_| Error::Bounds)?),
            length: u16::from_le_bytes([bytes[8], bytes[9]]),
            code: bytes[11],
            event_type: bytes[14],
            channel: bytes[15],
        };
        if event.channel != 1 {
            return Err(Error::EventChannel);
        }
        if event.transfer != self.commands[COUNT - 1].tre_iova {
            return Err(Error::TransferPointer);
        }
        Ok(event)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Event {
    pub transfer: u64,
    pub length: u16,
    pub code: u8,
    pub event_type: u8,
    pub channel: u8,
}
