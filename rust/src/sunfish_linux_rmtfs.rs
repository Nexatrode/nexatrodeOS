//! The selected Linux Sunfish RMTFS reservation, separate from the downstream
//! storage/heap profile. Share this single reserved buffer before MPSS starts;
//! allocation requests only report its address and never issue another SMC.
//! The platform must retain the physical mapping, scratch and verified EFS
//! shadows across modem startup. This module has no PAS or transport capability.
#![forbid(unsafe_code)]

use crate::qcom_modem_handlers::{Disk, Fault, Persistence};
use crate::qcom_modem_storage::RAM_IO_BYTES as IO_BYTES;
use crate::qcom_modem_ram_storage::RamShadow;
use crate::qcom_modem_services::Partition;
use crate::qcom_modem_storage::{self as storage, Storage};

pub const BUFFER_PA: u64 = 0xfde01000;
pub const BUFFER_BYTES: u32 = 0x200000;
pub const PAGE: u32 = 4096;
pub const CLIENT_ID: u32 = 1;
pub const ARG_BYTES: usize = 320;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mapping {
    pub virtual_address: u64,
    pub physical_address: u64,
    pub bytes: u32,
    pub writable: bool,
    pub executable: bool,
    pub cached: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mappings {
    pub buffer: Mapping,
    pub scratch: Mapping,
}
impl Mappings {
    /// Geometry only. The native factory independently proves actual frame
    /// ownership, attributes, guard holes, exclusions and boot provenance.
    pub fn validate(self) -> Result<Self, Fault> {
        for m in [self.buffer, self.scratch] {
            if m.virtual_address == 0
                || m.virtual_address % u64::from(PAGE) != 0
                || m.physical_address == 0
                || m.physical_address % u64::from(PAGE) != 0
                || m.bytes == 0
                || !m.writable
                || m.executable
                || m.cached
                || m.virtual_address.checked_add(u64::from(m.bytes)).is_none()
                || m.physical_address.checked_add(u64::from(m.bytes)).is_none()
            {
                return Err(Fault::Failed);
            }
        }
        let a = self.buffer;
        let b = self.scratch;
        let overlaps =
            |x: u64, n: u32, y: u64, k: u32| x < y + u64::from(k) && y < x + u64::from(n);
        if a.physical_address != BUFFER_PA
            || a.bytes != BUFFER_BYTES
            || b.bytes != PAGE
            || overlaps(a.virtual_address, a.bytes, b.virtual_address, b.bytes)
            || overlaps(a.physical_address, a.bytes, b.physical_address, b.bytes)
        {
            return Err(Fault::Failed);
        }
        Ok(self)
    }
}

/// Exact selected qcom_scm_assign_mem serialization: source list at +0,
/// range at +64, two 24-byte destinations at +128; zero padding/context.
/// Seven extra SCM64 arguments occupy a separate +256 slot. All lengths are
/// bytes, not entry counts. No caller-selected VMIDs or permissions.
pub struct Call {
    words: [u64; 8],
    bytes: [u8; ARG_BYTES],
}
impl Call {
    pub fn new(mappings: Mappings) -> Result<Self, Fault> {
        let pa = mappings.validate()?.scratch.physical_address;
        let mut bytes = [0; ARG_BYTES];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes()); // HLOS
        bytes[64..72].copy_from_slice(&BUFFER_PA.to_le_bytes());
        bytes[72..80].copy_from_slice(&u64::from(BUFFER_BYTES).to_le_bytes());
        for (at, vmid) in [(128, 3u32), (152, 15u32)] {
            bytes[at..at + 4].copy_from_slice(&vmid.to_le_bytes());
            bytes[at + 4..at + 8].copy_from_slice(&6u32.to_le_bytes()); // RW
        }
        for (at, value) in [(256, 4u64), (264, pa + 128), (272, 48)] {
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        Ok(Self {
            words: [0x42000c16, 0x1117, pa + 64, 16, pa, pa + 256, 0, 0],
            bytes,
        })
    }
    pub fn words(&self) -> &[u64; 8] {
        &self.words
    }
    pub fn bytes(&self) -> &[u8; ARG_BYTES] {
        &self.bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub generation: u64,
    pub identity: u64,
    pub ready: bool,
    pub mappings: Mappings,
    /// Actual stopped/reset lifecycle, not absence of QRTR discovery.
    pub modem_stopped: bool,
    /// Reviewed fresh-boot HLOS source convention, checked only at attach.
    /// Neither a readable mapping nor Linux's software VMID mask proves it.
    pub initial_hlos: bool,
}
/// Serialized trusted boundary; each copy/zero <=4 KiB. No cached aliases.
/// Failed proves no effect for that operation; Uncertain forbids retries.
/// stage synchronizes and verifies all scratch bytes; invoke uses the exact
/// call, with any source-matched monitor retry policy inside that boundary.
/// complete orders secure completion before publishing the shared address.
/// Resources stay retained after errors/unwind; Drop performs no transition.
pub trait Io {
    fn observe(&mut self) -> Result<Observation, Fault>;
    fn zero(&mut self, offset: u32, bytes: usize) -> Result<(), Fault>;
    fn stage(&mut self, call: &Call) -> Result<(), Fault>;
    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault>;
    fn complete(&mut self) -> Result<(), Fault>;
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Fault>;
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Fault>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Preparing,
    Prepared,
    Unknown,
    Shared,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShareCheckpoint {
    StorageValidationStart,
    StorageValidationDone,
    PoolValidationStart,
    PoolValidationDone,
    CallConstructionStart,
    CallConstructionDone,
    ScratchStageStart,
    ScratchStageDone,
    PostStageValidationStart,
    PostStageValidationDone,
    StorageFinalValidationStart,
    StorageFinalValidationDone,
    ReadyBeforeScm,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Attach,
    Lifecycle,
    Stopped,
    Preparation,
    Stage,
    Monitor,
    Completion,
    Access,
    Storage,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub phase: Phase,
    pub zeroed_pages: u32,
    pub assignments_attempted: u32,
    pub error: Option<Error>,
}
pub struct Pool<I: Io> {
    io: I,
    generation: u64,
    identity: u64,
    mappings: Mappings,
    status: Status,
    busy: bool,
    #[cfg(feature = "sunfish-linux-mpss-auth")]
    auth_submitted: bool,
}
impl<I: Io> Pool<I> {
    /// Retain the provider even on invalid configuration. No memory writes
    /// or monitor calls occur during attachment.
    pub fn attach(generation: u64, mappings: Mappings, io: I) -> Self {
        let mut s = Self {
            io,
            generation,
            identity: 0,
            mappings,
            status: Status {
                phase: Phase::Unknown,
                zeroed_pages: 0,
                assignments_attempted: 0,
                error: Some(Error::Attach),
            },
            busy: true,
            #[cfg(feature = "sunfish-linux-mpss-auth")]
            auth_submitted: false,
        };
        if generation != 0 && mappings.validate().is_ok() {
            if let Ok(o) = s.io.observe() {
                if s.valid(o) && o.modem_stopped && o.initial_hlos {
                    s.identity = o.identity;
                    s.status.phase = Phase::Preparing;
                    s.status.error = None;
                    s.busy = false;
                }
            }
        }
        s
    }
    fn valid(&self, o: Observation) -> bool {
        o.generation == self.generation && o.identity != 0 && o.ready && o.mappings == self.mappings
    }
    fn stop(&mut self, e: Error) -> Fault {
        self.status.error = Some(e);
        self.status.phase = Phase::Unknown;
        Fault::Uncertain
    }
    fn check(&mut self, stopped: bool) -> Result<(), Fault> {
        if self.status.error.is_some() {
            return Err(Fault::Uncertain);
        }
        if self.busy {
            return Err(self.stop(Error::Stopped));
        }
        self.busy = true;
        let o = self.io.observe().map_err(|_| self.stop(Error::Lifecycle))?;
        if !self.valid(o) || o.identity != self.identity || (stopped && !o.modem_stopped) {
            return Err(self.stop(Error::Lifecycle));
        }
        self.busy = false;
        Ok(())
    }
    pub fn status(&self) -> Status {
        let mut s = self.status;
        if self.busy && s.error.is_none() {
            s.error = Some(Error::Stopped);
            s.phase = Phase::Unknown;
        }
        s
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Native confidentiality hardening before sharing, not a Linux driver
    /// requirement. One page per step, verified by the provider before success.
    pub fn prepare_step(&mut self) -> Result<bool, Fault> {
        self.check(true)?;
        if self.status.phase == Phase::Prepared {
            return Ok(true);
        }
        if self.status.phase != Phase::Preparing {
            return Err(Fault::Failed);
        }
        self.busy = true;
        self.io
            .zero(self.status.zeroed_pages * PAGE, PAGE as usize)
            .map_err(|_| self.stop(Error::Preparation))?;
        self.busy = false;
        self.check(true)?;
        self.status.zeroed_pages += 1;
        if self.status.zeroed_pages == BUFFER_BYTES / PAGE {
            self.status.phase = Phase::Prepared;
        }
        Ok(self.status.phase == Phase::Prepared)
    }
    /// One ownership transition for the complete DT reservation before PAS4.
    /// An ambiguous or unsuccessful call is terminal; no reclaim/retry API.
    pub fn share(&mut self) -> Result<(), Fault> {
        self.check(true)?;
        if self.status.phase != Phase::Prepared || self.status.assignments_attempted != 0 {
            return Err(Fault::Failed);
        }
        let call = Call::new(self.mappings)?;
        self.busy = true;
        self.io.stage(&call).map_err(|_| self.stop(Error::Stage))?;
        self.busy = false;
        self.check(true)?;
        self.busy = true;
        self.status.phase = Phase::Unknown;
        self.status.assignments_attempted = 1;
        let reply = self
            .io
            .invoke(&call)
            .map_err(|_| self.stop(Error::Monitor))?;
        if reply[0] != 0 || reply[1] != 0 {
            return Err(self.stop(Error::Monitor));
        }
        self.io
            .complete()
            .map_err(|_| self.stop(Error::Completion))?;
        self.busy = false;
        self.check(true)?;
        self.status.phase = Phase::Shared;
        Ok(())
    }
    /// Validate and stage the exact ownership request without invoking SCM.
    /// This is a diagnostic preflight only; the retained staged call must not
    /// be followed by `share()` in the same boot.
    pub fn preflight_share_with(
        &mut self,
        mut checkpoint: impl FnMut(ShareCheckpoint),
    ) -> Result<(), Fault> {
        checkpoint(ShareCheckpoint::PoolValidationStart);
        self.check(true)?;
        checkpoint(ShareCheckpoint::PoolValidationDone);
        if self.status.phase != Phase::Prepared || self.status.assignments_attempted != 0 {
            return Err(Fault::Failed);
        }
        checkpoint(ShareCheckpoint::CallConstructionStart);
        let call = Call::new(self.mappings)?;
        checkpoint(ShareCheckpoint::CallConstructionDone);
        self.busy = true;
        checkpoint(ShareCheckpoint::ScratchStageStart);
        self.io
            .stage(&call)
            .map_err(|_| self.stop(Error::Stage))?;
        self.busy = false;
        checkpoint(ShareCheckpoint::ScratchStageDone);
        checkpoint(ShareCheckpoint::PostStageValidationStart);
        self.check(true)?;
        checkpoint(ShareCheckpoint::PostStageValidationDone);
        Ok(())
    }
    pub fn shared(&mut self) -> Result<(), Fault> {
        self.check(false)?;
        if self.status.phase != Phase::Shared {
            return Err(Fault::Failed);
        }
        Ok(())
    }
    /// Same constant reservation is returned on repeated allocation requests.
    /// This operation grants no new ownership and does not zero shared bytes.
    pub fn allocation(&mut self, bytes: u32) -> Result<u64, Fault> {
        self.shared()?;
        if bytes == 0 || bytes > BUFFER_BYTES {
            return Err(Fault::Failed);
        }
        Ok(BUFFER_PA)
    }
    fn span(&mut self, physical: u64, bytes: usize) -> Result<u32, Fault> {
        self.shared()?;
        if bytes == 0 || bytes > IO_BYTES {
            return Err(Fault::Failed);
        }
        let at = physical.checked_sub(BUFFER_PA).ok_or(Fault::Failed)?;
        if at
            .checked_add(bytes as u64)
            .is_none_or(|e| e > u64::from(BUFFER_BYTES))
        {
            return Err(Fault::Failed);
        }
        Ok(at as u32)
    }
    fn read(&mut self, physical: u64, out: &mut [u8]) -> Result<(), Fault> {
        let at = self.span(physical, out.len())?;
        self.busy = true;
        self.io
            .read(at, out)
            .map_err(|_| self.stop(Error::Access))?;
        self.busy = false;
        self.check(false)
    }
    fn write(&mut self, physical: u64, bytes: &[u8]) -> Result<(), Fault> {
        let at = self.span(physical, bytes.len())?;
        self.busy = true;
        self.io
            .write(at, bytes)
            .map_err(|_| self.stop(Error::Access))?;
        self.busy = false;
        self.check(false)
    }
}

#[cfg(feature = "sunfish-linux-mpss-auth")]
impl<I: crate::sunfish_mpss_linux_boot::AuthIo> Pool<I> {
    fn pre_auth_shared(&mut self, generation: u64) -> Result<(), Fault> {
        self.check(true)?;
        if generation != self.generation
            || self.auth_submitted
            || self.status.phase != Phase::Shared
            || self.status.assignments_attempted != 1
        {
            return Err(self.stop(Error::Lifecycle));
        }
        Ok(())
    }
    fn runtime_shared(&mut self) -> Result<(), Fault> {
        self.shared()?;
        if !self.auth_submitted {
            return Err(self.stop(Error::Lifecycle));
        }
        self.busy = true;
        let o = self.io.observe().map_err(|_| self.stop(Error::Lifecycle))?;
        if !self.valid(o) || o.identity != self.identity || o.modem_stopped {
            return Err(self.stop(Error::Lifecycle));
        }
        self.busy = false;
        Ok(())
    }
    fn submit_mpss_auth(
        &mut self,
        generation: u64,
        request: &crate::sunfish_mpss_linux_loader::Request,
    ) -> Result<(), Fault> {
        self.pre_auth_shared(generation)?;
        if generation != self.generation
            || self.status.phase != Phase::Shared
            || self.status.assignments_attempted != 1
            || self.auth_submitted
            || request.kind() != crate::sunfish_mpss_linux_loader::Kind::AuthAndReset
        {
            return Err(self.stop(Error::Lifecycle));
        }
        // Consume before the trusted gate can change the reset lifecycle.
        // Even an error/unwind cannot re-zero, re-share or retry this pool.
        self.auth_submitted = true;
        self.busy = true;
        self.io
            .submit_mpss_auth(generation, request)
            .map_err(|_| self.stop(Error::Lifecycle))?;
        self.busy = false;
        self.runtime_shared()
    }
}

/// Actual retained verified RAM-shadow provider plus the single shared pool.
/// No dummy heap or persistent storage capability is needed for this profile.
/// The QRTR server must validate handles/routes and prevalidate entire iovecs
/// before dispatching these bounded sector chunks.
pub struct Resources<'a, I: Io> {
    pool: Pool<I>,
    storage: Storage<RamShadow<'a>>,
    storage_identity: u64,
    failed: bool,
    busy: bool,
}

#[cfg(feature = "sunfish-linux-mpss-auth")]
impl<I: crate::sunfish_mpss_linux_boot::AuthIo> Resources<'_, I> {
    pub(crate) fn pre_auth_shared(&mut self, generation: u64) -> Result<(), Fault> {
        self.run(|pool, _| pool.pre_auth_shared(generation))
    }
    pub(crate) fn submit_mpss_auth(
        &mut self,
        generation: u64,
        request: &crate::sunfish_mpss_linux_loader::Request,
    ) -> Result<(), Fault> {
        self.run(|pool, _| pool.submit_mpss_auth(generation, request))
    }
    pub(crate) fn runtime_shared(&mut self) -> Result<(), Fault> {
        self.run(|pool, _| pool.runtime_shared())
    }
}
impl<'a, I: Io> Resources<'a, I> {
    pub fn attach(pool: Pool<I>, storage: Storage<RamShadow<'a>>) -> Self {
        let mut s = Self {
            pool,
            storage,
            storage_identity: 0,
            failed: true,
            busy: true,
        };
        if s.pool.status().error.is_none()
            && s.pool.generation() == s.storage.configuration().generation
            && s.storage.persistence() == Persistence::RamShadow
            && s.storage.status().dirty == [false; 4]
        {
            if let Ok(identity) = s.storage.observe() {
                s.storage_identity = identity;
                s.failed = false;
                s.busy = false;
            }
        }
        s
    }
    pub fn status(&self) -> (Status, storage::Status, bool) {
        (
            self.pool.status(),
            self.storage.status(),
            self.failed || self.busy,
        )
    }
    pub fn configuration(&self) -> storage::Configuration {
        self.storage.configuration()
    }
    fn check(&mut self) -> Result<(), Fault> {
        if self.failed || self.busy {
            self.failed = true;
            return Err(Fault::Uncertain);
        }
        self.busy = true;
        if self.storage.observe() != Ok(self.storage_identity) {
            self.failed = true;
            return Err(Fault::Uncertain);
        }
        self.busy = false;
        Ok(())
    }
    fn check_with(
        &mut self,
        start: ShareCheckpoint,
        done: ShareCheckpoint,
        checkpoint: &mut impl FnMut(ShareCheckpoint),
    ) -> Result<(), Fault> {
        if self.failed || self.busy {
            self.failed = true;
            return Err(Fault::Uncertain);
        }
        self.busy = true;
        checkpoint(start);
        if self.storage.observe() != Ok(self.storage_identity) {
            self.failed = true;
            return Err(Fault::Uncertain);
        }
        self.busy = false;
        checkpoint(done);
        Ok(())
    }
    fn run<T>(
        &mut self,
        f: impl FnOnce(&mut Pool<I>, &mut Storage<RamShadow<'a>>) -> Result<T, Fault>,
    ) -> Result<T, Fault> {
        self.check()?;
        self.busy = true;
        let result = f(&mut self.pool, &mut self.storage);
        self.busy = false;
        if matches!(result, Err(Fault::Uncertain)) {
            self.failed = true;
            return result;
        }
        self.check()?;
        result
    }
    pub fn prepare_step(&mut self) -> Result<bool, Fault> {
        self.run(|p, _| p.prepare_step())
    }
    pub fn share(&mut self) -> Result<(), Fault> {
        self.run(|p, _| p.share())
    }
    /// Diagnostic-only share preparation: perform owner checks and stage the
    /// call bytes, but stop before the pool invokes its secure monitor.
    pub fn preflight_share_with(
        &mut self,
        mut checkpoint: impl FnMut(ShareCheckpoint),
    ) -> Result<(), Fault> {
        self.check_with(
            ShareCheckpoint::StorageValidationStart,
            ShareCheckpoint::StorageValidationDone,
            &mut checkpoint,
        )?;
        self.busy = true;
        let result = self.pool.preflight_share_with(&mut checkpoint);
        self.busy = false;
        if matches!(result, Err(Fault::Uncertain)) {
            self.failed = true;
            return result;
        }
        self.check_with(
            ShareCheckpoint::StorageFinalValidationStart,
            ShareCheckpoint::StorageFinalValidationDone,
            &mut checkpoint,
        )?;
        result?;
        checkpoint(ShareCheckpoint::ReadyBeforeScm);
        Ok(())
    }
    pub fn shared(&mut self) -> Result<(), Fault> {
        self.run(|p, _| p.shared())
    }
    pub fn allocation(&mut self, bytes: u32) -> Result<u64, Fault> {
        self.run(|p, _| p.allocation(bytes))
    }
    pub fn disk(&mut self, partition: Partition) -> Result<Disk, Fault> {
        self.run(|p, d| {
            p.shared()?;
            d.disk(partition)
        })
    }
    pub fn transfer(
        &mut self,
        partition: Partition,
        sector: u32,
        physical: u64,
        sectors: u32,
        write: bool,
    ) -> Result<(), Fault> {
        self.run(|p, d| {
            let bytes = sectors
                .checked_mul(512)
                .filter(|n| *n != 0 && *n as usize <= IO_BYTES)
                .ok_or(Fault::Failed)? as usize;
            let at = u64::from(sector) * 512;
            let disk = d.disk(partition)?;
            if physical % 512 != 0 || at.checked_add(bytes as u64).is_none_or(|e| e > disk.bytes) {
                return Err(Fault::Failed);
            }
            p.span(physical, bytes)?;
            let mut buffer = [0; IO_BYTES];
            if write {
                p.read(physical, &mut buffer[..bytes])?;
                d.write(partition, at, &buffer[..bytes])?;
            } else {
                d.read(partition, at, &mut buffer[..bytes])?;
                p.write(physical, &buffer[..bytes])?;
            }
            Ok(())
        })
    }
    /// Acknowledges retained RAM only, like the selected rmtfs -r run.
    pub fn sync(&mut self, partition: Partition) -> Result<(), Fault> {
        self.run(|p, d| {
            p.shared()?;
            d.flush(partition)
        })
    }
}
