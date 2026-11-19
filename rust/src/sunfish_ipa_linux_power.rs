//! First native IPA power stage from the working Linux reference, 2267e248.
//!
//! Linux v4.2 IPA power uses three ICC paths and a 100 MHz IP0 clock. This
//! one-shot cold-boot experiment raises active floors while retaining every
//! observed component. Reads are aggregate hardware observations, NOT previous
//! AP votes or a complete client ledger. Nothing here can unvote, restore a
//! prior client vote, perform runtime PM, or mint IPA/modem/DMA readiness.
//! Each single-resource TCS is committed separately; interconnects precede
//! the core clock. All floors remain held until explicit reboot.
#![forbid(unsafe_code)]

use crate::qcom_bcm::Metadata;
use crate::qcom_bcm_vote::{Decoded, Vector};
use crate::qcom_rpmh_bus::{self, IPA_PATH_BCM_COUNT};
use crate::qcom_rpmh_handoff::{self as handoff, ReadRegisters};
use crate::qcom_rpmh_read::{
    ActiveTransaction, BcmRead, BcmTarget, BcmTargetName, Command, RegisterIo,
};

pub const CORE_HZ: u64 = 100_000_000;
pub const COUNT: usize = IPA_PATH_BCM_COUNT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    AlreadyStarted,
    Metadata,
    Baseline,
    Handoff,
    Controller,
    Readback,
}

/// A nondecreasing experimental floor, not a regular Linux client vote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Floor {
    target: BcmTarget,
    vector: Vector,
    data: u32,
    mask: bool,
    vcd: u8,
}
impl Floor {
    pub const fn target(self) -> BcmTarget {
        self.target
    }
    pub const fn data(self) -> u32 {
        self.data
    }
    pub const fn vector(self) -> Vector {
        self.vector
    }
}

fn div(n: u64, d: u64) -> Result<u64, Error> {
    if d == 0 {
        return Err(Error::Metadata);
    }
    Ok(if n != 0 && n < d { 1 } else { n / d })
}

/// BCM nodes on the three exact SM7150 routes. Tuples are total average and
/// peak kB/s, node average divisor (channels*buswidth), and peak buswidth.
/// Shared SN11 nodes see memory+imem; max_peak is max(465000, 68570), while
/// imem average is zero. CN0's narrowest selected member is qhs_ipa (4 bytes).
fn node(name: BcmTargetName) -> Option<(u64, u64, u64, u64)> {
    use BcmTargetName::*;
    Some(match name {
        Sn11 | Sn0 => (80000, 465000, 16, 16),
        Sh0 => (80000, 465000, 32, 16),
        Mc0 => (80000, 465000, 8, 4),
        Sn1 => (0, 68570, 8, 8),
        Sh5 => (0, 30000, 16, 16),
        Sh2 | Sn15 => (0, 30000, 8, 8),
        Cn0 => (0, 30000, 4, 4),
        Acv | Ip0 => return None,
    })
}

pub fn plan(metadata: [Metadata; COUNT], before: [u32; COUNT]) -> Result<[Floor; COUNT], Error> {
    let mut result = [None; COUNT];
    for (i, name) in BcmTargetName::IPA_PATHS.into_iter().enumerate() {
        let m = metadata[i];
        let mut id = [0; 8];
        id[..name.as_str().len()].copy_from_slice(name.as_str().as_bytes());
        if m.id != id
            || m.unit_size == 0
            || m.clock_domain >= 16
            || m.reserved != 0
            || metadata[..i].iter().any(|p| p.address == m.address)
        {
            return Err(Error::Metadata);
        }
        let target = BcmTarget::new(name, m.address).map_err(|_| Error::Metadata)?;
        let old = Decoded::parse(before[i]).map_err(|_| Error::Baseline)?;
        if !old.valid && old.vector != Vector::default() {
            return Err(Error::Baseline);
        }
        let mask = name == BcmTargetName::Acv;
        let mut vector = if mask {
            // The mainline SM7150 ACV uses BIT(3), not the legacy BIT(0).
            Vector {
                x: old.vector.x,
                y: old.vector.y | 8,
            }
        } else if name == BcmTargetName::Ip0 {
            // clk-rpmh.c converts the CMD_DB unit from kHz to Hz. Its scalar
            // clock request has X=0; refuse an unexpected nonzero X baseline.
            if old.vector.x != 0 {
                return Err(Error::Baseline);
            }
            let count = (CORE_HZ / (u64::from(m.unit_size) * 1000)).max(1);
            Vector {
                x: 0,
                y: old.vector.y.max(count.min(0x3fff)),
            }
        } else {
            if m.width == 0 {
                return Err(Error::Metadata);
            }
            let (avg, peak, avg_divisor, peak_divisor) = node(name).ok_or(Error::Metadata)?;
            // Mainline bcm-voter: scale by BCM width/node width, then multiply
            // by default vote_scale=1000 and divide by the CMD_DB unit.
            let x = div(
                div(avg * u64::from(m.width), avg_divisor)? * 1000,
                u64::from(m.unit_size),
            )?;
            let y = div(
                div(peak * u64::from(m.width), peak_divisor)? * 1000,
                u64::from(m.unit_size),
            )?;
            Vector {
                x: old.vector.x.max(x.min(0x3fff)),
                y: old.vector.y.max(y.min(0x3fff)),
            }
        };
        // Enabled ICCs have nonzero floors; never send an invalid zero vote.
        if vector == Vector::default() {
            vector = Vector { x: 1, y: 1 };
        }
        let data = (1 << 30) | (1 << 29) | ((vector.x as u32) << 14) | vector.y as u32;
        result[i] = Some(Floor {
            target,
            vector,
            data,
            mask,
            vcd: m.clock_domain,
        });
    }
    let mut floors = result.map(Option::unwrap);
    // Every one-command transaction has its own commit. Keep source interconnect
    // VCD/address order and enable the independent IP0 clock after all paths.
    floors.sort_unstable_by_key(|f| {
        (
            f.target.name() == BcmTargetName::Ip0,
            f.vcd,
            f.target.address(),
        )
    });
    Ok(floors)
}

fn idle(s: handoff::Snapshot) -> bool {
    s.irq_status == 0
        && s.banks
            .iter()
            .all(|b| b.idle == 1 && b.enable == 0 && b.wait == 0 && b.control == 0)
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub submitted: u16,
    pub completed: u16,
    pub observed: [u32; COUNT],
    pub resources: [Option<BcmTargetName>; COUNT],
    pub failed_resource: Option<BcmTargetName>,
    pub error: Option<Error>,
}
pub struct PowerOnce {
    used: bool,
}
impl PowerOnce {
    pub const fn new() -> Self {
        Self { used: false }
    }

    /// Caller owns the standalone, cold-boot Apps DRV2 for this entire lifetime.
    /// The native caller retains all resources/floors after return, including
    /// partial failure. No competing writer/reset/remap or suspend is allowed.
    pub fn enable<I: RegisterIo + ReadRegisters>(
        &mut self,
        io: &mut I,
        metadata: [Metadata; COUNT],
        polls: u32,
        mut wait: impl FnMut(),
        mut progress: impl FnMut(usize, BcmTargetName),
    ) -> Report {
        let mut report = Report {
            submitted: 0,
            completed: 0,
            observed: [0; COUNT],
            resources: [None; COUNT],
            failed_resource: None,
            error: None,
        };
        let result = (|| {
            if self.used {
                return Err(Error::AlreadyStarted);
            }
            self.used = true;
            let targets = core::array::from_fn(|i| {
                BcmTarget::new(BcmTargetName::IPA_PATHS[i], metadata[i].address)
            });
            if targets.iter().any(Result::is_err) {
                return Err(Error::Metadata);
            }
            let targets = targets.map(Result::unwrap);
            let baseline =
                qcom_rpmh_bus::read_ipa_path_bcms(io, targets, polls, &mut wait, |_, _| {});
            if !baseline.passed() || baseline.responses[0] != baseline.responses[1] {
                return Err(Error::Baseline);
            }
            let before = baseline.responses[0].map(Option::unwrap);
            let floors = plan(metadata, before)?;
            let handoff = baseline.after.ok_or(Error::Handoff)?;
            if !idle(handoff) {
                return Err(Error::Handoff);
            }
            for (i, floor) in floors.into_iter().enumerate() {
                report.resources[i] = Some(floor.target.name());
                report.failed_resource = Some(floor.target.name());
                progress(i, floor.target.name());
                if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)? != handoff {
                    return Err(Error::Handoff);
                }
                let mut t = ActiveTransaction::new(
                    Command::IpaLinuxFloor {
                        target: floor.target,
                        data: floor.data,
                    },
                    polls,
                );
                // start can fail after a partial register write; no retry follows.
                report.submitted |= 1 << i;
                t.start(io).map_err(|_| Error::Controller)?;
                loop {
                    if t.poll(io).map_err(|_| Error::Controller)? {
                        break;
                    }
                    wait();
                }
                if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)? != handoff {
                    return Err(Error::Handoff);
                }
                let mut read = BcmRead::for_target(floor.target, polls);
                read.start(io).map_err(|_| Error::Controller)?;
                loop {
                    if read.poll(io).map_err(|_| Error::Controller)? {
                        break;
                    }
                    wait();
                }
                if handoff::capture_stable(io, 3).map_err(|_| Error::Handoff)? != handoff {
                    return Err(Error::Handoff);
                }
                let raw = read.response().ok_or(Error::Readback)?;
                report.observed[i] = raw;
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
                report.completed |= 1 << i;
            }
            report.failed_resource = None;
            Ok(())
        })();
        report.error = result.err();
        report
    }
}
impl Default for PowerOnce {
    fn default() -> Self {
        Self::new()
    }
}
