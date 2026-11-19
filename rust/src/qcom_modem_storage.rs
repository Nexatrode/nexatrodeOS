//! Bounded, capability-bound backing storage for the four modem path enums.
//! Profile/hash labels are expected trusted evidence, not authentication by
//! themselves. Io must verify the device/data binding before reporting ready.
#![forbid(unsafe_code)]
use crate::qcom_modem_handlers::{Disk, Fault, IO_BYTES, Persistence};
/// Larger transfers are limited to the explicitly volatile RAM-shadow mode.
pub const RAM_IO_BYTES: usize = 128 * 1024;
use crate::qcom_modem_services::Partition;

pub const SECTOR_BYTES: u64 = 512;
pub const MAX_PARTITION_BYTES: u64 = 64 * 1024 * 1024;
pub type Digest = [u8; 32];
pub const PARTITIONS: [Partition; 4] = [
    Partition::ModemFs1,
    Partition::ModemFs2,
    Partition::ModemFsg,
    Partition::ModemFsc,
];
pub fn index(p: Partition) -> usize {
    match p {
        Partition::ModemFs1 => 0,
        Partition::ModemFs2 => 1,
        Partition::ModemFsg => 2,
        Partition::ModemFsc => 3,
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Binding {
    pub partition: Partition,
    /// Provider capability identity, not a peer-supplied pathname or block ID.
    pub object: u64,
    /// Digest of the authorized source/checkpoint, not mutable live contents.
    pub source: Digest,
    pub disk: Disk,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Configuration {
    pub generation: u64,
    pub device: Digest,
    pub bindings: [Binding; 4],
}
pub struct Profile(Configuration, Persistence);
impl Profile {
    pub fn validate(c: Configuration) -> Result<Self, Fault> {
        Self::validate_mode(c, Persistence::Durable)
    }
    /// Explicit volatile mode, to be paired with a verified RAM shadow Io.
    /// This cannot attach a default durable provider or claim persistence.
    pub fn validate_ram_shadow(c: Configuration) -> Result<Self, Fault> {
        Self::validate_mode(c, Persistence::RamShadow)
    }
    fn validate_mode(c: Configuration, mode: Persistence) -> Result<Self, Fault> {
        if c.generation == 0 || c.device == [0; 32] {
            return Err(Fault::Failed);
        }
        for (i, b) in c.bindings.iter().enumerate() {
            if b.partition != PARTITIONS[i]
                || b.object == 0
                || b.source == [0; 32]
                || b.disk.bytes == 0
                || b.disk.bytes > MAX_PARTITION_BYTES
                || b.disk.bytes % SECTOR_BYTES != 0
                || match mode {
                    Persistence::Durable => b.disk.writable && !b.disk.durable,
                    Persistence::RamShadow => !b.disk.writable || b.disk.durable,
                }
                || c.bindings[..i].iter().any(|a| a.object == b.object)
            {
                return Err(Fault::Failed);
            }
        }
        Ok(Self(c, mode))
    }
    pub fn configuration(&self) -> Configuration {
        self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub configuration: Configuration,
    pub identity: u64,
    /// Verified origin/device binding, exclusive media authority, live provider
    /// generation and real geometry/permissions/flush support. Never inferred
    /// from parsing metadata, an untrusted file path or an empty replacement.
    pub ready: bool,
}
/// Serialized provider boundary; one data call <=4096 bytes, no hidden retries.
/// Failed writes prove no effect; ambiguous/short writes must not be retried.
/// flush success confirms the selected persistence contract (not merely a
/// queued write). RAM-shadow mode is volatile and must be selected by both
/// profile and provider. A firmware/kernel call may still lack a wall-time bound.
/// Retain capabilities on faults; Drop is never a flush/ownership acknowledgement.
pub trait Io {
    fn persistence(&self) -> Persistence {
        Persistence::Durable
    }
    fn observe(&mut self) -> Result<Observation, Fault>;
    fn read(&mut self, partition: Partition, offset: u64, out: &mut [u8]) -> Result<usize, Fault>;
    fn write(&mut self, partition: Partition, offset: u64, bytes: &[u8]) -> Result<usize, Fault>;
    fn flush(&mut self, partition: Partition) -> Result<(), Fault>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Attach,
    Stopped,
    Lifecycle,
    Uncertain,
    ShortWrite,
    Flush,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub error: Option<Error>,
    pub dirty: [bool; 4],
}
pub struct Storage<T: Io> {
    io: T,
    configuration: Configuration,
    persistence: Persistence,
    identity: u64,
    status: Status,
    busy: bool,
}
impl<T: Io> Storage<T> {
    /// Own and retain the provider even if attachment fails; no data IO here.
    pub fn attach(profile: Profile, io: T) -> Self {
        let mut s = Self {
            io,
            configuration: profile.0,
            persistence: profile.1,
            identity: 0,
            status: Status {
                error: Some(Error::Attach),
                dirty: [false; 4],
            },
            busy: true,
        };
        if let Ok(o) = s.io.observe() {
            if o.ready && o.identity != 0 && o.configuration == s.configuration
                && s.io.persistence() == s.persistence
            {
                s.identity = o.identity;
                s.status.error = None;
                s.busy = false;
            }
        }
        s
    }
    pub fn configuration(&self) -> Configuration {
        self.configuration
    }
    pub fn persistence(&self) -> Persistence {
        self.persistence
    }
    pub fn status(&self) -> Status {
        self.status
    }
    fn stop(&mut self, e: Error) -> Fault {
        self.status.error = Some(e);
        Fault::Uncertain
    }
    pub fn observe(&mut self) -> Result<u64, Fault> {
        if self.status.error.is_some() {
            return Err(Fault::Uncertain);
        }
        if self.busy {
            return Err(self.stop(Error::Stopped));
        }
        self.busy = true;
        let o = self.io.observe().map_err(|_| self.stop(Error::Lifecycle))?;
        if !o.ready || o.identity != self.identity || o.configuration != self.configuration
            || self.io.persistence() != self.persistence
        {
            return Err(self.stop(Error::Lifecycle));
        }
        self.busy = false;
        Ok(self.identity)
    }
    pub fn disk(&mut self, p: Partition) -> Result<Disk, Fault> {
        self.observe()?;
        Ok(self.configuration.bindings[index(p)].disk)
    }
    fn bounds(&self, p: Partition, at: u64, n: usize) -> bool {
        n != 0
            && n <= if self.persistence == Persistence::RamShadow { RAM_IO_BYTES } else { IO_BYTES }
            && n as u64 % SECTOR_BYTES == 0
            && at % SECTOR_BYTES == 0
            && at
                .checked_add(n as u64)
                .is_some_and(|end| end <= self.configuration.bindings[index(p)].disk.bytes)
    }
    pub fn read(&mut self, p: Partition, at: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.observe()?;
        if !self.bounds(p, at, out.len()) {
            return Err(Fault::Failed);
        }
        self.busy = true;
        let result = self.io.read(p, at, out);
        self.busy = false;
        match result {
            Ok(n) if n == out.len() => {
                self.observe()?;
                Ok(())
            }
            Ok(n) if n > out.len() => Err(self.stop(Error::Uncertain)),
            Ok(_) | Err(Fault::Failed) => {
                self.observe()?;
                Err(Fault::Failed)
            }
            Err(Fault::Uncertain) => Err(self.stop(Error::Uncertain)),
        }
    }
    pub fn write(&mut self, p: Partition, at: u64, bytes: &[u8]) -> Result<(), Fault> {
        self.observe()?;
        let i = index(p);
        if !self.bounds(p, at, bytes.len()) || !self.configuration.bindings[i].disk.writable {
            return Err(Fault::Failed);
        }
        let old_dirty = self.status.dirty[i];
        self.status.dirty[i] = true;
        self.busy = true;
        let result = self.io.write(p, at, bytes);
        self.busy = false;
        match result {
            Ok(n) if n == bytes.len() => {
                self.observe()?;
                Ok(())
            }
            Ok(_) => Err(self.stop(Error::ShortWrite)),
            Err(Fault::Failed) => {
                self.status.dirty[i] = old_dirty;
                self.observe()?;
                Err(Fault::Failed)
            }
            Err(Fault::Uncertain) => Err(self.stop(Error::Uncertain)),
        }
    }
    pub fn flush(&mut self, p: Partition) -> Result<(), Fault> {
        self.observe()?;
        if !self.configuration.bindings[index(p)].disk.durable
            && self.persistence != Persistence::RamShadow
        {
            return Err(Fault::Failed);
        }
        self.busy = true;
        self.io.flush(p).map_err(|_| self.stop(Error::Flush))?;
        self.busy = false;
        self.observe()?;
        self.status.dirty[index(p)] = false;
        Ok(())
    }
}
