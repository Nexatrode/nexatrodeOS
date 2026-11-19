//! Fixed Sunfish WLAN MSA reservation profile and range checks.
//!
//! A reservation token records validated geometry for the native build. It is
//! not proof of current physical ownership, SMMU isolation, cache state, or a
//! live no-map translation. The caller must source its inputs from the trusted
//! boot-profile validator; this module never maps memory or invokes SCM.
#![forbid(unsafe_code)]

pub const SUNFISH_MSA_BASE: u64 = 0x98d0_0000;
pub const SUNFISH_MSA_BYTES: u32 = 0x0020_0000;
pub const PAGE_BYTES: u64 = 4096;
pub const MAX_EXCLUSIONS: usize = 64;
pub const VMID_HLOS: u32 = 0x03;
pub const VMID_MSS_MSA: u32 = 0x0f;
pub const VMID_WLAN: u32 = 0x18;
pub const VMID_WLAN_CE: u32 = 0x19;
pub const PERMISSION_READ_WRITE: u32 = 0x06;

const SECURE_REGION: Range = Range {
    base: SUNFISH_MSA_BASE,
    bytes: 0x4000,
};
const NONSECURE_REGION: Range = Range {
    base: SUNFISH_MSA_BASE + 0x4000,
    bytes: 0x1fc000,
};
const SUNFISH_MSA_EXCLUSIONS: [Range; 5] = [
    Range {
        base: 0x8b000000,
        bytes: 0x09800000,
    }, // modem image pool
    Range {
        base: 0x9c000000,
        bytes: 0x01600000,
    }, // splash
    Range {
        base: 0xa2000000,
        bytes: 0x00202000,
    }, // storage and both guards
    Range {
        base: 0xa2400000,
        bytes: 0x00502000,
    }, // heap and both guards
    Range {
        base: 0xa2a00000,
        bytes: 0x00003000,
    }, // SCM scratch and both guards
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Range {
    pub base: u64,
    pub bytes: u64,
}

impl Range {
    pub fn end(self) -> Option<u64> {
        self.base.checked_add(self.bytes)
    }

    fn overlaps(self, other: Self) -> bool {
        self.base < other.end().unwrap() && other.base < self.end().unwrap()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidGeneration,
    InvalidIdentity,
    WrongFixedPool,
    InvalidEvidence,
    InvalidRange,
    TooManyExclusions,
    Overlap,
    WrongRegionProfile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MsaRegion {
    pub base: u64,
    pub bytes: u32,
    pub secure: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VmPermission {
    vmid: u32,
    permissions: u32,
}

impl VmPermission {
    pub fn vmid(self) -> u32 {
        self.vmid
    }

    pub fn permissions(self) -> u32 {
        self.permissions
    }
}

/// Inert, source-derived ownership request data. This does not call SCM or
/// change secure-monitor state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferPlan {
    region: Range,
    source_vmids: [u32; 1],
    destinations: [Option<VmPermission>; 3],
    destination_count: usize,
}

impl TransferPlan {
    pub fn region(self) -> Range {
        self.region
    }

    pub fn source_vmids(self) -> [u32; 1] {
        self.source_vmids
    }

    pub fn destinations(self) -> [Option<VmPermission>; 3] {
        self.destinations
    }

    pub fn destination_count(self) -> usize {
        self.destination_count
    }
}

/// Fixed description of the two ownership transitions requested by the
/// matched Sunfish Lineage source. It does not prove live ownership; the native
/// owner must still validate the current lifecycle and exact SCM contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandoffPlan {
    generation: u64,
    identity: u64,
    transfers: [TransferPlan; 2],
}

/// One-shot evidence from the exact-once handoff coordinator that both fixed
/// ranges reached their planned ownership state. It is protocol sequencing
/// evidence only and does not represent firmware or WLAN readiness.
pub struct MsaReadyPermit {
    generation: u64,
    identity: u64,
    consumed: bool,
}

impl MsaReadyPermit {
    // Some isolated protocol test harnesses compile this module without the
    // handoff coordinator, which is the sole production minting call site.
    #[allow(dead_code)]
    pub(crate) fn from_verified_handoff(generation: u64, identity: u64) -> Self {
        Self {
            generation,
            identity,
            consumed: false,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    pub(crate) fn is_consumed(&self) -> bool {
        self.consumed
    }

    pub(crate) fn consume(&mut self) {
        self.consumed = true;
    }
}

impl HandoffPlan {
    pub fn generation(self) -> u64 {
        self.generation
    }

    pub fn identity(self) -> u64 {
        self.identity
    }

    pub fn transfers(self) -> [TransferPlan; 2] {
        self.transfers
    }
}

/// Geometry and boot-profile identity for the fixed WLAN MSA pool.
///
/// Private fields prevent callers from fabricating a differently sized pool.
/// Creating this value still does not establish live ownership; SCM remains a
/// separate, explicitly reviewed transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reservation {
    generation: u64,
    identity: u64,
}

impl Reservation {
    /// Validate the build-profile WLAN reservation against every fixed
    /// Sunfish image/pool/scratch exclusion. Build-time CapDL/DT validation
    /// remains responsible for proving that the pages are no-map and exclusive.
    pub fn validate_sunfish(generation: u64, identity: u64) -> Result<Self, Error> {
        Self::validate(
            generation,
            identity,
            SUNFISH_MSA_BASE,
            SUNFISH_MSA_BYTES,
            true,
            true,
            &SUNFISH_MSA_EXCLUSIONS,
        )
    }

    /// Validate the immutable Sunfish profile against the trusted exclusion
    /// inventory. `no_map` and `exclusive` must come from the boot owner, not
    /// from WLFW or an application request.
    pub fn validate(
        generation: u64,
        identity: u64,
        base: u64,
        bytes: u32,
        no_map: bool,
        exclusive: bool,
        excluded: &[Range],
    ) -> Result<Self, Error> {
        if generation == 0 {
            return Err(Error::InvalidGeneration);
        }
        if identity == 0 {
            return Err(Error::InvalidIdentity);
        }
        if (base, bytes) != (SUNFISH_MSA_BASE, SUNFISH_MSA_BYTES) {
            return Err(Error::WrongFixedPool);
        }
        if !no_map || !exclusive || base % PAGE_BYTES != 0 || u64::from(bytes) % PAGE_BYTES != 0 {
            return Err(Error::InvalidEvidence);
        }
        if excluded.len() > MAX_EXCLUSIONS {
            return Err(Error::TooManyExclusions);
        }
        let pool = Range {
            base,
            bytes: u64::from(bytes),
        };
        if pool.end().is_none() {
            return Err(Error::InvalidRange);
        }
        for item in excluded {
            if item.bytes == 0 || item.end().is_none() {
                return Err(Error::InvalidRange);
            }
            if pool.overlaps(*item) {
                return Err(Error::Overlap);
            }
        }
        Ok(Self {
            generation,
            identity,
        })
    }

    pub fn generation(self) -> u64 {
        self.generation
    }

    pub fn identity(self) -> u64 {
        self.identity
    }

    pub const fn base(self) -> u64 {
        SUNFISH_MSA_BASE
    }

    pub const fn bytes(self) -> u32 {
        SUNFISH_MSA_BYTES
    }

    pub fn contains(self, base: u64, bytes: u32) -> bool {
        let Some(pool_end) = self.base().checked_add(u64::from(self.bytes())) else {
            return false;
        };
        let Some(region_end) = base.checked_add(u64::from(bytes)) else {
            return false;
        };
        bytes != 0 && base >= self.base() && region_end <= pool_end
    }
}

/// Match WLFW's regions to the exact Sunfish profile and describe the fixed
/// Linux-derived VMID permissions. The result is policy data only; this
/// function performs no assignment and makes no ownership claim.
pub fn plan_handoff(reservation: Reservation, regions: &[MsaRegion]) -> Result<HandoffPlan, Error> {
    if regions.len() != 2 {
        return Err(Error::WrongRegionProfile);
    }

    let secure = regions.iter().find(|region| region.secure);
    let nonsecure = regions.iter().find(|region| !region.secure);
    let (Some(secure), Some(nonsecure)) = (secure, nonsecure) else {
        return Err(Error::WrongRegionProfile);
    };
    if (secure.base, secure.bytes) != (SECURE_REGION.base, SECURE_REGION.bytes as u32)
        || (nonsecure.base, nonsecure.bytes)
            != (NONSECURE_REGION.base, NONSECURE_REGION.bytes as u32)
        || !reservation.contains(secure.base, secure.bytes)
        || !reservation.contains(nonsecure.base, nonsecure.bytes)
    {
        return Err(Error::WrongRegionProfile);
    }

    let permission = |vmid| {
        Some(VmPermission {
            vmid,
            permissions: PERMISSION_READ_WRITE,
        })
    };
    Ok(HandoffPlan {
        generation: reservation.generation,
        identity: reservation.identity,
        transfers: [
            TransferPlan {
                region: SECURE_REGION,
                source_vmids: [VMID_HLOS],
                destinations: [permission(VMID_MSS_MSA), permission(VMID_WLAN), None],
                destination_count: 2,
            },
            TransferPlan {
                region: NONSECURE_REGION,
                source_vmids: [VMID_HLOS],
                destinations: [
                    permission(VMID_MSS_MSA),
                    permission(VMID_WLAN),
                    permission(VMID_WLAN_CE),
                ],
                destination_count: 3,
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reservation(excluded: &[Range]) -> Result<Reservation, Error> {
        Reservation::validate(
            7,
            0xabc,
            SUNFISH_MSA_BASE,
            SUNFISH_MSA_BYTES,
            true,
            true,
            excluded,
        )
    }

    fn observed_regions() -> [MsaRegion; 2] {
        [
            MsaRegion {
                base: SECURE_REGION.base,
                bytes: SECURE_REGION.bytes as u32,
                secure: true,
            },
            MsaRegion {
                base: NONSECURE_REGION.base,
                bytes: NONSECURE_REGION.bytes as u32,
                secure: false,
            },
        ]
    }

    #[test]
    fn validates_fixed_pool_and_bounded_subranges() {
        let pool = reservation(&[]).unwrap();
        assert_eq!(Reservation::validate_sunfish(7, 0xabc), Ok(pool));
        assert_eq!(
            Reservation::validate_sunfish(0, 0xabc),
            Err(Error::InvalidGeneration)
        );
        assert_eq!(pool.generation(), 7);
        assert_eq!(pool.identity(), 0xabc);
        assert!(pool.contains(SUNFISH_MSA_BASE, 0x4000));
        assert!(pool.contains(SUNFISH_MSA_BASE + 0x4000, 0x1fc000));
        assert!(!pool.contains(SUNFISH_MSA_BASE - PAGE_BYTES, 1));
        assert!(!pool.contains(SUNFISH_MSA_BASE, 0));
        assert!(!pool.contains(SUNFISH_MSA_BASE + 0x1fffff, 2));
        assert!(!pool.contains(u64::MAX - 3, 8));
    }

    #[test]
    fn rejects_untrusted_or_conflicting_profile_evidence() {
        assert_eq!(
            Reservation::validate(0, 1, SUNFISH_MSA_BASE, SUNFISH_MSA_BYTES, true, true, &[]),
            Err(Error::InvalidGeneration)
        );
        assert_eq!(
            Reservation::validate(7, 0, SUNFISH_MSA_BASE, SUNFISH_MSA_BYTES, true, true, &[]),
            Err(Error::InvalidIdentity)
        );
        assert_eq!(
            Reservation::validate(
                7,
                1,
                SUNFISH_MSA_BASE + PAGE_BYTES,
                SUNFISH_MSA_BYTES,
                true,
                true,
                &[]
            ),
            Err(Error::WrongFixedPool)
        );
        assert_eq!(
            Reservation::validate(7, 1, SUNFISH_MSA_BASE, SUNFISH_MSA_BYTES, false, true, &[]),
            Err(Error::InvalidEvidence)
        );
        assert_eq!(
            reservation(&[Range {
                base: SUNFISH_MSA_BASE + 0x1000,
                bytes: 0x1000
            }]),
            Err(Error::Overlap)
        );
        assert_eq!(
            reservation(&[Range { base: 0, bytes: 0 }]),
            Err(Error::InvalidRange)
        );
    }

    #[test]
    fn plans_only_the_exact_source_derived_sunfish_handoff() {
        let pool = reservation(&[]).unwrap();
        let plan = plan_handoff(pool, &observed_regions()).unwrap();
        assert_eq!((plan.generation(), plan.identity()), (7, 0xabc));
        let [secure, nonsecure] = plan.transfers();
        assert_eq!(secure.region(), SECURE_REGION);
        assert_eq!(secure.source_vmids(), [VMID_HLOS]);
        assert_eq!(secure.destination_count(), 2);
        assert_eq!(
            secure.destinations(),
            [
                Some(VmPermission {
                    vmid: VMID_MSS_MSA,
                    permissions: PERMISSION_READ_WRITE
                }),
                Some(VmPermission {
                    vmid: VMID_WLAN,
                    permissions: PERMISSION_READ_WRITE
                }),
                None,
            ]
        );
        assert_eq!(nonsecure.region(), NONSECURE_REGION);
        assert_eq!(nonsecure.source_vmids(), [VMID_HLOS]);
        assert_eq!(nonsecure.destination_count(), 3);
        assert_eq!(
            nonsecure.destinations(),
            [
                Some(VmPermission {
                    vmid: VMID_MSS_MSA,
                    permissions: PERMISSION_READ_WRITE
                }),
                Some(VmPermission {
                    vmid: VMID_WLAN,
                    permissions: PERMISSION_READ_WRITE
                }),
                Some(VmPermission {
                    vmid: VMID_WLAN_CE,
                    permissions: PERMISSION_READ_WRITE
                }),
            ]
        );
    }

    #[test]
    fn handoff_plan_rejects_any_region_profile_deviation() {
        let pool = reservation(&[]).unwrap();
        assert_eq!(plan_handoff(pool, &[]), Err(Error::WrongRegionProfile));
        assert_eq!(
            plan_handoff(pool, &[observed_regions()[0]]),
            Err(Error::WrongRegionProfile)
        );
        let mut regions = observed_regions();
        regions[0].bytes -= PAGE_BYTES as u32;
        assert_eq!(plan_handoff(pool, &regions), Err(Error::WrongRegionProfile));
        let mut regions = observed_regions();
        regions[0].secure = false;
        assert_eq!(plan_handoff(pool, &regions), Err(Error::WrongRegionProfile));
        let mut regions = observed_regions();
        regions.swap(0, 1);
        assert!(plan_handoff(pool, &regions).is_ok());
        let mut regions = observed_regions();
        regions[1].base += PAGE_BYTES;
        assert_eq!(plan_handoff(pool, &regions), Err(Error::WrongRegionProfile));
    }
}
