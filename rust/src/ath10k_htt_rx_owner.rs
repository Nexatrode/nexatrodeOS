//! Host-testable ownership ledger for the WCN3990 ath10k HTT RX ring.
//!
//! The pinned ath10k path has two mutually exclusive completion modes:
//! low-latency RX consumes buffers in FIFO order, while full-reorder
//! indications return explicit buffer IOVAs. This ledger models both modes
//! and the source-ordered refill publication (address slot, write barrier,
//! allocation index). It does not read descriptors, access DMA memory,
//! synchronize caches, or touch hardware.
#![forbid(unsafe_code)]

use crate::ath10k_htt_rx_ring::{
    RxRingPlan, WCN3990_RX_BUFFER_BYTES, WCN3990_RX_RING_FILL_LEVEL, WCN3990_RX_RING_SLOTS,
};

const RING_SLOTS: usize = WCN3990_RX_RING_SLOTS as usize;
const FILL_LEVEL: usize = WCN3990_RX_RING_FILL_LEVEL as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidGeneration,
    WrongCompletionMode,
    ServiceBitmapTooShort,
    InvalidIova,
    CompletionBeyondPosted,
    DuplicateCompletion,
    OutputTooSmall,
    LeaseExhausted,
    StaleLease,
    RepostPending,
    NoPendingRepost,
    RepostSlotOccupied,
    RingAtFillLevel,
    Quarantined,
}

/// The completion protocol is selected from the WMI service map before RX
/// starts. Full-reorder firmware returns explicit IOVAs and has no FIFO slot
/// completion path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionMode {
    Fifo,
    FullReorder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BufferState {
    Unallocated,
    DeviceFifo { slot: u16 },
    DeviceFullReorder,
    Host { lease_id: u64 },
}

/// Opaque proof that one posted buffer has been returned to the host.
///
/// The ID is single-use: replaying a copied lease is rejected after repost.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxBufferLease {
    generation: u64,
    buffer_id: u16,
    lease_id: u64,
    iova: u64,
    ring_slot: Option<u16>,
    clear_ring_entry: bool,
}

impl RxBufferLease {
    pub const fn buffer_id(self) -> u16 {
        self.buffer_id
    }

    pub const fn iova(self) -> u64 {
        self.iova
    }

    /// Slot removed by this completion. Full-reorder completions identify a
    /// buffer by IOVA, so no ring slot is associated with the returned lease.
    pub const fn ring_slot(self) -> Option<u16> {
        self.ring_slot
    }

    pub const fn clear_ring_entry(self) -> bool {
        self.clear_ring_entry
    }
}

/// The write to perform before publishing the new allocation index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedRepost {
    generation: u64,
    buffer_id: u16,
    lease_id: u64,
    slot: u16,
    iova: u64,
    next_alloc_index: u16,
}

impl PreparedRepost {
    pub const fn slot(self) -> u16 {
        self.slot
    }

    pub const fn iova(self) -> u64 {
        self.iova
    }

    /// Value to publish to the shared allocation-index word after writing the
    /// 64-bit address-ring entry and completing the source-required write
    /// barrier.
    pub const fn next_alloc_index(self) -> u16 {
        self.next_alloc_index
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingRepost {
    prepared: PreparedRepost,
}

/// Software ownership state for one source-matched 2048-entry WCN3990 ring.
///
/// The initial ring has 2047 device-owned buffers and one empty entry. Call
/// [`complete_fifo`](Self::complete_fifo) for low-latency HTT RX counts or
/// [`complete_in_order_iovas`](Self::complete_in_order_iovas) for full-reorder
/// address indications. Select one completion mode at construction; a single
/// owner never mixes the two. Refill is a prepare/publish/commit operation;
/// after an uncertain index publication, quarantine this ledger instead of
/// reusing a possibly device-visible buffer.
pub struct RxRingOwner {
    plan: RxRingPlan,
    generation: u64,
    mode: CompletionMode,
    slots: [Option<u16>; RING_SLOTS],
    buffers: [BufferState; RING_SLOTS],
    read_index: u16,
    alloc_index: u16,
    fill_count: u16,
    next_lease_id: u64,
    pending: Option<PendingRepost>,
    quarantined: bool,
}

impl RxRingOwner {
    pub fn new(plan: RxRingPlan, generation: u64) -> Result<Self, Error> {
        Self::new_with_mode(plan, generation, CompletionMode::Fifo)
    }

    pub fn new_with_mode(
        plan: RxRingPlan,
        generation: u64,
        mode: CompletionMode,
    ) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }

        let mut owner = Self {
            plan,
            generation,
            mode,
            slots: [None; RING_SLOTS],
            buffers: [BufferState::Unallocated; RING_SLOTS],
            read_index: 0,
            alloc_index: WCN3990_RX_RING_FILL_LEVEL as u16,
            fill_count: WCN3990_RX_RING_FILL_LEVEL as u16,
            next_lease_id: 1,
            pending: None,
            quarantined: false,
        };
        for slot in 0..FILL_LEVEL {
            let id = slot as u16;
            owner.buffers[slot] = match mode {
                CompletionMode::Fifo => {
                    owner.slots[slot] = Some(id);
                    BufferState::DeviceFifo { slot: id }
                }
                CompletionMode::FullReorder => BufferState::DeviceFullReorder,
            };
        }
        Ok(owner)
    }

    /// Select the one valid completion mode from the parsed WMI SERVICE_READY
    /// capability bitmap. An absent service word is not treated as FIFO.
    pub fn new_from_service_ready(
        plan: RxRingPlan,
        generation: u64,
        service_ready: &crate::ath10k_wmi::WmiServiceReadyView<'_>,
    ) -> Result<Self, Error> {
        let mode = match service_ready.rx_full_reorder_enabled() {
            Some(true) => CompletionMode::FullReorder,
            Some(false) => CompletionMode::Fifo,
            None => return Err(Error::ServiceBitmapTooShort),
        };
        Self::new_with_mode(plan, generation, mode)
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn indices(&self) -> (u16, u16) {
        (self.read_index, self.alloc_index)
    }

    pub const fn fill_count(&self) -> u16 {
        self.fill_count
    }

    pub const fn is_quarantined(&self) -> bool {
        self.quarantined
    }

    /// Consume `count` completed buffers from the low-latency FIFO head.
    /// Every slot and output-capacity check completes before any ownership
    /// state changes.
    pub fn complete_fifo(
        &mut self,
        count: usize,
        output: &mut [Option<RxBufferLease>],
    ) -> Result<usize, Error> {
        self.check_active()?;
        if self.mode != CompletionMode::Fifo {
            return Err(Error::WrongCompletionMode);
        }
        if self.pending.is_some() {
            return Err(Error::RepostPending);
        }
        if count > usize::from(self.fill_count) {
            return Err(Error::CompletionBeyondPosted);
        }
        if output.len() < count {
            return Err(Error::OutputTooSmall);
        }
        let next_lease_id = self
            .next_lease_id
            .checked_add(count as u64)
            .ok_or(Error::LeaseExhausted)?;

        let mut slot = usize::from(self.read_index);
        for _ in 0..count {
            let buffer_id = self.slots[slot].ok_or(Error::CompletionBeyondPosted)?;
            if self.buffers[usize::from(buffer_id)]
                != (BufferState::DeviceFifo { slot: slot as u16 })
            {
                return Err(Error::CompletionBeyondPosted);
            }
            slot = (slot + 1) & (RING_SLOTS - 1);
        }

        slot = usize::from(self.read_index);
        for out in output.iter_mut().take(count) {
            let buffer_id = self.slots[slot].ok_or(Error::CompletionBeyondPosted)?;
            let lease_id = self.next_lease_id;
            self.next_lease_id += 1;
            self.slots[slot] = None;
            self.buffers[usize::from(buffer_id)] = BufferState::Host { lease_id };
            *out = Some(RxBufferLease {
                generation: self.generation,
                buffer_id,
                lease_id,
                iova: self.buffer_iova(buffer_id),
                ring_slot: Some(slot as u16),
                clear_ring_entry: true,
            });
            slot = (slot + 1) & (RING_SLOTS - 1);
        }
        self.read_index = slot as u16;
        self.fill_count -= count as u16;
        self.next_lease_id = next_lease_id;
        Ok(count)
    }

    /// Consume an explicit full-reorder batch by its reported IOVAs. The batch
    /// must contain distinct currently device-owned buffers; any bad member
    /// leaves the whole batch untouched.
    pub fn complete_in_order_iovas(
        &mut self,
        iovas: &[u64],
        output: &mut [Option<RxBufferLease>],
    ) -> Result<usize, Error> {
        self.check_active()?;
        if self.mode != CompletionMode::FullReorder {
            return Err(Error::WrongCompletionMode);
        }
        if self.pending.is_some() {
            return Err(Error::RepostPending);
        }
        if iovas.len() > usize::from(self.fill_count) {
            return Err(Error::CompletionBeyondPosted);
        }
        if output.len() < iovas.len() {
            return Err(Error::OutputTooSmall);
        }
        let next_lease_id = self
            .next_lease_id
            .checked_add(iovas.len() as u64)
            .ok_or(Error::LeaseExhausted)?;

        let mut ids = [u16::MAX; RING_SLOTS];
        for (index, iova) in iovas.iter().copied().enumerate() {
            let buffer_id = self.buffer_id_for_iova(iova).ok_or(Error::InvalidIova)?;
            match self.buffers[usize::from(buffer_id)] {
                BufferState::DeviceFullReorder => {}
                BufferState::DeviceFifo { .. }
                | BufferState::Host { .. }
                | BufferState::Unallocated => return Err(Error::CompletionBeyondPosted),
            }
            if ids[..index].contains(&buffer_id) {
                return Err(Error::DuplicateCompletion);
            }
            ids[index] = buffer_id;
        }

        for (index, buffer_id) in ids[..iovas.len()].iter().copied().enumerate() {
            if self.buffers[usize::from(buffer_id)] != BufferState::DeviceFullReorder {
                return Err(Error::CompletionBeyondPosted);
            }
            let lease_id = self.next_lease_id;
            self.next_lease_id += 1;
            self.buffers[usize::from(buffer_id)] = BufferState::Host { lease_id };
            output[index] = Some(RxBufferLease {
                generation: self.generation,
                buffer_id,
                lease_id,
                iova: iovas[index],
                ring_slot: None,
                clear_ring_entry: false,
            });
        }
        self.fill_count -= iovas.len() as u16;
        self.next_lease_id = next_lease_id;
        Ok(iovas.len())
    }

    /// Prepare one refill. The caller must write the 64-bit address at
    /// `slot`, issue the source-required write barrier, then publish
    /// `next_alloc_index`; only then call [`commit_repost`](Self::commit_repost).
    pub fn prepare_repost(&mut self, lease: RxBufferLease) -> Result<PreparedRepost, Error> {
        self.check_active()?;
        if self.pending.is_some() {
            return Err(Error::RepostPending);
        }
        self.validate_lease(lease)?;
        if self.fill_count as usize >= FILL_LEVEL {
            return Err(Error::RingAtFillLevel);
        }
        let slot = self.alloc_index;
        if self.mode == CompletionMode::Fifo && self.slots[usize::from(slot)].is_some() {
            return Err(Error::RepostSlotOccupied);
        }
        let prepared = PreparedRepost {
            generation: self.generation,
            buffer_id: lease.buffer_id,
            lease_id: lease.lease_id,
            slot,
            iova: lease.iova,
            next_alloc_index: ((usize::from(slot) + 1) & (RING_SLOTS - 1)) as u16,
        };
        self.pending = Some(PendingRepost { prepared });
        Ok(prepared)
    }

    /// Commit a prepared refill only after its address and allocation index
    /// have both been published in source order.
    pub fn commit_repost(&mut self, prepared: PreparedRepost) -> Result<(), Error> {
        self.check_active()?;
        if self.pending != Some(PendingRepost { prepared })
            || prepared.generation != self.generation
        {
            return Err(Error::NoPendingRepost);
        }
        self.validate_host_buffer(prepared.buffer_id, prepared.lease_id, prepared.iova)?;
        if (self.mode == CompletionMode::Fifo && self.slots[usize::from(prepared.slot)].is_some())
            || self.alloc_index != prepared.slot
            || prepared.next_alloc_index
                != ((usize::from(prepared.slot) + 1) & (RING_SLOTS - 1)) as u16
        {
            return Err(Error::RepostSlotOccupied);
        }
        self.buffers[usize::from(prepared.buffer_id)] = match self.mode {
            CompletionMode::Fifo => {
                self.slots[usize::from(prepared.slot)] = Some(prepared.buffer_id);
                BufferState::DeviceFifo {
                    slot: prepared.slot,
                }
            }
            CompletionMode::FullReorder => BufferState::DeviceFullReorder,
        };
        self.fill_count += 1;
        self.alloc_index = prepared.next_alloc_index;
        self.pending = None;
        Ok(())
    }

    /// Cancel only if the caller can prove the new allocation index was never
    /// made visible to the device.
    pub fn cancel_repost(&mut self, prepared: PreparedRepost) -> Result<(), Error> {
        self.check_active()?;
        if self.pending != Some(PendingRepost { prepared }) {
            return Err(Error::NoPendingRepost);
        }
        self.pending = None;
        Ok(())
    }

    /// A partially or uncertainly published refill makes device ownership
    /// ambiguous. Latch the entire ring until an external reset proves DMA
    /// quiescent; this ledger cannot produce that proof.
    pub fn quarantine_repost(&mut self, prepared: PreparedRepost) -> Result<(), Error> {
        if self.pending != Some(PendingRepost { prepared }) {
            return Err(Error::NoPendingRepost);
        }
        self.quarantined = true;
        self.pending = None;
        Ok(())
    }

    fn check_active(&self) -> Result<(), Error> {
        if self.quarantined {
            Err(Error::Quarantined)
        } else {
            Ok(())
        }
    }

    fn validate_lease(&self, lease: RxBufferLease) -> Result<(), Error> {
        if lease.generation != self.generation {
            return Err(Error::StaleLease);
        }
        self.validate_host_buffer(lease.buffer_id, lease.lease_id, lease.iova)
    }

    fn validate_host_buffer(&self, buffer_id: u16, lease_id: u64, iova: u64) -> Result<(), Error> {
        let id = usize::from(buffer_id);
        if id >= FILL_LEVEL
            || self.buffers[id] != (BufferState::Host { lease_id })
            || self.buffer_iova(buffer_id) != iova
        {
            return Err(Error::StaleLease);
        }
        Ok(())
    }

    fn buffer_id_for_iova(&self, iova: u64) -> Option<u16> {
        let base = self.plan.memory().buffers_iova;
        let offset = iova.checked_sub(base)?;
        if offset % u64::from(WCN3990_RX_BUFFER_BYTES) != 0 {
            return None;
        }
        let id = offset / u64::from(WCN3990_RX_BUFFER_BYTES);
        (id < FILL_LEVEL as u64).then_some(id as u16)
    }

    fn buffer_iova(&self, buffer_id: u16) -> u64 {
        self.plan.memory().buffers_iova + u64::from(buffer_id) * u64::from(WCN3990_RX_BUFFER_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ath10k_htt_rx_ring::{RxRingMemory, plan_rx_ring};

    const GENERATION: u64 = 9;

    fn owner() -> RxRingOwner {
        owner_with_mode(CompletionMode::Fifo)
    }

    fn owner_with_mode(mode: CompletionMode) -> RxRingOwner {
        let plan = plan();
        RxRingOwner::new_with_mode(plan, GENERATION, mode).unwrap()
    }

    fn plan() -> RxRingPlan {
        plan_rx_ring(RxRingMemory {
            ring_iova: 0x1000_0000,
            alloc_index_iova: 0x1000_4000,
            buffers_iova: 0x1001_0000,
            iova_bits: 39,
        })
        .unwrap()
    }

    #[test]
    fn starts_with_2047_device_buffers_and_one_empty_slot() {
        let owner = owner();
        assert_eq!(owner.generation(), GENERATION);
        assert_eq!(owner.fill_count(), 2047);
        assert_eq!(owner.indices(), (0, 2047));
    }

    #[test]
    fn fifo_completion_and_repost_follow_ring_order() {
        let mut owner = owner();
        let mut leases = [None; 3];
        assert_eq!(owner.complete_fifo(2, &mut leases), Ok(2));
        let first = leases[0].unwrap();
        let second = leases[1].unwrap();
        assert_eq!((first.buffer_id(), first.ring_slot()), (0, Some(0)));
        assert_eq!((second.buffer_id(), second.ring_slot()), (1, Some(1)));
        assert!(first.clear_ring_entry());
        assert_eq!(owner.indices(), (2, 2047));
        assert_eq!(owner.fill_count(), 2045);

        let prepared = owner.prepare_repost(first).unwrap();
        assert_eq!(prepared.slot(), 2047);
        assert_eq!(prepared.iova(), first.iova());
        assert_eq!(prepared.next_alloc_index(), 0);
        assert_eq!(owner.fill_count(), 2045);
        owner.commit_repost(prepared).unwrap();
        assert_eq!(owner.indices(), (2, 0));
        assert_eq!(owner.fill_count(), 2046);
        assert_eq!(owner.prepare_repost(first), Err(Error::StaleLease));

        let next = owner.prepare_repost(second).unwrap();
        assert_eq!(next.slot(), 0);
        owner.commit_repost(next).unwrap();
        assert_eq!(owner.fill_count(), 2047);
    }

    #[test]
    fn fifo_batch_validation_is_atomic_and_wraps() {
        let mut owner = owner();
        let mut leases = [None; 2047];
        assert_eq!(
            owner.complete_fifo(2047, &mut leases[..2046]),
            Err(Error::OutputTooSmall)
        );
        assert_eq!(owner.fill_count(), 2047);
        assert_eq!(owner.indices(), (0, 2047));
        assert_eq!(owner.complete_fifo(2047, &mut leases), Ok(2047));
        assert_eq!(owner.fill_count(), 0);
        assert_eq!(owner.indices(), (2047, 2047));
        let prepared = owner.prepare_repost(leases[0].unwrap()).unwrap();
        assert_eq!(prepared.slot(), 2047);
        owner.commit_repost(prepared).unwrap();
        assert_eq!(owner.indices(), (2047, 0));
        assert_eq!(owner.fill_count(), 1);
    }

    #[test]
    fn explicit_iova_completion_accepts_out_of_order_and_rejects_duplicates_atomically() {
        let mut owner = owner_with_mode(CompletionMode::FullReorder);
        let base = 0x1001_0000;
        let mut output = [None; 2];
        assert_eq!(
            owner.complete_in_order_iovas(&[base + 7 * 2048, base + 3 * 2048], &mut output),
            Ok(2)
        );
        assert_eq!(output[0].unwrap().buffer_id(), 7);
        assert_eq!(output[1].unwrap().buffer_id(), 3);
        assert_eq!(output[0].unwrap().ring_slot(), None);
        assert!(!output[0].unwrap().clear_ring_entry());
        assert_eq!(owner.indices(), (0, 2047));
        assert_eq!(owner.fill_count(), 2045);

        let mut untouched = [None; 2];
        assert_eq!(
            owner.complete_in_order_iovas(&[base + 11 * 2048, base + 11 * 2048], &mut untouched),
            Err(Error::DuplicateCompletion)
        );
        assert_eq!(untouched, [None; 2]);
        assert_eq!(owner.fill_count(), 2045);
        assert_eq!(
            owner.complete_in_order_iovas(&[base + 3 * 2048], &mut untouched),
            Err(Error::CompletionBeyondPosted)
        );

        // Full-reorder completion returns a buffer by IOVA, independently of
        // the producer's ring cursor. The wrapped producer slot may contain a
        // stale address from the initial fill; source behavior overwrites it.
        let first_repost = owner.prepare_repost(output[0].unwrap()).unwrap();
        assert_eq!(first_repost.slot(), 2047);
        owner.commit_repost(first_repost).unwrap();
        let mut one = [None; 1];
        owner
            .complete_in_order_iovas(&[base + 11 * 2048], &mut one)
            .unwrap();
        let wrapped_repost = owner.prepare_repost(one[0].unwrap()).unwrap();
        assert_eq!(wrapped_repost.slot(), 0);
        owner.commit_repost(wrapped_repost).unwrap();
        assert_eq!(owner.fill_count(), 2046);
    }

    #[test]
    fn uncertain_refill_quarantines_and_cancel_is_only_a_prepublication_path() {
        let mut owner = owner();
        let mut leases = [None; 1];
        owner.complete_fifo(1, &mut leases).unwrap();
        let lease = leases[0].unwrap();

        let prepared = owner.prepare_repost(lease).unwrap();
        owner.cancel_repost(prepared).unwrap();
        assert_eq!(owner.fill_count(), 2046);
        let prepared = owner.prepare_repost(lease).unwrap();
        owner.quarantine_repost(prepared).unwrap();
        assert!(owner.is_quarantined());
        assert_eq!(owner.complete_fifo(0, &mut []), Err(Error::Quarantined));
        assert_eq!(owner.prepare_repost(lease), Err(Error::Quarantined));
    }

    #[test]
    fn invalid_generation_iova_and_zero_generation_fail_closed() {
        let plan = owner().plan;
        assert_eq!(
            RxRingOwner::new(plan, 0).err(),
            Some(Error::InvalidGeneration)
        );

        let mut owner = owner_with_mode(CompletionMode::FullReorder);
        let mut output = [None; 1];
        assert_eq!(
            owner.complete_in_order_iovas(&[0xdead_0000], &mut output),
            Err(Error::InvalidIova)
        );
        owner
            .complete_in_order_iovas(&[0x1001_0000], &mut output)
            .unwrap();
        let lease = output[0].unwrap();
        let stale = RxBufferLease {
            generation: GENERATION + 1,
            ..lease
        };
        assert_eq!(owner.prepare_repost(stale), Err(Error::StaleLease));
    }

    #[test]
    fn completion_modes_cannot_be_mixed() {
        let mut fifo = owner();
        let mut in_order = owner_with_mode(CompletionMode::FullReorder);
        let mut output = [None; 1];
        assert_eq!(
            fifo.complete_in_order_iovas(&[0x1001_0000], &mut output),
            Err(Error::WrongCompletionMode)
        );
        assert_eq!(
            in_order.complete_fifo(1, &mut output),
            Err(Error::WrongCompletionMode)
        );
        assert_eq!(fifo.fill_count(), 2047);
        assert_eq!(in_order.fill_count(), 2047);
    }

    #[test]
    fn service_ready_selects_one_mode_and_missing_capability_word_fails_closed() {
        use crate::ath10k_wmi::WmiServiceReadyView;

        let plan = plan();
        let mut words = [0u8; 12];
        let fifo_view = WmiServiceReadyView {
            event: &[],
            service_bitmap: &words,
            host_memory_requests: &[],
            declared_host_memory_request_count: 0,
        };
        let fifo = RxRingOwner::new_from_service_ready(plan, GENERATION, &fifo_view).unwrap();
        assert_eq!(fifo.mode, CompletionMode::Fifo);

        words[8] = 1 << 1;
        let full_reorder_view = WmiServiceReadyView {
            event: &[],
            service_bitmap: &words,
            host_memory_requests: &[],
            declared_host_memory_request_count: 0,
        };
        let full_reorder =
            RxRingOwner::new_from_service_ready(plan, GENERATION, &full_reorder_view).unwrap();
        assert_eq!(full_reorder.mode, CompletionMode::FullReorder);

        let short_view = WmiServiceReadyView {
            event: &[],
            service_bitmap: &words[..8],
            host_memory_requests: &[],
            declared_host_memory_request_count: 0,
        };
        assert_eq!(
            RxRingOwner::new_from_service_ready(plan, GENERATION, &short_view).err(),
            Some(Error::ServiceBitmapTooShort)
        );
    }
}
