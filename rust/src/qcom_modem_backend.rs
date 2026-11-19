//! Pair validated disk capabilities and SCM pools into the service Backend.
//! No native provider is instantiated here. Every action checks both lifetimes.
#![forbid(unsafe_code)]
use crate::qcom_modem_handlers::{self as h, Disk, Fault, Grant, Pool};
use crate::qcom_modem_pools::{self as pools, Pools, Receipt};
use crate::qcom_modem_services::Partition;
use crate::qcom_modem_storage::{self as storage, Storage};
use core::sync::atomic::{AtomicU64, Ordering};
static NEXT_BACKEND: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Attach,
    Stopped,
    Pool,
    Storage,
    Identity,
    Uncertain,
}
pub struct Backend<P: pools::Io, D: storage::Io> {
    pools: Pools<P>,
    storage: Storage<D>,
    configuration: h::Configuration,
    identity: u64,
    pool_identity: u64,
    storage_identity: u64,
    error: Option<Error>,
    busy: bool,
}
impl<P: pools::Io, D: storage::Io> Backend<P, D> {
    /// Always retain both resources, even on invalid configuration. No grant,
    /// disk data, rollback or cleanup is performed at attachment.
    pub fn attach(c: h::Configuration, pools: Pools<P>, storage: Storage<D>) -> Self {
        let identity = NEXT_BACKEND
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .unwrap_or(0);
        let mut s = Self {
            pools,
            storage,
            configuration: c,
            identity,
            pool_identity: 0,
            storage_identity: 0,
            error: Some(Error::Attach),
            busy: true,
        };
        let result = (|| {
            let dc = s.storage.configuration();
            if identity == 0
                || c.generation != s.pools.generation()
                || c.generation != dc.generation
                || c.peer_node != 0
                || c.local_node != 1
                || c.storage != pools::STORAGE
                || c.heap != pools::HEAP
                || c.disks != dc.bindings.map(|b| b.disk)
                || s.storage.status().dirty != [false; 4]
                || match s.storage.persistence() {
                    h::Persistence::Durable => h::Profile::validate(c, &[]),
                    h::Persistence::RamShadow => h::Profile::validate_ram_shadow(c, &[]),
                }.is_err()
            {
                return Err(Fault::Failed);
            }
            let (pi, owners) = s.pools.observe()?;
            if owners != [h::Ownership::Host; 2] {
                return Err(Fault::Failed);
            }
            s.pool_identity = pi;
            s.storage_identity = s.storage.observe()?;
            Ok(())
        })();
        if result.is_ok() {
            s.error = None;
            s.busy = false;
        }
        s
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn storage_status(&self) -> storage::Status {
        self.storage.status()
    }
    pub fn pool_status(&self) -> pools::Status {
        self.pools.status()
    }
    pub fn configuration(&self) -> h::Configuration {
        self.configuration
    }
    fn stop(&mut self, e: Error) -> Fault {
        self.error = Some(e);
        Fault::Uncertain
    }
    fn check(&mut self) -> Result<h::Observation, Fault> {
        if self.error.is_some() {
            return Err(Fault::Uncertain);
        }
        if self.busy {
            return Err(self.stop(Error::Stopped));
        }
        self.busy = true;
        let (pi, owners) = self.pools.observe().map_err(|_| self.stop(Error::Pool))?;
        let di = self
            .storage
            .observe()
            .map_err(|_| self.stop(Error::Storage))?;
        if pi != self.pool_identity || di != self.storage_identity {
            return Err(self.stop(Error::Identity));
        }
        self.busy = false;
        Ok(h::Observation {
            generation: self.configuration.generation,
            identity: self.identity,
            ready: true,
            storage: owners[0],
            heap: owners[1],
        })
    }
    fn run<R>(
        &mut self,
        f: impl FnOnce(&mut Pools<P>, &mut Storage<D>) -> Result<R, Fault>,
    ) -> Result<R, Fault> {
        self.check()?;
        self.busy = true;
        let r = f(&mut self.pools, &mut self.storage);
        self.busy = false;
        match r {
            Ok(value) => {
                self.check()?;
                Ok(value)
            }
            Err(Fault::Failed) => {
                self.check()?;
                Err(Fault::Failed)
            }
            Err(Fault::Uncertain) => Err(self.stop(Error::Uncertain)),
        }
    }
}
impl<P: pools::Io, D: storage::Io> h::Backend for Backend<P, D> {
    type Receipt = Receipt;
    fn persistence(&self) -> h::Persistence {
        self.storage.persistence()
    }
    fn observe(&mut self) -> Result<h::Observation, Fault> {
        self.check()
    }
    fn disk(&mut self, p: Partition) -> Result<Disk, Fault> {
        self.run(|_, d| d.disk(p))
    }
    fn zero(&mut self, p: Pool, at: u32, n: usize) -> Result<(), Fault> {
        self.run(|m, _| m.zero(p, at, n))
    }
    fn grant(&mut self, g: Grant) -> Result<Receipt, Fault> {
        self.run(|m, _| m.grant(g))
    }
    fn info(&mut self, r: &Receipt) -> Result<Grant, Fault> {
        self.run(|m, _| m.info(r))
    }
    fn reclaim(&mut self, r: &Receipt) -> Result<(), Fault> {
        self.run(|m, _| m.reclaim(r))
    }
    fn disk_read(&mut self, p: Partition, at: u64, out: &mut [u8]) -> Result<(), Fault> {
        self.run(|_, d| d.read(p, at, out))
    }
    fn disk_write(&mut self, p: Partition, at: u64, b: &[u8]) -> Result<(), Fault> {
        self.run(|_, d| d.write(p, at, b))
    }
    fn sync(&mut self, p: Partition) -> Result<(), Fault> {
        self.run(|_, d| d.flush(p))
    }
    fn pool_read(&mut self, r: &Receipt, at: u32, out: &mut [u8]) -> Result<(), Fault> {
        self.run(|m, _| m.read(r, at, out))
    }
    fn pool_write(&mut self, r: &Receipt, at: u32, b: &[u8]) -> Result<(), Fault> {
        self.run(|m, _| m.write(r, at, b))
    }
}
