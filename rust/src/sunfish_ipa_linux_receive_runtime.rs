//! Retained exception-RX completion and CPU access, not a Ready capability.
//!
//! Owns the actual startup coordinator and its two pools for its entire
//! lifetime. The event queue cannot supply packet access on its own. A
//! native adapter must still bind guards, ordering, MMIO and IRQ ACK to the
//! retained physical resources. No pool extraction, retry or reset exists.
#![forbid(unsafe_code)]

use crate::{
    sunfish_ipa_linux_buffers::{Memory, Pool},
    sunfish_ipa_linux_command as command, sunfish_ipa_linux_receive as rx,
    sunfish_ipa_linux_receive_events::{self as events, Completion, Queue},
    sunfish_ipa_linux_receive_pool::DataPool,
    sunfish_ipa_linux_receive_start::{self as start, GuardState, ReceiveOnce},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Startup,
    Quarantined,
    Guard,
    State,
    Io,
    Memory,
    PacketBounds,
    Consumer,
    Acknowledge,
    Counter,
    Event(events::Error),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Register {
    IeobMask,
    IeobClear,
    EventDoorbell,
    ChannelDoorbell,
}
impl Register {
    pub const fn offset(self) -> usize {
        match self {
            Self::IeobMask => 0x1f0b8,
            Self::IeobClear => 0x1f0c0,
            Self::EventDoorbell => events::EVENT_DOORBELL,
            Self::ChannelDoorbell => events::CHANNEL_DOORBELL,
        }
    }
}
/// Constructed only by this core. The native adapter independently checks
/// this closed register/value set before its ordered volatile store.
pub struct Request {
    register: Register,
    value: u32,
}
impl Request {
    pub const fn register(&self) -> Register {
        self.register
    }
    pub const fn value(&self) -> u32 {
        self.value
    }
}
pub trait Io<R: Memory, D: Memory>: start::Io<R, D> {
    fn acquire(&mut self) -> Result<(), Error>;
    fn release(&mut self) -> Result<(), Error>;
    fn write_receive(&mut self, request: &Request) -> Result<(), Error>;
    fn acknowledge_receive(&mut self) -> Result<(), Error>;
}
/// A temporary borrow of one validated, CPU-owned completion. It cannot be
/// retained across recycling and supplies neither a pointer nor a writer.
pub trait PacketRead {
    fn completion(&self) -> Completion;
    fn read_into(&mut self, offset: usize, destination: &mut [u8]) -> Result<(), Error>;
}
pub trait Consumer {
    /// Returning success releases the borrow and permits this buffer's
    /// repost. A parser/bridge must finish copying or processing first.
    fn consume(&mut self, packet: &mut impl PacketRead) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub notifications: u64,
    pub spurious: u64,
    pub delivered: u64,
    pub cpu_data_words: u64,
    pub guard_words: u64,
    pub acks_sent: u64,
    pub acks_done: u64,
    pub error: Option<Error>,
}
pub struct Runtime<R: Memory, D: Memory> {
    startup: ReceiveOnce<R, D>,
    queue: Option<Queue>,
    report: Report,
}
fn count(value: &mut u64, n: u64) -> Result<(), Error> {
    *value = value.checked_add(n).ok_or(Error::Counter)?;
    Ok(())
}

struct Access<'a, R: Memory, D: Memory, I: Io<R, D>> {
    rings: &'a mut Pool<R>,
    data: &'a mut DataPool<D>,
    io: &'a mut I,
    expected: GuardState,
    report: &'a mut Report,
    error: Option<Error>,
}
impl<R: Memory, D: Memory, I: Io<R, D>> Access<'_, R, D, I> {
    fn guard(&mut self) -> Result<(), Error> {
        self.io
            .guard(self.rings, self.data, self.expected)
            .map_err(|_| Error::Guard)
    }
    fn read(&mut self, r: start::Read) -> Result<u32, Error> {
        self.guard()?;
        self.io.read(r).map_err(|_| Error::Io)
    }
    fn write(&mut self, register: Register, value: u32) -> Result<(), Error> {
        self.guard()?;
        self.io.write_receive(&Request { register, value })?;
        if register == Register::IeobMask {
            self.expected.set_runtime_ieob(value);
            if self.read(start::Read::Gsi(command::Read::IeobMask))? != value {
                return Err(Error::State);
            }
        }
        Ok(())
    }
    fn event_result<T>(&mut self, result: Result<T, Error>) -> Result<T, events::Error> {
        result.map_err(|e| {
            self.error.get_or_insert(e);
            events::Error::Io
        })
    }
    fn sources(&mut self) -> Result<(u32, u32), Error> {
        use command::Read as Q;
        let types = self.read(start::Read::Gsi(Q::TypeStatus))?;
        let ieob = self.read(start::Read::Gsi(Q::IeobStatus))?;
        if types & !8 != 0 || ieob & !2 != 0 {
            return Err(Error::State);
        }
        for r in [
            Q::ChannelStatus,
            Q::EventStatus,
            Q::GlobalStatus,
            Q::GeneralStatus,
        ] {
            if self.read(start::Read::Gsi(r))? != 0 {
                return Err(Error::State);
            }
        }
        Ok((types, ieob))
    }
    fn deliver(
        &mut self,
        completion: Completion,
        consumer: &mut impl Consumer,
    ) -> Result<(), Error> {
        self.guard()?;
        // Producer acquire and stable event reads precede packet access.
        // This additional acquire also orders accesses in the consumer.
        self.io.acquire()?;
        let mut packet = Packet {
            access: self,
            completion,
            error: None,
        };
        let result = consumer.consume(&mut packet);
        // A consumer cannot ignore a failed/bounds-violating read and then
        // return success to make an uncertain buffer appear reusable.
        if let Some(e) = packet.error {
            return Err(e);
        }
        result.map_err(|_| Error::Consumer)?;
        count(&mut self.report.delivered, 1)?;
        self.guard()
    }
}
struct Packet<'a, 'b, R: Memory, D: Memory, I: Io<R, D>> {
    access: &'a mut Access<'b, R, D, I>,
    completion: Completion,
    error: Option<Error>,
}
impl<R: Memory, D: Memory, I: Io<R, D>> PacketRead for Packet<'_, '_, R, D, I> {
    fn completion(&self) -> Completion {
        self.completion
    }
    fn read_into(&mut self, offset: usize, destination: &mut [u8]) -> Result<(), Error> {
        if self.error.is_some() {
            return Err(Error::Quarantined);
        }
        let result = (|| {
            let end = offset
                .checked_add(destination.len())
                .ok_or(Error::PacketBounds)?;
            if end > self.completion.length() {
                return Err(Error::PacketBounds);
            }
            let mut copied = 0;
            while copied < destination.len() {
                self.access.guard()?;
                let byte = offset + copied;
                let aligned = byte & !3;
                let word = self
                    .access
                    .data
                    .completed_read32(self.completion.buffer(), aligned)
                    .map_err(|_| Error::Memory)?
                    .to_le_bytes();
                count(&mut self.access.report.cpu_data_words, 1)?;
                let start = byte & 3;
                let n = (4 - start).min(destination.len() - copied);
                destination[copied..copied + n].copy_from_slice(&word[start..start + n]);
                copied += n;
            }
            self.access.guard()
        })();
        if let Err(e) = result {
            self.error = Some(e);
        }
        result
    }
}
impl<R: Memory, D: Memory, I: Io<R, D>> events::Io for Access<'_, R, D, I> {
    fn producer(&mut self) -> Result<u32, events::Error> {
        let r = self.read(start::Read::RxProducer);
        self.event_result(r)
    }
    fn acquire(&mut self) -> Result<(), events::Error> {
        let r = self.guard().and_then(|()| self.io.acquire());
        self.event_result(r)
    }
    fn release(&mut self) -> Result<(), events::Error> {
        let r = self.guard().and_then(|()| self.io.release());
        self.event_result(r)
    }
    fn event32(&mut self, offset: usize) -> Result<u32, events::Error> {
        let r = self.guard().and_then(|()| {
            self.rings
                .receive_read32(offset, true)
                .map_err(|_| Error::Memory)
        });
        self.event_result(r)
    }
    fn return_events(&mut self, request: &events::Store) -> Result<(), events::Error> {
        let r = if request.offset() == events::EVENT_DOORBELL {
            self.write(Register::EventDoorbell, request.value())
        } else {
            Err(Error::State)
        };
        self.event_result(r)
    }
    fn write_tre(&mut self, request: &events::Store) -> Result<(), events::Error> {
        let r = self.guard().and_then(|()| {
            self.rings
                .receive_write32(request.offset(), request.value())
                .map_err(|_| Error::Memory)
        });
        self.event_result(r)
    }
    fn read_tre(&mut self, offset: usize) -> Result<u32, events::Error> {
        let r = self.guard().and_then(|()| {
            self.rings
                .receive_read32(offset, false)
                .map_err(|_| Error::Memory)
        });
        self.event_result(r)
    }
    fn publish(&mut self, request: &events::Store) -> Result<(), events::Error> {
        let r = if request.offset() == events::CHANNEL_DOORBELL {
            self.write(Register::ChannelDoorbell, request.value())
        } else {
            Err(Error::State)
        };
        self.event_result(r)
    }
}
impl<R: Memory, D: Memory> Runtime<R, D> {
    /// Always retains startup and its pools, including a rejected attempt.
    pub fn new(mut startup: ReceiveOnce<R, D>) -> Self {
        let queue = if startup.runtime_parts().is_ok() {
            Queue::new(startup.plan()).ok()
        } else {
            None
        };
        let mut report = Report::default();
        if queue.is_none() {
            startup.quarantine_pools();
            report.error = Some(Error::Startup);
        }
        Self {
            startup,
            queue,
            report,
        }
    }
    pub const fn report(&self) -> Report {
        self.report
    }
    pub fn queue_report(&self) -> Option<events::Report> {
        self.queue.as_ref().map(Queue::report)
    }
    pub fn pools(&self) -> (&Pool<R>, &DataPool<D>) {
        self.startup.pools()
    }
    pub fn startup_report(&self) -> start::Report {
        self.startup.report()
    }
    #[allow(dead_code)] // Standalone RX suites exclude combined IPA dispatch.
    pub(crate) fn interrupt_state(&self) -> Result<GuardState, Error> {
        if self.report.error.is_some()
            || self
                .queue
                .as_ref()
                .is_none_or(|q| q.report().error.is_some())
        {
            return Err(Error::Quarantined);
        }
        self.startup.runtime_state().map_err(|_| Error::Startup)
    }
    #[allow(dead_code)]
    pub(crate) fn check_interrupt_sources(&mut self, io: &mut impl Io<R, D>) -> Result<(), Error> {
        self.interrupt_state()?;
        let (rings, data, expected) = self.startup.runtime_parts().map_err(|_| Error::Startup)?;
        let mut access = Access {
            rings,
            data,
            io,
            expected,
            report: &mut self.report,
            error: None,
        };
        access.sources().map(|_| ())
    }
    #[allow(dead_code)]
    pub(crate) fn ipa_dispatch_failed(&mut self, io: &mut impl Io<R, D>) {
        if self.report.error.is_some() {
            return;
        }
        self.report.error = Some(Error::Guard);
        if let Some(queue) = self.queue.as_mut() {
            queue.quarantine(events::Error::Io);
        }
        self.startup.ipa_dispatch_failed(io);
    }
    #[allow(dead_code)]
    pub(crate) fn reject_dispatch_handoff(&mut self) {
        self.report.error.get_or_insert(Error::Startup);
        if let Some(queue) = self.queue.as_mut() {
            queue.quarantine(events::Error::Io);
        }
        self.startup.quarantine_pools();
    }
    fn fail(&mut self, io: &mut impl Io<R, D>, error: Error) -> Error {
        self.report.error.get_or_insert(error);
        if let Some(queue) = self.queue.as_mut() {
            queue.quarantine(events::Error::Io);
        }
        let _ = self.startup.external_fault(io);
        self.startup.quarantine_pools();
        error
    }
    /// Called only for an exclusive physical GSI IRQ delivery. A bounded
    /// snapshot is drained; arrivals during processing remain latched for
    /// the next delivery. Re-enable before ACK, without clearing new work.
    pub fn notification(
        &mut self,
        io: &mut impl Io<R, D>,
        consumer: &mut impl Consumer,
    ) -> Result<usize, Error> {
        if self.report.error.is_some() {
            return Err(Error::Quarantined);
        }
        let result = (|| {
            count(&mut self.report.notifications, 1)?;
            let queue = self.queue.as_mut().ok_or(Error::Startup)?;
            let (rings, data, expected) =
                self.startup.runtime_parts().map_err(|_| Error::Startup)?;
            let mut access = Access {
                rings,
                data,
                io,
                expected,
                report: &mut self.report,
                error: None,
            };
            let (_, source) = access.sources()?;
            let mut drained = 0;
            if source != 0 {
                // Original gsi_isr_ieob(): disable this event, then clear
                // the captured source. Event 0 stays enabled, so the type
                // mask stays 0x4c (not the last-event disable sequence).
                access.write(Register::IeobMask, 1)?;
                access.write(Register::IeobClear, source)?;
                drained = match queue.drain(&mut access) {
                    Ok(n) => n,
                    Err(e) => return Err(access.error.unwrap_or(Error::Event(e))),
                };
                for _ in 0..rx::BUFFER_COUNT {
                    if queue.front().map_err(Error::Event)?.is_none() {
                        break;
                    }
                    let r = queue.consume_and_recycle_with(&mut access, |a, c| {
                        let r = a.deliver(c, consumer);
                        a.event_result(r)
                    });
                    if let Err(e) = r {
                        return Err(access.error.unwrap_or(Error::Event(e)));
                    }
                }
                if queue.report().held != 0 {
                    return Err(Error::State);
                }
                let rings = &*access.rings;
                let state = access.expected;
                let io = &mut *access.io;
                let guard_report = access.data.verify_guards_with(|data| {
                    io.guard(rings, data, state)
                        .map_err(|_| crate::sunfish_ipa_linux_receive_pool::Error::Guard)
                });
                count(
                    &mut access.report.guard_words,
                    guard_report.checked_words as u64,
                )?;
                if guard_report.error.is_some() {
                    return Err(Error::Guard);
                }
                access.write(Register::IeobMask, rx::GSI_IEOB_MASK)?;
            } else {
                count(&mut access.report.spurious, 1)?;
            }
            // A new receive indication is permitted and left intact. Any
            // foreign/error source still prevents ACK and buffer reuse.
            access.sources()?;
            access.guard()?;
            count(&mut access.report.acks_sent, 1)?;
            access
                .io
                .acknowledge_receive()
                .map_err(|_| Error::Acknowledge)?;
            count(&mut access.report.acks_done, 1)?;
            Ok(drained)
        })();
        result.map_err(|e| self.fail(io, e))
    }
    pub fn external_fault(&mut self, io: &mut impl Io<R, D>) -> Result<(), Error> {
        if self.report.error.is_some() {
            return Err(Error::Quarantined);
        }
        Err(self.fail(io, Error::Guard))
    }
}
