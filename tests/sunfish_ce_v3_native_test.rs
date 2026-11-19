#![allow(dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;
#[path = "../rust/src/qcacld_wmi.rs"]
mod qcacld_wmi;
#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_ce_v3_init.rs"]
mod qcom_ce_v3_init;
#[path = "../rust/src/qcom_ce_v3_native_io.rs"]
mod qcom_ce_v3_native_io;
#[path = "../rust/src/qcom_ce_v3_profile.rs"]
mod qcom_ce_v3_profile;
#[path = "../rust/src/qcom_ce_v3_regs.rs"]
mod qcom_ce_v3_regs;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/qcom_ufs_platform.rs"]
mod qcom_ufs_platform;
#[path = "../rust/src/qcom_wmi_ce.rs"]
mod qcom_wmi_ce;
#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_qcacld_ce_tx.rs"]
mod sunfish_qcacld_ce_tx;
#[path = "../rust/src/sunfish_qcacld_htc.rs"]
mod sunfish_qcacld_htc;
#[path = "../rust/src/sunfish_qcacld_mgmt_tx.rs"]
mod sunfish_qcacld_mgmt_tx;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;
#[path = "../rust/src/sunfish_qcacld_tx.rs"]
mod sunfish_qcacld_tx;
#[path = "../rust/src/sunfish_smmu.rs"]
mod sunfish_smmu;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;

#[cfg(test)]
mod tests {
    use super::qcom_ce_v3::MAX_CE_IOVA;
    use super::qcom_ce_v3_init::{
        ApplyError, ApplyProgress, EngineSnapshot, SetupApply, SetupEvidence, plan_profile_setup,
    };
    use super::qcom_ce_v3_native_io::{
        CE_MMIO_BYTES, CE_MMIO_PHYSICAL_BASE, CeMmioMapping, Gate, Guard, NativeIo, Observation,
        WLAN_SIDS,
    };
    use super::qcom_ce_v3_profile::{Profile, plan_rings};
    use super::qcom_ce_v3_regs::{self as regs, RegisterIo};
    use core::cell::Cell;
    use core::ptr::NonNull;
    use std::alloc::{Layout, alloc_zeroed, dealloc};
    use std::rc::Rc;

    const GENERATION: u64 = 5;

    fn empty_snapshot() -> EngineSnapshot {
        EngineSnapshot {
            generation: GENERATION,
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
        }
    }

    fn setup_plan() -> super::qcom_ce_v3_init::ProfileSetupPlan {
        let evidence = SetupEvidence {
            generation: GENERATION,
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
            iova_bytes: 0x20_000,
        };
        let rings = plan_rings(Profile::Mission, evidence.iova_base, evidence.iova_bytes).unwrap();
        plan_profile_setup(
            Profile::Mission,
            &rings,
            &[empty_snapshot(); regs::CE_COUNT],
            evidence,
        )
        .unwrap()
    }

    fn observation(virtual_base: u64) -> Observation {
        Observation {
            generation: GENERATION,
            fresh_boot: true,
            exclusive: true,
            platform_ready: true,
            ce_owner_exclusive: true,
            ce_powered: true,
            ce_reset_released: true,
            ce_quiescent: true,
            ce_irq_intid: Some(0x01c0),
            ce_irq_bound: true,
            ce_irq_exclusive: true,
            htt_ce_irq_intid: Some(0x01bf),
            htt_ce_irq_bound: true,
            htt_ce_irq_exclusive: true,
            wmi_map_generation: 0,
            wmi_host_to_target_engine: None,
            wmi_target_to_host_engine: None,
            htc_control_map_generation: 0,
            htc_control_host_to_target_engine: None,
            htc_control_target_to_host_engine: None,
            htt_map_generation: 0,
            htt_host_to_target_engine: None,
            htt_target_to_host_engine: None,
            htt_endpoint_generation: 0,
            htt_endpoint_id: None,
            htt_endpoint_max_message_size: 0,
            mmio_device_uncached: true,
            ce_mmio_virtual: virtual_base,
            ce_mmio_physical: CE_MMIO_PHYSICAL_BASE,
            ce_mmio_bytes: CE_MMIO_BYTES as u64,
            dma_virtual: virtual_base + 0x0100_0000,
            dma_iova: 0x8000_0000,
            dma_bytes: 0x20_000,
            dma_uncached: true,
            dma_exclusive: true,
            dma_translated: true,
            smmu_route_slot: Some(7),
            smmu_route_id: 0x240,
            smmu_route_mask: 1,
            smmu_route_sids: WLAN_SIDS,
            smmu_route_translated: true,
            smmu_context_bank: Some(4),
            smmu_context_s1_translation_enabled: true,
            smmu_context_enabled: true,
        }
    }

    #[derive(Clone)]
    struct MockGate {
        current: Rc<Cell<Observation>>,
        locks: Rc<Cell<usize>>,
    }

    struct MockGuard(Observation);

    impl Guard for MockGuard {
        fn observation(&self) -> Observation {
            self.0
        }
    }

    unsafe impl Gate for MockGate {
        type Guard<'a> = MockGuard;

        fn lock(&mut self) -> Result<Self::Guard<'_>, regs::Error> {
            self.locks.set(self.locks.get() + 1);
            Ok(MockGuard(self.current.get()))
        }
    }

    fn mock_gate(value: Observation) -> (MockGate, Rc<Cell<Observation>>, Rc<Cell<usize>>) {
        let current = Rc::new(Cell::new(value));
        let locks = Rc::new(Cell::new(0));
        (
            MockGate {
                current: Rc::clone(&current),
                locks: Rc::clone(&locks),
            },
            current,
            locks,
        )
    }

    struct TestMemory {
        base: NonNull<u8>,
        layout: Layout,
    }

    impl TestMemory {
        fn new() -> Self {
            // The host backing preserves absolute ICNSS register offsets; the
            // adapter receives only the CE subwindow starting at CE0_OFFSET.
            let layout = Layout::from_size_align(0x0080_0000, 4096).unwrap();
            // SAFETY: the matching layout is retained and used by Drop.
            let base = unsafe { alloc_zeroed(layout) };
            let base = NonNull::new(base).expect("test allocation failed");
            Self { base, layout }
        }

        fn mapping(&mut self) -> CeMmioMapping {
            // SAFETY: the backing allocation covers this offset and the full
            // bounded CE register aperture after it.
            let base = unsafe { self.base.as_ptr().add(regs::CE0_OFFSET) };
            CeMmioMapping {
                base,
                virtual_base: base as usize as u64,
                physical_base: CE_MMIO_PHYSICAL_BASE,
                bytes: CE_MMIO_BYTES,
            }
        }

        fn read_word(&self, index: usize) -> u32 {
            // SAFETY: test indexes are within the allocated ICNSS window.
            unsafe {
                self.base
                    .as_ptr()
                    .add(index * core::mem::size_of::<u32>())
                    .cast::<u32>()
                    .read()
            }
        }
    }

    impl Drop for TestMemory {
        fn drop(&mut self) {
            // SAFETY: `base` was allocated with this exact layout in `new`.
            unsafe { dealloc(self.base.as_ptr(), self.layout) };
        }
    }

    fn mapping(memory: &mut TestMemory) -> CeMmioMapping {
        // SAFETY: the backing allocation covers this offset and the full
        // bounded CE register aperture after it.
        let base = unsafe { memory.base.as_ptr().add(regs::CE0_OFFSET) };
        CeMmioMapping {
            base,
            virtual_base: base as usize as u64,
            physical_base: CE_MMIO_PHYSICAL_BASE,
            bytes: CE_MMIO_BYTES,
        }
    }

    #[test]
    fn native_ce_io_allows_only_setup_and_snapshot_registers() {
        let mut memory = TestMemory::new();
        let map = mapping(&mut memory);
        let (gate, _, locks) = mock_gate(observation(map.virtual_base));
        // SAFETY: the host allocation is live for this test and the mock gate
        // supplies a matching, exclusive test observation.
        let mut io = unsafe { NativeIo::new(map, GENERATION, gate) }.unwrap();
        let source_low = regs::register_spec(0, regs::Register::SourceBaseLow).unwrap();
        io.write32(source_low.offset, 0x8000_0000).unwrap();
        assert_eq!(io.read32(source_low.offset), Err(regs::Error::ReadOnly));
        assert_eq!(
            io.write32(
                regs::register_spec(0, regs::Register::Command)
                    .unwrap()
                    .offset,
                1
            ),
            Err(regs::Error::ReadOnly)
        );
        assert_eq!(
            io.write32(
                regs::register_spec(0, regs::Register::HostInterruptStatus)
                    .unwrap()
                    .offset,
                1
            ),
            Err(regs::Error::ReadOnly)
        );
        assert_eq!(memory.read_word(source_low.offset / 4), 0x8000_0000);
        assert_eq!(locks.get(), 2);
    }

    #[test]
    fn native_ce_io_rechecks_lifecycle_route_and_dma_state_for_every_access() {
        let mut memory = TestMemory::new();
        let map = mapping(&mut memory);
        let (gate, current, locks) = mock_gate(observation(map.virtual_base));
        // SAFETY: the host allocation is live for this test and the mock gate
        // supplies a matching, exclusive test observation.
        let mut io = unsafe { NativeIo::new(map, GENERATION, gate) }.unwrap();
        let source_size = regs::register_spec(0, regs::Register::SourceSize).unwrap();
        current.set(Observation {
            generation: GENERATION + 1,
            ..current.get()
        });
        assert_eq!(io.read32(source_size.offset), Err(regs::Error::Io));
        assert_eq!(locks.get(), 2);

        current.set(Observation {
            generation: GENERATION,
            smmu_route_sids: [0x240, 0x242],
            ..current.get()
        });
        assert_eq!(io.read32(source_size.offset), Err(regs::Error::Io));
        assert_eq!(locks.get(), 3);
    }

    #[test]
    fn native_ce_constructor_rejects_bypass_ambiguous_routes_and_bad_dma_windows() {
        let mut memory = TestMemory::new();
        let map = mapping(&mut memory);
        let mut bad_observations = [
            Observation {
                smmu_route_mask: 0,
                ..observation(map.virtual_base)
            },
            Observation {
                smmu_route_translated: false,
                ..observation(map.virtual_base)
            },
            Observation {
                smmu_context_enabled: false,
                ..observation(map.virtual_base)
            },
            Observation {
                smmu_context_s1_translation_enabled: false,
                ..observation(map.virtual_base)
            },
            Observation {
                dma_iova: MAX_CE_IOVA - 4096,
                dma_bytes: 8192,
                ..observation(map.virtual_base)
            },
            Observation {
                dma_uncached: false,
                ..observation(map.virtual_base)
            },
        ];
        for invalid in &mut bad_observations {
            let (gate, _, _) = mock_gate(*invalid);
            // SAFETY: the host allocation remains live; each mock observation
            // intentionally fails a required readiness or isolation check.
            assert!(unsafe { NativeIo::new(map, GENERATION, gate) }.is_err());
        }
    }

    #[test]
    fn native_ce_io_rejects_offsets_outside_the_typed_ce_register_set() {
        let mut memory = TestMemory::new();
        let map = mapping(&mut memory);
        let (gate, _, _) = mock_gate(observation(map.virtual_base));
        // SAFETY: the host allocation is live for this test and the mock gate
        // supplies a matching, exclusive test observation.
        let mut io = unsafe { NativeIo::new(map, GENERATION, gate) }.unwrap();
        assert_eq!(
            io.read32(regs::CE0_OFFSET + CE_MMIO_BYTES),
            Err(regs::Error::OutsideWindow)
        );
        assert_eq!(io.read32(regs::CE0_OFFSET + 1), Err(regs::Error::ReadOnly));
        assert_eq!(
            io.write32(regs::CE0_OFFSET + regs::CE_STRIDE * regs::CE_COUNT, 1),
            Err(regs::Error::OutsideWindow)
        );
    }

    #[test]
    fn native_ce_mapping_is_limited_to_the_twelve_page_ce_aperture() {
        assert_eq!(CE_MMIO_PHYSICAL_BASE, 0x18a4_0000);
        assert_eq!(CE_MMIO_BYTES, 0xc000);

        let mut memory = TestMemory::new();
        let mut mapping = mapping(&mut memory);
        let (gate, _, _) = mock_gate(observation(mapping.virtual_base));
        // SAFETY: the host allocation covers this test mapping. The supplied
        // broad length must be rejected before it can grant unrelated ICNSS
        // register access.
        mapping.bytes = 0x0080_0000;
        assert!(unsafe { NativeIo::new(mapping, GENERATION, gate) }.is_err());
    }

    #[test]
    fn setup_apply_uses_one_live_gate_per_write_and_completes_native_plan() {
        let mut memory = TestMemory::new();
        let map = mapping(&mut memory);
        let (gate, _, locks) = mock_gate(observation(map.virtual_base));
        // SAFETY: the host allocation is live and the mock gate pins the exact
        // mapping plus translated DMA aperture for this test.
        let mut writer = unsafe { NativeIo::new(map, GENERATION, gate) }.unwrap();
        let mut apply = SetupApply::new(setup_plan());
        let mut writes = 0;
        loop {
            match apply.step(&mut writer).unwrap() {
                ApplyProgress::Wrote { .. } => writes += 1,
                ApplyProgress::Complete => break,
            }
        }
        assert_eq!(writes, 49);
        assert_eq!(locks.get(), 1 + writes);
        let source_size = regs::register_spec(0, regs::Register::SourceSize).unwrap();
        assert_eq!(memory.read_word(source_size.offset / 4), 16);
        let destination_size = regs::register_spec(1, regs::Register::DestinationSize).unwrap();
        assert_eq!(memory.read_word(destination_size.offset / 4), 512);
        assert_eq!(apply.step(&mut writer), Ok(ApplyProgress::Complete));
        assert_eq!(locks.get(), 1 + writes);
    }

    #[test]
    fn setup_apply_detects_stale_live_snapshot_before_first_native_write() {
        let mut memory = TestMemory::new();
        let map = mapping(&mut memory);
        let (gate, _, locks) = mock_gate(observation(map.virtual_base));
        // SAFETY: the host allocation is live and the mock gate pins the exact
        // mapping plus translated DMA aperture for this test.
        let mut writer = unsafe { NativeIo::new(map, GENERATION, gate) }.unwrap();
        let source_size = regs::register_spec(0, regs::Register::SourceSize).unwrap();
        // Simulate CE state changing after the offline plan captured an empty
        // snapshot but before the live owner starts applying it.
        // SAFETY: the test mapping is valid and the offset is a CE register.
        unsafe {
            memory
                .base
                .as_ptr()
                .add(source_size.offset)
                .cast::<u32>()
                .write_volatile(1);
        }
        let mut apply = SetupApply::new(setup_plan());
        assert_eq!(apply.step(&mut writer), Err(ApplyError::LeaseLost));
        assert_eq!(apply.step(&mut writer), Err(ApplyError::Terminal));
        assert_eq!(locks.get(), 2);
        let source_low = regs::register_spec(0, regs::Register::SourceBaseLow).unwrap();
        assert_eq!(memory.read_word(source_low.offset / 4), 0);
        assert_eq!(memory.read_word(source_size.offset / 4), 1);
    }
}
