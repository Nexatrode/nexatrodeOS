//! Bounded, retained IPA exception-RX ring bookkeeping.
//!
//! Pinned reference: Linux 2267e248204e7e051e0f7355298949a6440de579,
//! gsi_event_trans(), gsi_evt_ring_update() and the two ring doorbells.
//! This core has no physical memory, IRQ, mapping or packet-read authority.
//! A native owner must retain both real pools, perform every DMA ordering
//! operation and guard each I/O. Constructing this model proves no RX.
#![forbid(unsafe_code)]

use crate::sunfish_ipa_linux_receive::{self as rx, Plan};

pub const PRODUCER: usize = 0x1d090;
pub const EVENT_DOORBELL: usize = 0x1e108;
pub const CHANNEL_DOORBELL: usize = 0x1e010;
const ELEMENT: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Layout,
    Quarantined,
    Io,
    Producer,
    ExcessEvents,
    TornEvent,
    Channel,
    Transfer,
    Length,
    Ownership,
    Consumer,
    Readback,
    Counter,
}
/// Metadata only. It grants no CPU access to a device buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    buffer: usize,
    event_index: usize,
    transfer_iova: u64,
    length: usize,
    code: u8,
    kind: u8,
    raw: [u8; ELEMENT],
}
impl Completion {
    pub const fn buffer(self) -> usize {
        self.buffer
    }
    pub const fn event_index(self) -> usize {
        self.event_index
    }
    pub const fn transfer_iova(self) -> u64 {
        self.transfer_iova
    }
    pub const fn length(self) -> usize {
        self.length
    }
    pub const fn code(self) -> u8 {
        self.code
    }
    pub const fn kind(self) -> u8 {
        self.kind
    }
    pub const fn raw(self) -> [u8; ELEMENT] {
        self.raw
    }
}
/// Only this core constructs a store request. Diagnostics expose its value.
pub struct Store {
    offset: usize,
    value: u32,
}
impl Store {
    pub const fn offset(&self) -> usize {
        self.offset
    }
    pub const fn value(&self) -> u32 {
        self.value
    }
}
pub trait Io {
    fn producer(&mut self) -> Result<u32, Error>;
    /// Producer load -> actual device-to-CPU DMA acquire barrier.
    fn acquire(&mut self) -> Result<(), Error>;
    fn event32(&mut self, offset: usize) -> Result<u32, Error>;
    fn return_events(&mut self, request: &Store) -> Result<(), Error>;
    fn write_tre(&mut self, request: &Store) -> Result<(), Error>;
    fn read_tre(&mut self, offset: usize) -> Result<u32, Error>;
    /// Full DMA ordering barrier: prior reads AND writes precede the next
    /// doorbell. Used for event credits and completed-buffer reposting.
    fn release(&mut self) -> Result<(), Error>;
    fn publish(&mut self, request: &Store) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub event_cursor: usize,
    pub completion_cursor: usize,
    pub publish_cursor: usize,
    pub in_flight: usize,
    pub held: usize,
    pub events: u64,
    pub bytes: u64,
    pub event_doorbells_sent: u64,
    pub event_doorbells_done: u64,
    pub descriptor_words_sent: u64,
    pub descriptor_words_done: u64,
    pub descriptor_words_checked: u64,
    pub channel_doorbells_sent: u64,
    pub channel_doorbells_done: u64,
    pub consumed: u64,
    pub recycled: u64,
    pub error: Option<Error>,
}
pub struct Queue {
    plan: Plan,
    tre_base: u64,
    tre_offset: usize,
    slots: [Option<usize>; rx::RING_COUNT],
    owned: [bool; rx::BUFFER_COUNT],
    completed: [Option<Completion>; rx::BUFFER_COUNT],
    front: usize,
    back: usize,
    report: Report,
}
impl Queue {
    /// Initial software state only. The caller must bind this to its actual
    /// completed ReceiveOnce and all 251 published buffers before native use.
    pub fn new(plan: Plan) -> Result<Self, Error> {
        let first = plan.buffer(0).map_err(|_| Error::Layout)?;
        let mut slots = [None; rx::RING_COUNT];
        for (n, slot) in slots.iter_mut().enumerate().take(rx::BUFFER_COUNT) {
            *slot = Some(n);
        }
        Ok(Self {
            plan,
            tre_base: first.tre_iova(),
            tre_offset: first.tre_offset(),
            slots,
            owned: [true; rx::BUFFER_COUNT],
            completed: [None; rx::BUFFER_COUNT],
            front: 0,
            back: 0,
            report: Report {
                event_cursor: 0,
                completion_cursor: 0,
                publish_cursor: rx::BUFFER_COUNT,
                in_flight: rx::BUFFER_COUNT,
                held: 0,
                events: 0,
                bytes: 0,
                event_doorbells_sent: 0,
                event_doorbells_done: 0,
                descriptor_words_sent: 0,
                descriptor_words_done: 0,
                descriptor_words_checked: 0,
                channel_doorbells_sent: 0,
                channel_doorbells_done: 0,
                consumed: 0,
                recycled: 0,
                error: None,
            },
        })
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    pub fn quarantine(&mut self, error: Error) {
        self.report.error.get_or_insert(error);
    }
    fn live(&self) -> Result<(), Error> {
        if self.report.error.is_some() {
            Err(Error::Quarantined)
        } else {
            Ok(())
        }
    }
    fn count(value: &mut u64, n: u64) -> Result<(), Error> {
        *value = value.checked_add(n).ok_or(Error::Counter)?;
        Ok(())
    }
    fn failed<T>(&mut self, result: Result<T, Error>) -> Result<T, Error> {
        if let Err(e) = result {
            self.quarantine(e);
        }
        result
    }
    fn event(&self, io: &mut impl Io) -> Result<[u8; ELEMENT], Error> {
        let mut raw = [0; ELEMENT];
        let offset = self.plan.event_offset() + self.report.event_cursor * ELEMENT;
        for (n, chunk) in raw.chunks_exact_mut(4).enumerate() {
            chunk.copy_from_slice(&io.event32(offset + 4 * n)?.to_le_bytes());
        }
        Ok(raw)
    }
    fn accept(&mut self, raw: [u8; ELEMENT]) -> Result<(), Error> {
        if raw[15] != rx::CHANNEL {
            return Err(Error::Channel);
        }
        let ptr = u64::from_le_bytes(raw[..8].try_into().map_err(|_| Error::Layout)?);
        // The Linux transaction map is replaced by the retained exact slot
        // map. Do not truncate high pointer bits or round misaligned input.
        if ptr != self.tre_base + (self.report.completion_cursor * ELEMENT) as u64 {
            return Err(Error::Transfer);
        }
        let length = u16::from_le_bytes([raw[8], raw[9]]) as usize;
        if length > rx::DMA_BYTES {
            return Err(Error::Length);
        }
        let slot = &mut self.slots[self.report.completion_cursor];
        let buffer = slot.ok_or(Error::Ownership)?;
        if !self.owned[buffer]
            || self.completed[self.back].is_some()
            || self.report.held >= rx::BUFFER_COUNT
            || self.report.in_flight == 0
        {
            return Err(Error::Ownership);
        }
        self.completed[self.back] = Some(Completion {
            buffer,
            event_index: self.report.event_cursor,
            transfer_iova: ptr,
            length,
            code: raw[11],
            kind: raw[14],
            raw,
        });
        // Raw code/type/reserved bytes are preserved, not invented success
        // predicates. The pinned RX path uses channel, TRE and actual length.
        self.owned[buffer] = false;
        *slot = None;
        self.report.in_flight -= 1;
        self.report.held += 1;
        self.back = (self.back + 1) % rx::BUFFER_COUNT;
        self.report.event_cursor = (self.report.event_cursor + 1) % rx::RING_COUNT;
        self.report.completion_cursor = (self.report.completion_cursor + 1) % rx::RING_COUNT;
        Self::count(&mut self.report.events, 1)?;
        Self::count(&mut self.report.bytes, length as u64)
    }
    /// Drain one producer snapshot; events arriving later remain for the
    /// next physical IRQ/poll cycle. No packet RAM is read by this method.
    pub fn drain(&mut self, io: &mut impl Io) -> Result<usize, Error> {
        self.live()?;
        let result = (|| {
            let producer = u64::from(io.producer()?);
            let relative = producer
                .checked_sub(self.plan.event_iova())
                .ok_or(Error::Producer)? as usize;
            if relative >= rx::RING_COUNT * ELEMENT || relative % ELEMENT != 0 {
                return Err(Error::Producer);
            }
            let next = relative / ELEMENT;
            let count = (next + rx::RING_COUNT - self.report.event_cursor) % rx::RING_COUNT;
            if count > self.report.in_flight || count > rx::BUFFER_COUNT - self.report.held {
                return Err(Error::ExcessEvents);
            }
            if count == 0 {
                return Ok(0);
            }
            io.acquire()?;
            for _ in 0..count {
                let first = self.event(io)?;
                io.acquire()?;
                if self.event(io)? != first {
                    return Err(Error::TornEvent);
                }
                self.accept(first)?;
            }
            // Hardware receives the LAST processed event, not next producer.
            let last = (next + rx::RING_COUNT - 1) % rx::RING_COUNT;
            io.release()?;
            Self::count(&mut self.report.event_doorbells_sent, 1)?;
            io.return_events(&Store {
                offset: EVENT_DOORBELL,
                value: (self.plan.event_iova() + (last * ELEMENT) as u64) as u32,
            })?;
            Self::count(&mut self.report.event_doorbells_done, 1)?;
            Ok(count)
        })();
        self.failed(result)
    }
    /// Metadata only, unavailable after any uncertain/failed operation.
    pub fn front(&self) -> Result<Option<Completion>, Error> {
        self.live()?;
        Ok(self.completed[self.front])
    }
    /// Recycle only the oldest held buffer AFTER successful consumption.
    /// No caller-supplied completion, buffer address, TRE or doorbell exists.
    /// A native consumer must read only that completed buffer's bounded data.
    /// One doorbell per recycled buffer is a valid immediate-flush commit;
    /// it deliberately does not reproduce Linux's delayed work batching.
    pub fn consume_and_recycle(
        &mut self,
        io: &mut impl Io,
        consume: impl FnOnce(Completion) -> Result<(), Error>,
    ) -> Result<Option<Completion>, Error> {
        self.consume_and_recycle_with(io, |_, completion| consume(completion))
    }
    /// The retained coordinator uses one access object for both consumption
    /// and reposting, without a second alias of either physical pool.
    pub(crate) fn consume_and_recycle_with<I: Io>(
        &mut self,
        io: &mut I,
        consume: impl FnOnce(&mut I, Completion) -> Result<(), Error>,
    ) -> Result<Option<Completion>, Error> {
        self.live()?;
        let result = (|| {
            let Some(completion) = self.completed[self.front] else {
                return Ok(None);
            };
            let tail = self.report.publish_cursor;
            if self.report.in_flight >= rx::BUFFER_COUNT
                || self.report.held == 0
                || self.owned[completion.buffer]
                || self.slots[tail].is_some()
            {
                return Err(Error::Ownership);
            }
            consume(io, completion).map_err(|_| Error::Consumer)?;
            Self::count(&mut self.report.consumed, 1)?;
            let buffer = self
                .plan
                .buffer(completion.buffer)
                .map_err(|_| Error::Layout)?;
            let tre = buffer.tre();
            let offset = self.tre_offset + tail * ELEMENT;
            for pass in 0..2 {
                for (n, chunk) in tre.chunks_exact(4).enumerate() {
                    let value = u32::from_le_bytes(chunk.try_into().map_err(|_| Error::Layout)?);
                    let request = Store {
                        offset: offset + 4 * n,
                        value,
                    };
                    if pass == 0 {
                        Self::count(&mut self.report.descriptor_words_sent, 1)?;
                        io.write_tre(&request)?;
                        Self::count(&mut self.report.descriptor_words_done, 1)?;
                    }
                    if io.read_tre(request.offset)? != value {
                        return Err(Error::Readback);
                    }
                    Self::count(&mut self.report.descriptor_words_checked, 1)?;
                }
            }
            io.release()?;
            let next = (tail + 1) % rx::RING_COUNT;
            Self::count(&mut self.report.channel_doorbells_sent, 1)?;
            io.publish(&Store {
                offset: CHANNEL_DOORBELL,
                value: (self.tre_base + (next * ELEMENT) as u64) as u32,
            })?;
            Self::count(&mut self.report.channel_doorbells_done, 1)?;
            // Commit bookkeeping only after publication returns success.
            // Uncertain writes never restore an apparently free buffer.
            self.slots[tail] = Some(completion.buffer);
            self.owned[completion.buffer] = true;
            self.completed[self.front] = None;
            self.front = (self.front + 1) % rx::BUFFER_COUNT;
            self.report.publish_cursor = next;
            self.report.in_flight += 1;
            self.report.held -= 1;
            Self::count(&mut self.report.recycled, 1)?;
            Ok(Some(completion))
        })();
        self.failed(result)
    }
}
