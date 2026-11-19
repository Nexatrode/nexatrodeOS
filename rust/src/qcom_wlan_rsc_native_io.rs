//! Lease-gated native Apps RSC access for the shared Sunfish RPMh broker.
//!
//! This adapter exposes only bounded DRV2 snapshot reads and active-TCS0
//! transaction registers. It does not create the exclusive RSC owner, invent
//! a client vote ledger, or enable WLAN power. A trusted platform `Gate` must
//! pin the exact uncached mappings and live exclusive lease for each access.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::qcom_rpmh_handoff::{self as handoff, ReadRegisters, Register as SnapshotRegister};
use crate::qcom_rpmh_vrm_tcs_io::{AccessError, GuardedRegisters};
use crate::qcom_rpmh_vrm_tcs_transport::Backend;

pub const PHYSICAL_BASE: u64 = 0x1822_0000;
pub const FIRST_PAGE_BYTES: usize = 0x1000;
pub const TAIL_BYTES: usize = 0x2000;
pub const TOTAL_BYTES: usize = FIRST_PAGE_BYTES + TAIL_BYTES;
pub const FIRST_VIRTUAL_BASE: u64 = 0x4000_0000;
pub const TAIL_VIRTUAL_BASE: u64 = FIRST_VIRTUAL_BASE + FIRST_PAGE_BYTES as u64;

const DRV_BASE: usize = 0xd00;
const TCS_STRIDE: usize = 0x2a0;
const COMMAND_COUNT: usize = 16;
const COMMAND_STRIDE: usize = 20;

#[derive(Clone, Copy)]
pub struct Mapping {
    pub first_rw: *mut u8,
    pub tail_ro: *const u8,
}

/// Facts supplied by the trusted RSC owner while its lease guard is held.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Observation {
    pub identity: u64,
    pub generation: u64,
    pub fresh_boot: bool,
    pub exclusive: bool,
    pub first_virtual: u64,
    pub first_physical: u64,
    pub first_bytes: u64,
    pub first_device_uncached: bool,
    pub first_writable: bool,
    pub tail_virtual: u64,
    pub tail_physical: u64,
    pub tail_bytes: u64,
    pub tail_device_uncached: bool,
    pub tail_read_only: bool,
}

pub trait Guard<L> {
    fn observation(&self) -> Observation;

    /// Confirm that this guard pins the exact lease passed by the caller.
    fn owns_lease(&self, lease: &L) -> bool;
}

/// Exclusive Apps RSC authority used by each native register access.
///
/// # Safety
/// `lock` must serialize with lease revocation, reset, remapping, and every
/// competing RSC owner. Its guard must pin the reported mapping and exclusive
/// lease until drop. The observation must be live authority state, not a
/// manifest constant or peer-provided claim.
pub unsafe trait Gate<L> {
    type Guard<'a>: Guard<L>
    where
        Self: 'a;
    type Error;

    fn lock(&mut self) -> Result<Self::Guard<'_>, Self::Error>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error<E> {
    InvalidMapping,
    InvalidAuthority,
    LeaseLost,
    Gate(E),
    RegisterAccess,
    Snapshot(handoff::Error),
}

pub struct NativeRscIo<L, G: Gate<L>> {
    mapping: Mapping,
    identity: u64,
    generation: u64,
    gate: G,
    _lease: core::marker::PhantomData<fn(&L)>,
}

fn ranges_overlap(a: u64, a_bytes: u64, b: u64, b_bytes: u64) -> bool {
    let Some(a_end) = a.checked_add(a_bytes) else {
        return true;
    };
    let Some(b_end) = b.checked_add(b_bytes) else {
        return true;
    };
    a < b_end && b < a_end
}

fn mapping_shape_valid(mapping: Mapping) -> bool {
    if mapping.first_rw.is_null()
        || mapping.tail_ro.is_null()
        || mapping.first_rw as usize % 4 != 0
        || mapping.tail_ro as usize % 4 != 0
        || (mapping.first_rw as usize)
            .checked_add(FIRST_PAGE_BYTES)
            .is_none()
        || (mapping.tail_ro as usize).checked_add(TAIL_BYTES).is_none()
        || ranges_overlap(
            mapping.first_rw as usize as u64,
            FIRST_PAGE_BYTES as u64,
            mapping.tail_ro as usize as u64,
            TAIL_BYTES as u64,
        )
    {
        return false;
    }
    (cfg!(test) && !cfg!(target_os = "none"))
        || (mapping.first_rw as usize as u64 == FIRST_VIRTUAL_BASE
            && mapping.tail_ro as usize as u64 == TAIL_VIRTUAL_BASE)
}

fn authorized(observation: Observation, mapping: Mapping, identity: u64, generation: u64) -> bool {
    identity != 0
        && generation != 0
        && observation.identity == identity
        && observation.generation == generation
        && observation.fresh_boot
        && observation.exclusive
        && observation.first_virtual == FIRST_VIRTUAL_BASE
        && observation.first_physical == PHYSICAL_BASE
        && observation.first_bytes == FIRST_PAGE_BYTES as u64
        && observation.first_device_uncached
        && observation.first_writable
        && observation.tail_virtual == TAIL_VIRTUAL_BASE
        && observation.tail_physical == PHYSICAL_BASE + FIRST_PAGE_BYTES as u64
        && observation.tail_bytes == TAIL_BYTES as u64
        && observation.tail_device_uncached
        && observation.tail_read_only
        && mapping_shape_valid(mapping)
}

fn readable(offset: usize) -> bool {
    if offset % 4 != 0 || offset >= TOTAL_BYTES {
        return false;
    }
    if offset == 0x00c || offset == DRV_BASE || offset == DRV_BASE + 4 {
        return true;
    }
    for bank in 0..handoff::TCS_COUNT {
        let base = DRV_BASE + bank * TCS_STRIDE;
        if matches!(offset, x if x == base + 0x10 || x == base + 0x14 || x == base + 0x18 || x == base + 0x1c)
        {
            return true;
        }
        for command in 0..COMMAND_COUNT {
            let command_base = base + command * COMMAND_STRIDE;
            if matches!(offset, x if x == command_base + 0x30 || x == command_base + 0x34 || x == command_base + 0x38)
            {
                return true;
            }
        }
    }
    (0..COMMAND_COUNT).any(|slot| offset == DRV_BASE + 0x3c + slot * COMMAND_STRIDE)
}

fn writable(offset: usize) -> bool {
    if offset % 4 != 0 || offset >= FIRST_PAGE_BYTES {
        return false;
    }
    if offset == DRV_BASE + 0x08
        || offset == DRV_BASE + 0x10
        || offset == DRV_BASE + 0x14
        || offset == DRV_BASE + 0x1c
    {
        return true;
    }
    (0..COMMAND_COUNT).any(|slot| {
        let base = DRV_BASE + slot * COMMAND_STRIDE;
        matches!(offset, x if x == base + 0x30 || x == base + 0x34 || x == base + 0x38)
    })
}

impl<L, G: Gate<L>> NativeRscIo<L, G> {
    /// # Safety
    /// `mapping` must be the exact two-part uncached Apps RSC mapping in the
    /// Sunfish Microkit profile: page zero writable, pages one and two
    /// read-only. The pointers must remain valid for this object's lifetime
    /// and must have no writable alias in another protection domain. `gate`
    /// must represent the live exclusive owner described by its safety
    /// contract. This constructor does not create that owner.
    pub unsafe fn new(mapping: Mapping, mut gate: G) -> Result<Self, Error<G::Error>> {
        if !mapping_shape_valid(mapping) {
            return Err(Error::InvalidMapping);
        }
        let guard = gate.lock().map_err(Error::Gate)?;
        let observation = guard.observation();
        if observation.identity == 0
            || observation.generation == 0
            || !authorized(
                observation,
                mapping,
                observation.identity,
                observation.generation,
            )
        {
            return Err(Error::InvalidAuthority);
        }
        let identity = observation.identity;
        let generation = observation.generation;
        drop(guard);
        Ok(Self {
            mapping,
            identity,
            generation,
            gate,
            _lease: core::marker::PhantomData,
        })
    }

    fn lock_for_access<'a>(
        &'a mut self,
        lease: &L,
        identity: u64,
        generation: u64,
    ) -> Result<G::Guard<'a>, AccessError<Error<G::Error>>> {
        if identity != self.identity || generation != self.generation {
            return Err(AccessError::LeaseLost);
        }
        let mapping = self.mapping;
        let guard = self
            .gate
            .lock()
            .map_err(|error| AccessError::Io(Error::Gate(error)))?;
        if !guard.owns_lease(lease)
            || !authorized(guard.observation(), mapping, identity, generation)
        {
            return Err(AccessError::LeaseLost);
        }
        Ok(guard)
    }

    fn read_volatile(mapping: Mapping, offset: usize) -> u32 {
        barrier();
        let value = if offset < FIRST_PAGE_BYTES {
            // SAFETY: constructor and live guard establish the exact first
            // page mapping; the caller has checked the read allowlist.
            unsafe { mapping.first_rw.add(offset).cast::<u32>().read_volatile() }
        } else {
            // SAFETY: constructor and live guard establish the exact read-only
            // tail mapping; the caller has checked the bounded offset.
            unsafe {
                mapping
                    .tail_ro
                    .add(offset - FIRST_PAGE_BYTES)
                    .cast::<u32>()
                    .read_volatile()
            }
        };
        barrier();
        u32::from_le(value)
    }

    fn write_volatile(mapping: Mapping, offset: usize, value: u32) {
        barrier();
        // SAFETY: constructor and live guard establish the exact writable
        // first page, and the caller has checked the narrow TCS0 allowlist.
        unsafe {
            mapping
                .first_rw
                .add(offset)
                .cast::<u32>()
                .write_volatile(value.to_le())
        };
        barrier();
    }
}

impl<L, G: Gate<L>> GuardedRegisters<L> for NativeRscIo<L, G> {
    type Error = Error<G::Error>;

    fn validate(&mut self, lease: &L, identity: u64, generation: u64) -> bool {
        self.lock_for_access(lease, identity, generation).is_ok()
    }

    fn read32(
        &mut self,
        lease: &L,
        identity: u64,
        generation: u64,
        offset: usize,
    ) -> Result<u32, AccessError<Self::Error>> {
        if !readable(offset) {
            return Err(AccessError::Io(Error::RegisterAccess));
        }
        let mapping = self.mapping;
        let _guard = self.lock_for_access(lease, identity, generation)?;
        if !authorized(_guard.observation(), mapping, identity, generation) {
            return Err(AccessError::LeaseLost);
        }
        Ok(Self::read_volatile(mapping, offset))
    }

    fn write32(
        &mut self,
        lease: &L,
        identity: u64,
        generation: u64,
        offset: usize,
        value: u32,
    ) -> Result<(), AccessError<Self::Error>> {
        if !writable(offset) {
            return Err(AccessError::Io(Error::RegisterAccess));
        }
        let mapping = self.mapping;
        let _guard = self.lock_for_access(lease, identity, generation)?;
        if !authorized(_guard.observation(), mapping, identity, generation) {
            return Err(AccessError::LeaseLost);
        }
        Self::write_volatile(mapping, offset, value);
        Ok(())
    }
}

impl<L, G: Gate<L>> Backend<L> for NativeRscIo<L, G> {
    fn capture_stable(
        &mut self,
        lease: &L,
        identity: u64,
        generation: u64,
    ) -> Result<handoff::Snapshot, Error<G::Error>> {
        if !self.validate(lease, identity, generation) {
            return Err(Error::LeaseLost);
        }
        let mut reader = SnapshotReader {
            io: self,
            lease,
            identity,
            generation,
            first_error: None,
        };
        let snapshot = handoff::capture_stable(&mut reader, 3);
        if let Some(error) = reader.first_error.take() {
            return Err(access_error(error));
        }
        snapshot.map_err(Error::Snapshot)
    }
}

struct SnapshotReader<'a, L, G: Gate<L>> {
    io: &'a mut NativeRscIo<L, G>,
    lease: &'a L,
    identity: u64,
    generation: u64,
    first_error: Option<AccessError<Error<G::Error>>>,
}

impl<L, G: Gate<L>> ReadRegisters for SnapshotReader<'_, L, G> {
    fn read(&mut self, register: SnapshotRegister) -> u32 {
        if self.first_error.is_some() {
            return 0;
        }
        match self.io.read32(
            self.lease,
            self.identity,
            self.generation,
            register.offset(),
        ) {
            Ok(value) => value,
            Err(error) => {
                self.first_error = Some(error);
                0
            }
        }
    }
}

fn access_error<E>(error: AccessError<Error<E>>) -> Error<E> {
    match error {
        AccessError::LeaseLost => Error::LeaseLost,
        AccessError::Io(error) => error,
    }
}

fn barrier() {
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: DSB SY orders device-register accesses.
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::{Cell, Ref, RefCell};
    use std::boxed::Box;
    use std::rc::Rc;

    const IDENTITY: u64 = 0x51;
    const GENERATION: u64 = 9;
    const LEASE_ID: u64 = 0xabc;

    #[repr(align(4096))]
    struct First([u32; FIRST_PAGE_BYTES / 4]);

    #[repr(align(4096))]
    struct Tail([u32; TAIL_BYTES / 4]);

    #[derive(Clone, Copy)]
    struct Lease(u64);

    struct Authority {
        observation: Observation,
        lease_id: u64,
    }

    #[derive(Clone)]
    struct FakeGate {
        authority: Rc<RefCell<Authority>>,
        locks: Rc<Cell<usize>>,
    }

    struct FakeGuard<'a>(Ref<'a, Authority>);

    impl Guard<Lease> for FakeGuard<'_> {
        fn observation(&self) -> Observation {
            self.0.observation
        }

        fn owns_lease(&self, lease: &Lease) -> bool {
            lease.0 == self.0.lease_id
        }
    }

    // SAFETY: the fixture is single-threaded; the RefCell borrow pins the
    // authority record while each volatile operation touches aligned arrays.
    unsafe impl Gate<Lease> for FakeGate {
        type Guard<'a> = FakeGuard<'a>;
        type Error = ();

        fn lock(&mut self) -> Result<Self::Guard<'_>, Self::Error> {
            let authority = self.authority.try_borrow().map_err(|_| ())?;
            self.locks.set(self.locks.get() + 1);
            Ok(FakeGuard(authority))
        }
    }

    fn observation() -> Observation {
        Observation {
            identity: IDENTITY,
            generation: GENERATION,
            fresh_boot: true,
            exclusive: true,
            first_virtual: FIRST_VIRTUAL_BASE,
            first_physical: PHYSICAL_BASE,
            first_bytes: FIRST_PAGE_BYTES as u64,
            first_device_uncached: true,
            first_writable: true,
            tail_virtual: TAIL_VIRTUAL_BASE,
            tail_physical: PHYSICAL_BASE + FIRST_PAGE_BYTES as u64,
            tail_bytes: TAIL_BYTES as u64,
            tail_device_uncached: true,
            tail_read_only: true,
        }
    }

    struct Fixture {
        first: Box<First>,
        tail: Box<Tail>,
        authority: Rc<RefCell<Authority>>,
        locks: Rc<Cell<usize>>,
        io: NativeRscIo<Lease, FakeGate>,
    }

    impl Fixture {
        fn new() -> Self {
            let mut first = Box::new(First([0; FIRST_PAGE_BYTES / 4]));
            let tail = Box::new(Tail([0; TAIL_BYTES / 4]));
            first.0[0x00c / 4] = handoff::OBSERVED_CONFIG;
            first.0[DRV_BASE / 4] = 1;
            first.0[(DRV_BASE + 0x18) / 4] = 1;
            first.0[(DRV_BASE + TCS_STRIDE + 0x18) / 4] = 1;
            let authority = Rc::new(RefCell::new(Authority {
                observation: observation(),
                lease_id: LEASE_ID,
            }));
            let locks = Rc::new(Cell::new(0));
            let gate = FakeGate {
                authority: authority.clone(),
                locks: locks.clone(),
            };
            let mapping = Mapping {
                first_rw: first.0.as_mut_ptr().cast(),
                tail_ro: tail.0.as_ptr().cast(),
            };
            // SAFETY: both boxes are aligned fixtures with exact bounded
            // extents; FakeGate pins their authority record for each access.
            let io = unsafe { NativeRscIo::new(mapping, gate).unwrap() };
            Self {
                first,
                tail,
                authority,
                locks,
                io,
            }
        }

        fn lease(&self) -> Lease {
            Lease(LEASE_ID)
        }
    }

    #[test]
    fn stable_snapshot_uses_exact_rw_and_ro_windows() {
        let mut fixture = Fixture::new();
        let lease = fixture.lease();
        let snapshot = fixture
            .io
            .capture_stable(&lease, IDENTITY, GENERATION)
            .unwrap();
        assert_eq!(snapshot.config, handoff::OBSERVED_CONFIG);
        assert_eq!(snapshot.irq_enable, 1);
        assert_eq!(snapshot.banks[0].idle, 1);
        assert_eq!(snapshot.banks[1].idle, 1);
        let offset = handoff::Register::bank(7, handoff::BankField::Idle)
            .unwrap()
            .offset();
        fixture.tail.0[(offset - FIRST_PAGE_BYTES) / 4] = 0x1234_5678;
        assert_eq!(
            fixture
                .io
                .read32(&fixture.lease(), IDENTITY, GENERATION, offset),
            Ok(0x1234_5678)
        );
    }

    #[test]
    fn writes_are_limited_to_active_tcs0_and_its_irq_clear() {
        let mut fixture = Fixture::new();
        let lease = fixture.lease();
        for offset in [
            DRV_BASE + 0x08,
            DRV_BASE + 0x10,
            DRV_BASE + 0x14,
            DRV_BASE + 0x1c,
            DRV_BASE + 0x30,
            DRV_BASE + 0x34,
            DRV_BASE + 0x38,
        ] {
            fixture
                .io
                .write32(&lease, IDENTITY, GENERATION, offset, 0x55aa)
                .unwrap();
        }
        for offset in [0x00c, DRV_BASE + TCS_STRIDE + 0x10, 0x1000] {
            let locks_before = fixture.locks.get();
            assert_eq!(
                fixture.io.write32(&lease, IDENTITY, GENERATION, offset, 1),
                Err(AccessError::Io(Error::RegisterAccess))
            );
            assert_eq!(fixture.locks.get(), locks_before);
        }
        assert_eq!(fixture.first.0[(DRV_BASE + 0x10) / 4], 0x55aa);
    }

    #[test]
    fn stale_generation_identity_and_lease_stop_before_mmio() {
        let mut fixture = Fixture::new();
        let lease = fixture.lease();
        let before = fixture.first.0[DRV_BASE / 4];
        assert_eq!(
            fixture
                .io
                .write32(&lease, IDENTITY, GENERATION + 1, DRV_BASE + 0x10, 1),
            Err(AccessError::LeaseLost)
        );
        assert_eq!(
            fixture
                .io
                .write32(&lease, IDENTITY + 1, GENERATION, DRV_BASE + 0x10, 1),
            Err(AccessError::LeaseLost)
        );
        assert_eq!(
            fixture
                .io
                .write32(&Lease(0), IDENTITY, GENERATION, DRV_BASE + 0x10, 1),
            Err(AccessError::LeaseLost)
        );
        assert_eq!(fixture.first.0[DRV_BASE / 4], before);

        fixture.authority.borrow_mut().observation.generation += 1;
        assert!(!fixture.io.validate(&lease, IDENTITY, GENERATION));
        assert_eq!(
            fixture.io.read32(&lease, IDENTITY, GENERATION, 0x00c),
            Err(AccessError::LeaseLost)
        );
    }

    #[test]
    fn constructor_refuses_stale_or_nonexclusive_mapping_evidence() {
        let mutations: [fn(&mut Observation); 4] = [
            |value: &mut Observation| value.fresh_boot = false,
            |value: &mut Observation| value.exclusive = false,
            |value: &mut Observation| value.tail_read_only = false,
            |value: &mut Observation| value.first_physical += 0x1000,
        ];
        for mutate in mutations {
            let first = Box::new(First([0; FIRST_PAGE_BYTES / 4]));
            let tail = Box::new(Tail([0; TAIL_BYTES / 4]));
            let mut observed = observation();
            mutate(&mut observed);
            let authority = Rc::new(RefCell::new(Authority {
                observation: observed,
                lease_id: LEASE_ID,
            }));
            let gate = FakeGate {
                authority,
                locks: Rc::new(Cell::new(0)),
            };
            let mapping = Mapping {
                first_rw: first.0.as_ptr().cast_mut().cast(),
                tail_ro: tail.0.as_ptr().cast(),
            };
            // SAFETY: fixture pointers are valid; mutated observations are
            // deliberately rejected before they can authorize MMIO.
            assert!(matches!(
                unsafe { NativeRscIo::new(mapping, gate) },
                Err(Error::InvalidAuthority)
            ));
        }
    }
}
