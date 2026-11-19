//! Bounded host ownership model for the Sunfish QCACLD HTT RX ring.
//!
//! QCACLD removes an RX buffer from the device ring before delivering its
//! MSDU to upper layers, then refills the ring only after the packet is no
//! longer using that buffer. This module models that lifetime with opaque
//! buffer IDs and generation-tagged leases. It does not map DMA, synchronize
//! caches, inspect hardware indices, or publish a replacement address.
#![forbid(unsafe_code)]

use crate::sunfish_qcacld_rx::{self, ParseOptions, RxDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidCapacity,
    RingFull,
    NoCompletedBuffer,
    SlotUnavailable,
    DuplicateBufferId(u32),
    BufferUnavailable(u32),
    InvalidLease,
    BufferIdMismatch { expected: u32, actual: u32 },
    GenerationExhausted,
    OutputTooSmall { needed: usize, available: usize },
    WrongDecapFormat { expected: u8, observed: u8 },
    FirmwareDiscard,
    InvalidChainLength,
    ChainExceedsPosted,
    MissingLastMsdu,
    Descriptor(sunfish_qcacld_rx::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RxGroupSyncError<E> {
    Session(Error),
    Sync(E),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SlotState {
    Empty,
    Posted,
    Leased,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Slot {
    buffer_id: u32,
    generation: u32,
    state: SlotState,
}

impl Slot {
    const EMPTY: Self = Self {
        buffer_id: 0,
        generation: 0,
        state: SlotState::Empty,
    };
}

/// Opaque CPU ownership of one completed RX buffer. A lease is valid only
/// until `release` succeeds; callers must not retain packet slices after that.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxBufferLease {
    slot: u16,
    buffer_id: u32,
    generation: u32,
}

impl RxBufferLease {
    pub const fn buffer_id(self) -> u32 {
        self.buffer_id
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CopiedMsdu {
    pub descriptor: RxDescriptor,
    pub bytes_copied: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxMpduGroup {
    pub first_descriptor: RxDescriptor,
    pub msdu_count: usize,
    pub buffer_count: usize,
}

/// One MSDU and its consecutive HTT RX buffers inside the lease array
/// returned for an MPDU. `first_buffer_index` is relative to the caller's
/// `leases_output`; the descriptor lives only on this head buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxMsduChain {
    pub descriptor: RxDescriptor,
    pub first_buffer_index: usize,
    pub buffer_count: usize,
}

/// Fixed-capacity receive-ring ownership state. `N - 1` is the maximum number
/// of posted or leased buffers, matching the target's one-empty-slot ring
/// convention. Consumed buffers remain unavailable for repost until released.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SunfishQcacldRxSession<const N: usize> {
    slots: [Slot; N],
    producer_index: usize,
    consumer_index: usize,
    occupied: usize,
    posted: usize,
}

impl<const N: usize> SunfishQcacldRxSession<N> {
    pub fn new() -> Result<Self, Error> {
        if N < 2 || !N.is_power_of_two() || N > u16::MAX as usize {
            return Err(Error::InvalidCapacity);
        }
        Ok(Self {
            slots: [Slot::EMPTY; N],
            producer_index: 0,
            consumer_index: 0,
            occupied: 0,
            posted: 0,
        })
    }

    /// Post an opaque buffer identity to the next ring slot. Posting is only a
    /// software transition; an external owner must separately prove that the
    /// buffer is mapped, synchronized, and safe for device access.
    pub fn post(&mut self, buffer_id: u32) -> Result<u16, Error> {
        self.post_batch(core::slice::from_ref(&buffer_id))
    }

    /// Post a bounded batch of opaque buffer identities. The entire batch is
    /// validated before any session slot or cursor changes.
    pub fn post_batch(&mut self, buffer_ids: &[u32]) -> Result<u16, Error> {
        let first_index = self.preview_post_batch(buffer_ids)?;
        let mut index = usize::from(first_index);
        for &buffer_id in buffer_ids {
            let slot = &mut self.slots[index];
            // `preview_post_batch` validated every target slot and generation
            // before this commit loop begins.
            slot.buffer_id = buffer_id;
            slot.generation += 1;
            slot.state = SlotState::Posted;
            index = next_index::<N>(index);
        }
        self.producer_index = index;
        self.occupied += buffer_ids.len();
        self.posted += buffer_ids.len();
        Ok(first_index)
    }

    /// Validate a future post without changing slot ownership or advancing
    /// the producer index. This lets a hardware publisher check all software
    /// conditions before making a ring entry visible to the device.
    pub fn preview_post(&self, buffer_id: u32) -> Result<u16, Error> {
        self.preview_post_batch(core::slice::from_ref(&buffer_id))
    }

    /// Preflight a complete refill without changing ownership or the producer
    /// cursor. Buffer identities must be unique both against live slots and
    /// within the requested batch.
    pub fn preview_post_batch(&self, buffer_ids: &[u32]) -> Result<u16, Error> {
        for (batch_index, &buffer_id) in buffer_ids.iter().enumerate() {
            if self
                .slots
                .iter()
                .any(|slot| slot.state != SlotState::Empty && slot.buffer_id == buffer_id)
                || buffer_ids[..batch_index].contains(&buffer_id)
            {
                return Err(Error::DuplicateBufferId(buffer_id));
            }
        }
        if buffer_ids.len() > self.available_slots() {
            return Err(Error::RingFull);
        }

        let mut index = self.producer_index;
        for _ in buffer_ids {
            let slot = self.slots.get(index).ok_or(Error::SlotUnavailable)?;
            if slot.state != SlotState::Empty {
                return Err(Error::SlotUnavailable);
            }
            slot.generation
                .checked_add(1)
                .ok_or(Error::GenerationExhausted)?;
            index = next_index::<N>(index);
        }
        Ok(self.producer_index as u16)
    }

    /// Claim the next buffer named by the caller's already-validated RX
    /// indication. The caller must perform required DMA/cache synchronization
    /// before reading bytes from the leased buffer.
    pub fn take_next(&mut self) -> Result<RxBufferLease, Error> {
        if self.posted == 0 {
            return Err(Error::NoCompletedBuffer);
        }
        let index = self.consumer_index;
        let slot = self.slots.get_mut(index).ok_or(Error::SlotUnavailable)?;
        if slot.state != SlotState::Posted {
            return Err(Error::SlotUnavailable);
        }
        slot.state = SlotState::Leased;
        self.consumer_index = next_index::<N>(index);
        self.posted -= 1;
        Ok(RxBufferLease {
            slot: index as u16,
            buffer_id: slot.buffer_id,
            generation: slot.generation,
        })
    }

    /// Claim every HTT MSDU-ring buffer belonging to the next MPDU.
    ///
    /// QCACLD's low-latency path reads a descriptor at each MSDU head, pops
    /// `1 + ring2_more_count` FIFO buffers for that MSDU, and continues until
    /// `last_msdu` is set. This method validates that whole variable-length
    /// group before changing ownership, so bad or incomplete metadata cannot
    /// strand a partially leased MPDU. The per-MSDU output describes each
    /// chain's head descriptor and its span in the lease output. Callers still
    /// perform source-appropriate cache synchronization, packet assembly,
    /// policy, and explicit releases.
    pub fn take_next_mpdu_group(
        &mut self,
        buffers_by_id: &[&[u8]],
        options: ParseOptions,
        msdus_output: &mut [Option<RxMsduChain>],
        leases_output: &mut [Option<RxBufferLease>],
    ) -> Result<RxMpduGroup, Error> {
        self.take_next_mpdu_group_inner(buffers_by_id, options, None, msdus_output, leases_output)
    }

    /// Drain one MPDU while taking each MSDU's firmware status byte from the
    /// enclosing RX indication. `firmware_descriptor_start` is the byte index
    /// for this MPDU within `HTT_RX_IND`; missing tail bytes follow QCACLD and
    /// decode as a zero status byte.
    pub fn take_next_mpdu_group_with_firmware_descriptors(
        &mut self,
        buffers_by_id: &[&[u8]],
        options: ParseOptions,
        firmware_descriptor_bytes: &[u8],
        firmware_descriptor_start: usize,
        msdus_output: &mut [Option<RxMsduChain>],
        leases_output: &mut [Option<RxBufferLease>],
    ) -> Result<RxMpduGroup, Error> {
        self.take_next_mpdu_group_inner(
            buffers_by_id,
            options,
            Some((firmware_descriptor_bytes, firmware_descriptor_start)),
            msdus_output,
            leases_output,
        )
    }

    /// Synchronize each next buffer for CPU access before reading from it: the
    /// head before its descriptor is decoded, and continuations before packet
    /// copy. The callback receives opaque IDs in FIFO order. A callback failure
    /// leaves session ownership unchanged, but the caller must quarantine this
    /// ring generation because earlier buffers may already have transitioned
    /// to CPU ownership; do not retry unless the provider guarantees that is
    /// safe.
    pub fn sync_next_mpdu_group_for_cpu<E>(
        &self,
        buffers_by_id: &[&[u8]],
        options: ParseOptions,
        firmware_descriptor_bytes: &[u8],
        firmware_descriptor_start: usize,
        mut sync_for_cpu: impl FnMut(u32) -> Result<(), E>,
    ) -> Result<usize, RxGroupSyncError<E>> {
        if self.posted == 0 {
            return Err(RxGroupSyncError::Session(Error::NoCompletedBuffer));
        }

        let mut ring_index = self.consumer_index;
        let mut buffer_count = 0usize;
        let mut msdu_count = 0usize;
        loop {
            if buffer_count >= self.posted {
                return Err(RxGroupSyncError::Session(Error::MissingLastMsdu));
            }
            let head_slot = self.slots[ring_index];
            if head_slot.state != SlotState::Posted {
                return Err(RxGroupSyncError::Session(Error::SlotUnavailable));
            }
            let buffer_index = usize::try_from(head_slot.buffer_id).map_err(|_| {
                RxGroupSyncError::Session(Error::BufferUnavailable(head_slot.buffer_id))
            })?;
            let buffer =
                buffers_by_id
                    .get(buffer_index)
                    .copied()
                    .ok_or(RxGroupSyncError::Session(Error::BufferUnavailable(
                        head_slot.buffer_id,
                    )))?;
            sync_for_cpu(head_slot.buffer_id).map_err(RxGroupSyncError::Sync)?;
            let descriptor = decode_group_descriptor(
                buffer,
                options,
                Some((firmware_descriptor_bytes, firmware_descriptor_start)),
                msdu_count,
            )
            .map_err(RxGroupSyncError::Session)?;

            let buffers_for_msdu = 1usize + usize::from(descriptor.more_buffers);
            let group_end = buffer_count
                .checked_add(buffers_for_msdu)
                .ok_or(RxGroupSyncError::Session(Error::ChainExceedsPosted))?;
            if group_end > self.posted {
                return Err(RxGroupSyncError::Session(Error::ChainExceedsPosted));
            }
            for continuation in 1..buffers_for_msdu {
                let index = (ring_index + continuation) & (N - 1);
                let slot = self.slots[index];
                if slot.state != SlotState::Posted {
                    return Err(RxGroupSyncError::Session(Error::SlotUnavailable));
                }
                let continuation_index = usize::try_from(slot.buffer_id).map_err(|_| {
                    RxGroupSyncError::Session(Error::BufferUnavailable(slot.buffer_id))
                })?;
                buffers_by_id
                    .get(continuation_index)
                    .ok_or(RxGroupSyncError::Session(Error::BufferUnavailable(
                        slot.buffer_id,
                    )))?;
                sync_for_cpu(slot.buffer_id).map_err(RxGroupSyncError::Sync)?;
            }

            buffer_count = group_end;
            msdu_count += 1;
            ring_index = (ring_index + buffers_for_msdu) & (N - 1);
            if descriptor.last_msdu {
                return Ok(buffer_count);
            }
        }
    }

    /// Copy the next `count` posted buffer IDs in consumer/FIFO order without
    /// changing ownership. `count` should come from a successful group
    /// preflight or CPU-sync operation; all requested slots are checked before
    /// the output slice is written.
    pub fn copy_next_posted_buffer_ids(
        &self,
        count: usize,
        output: &mut [u32],
    ) -> Result<(), Error> {
        if count > self.posted {
            return Err(Error::ChainExceedsPosted);
        }
        if output.len() < count {
            return Err(Error::OutputTooSmall {
                needed: count,
                available: output.len(),
            });
        }
        let mut index = self.consumer_index;
        for _ in 0..count {
            let slot = self.slots.get(index).ok_or(Error::SlotUnavailable)?;
            if slot.state != SlotState::Posted {
                return Err(Error::SlotUnavailable);
            }
            index = next_index::<N>(index);
        }
        index = self.consumer_index;
        for item in output.iter_mut().take(count) {
            *item = self.slots[index].buffer_id;
            index = next_index::<N>(index);
        }
        Ok(())
    }

    fn take_next_mpdu_group_inner(
        &mut self,
        buffers_by_id: &[&[u8]],
        options: ParseOptions,
        firmware_descriptor_bytes: Option<(&[u8], usize)>,
        msdus_output: &mut [Option<RxMsduChain>],
        leases_output: &mut [Option<RxBufferLease>],
    ) -> Result<RxMpduGroup, Error> {
        if self.posted == 0 {
            return Err(Error::NoCompletedBuffer);
        }

        let mut ring_index = self.consumer_index;
        let mut buffer_count = 0usize;
        let mut msdu_count = 0usize;
        let mut first_descriptor = None;
        loop {
            if buffer_count >= self.posted {
                return Err(Error::MissingLastMsdu);
            }
            let head_slot = self.slots[ring_index];
            if head_slot.state != SlotState::Posted {
                return Err(Error::SlotUnavailable);
            }
            let Some(buffer) = buffers_by_id
                .get(
                    usize::try_from(head_slot.buffer_id)
                        .map_err(|_| Error::BufferUnavailable(head_slot.buffer_id))?,
                )
                .copied()
            else {
                return Err(Error::BufferUnavailable(head_slot.buffer_id));
            };
            let descriptor =
                decode_group_descriptor(buffer, options, firmware_descriptor_bytes, msdu_count)?;
            first_descriptor.get_or_insert(descriptor);

            let buffers_for_msdu = 1usize + usize::from(descriptor.more_buffers);
            let group_end = buffer_count
                .checked_add(buffers_for_msdu)
                .ok_or(Error::ChainExceedsPosted)?;
            if group_end > self.posted {
                return Err(Error::ChainExceedsPosted);
            }
            for continuation in 1..buffers_for_msdu {
                let index = (ring_index + continuation) & (N - 1);
                if self.slots[index].state != SlotState::Posted {
                    return Err(Error::SlotUnavailable);
                }
            }

            buffer_count = group_end;
            msdu_count += 1;
            ring_index = (ring_index + buffers_for_msdu) & (N - 1);
            if descriptor.last_msdu {
                break;
            }
        }
        let first_descriptor = first_descriptor.ok_or(Error::NoCompletedBuffer)?;

        if leases_output.len() < buffer_count {
            return Err(Error::OutputTooSmall {
                needed: buffer_count,
                available: leases_output.len(),
            });
        }
        if msdus_output.len() < msdu_count {
            return Err(Error::OutputTooSmall {
                needed: msdu_count,
                available: msdus_output.len(),
            });
        }

        // Rewalk the already-validated immutable descriptors to publish the
        // MSDU-to-buffer boundaries before changing session ownership.
        ring_index = self.consumer_index;
        let mut first_buffer_index = 0usize;
        for (msdu_index, output) in msdus_output.iter_mut().take(msdu_count).enumerate() {
            let head_slot = self.slots[ring_index];
            let buffer_index = usize::try_from(head_slot.buffer_id)
                .map_err(|_| Error::BufferUnavailable(head_slot.buffer_id))?;
            let buffer = buffers_by_id
                .get(buffer_index)
                .copied()
                .ok_or(Error::BufferUnavailable(head_slot.buffer_id))?;
            let descriptor =
                decode_group_descriptor(buffer, options, firmware_descriptor_bytes, msdu_index)?;
            let buffers_for_msdu = 1usize + usize::from(descriptor.more_buffers);
            *output = Some(RxMsduChain {
                descriptor,
                first_buffer_index,
                buffer_count: buffers_for_msdu,
            });
            first_buffer_index += buffers_for_msdu;
            ring_index = (ring_index + buffers_for_msdu) & (N - 1);
        }

        ring_index = self.consumer_index;
        for output in leases_output.iter_mut().take(buffer_count) {
            let slot = &mut self.slots[ring_index];
            slot.state = SlotState::Leased;
            *output = Some(RxBufferLease {
                slot: ring_index as u16,
                buffer_id: slot.buffer_id,
                generation: slot.generation,
            });
            ring_index = next_index::<N>(ring_index);
        }
        self.consumer_index = ring_index;
        self.posted -= buffer_count;

        Ok(RxMpduGroup {
            first_descriptor,
            msdu_count,
            buffer_count,
        })
    }

    /// Copy one supported single-buffer MSDU into caller-owned network
    /// storage. The lease remains active on success or failure, so the caller
    /// explicitly releases it only after delivery or deliberate drop.
    pub fn copy_single_buffer(
        &self,
        lease: RxBufferLease,
        actual_buffer_id: u32,
        buffer: &[u8],
        options: ParseOptions,
        output: &mut [u8],
    ) -> Result<CopiedMsdu, Error> {
        let (descriptor, payload) =
            self.decode_leased_single_buffer(lease, actual_buffer_id, buffer, options)?;
        Self::copy_payload(descriptor, payload, output)
    }

    /// Copy one single-buffer MSDU only when it matches the captured Sunfish
    /// Ethernet-decapsulation mode and QCACLD did not mark it for discard.
    /// This is the format expected by the sDDF Ethernet network bridge.
    pub fn copy_ethernet_single_buffer(
        &self,
        lease: RxBufferLease,
        actual_buffer_id: u32,
        buffer: &[u8],
        options: ParseOptions,
        output: &mut [u8],
    ) -> Result<CopiedMsdu, Error> {
        let (descriptor, payload) =
            self.decode_leased_single_buffer(lease, actual_buffer_id, buffer, options)?;
        if descriptor.decap_format != sunfish_qcacld_rx::SUNFISH_RX_DECAP_FORMAT_8023 {
            return Err(Error::WrongDecapFormat {
                expected: sunfish_qcacld_rx::SUNFISH_RX_DECAP_FORMAT_8023,
                observed: descriptor.decap_format,
            });
        }
        if descriptor.firmware.discard {
            return Err(Error::FirmwareDiscard);
        }
        Self::copy_payload(descriptor, payload, output)
    }

    /// Ethernet-copy variant using the per-MSDU firmware status byte from the
    /// enclosing RX indication, matching QCACLD's descriptor fix-up step.
    pub fn copy_ethernet_single_buffer_with_firmware_descriptor(
        &self,
        lease: RxBufferLease,
        actual_buffer_id: u32,
        buffer: &[u8],
        firmware_descriptor: u8,
        options: ParseOptions,
        output: &mut [u8],
    ) -> Result<CopiedMsdu, Error> {
        self.validate_lease(lease)?;
        if actual_buffer_id != lease.buffer_id {
            return Err(Error::BufferIdMismatch {
                expected: lease.buffer_id,
                actual: actual_buffer_id,
            });
        }
        let (descriptor, payload) =
            sunfish_qcacld_rx::decode_single_buffer_with_firmware_descriptor(
                buffer,
                firmware_descriptor,
                options,
            )
            .map_err(Error::Descriptor)?;
        if descriptor.decap_format != sunfish_qcacld_rx::SUNFISH_RX_DECAP_FORMAT_8023 {
            return Err(Error::WrongDecapFormat {
                expected: sunfish_qcacld_rx::SUNFISH_RX_DECAP_FORMAT_8023,
                observed: descriptor.decap_format,
            });
        }
        if descriptor.firmware.discard {
            return Err(Error::FirmwareDiscard);
        }
        Self::copy_payload(descriptor, payload, output)
    }

    /// Copy one Ethernet-decapsulated QCACLD MSDU from its leased HTT ring
    /// buffers into contiguous caller-owned storage.
    ///
    /// The pinned low-latency path strips the descriptor reservation and
    /// target-reported L3 padding from the first buffer. Ring-2 continuation
    /// buffers start at byte zero; the source trims the last continuation to
    /// `RX_STD_DESC_SIZE + (msdu_length - continuation_count * HTT_RX_BUF_SIZE)`.
    /// This routine follows those byte spans, rejects inconsistent counts or
    /// lengths before writing, and leaves all leases active for explicit
    /// delivery/drop and release by the caller.
    pub fn copy_ethernet_msdu_chain(
        &self,
        chain: RxMsduChain,
        leases: &[Option<RxBufferLease>],
        buffers_by_id: &[&[u8]],
        options: ParseOptions,
        output: &mut [u8],
    ) -> Result<CopiedMsdu, Error> {
        self.copy_ethernet_msdu_chain_inner(chain, leases, buffers_by_id, None, options, output)
    }

    /// Ethernet-copy variant using the per-MSDU firmware descriptor byte from
    /// `HTT_RX_IND`; it does not trust the corresponding byte in the HTT ring
    /// buffer.
    pub fn copy_ethernet_msdu_chain_with_firmware_descriptor(
        &self,
        chain: RxMsduChain,
        leases: &[Option<RxBufferLease>],
        buffers_by_id: &[&[u8]],
        firmware_descriptor: u8,
        options: ParseOptions,
        output: &mut [u8],
    ) -> Result<CopiedMsdu, Error> {
        self.copy_ethernet_msdu_chain_inner(
            chain,
            leases,
            buffers_by_id,
            Some(firmware_descriptor),
            options,
            output,
        )
    }

    fn copy_ethernet_msdu_chain_inner(
        &self,
        chain: RxMsduChain,
        leases: &[Option<RxBufferLease>],
        buffers_by_id: &[&[u8]],
        firmware_descriptor: Option<u8>,
        options: ParseOptions,
        output: &mut [u8],
    ) -> Result<CopiedMsdu, Error> {
        let chain_end = chain
            .first_buffer_index
            .checked_add(chain.buffer_count)
            .ok_or(Error::InvalidChainLength)?;
        let chain_leases = leases
            .get(chain.first_buffer_index..chain_end)
            .ok_or(Error::InvalidChainLength)?;
        let first_lease = chain_leases
            .first()
            .and_then(|lease| *lease)
            .ok_or(Error::InvalidLease)?;
        self.validate_lease(first_lease)?;
        let first_buffer = buffer_for_lease(first_lease, buffers_by_id)?;
        let descriptor = match firmware_descriptor {
            Some(value) => {
                RxDescriptor::decode_with_firmware_descriptor(first_buffer, value, options)
            }
            None => RxDescriptor::decode(first_buffer, options),
        }
        .map_err(Error::Descriptor)?;
        if descriptor != chain.descriptor {
            return Err(Error::InvalidChainLength);
        }
        if descriptor.decap_format != sunfish_qcacld_rx::SUNFISH_RX_DECAP_FORMAT_8023 {
            return Err(Error::WrongDecapFormat {
                expected: sunfish_qcacld_rx::SUNFISH_RX_DECAP_FORMAT_8023,
                observed: descriptor.decap_format,
            });
        }
        if descriptor.firmware.discard {
            return Err(Error::FirmwareDiscard);
        }
        if chain.buffer_count != 1 + usize::from(descriptor.more_buffers) {
            return Err(Error::InvalidChainLength);
        }

        if descriptor.more_buffers == 0 {
            return match firmware_descriptor {
                Some(value) => self.copy_ethernet_single_buffer_with_firmware_descriptor(
                    first_lease,
                    first_lease.buffer_id,
                    first_buffer,
                    value,
                    options,
                    output,
                ),
                None => self.copy_ethernet_single_buffer(
                    first_lease,
                    first_lease.buffer_id,
                    first_buffer,
                    options,
                    output,
                ),
            };
        }

        let profile = crate::sunfish_htt_rx_profile::plan_htt_rx_profile(
            crate::sunfish_htt_rx_profile::SUNFISH_QCACLD_CONFIG,
        )
        .map_err(|_| Error::Descriptor(sunfish_qcacld_rx::Error::InvalidProfile))?;
        let buffer_bytes =
            usize::try_from(profile.rx_buffer_bytes()).map_err(|_| Error::InvalidChainLength)?;
        let descriptor_bytes = usize::try_from(
            crate::sunfish_htt_rx_profile::SUNFISH_QCACLD_CONFIG.host_rx_desc_bytes,
        )
        .map_err(|_| Error::InvalidChainLength)?;
        let padding = usize::from(descriptor.l3_header_padding);
        let first_payload_offset = descriptor_bytes
            .checked_add(padding)
            .ok_or(Error::InvalidChainLength)?;
        let first_payload_bytes = buffer_bytes
            .checked_sub(first_payload_offset)
            .ok_or(Error::InvalidChainLength)?;
        let payload_bytes = usize::from(descriptor.msdu_length)
            .checked_sub(padding)
            .ok_or(Error::InvalidChainLength)?;
        let continuation_count = usize::from(descriptor.more_buffers);

        // QCACLD's continuation count must agree with the descriptor's total
        // length. Earlier continuations occupy a whole buffer; the last one
        // includes the source's preserved descriptor-sized prefix plus the
        // remaining bytes after subtracting one full HTT buffer per fragment.
        let consumed_full_buffers = continuation_count
            .checked_mul(buffer_bytes)
            .ok_or(Error::InvalidChainLength)?;
        let final_remainder = usize::from(descriptor.msdu_length)
            .checked_sub(consumed_full_buffers)
            .ok_or(Error::InvalidChainLength)?;
        let final_continuation_bytes = descriptor_bytes
            .checked_add(final_remainder)
            .filter(|bytes| *bytes <= buffer_bytes)
            .ok_or(Error::InvalidChainLength)?;
        let continuation_bytes = continuation_count
            .checked_sub(1)
            .and_then(|full| full.checked_mul(buffer_bytes))
            .and_then(|full| full.checked_add(final_continuation_bytes))
            .ok_or(Error::InvalidChainLength)?;
        let planned_output_bytes = first_payload_bytes
            .checked_add(continuation_bytes)
            .ok_or(Error::InvalidChainLength)?;
        if payload_bytes <= first_payload_bytes || planned_output_bytes != payload_bytes {
            return Err(Error::InvalidChainLength);
        }
        if output.len() < payload_bytes {
            return Err(Error::OutputTooSmall {
                needed: payload_bytes,
                available: output.len(),
            });
        }

        // Validate every lease and mapping before the first output write.
        let mut continuation_buffers = [&[][..]; u8::MAX as usize];
        for index in 0..continuation_count {
            let lease = chain_leases
                .get(index + 1)
                .and_then(|lease| *lease)
                .ok_or(Error::InvalidLease)?;
            self.validate_lease(lease)?;
            let buffer = buffer_for_lease(lease, buffers_by_id)?;
            if buffer.len() < buffer_bytes {
                return Err(Error::Descriptor(sunfish_qcacld_rx::Error::BufferTooShort));
            }
            continuation_buffers[index] = buffer;
        }
        if first_buffer.len() < buffer_bytes {
            return Err(Error::Descriptor(sunfish_qcacld_rx::Error::BufferTooShort));
        }

        output[..first_payload_bytes]
            .copy_from_slice(&first_buffer[first_payload_offset..buffer_bytes]);
        let mut output_offset = first_payload_bytes;
        for (index, buffer) in continuation_buffers[..continuation_count]
            .iter()
            .enumerate()
        {
            let fragment_bytes = if index + 1 == continuation_count {
                final_continuation_bytes
            } else {
                buffer_bytes
            };
            let end = output_offset + fragment_bytes;
            output[output_offset..end].copy_from_slice(&buffer[..fragment_bytes]);
            output_offset = end;
        }
        debug_assert_eq!(output_offset, payload_bytes);
        Ok(CopiedMsdu {
            descriptor,
            bytes_copied: payload_bytes,
        })
    }

    fn decode_leased_single_buffer<'a>(
        &self,
        lease: RxBufferLease,
        actual_buffer_id: u32,
        buffer: &'a [u8],
        options: ParseOptions,
    ) -> Result<(RxDescriptor, &'a [u8]), Error> {
        self.validate_lease(lease)?;
        if actual_buffer_id != lease.buffer_id {
            return Err(Error::BufferIdMismatch {
                expected: lease.buffer_id,
                actual: actual_buffer_id,
            });
        }
        sunfish_qcacld_rx::decode_single_buffer(buffer, options).map_err(Error::Descriptor)
    }

    fn copy_payload(
        descriptor: RxDescriptor,
        payload: &[u8],
        output: &mut [u8],
    ) -> Result<CopiedMsdu, Error> {
        if output.len() < payload.len() {
            return Err(Error::OutputTooSmall {
                needed: payload.len(),
                available: output.len(),
            });
        }
        output[..payload.len()].copy_from_slice(payload);
        Ok(CopiedMsdu {
            descriptor,
            bytes_copied: payload.len(),
        })
    }

    /// Return a delivered or dropped buffer to software ownership. It may be
    /// reposted only after this transition. Stale or forged leases cannot
    /// release a buffer that has since been reused.
    pub fn release(&mut self, lease: RxBufferLease) -> Result<(), Error> {
        self.validate_lease(lease)?;
        let slot = self
            .slots
            .get_mut(usize::from(lease.slot))
            .ok_or(Error::InvalidLease)?;
        slot.state = SlotState::Empty;
        self.occupied -= 1;
        Ok(())
    }

    /// Resolve a currently leased buffer identity for use with an external
    /// DMA-window plan. A released, stale, or forged lease is rejected.
    pub fn buffer_id_for_lease(&self, lease: RxBufferLease) -> Result<u32, Error> {
        self.validate_lease(lease)?;
        Ok(lease.buffer_id)
    }

    pub const fn posted_count(&self) -> usize {
        self.posted
    }

    pub const fn occupied_count(&self) -> usize {
        self.occupied
    }

    pub const fn available_slots(&self) -> usize {
        N.saturating_sub(1).saturating_sub(self.occupied)
    }

    fn validate_lease(&self, lease: RxBufferLease) -> Result<(), Error> {
        let slot = self
            .slots
            .get(usize::from(lease.slot))
            .ok_or(Error::InvalidLease)?;
        if slot.state != SlotState::Leased
            || slot.buffer_id != lease.buffer_id
            || slot.generation != lease.generation
        {
            return Err(Error::InvalidLease);
        }
        Ok(())
    }
}

fn decode_group_descriptor(
    buffer: &[u8],
    options: ParseOptions,
    firmware_descriptor_bytes: Option<(&[u8], usize)>,
    msdu_index: usize,
) -> Result<RxDescriptor, Error> {
    match firmware_descriptor_bytes {
        Some((bytes, start)) => {
            let index = start
                .checked_add(msdu_index)
                .ok_or(Error::InvalidChainLength)?;
            let firmware_descriptor = bytes.get(index).copied().unwrap_or(0);
            RxDescriptor::decode_with_firmware_descriptor(buffer, firmware_descriptor, options)
                .map_err(Error::Descriptor)
        }
        None => RxDescriptor::decode(buffer, options).map_err(Error::Descriptor),
    }
}

fn buffer_for_lease<'a>(
    lease: RxBufferLease,
    buffers_by_id: &'a [&'a [u8]],
) -> Result<&'a [u8], Error> {
    let index =
        usize::try_from(lease.buffer_id).map_err(|_| Error::BufferUnavailable(lease.buffer_id))?;
    buffers_by_id
        .get(index)
        .copied()
        .ok_or(Error::BufferUnavailable(lease.buffer_id))
}

const fn next_index<const N: usize>(index: usize) -> usize {
    if index + 1 == N { 0 } else { index + 1 }
}
