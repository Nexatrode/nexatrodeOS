//! Cold SMP2P/GLINK allocation extension retaining the actual IPA-497 owner.
//! Fixed selected Linux order; no payload activation, doorbell, PAS or runtime
//! reset. The prepared regions remain owned here on every outcome.
#![forbid(unsafe_code)]
use crate::{
    qcom_modem_status::{self as dir, Directory, Item, Read},
    sunfish_ipa_smem as ipa,
};
use core::mem::ManuallyDrop;

pub const PART: usize = 0x11d000;
pub const PART_BYTES: usize = 0x7c000;
pub const INITIAL_FREE: usize = 0x2030;
pub const FINAL_FREE: usize = 0x61d8;
pub const CHUNK: usize = 4096;
pub const ITEMS: [(u16, Item); 3] = [
    (
        428,
        Item {
            offset: PART + 0x2040,
            size: 340,
        },
    ),
    (
        478,
        Item {
            offset: PART + 0x21a8,
            size: 32,
        },
    ),
    (
        479,
        Item {
            offset: PART + 0x21d8,
            size: 16384,
        },
    ),
];
const FREES: [usize; 4] = [INITIAL_FREE, 0x2198, 0x21c8, FINAL_FREE];
const HEADERS: [usize; 3] = [PART + 0x2030, PART + 0x2198, PART + 0x21c8];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Consumed,
    Predecessor,
    Guard,
    LockBusy,
    LockLost,
    Directory(dir::Error),
    Layout,
    Unstable,
    Store,
    Readback,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Validate,
    Allocate,
    Zero,
    Verify,
    Prepared,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Header,
    Cursor,
    Payload,
}
/// Sealed aligned word operation. Consumers can inspect but cannot construct
/// requests for IPA payloads, remote memory or arbitrary cursors.
pub struct Store {
    offset: usize,
    value: u32,
    kind: Kind,
}
impl Store {
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn value(&self) -> u32 {
        self.value
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
}
/// Trusted platform boundary. guard must bind the real cold pre-auth lifecycle,
/// generation and retained IPA/domain health. store admits only this extension's
/// metadata words or its three fresh payloads. It must not reset/retry/repair.
pub trait Payload {
    fn guard(&mut self, generation: u64) -> Result<(), Error>;
    fn store(&mut self, request: &Store) -> Result<(), Error>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Report {
    pub phase: Phase,
    pub error: Option<Error>,
    pub allocations: u8,
    pub lock_attempts: u32,
    pub locks: u32,
    pub releases: u32,
    pub submitted: u32,
    pub completed: u32,
    pub zeroed: usize,
    pub verified: usize,
    pub last_offset: usize,
    pub last_value: u32,
}
/// A bounded preparation result, not MPSS ready or permission to send GLINK.
pub struct Prepared {
    generation: u64,
}
impl Prepared {
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
pub struct Extension<I: ipa::Io, P: Payload> {
    parent: ManuallyDrop<ipa::Owner<I>>,
    payload: ManuallyDrop<P>,
    generation: u64,
    report: Report,
    busy: bool,
    item: usize,
    at: usize,
    taken: bool,
    activation_claimed: bool,
}
fn u16_at(m: &impl Read, at: usize) -> u16 {
    u16::from_le_bytes([m.read8(at), m.read8(at + 1)])
}
fn u32_at(m: &impl Read, at: usize) -> u32 {
    u32::from_le_bytes(core::array::from_fn(|n| m.read8(at + n)))
}
impl<I: ipa::Io, P: Payload> Extension<I, P> {
    /// Own both providers before checking anything. Even a bad predecessor is
    /// retained and terminal; no borrowed address can substitute for it.
    pub fn new(mut parent: ipa::Owner<I>, payload: P, generation: u64) -> Self {
        let error = if generation == 0 || parent.claim_modem_extension().is_err() {
            Some(Error::Predecessor)
        } else {
            None
        };
        Self {
            parent: ManuallyDrop::new(parent),
            payload: ManuallyDrop::new(payload),
            generation,
            report: Report {
                phase: if error.is_some() {
                    Phase::Failed
                } else {
                    Phase::Validate
                },
                error,
                allocations: 0,
                lock_attempts: 0,
                locks: 0,
                releases: 0,
                submitted: 0,
                completed: 0,
                zeroed: 0,
                verified: 0,
                last_offset: 0,
                last_value: 0,
            },
            busy: false,
            item: 0,
            at: 0,
            taken: false,
            activation_claimed: false,
        }
    }
    pub fn parent(&self) -> &ipa::Owner<I> {
        &self.parent
    }
    pub fn report(&self) -> Report {
        let mut r = self.report;
        if self.busy && r.error.is_none() {
            r.phase = Phase::Failed;
            r.error = Some(Error::Consumed);
        }
        r
    }
    fn io(&mut self) -> Result<&mut I, Error> {
        self.parent.modem_io().map_err(|_| Error::Predecessor)
    }
    fn owned(&mut self) -> Result<bool, Error> {
        let io = self.io()?;
        io.barrier();
        Ok(io.lock_read() == 1)
    }
    fn fail(&mut self, e: Error) -> Error {
        self.report.phase = Phase::Failed;
        self.report.error.get_or_insert(e);
        e
    }
    pub(crate) fn table(io: &I) -> Result<(), Error> {
        let t = ipa::SMEM_BYTES - 4096;
        let count = u32_at(io, t + 8) as usize;
        if count != 11
            || u32_at(io, t + 32) as usize != t
            || u32_at(io, t + 36) != 4096
            || u16_at(io, t + 44) != 0xffff
            || u16_at(io, t + 46) != 0xffff
        {
            return Err(Error::Layout);
        }
        let info = t + 32 + count * 48;
        let limit = if u32_at(io, info) == u32::from_le_bytes(*b"SIII") {
            u16_at(io, info + 16)
        } else {
            512
        };
        if limit <= 497 {
            return Err(Error::Layout);
        }
        for i in 1..count {
            if i == 7 {
                continue;
            }
            let p = t + 32 + i * 48;
            let at = u32_at(io, p) as usize;
            let n = u32_at(io, p + 4) as usize;
            if at == 0 || n == 0 {
                continue;
            }
            if at > t || n > t - at || (at < PART + PART_BYTES && PART < at + n) {
                return Err(Error::Layout);
            }
        }
        Ok(())
    }
    fn scan(&mut self) -> Result<[Directory; 5], Error> {
        let n = self.report.allocations as usize;
        let io = self.io()?;
        let mut all = [dir::allocation_directory(io, 497).map_err(Error::Directory)?; 5];
        for (i, id) in [428, 478, 479, 480].into_iter().enumerate() {
            all[i + 1] = dir::allocation_directory(io, id).map_err(Error::Directory)?;
        }
        Self::table(io)?;
        let d = all[0];
        if d.route.index != 7
            || d.route.offset != PART
            || d.route.size != PART_BYTES
            || d.route.cacheline != 32
            || d.free_uncached != FREES[n]
            || d.free_cached != PART_BYTES
            || d.item
                != Some(Item {
                    offset: PART + 48,
                    size: 8192,
                })
        {
            return Err(Error::Layout);
        }
        for i in 1..5 {
            if all[i].route != d.route
                || all[i].free_uncached != d.free_uncached
                || all[i].free_cached != d.free_cached
                || all[i].item != if i <= n { Some(ITEMS[i - 1].1) } else { None }
            {
                return Err(Error::Layout);
            }
        }
        // Exact publicized header fields, retaining Linux's unused reserved
        // word untouched. In particular item497's header/payload is read-only.
        if u32_at(io, PART + 32) != ((497 << 16) | 0xa5a5)
            || u32_at(io, PART + 36) != 8192
            || u32_at(io, PART + 40) != 0
        {
            return Err(Error::Layout);
        }
        for i in 0..n {
            if u32_at(io, HEADERS[i]) != ((u32::from(ITEMS[i].0) << 16) | 0xa5a5)
                || u32_at(io, HEADERS[i] + 4) != ((ITEMS[i].1.size + 7) & !7) as u32
                || u32_at(io, HEADERS[i] + 8) != if i == 0 { 4 } else { 0 }
            {
                return Err(Error::Layout);
            }
        }
        Ok(all)
    }
    fn stable(&mut self) -> Result<(), Error> {
        if !self.owned()? {
            return Err(Error::LockLost);
        }
        let a = self.scan()?;
        self.io()?.barrier();
        if !self.owned()? {
            return Err(Error::LockLost);
        }
        if a != self.scan()? {
            return Err(Error::Unstable);
        }
        Ok(())
    }
    fn store(&mut self, r: Store) -> Result<(), Error> {
        self.payload
            .guard(self.generation)
            .map_err(|_| Error::Guard)?;
        if !self.owned()? {
            return Err(Error::LockLost);
        }
        self.report.last_offset = r.offset;
        self.report.last_value = r.value;
        self.report.submitted += 1; // consume before potentially uncertain effect
        self.payload.store(&r).map_err(|_| Error::Store)?;
        self.io()?.barrier();
        if !self.owned()? {
            return Err(Error::LockLost);
        }
        if u32_at(self.io()?, r.offset) != r.value {
            return Err(Error::Readback);
        }
        self.report.completed += 1;
        Ok(())
    }
    fn locked_step(&mut self) -> Result<(), Error> {
        self.stable()?;
        match self.report.phase {
            Phase::Validate => self.report.phase = Phase::Allocate,
            Phase::Allocate => {
                let n = self.report.allocations as usize;
                for (off, value) in [
                    (0, (u32::from(ITEMS[n].0) << 16) | 0xa5a5),
                    (4, ((ITEMS[n].1.size + 7) & !7) as u32),
                    (8, if n == 0 { 4 } else { 0 }),
                ] {
                    self.store(Store {
                        offset: HEADERS[n] + off,
                        value,
                        kind: Kind::Header,
                    })?;
                }
                self.io()?.barrier(); // publish only after complete header/readback
                self.store(Store {
                    offset: PART + 12,
                    value: FREES[n + 1] as u32,
                    kind: Kind::Cursor,
                })?;
                self.report.allocations += 1;
                self.stable()?;
                if self.report.allocations == 3 {
                    self.report.phase = Phase::Zero;
                }
            }
            Phase::Zero | Phase::Verify => {
                let i = ITEMS[self.item].1;
                let end = (self.at + CHUNK).min(i.size);
                for at in (self.at..end).step_by(4) {
                    if self.report.phase == Phase::Zero {
                        self.store(Store {
                            offset: i.offset + at,
                            value: 0,
                            kind: Kind::Payload,
                        })?;
                        self.report.zeroed += 4;
                    } else {
                        if !self.owned()? {
                            return Err(Error::LockLost);
                        }
                        if u32_at(self.io()?, i.offset + at) != 0 {
                            return Err(Error::Readback);
                        }
                        self.report.verified += 4;
                    }
                }
                self.at = end;
                if end == i.size {
                    self.item += 1;
                    self.at = 0;
                    if self.item == ITEMS.len() {
                        self.item = 0;
                        self.report.phase = if self.report.phase == Phase::Zero {
                            Phase::Verify
                        } else {
                            Phase::Prepared
                        };
                    }
                }
            }
            _ => return Err(Error::Consumed),
        }
        self.payload
            .guard(self.generation)
            .map_err(|_| Error::Guard)?;
        self.stable()
    }
    /// One metadata allocation or <=4 KiB of bounded payload work, under the
    /// same actual shared lock retained by the predecessor. Fresh allocations
    /// only; no reuse, free, rollback, peer cursor write or notification.
    pub fn step(&mut self) -> Result<Phase, Error> {
        if self.busy || self.report.error.is_some() || self.report.phase == Phase::Prepared {
            return Err(self.fail(Error::Consumed));
        }
        self.busy = true;
        let result = (|| {
            self.payload
                .guard(self.generation)
                .map_err(|_| Error::Guard)?;
            let mut acquired = false;
            for _ in 0..ipa::LOCK_ATTEMPT_LIMIT {
                self.report.lock_attempts += 1;
                self.io()?.lock_write(1);
                if self.owned()? {
                    acquired = true;
                    self.report.locks += 1;
                    break;
                }
            }
            if !acquired {
                return Err(Error::LockBusy);
            }
            let result = self.locked_step();
            if !self.owned()? {
                return Err(Error::LockLost);
            }
            self.io()?.lock_write(0);
            self.io()?.barrier();
            self.report.releases += 1;
            result
        })();
        match result {
            Ok(()) => {
                self.busy = false;
                Ok(self.report.phase)
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    pub fn take_prepared(&mut self) -> Result<Prepared, Error> {
        if self.busy
            || self.taken
            || self.report.error.is_some()
            || self.report.phase != Phase::Prepared
        {
            return Err(self.fail(Error::Consumed));
        }
        self.taken = true;
        Ok(Prepared {
            generation: self.generation,
        })
    }
    pub(crate) fn claim_smp2p(&mut self, p: Prepared) -> Result<u64, Error> {
        if self.activation_claimed
            || self.busy
            || !self.taken
            || self.report.error.is_some()
            || self.report.phase != Phase::Prepared
            || p.generation != self.generation
        {
            return Err(self.fail(Error::Consumed));
        }
        self.activation_claimed = true;
        Ok(self.generation)
    }
    pub(crate) fn smp2p_parts(&mut self) -> Result<(&mut I, &mut P), Error> {
        if !self.activation_claimed
            || self.busy
            || self.report.error.is_some()
            || self.report.phase != Phase::Prepared
        {
            return Err(Error::Consumed);
        }
        let io = self.parent.modem_io().map_err(|_| Error::Predecessor)?;
        Ok((io, &mut self.payload))
    }
}
