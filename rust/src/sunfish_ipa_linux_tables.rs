//! Offline encoding of IPA v4.2's table transaction after memory setup.
//!
//! Pinned Linux ipa_table_setup() reserves eight TREs but uses six on v4.2:
//! IPv4/IPv6 route initialization, then each filter init plus unused-tail
//! zeroing. Hashed tables are absent. This planning/parser type supplies no
//! pool ownership, publication path, completion proof or production readiness.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_buffers::{self as buffers, Kind};
use crate::sunfish_ipa_linux_memory as memory;
use crate::sunfish_ipa_linux_memsetup::Event;

pub const COUNT: usize = 6;
pub const FIRST_TRE: usize = 4;
pub const FIRST_PAYLOAD: usize = 4;
pub const EVENT_INDEX: usize = 1;
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
    source_iova: u64,
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
    pub const fn source_iova(self) -> u64 {
        self.source_iova
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
pub struct Plan {
    commands: [Command; COUNT],
    table_iova: u64,
    table_offset: usize,
    table_bytes: [u8; buffers::TABLE_BYTES],
    event_iova: u64,
    tre_iova: u64,
}
impl Plan {
    pub fn new(shared_mem_size: u32) -> Result<Self, Error> {
        let geometry = memory::geometry(shared_mem_size).map_err(|_| Error::Geometry)?;
        if geometry.offset != 0 || geometry.usable_bytes != memory::SHARED_BYTES {
            return Err(Error::Geometry);
        }
        let pool = buffers::Plan::new().map_err(|_| Error::Layout)?;
        let table = pool.find(Kind::Table).ok_or(Error::Layout)?;
        let zero = pool.find(Kind::Zero).ok_or(Error::Layout)?;
        let payload = pool.find(Kind::CommandPayload).ok_or(Error::Layout)?;
        let tre = pool.find(Kind::Tre(1)).ok_or(Error::Layout)?;
        let event = pool.find(Kind::Event(1)).ok_or(Error::Layout)?;
        if table.requested() != buffers::TABLE_BYTES
            || table.iova() & 127 != 0
            || (FIRST_PAYLOAD + COUNT) * PAYLOAD_BYTES > payload.requested()
            || (FIRST_TRE + COUNT) * TRE_BYTES > tre.requested()
            || (EVENT_INDEX + 1) * TRE_BYTES >= event.requested()
            || buffers::FILTER_BITMAP != 0x24
            || buffers::TABLE_ENTRIES != 15
        {
            return Err(Error::Layout);
        }
        let mut table_bytes = [0; buffers::TABLE_BYTES];
        table_bytes[8..16].copy_from_slice(&buffers::FILTER_BITMAP.to_le_bytes());
        for slot in 2..2 + buffers::TABLE_ENTRIES {
            table_bytes[slot * 8..slot * 8 + 8].copy_from_slice(&table.iova().to_le_bytes());
        }
        let empty = Command {
            opcode: 0,
            local_offset: 0,
            size: 0,
            source_iova: 0,
            payload_offset: 0,
            payload_iova: 0,
            tre_offset: 0,
            tre_iova: 0,
            payload: [0; PAYLOAD_BYTES],
            tre: [0; TRE_BYTES],
        };
        let mut commands = [empty; COUNT];
        let mut index = 0;
        for (name, opcode, filter) in [
            ("V4_ROUTE", 7, false),
            ("V6_ROUTE", 8, false),
            ("V4_FILTER", 3, true),
            ("V6_FILTER", 4, true),
        ] {
            let region = memory::REGIONS
                .iter()
                .find(|r| r.name == name)
                .ok_or(Error::Layout)?;
            if region.size != 0x78 || region.offset + region.size > geometry.usable_bytes {
                return Err(Error::Bounds);
            }
            let size = if filter {
                8 * (1 + buffers::FILTER_BITMAP.count_ones() as usize)
            } else {
                region.size
            };
            if size > region.size || size > 0xfff || region.offset > 0xffff {
                return Err(Error::Bounds);
            }
            let c = &mut commands[index];
            c.opcode = opcode;
            c.local_offset = region.offset;
            c.size = size;
            // Skip the zero rule; routes also skip the filter bitmap.
            c.source_iova = table.iova() + if filter { 8 } else { 16 };
            let flags = (region.offset as u64) << 40 | (size as u64) << 28;
            c.payload[8..16].copy_from_slice(&flags.to_le_bytes());
            c.payload[16..24].copy_from_slice(&c.source_iova.to_le_bytes());
            index += 1;
            if filter {
                let c = &mut commands[index];
                c.opcode = 0x13;
                c.local_offset = region.offset + size;
                c.size = region.size - size;
                c.source_iova = zero.iova();
                if c.size == 0 || c.size > zero.requested() || c.local_offset > 0xffff {
                    return Err(Error::Bounds);
                }
                c.payload[2..4].copy_from_slice(&(c.size as u16).to_le_bytes());
                c.payload[4..6].copy_from_slice(&(c.local_offset as u16).to_le_bytes());
                c.payload[8..16].copy_from_slice(&zero.iova().to_le_bytes());
                index += 1;
            }
        }
        if index != COUNT {
            return Err(Error::Layout);
        }
        for (i, c) in commands.iter_mut().enumerate() {
            c.payload_offset = payload.offset() + (FIRST_PAYLOAD + i) * PAYLOAD_BYTES;
            c.payload_iova = payload.iova() + ((FIRST_PAYLOAD + i) * PAYLOAD_BYTES) as u64;
            c.tre_offset = tre.offset() + (FIRST_TRE + i) * TRE_BYTES;
            c.tre_iova = tre.iova() + ((FIRST_TRE + i) * TRE_BYTES) as u64;
            c.tre[..8].copy_from_slice(&c.payload_iova.to_le_bytes());
            c.tre[8..10].copy_from_slice(&c.opcode.to_le_bytes());
            let flags = 0x00030000u32 | if i + 1 == COUNT { 0x600 } else { 1 };
            c.tre[12..16].copy_from_slice(&flags.to_le_bytes());
        }
        Ok(Self {
            commands,
            table_iova: table.iova(),
            table_offset: table.offset(),
            table_bytes,
            event_iova: event.iova(),
            tre_iova: tre.iova(),
        })
    }
    pub const fn commands(&self) -> &[Command; COUNT] {
        &self.commands
    }
    pub const fn table_bytes(&self) -> &[u8; buffers::TABLE_BYTES] {
        &self.table_bytes
    }
    pub const fn table_iova(&self) -> u64 {
        self.table_iova
    }
    pub const fn table_offset(&self) -> usize {
        self.table_offset
    }
    pub const fn event_offset(&self) -> usize {
        EVENT_INDEX * TRE_BYTES
    }
    pub const fn initial_event_producer(&self) -> u32 {
        (self.event_iova + (EVENT_INDEX * TRE_BYTES) as u64) as u32
    }
    pub const fn first_unfilled_event(&self) -> u32 {
        (self.event_iova + ((EVENT_INDEX + 1) * TRE_BYTES) as u64) as u32
    }
    pub const fn channel_doorbell(&self) -> (usize, u32) {
        (
            0x1e008,
            (self.tre_iova + ((FIRST_TRE + COUNT) * TRE_BYTES) as u64) as u32,
        )
    }
    pub const fn event_doorbell(&self) -> (usize, u32) {
        (0x1e100, self.initial_event_producer())
    }
    /// Expected final target word, including filter bitmap and unused tail.
    /// None excludes UC memory, hashed pseudo-regions, canaries and headers.
    pub fn expected32(&self, offset: usize) -> Option<u32> {
        if offset % 4 != 0 {
            return None;
        }
        for c in self.commands {
            if !(c.local_offset..c.local_offset + c.size).contains(&offset) {
                continue;
            }
            if c.opcode == 0x13 {
                return Some(0);
            }
            let source = usize::try_from(c.source_iova.checked_sub(self.table_iova)?).ok()?;
            let start = source + offset - c.local_offset;
            return Some(u32::from_le_bytes(
                self.table_bytes.get(start..start + 4)?.try_into().ok()?,
            ));
        }
        None
    }
    pub fn completion(&self, producer: u32, bytes: &[u8; 16]) -> Result<Event, Error> {
        if producer != self.first_unfilled_event() {
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
