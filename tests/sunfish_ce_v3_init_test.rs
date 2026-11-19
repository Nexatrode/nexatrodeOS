#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_ce_v3_init.rs"]
mod qcom_ce_v3_init;
#[path = "../rust/src/qcom_ce_v3_profile.rs"]
mod qcom_ce_v3_profile;
#[path = "../rust/src/qcom_ce_v3_regs.rs"]
mod qcom_ce_v3_regs;

#[cfg(test)]
mod profile_plan_tests {
    use super::qcom_ce_v3_init::{
        EngineSnapshot, MAX_SETUP_WRITES, SetupEvidence, plan_profile_setup,
    };
    use super::qcom_ce_v3_profile::{Profile, plan_rings};
    use super::qcom_ce_v3_regs::CE_COUNT;

    const READY: SetupEvidence = SetupEvidence {
        generation: 7,
        fresh_boot: true,
        exclusive_ce_owner: true,
        platform_ready: true,
        ce_powered: true,
        ce_reset_released: true,
        engine_quiescent: true,
        translated_dma_ready: true,
        dma_exclusive: true,
        dma_uncached: true,
        iova_base: 0x8000_0000,
        iova_bytes: 0x20000,
    };

    const EMPTY: EngineSnapshot = EngineSnapshot {
        generation: 7,
        source_size: 0,
        destination_size: 0,
        source_read_index: 0,
        source_write_index: 0,
        destination_read_index: 0,
        destination_write_index: 0,
        source_base_high: 0,
        destination_base_high: 0,
        control1: 0,
        source_watermark: 0,
        destination_watermark: 0,
    };

    #[test]
    fn profile_plan_covers_all_engines_and_carries_the_lifecycle_generation() {
        let rings = plan_rings(Profile::Mission, READY.iova_base, READY.iova_bytes).unwrap();
        let snapshots = [EMPTY; CE_COUNT];
        let plan = plan_profile_setup(Profile::Mission, &rings, &snapshots, READY).unwrap();
        assert_eq!(plan.profile(), Profile::Mission);
        assert_eq!(plan.generation(), READY.generation);
        assert_eq!(plan.iova_base(), READY.iova_base);
        assert_eq!(plan.iova_bytes(), READY.iova_bytes);
        assert_eq!(plan.engine(CE_COUNT), None);
        assert_eq!(plan.engine(0).unwrap().writes().len(), 6);
        assert_eq!(plan.engine(1).unwrap().writes().len(), 5);
        for engine in 6..9 {
            assert_eq!(plan.engine(engine).unwrap().writes().len(), 0);
        }
        assert_eq!(MAX_SETUP_WRITES, 11);
    }

    #[test]
    fn profile_plan_rejects_profile_mismatch_and_nonempty_engine_snapshot() {
        let mission_rings =
            plan_rings(Profile::Mission, READY.iova_base, READY.iova_bytes).unwrap();
        let snapshots = [EMPTY; CE_COUNT];
        assert!(
            plan_profile_setup(Profile::MissionNoPktlog, &mission_rings, &snapshots, READY)
                .is_err()
        );

        let mut stale_snapshots = snapshots;
        stale_snapshots[1].destination_write_index = 1;
        assert_eq!(
            plan_profile_setup(Profile::Mission, &mission_rings, &stale_snapshots, READY),
            Err(super::qcom_ce_v3_init::Error::EngineNotEmpty)
        );
    }
}
