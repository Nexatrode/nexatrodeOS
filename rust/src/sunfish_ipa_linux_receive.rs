//! Offline IPA v4.2 exception receive plan, pinned Linux 2267e248204e7e051e0f7355298949a6440de579.
//!
//! This describes the complete initial Linux replenish (251 single-TRE
//! transactions), channel-2 START, suspend enable and default route. It
//! grants no pool/mapping authority, performs no I/O and proves no RX/DMA.
//! The separate host-chosen receive IOVA must have real retained backing
//! and verified SMMU mappings before any descriptor can be published.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_buffers::{self as buffers, Kind};
use crate::sunfish_ipa_linux_endpoint::Endpoint;

pub const CHANNEL: u8 = 2;
pub const EVENT: u8 = 1;
pub const ENDPOINT: u8 = 8;
pub const RING_COUNT: usize = 256;
pub const TLV_COUNT: usize = 6;
pub const BUFFER_COUNT: usize = RING_COUNT - (TLV_COUNT - 1);
pub const BUFFER_BYTES: usize = 8192;
/// Exact pinned reference-kernel ABI, independently compiled in the endpoint suite.
pub const HEADROOM: usize = 64;
pub const DMA_BYTES: usize = BUFFER_BYTES - HEADROOM;
pub const BATCH: usize = 16;
pub const DOORBELL_COUNT: usize = BUFFER_COUNT.div_ceil(BATCH);
/// Host-chosen, separate allocation; these are not firmware-defined addresses.
pub const DATA_IOVA: u64 = 0x80200000;
pub const DATA_RESERVED_BYTES: usize = 0x200000;
pub const DATA_USED_BYTES: usize = BUFFER_COUNT * BUFFER_BYTES;
pub const DATA_USED_PAGES: usize = DATA_USED_BYTES / buffers::PAGE_BYTES;
pub const GSI_IEOB_MASK: u32 = 3; // Existing command event 0 plus receive event 1.
pub const IPA_IRQ_MASK: u32 = 0x400c; // Retained UC bits 2/3 plus TX_SUSPEND bit 14.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Geometry,
    Bounds,
    Window,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Space {
    Gsi,
    Ipa,
}
/// Diagnostic word only. There is no native store or Request constructor here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Word {
    pub space: Space,
    pub offset: usize,
    pub value: u32,
}
const fn g(offset: usize, value: u32) -> Word {
    Word {
        space: Space::Gsi,
        offset,
        value,
    }
}
const fn i(offset: usize, value: u32) -> Word {
    Word {
        space: Space::Ipa,
        offset,
        value,
    }
}
/// Full original gsi_channel_start(), assuming retained command event 0 is
/// enabled and all raw status sources are quiet. A real owner must dispatch
/// channel-control completion between the START and source-clear words.
pub const START_WORDS: [Word; 8] = [
    g(0x1f0b8, GSI_IEOB_MASK),
    g(0x1f0a0, u32::MAX),
    g(0x1f098, 4),
    g(0x1f088, 0x4d),
    g(0x1f008, 0x01000002),
    g(0x1f0a0, 4),
    g(0x1f088, 0x4c),
    g(0x1f098, 0),
];
/// Original suspend-control RMW results for an empty endpoint suspend mask
/// and the retained two-UC-bit IPA enable mask. Preconditions need readback.
pub const SUSPEND_WORDS: [Word; 2] = [i(0x3034, 1 << ENDPOINT), i(0x300c, IPA_IRQ_MASK)];
pub const DEFAULT_ROUTE: Word = i(0x48, 0x01100050);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Buffer {
    index: usize,
    tre_iova: u64,
    tre_offset: usize,
}
impl Buffer {
    pub const fn index(self) -> usize {
        self.index
    }
    pub const fn allocation_iova(self) -> u64 {
        DATA_IOVA + (self.index * BUFFER_BYTES) as u64
    }
    pub const fn allocation_offset(self) -> usize {
        self.index * BUFFER_BYTES
    }
    pub const fn dma_iova(self) -> u64 {
        self.allocation_iova() + HEADROOM as u64
    }
    pub const fn dma_bytes(self) -> usize {
        DMA_BYTES
    }
    pub const fn tre_iova(self) -> u64 {
        self.tre_iova
    }
    pub const fn tre_offset(self) -> usize {
        self.tre_offset
    }
    pub fn tre(self) -> [u8; 16] {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&self.dma_iova().to_le_bytes());
        bytes[8..10].copy_from_slice(&(DMA_BYTES as u16).to_le_bytes());
        // Single XFER TRE, IEOT, no CHAIN and no outbound BEI suppression.
        bytes[12..].copy_from_slice(&0x00020200u32.to_le_bytes());
        bytes
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Doorbell {
    pub published_buffers: usize,
    pub word: Word,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Plan {
    tre_iova: u64,
    tre_offset: usize,
    event_iova: u64,
    event_offset: usize,
}
impl Plan {
    pub fn new() -> Result<Self, Error> {
        let p = buffers::Plan::new().map_err(|_| Error::Geometry)?;
        let ch = buffers::CHANNELS
            .iter()
            .find(|c| c.channel_id == CHANNEL)
            .ok_or(Error::Geometry)?;
        let e = Endpoint::LanRx;
        let tre = p.find(Kind::Tre(CHANNEL)).ok_or(Error::Geometry)?;
        let event = p.find(Kind::Event(CHANNEL)).ok_or(Error::Geometry)?;
        if ch.endpoint_id != ENDPOINT
            || ch.tre_count != RING_COUNT
            || ch.event_count != RING_COUNT
            || ch.tlv_count != TLV_COUNT
            || e.channel() != CHANNEL
            || e.event() != EVENT
            || e.id() != ENDPOINT
            || e.toward_ipa()
            || tre.requested() != RING_COUNT * 16
            || event.requested() != RING_COUNT * 16
            || DATA_IOVA < buffers::POOL_IOVA + buffers::POOL_BYTES as u64
            || DATA_USED_BYTES > DATA_RESERVED_BYTES
            || DMA_BYTES > u16::MAX as usize
        {
            return Err(Error::Geometry);
        }
        Ok(Self {
            tre_iova: tre.iova(),
            tre_offset: tre.offset(),
            event_iova: event.iova(),
            event_offset: event.offset(),
        })
    }
    pub const fn event_iova(self) -> u64 {
        self.event_iova
    }
    pub const fn event_offset(self) -> usize {
        self.event_offset
    }
    pub fn buffer(self, index: usize) -> Result<Buffer, Error> {
        if index >= BUFFER_COUNT {
            return Err(Error::Bounds);
        }
        Ok(Buffer {
            index,
            tre_iova: self.tre_iova + (index * 16) as u64,
            tre_offset: self.tre_offset + index * 16,
        })
    }
    pub fn doorbell(self, batch: usize) -> Result<Doorbell, Error> {
        if batch >= DOORBELL_COUNT {
            return Err(Error::Bounds);
        }
        let published_buffers = ((batch + 1) * BATCH).min(BUFFER_COUNT);
        Ok(Doorbell {
            published_buffers,
            word: g(
                0x1e010,
                (self.tre_iova + (published_buffers * 16) as u64) as u32,
            ),
        })
    }
    /// Validate the distinct candidate IOVA reservation, never a mapping proof.
    /// The old command pool cannot satisfy this full receive allocation.
    pub fn validate_window(self, iova: u64, bytes: usize) -> Result<(), Error> {
        if iova != DATA_IOVA
            || bytes != DATA_RESERVED_BYTES
            || iova % BUFFER_BYTES as u64 != 0
            || iova
                .checked_add(bytes as u64)
                .is_none_or(|end| end > 1u64 << 32)
        {
            return Err(Error::Window);
        }
        Ok(())
    }
}
