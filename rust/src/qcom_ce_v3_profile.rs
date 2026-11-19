//! Source-derived Adrastea CE v3 host ring profiles and bounded IOVA planning.
//!
//! Counts match QCACLD's `host_ce_config_wlan_adrastea` and its explicit
//! `HIF_LOWDESC_CE_NO_PKTLOG_CFG` alternative. Planning reserves descriptor
//! rings only; it does not allocate/map DMA, provision receive buffers, access
//! hardware, or authorize either profile for a running target.

#![forbid(unsafe_code)]

use crate::qcom_ce_v3::DESCRIPTOR_BYTES;
use crate::qcom_ce_v3_regs::CE_COUNT;

pub const DMA_ALIGNMENT: u64 = 8;
pub const PAGE_BYTES: u64 = 4096;
pub const RING_ALIGNMENT_SLACK_BYTES: u64 = 8;
pub const MAX_RING_ALLOCATIONS: usize = CE_COUNT * 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidEngine,
    InvalidIovaRange,
    AddressOverflow,
    IovaOutOfRange,
    PoolTooSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Mission,
    MissionNoPktlog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RingDirection {
    Source,
    Destination,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EngineConfig {
    pub source_entries: u16,
    pub source_max_bytes: u16,
    pub destination_entries: u16,
    pub interrupts_disabled: bool,
}

const MISSION: [EngineConfig; CE_COUNT] = [
    EngineConfig {
        source_entries: 16,
        source_max_bytes: 2048,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 512,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 128,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 32,
        source_max_bytes: 2048,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 2048,
        source_max_bytes: 256,
        destination_entries: 0,
        interrupts_disabled: true,
    },
    EngineConfig {
        source_entries: 2048,
        source_max_bytes: 512,
        destination_entries: 0,
        interrupts_disabled: true,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 0,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 0,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 0,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 512,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 512,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 512,
        interrupts_disabled: false,
    },
];

const MISSION_NO_PKTLOG: [EngineConfig; CE_COUNT] = [
    EngineConfig {
        source_entries: 16,
        source_max_bytes: 2048,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 256,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 64,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 32,
        source_max_bytes: 2048,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 2048,
        source_max_bytes: 256,
        destination_entries: 0,
        interrupts_disabled: true,
    },
    EngineConfig {
        source_entries: 2048,
        source_max_bytes: 512,
        destination_entries: 0,
        interrupts_disabled: true,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 0,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 0,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 0,
        destination_entries: 0,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 64,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 64,
        interrupts_disabled: false,
    },
    EngineConfig {
        source_entries: 0,
        source_max_bytes: 2048,
        destination_entries: 0,
        interrupts_disabled: true,
    },
];

impl Profile {
    pub const fn config(self, engine: usize) -> Result<EngineConfig, Error> {
        if engine >= CE_COUNT {
            return Err(Error::InvalidEngine);
        }
        Ok(match self {
            Self::Mission => MISSION[engine],
            Self::MissionNoPktlog => MISSION_NO_PKTLOG[engine],
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingAllocation {
    pub engine: u8,
    pub direction: RingDirection,
    pub entries: u16,
    pub descriptor_iova: u64,
    pub descriptor_bytes: u32,
    pub allocation_bytes: u32,
}

impl RingAllocation {
    const EMPTY: Self = Self {
        engine: 0,
        direction: RingDirection::Source,
        entries: 0,
        descriptor_iova: 0,
        descriptor_bytes: 0,
        allocation_bytes: 0,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RingPlan {
    allocations: [RingAllocation; MAX_RING_ALLOCATIONS],
    count: usize,
    total_bytes: u64,
}

impl RingPlan {
    pub fn allocations(&self) -> &[RingAllocation] {
        &self.allocations[..self.count]
    }

    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

/// Place every configured source and destination descriptor ring consecutively
/// in a page-bounded IOVA pool. The eight-byte tail allowance matches the
/// pinned QCACLD allocator's `nentries * desc_size + CE_DESC_RING_ALIGN` size.
pub fn plan_rings(profile: Profile, iova_base: u64, pool_bytes: u64) -> Result<RingPlan, Error> {
    if iova_base == 0
        || !iova_base.is_multiple_of(PAGE_BYTES)
        || pool_bytes == 0
        || !pool_bytes.is_multiple_of(PAGE_BYTES)
    {
        return Err(Error::InvalidIovaRange);
    }
    let pool_end = iova_base
        .checked_add(pool_bytes)
        .ok_or(Error::AddressOverflow)?;
    if pool_end > crate::qcom_ce_v3::MAX_CE_IOVA {
        return Err(Error::IovaOutOfRange);
    }

    let mut plan = RingPlan {
        allocations: [RingAllocation::EMPTY; MAX_RING_ALLOCATIONS],
        count: 0,
        total_bytes: 0,
    };

    for engine in 0..CE_COUNT {
        let config = profile.config(engine)?;
        for (direction, entries) in [
            (RingDirection::Source, config.source_entries),
            (RingDirection::Destination, config.destination_entries),
        ] {
            if entries == 0 {
                continue;
            }
            if !usize::from(entries).is_power_of_two() {
                return Err(Error::InvalidIovaRange);
            }
            let descriptor_bytes = u64::from(entries)
                .checked_mul(DESCRIPTOR_BYTES as u64)
                .ok_or(Error::AddressOverflow)?;
            let allocation_bytes = descriptor_bytes
                .checked_add(RING_ALIGNMENT_SLACK_BYTES)
                .ok_or(Error::AddressOverflow)?;
            let descriptor_iova = iova_base
                .checked_add(plan.total_bytes)
                .ok_or(Error::AddressOverflow)?;
            let allocation_end = descriptor_iova
                .checked_add(allocation_bytes)
                .ok_or(Error::AddressOverflow)?;
            if allocation_end > pool_end {
                return Err(Error::PoolTooSmall);
            }
            if !descriptor_iova.is_multiple_of(DMA_ALIGNMENT) {
                return Err(Error::InvalidIovaRange);
            }
            let allocation = plan
                .allocations
                .get_mut(plan.count)
                .ok_or(Error::PoolTooSmall)?;
            *allocation = RingAllocation {
                engine: engine as u8,
                direction,
                entries,
                descriptor_iova,
                descriptor_bytes: u32::try_from(descriptor_bytes)
                    .map_err(|_| Error::AddressOverflow)?,
                allocation_bytes: u32::try_from(allocation_bytes)
                    .map_err(|_| Error::AddressOverflow)?,
            };
            plan.count += 1;
            plan.total_bytes = plan
                .total_bytes
                .checked_add(allocation_bytes)
                .ok_or(Error::AddressOverflow)?;
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mission_profiles_match_the_adrastea_host_assignment_tables() {
        assert_eq!(Profile::Mission.config(0).unwrap().source_entries, 16);
        assert_eq!(Profile::Mission.config(0).unwrap().destination_entries, 0);
        assert_eq!(Profile::Mission.config(1).unwrap().destination_entries, 512);
        assert_eq!(Profile::Mission.config(2).unwrap().destination_entries, 128);
        assert_eq!(Profile::Mission.config(3).unwrap().source_entries, 32);
        assert_eq!(Profile::Mission.config(4).unwrap().source_entries, 2048);
        assert_eq!(Profile::Mission.config(4).unwrap().source_max_bytes, 256);
        assert_eq!(Profile::Mission.config(5).unwrap().source_max_bytes, 512);
        assert!(Profile::Mission.config(4).unwrap().interrupts_disabled);
        assert_eq!(Profile::Mission.config(6).unwrap().source_entries, 0);
        assert_eq!(Profile::Mission.config(9).unwrap().destination_entries, 512);
        assert_eq!(
            Profile::Mission.config(11).unwrap().destination_entries,
            512
        );
        assert_eq!(
            Profile::MissionNoPktlog
                .config(1)
                .unwrap()
                .destination_entries,
            256
        );
        assert_eq!(
            Profile::MissionNoPktlog
                .config(2)
                .unwrap()
                .destination_entries,
            64
        );
        assert_eq!(
            Profile::MissionNoPktlog
                .config(11)
                .unwrap()
                .destination_entries,
            0
        );
        assert!(
            Profile::MissionNoPktlog
                .config(11)
                .unwrap()
                .interrupts_disabled
        );
        assert_eq!(Profile::Mission.config(CE_COUNT), Err(Error::InvalidEngine));
    }

    #[test]
    fn mission_ring_plan_is_disjoint_aligned_and_within_the_dma_pool() {
        let plan = plan_rings(Profile::Mission, 0x8000_0000, 0x20000).unwrap();
        assert_eq!(plan.allocations().len(), 9);
        assert_eq!(plan.total_bytes(), 101_192);
        for allocation in plan.allocations() {
            assert!(allocation.descriptor_iova.is_multiple_of(DMA_ALIGNMENT));
            assert_eq!(
                allocation.descriptor_bytes,
                u32::from(allocation.entries) * DESCRIPTOR_BYTES as u32
            );
            assert_eq!(
                allocation.allocation_bytes,
                allocation.descriptor_bytes + RING_ALIGNMENT_SLACK_BYTES as u32
            );
        }
        for adjacent in plan.allocations().windows(2) {
            assert_eq!(
                adjacent[0].descriptor_iova + u64::from(adjacent[0].allocation_bytes),
                adjacent[1].descriptor_iova
            );
        }
    }

    #[test]
    fn low_memory_profile_uses_fewer_descriptors_and_reserves_less_space() {
        let mission = plan_rings(Profile::Mission, 0x8000_0000, 0x20000).unwrap();
        let low = plan_rings(Profile::MissionNoPktlog, 0x8000_0000, 0x20000).unwrap();
        assert_eq!(low.allocations().len(), 8);
        assert_eq!(low.total_bytes(), 73_536);
        assert!(low.total_bytes() < mission.total_bytes());
    }

    #[test]
    fn ring_planning_fails_closed_on_bad_or_insufficient_iova_ranges() {
        assert_eq!(
            plan_rings(Profile::Mission, 0x8000_0001, 0x10000),
            Err(Error::InvalidIovaRange)
        );
        assert_eq!(
            plan_rings(Profile::Mission, 0x8000_0000, 0x1000),
            Err(Error::PoolTooSmall)
        );
        assert_eq!(
            plan_rings(
                Profile::Mission,
                crate::qcom_ce_v3::MAX_CE_IOVA - 0x1000,
                0x2000
            ),
            Err(Error::IovaOutOfRange)
        );
        assert_eq!(
            plan_rings(Profile::Mission, u64::MAX - 0xfff, 0x1000),
            Err(Error::AddressOverflow)
        );
    }
}
