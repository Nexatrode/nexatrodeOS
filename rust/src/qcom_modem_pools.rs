//! Fixed Sunfish service reservations and one-call SCM ownership transitions.
//! No constructor proves QHEE ownership: Io::observe must supply an explicit
//! fresh-boot HLOS/SCM64 contract. The native owner does not enable this yet.
#![forbid(unsafe_code)]

use crate::qcom_modem_handlers::{Fault, Grant, IO_BYTES, Ownership, Pool, Region};
use core::sync::atomic::{AtomicU64, Ordering};

pub const PAGE: u32 = 4096;
pub const STORAGE: Region = Region {
    base: 0xa2001000,
    bytes: 0x200000,
};
pub const HEAP: Region = Region {
    base: 0xa2401000,
    bytes: 0x500000,
};
pub const SCRATCH: Region = Region {
    base: 0xa2a01000,
    bytes: PAGE,
};
pub const VIRTUAL: [u64; 3] = [0x61001000, 0x62001000, 0x63001000];
// Range/source/destination tables occupy fixed separated slots; seven SCM
// extra arguments start at +256 so a three-destination record cannot overlap.
pub const ARG_BYTES: usize = 320;
pub const MAX_EXCLUSIONS: usize = 64;
const HLOS: u32 = 3;
const MSS_MSA: u32 = 15;
const RW: u32 = 6;
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mapping {
    pub region: Region,
    pub virtual_address: u64,
    pub writable: bool,
    pub executable: bool,
    pub cached: bool,
}
pub const MAPPINGS: [Mapping; 3] = [
    Mapping {
        region: STORAGE,
        virtual_address: VIRTUAL[0],
        writable: true,
        executable: false,
        cached: false,
    },
    Mapping {
        region: HEAP,
        virtual_address: VIRTUAL[1],
        writable: true,
        executable: false,
        cached: false,
    },
    Mapping {
        region: SCRATCH,
        virtual_address: VIRTUAL[2],
        writable: true,
        executable: false,
        cached: false,
    },
];

fn end(r: Region) -> Option<u64> {
    r.base.checked_add(u64::from(r.bytes))
}
fn overlaps(a: Region, b: Region) -> bool {
    a.base < end(b).unwrap() && b.base < end(a).unwrap()
}
fn envelope(r: Region) -> Region {
    Region {
        base: r.base - u64::from(PAGE),
        bytes: r.bytes + 2 * PAGE,
    }
}
fn index(p: Pool) -> usize {
    if p == Pool::Storage { 0 } else { 1 }
}

/// Build validator checks actual CapDL frames, mappings, unmapped guards,
/// cached-live/kernel DT exclusions and loader placement. This token checks
/// the injected runtime geometry against that exact fixed profile; it cannot
/// replace the allocator/boot contract supplied by Io.
pub struct Reservations {
    generation: u64,
    peer_node: u32,
}
impl Reservations {
    pub fn validate(
        generation: u64,
        peer_node: u32,
        mappings: [Mapping; 3],
        excluded: &[Region],
    ) -> Result<Self, Fault> {
        if generation == 0
            || peer_node != 0
            || mappings != MAPPINGS
            || excluded.len() > MAX_EXCLUSIONS
        {
            return Err(Fault::Failed);
        }
        for r in excluded {
            if r.bytes == 0
                || end(*r).is_none()
                || MAPPINGS.iter().any(|m| overlaps(envelope(m.region), *r))
            {
                return Err(Fault::Failed);
            }
        }
        Ok(Self {
            generation,
            peer_node,
        })
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub generation: u64,
    pub identity: u64,
    pub ready: bool,
    pub mappings: [Mapping; 3],
    /// Exclusive allocator reservation, no aliases and no other CPU/driver
    /// users; exact SCM64 ABI and zero-return ownership contract are reviewed.
    /// Availability or Linux's locally updated VMID mask is not this proof.
    pub contract: bool,
    /// Fresh boot only; not inferred from mapping rights or remembered leases.
    pub initial_hlos: bool,
}

/// Sealed seven-argument MEM_PROT_ASSIGN envelope, never caller-chosen VMIDs.
pub struct Call {
    words: [u64; 8],
    bytes: [u8; ARG_BYTES],
}
impl Call {
    pub fn words(&self) -> &[u64; 8] {
        &self.words
    }
    pub fn bytes(&self) -> &[u8; ARG_BYTES] {
        &self.bytes
    }
    fn new(region: Region, from: Ownership, to: Ownership) -> Result<Self, Error> {
        let sources: &[u32] = match from {
            Ownership::Host => &[HLOS],
            Ownership::SharedStorage => &[HLOS, MSS_MSA],
            Ownership::ModemExclusive => &[MSS_MSA],
            Ownership::Unknown => return Err(Error::Invalid),
        };
        let destinations: &[(u32, u32)] = match to {
            Ownership::Host => &[(HLOS, RW)],
            Ownership::SharedStorage => &[(HLOS, RW), (MSS_MSA, RW)],
            Ownership::ModemExclusive => &[(MSS_MSA, RW)],
            Ownership::Unknown => return Err(Error::Invalid),
        };
        Self::encode(region, sources, destinations)
    }

    /// Encode one validated MSA transfer plan in the same exact SCM64 envelope
    /// used by the modem storage/heap owner. This constructs bytes only; it
    /// does not invoke the SMC capability.
    pub fn for_msa_transfer(transfer: crate::qcom_msa::TransferPlan) -> Result<Self, Error> {
        let region = transfer.region();
        let bytes = u32::try_from(region.bytes).map_err(|_| Error::Invalid)?;
        let slots = transfer.destinations();
        let count = transfer.destination_count();
        if count == 0 || count > slots.len() || slots[count..].iter().any(Option::is_some) {
            return Err(Error::Invalid);
        }
        let mut destinations = [(0u32, 0u32); 3];
        for (index, slot) in slots[..count].iter().enumerate() {
            let permission = slot.ok_or(Error::Invalid)?;
            destinations[index] = (permission.vmid(), permission.permissions());
        }
        Self::encode(
            Region {
                base: region.base,
                bytes,
            },
            &transfer.source_vmids(),
            &destinations[..count],
        )
    }

    fn encode(region: Region, sources: &[u32], destinations: &[(u32, u32)]) -> Result<Self, Error> {
        if region.bytes == 0
            || region.base % u64::from(PAGE) != 0
            || region.bytes % PAGE != 0
            || sources.is_empty()
            || sources.len() > 3
            || sources.iter().any(|vmid| *vmid == 0)
            || sources
                .iter()
                .enumerate()
                .any(|(i, vmid)| sources[..i].contains(vmid))
            || destinations.is_empty()
            || destinations.len() > 3
            || destinations
                .iter()
                .any(|(vmid, perm)| *vmid == 0 || *perm != RW)
            || destinations
                .iter()
                .enumerate()
                .any(|(i, entry)| destinations[..i].iter().any(|prior| prior.0 == entry.0))
        {
            return Err(Error::Invalid);
        }
        let mut bytes = [0; ARG_BYTES];
        fn u32_at(b: &mut [u8], i: usize, v: u32) {
            b[i..i + 4].copy_from_slice(&v.to_le_bytes());
        }
        fn u64_at(b: &mut [u8], i: usize, v: u64) {
            b[i..i + 8].copy_from_slice(&v.to_le_bytes());
        }
        // Separate cache lines: range[16], sources[4/8], dest[24/48],
        // extra-argument array[7*8]. All unused/context/padding bytes zero.
        u64_at(&mut bytes, 0, region.base);
        u64_at(&mut bytes, 8, u64::from(region.bytes));
        for (i, vm) in sources.iter().enumerate() {
            u32_at(&mut bytes, 64 + i * 4, *vm);
        }
        for (i, (vm, perm)) in destinations.iter().enumerate() {
            u32_at(&mut bytes, 128 + i * 24, *vm);
            u32_at(&mut bytes, 132 + i * 24, *perm);
        }
        // args3..9; args6..9 remain zero (Linux's full extra buffer).
        u64_at(&mut bytes, 256, (sources.len() * 4) as u64);
        u64_at(&mut bytes, 264, SCRATCH.base + 128);
        u64_at(&mut bytes, 272, (destinations.len() * 24) as u64);
        Ok(Self {
            words: [
                0x42000c16,
                0x1117,
                SCRATCH.base,
                16,
                SCRATCH.base + 64,
                SCRATCH.base + 256,
                0,
                0,
            ],
            bytes,
        })
    }
}

/// Trusted, serialized boundary. All operations bounded (<=4096 data bytes),
/// uncached mappings only, no effects on Drop, resources retained on failure.
/// stage synchronizes/quiesces the target and copies *exactly* Call::bytes to
/// the private scratch page with system barriers before invoke. complete
/// orders ownership completion before host access or response publication.
/// invoke errors/any nonzero x0 are ambiguous; no retries or resume loops.
/// A single monitor invocation itself has no software wall-time guarantee.
pub trait Io {
    fn observe(&mut self) -> Result<Observation, Fault>;
    fn stage(&mut self, call: &Call) -> Result<(), Fault>;
    fn invoke(&mut self, call: &Call) -> Result<[u64; 8], Fault>;
    fn complete(&mut self) -> Result<(), Fault>;
    fn zero(&mut self, pool: Pool, offset: u32, bytes: usize) -> Result<(), Fault>;
    fn read(&mut self, pool: Pool, offset: u32, out: &mut [u8]) -> Result<(), Fault>;
    fn write(&mut self, pool: Pool, offset: u32, bytes: &[u8]) -> Result<(), Fault>;
}

/// No Clone, public fields, or Drop side effects; bound to one manager/lease.
pub struct Receipt {
    instance: u64,
    grant: Grant,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Attach,
    Lifecycle,
    Stopped,
    Invalid,
    Monitor,
    Completion,
    Access,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    pub owners: [Ownership; 2],
    pub error: Option<Error>,
    pub attempted: u64,
    pub completed: u64,
}

pub struct Pools<T: Io> {
    io: T,
    reservations: Reservations,
    instance: u64,
    identity: u64,
    status: Status,
    leases: [Option<Grant>; 2],
    last_serial: u64,
    // One bit per physical page. Any host write invalidates scrub evidence.
    clean: [[u64; 20]; 2],
    busy: bool,
}
impl<T: Io> Pools<T> {
    /// Always retain Io/reservations, including failed attachment. No initial
    /// zero/access/SMC is performed. Check status before composing a Backend.
    pub fn attach(reservations: Reservations, io: T) -> Self {
        let instance = NEXT_INSTANCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .unwrap_or(0);
        let mut s = Self {
            io,
            reservations,
            instance,
            identity: 0,
            status: Status {
                owners: [Ownership::Unknown; 2],
                error: Some(Error::Attach),
                attempted: 0,
                completed: 0,
            },
            leases: [None; 2],
            last_serial: 0,
            clean: [[0; 20]; 2],
            busy: true,
        };
        if instance != 0 {
            if let Ok(o) = s.io.observe() {
                if s.valid(o) && o.initial_hlos {
                    s.identity = o.identity;
                    s.status.owners = [Ownership::Host; 2];
                    s.status.error = None;
                    s.busy = false;
                }
            }
        }
        s
    }
    fn valid(&self, o: Observation) -> bool {
        o.generation == self.reservations.generation
            && o.identity != 0
            && o.ready
            && o.contract
            && o.mappings == MAPPINGS
    }
    pub fn generation(&self) -> u64 {
        self.reservations.generation
    }
    pub fn status(&self) -> Status {
        self.status
    }
    fn stop(&mut self, e: Error) -> Fault {
        self.status.error = Some(e);
        Fault::Uncertain
    }
    fn check(&mut self) -> Result<(), Fault> {
        if self.status.error.is_some() {
            return Err(Fault::Uncertain);
        }
        // Set before calling external code so unwind/reentrancy cannot retry.
        if self.busy {
            return Err(self.stop(Error::Stopped));
        }
        self.busy = true;
        let o = self.io.observe().map_err(|_| {
            self.status.owners = [Ownership::Unknown; 2];
            self.stop(Error::Lifecycle)
        })?;
        if !self.valid(o) || o.identity != self.identity {
            self.status.owners = [Ownership::Unknown; 2];
            return Err(self.stop(Error::Lifecycle));
        }
        self.busy = false;
        Ok(())
    }
    pub fn observe(&mut self) -> Result<(u64, [Ownership; 2]), Fault> {
        self.check()?;
        Ok((self.identity, self.status.owners))
    }
    fn bounds(&self, p: Pool, offset: u32, bytes: usize) -> bool {
        bytes != 0
            && bytes <= IO_BYTES
            && (offset as usize)
                .checked_add(bytes)
                .is_some_and(|v| v <= MAPPINGS[index(p)].region.bytes as usize)
    }
    fn dirty(&mut self, p: Pool, offset: u32, bytes: usize) {
        for page in offset / PAGE..=(offset + bytes as u32 - 1) / PAGE {
            self.clean[index(p)][page as usize / 64] &= !(1 << (page % 64));
        }
    }
    pub fn zero(&mut self, p: Pool, offset: u32, bytes: usize) -> Result<(), Fault> {
        self.check()?;
        if self.status.owners[index(p)] != Ownership::Host || !self.bounds(p, offset, bytes) {
            return Err(Fault::Failed);
        }
        self.dirty(p, offset, bytes);
        self.busy = true;
        match self.io.zero(p, offset, bytes) {
            Ok(()) => {}
            Err(Fault::Failed) => {
                self.busy = false;
                return Err(Fault::Failed);
            }
            Err(Fault::Uncertain) => return Err(self.stop(Error::Access)),
        }
        self.busy = false;
        self.check()?;
        if offset % PAGE == 0 && bytes == PAGE as usize {
            let page = (offset / PAGE) as usize;
            self.clean[index(p)][page / 64] |= 1 << (page % 64);
        }
        Ok(())
    }
    fn lease(&self, receipt: &Receipt) -> Result<Grant, Fault> {
        let g = receipt.grant;
        if receipt.instance != self.instance
            || self.leases[index(g.pool)] != Some(g)
            || self.status.owners[index(g.pool)] != g.ownership
        {
            return Err(Fault::Failed);
        }
        Ok(g)
    }
    pub fn info(&mut self, receipt: &Receipt) -> Result<Grant, Fault> {
        self.check()?;
        self.lease(receipt)
    }
    pub fn grant(&mut self, g: Grant) -> Result<Receipt, Fault> {
        self.check()?;
        let i = index(g.pool);
        let r = MAPPINGS[i].region;
        let expected = if g.pool == Pool::Storage {
            Ownership::SharedStorage
        } else {
            Ownership::ModemExclusive
        };
        if g.serial <= self.last_serial
            || g.serial == 0
            || g.generation != self.reservations.generation
            || g.peer.node != self.reservations.peer_node
            || g.peer.port == 0
            || g.peer.port >= 0xffff_fffe
            || self.status.owners[i] != Ownership::Host
            || self.leases[i].is_some()
            || g.ownership != expected
            || g.region.bytes == 0
            || g.region.base % u64::from(PAGE) != 0
            || g.region.bytes % PAGE != 0
            || g.region.base < r.base
            || end(g.region).is_none_or(|e| e > end(r).unwrap())
        {
            return Err(Fault::Failed);
        }
        let first = ((g.region.base - r.base) / u64::from(PAGE)) as usize;
        let count = (g.region.bytes / PAGE) as usize;
        if (first..first + count).any(|p| self.clean[i][p / 64] & (1 << (p % 64)) == 0) {
            return Err(Fault::Failed);
        }
        self.transition(g.pool, g.region, Ownership::Host, expected)?;
        self.last_serial = g.serial;
        self.leases[i] = Some(g);
        self.clean[i] = [0; 20];
        Ok(Receipt {
            instance: self.instance,
            grant: g,
        })
    }
    pub fn reclaim(&mut self, receipt: &Receipt) -> Result<(), Fault> {
        self.check()?;
        let g = self.lease(receipt)?;
        self.transition(g.pool, g.region, g.ownership, Ownership::Host)?;
        self.leases[index(g.pool)] = None;
        self.clean[index(g.pool)] = [0; 20];
        Ok(())
    }
    fn transition(
        &mut self,
        p: Pool,
        region: Region,
        from: Ownership,
        to: Ownership,
    ) -> Result<(), Fault> {
        let call = Call::new(region, from, to).map_err(|error| self.stop(error))?;
        self.busy = true;
        match self.io.stage(&call) {
            Ok(()) => {}
            Err(Fault::Failed) => {
                self.busy = false;
                return Err(Fault::Failed);
            }
            Err(Fault::Uncertain) => return Err(self.stop(Error::Completion)),
        }
        // A lifecycle change before submission is terminal; do not invoke.
        self.busy = false;
        self.check()?;
        self.busy = true;
        let i = index(p);
        // Mark unknown *before* submission. Includes kernel errors/unwind,
        // interrupted/busy returns and post-success barrier/gate failure.
        self.status.owners[i] = Ownership::Unknown;
        self.status.attempted = self
            .status
            .attempted
            .checked_add(1)
            .ok_or_else(|| self.stop(Error::Stopped))?;
        let reply = self
            .io
            .invoke(&call)
            .map_err(|_| self.stop(Error::Monitor))?;
        // Linux's __qcom_scm_assign_mem() checks the SMC transport status in
        // a0, then returns the assignment command status from a1.
        if reply[0] != 0 || reply[1] != 0 {
            return Err(self.stop(Error::Monitor));
        }
        self.io
            .complete()
            .map_err(|_| self.stop(Error::Completion))?;
        self.busy = false;
        self.check()?;
        self.status.owners[i] = to;
        self.status.completed += 1;
        Ok(())
    }
    fn access(
        &mut self,
        receipt: &Receipt,
        offset: u32,
        bytes: usize,
    ) -> Result<(Pool, u32), Fault> {
        self.check()?;
        let g = self.lease(receipt)?;
        // Heap becomes inaccessible to ALL host data APIs after assignment.
        if g.pool != Pool::Storage
            || !self.bounds(g.pool, offset, bytes)
            || (offset as usize)
                .checked_add(bytes)
                .is_none_or(|n| n > g.region.bytes as usize)
        {
            return Err(Fault::Failed);
        }
        Ok((g.pool, (g.region.base - STORAGE.base) as u32 + offset))
    }
    pub fn read(&mut self, receipt: &Receipt, offset: u32, out: &mut [u8]) -> Result<(), Fault> {
        let (p, at) = self.access(receipt, offset, out.len())?;
        self.busy = true;
        let r = self.io.read(p, at, out);
        self.finish_access(r)
    }
    pub fn write(&mut self, receipt: &Receipt, offset: u32, bytes: &[u8]) -> Result<(), Fault> {
        let (p, at) = self.access(receipt, offset, bytes.len())?;
        self.busy = true;
        let r = self.io.write(p, at, bytes);
        self.finish_access(r)
    }
    fn finish_access(&mut self, r: Result<(), Fault>) -> Result<(), Fault> {
        match r {
            Ok(()) => {
                self.busy = false;
                self.check()
            }
            Err(Fault::Failed) => {
                self.busy = false;
                self.check()?;
                Err(Fault::Failed)
            }
            Err(Fault::Uncertain) => Err(self.stop(Error::Access)),
        }
    }
}
