//! Host-testable descriptor and ring primitives for Qualcomm CE v3.
//!
//! The byte layout and ring behavior follow the QCA_WIFI_3_0 path in the
//! Sunfish Lineage kernel's `qca-wifi-host-cmn/hif/src/ce/ce_internal.h` and
//! `ce_service.c`. This module has no MMIO, DMA allocation, cache operation,
//! interrupt handling, or hardware-ownership capability.

#![forbid(unsafe_code)]

pub const DESCRIPTOR_BYTES: usize = 16;
pub const DESCRIPTOR_RING_ALIGNMENT: usize = 8;
pub const MAX_CE_IOVA: u64 = 1 << 37;
pub const MAX_DESCRIPTOR_BYTES: u32 = u16::MAX as u32;
const ADDRESS_HIGH_MASK: u32 = 0x1f;
const SOURCE_BYTE_SWAP: u32 = 1 << 8;
const DESTINATION_BYTE_SWAP: u32 = 1 << 9;
const GATHER: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidRingSize,
    InvalidAddress,
    InvalidLength,
    InvalidTransferId,
    RingFull,
    StaleTicket,
    GenerationExhausted,
    InvalidHardwareIndex,
    InvalidSlot,
    NotReady,
    AddressChanged,
    BufferTooSmall,
}

/// A host-to-target CE v3 descriptor. Its four little-endian words are
/// address-low, address/control, byte-count/transfer-ID, and hash result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TxDescriptor([u8; DESCRIPTOR_BYTES]);

impl TxDescriptor {
    /// Encode selected fields in the QCACLD v3 `CE_src_desc` layout. Other
    /// control fields and the hash result are zero. This specifies the wire
    /// descriptor only; it does not claim every QCACLD helper publishes all
    /// four words correctly.
    pub fn encode(
        address: u64,
        nbytes: u32,
        transfer_id: u32,
        byte_swap: bool,
        gather: bool,
    ) -> Result<Self, Error> {
        validate_address(address)?;
        validate_length(nbytes)?;
        if transfer_id > u32::from(u16::MAX) {
            return Err(Error::InvalidTransferId);
        }
        let control = ((address >> 32) as u32 & ADDRESS_HIGH_MASK)
            | if byte_swap { SOURCE_BYTE_SWAP } else { 0 }
            | if gather { GATHER } else { 0 };
        let byte_count_and_id = nbytes | (transfer_id << 16);
        Ok(Self(words_to_bytes([
            address as u32,
            control,
            byte_count_and_id,
            0,
        ])))
    }

    pub const fn as_bytes(&self) -> &[u8; DESCRIPTOR_BYTES] {
        &self.0
    }

    pub const fn address(&self) -> u64 {
        descriptor_address(&self.0)
    }

    pub const fn nbytes(&self) -> u16 {
        (read_word(&self.0, 2) & 0xffff) as u16
    }

    pub const fn transfer_id(&self) -> u16 {
        (read_word(&self.0, 2) >> 16) as u16
    }
}

/// A target-to-host CE v3 descriptor posted by the host before receive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxDescriptor([u8; DESCRIPTOR_BYTES]);

impl RxDescriptor {
    /// Create the descriptor form used by `ce_recv_buf_enqueue_legacy`: the
    /// address is set and `nbytes` is cleared until hardware completes it.
    pub fn post(address: u64) -> Result<Self, Error> {
        validate_address(address)?;
        let control = (address >> 32) as u32 & ADDRESS_HIGH_MASK;
        Ok(Self(words_to_bytes([address as u32, control, 0, 0])))
    }

    pub const fn from_bytes(bytes: [u8; DESCRIPTOR_BYTES]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; DESCRIPTOR_BYTES] {
        &self.0
    }

    pub const fn address(&self) -> u64 {
        descriptor_address(&self.0)
    }

    pub fn completion(
        &self,
        posted_address: u64,
        buffer_capacity: u32,
    ) -> Result<RxCompletion, Error> {
        let address = descriptor_address(&self.0);
        if address != posted_address {
            return Err(Error::AddressChanged);
        }
        let result = read_word(&self.0, 2);
        let nbytes = result as u16;
        if nbytes == 0 {
            // The reference driver treats an updated destination read-index
            // with a still-zero descriptor length as a completion race.
            return Err(Error::NotReady);
        }
        if u32::from(nbytes) > buffer_capacity {
            return Err(Error::BufferTooSmall);
        }
        let control = read_word(&self.0, 1);
        Ok(RxCompletion {
            address,
            nbytes,
            transfer_id: (result >> 16) as u16,
            byte_swapped: control & DESTINATION_BYTE_SWAP != 0,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxCompletion {
    pub address: u64,
    pub nbytes: u16,
    pub transfer_id: u16,
    pub byte_swapped: bool,
}

/// A reservation ticket for a single descriptor slot. The generation makes a
/// stale ticket invalid even if its slot number recurs after ring wraparound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotTicket {
    index: usize,
    generation: u64,
}

impl SlotTicket {
    pub const fn index(self) -> usize {
        self.index
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }
}

/// Software-only model of a CE v3 ring cursor. It preserves QCACLD's
/// one-empty-slot rule and separates slot preparation from producer-index
/// publication. A native adapter must write the descriptor, order those
/// stores, publish the hardware index, and only then commit this cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingCursor {
    entries: usize,
    producer: usize,
    consumer: usize,
    producer_generation: u64,
    consumer_generation: u64,
}

impl RingCursor {
    pub fn new(entries: usize) -> Result<Self, Error> {
        if !(2..=u16::MAX as usize + 1).contains(&entries) || !entries.is_power_of_two() {
            return Err(Error::InvalidRingSize);
        }
        Ok(Self {
            entries,
            producer: 0,
            consumer: 0,
            producer_generation: 0,
            consumer_generation: 0,
        })
    }

    pub const fn entries(self) -> usize {
        self.entries
    }

    pub const fn producer_index(self) -> usize {
        self.producer
    }

    pub const fn consumer_index(self) -> usize {
        self.consumer
    }

    pub const fn producer_generation(self) -> u64 {
        self.producer_generation
    }

    pub const fn consumer_generation(self) -> u64 {
        self.consumer_generation
    }

    pub fn available_to_publish(self) -> usize {
        self.entries - 1 - self.pending()
    }

    pub fn pending(self) -> usize {
        self.distance(self.consumer, self.producer)
    }

    /// Return the ticket for the oldest posted destination descriptor.
    pub fn next_rx_completion_ticket(self) -> Result<SlotTicket, Error> {
        if self.pending() == 0 {
            return Err(Error::InvalidSlot);
        }
        Ok(SlotTicket {
            index: self.consumer,
            generation: self.consumer_generation,
        })
    }

    /// Count descriptors the hardware has completed up to its current
    /// destination read index, rejecting an index beyond the posted window.
    pub fn rx_completions_to(self, hardware_read_index: usize) -> Result<usize, Error> {
        if hardware_read_index >= self.entries {
            return Err(Error::InvalidHardwareIndex);
        }
        let completed = self.distance(self.consumer, hardware_read_index);
        if completed > self.pending() {
            return Err(Error::InvalidHardwareIndex);
        }
        Ok(completed)
    }

    pub fn prepare_next(self) -> Result<SlotTicket, Error> {
        if self.pending() >= self.entries - 1 {
            return Err(Error::RingFull);
        }
        Ok(SlotTicket {
            index: self.producer,
            generation: self.producer_generation,
        })
    }

    /// Commit after the descriptor write and device producer-index write have
    /// succeeded. An ambiguous hardware write must stop the caller rather than
    /// be retried with this logical cursor.
    pub fn commit_publish(&mut self, ticket: SlotTicket) -> Result<usize, Error> {
        self.commit_publish_batch(ticket, 1)
    }

    /// Commit a consecutive group after every descriptor and the single
    /// device producer-index write have succeeded. Gather transfers use two
    /// source descriptors but publish them together as one packet.
    pub fn commit_publish_batch(
        &mut self,
        first_ticket: SlotTicket,
        descriptor_count: usize,
    ) -> Result<usize, Error> {
        let next = self.preview_publish_batch(first_ticket, descriptor_count)?;
        let increment = u64::try_from(descriptor_count).map_err(|_| Error::GenerationExhausted)?;
        let generation = self
            .producer_generation
            .checked_add(increment)
            .ok_or(Error::GenerationExhausted)?;
        self.producer = next;
        self.producer_generation = generation;
        Ok(self.producer)
    }

    /// Validate a publication ticket and compute the next producer index
    /// without changing cursor state. Native adapters use this before DMA or
    /// MMIO side effects so a failed ticket cannot strand a published slot.
    pub fn preview_publish(self, ticket: SlotTicket) -> Result<usize, Error> {
        self.preview_publish_batch(ticket, 1)
    }

    /// Validate a consecutive descriptor batch before any DMA or MMIO effect.
    /// The packet is rejected if the whole batch cannot fit under the
    /// one-empty-slot rule.
    pub fn preview_publish_batch(
        self,
        first_ticket: SlotTicket,
        descriptor_count: usize,
    ) -> Result<usize, Error> {
        if descriptor_count == 0 {
            return Err(Error::InvalidSlot);
        }
        if first_ticket.index != self.producer
            || first_ticket.generation != self.producer_generation
        {
            return Err(Error::StaleTicket);
        }
        if descriptor_count > self.available_to_publish() {
            return Err(Error::RingFull);
        }
        let increment = u64::try_from(descriptor_count).map_err(|_| Error::GenerationExhausted)?;
        self.producer_generation
            .checked_add(increment)
            .ok_or(Error::GenerationExhausted)?;
        Ok((self.producer + descriptor_count) & (self.entries - 1))
    }

    /// Retire source descriptors up to the hardware read index. The read
    /// index must lie within the currently published producer window.
    pub fn preview_reclaim_tx_to(self, hardware_read_index: usize) -> Result<usize, Error> {
        if hardware_read_index >= self.entries {
            return Err(Error::InvalidHardwareIndex);
        }
        let completed = self.distance(self.consumer, hardware_read_index);
        if completed > self.pending() {
            return Err(Error::InvalidHardwareIndex);
        }
        self.consumer_generation
            .checked_add(completed as u64)
            .ok_or(Error::GenerationExhausted)?;
        Ok(completed)
    }

    /// Whether a published TX ticket lies in the hardware-completed interval
    /// from the current software consumer through `hardware_read_index`.
    pub fn tx_ticket_completed_to(
        self,
        ticket: SlotTicket,
        hardware_read_index: usize,
    ) -> Result<bool, Error> {
        let completed = self.preview_reclaim_tx_to(hardware_read_index)?;
        if ticket.generation < self.consumer_generation
            || ticket.generation >= self.producer_generation
        {
            return Err(Error::StaleTicket);
        }
        let offset = usize::try_from(ticket.generation - self.consumer_generation)
            .map_err(|_| Error::GenerationExhausted)?;
        if offset >= completed {
            return Ok(false);
        }
        let expected_index = (self.consumer + offset) & (self.entries - 1);
        if ticket.index != expected_index {
            return Err(Error::StaleTicket);
        }
        Ok(true)
    }

    pub fn reclaim_tx_to(&mut self, hardware_read_index: usize) -> Result<usize, Error> {
        let completed = self.preview_reclaim_tx_to(hardware_read_index)?;
        let generation = self
            .consumer_generation
            .checked_add(completed as u64)
            .ok_or(Error::GenerationExhausted)?;
        self.consumer = hardware_read_index;
        self.consumer_generation = generation;
        Ok(completed)
    }

    /// Consume one destination descriptor only after it has a valid nonzero
    /// completion length and still names the posted IOVA.
    pub fn consume_rx(
        &mut self,
        ticket: SlotTicket,
        descriptor: &RxDescriptor,
        posted_address: u64,
        buffer_capacity: u32,
    ) -> Result<RxCompletion, Error> {
        let completion =
            self.preview_consume_rx(ticket, descriptor, posted_address, buffer_capacity)?;
        let generation = self
            .consumer_generation
            .checked_add(1)
            .ok_or(Error::GenerationExhausted)?;
        self.consumer = (self.consumer + 1) & (self.entries - 1);
        self.consumer_generation = generation;
        Ok(completion)
    }

    /// Validate a receive completion without changing cursor state. This lets
    /// native code verify all fallible bookkeeping before copying to an
    /// application buffer.
    pub fn preview_consume_rx(
        self,
        ticket: SlotTicket,
        descriptor: &RxDescriptor,
        posted_address: u64,
        buffer_capacity: u32,
    ) -> Result<RxCompletion, Error> {
        if ticket.index != self.consumer || ticket.generation != self.consumer_generation {
            return Err(Error::InvalidSlot);
        }
        if self.pending() == 0 {
            return Err(Error::InvalidSlot);
        }
        let completion = descriptor.completion(posted_address, buffer_capacity)?;
        self.consumer_generation
            .checked_add(1)
            .ok_or(Error::GenerationExhausted)?;
        Ok(completion)
    }

    const fn distance(self, from: usize, to: usize) -> usize {
        to.wrapping_sub(from) & (self.entries - 1)
    }
}

fn validate_address(address: u64) -> Result<(), Error> {
    if address >= MAX_CE_IOVA {
        return Err(Error::InvalidAddress);
    }
    Ok(())
}

fn validate_length(nbytes: u32) -> Result<(), Error> {
    if nbytes == 0 || nbytes > MAX_DESCRIPTOR_BYTES {
        return Err(Error::InvalidLength);
    }
    Ok(())
}

const fn words_to_bytes(words: [u32; 4]) -> [u8; DESCRIPTOR_BYTES] {
    let mut bytes = [0; DESCRIPTOR_BYTES];
    let mut word = 0;
    while word < words.len() {
        let raw = words[word].to_le_bytes();
        let mut byte = 0;
        while byte < 4 {
            bytes[word * 4 + byte] = raw[byte];
            byte += 1;
        }
        word += 1;
    }
    bytes
}

const fn read_word(bytes: &[u8; DESCRIPTOR_BYTES], word: usize) -> u32 {
    u32::from_le_bytes([
        bytes[word * 4],
        bytes[word * 4 + 1],
        bytes[word * 4 + 2],
        bytes[word * 4 + 3],
    ])
}

const fn descriptor_address(bytes: &[u8; DESCRIPTOR_BYTES]) -> u64 {
    read_word(bytes, 0) as u64 | (((read_word(bytes, 1) & ADDRESS_HIGH_MASK) as u64) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_descriptor(
        address: u64,
        nbytes: u16,
        transfer_id: u16,
        swapped: bool,
    ) -> RxDescriptor {
        let control = ((address >> 32) as u32 & ADDRESS_HIGH_MASK)
            | if swapped { DESTINATION_BYTE_SWAP } else { 0 };
        RxDescriptor::from_bytes(words_to_bytes([
            address as u32,
            control,
            u32::from(nbytes) | (u32::from(transfer_id) << 16),
            0,
        ]))
    }

    #[test]
    fn qcacld_v3_source_descriptor_matches_little_endian_wire_words() {
        let descriptor = TxDescriptor::encode(0x1f_ffff_ffff, 0x1234, 0xabcd, true, true).unwrap();
        assert_eq!(
            descriptor.as_bytes(),
            &[
                0xff, 0xff, 0xff, 0xff, 0x1f, 0x01, 0x00, 0x80, 0x34, 0x12, 0xcd, 0xab, 0x00, 0x00,
                0x00, 0x00
            ]
        );
        assert_eq!(descriptor.address(), 0x1f_ffff_ffff);
        assert_eq!(descriptor.nbytes(), 0x1234);
        assert_eq!(descriptor.transfer_id(), 0xabcd);
    }

    #[test]
    fn receive_post_has_address_and_zero_length_until_hardware_completion() {
        let descriptor = RxDescriptor::post(0x12_3456_789a).unwrap();
        assert_eq!(
            descriptor.as_bytes(),
            &[
                0x9a, 0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00
            ]
        );
        assert_eq!(
            descriptor.completion(0x12_3456_789a, 2048),
            Err(Error::NotReady)
        );
    }

    #[test]
    fn descriptor_bounds_reject_invalid_address_and_lengths() {
        assert_eq!(
            TxDescriptor::encode(MAX_CE_IOVA, 1, 0, false, false),
            Err(Error::InvalidAddress)
        );
        assert_eq!(RxDescriptor::post(MAX_CE_IOVA), Err(Error::InvalidAddress));
        assert_eq!(
            TxDescriptor::encode(0, 0, 0, false, false),
            Err(Error::InvalidLength)
        );
        assert_eq!(
            TxDescriptor::encode(0, MAX_DESCRIPTOR_BYTES + 1, 0, false, false),
            Err(Error::InvalidLength)
        );
        assert_eq!(
            TxDescriptor::encode(0, 1, u32::from(u16::MAX) + 1, false, false),
            Err(Error::InvalidTransferId)
        );
        assert_eq!(
            TxDescriptor::encode(0, 1, 0, false, false)
                .unwrap()
                .nbytes(),
            1
        );
    }

    #[test]
    fn rx_completion_checks_address_capacity_and_reports_transfer_metadata() {
        let address = 0x1_2345_6000;
        let descriptor = completed_descriptor(address, 1500, 0x42, true);
        assert_eq!(
            descriptor.as_bytes(),
            &[
                0x00, 0x60, 0x45, 0x23, 0x01, 0x02, 0x00, 0x00, 0xdc, 0x05, 0x42, 0x00, 0x00, 0x00,
                0x00, 0x00,
            ]
        );
        assert_eq!(
            descriptor.completion(address, 2048),
            Ok(RxCompletion {
                address,
                nbytes: 1500,
                transfer_id: 0x42,
                byte_swapped: true,
            })
        );
        assert_eq!(
            descriptor.completion(address, 1499),
            Err(Error::BufferTooSmall)
        );
        assert_eq!(
            descriptor.completion(address + 0x1000, 2048),
            Err(Error::AddressChanged)
        );
    }

    #[test]
    fn ring_uses_one_empty_slot_and_wraps_without_aliasing_full_and_empty() {
        let mut ring = RingCursor::new(4).unwrap();
        assert_eq!(ring.available_to_publish(), 3);
        for expected in 0..3 {
            let ticket = ring.prepare_next().unwrap();
            assert_eq!(ticket.index(), expected);
            assert_eq!(ring.commit_publish(ticket), Ok((expected + 1) & 3));
        }
        assert_eq!(ring.prepare_next(), Err(Error::RingFull));
        assert_eq!(ring.reclaim_tx_to(3), Ok(3));
        let wrapped = ring.prepare_next().unwrap();
        assert_eq!(wrapped.index(), 3);
        assert_eq!(ring.commit_publish(wrapped), Ok(0));
        assert_eq!(ring.reclaim_tx_to(1), Err(Error::InvalidHardwareIndex));
    }

    #[test]
    fn tx_completion_preflight_binds_read_index_to_ticket_generation() {
        let mut ring = RingCursor::new(4).unwrap();
        let first = ring.prepare_next().unwrap();
        ring.commit_publish(first).unwrap();
        let second = ring.prepare_next().unwrap();
        ring.commit_publish(second).unwrap();

        assert_eq!(ring.preview_reclaim_tx_to(1), Ok(1));
        assert_eq!(ring.tx_ticket_completed_to(first, 1), Ok(true));
        assert_eq!(ring.tx_ticket_completed_to(second, 1), Ok(false));
        assert_eq!(
            ring.tx_ticket_completed_to(first, 4),
            Err(Error::InvalidHardwareIndex)
        );

        ring.reclaim_tx_to(1).unwrap();
        assert_eq!(
            ring.tx_ticket_completed_to(first, 1),
            Err(Error::StaleTicket)
        );
        let wrapped = ring.prepare_next().unwrap();
        assert_eq!(wrapped.index(), 2);
        assert_eq!(wrapped.generation(), 2);
        ring.commit_publish(wrapped).unwrap();
        assert_eq!(ring.tx_ticket_completed_to(second, 2), Ok(true));
        assert_eq!(ring.tx_ticket_completed_to(wrapped, 2), Ok(false));
    }

    #[test]
    fn gather_batch_reserves_and_commits_two_descriptors_atomically() {
        let mut ring = RingCursor::new(4).unwrap();
        let first = ring.prepare_next().unwrap();
        assert_eq!(ring.preview_publish_batch(first, 2), Ok(2));
        assert_eq!(ring.producer_index(), 0);
        assert_eq!(ring.commit_publish_batch(first, 2), Ok(2));
        assert_eq!(ring.producer_index(), 2);
        assert_eq!(ring.pending(), 2);
    }

    #[test]
    fn gather_batch_wraps_and_refuses_partial_capacity_or_zero_count() {
        let mut ring = RingCursor::new(4).unwrap();
        for _ in 0..3 {
            let ticket = ring.prepare_next().unwrap();
            ring.commit_publish(ticket).unwrap();
        }
        ring.reclaim_tx_to(3).unwrap();
        let first = ring.prepare_next().unwrap();
        assert_eq!(first.index(), 3);
        assert_eq!(ring.preview_publish_batch(first, 2), Ok(1));
        assert_eq!(ring.commit_publish_batch(first, 2), Ok(1));
        assert_eq!(ring.producer_index(), 1);
        let one_free = ring.prepare_next().unwrap();
        assert_eq!(
            ring.preview_publish_batch(one_free, 2),
            Err(Error::RingFull)
        );
        assert_eq!(
            ring.preview_publish_batch(one_free, 0),
            Err(Error::InvalidSlot)
        );
    }

    #[test]
    fn ticket_generation_prevents_reuse_after_slot_wrap() {
        let mut ring = RingCursor::new(2).unwrap();
        let old = ring.prepare_next().unwrap();
        ring.commit_publish(old).unwrap();
        ring.reclaim_tx_to(1).unwrap();
        let current = ring.prepare_next().unwrap();
        ring.commit_publish(current).unwrap();
        ring.reclaim_tx_to(0).unwrap();
        let reused = ring.prepare_next().unwrap();
        assert_eq!(old.index(), reused.index());
        assert_ne!(old.generation, reused.generation);
        assert_eq!(ring.commit_publish(old), Err(Error::StaleTicket));
        assert_eq!(ring.commit_publish(reused), Ok(1));
    }

    #[test]
    fn invalid_rx_completion_does_not_advance_consumer() {
        let mut ring = RingCursor::new(4).unwrap();
        let ticket = ring.prepare_next().unwrap();
        ring.commit_publish(ticket).unwrap();
        let address = 0x1000;
        let not_ready = RxDescriptor::post(address).unwrap();
        assert_eq!(
            ring.consume_rx(ticket, &not_ready, address, 2048),
            Err(Error::NotReady)
        );
        assert_eq!(ring.consumer_index(), 0);
        let ready = completed_descriptor(address, 512, 7, false);
        assert_eq!(
            ring.consume_rx(ticket, &ready, address, 2048)
                .unwrap()
                .nbytes,
            512
        );
        assert_eq!(ring.consumer_index(), 1);
    }

    #[test]
    fn ring_size_and_hardware_index_are_bounded() {
        for size in [0, 1, 3, 128 * 1024] {
            assert_eq!(RingCursor::new(size), Err(Error::InvalidRingSize));
        }
        let mut ring = RingCursor::new(8).unwrap();
        assert_eq!(ring.reclaim_tx_to(8), Err(Error::InvalidHardwareIndex));
        assert_eq!(ring.reclaim_tx_to(1), Err(Error::InvalidHardwareIndex));
    }

    #[test]
    fn generation_exhaustion_fails_closed_without_advancing_ring_state() {
        let mut tx = RingCursor::new(2).unwrap();
        tx.producer_generation = u64::MAX;
        let ticket = tx.prepare_next().unwrap();
        assert_eq!(tx.commit_publish(ticket), Err(Error::GenerationExhausted));
        assert_eq!(tx.producer_index(), 0);
    }
}
