//! Source-derived SCM crypto-to-EBI cold-test hold for Sunfish.
//!
//! Linux requests ICC average=0, peak=UINT_MAX kB/s around PAS operations.
//! The audited Sunfish SCM node has no optional clocks. This experiment raises
//! six nondecreasing active floors and keeps them until reboot. It does not
//! restore unknown previous AP contributions, implement a full shared ledger,
//! or grant production power/DMA readiness. Every operation is bounded; partial
//! writes and failed handoff/readback remain retained without replay.
#![forbid(unsafe_code)]
use crate::{
    qcom_bcm::Metadata,
    qcom_bcm_vote::{Decoded, Vector},
    qcom_rpmh_bus,
    qcom_rpmh_handoff::{self as handoff, ReadRegisters},
    qcom_rpmh_read::{ActiveTransaction, BcmRead, BcmResource, Command, RegisterIo},
    sunfish_ipa_linux_pas::Resources,
};
pub const COUNT: usize = 6;
pub const NAMES: [&str; COUNT] = ["MC0", "ACV", "CE0", "SN11", "SN0", "SH0"];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Consumed,
    Metadata,
    Baseline,
    Handoff,
    Controller,
    Readback,
    NotHeld,
    Lease,
}
impl Error {
    pub const fn code(self) -> u64 {
        self as u64 + 1
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Floor {
    pub resource: BcmResource,
    pub vector: Vector,
    pub data: u32,
    mask: bool,
    vcd: u8,
}
pub fn plan(metadata: [Metadata; COUNT], before: [u32; COUNT]) -> Result<[Floor; COUNT], Error> {
    let mut result = [None; COUNT];
    for (i, resource) in BcmResource::ALL.into_iter().enumerate() {
        let m = metadata[i];
        let mut id = [0; 8];
        id[..NAMES[i].len()].copy_from_slice(NAMES[i].as_bytes());
        if m.id != id
            || m.address != resource.address()
            || m.unit_size == 0
            || m.clock_domain >= 16
            || m.reserved != 0
            || (resource != BcmResource::Acv && m.width == 0)
        {
            return Err(Error::Metadata);
        }
        let old = Decoded::parse(before[i]).map_err(|_| Error::Baseline)?;
        if !old.valid && old.vector != Vector::default() {
            return Err(Error::Baseline);
        }
        let mask = resource == BcmResource::Acv;
        let vector = if mask {
            Vector {
                x: old.vector.x,
                y: old.vector.y | 8,
            }
        } else {
            let width = match resource {
                BcmResource::Mc0 => 4,
                BcmResource::Ce0 => 8,
                _ => 16,
            };
            // bcm-voter's division rounds nonzero sub-unit values up to one,
            // otherwise truncates, then applies vote_scale=1000/unit.
            let div = |n: u64, d: u64| if n != 0 && n < d { 1 } else { n / d };
            let peak = div(
                div(u64::from(u32::MAX) * u64::from(m.width), width) * 1000,
                u64::from(m.unit_size),
            );
            Vector {
                x: old.vector.x,
                y: old.vector.y.max(peak.min(0x3fff)),
            }
        };
        result[i] = Some(Floor {
            resource,
            vector,
            data: (1 << 30) | (1 << 29) | ((vector.x as u32) << 14) | vector.y as u32,
            mask,
            vcd: m.clock_domain,
        });
    }
    let mut floors = result.map(Option::unwrap);
    floors.sort_unstable_by_key(|f| (f.vcd, f.resource.address()));
    Ok(floors)
}
#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub submitted: u8,
    pub completed: u8,
    pub observed: [u32; COUNT],
    pub failed: Option<BcmResource>,
    pub error: Option<Error>,
}
pub struct HoldOnce {
    used: bool,
    floors: Option<[Floor; COUNT]>,
    handoff: Option<handoff::Snapshot>,
    report: Report,
    failure: core::cell::Cell<Option<Error>>,
    sequence: core::cell::Cell<u64>,
    outstanding: core::cell::Cell<Option<u64>>,
}
impl HoldOnce {
    pub const fn new() -> Self {
        Self {
            used: false,
            floors: None,
            handoff: None,
            failure: core::cell::Cell::new(None),
            sequence: core::cell::Cell::new(0),
            outstanding: core::cell::Cell::new(None),
            report: Report {
                submitted: 0,
                completed: 0,
                observed: [0; COUNT],
                failed: None,
                error: None,
            },
        }
    }
    pub fn report(&self) -> Report {
        let mut report = self.report;
        if report.error.is_none() {
            report.error = self.failure.get();
        }
        report
    }
    pub fn enable<I: RegisterIo + ReadRegisters>(
        &mut self,
        io: &mut I,
        metadata: [Metadata; COUNT],
        polls: u32,
        mut wait: impl FnMut(),
        mut progress: impl FnMut(BcmResource),
    ) -> Result<(), Error> {
        if self.used {
            return Err(Error::Consumed);
        }
        self.used = true;
        let result = (|| {
            let baseline = qcom_rpmh_bus::read_bcm_series(io, polls, &mut wait, |_, _| {});
            if !baseline.passed() || baseline.responses[0] != baseline.responses[1] {
                return Err(Error::Baseline);
            }
            let before = baseline.responses[0].map(Option::unwrap);
            let floors = plan(metadata, before)?;
            let expected = baseline.after.ok_or(Error::Handoff)?;
            self.handoff = Some(expected);
            for (i, floor) in floors.iter().enumerate() {
                self.report.failed = Some(floor.resource);
                progress(floor.resource);
                if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)? != expected {
                    return Err(Error::Handoff);
                }
                let mut t = ActiveTransaction::new(
                    Command::ScmLinuxFloor {
                        resource: floor.resource,
                        data: floor.data,
                    },
                    polls,
                );
                self.report.submitted |= 1 << i;
                t.start(io).map_err(|_| Error::Controller)?;
                while !t.poll(io).map_err(|_| Error::Controller)? {
                    wait();
                }
                if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)? != expected {
                    return Err(Error::Handoff);
                }
                self.report.observed[i] = verify_floor(io, *floor, polls, &mut wait)?;
                if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)? != expected {
                    return Err(Error::Handoff);
                }
                self.report.completed |= 1 << i;
            }
            self.floors = Some(floors);
            self.report.failed = None;
            Ok(())
        })();
        self.report.error = result.err();
        result
    }
    pub fn verify<I: RegisterIo + ReadRegisters>(
        &self,
        io: &mut I,
        polls: u32,
        mut wait: impl FnMut(),
    ) -> Result<(), Error> {
        let floors = self.floors.ok_or(Error::NotHeld)?;
        if self.failure.get().is_some()
            || self.report.error.is_some()
            || self.report.completed != 0x3f
        {
            return Err(Error::NotHeld);
        }
        let result = (|| {
            let expected = self.handoff.ok_or(Error::NotHeld)?;
            if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)? != expected {
                return Err(Error::Handoff);
            }
            for floor in floors {
                verify_floor(io, floor, polls, &mut wait)?;
                if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)? != expected {
                    return Err(Error::Handoff);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.failure.set(Some(error));
        }
        result
    }
}
impl Default for HoldOnce {
    fn default() -> Self {
        Self::new()
    }
}
impl HoldOnce {
    /// Shared by the original provider and the modem's composed provider;
    /// lease identity/failure remains in this actual retained hold.
    pub(crate) fn acquire_lease<I: RegisterIo + ReadRegisters>(
        &self,
        io: &mut I,
        polls: u32,
    ) -> Result<Lease, Error> {
        let result = (|| {
            if self.failure.get().is_some() || self.outstanding.get().is_some() {
                return Err(Error::Lease);
            }
            let sequence = self.sequence.get().checked_add(1).ok_or(Error::Lease)?;
            self.sequence.set(sequence);
            self.verify(io, polls, core::hint::spin_loop)?;
            self.outstanding.set(Some(sequence));
            Ok(Lease { sequence })
        })();
        if let Err(e) = result {
            if self.failure.get().is_none() {
                self.failure.set(Some(e));
            }
        }
        result
    }
    pub(crate) fn release_lease<I: RegisterIo + ReadRegisters>(
        &self,
        io: &mut I,
        polls: u32,
        lease: Lease,
    ) -> Result<(), Error> {
        let result = (|| {
            if self.failure.get().is_some() || self.outstanding.get() != Some(lease.sequence) {
                return Err(Error::Lease);
            }
            self.outstanding.set(None); // consume before uncertain verification
            self.verify(io, polls, core::hint::spin_loop)
        })();
        if let Err(e) = result {
            if self.failure.get().is_none() {
                self.failure.set(Some(e));
            }
        }
        result
    }
}
fn verify_floor<I: RegisterIo>(
    io: &mut I,
    floor: Floor,
    polls: u32,
    wait: &mut impl FnMut(),
) -> Result<u32, Error> {
    let mut read = BcmRead::new(floor.resource, polls);
    read.start(io).map_err(|_| Error::Controller)?;
    while !read.poll(io).map_err(|_| Error::Controller)? {
        wait();
    }
    let raw = read.response().ok_or(Error::Readback)?;
    let actual = Decoded::parse(raw).map_err(|_| Error::Readback)?;
    if !actual.valid
        || if floor.mask {
            actual.vector.x & floor.vector.x != floor.vector.x
                || actual.vector.y & floor.vector.y != floor.vector.y
        } else {
            actual.vector.x < floor.vector.x || actual.vector.y < floor.vector.y
        }
    {
        return Err(Error::Readback);
    }
    Ok(raw)
}
pub struct Lease {
    sequence: u64,
}
/// Borrow the real retained hold and controller. Rechecks all six real BCMs
/// before and after each PAS call. Releasing a *call lease* performs no unvote;
/// the declared experimental floors and all partial state stay until reboot.
pub struct HeldResources<'a, I> {
    hold: &'a HoldOnce,
    io: &'a mut I,
    polls: u32,
    failed: bool,
}
impl<'a, I> HeldResources<'a, I> {
    pub fn new(hold: &'a HoldOnce, io: &'a mut I, polls: u32) -> Result<Self, Error> {
        if hold.floors.is_none()
            || hold.report().error.is_some()
            || hold.outstanding.get().is_some()
            || polls == 0
        {
            return Err(Error::NotHeld);
        }
        Ok(Self {
            hold,
            io,
            polls,
            failed: false,
        })
    }
}
impl<I: RegisterIo + ReadRegisters> Resources for HeldResources<'_, I> {
    type Lease = Lease;
    fn acquire(&mut self) -> Result<Lease, u64> {
        let result = (|| {
            if self.failed {
                return Err(Error::Lease);
            }
            self.hold.acquire_lease(self.io, self.polls)
        })();
        if result.is_err() {
            self.failed = true;
            if self.hold.failure.get().is_none() {
                self.hold.failure.set(result.as_ref().err().copied());
            }
        }
        result.map_err(Error::code)
    }
    fn release(&mut self, lease: Lease) -> Result<(), u64> {
        let result = (|| {
            if self.failed {
                return Err(Error::Lease);
            }
            self.hold.release_lease(self.io, self.polls, lease)
        })();
        if result.is_err() {
            self.failed = true;
            if self.hold.failure.get().is_none() {
                self.hold.failure.set(result.as_ref().err().copied());
            }
        }
        result.map_err(Error::code)
    }
}
