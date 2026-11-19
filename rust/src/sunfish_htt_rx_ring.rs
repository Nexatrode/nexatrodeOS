//! Source-matched HTT low-latency receive-ring setup for Sunfish.
//!
//! This plans and encodes the host-to-target HTT ring configuration and can
//! initialize a caller-owned DMA ring image. It does not allocate or map DMA
//! memory, establish page ownership, send the message, or start WLAN.
#![forbid(unsafe_code)]

use crate::sunfish_htt_rx_profile::{HttRxProfile, SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use crate::sunfish_qcacld_rx_session::{Error as RxSessionError, SunfishQcacldRxSession};
use crate::sunfish_smmu_lpae::{LpaePageMapping, SMMU_LPAE_PAGE_BYTES};

pub const SUNFISH_HTT_RX_RING_CFG_BYTES: usize = 48;
pub const SUNFISH_HTT_RX_RING_CFG_WORDS: usize = SUNFISH_HTT_RX_RING_CFG_BYTES / 4;
pub const SUNFISH_HTT_RX_RING_ENTRY_BYTES: u64 = 8;
pub const SUNFISH_HTT_RX_RING_INDEX_BYTES: u64 = 4;
pub const SUNFISH_HTT_RX_RING_BASE_ADDRESS_BITS: u32 = 37;
pub const SUNFISH_HTT_RX_IOVA_BITS: u32 = 39;

const HTT_H2T_MSG_TYPE_RX_RING_CFG: u32 = 0x2;
const HTT_RX_RING_CFG_NUM_RINGS: u32 = 1;
const RX_RING_CFG_HEADER_WORD: usize = 0;
const RX_RING_CFG_ALLOC_IDX_LO_WORD: usize = 1;
const RX_RING_CFG_ALLOC_IDX_HI_WORD: usize = 2;
const RX_RING_CFG_RING_BASE_LO_WORD: usize = 3;
const RX_RING_CFG_RING_BASE_HI_WORD: usize = 4;
const RX_RING_CFG_LEN_AND_BUFFER_SIZE_WORD: usize = 5;
const RX_RING_CFG_FLAGS_AND_INITIAL_INDEX_WORD: usize = 6;
const RX_RING_CFG_80211_AND_PAYLOAD_OFFSET_WORD: usize = 7;
const RX_RING_CFG_PPDU_OFFSETS_WORD: usize = 8;
const RX_RING_CFG_MPDU_OFFSETS_WORD: usize = 9;
const RX_RING_CFG_MSDU_OFFSETS_WORD: usize = 10;
const RX_RING_CFG_ATTN_AND_FRAG_OFFSETS_WORD: usize = 11;

const FLAG_80211_HEADER: u32 = 1 << 0;
const FLAG_MSDU_PAYLOAD: u32 = 1 << 1;
const FLAG_PPDU_START: u32 = 1 << 2;
const FLAG_PPDU_END: u32 = 1 << 3;
const FLAG_MPDU_START: u32 = 1 << 4;
const FLAG_MPDU_END: u32 = 1 << 5;
const FLAG_MSDU_START: u32 = 1 << 6;
const FLAG_MSDU_END: u32 = 1 << 7;
const FLAG_RX_ATTN: u32 = 1 << 8;
const FLAG_FRAG_INFO: u32 = 1 << 9;
const FLAG_UCAST: u32 = 1 << 10;
const FLAG_MCAST: u32 = 1 << 11;
const FLAG_CTRL: u32 = 1 << 12;
const FLAG_MGMT: u32 = 1 << 13;
const FLAG_NULL: u32 = 1 << 14;
const FLAG_PHY: u32 = 1 << 15;

// Offsets are offsetof(struct htt_host_rx_desc_base, field) / 4 from the
// matched AArch64 QCACLD build. The wire schema expresses these in words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishHttRxDescriptorOffsets {
    pub hdr_status_words: u16,
    pub ppdu_start_words: u16,
    pub ppdu_end_words: u16,
    pub mpdu_start_words: u16,
    pub mpdu_end_words: u16,
    pub msdu_start_words: u16,
    pub msdu_end_words: u16,
    pub attention_words: u16,
    pub fragment_info_words: u16,
    pub msdu_payload_words: u16,
}

pub const SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS: SunfishHttRxDescriptorOffsets =
    SunfishHttRxDescriptorOffsets {
        hdr_status_words: 74,    // byte offset 296
        ppdu_start_words: 27,    // byte offset 108
        ppdu_end_words: 37,      // byte offset 148
        mpdu_start_words: 4,     // byte offset 16
        mpdu_end_words: 26,      // byte offset 104
        msdu_start_words: 7,     // byte offset 28
        msdu_end_words: 12,      // byte offset 48
        attention_words: 1,      // byte offset 4
        fragment_info_words: 2,  // byte offset 8
        msdu_payload_words: 138, // max(32, 552) / 4
    };

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishHttRxRingMemory {
    pub ring_iova: u64,
    pub alloc_index_iova: u64,
    pub initial_buffers_iova: u64,
    pub fallback_buffers_iova: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishHttRxRingOptions {
    /// Mirrors `ol_cfg_is_packet_log_enabled()` in the QCACLD ring setup.
    pub packet_log_enabled: bool,
    /// Mirrors `QDF_GLOBAL_MONITOR_MODE` in the QCACLD ring setup.
    pub global_monitor_mode: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishHttRxRingError {
    InvalidProfile,
    MisalignedRing,
    MisalignedAllocIndex,
    MisalignedBufferPool,
    AddressOverflow,
    AddressOutOfRange,
    IovaRootMismatch,
    MemoryOverlap,
    MappingsNotSorted,
    UnmappedDmaPage(u64),
    InvalidBufferIndex,
    OutputTooSmall,
    InvalidSessionCapacity,
    RefillPlanChanged,
    UnexpectedAllocIndex { expected: u32, observed: u32 },
    SessionIndexMismatch { expected: u32, observed: u32 },
    RefillPoisoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SunfishHttRxRefillError<E> {
    Ring(SunfishHttRxRingError),
    Session(RxSessionError),
    Io(E),
    /// Hardware publication succeeded but the previously validated software
    /// ownership commit failed; a full ring reinitialization is required.
    SessionCommitUncertain(RxSessionError),
    /// The allocation-index write may have reached the device. Further
    /// refills are forbidden until the ring is fully reinitialized.
    AllocIndexWriteUncertain(E),
}

/// Operations required to publish a batch of recycled buffers. Implementations must
/// keep these operations ordered and exclusive with other producers. An error
/// from `write_ring_entry` must not publish the allocation index; an error
/// from `write_alloc_index` is treated as ambiguous because the device may
/// already have observed it.
pub trait SunfishHttRxRingIo {
    type Error;

    fn read_alloc_index(&mut self, alloc_index_iova: u64) -> Result<u32, Self::Error>;
    fn sync_buffer_for_device(
        &mut self,
        buffer_iova: u64,
        buffer_bytes: usize,
    ) -> Result<(), Self::Error>;
    fn write_ring_entry(
        &mut self,
        ring_entry_iova: u64,
        buffer_iova: u64,
    ) -> Result<(), Self::Error>;
    fn dma_write_barrier(&mut self);
    fn write_alloc_index(
        &mut self,
        alloc_index_iova: u64,
        next_index: u32,
    ) -> Result<(), Self::Error>;
}

/// Exclusive single-producer state for live HTT RX ring refill. The session
/// remains unchanged until the allocation-index publication succeeds. This
/// still does not establish DMA authority, SMMU mappings, or an IRQ path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishHttRxRingRefillState {
    plan: SunfishHttRxRingPlan,
    next_alloc_index: u32,
    poisoned: bool,
}

impl SunfishHttRxRingRefillState {
    pub const fn new(plan: SunfishHttRxRingPlan) -> Self {
        Self {
            plan,
            next_alloc_index: plan.initial_alloc_index(),
            poisoned: false,
        }
    }

    pub const fn next_alloc_index(self) -> u32 {
        self.next_alloc_index
    }

    pub const fn is_poisoned(self) -> bool {
        self.poisoned
    }

    /// Publish one released RX buffer. The validated pool binds the identity
    /// to its backing storage; the IO implementation performs platform cache
    /// and DMA synchronization. Refills must be serialized by this state's
    /// exclusive owner.
    pub fn refill_one<const N: usize, IO: SunfishHttRxRingIo>(
        &mut self,
        plan: SunfishHttRxRingPlan,
        session: &mut SunfishQcacldRxSession<N>,
        buffer_id: u32,
        buffers: &mut SunfishHttRxBufferPool<'_>,
        io: &mut IO,
    ) -> Result<u32, SunfishHttRxRefillError<IO::Error>> {
        let buffer_ids = [buffer_id];
        self.refill_batch(plan, session, &buffer_ids, buffers, io)
    }

    /// Publish a batch using QCACLD's fill-N ordering: prepare and write each
    /// ring entry, issue one DMA write barrier, then publish the final
    /// allocation index once. All software ownership changes are committed
    /// only after that publication succeeds.
    pub fn refill_batch<const N: usize, IO: SunfishHttRxRingIo>(
        &mut self,
        plan: SunfishHttRxRingPlan,
        session: &mut SunfishQcacldRxSession<N>,
        buffer_ids: &[u32],
        buffers: &mut SunfishHttRxBufferPool<'_>,
        io: &mut IO,
    ) -> Result<u32, SunfishHttRxRefillError<IO::Error>> {
        if self.poisoned {
            return Err(SunfishHttRxRefillError::Ring(
                SunfishHttRxRingError::RefillPoisoned,
            ));
        }
        if self.plan != plan {
            return Err(SunfishHttRxRefillError::Ring(
                SunfishHttRxRingError::RefillPlanChanged,
            ));
        }
        if buffers.plan() != plan {
            return Err(SunfishHttRxRefillError::Ring(
                SunfishHttRxRingError::RefillPlanChanged,
            ));
        }
        let ring_slots = usize::try_from(plan.ring_slots()).map_err(|_| {
            SunfishHttRxRefillError::Ring(SunfishHttRxRingError::InvalidSessionCapacity)
        })?;
        if N != ring_slots {
            return Err(SunfishHttRxRefillError::Ring(
                SunfishHttRxRingError::InvalidSessionCapacity,
            ));
        }
        if buffer_ids.is_empty() {
            return Ok(self.next_alloc_index);
        }

        // Preflight every software slot, ID, and pool mapping before touching
        // the first descriptor or performing any publication I/O.
        let first_slot = session
            .preview_post_batch(buffer_ids)
            .map_err(SunfishHttRxRefillError::Session)?;
        let first_slot = u32::from(first_slot);
        if first_slot != self.next_alloc_index {
            return Err(SunfishHttRxRefillError::Ring(
                SunfishHttRxRingError::SessionIndexMismatch {
                    expected: self.next_alloc_index,
                    observed: first_slot,
                },
            ));
        }
        for buffer_id in buffer_ids {
            buffer_iova(plan, *buffer_id).map_err(SunfishHttRxRefillError::Ring)?;
            buffers
                .buffer(*buffer_id)
                .map_err(SunfishHttRxRefillError::Ring)?;
        }

        let memory = plan.memory();
        let observed_index = io
            .read_alloc_index(memory.alloc_index_iova)
            .map_err(SunfishHttRxRefillError::Io)?;
        if observed_index != self.next_alloc_index {
            return Err(SunfishHttRxRefillError::Ring(
                SunfishHttRxRingError::UnexpectedAllocIndex {
                    expected: self.next_alloc_index,
                    observed: observed_index,
                },
            ));
        }

        let ring_mask = plan.ring_slots() - 1;
        for (offset, &buffer_id) in buffer_ids.iter().enumerate() {
            let slot = (first_slot + offset as u32) & ring_mask;
            let buffer_iova =
                buffer_iova(plan, buffer_id).map_err(SunfishHttRxRefillError::Ring)?;
            let ring_entry_iova = memory
                .ring_iova
                .checked_add(
                    u64::from(slot)
                        .checked_mul(SUNFISH_HTT_RX_RING_ENTRY_BYTES)
                        .ok_or(SunfishHttRxRefillError::Ring(
                            SunfishHttRxRingError::AddressOverflow,
                        ))?,
                )
                .ok_or(SunfishHttRxRefillError::Ring(
                    SunfishHttRxRingError::AddressOverflow,
                ))?;
            plan.prepare_buffer_for_device(
                buffer_id,
                buffers
                    .buffer_mut(buffer_id)
                    .map_err(SunfishHttRxRefillError::Ring)?,
            )
            .map_err(SunfishHttRxRefillError::Ring)?;
            io.sync_buffer_for_device(buffer_iova, plan.buffer_bytes() as usize)
                .map_err(SunfishHttRxRefillError::Io)?;
            io.write_ring_entry(ring_entry_iova, buffer_iova)
                .map_err(SunfishHttRxRefillError::Io)?;
        }

        let next_index = first_slot.wrapping_add(buffer_ids.len() as u32) & ring_mask;
        io.dma_write_barrier();
        if let Err(error) = io.write_alloc_index(memory.alloc_index_iova, next_index) {
            self.poisoned = true;
            return Err(SunfishHttRxRefillError::AllocIndexWriteUncertain(error));
        }

        if let Err(error) = session.post_batch(buffer_ids) {
            self.poisoned = true;
            return Err(SunfishHttRxRefillError::SessionCommitUncertain(error));
        }
        self.next_alloc_index = next_index;
        Ok(first_slot)
    }
}

fn buffer_iova(plan: SunfishHttRxRingPlan, buffer_id: u32) -> Result<u64, SunfishHttRxRingError> {
    plan.buffer_iova(buffer_id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishHttRxRingPlan {
    profile: HttRxProfile,
    memory: SunfishHttRxRingMemory,
    capture_extra_metadata: bool,
}

impl SunfishHttRxRingPlan {
    pub const fn ring_slots(self) -> u32 {
        self.profile.ring_slots()
    }

    pub const fn initial_fill_level(self) -> u32 {
        self.profile.initial_fill_level()
    }

    pub const fn buffer_bytes(self) -> u32 {
        self.profile.rx_buffer_bytes()
    }

    pub const fn memory(self) -> SunfishHttRxRingMemory {
        self.memory
    }

    pub const fn fallback_buffer_bytes(self) -> u64 {
        self.profile.fallback_buffer_capacity_bytes()
    }

    pub const fn fallback_buffer_count(self) -> u32 {
        (self.profile.fallback_buffer_capacity_bytes() / self.profile.rx_buffer_bytes() as u64)
            as u32
    }

    pub const fn initial_alloc_index(self) -> u32 {
        self.profile.initial_fill_level()
    }

    pub const fn initial_buffer_bytes(self) -> Option<u64> {
        (self.profile.rx_buffer_bytes() as u64)
            .checked_mul(self.profile.initial_fill_level() as u64)
    }

    /// Clear the source-required RX attention word in one buffer before it is
    /// returned to the target. The caller must still provide the appropriate
    /// DMA synchronization and publication ordering.
    pub fn prepare_buffer_for_device(
        self,
        buffer_id: u32,
        buffer: &mut [u8],
    ) -> Result<(), SunfishHttRxRingError> {
        self.buffer_iova(buffer_id)?;
        if buffer.len() < self.buffer_bytes() as usize {
            return Err(SunfishHttRxRingError::OutputTooSmall);
        }
        let attention_start = usize::from(SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS.attention_words)
            .checked_mul(core::mem::size_of::<u32>())
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        let attention_end = attention_start
            .checked_add(core::mem::size_of::<u32>())
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        if attention_end > self.buffer_bytes() as usize {
            return Err(SunfishHttRxRingError::InvalidProfile);
        }
        buffer[attention_start..attention_end].fill(0);
        Ok(())
    }

    /// Prepare the complete initial fill before its addresses are published.
    /// Buffer count and every buffer length are checked before any attention
    /// word is cleared; extra buffers are left untouched.
    pub fn prepare_initial_buffers(
        self,
        buffers: &mut [&mut [u8]],
    ) -> Result<usize, SunfishHttRxRingError> {
        let needed = self.profile.initial_fill_level() as usize;
        if buffers.len() < needed {
            return Err(SunfishHttRxRingError::OutputTooSmall);
        }
        if buffers
            .iter()
            .take(needed)
            .any(|buffer| buffer.len() < self.buffer_bytes() as usize)
        {
            return Err(SunfishHttRxRingError::OutputTooSmall);
        }
        for (index, buffer) in buffers.iter_mut().take(needed).enumerate() {
            self.prepare_buffer_for_device(index as u32, buffer)?;
        }
        Ok(needed)
    }

    /// Confirm that the sorted LPAE input page map covers the ring, index,
    /// startup buffers, and fallback reserve. This checks virtual coverage;
    /// it does not prove physical ownership, cache policy, or device authority.
    pub fn validate_mappings(
        self,
        mappings: &[LpaePageMapping],
    ) -> Result<(), SunfishHttRxRingError> {
        if mappings
            .windows(2)
            .any(|pair| pair[0].iova_page >= pair[1].iova_page)
        {
            return Err(SunfishHttRxRingError::MappingsNotSorted);
        }
        let ring_bytes = ring_storage_bytes(self.profile)?;
        let initial_buffer_bytes = self
            .initial_buffer_bytes()
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        self.require_mapped_range(self.memory.ring_iova, ring_bytes, mappings)?;
        self.require_mapped_range(
            self.memory.alloc_index_iova,
            SUNFISH_HTT_RX_RING_INDEX_BYTES,
            mappings,
        )?;
        self.require_mapped_range(
            self.memory.initial_buffers_iova,
            initial_buffer_bytes,
            mappings,
        )?;
        self.require_mapped_range(
            self.memory.fallback_buffers_iova,
            self.profile.fallback_buffer_capacity_bytes(),
            mappings,
        )?;
        Ok(())
    }

    /// Write the 64-bit target buffer addresses to a caller-owned ring image.
    /// The unfilled final ring slot and all trailing caller bytes are preserved
    /// as zero/untouched respectively. A short output causes no writes.
    pub fn write_initial_address_ring(
        self,
        output: &mut [u8],
    ) -> Result<usize, SunfishHttRxRingError> {
        let ring_bytes = ring_storage_bytes(self.profile)?;
        let output_len =
            usize::try_from(ring_bytes).map_err(|_| SunfishHttRxRingError::AddressOverflow)?;
        if output.len() < output_len {
            return Err(SunfishHttRxRingError::OutputTooSmall);
        }

        output[..output_len].fill(0);
        for index in 0..self.profile.initial_fill_level() {
            let buffer_address = self.initial_buffer_iova(index)?;
            let offset = usize::try_from(u64::from(index) * SUNFISH_HTT_RX_RING_ENTRY_BYTES)
                .map_err(|_| SunfishHttRxRingError::AddressOverflow)?;
            output[offset..offset + 8].copy_from_slice(&buffer_address.to_le_bytes());
        }
        Ok(output_len)
    }

    /// Write the host allocation index after the initial ring fill.
    pub fn write_initial_alloc_index(
        self,
        output: &mut [u8],
    ) -> Result<usize, SunfishHttRxRingError> {
        if output.len() < SUNFISH_HTT_RX_RING_INDEX_BYTES as usize {
            return Err(SunfishHttRxRingError::OutputTooSmall);
        }
        output[..4].copy_from_slice(&self.initial_alloc_index().to_le_bytes());
        Ok(SUNFISH_HTT_RX_RING_INDEX_BYTES as usize)
    }

    /// Encode the 48-byte, one-ring HTT payload (without its HTC frame header).
    /// Output capacity is checked before any byte is changed.
    pub fn encode_ring_config(self, output: &mut [u8]) -> Result<usize, SunfishHttRxRingError> {
        if output.len() < SUNFISH_HTT_RX_RING_CFG_BYTES {
            return Err(SunfishHttRxRingError::OutputTooSmall);
        }

        let mut words = [0u32; SUNFISH_HTT_RX_RING_CFG_WORDS];
        words[RX_RING_CFG_HEADER_WORD] =
            HTT_H2T_MSG_TYPE_RX_RING_CFG | (HTT_RX_RING_CFG_NUM_RINGS << 8);
        words[RX_RING_CFG_ALLOC_IDX_LO_WORD] = self.memory.alloc_index_iova as u32;
        words[RX_RING_CFG_ALLOC_IDX_HI_WORD] = (self.memory.alloc_index_iova >> 32) as u32;
        words[RX_RING_CFG_RING_BASE_LO_WORD] = self.memory.ring_iova as u32;
        words[RX_RING_CFG_RING_BASE_HI_WORD] = ((self.memory.ring_iova >> 32) & 0x1f) as u32;
        words[RX_RING_CFG_LEN_AND_BUFFER_SIZE_WORD] =
            self.profile.ring_slots() | (self.profile.rx_buffer_bytes() << 16);

        let mut flags = FLAG_MSDU_PAYLOAD
            | FLAG_MPDU_START
            | FLAG_MPDU_END
            | FLAG_MSDU_START
            | FLAG_MSDU_END
            | FLAG_RX_ATTN
            | FLAG_FRAG_INFO
            | FLAG_UCAST
            | FLAG_MCAST;
        if self.capture_extra_metadata {
            flags |= FLAG_80211_HEADER
                | FLAG_PPDU_START
                | FLAG_PPDU_END
                | FLAG_CTRL
                | FLAG_MGMT
                | FLAG_NULL
                | FLAG_PHY;
        }
        words[RX_RING_CFG_FLAGS_AND_INITIAL_INDEX_WORD] =
            flags | (self.initial_alloc_index() << 16);

        let offsets = SUNFISH_HTT_RX_DESCRIPTOR_OFFSETS;
        words[RX_RING_CFG_80211_AND_PAYLOAD_OFFSET_WORD] =
            u32::from(offsets.hdr_status_words) | (u32::from(offsets.msdu_payload_words) << 16);
        words[RX_RING_CFG_PPDU_OFFSETS_WORD] =
            u32::from(offsets.ppdu_start_words) | (u32::from(offsets.ppdu_end_words) << 16);
        words[RX_RING_CFG_MPDU_OFFSETS_WORD] =
            u32::from(offsets.mpdu_start_words) | (u32::from(offsets.mpdu_end_words) << 16);
        words[RX_RING_CFG_MSDU_OFFSETS_WORD] =
            u32::from(offsets.msdu_start_words) | (u32::from(offsets.msdu_end_words) << 16);
        words[RX_RING_CFG_ATTN_AND_FRAG_OFFSETS_WORD] =
            u32::from(offsets.attention_words) | (u32::from(offsets.fragment_info_words) << 16);

        let mut encoded = [0u8; SUNFISH_HTT_RX_RING_CFG_BYTES];
        for (index, word) in words.into_iter().enumerate() {
            let offset = index * 4;
            encoded[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        output[..encoded.len()].copy_from_slice(&encoded);
        Ok(encoded.len())
    }

    /// Return the fixed IOVA for one of the buffers published in the initial
    /// HTT RX address ring. IDs are the ring's software buffer identities.
    pub fn initial_buffer_iova(self, index: u32) -> Result<u64, SunfishHttRxRingError> {
        if index >= self.profile.initial_fill_level() {
            return Err(SunfishHttRxRingError::InvalidBufferIndex);
        }
        let displacement = u64::from(index)
            .checked_mul(u64::from(self.profile.rx_buffer_bytes()))
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        let address = self
            .memory
            .initial_buffers_iova
            .checked_add(displacement)
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        address_span(
            address,
            u64::from(self.profile.rx_buffer_bytes()),
            SUNFISH_HTT_RX_IOVA_BITS,
        )?;
        Ok(address)
    }

    /// Resolve a software buffer identity to its fixed startup or fallback
    /// IOVA window.
    pub fn buffer_iova(self, buffer_id: u32) -> Result<u64, SunfishHttRxRingError> {
        if buffer_id < self.profile.initial_fill_level() {
            self.initial_buffer_iova(buffer_id)
        } else {
            self.fallback_buffer_iova(buffer_id - self.profile.initial_fill_level())
        }
    }

    /// Return the fixed IOVA for one buffer in the separate fallback reserve.
    /// Fallback IDs begin immediately after the initial ring's posted IDs.
    pub fn fallback_buffer_iova(self, index: u32) -> Result<u64, SunfishHttRxRingError> {
        if index >= self.fallback_buffer_count() {
            return Err(SunfishHttRxRingError::InvalidBufferIndex);
        }
        let displacement = u64::from(index)
            .checked_mul(u64::from(self.profile.rx_buffer_bytes()))
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        let address = self
            .memory
            .fallback_buffers_iova
            .checked_add(displacement)
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        address_span(
            address,
            u64::from(self.profile.rx_buffer_bytes()),
            SUNFISH_HTT_RX_IOVA_BITS,
        )?;
        Ok(address)
    }

    fn require_mapped_range(
        self,
        start: u64,
        bytes: u64,
        mappings: &[LpaePageMapping],
    ) -> Result<(), SunfishHttRxRingError> {
        let end = start
            .checked_add(bytes)
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        let first_page = start & !(SMMU_LPAE_PAGE_BYTES - 1);
        let rounded_end = end
            .checked_add(SMMU_LPAE_PAGE_BYTES - 1)
            .ok_or(SunfishHttRxRingError::AddressOverflow)?
            & !(SMMU_LPAE_PAGE_BYTES - 1);
        let mut page = first_page;
        while page < rounded_end {
            if mappings
                .binary_search_by_key(&page, |mapping| mapping.iova_page)
                .is_err()
            {
                return Err(SunfishHttRxRingError::UnmappedDmaPage(page));
            }
            page = page
                .checked_add(SMMU_LPAE_PAGE_BYTES)
                .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        }
        Ok(())
    }
}

/// Fixed, caller-owned backing for the RX buffer IDs and IOVAs in one ring
/// plan. This binds IDs to offsets in the two planned pools; it does not prove
/// that the caller's CPU mappings identify the same physical pages as the
/// SMMU IOVAs.
pub struct SunfishHttRxBufferPool<'a> {
    plan: SunfishHttRxRingPlan,
    initial: &'a mut [u8],
    fallback: &'a mut [u8],
}

impl<'a> SunfishHttRxBufferPool<'a> {
    pub fn new(
        plan: SunfishHttRxRingPlan,
        initial: &'a mut [u8],
        fallback: &'a mut [u8],
    ) -> Result<Self, SunfishHttRxRingError> {
        let initial_bytes = usize::try_from(
            plan.initial_buffer_bytes()
                .ok_or(SunfishHttRxRingError::AddressOverflow)?,
        )
        .map_err(|_| SunfishHttRxRingError::AddressOverflow)?;
        let fallback_bytes = usize::try_from(plan.fallback_buffer_bytes())
            .map_err(|_| SunfishHttRxRingError::AddressOverflow)?;
        if initial.len() < initial_bytes || fallback.len() < fallback_bytes {
            return Err(SunfishHttRxRingError::OutputTooSmall);
        }
        Ok(Self {
            plan,
            initial: &mut initial[..initial_bytes],
            fallback: &mut fallback[..fallback_bytes],
        })
    }

    pub const fn plan(&self) -> SunfishHttRxRingPlan {
        self.plan
    }

    /// Check that both pool slices begin at the virtual addresses selected by
    /// a live DMA-window mapping. The caller remains responsible for proving
    /// that mapping's ownership and device translation.
    pub fn backing_matches_virtual_bases(
        &self,
        initial_virtual: usize,
        fallback_virtual: usize,
    ) -> bool {
        self.initial.as_ptr() as usize == initial_virtual
            && self.fallback.as_ptr() as usize == fallback_virtual
    }

    pub fn buffer(&self, buffer_id: u32) -> Result<&[u8], SunfishHttRxRingError> {
        let (is_initial, offset) = self.backing_and_offset(buffer_id)?;
        let end = offset
            .checked_add(self.plan.buffer_bytes() as usize)
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        let backing = if is_initial {
            &self.initial[..]
        } else {
            &self.fallback[..]
        };
        backing
            .get(offset..end)
            .ok_or(SunfishHttRxRingError::InvalidBufferIndex)
    }

    pub fn buffer_mut(&mut self, buffer_id: u32) -> Result<&mut [u8], SunfishHttRxRingError> {
        let (is_initial, offset) = self.backing_and_offset(buffer_id)?;
        let end = offset
            .checked_add(self.plan.buffer_bytes() as usize)
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        let backing = if is_initial {
            &mut self.initial[..]
        } else {
            &mut self.fallback[..]
        };
        backing
            .get_mut(offset..end)
            .ok_or(SunfishHttRxRingError::InvalidBufferIndex)
    }

    /// Prepare every initially posted buffer in place, clearing only its RX
    /// attention word. The pool geometry was validated at construction.
    pub fn prepare_initial_buffers(&mut self) -> Result<usize, SunfishHttRxRingError> {
        let count = self.plan.initial_fill_level();
        for buffer_id in 0..count {
            let plan = self.plan;
            plan.prepare_buffer_for_device(buffer_id, self.buffer_mut(buffer_id)?)?;
        }
        Ok(count as usize)
    }

    fn backing_and_offset(&self, buffer_id: u32) -> Result<(bool, usize), SunfishHttRxRingError> {
        let initial_count = self.plan.initial_fill_level();
        let (is_initial, index) = if buffer_id < initial_count {
            (true, buffer_id)
        } else {
            (false, buffer_id - initial_count)
        };
        let iova = self.plan.buffer_iova(buffer_id)?;
        let base_iova = if is_initial {
            self.plan.memory.initial_buffers_iova
        } else {
            self.plan.memory.fallback_buffers_iova
        };
        let offset = usize::try_from(iova - base_iova)
            .map_err(|_| SunfishHttRxRingError::AddressOverflow)?;
        let expected_offset = usize::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(self.plan.buffer_bytes() as usize))
            .ok_or(SunfishHttRxRingError::AddressOverflow)?;
        if offset != expected_offset {
            return Err(SunfishHttRxRingError::AddressOverflow);
        }
        Ok((is_initial, offset))
    }
}

pub fn plan_sunfish_htt_rx_ring(
    profile: HttRxProfile,
    memory: SunfishHttRxRingMemory,
    options: SunfishHttRxRingOptions,
) -> Result<SunfishHttRxRingPlan, SunfishHttRxRingError> {
    let expected_profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG)
        .map_err(|_| SunfishHttRxRingError::InvalidProfile)?;
    if profile != expected_profile {
        return Err(SunfishHttRxRingError::InvalidProfile);
    }

    let ring_bytes = ring_storage_bytes(profile)?;
    let initial_buffer_bytes = u64::from(profile.rx_buffer_bytes())
        .checked_mul(u64::from(profile.initial_fill_level()))
        .ok_or(SunfishHttRxRingError::AddressOverflow)?;
    let fallback_buffer_bytes = profile.fallback_buffer_capacity_bytes();
    address_span(
        memory.ring_iova,
        ring_bytes,
        SUNFISH_HTT_RX_RING_BASE_ADDRESS_BITS,
    )?;
    address_span(
        memory.alloc_index_iova,
        SUNFISH_HTT_RX_RING_INDEX_BYTES,
        SUNFISH_HTT_RX_IOVA_BITS,
    )?;
    address_span(
        memory.initial_buffers_iova,
        initial_buffer_bytes,
        SUNFISH_HTT_RX_IOVA_BITS,
    )?;
    address_span(
        memory.fallback_buffers_iova,
        fallback_buffer_bytes,
        SUNFISH_HTT_RX_IOVA_BITS,
    )?;

    if memory.ring_iova & (SUNFISH_HTT_RX_RING_ENTRY_BYTES - 1) != 0 {
        return Err(SunfishHttRxRingError::MisalignedRing);
    }
    if memory.alloc_index_iova & (SUNFISH_HTT_RX_RING_INDEX_BYTES - 1) != 0 {
        return Err(SunfishHttRxRingError::MisalignedAllocIndex);
    }
    if memory.initial_buffers_iova & 7 != 0 {
        return Err(SunfishHttRxRingError::MisalignedBufferPool);
    }
    if memory.fallback_buffers_iova & 7 != 0 {
        return Err(SunfishHttRxRingError::MisalignedBufferPool);
    }

    let root = memory.ring_iova >> 30;
    if (memory.ring_iova + ring_bytes - 1) >> 30 != root
        || memory.alloc_index_iova >> 30 != root
        || (memory.alloc_index_iova + SUNFISH_HTT_RX_RING_INDEX_BYTES - 1) >> 30 != root
        || memory.initial_buffers_iova >> 30 != root
        || (memory.initial_buffers_iova + initial_buffer_bytes - 1) >> 30 != root
        || memory.fallback_buffers_iova >> 30 != root
        || (memory.fallback_buffers_iova + fallback_buffer_bytes - 1) >> 30 != root
    {
        return Err(SunfishHttRxRingError::IovaRootMismatch);
    }

    let ring_end = memory.ring_iova + ring_bytes;
    let index_end = memory.alloc_index_iova + SUNFISH_HTT_RX_RING_INDEX_BYTES;
    let buffer_end = memory.initial_buffers_iova + initial_buffer_bytes;
    let fallback_end = memory.fallback_buffers_iova + fallback_buffer_bytes;
    if spans_overlap(
        memory.ring_iova,
        ring_end,
        memory.alloc_index_iova,
        index_end,
    ) || spans_overlap(
        memory.ring_iova,
        ring_end,
        memory.initial_buffers_iova,
        buffer_end,
    ) || spans_overlap(
        memory.ring_iova,
        ring_end,
        memory.fallback_buffers_iova,
        fallback_end,
    ) || spans_overlap(
        memory.alloc_index_iova,
        index_end,
        memory.initial_buffers_iova,
        buffer_end,
    ) || spans_overlap(
        memory.alloc_index_iova,
        index_end,
        memory.fallback_buffers_iova,
        fallback_end,
    ) || spans_overlap(
        memory.initial_buffers_iova,
        buffer_end,
        memory.fallback_buffers_iova,
        fallback_end,
    ) {
        return Err(SunfishHttRxRingError::MemoryOverlap);
    }

    Ok(SunfishHttRxRingPlan {
        profile,
        memory,
        capture_extra_metadata: options.packet_log_enabled || options.global_monitor_mode,
    })
}

fn ring_storage_bytes(profile: HttRxProfile) -> Result<u64, SunfishHttRxRingError> {
    u64::from(profile.ring_slots())
        .checked_mul(SUNFISH_HTT_RX_RING_ENTRY_BYTES)
        .ok_or(SunfishHttRxRingError::AddressOverflow)
}

fn address_span(start: u64, bytes: u64, width: u32) -> Result<(), SunfishHttRxRingError> {
    let limit = 1u64
        .checked_shl(width)
        .ok_or(SunfishHttRxRingError::AddressOutOfRange)?;
    let end = start
        .checked_add(bytes)
        .ok_or(SunfishHttRxRingError::AddressOverflow)?;
    if start >= limit || end > limit {
        return Err(SunfishHttRxRingError::AddressOutOfRange);
    }
    Ok(())
}

fn spans_overlap(left_start: u64, left_end: u64, right_start: u64, right_end: u64) -> bool {
    left_start < right_end && right_start < left_end
}
