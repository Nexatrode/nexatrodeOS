#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/qcom_ufs_dma.rs"]
mod qcom_ufs_dma;
#[path = "../rust/src/qcom_ufs_platform.rs"]
mod qcom_ufs_platform;

#[path = "../rust/src/qcom_ce_v3.rs"]
mod qcom_ce_v3;
#[path = "../rust/src/qcom_ce_v3_profile.rs"]
mod qcom_ce_v3_profile;
#[path = "../rust/src/qcom_ce_v3_regs.rs"]
mod qcom_ce_v3_regs;
#[path = "../rust/src/qcom_htc.rs"]
mod qcom_htc;
#[path = "../rust/src/qcom_wmi_ce.rs"]
mod qcom_wmi_ce;
#[path = "../rust/src/sunfish_htt_rx_profile.rs"]
mod sunfish_htt_rx_profile;
#[path = "../rust/src/sunfish_htt_rx_ring.rs"]
mod sunfish_htt_rx_ring;
#[path = "../rust/src/sunfish_ipa_dma.rs"]
mod sunfish_ipa_dma;
#[path = "../rust/src/sunfish_ipa_gsi_profile.rs"]
mod sunfish_ipa_gsi_profile;
#[path = "../rust/src/sunfish_qcacld_rx.rs"]
mod sunfish_qcacld_rx;
#[path = "../rust/src/sunfish_qcacld_rx_session.rs"]
mod sunfish_qcacld_rx_session;
#[path = "../rust/src/sunfish_smmu.rs"]
mod sunfish_smmu;
#[path = "../rust/src/sunfish_smmu_exec.rs"]
mod sunfish_smmu_exec;
#[path = "../rust/src/sunfish_smmu_lpae.rs"]
mod sunfish_smmu_lpae;
#[path = "../rust/src/sunfish_smmu_program.rs"]
mod sunfish_smmu_program;
#[path = "../rust/src/sunfish_smmu_snapshot.rs"]
mod sunfish_smmu_snapshot;
#[path = "../rust/src/sunfish_wlan_dma.rs"]
mod sunfish_wlan_dma;

use qcom_ce_v3_profile::{Profile, plan_rings};
use qcom_ufs_dma::plan_ufs_dma_translation;
use qcom_ufs_platform::DmaAperture;
use sunfish_htt_rx_profile::{SUNFISH_QCACLD_CONFIG, plan_htt_rx_profile};
use sunfish_htt_rx_ring::{
    SunfishHttRxRingMemory, SunfishHttRxRingOptions, plan_sunfish_htt_rx_ring,
};
use sunfish_ipa_dma::plan_ipa_ap_dma_translation;
use sunfish_ipa_gsi_profile::{
    AP_DMA_PAGE_MAPPING_COUNT, DmaIovaWindow, DmaPool, LINUX_IPA_AP_IOVA_BYTES,
    LINUX_IPA_AP_IOVA_START, plan_ap_dma_page_mappings, plan_ap_dma_rings,
};
use sunfish_qcacld_rx_session::{Error as QcacldRxSessionError, SunfishQcacldRxSession};
use sunfish_smmu::{plan_ipa_ap_dma_route, plan_ufs_route, plan_wlan_alias_route};
use sunfish_smmu_exec::{
    ContextRegister, InstallAuthority, InstallError, InstallIo, InstallReceipt, Installer,
    MAX_TLB_SYNC_POLLS, Operation, RouteRegister,
};
use sunfish_smmu_lpae::{
    LPAE_ENTRIES_PER_TABLE, LpaeMultiTableBuffers, LpaeMultiTablePages, LpaePageMapping,
    LpaeS1Config, LpaeTablePages, RingMemoryAttribute, SMMU_LPAE_PAGE_BYTES, TableWalkPolicy,
};
use sunfish_smmu_program::{
    AccessWidth, CB_ACTLR, CB_MAIR0, CB_MAIR1, CB_SCTLR, CB_TLBIALL, CB_TLBSTATUS, CB_TLBSYNC,
    CB_TTBCR, CB_TTBCR2, CB_TTBR0, CB_TTBR1, GR0_S2CR_BASE, GR0_SMR_BASE, GR1_CBA2R_BASE,
    GR1_CBAR_BASE, InstallStep, RegisterBlock, SUNFISH_WLAN_FRESH_UNMATCHED_ACTLR,
    plan_ipa_ap_smmu_install, plan_ufs_smmu_install, plan_wlan_ce_smmu_install,
    plan_wlan_ce_smmu_install_for_fresh_unmatched_context, plan_wlan_htt_smmu_install,
    plan_wlan_htt_smmu_install_for_fresh_unmatched_context,
};
use sunfish_smmu_snapshot::{
    CapturedRoutePlan, ContextBankPreimage, RawRouteEntry, SMMU_GLOBAL_FSR,
};
use sunfish_wlan_dma::{
    HttRxBufferClass, HttRxBufferResolveError, HttRxDmaBufferWindow, WLAN_CONTROL_DMA_POOL_BYTES,
    WlanDmaPlanError, plan_wlan_ce_dma_translation, plan_wlan_htt_dma_translation,
};

const WLAN_S2CR_OFFSET: u32 = GR0_S2CR_BASE + 6 * 4;
const WLAN_SMR_OFFSET: u32 = GR0_SMR_BASE + 6 * 4;
const UFS_LEVEL3_PAGES: [u64; 1] = [0x9100_2000];

#[test]
fn linux_reference_install_keeps_its_register_policy_separate_from_lineage() {
    use sunfish_smmu_program::{RegisterProfile, plan_linux_reference_install};
    let dma = wlan_dma_plan();
    let plan = plan_linux_reference_install(dma.route(), &dma.lpae(), 3)
        .with_captured_route_preimage(CapturedRoutePlan::for_test(
            dma.route(),
            RawRouteEntry { smr: 0, s2cr: 0 },
        ))
        .unwrap()
        .with_captured_context_preimage(ContextBankPreimage::for_test(4).with_fsr_for_test(0x400))
        .unwrap();
    assert_eq!(plan.profile(), RegisterProfile::LinuxReference);
    assert!(plan.steps().contains(&InstallStep::Write {
        block: RegisterBlock::ContextBank,
        index: 4,
        offset: CB_ACTLR,
        width: AccessWidth::U32,
        value: 1,
    }));
    assert!(plan.steps().contains(&InstallStep::Write {
        block: RegisterBlock::Global0,
        index: 0,
        offset: WLAN_S2CR_OFFSET,
        width: AccessWidth::U32,
        value: 4,
    }));
    assert!(plan.steps().contains(&InstallStep::Write {
        block: RegisterBlock::ContextBank,
        index: 4,
        offset: CB_SCTLR,
        width: AccessWidth::U32,
        value: 0x1067,
    }));
    assert!(plan.steps().contains(&InstallStep::Write {
        block: RegisterBlock::ContextBank,
        index: 4,
        offset: sunfish_smmu_program::CB_S1_TLBIASID,
        width: AccessWidth::U32,
        value: dma.lpae().context().ttbr0 >> 48,
    }));
    assert!(!plan.steps().iter().any(|step| matches!(
        step,
        InstallStep::Write {
            offset: CB_TLBIALL,
            ..
        }
    )));
    assert_eq!(wlan_plan().profile(), RegisterProfile::Lineage);
}

#[test]
fn healthy_fsr_format_is_not_a_fault_but_translation_fault_still_blocks_writes() {
    for (fsr, healthy) in [(0x400, true), (0x402, false)] {
        let plan = plan_without_context_preimage()
            .with_captured_context_preimage(ContextBankPreimage::for_test(9).with_fsr_for_test(fsr))
            .unwrap();
        let mut io = FakeInstallIo {
            read_values: vec![0; 16],
            ..Default::default()
        };
        io.read_values[14] = fsr;
        let mut installer = Installer::default();
        let result = installer.install(InstallAuthority::for_test(), &mut io, &plan, 8);
        if healthy {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result,
                Err(InstallError::ContextFaultPending {
                    context_bank: 9,
                    fsr
                })
            );
            assert!(io.events.iter().all(|e| matches!(e, ObservedIo::Read(..))));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ObservedIo {
    Write32(RegisterBlock, usize, u32, u32),
    Write64(RegisterBlock, usize, u32, u64),
    Publish,
    PublishMulti,
    Bind(usize),
    Read(RegisterBlock, usize, u32, u32),
}

#[derive(Default)]
struct FakeInstallIo {
    events: Vec<ObservedIo>,
    read_values: Vec<u32>,
    next_read: usize,
    fail_at_event: Option<usize>,
}

impl FakeInstallIo {
    fn record(&mut self, event: ObservedIo) -> Result<(), usize> {
        self.events.push(event);
        let index = self.events.len() - 1;
        if self.fail_at_event == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

impl InstallIo for FakeInstallIo {
    type Error = usize;

    fn write32(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        value: u32,
    ) -> Result<(), Self::Error> {
        self.record(ObservedIo::Write32(block, index, offset, value))
    }

    fn write64(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        value: u64,
    ) -> Result<(), Self::Error> {
        self.record(ObservedIo::Write64(block, index, offset, value))
    }

    fn publish_page_tables(
        &mut self,
        _physical_pages: [u64; 4],
        _page_bytes: u64,
    ) -> Result<(), Self::Error> {
        self.record(ObservedIo::Publish)
    }

    fn publish_multi_page_tables(
        &mut self,
        _ttbr0_root: u64,
        _level2: u64,
        _level3_pages: &[u64],
        _ttbr1_root: u64,
        _page_bytes: u64,
    ) -> Result<(), Self::Error> {
        self.record(ObservedIo::PublishMulti)
    }

    fn bind_context_fault_handler(&mut self, context_bank: usize) -> Result<(), Self::Error> {
        self.record(ObservedIo::Bind(context_bank))
    }
    fn publish_root_page_tables(
        &mut self,
        _pages: [u64; 7],
        _bytes: u64,
    ) -> Result<(), Self::Error> {
        self.record(ObservedIo::PublishMulti)
    }
    fn publish_receive_root_page_tables(
        &mut self,
        _pages: [u64; 8],
        _bytes: u64,
    ) -> Result<(), Self::Error> {
        self.record(ObservedIo::PublishMulti)
    }

    fn read32(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
    ) -> Result<u32, Self::Error> {
        let value = self
            .read_values
            .get(self.next_read)
            .copied()
            .or_else(|| self.read_values.last().copied())
            .unwrap_or(0);
        self.next_read += 1;
        self.record(ObservedIo::Read(block, index, offset, value))?;
        Ok(value)
    }
}

fn htt_rx_ring_plan() -> sunfish_htt_rx_ring::SunfishHttRxRingPlan {
    let profile = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG).unwrap();
    let ring_iova = 0x8020_0000;
    let alloc_index_iova = ring_iova + 0x4000;
    let initial_buffers_iova = alloc_index_iova + 8;
    let fallback_buffers_iova = initial_buffers_iova
        + u64::from(profile.rx_buffer_bytes()) * u64::from(profile.initial_fill_level());
    plan_sunfish_htt_rx_ring(
        profile,
        SunfishHttRxRingMemory {
            ring_iova,
            alloc_index_iova,
            initial_buffers_iova,
            fallback_buffers_iova,
        },
        SunfishHttRxRingOptions {
            packet_log_enabled: false,
            global_monitor_mode: false,
        },
    )
    .unwrap()
}

fn walk_multi_lpae(
    ttbr0_root: &[u64; LPAE_ENTRIES_PER_TABLE],
    level2: &[u64; LPAE_ENTRIES_PER_TABLE],
    level3: &[u64],
    level3_pages: &[u64],
    iova: u64,
) -> Option<u64> {
    let root_index = ((iova >> 30) & 0x1ff) as usize;
    let root_entry = ttbr0_root[root_index];
    if root_entry & 0x3 != 0x3 {
        return None;
    }
    let level2_index = ((iova >> 21) & 0x1ff) as usize;
    let level2_entry = level2[level2_index];
    if level2_entry & 0x3 != 0x3 {
        return None;
    }
    let level3_table_index = level3_pages
        .iter()
        .position(|page| *page == (level2_entry & !0xfff))?;
    let level3_index = ((iova >> 12) & 0x1ff) as usize;
    let level3_entry = *level3.get(level3_table_index * LPAE_ENTRIES_PER_TABLE + level3_index)?;
    if level3_entry & 0x3 != 0x3 {
        return None;
    }
    Some((level3_entry & 0x0000_ffff_ffff_f000) | (iova & 0xfff))
}

fn plan_without_context_preimage_with_global_fsr(
    global_fsr: u32,
) -> sunfish_smmu_program::IpaApSmmuInstallPlan {
    let route = plan_ipa_ap_dma_route(true, true, true, 15, 78, 61, &[], &[7], &[9]).unwrap();
    let rings = plan_ap_dma_rings(DmaPool {
        base_iova: LINUX_IPA_AP_IOVA_START,
        bytes: 0xe000,
    })
    .unwrap();
    let physical_pages: [u64; AP_DMA_PAGE_MAPPING_COUNT] =
        core::array::from_fn(|index| 0x8800_0000 + index as u64 * 0x1000);
    let mappings = plan_ap_dma_page_mappings(
        &rings,
        DmaIovaWindow {
            base_iova: LINUX_IPA_AP_IOVA_START,
            bytes: LINUX_IPA_AP_IOVA_BYTES,
        },
        &physical_pages,
        40,
    )
    .unwrap();
    let dma = plan_ipa_ap_dma_translation(
        route,
        mappings,
        LpaeTablePages {
            ttbr0_root: 0x9000_0000,
            level2: 0x9000_1000,
            level3: 0x9000_2000,
            ttbr1_root: 0x9000_3000,
        },
        LpaeS1Config {
            input_address_bits: 39,
            output_address_bits: 40,
            asid: 2,
            table_walk_policy: TableWalkPolicy::NonCoherent,
            ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
            arm_ns_quirk: false,
        },
    )
    .unwrap();
    plan_ipa_ap_smmu_install(&dma, 0x12)
        .with_captured_route_preimage(
            CapturedRoutePlan::for_test(route, RawRouteEntry { smr: 0, s2cr: 0 })
                .with_global_fsr_for_test(global_fsr),
        )
        .unwrap()
}

fn plan_without_context_preimage() -> sunfish_smmu_program::IpaApSmmuInstallPlan {
    plan_without_context_preimage_with_global_fsr(0)
}

fn plan() -> sunfish_smmu_program::IpaApSmmuInstallPlan {
    plan_without_context_preimage()
        .with_captured_context_preimage(ContextBankPreimage::for_test(9))
        .unwrap()
}

fn wlan_dma_plan() -> sunfish_wlan_dma::WlanCeDmaPlan {
    let route = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    let rings = plan_rings(Profile::Mission, 0x8000_0000, 0x2_0000).unwrap();
    let physical_pages: [u64; 104] = core::array::from_fn(|index| {
        0xa000_0000 + index as u64 * sunfish_smmu_lpae::SMMU_LPAE_PAGE_BYTES
    });
    let dma = plan_wlan_ce_dma_translation(
        route,
        rings,
        WLAN_CONTROL_DMA_POOL_BYTES,
        &physical_pages,
        LpaeTablePages {
            ttbr0_root: 0x9000_0000,
            level2: 0x9000_1000,
            level3: 0x9000_2000,
            ttbr1_root: 0x9000_3000,
        },
        LpaeS1Config {
            input_address_bits: 39,
            output_address_bits: 40,
            asid: 3,
            table_walk_policy: TableWalkPolicy::NonCoherent,
            ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
            arm_ns_quirk: false,
        },
    )
    .unwrap();
    dma
}

fn wlan_plan() -> sunfish_smmu_program::WlanCeSmmuInstallPlan {
    plan_wlan_ce_smmu_install(&wlan_dma_plan(), 0x34)
}

#[derive(Default)]
struct FakeRegisterBackend {
    writes: Vec<InstallStep>,
    waits: Vec<InstallStep>,
    publications: Vec<InstallStep>,
    fault_bindings: Vec<InstallStep>,
}

impl FakeRegisterBackend {
    fn replay(&mut self, steps: &[InstallStep]) {
        for step in steps.iter().copied() {
            match step {
                InstallStep::Write { .. } => self.writes.push(step),
                InstallStep::WaitForClear { .. } => self.waits.push(step),
                InstallStep::PublishPageTables { .. } => self.publications.push(step),
                InstallStep::PublishMultiPageTables { .. } => self.publications.push(step),
                InstallStep::PublishRootPageTables { .. } => self.publications.push(step),
                InstallStep::PublishReceiveRootPageTables { .. } => self.publications.push(step),
                InstallStep::BindContextFaultHandler { .. } => self.fault_bindings.push(step),
            }
        }
    }
}

#[test]
fn installer_executes_publication_sync_fault_binding_and_route_in_order() {
    let mut installer = Installer::default();
    let mut io = FakeInstallIo {
        read_values: [vec![0; 16], vec![1, 1, 0]].concat(),
        ..FakeInstallIo::default()
    };

    let receipt = installer
        .install(InstallAuthority::for_test(), &mut io, &plan(), 8)
        .unwrap();

    assert_eq!(
        receipt,
        InstallReceipt {
            confirmed_writes: 15,
            table_publications: 1,
            fault_handler_bindings: 1,
            precondition_reads: 16,
            wait_reads: 3,
        }
    );
    assert!(installer.is_installed());
    assert_eq!(
        io.events[0],
        ObservedIo::Read(RegisterBlock::Global0, 0, GR0_SMR_BASE + 7 * 4, 0)
    );
    assert_eq!(
        io.events[1],
        ObservedIo::Read(RegisterBlock::Global0, 0, GR0_S2CR_BASE + 7 * 4, 0)
    );
    assert_eq!(
        io.events[2],
        ObservedIo::Read(RegisterBlock::Global1, 0, GR1_CBAR_BASE + 9 * 4, 0)
    );
    assert_eq!(
        io.events[3],
        ObservedIo::Read(RegisterBlock::Global1, 0, GR1_CBA2R_BASE + 9 * 4, 0)
    );
    assert_eq!(
        io.events[4],
        ObservedIo::Read(RegisterBlock::ContextBank, 9, CB_SCTLR, 0)
    );
    assert_eq!(
        io.events[14],
        ObservedIo::Read(
            RegisterBlock::ContextBank,
            9,
            crate::sunfish_smmu_snapshot::SMMU_CB_FSR,
            0
        )
    );
    assert_eq!(
        io.events[15],
        ObservedIo::Read(RegisterBlock::Global0, 0, SMMU_GLOBAL_FSR, 0)
    );
    assert_eq!(
        io.events[16],
        ObservedIo::Write32(RegisterBlock::ContextBank, 9, CB_SCTLR, 0)
    );
    let publication = io
        .events
        .iter()
        .position(|event| *event == ObservedIo::Publish)
        .unwrap();
    let bind = io
        .events
        .iter()
        .position(|event| *event == ObservedIo::Bind(9))
        .unwrap();
    let enable = io
        .events
        .iter()
        .position(|event| matches!(event, ObservedIo::Write32(RegisterBlock::ContextBank, 9, CB_SCTLR, value) if *value != 0))
        .unwrap();
    let s2cr = io
        .events
        .iter()
        .position(|event| matches!(event, ObservedIo::Write32(RegisterBlock::Global0, 0, offset, _) if *offset == GR0_S2CR_BASE + 7 * 4))
        .unwrap();
    let smr = io
        .events
        .iter()
        .position(|event| matches!(event, ObservedIo::Write32(RegisterBlock::Global0, 0, offset, _) if *offset == GR0_SMR_BASE + 7 * 4))
        .unwrap();
    assert!(publication < bind);
    assert!(bind < enable);
    assert!(enable < s2cr);
    assert!(s2cr < smr);
    assert_eq!(
        io.events
            .iter()
            .filter(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
            .count(),
        19
    );
    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut io, &plan(), 8),
        Err(InstallError::AlreadyInstalled)
    );
}

#[test]
fn installer_quarantines_after_uncertain_io_failure_and_refuses_replay() {
    let mut installer = Installer::default();
    let mut io = FakeInstallIo {
        // The fake records the operation before returning an error, modeling
        // an outcome where the device may already have accepted the write.
        fail_at_event: Some(16),
        ..FakeInstallIo::default()
    };

    assert!(matches!(
        installer.install(InstallAuthority::for_test(), &mut io, &plan(), 8),
        Err(InstallError::Io {
            step_index: 0,
            operation: Operation::Write32,
            confirmed_writes: 0,
            source: 16,
        })
    ));
    assert!(installer.is_quarantined());
    let event_count = io.events.len();
    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut io, &plan(), 8),
        Err(InstallError::Quarantined)
    );
    assert_eq!(io.events.len(), event_count);
}

#[test]
fn installer_bounds_tlb_wait_and_quarantines_timeout() {
    let mut installer = Installer::default();
    let mut io = FakeInstallIo {
        read_values: [vec![0; 16], vec![1]].concat(),
        ..FakeInstallIo::default()
    };

    assert!(matches!(
        installer.install(InstallAuthority::for_test(), &mut io, &plan(), 3),
        Err(InstallError::WaitTimedOut {
            mask: 1,
            last_value: 1,
            polls: 3,
            ..
        })
    ));
    assert_eq!(
        io.events
            .iter()
            .filter(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
            .count(),
        19
    );
    assert!(installer.is_quarantined());
}

#[test]
fn invalid_wait_budget_is_rejected_before_io() {
    let mut installer = Installer::default();
    let mut io = FakeInstallIo::default();

    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut io, &plan(), 0),
        Err(InstallError::InvalidPollBudget)
    );
    assert_eq!(
        installer.install(
            InstallAuthority::for_test(),
            &mut io,
            &plan(),
            MAX_TLB_SYNC_POLLS + 1,
        ),
        Err(InstallError::InvalidPollBudget)
    );
    assert!(io.events.is_empty());
    assert!(!installer.is_quarantined());
}

#[test]
fn installer_requires_and_rechecks_captured_route_preimage_before_any_write() {
    let mut installer = Installer::default();
    let mut missing_preimage_io = FakeInstallIo::default();
    assert_eq!(
        installer.install(
            InstallAuthority::for_test(),
            &mut missing_preimage_io,
            &wlan_plan(),
            8,
        ),
        Err(InstallError::MissingRoutePreimage)
    );
    assert!(missing_preimage_io.events.is_empty());

    let mut missing_context_io = FakeInstallIo::default();
    assert_eq!(
        installer.install(
            InstallAuthority::for_test(),
            &mut missing_context_io,
            &plan_without_context_preimage(),
            8,
        ),
        Err(InstallError::MissingContextPreimage)
    );
    assert!(missing_context_io.events.is_empty());

    for (read_values, register, expected, actual) in [
        (vec![1 << 31], RouteRegister::Smr, 0, 1 << 31),
        (vec![0, 1], RouteRegister::S2cr, 0, 1),
        (
            [vec![0; 15], vec![1]].concat(),
            RouteRegister::GlobalFsr,
            0,
            1,
        ),
    ] {
        let mut io = FakeInstallIo {
            read_values,
            ..FakeInstallIo::default()
        };
        assert_eq!(
            installer.install(InstallAuthority::for_test(), &mut io, &plan(), 8),
            Err(InstallError::RoutePreconditionMismatch {
                slot: 7,
                register,
                expected,
                actual,
            })
        );
        assert!(!installer.is_quarantined());
        assert!(
            io.events
                .iter()
                .all(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
        );
    }

    // A stale capture is retryable after a new capture observes the same slot
    // preimage again; the failed preflight issued no hardware writes.
    let mut fresh_io = FakeInstallIo::default();
    installer
        .install(InstallAuthority::for_test(), &mut fresh_io, &plan(), 8)
        .unwrap();
    assert!(installer.is_installed());
}

#[test]
fn pending_global_smmu_fault_blocks_install_before_any_write() {
    let fsr = 1 << 1;
    let mut installer = Installer::default();
    let plan = plan_without_context_preimage_with_global_fsr(fsr)
        .with_captured_context_preimage(ContextBankPreimage::for_test(9))
        .unwrap();
    let mut io = FakeInstallIo {
        read_values: [vec![0; 15], vec![fsr]].concat(),
        ..FakeInstallIo::default()
    };

    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut io, &plan, 8),
        Err(InstallError::GlobalFaultPending { fsr })
    );
    assert_eq!(io.events.len(), 16);
    assert_eq!(
        io.events.last(),
        Some(&ObservedIo::Read(
            RegisterBlock::Global0,
            0,
            SMMU_GLOBAL_FSR,
            fsr
        ))
    );
    assert!(
        io.events
            .iter()
            .all(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
    );
    assert!(!installer.is_quarantined());
}

#[test]
fn context_preimage_mismatch_fault_or_read_error_fails_before_writes() {
    let mut installer = Installer::default();
    let plan = plan();

    let mut stale = FakeInstallIo {
        read_values: vec![0, 0, 1],
        ..FakeInstallIo::default()
    };
    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut stale, &plan, 8),
        Err(InstallError::ContextPreconditionMismatch {
            context_bank: 9,
            register: ContextRegister::Cbar,
            expected: 0,
            actual: 1,
        })
    );
    assert!(!installer.is_quarantined());
    assert!(
        stale
            .events
            .iter()
            .all(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
    );

    let mut faulted = FakeInstallIo {
        read_values: [vec![0; 14], vec![1]].concat(),
        ..FakeInstallIo::default()
    };
    let fault_plan = plan_without_context_preimage()
        .with_captured_context_preimage(ContextBankPreimage::for_test(9).with_fsr_for_test(1))
        .unwrap();
    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut faulted, &fault_plan, 8),
        Err(InstallError::ContextFaultPending {
            context_bank: 9,
            fsr: 1,
        })
    );
    assert!(!installer.is_quarantined());
    assert!(
        faulted
            .events
            .iter()
            .all(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
    );

    let enabled_plan = plan_without_context_preimage()
        .with_captured_context_preimage(ContextBankPreimage::for_test(9).with_sctlr_for_test(1))
        .unwrap();
    let mut enabled = FakeInstallIo {
        read_values: [vec![0; 4], vec![1], vec![0; 10]].concat(),
        ..FakeInstallIo::default()
    };
    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut enabled, &enabled_plan, 8),
        Err(InstallError::ContextTranslationAlreadyEnabled {
            context_bank: 9,
            sctlr: 1,
        })
    );
    assert!(!installer.is_quarantined());
    assert!(
        enabled
            .events
            .iter()
            .all(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
    );

    let mut failed_read = FakeInstallIo {
        fail_at_event: Some(2),
        ..FakeInstallIo::default()
    };
    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut failed_read, &plan, 8),
        Err(InstallError::ContextPreconditionIo {
            context_bank: 9,
            register: ContextRegister::Cbar,
            source: 2,
        })
    );
    assert!(!installer.is_quarantined());
    assert!(
        failed_read
            .events
            .iter()
            .all(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
    );

    let mut fresh = FakeInstallIo::default();
    installer
        .install(InstallAuthority::for_test(), &mut fresh, &plan, 8)
        .unwrap();
    assert!(installer.is_installed());
}

#[test]
fn route_precondition_read_error_does_not_quarantine_or_write() {
    let mut installer = Installer::default();
    let mut io = FakeInstallIo {
        fail_at_event: Some(0),
        ..FakeInstallIo::default()
    };
    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut io, &plan(), 8),
        Err(InstallError::RoutePreconditionIo {
            slot: 7,
            register: RouteRegister::Smr,
            source: 0,
        })
    );
    assert!(!installer.is_quarantined());
    assert_eq!(io.events.len(), 1);
    assert!(matches!(io.events[0], ObservedIo::Read(_, _, _, _)));
}

#[test]
fn final_global_fault_status_read_error_does_not_quarantine_or_write() {
    let mut installer = Installer::default();
    let mut io = FakeInstallIo {
        read_values: vec![0; 15],
        fail_at_event: Some(15),
        ..FakeInstallIo::default()
    };
    assert_eq!(
        installer.install(InstallAuthority::for_test(), &mut io, &plan(), 8),
        Err(InstallError::RoutePreconditionIo {
            slot: 7,
            register: RouteRegister::GlobalFsr,
            source: 15,
        })
    );
    assert!(!installer.is_quarantined());
    assert_eq!(io.events.len(), 16);
    assert!(
        io.events
            .iter()
            .all(|event| matches!(event, ObservedIo::Read(_, _, _, _)))
    );
}

#[test]
fn install_plan_binds_only_the_captured_invalid_slot_and_matching_route() {
    let dma = wlan_dma_plan();
    let base = plan_wlan_ce_smmu_install(&dma, 0x34);
    let captured = CapturedRoutePlan::for_test(
        dma.route(),
        RawRouteEntry {
            smr: 1 << 31,
            s2cr: 0,
        },
    );
    assert_eq!(
        base.with_captured_route_preimage(captured),
        Err(sunfish_smmu_program::RoutePreimageBindingError::SlotWasAlreadyValid)
    );

    let other_route = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[5], &[4]).unwrap();
    let captured = CapturedRoutePlan::for_test(other_route, RawRouteEntry { smr: 0, s2cr: 0 });
    assert_eq!(
        base.with_captured_route_preimage(captured),
        Err(sunfish_smmu_program::RoutePreimageBindingError::RouteWritesDoNotMatchCapture)
    );
}

#[test]
fn install_plan_binds_context_preimage_to_selected_route_bank() {
    let dma = wlan_dma_plan();
    let base = plan_wlan_ce_smmu_install(&dma, 0x34);
    let captured_route =
        CapturedRoutePlan::for_test(dma.route(), RawRouteEntry { smr: 0, s2cr: 0 });
    let route_bound = base.with_captured_route_preimage(captured_route).unwrap();
    assert_eq!(
        route_bound.with_captured_context_preimage(ContextBankPreimage::for_test(5)),
        Err(sunfish_smmu_program::ContextPreimageBindingError::ContextBankDoesNotMatchRoute)
    );
    assert!(
        route_bound
            .with_captured_context_preimage(ContextBankPreimage::for_test(4))
            .is_ok()
    );
    assert_eq!(
        base.with_captured_context_preimage(ContextBankPreimage::for_test(4)),
        Err(sunfish_smmu_program::ContextPreimageBindingError::RoutePreimageRequired)
    );
}

#[test]
fn register_plan_programs_context_before_translated_route() {
    let plan = plan();
    let steps = plan.steps();
    let writes: Vec<_> = steps
        .iter()
        .filter_map(|step| match step {
            InstallStep::Write { .. } => Some(*step),
            _ => None,
        })
        .collect();

    assert_eq!(writes.len(), 15);
    assert_eq!(
        writes[0],
        InstallStep::Write {
            block: RegisterBlock::ContextBank,
            index: 9,
            offset: CB_SCTLR,
            width: AccessWidth::U32,
            value: 0,
        }
    );
    assert_eq!(
        writes
            .iter()
            .map(|write| match write {
                InstallStep::Write { block, offset, .. } => (*block, *offset),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>(),
        [
            (RegisterBlock::ContextBank, CB_SCTLR),
            (RegisterBlock::Global1, GR1_CBA2R_BASE + 9 * 4),
            (RegisterBlock::Global1, GR1_CBAR_BASE + 9 * 4),
            (RegisterBlock::ContextBank, CB_TTBCR2),
            (RegisterBlock::ContextBank, CB_TTBCR),
            (RegisterBlock::ContextBank, CB_TTBR0),
            (RegisterBlock::ContextBank, CB_TTBR1),
            (RegisterBlock::ContextBank, CB_MAIR0),
            (RegisterBlock::ContextBank, CB_MAIR1),
            (RegisterBlock::ContextBank, CB_ACTLR),
            (RegisterBlock::ContextBank, CB_TLBIALL),
            (RegisterBlock::ContextBank, CB_TLBSYNC),
            (RegisterBlock::ContextBank, CB_SCTLR),
            (RegisterBlock::Global0, GR0_S2CR_BASE + 7 * 4),
            (RegisterBlock::Global0, GR0_SMR_BASE + 7 * 4),
        ]
    );

    let (ttbr0, ttbr1, sctlr, s2cr, smr) =
        (writes[5], writes[6], writes[12], writes[13], writes[14]);
    assert!(matches!(writes[1], InstallStep::Write { value: 1, .. }));
    assert!(matches!(
        writes[2],
        InstallStep::Write { value: 0x1f300, .. }
    ));
    assert!(matches!(
        writes[3],
        InstallStep::Write {
            value: 0x0003_8012,
            ..
        }
    ));
    assert!(matches!(
        writes[4],
        InstallStep::Write {
            value: 0x0080_2019,
            ..
        }
    ));
    assert!(matches!(
        writes[7],
        InstallStep::Write {
            value: 0xf404_ff44,
            ..
        }
    ));
    assert!(matches!(writes[8], InstallStep::Write { value: 0xe4, .. }));
    assert!(matches!(writes[9], InstallStep::Write { value: 0x12, .. }));
    assert!(matches!(
        ttbr0,
        InstallStep::Write {
            width: AccessWidth::U64,
            value: 0x0002_0000_9000_0000,
            ..
        }
    ));
    assert!(matches!(
        ttbr1,
        InstallStep::Write {
            width: AccessWidth::U64,
            value: 0x0002_0000_9000_3000,
            ..
        }
    ));
    assert!(matches!(
        sctlr,
        InstallStep::Write {
            value: 0x00c0_10e7,
            ..
        }
    ));
    assert!(matches!(s2cr, InstallStep::Write { value: 0x309, .. }));
    assert!(matches!(
        smr,
        InstallStep::Write {
            value: 0x8000_0520,
            ..
        }
    ));

    let mut fake = FakeRegisterBackend::default();
    fake.replay(steps);
    assert_eq!(fake.writes, writes);
    assert_eq!(fake.waits.len(), 1);
    assert_eq!(fake.publications.len(), 1);
    assert_eq!(fake.fault_bindings.len(), 1);
}

#[test]
fn wlan_install_encodes_both_alias_sids_and_sets_up_context_before_route() {
    let plan = wlan_plan();
    let steps = plan.steps();
    let (s2cr_offset, s2cr, smr_offset, smr) = match (steps[16], steps[17]) {
        (
            InstallStep::Write {
                block: RegisterBlock::Global0,
                offset: s2cr_offset,
                value: s2cr,
                ..
            },
            InstallStep::Write {
                block: RegisterBlock::Global0,
                offset: smr_offset,
                value: smr,
                ..
            },
        ) => (s2cr_offset, s2cr, smr_offset, smr),
        other => panic!("unexpected WLAN route tail: {other:?}"),
    };
    assert_eq!(s2cr_offset, GR0_S2CR_BASE + 6 * 4);
    assert_eq!(s2cr, 0x304);
    assert_eq!(smr_offset, GR0_SMR_BASE + 6 * 4);
    assert_eq!(smr, 0x8001_0240);
    assert!(matches!(
        steps[15],
        InstallStep::Write {
            block: RegisterBlock::ContextBank,
            index: 4,
            offset: CB_SCTLR,
            value: 0x00c0_10e7,
            ..
        }
    ));
    assert!(matches!(
        steps[14],
        InstallStep::BindContextFaultHandler { context_bank: 4 }
    ));
    assert!(matches!(
        steps[10],
        InstallStep::PublishPageTables {
            physical_pages: [0x9000_0000, 0x9000_1000, 0x9000_2000, 0x9000_3000],
            page_bytes: 0x1000,
        }
    ));
}

#[test]
fn wlan_fresh_unmatched_context_helper_uses_source_derived_actlr_zero() {
    let dma = wlan_dma_plan();
    let install = plan_wlan_ce_smmu_install_for_fresh_unmatched_context(&dma);
    assert_eq!(SUNFISH_WLAN_FRESH_UNMATCHED_ACTLR, 0);
    assert!(matches!(
        install.steps()[9],
        InstallStep::Write {
            block: RegisterBlock::ContextBank,
            offset: CB_ACTLR,
            value: 0,
            ..
        }
    ));
    assert_eq!(install, plan_wlan_ce_smmu_install(&dma, 0));
}

#[test]
fn combined_ce_and_htt_map_binds_the_wlan_alias_and_publishes_every_leaf_frame() {
    let rx_ring = htt_rx_ring_plan();
    let htt_page_count = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG)
        .unwrap()
        .minimum_reserved_page_count()
        .unwrap() as usize;
    let ce_page_count = (WLAN_CONTROL_DMA_POOL_BYTES / SMMU_LPAE_PAGE_BYTES) as usize;
    let mappings: Vec<_> = (0..ce_page_count + htt_page_count)
        .map(|index| {
            if index < ce_page_count {
                LpaePageMapping {
                    iova_page: 0x8000_0000 + index as u64 * SMMU_LPAE_PAGE_BYTES,
                    physical_page: 0xa000_0000 + index as u64 * 2 * SMMU_LPAE_PAGE_BYTES,
                }
            } else {
                let htt_index = index - ce_page_count;
                LpaePageMapping {
                    iova_page: rx_ring.memory().ring_iova + htt_index as u64 * SMMU_LPAE_PAGE_BYTES,
                    physical_page: 0xb000_0000 + htt_index as u64 * 2 * SMMU_LPAE_PAGE_BYTES,
                }
            }
        })
        .collect();
    let level3_pages = [0x9100_2000, 0x9100_3000, 0x9100_4000, 0x9100_5000];
    let table_pages = LpaeMultiTablePages {
        ttbr0_root: 0x9100_0000,
        level2: 0x9100_1000,
        level3: &level3_pages,
        ttbr1_root: 0x9100_6000,
    };
    let route = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    let dma = plan_wlan_htt_dma_translation(
        route,
        rx_ring,
        &mappings,
        table_pages,
        LpaeS1Config {
            input_address_bits: 39,
            output_address_bits: 40,
            asid: 5,
            table_walk_policy: TableWalkPolicy::NonCoherent,
            ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
            arm_ns_quirk: false,
        },
    )
    .unwrap();
    assert_eq!(dma.stream_match_slot(), 6);
    assert_eq!(dma.context_bank(), 4);
    assert_eq!(dma.minimum_reserved_pages(), htt_page_count);
    assert_eq!(dma.rx_ring(), rx_ring);
    assert_eq!(dma.mappings().len(), ce_page_count + htt_page_count);
    assert_eq!(dma.lpae().level3_table_count(), 4);

    assert_eq!(
        dma.rx_buffer_window(0),
        Some(HttRxDmaBufferWindow {
            buffer_id: 0,
            iova: 0x8020_4008,
            capacity_bytes: 2176,
            class: HttRxBufferClass::Initial,
        })
    );
    let fallback_id = rx_ring.initial_fill_level();
    assert_eq!(
        dma.rx_buffer_window(fallback_id),
        Some(HttRxDmaBufferWindow {
            buffer_id: fallback_id,
            iova: 0x8064_3788,
            capacity_bytes: 2176,
            class: HttRxBufferClass::Fallback,
        })
    );
    assert_eq!(
        dma.rx_buffer_window(fallback_id + rx_ring.fallback_buffer_count()),
        None
    );

    let mut rx_session = SunfishQcacldRxSession::<4>::new().unwrap();
    rx_session.post(0).unwrap();
    let initial_lease = rx_session.take_next().unwrap();
    assert_eq!(
        dma.rx_buffer_window_for_lease(&rx_session, initial_lease),
        dma.rx_buffer_window(0)
            .ok_or(HttRxBufferResolveError::BufferIdOutOfRange(0))
    );
    rx_session.release(initial_lease).unwrap();
    assert_eq!(
        dma.rx_buffer_window_for_lease(&rx_session, initial_lease),
        Err(HttRxBufferResolveError::Session(
            QcacldRxSessionError::InvalidLease
        ))
    );

    let mut fallback_session = SunfishQcacldRxSession::<4>::new().unwrap();
    fallback_session.post(fallback_id).unwrap();
    let fallback_lease = fallback_session.take_next().unwrap();
    assert_eq!(
        dma.rx_buffer_window_for_lease(&fallback_session, fallback_lease),
        dma.rx_buffer_window(fallback_id)
            .ok_or(HttRxBufferResolveError::BufferIdOutOfRange(fallback_id))
    );

    let install = plan_wlan_htt_smmu_install(&dma, 0x34)
        .with_captured_route_preimage(CapturedRoutePlan::for_test(
            dma.route(),
            RawRouteEntry { smr: 0, s2cr: 0 },
        ))
        .unwrap()
        .with_captured_context_preimage(ContextBankPreimage::for_test(4))
        .unwrap();
    let fresh_context_install = plan_wlan_htt_smmu_install_for_fresh_unmatched_context(&dma);
    assert!(matches!(
        fresh_context_install.steps()[9],
        InstallStep::Write {
            block: RegisterBlock::ContextBank,
            offset: CB_ACTLR,
            value: 0,
            ..
        }
    ));
    assert!(matches!(
        install.steps()[10],
        InstallStep::PublishMultiPageTables {
            ttbr0_root: 0x9100_0000,
            level2: 0x9100_1000,
            level3_pages: [
                0x9100_2000,
                0x9100_3000,
                0x9100_4000,
                0x9100_5000,
                0,
                0,
                0,
                0,
            ],
            level3_page_count: 4,
            ttbr1_root: 0x9100_6000,
            page_bytes: 0x1000,
        }
    ));
    assert!(matches!(
        install.steps()[16],
        InstallStep::Write {
            block: RegisterBlock::Global0,
            offset: WLAN_S2CR_OFFSET,
            value: 0x304,
            ..
        }
    ));
    assert!(matches!(
        install.steps()[17],
        InstallStep::Write {
            block: RegisterBlock::Global0,
            offset: WLAN_SMR_OFFSET,
            value: 0x8001_0240,
            ..
        }
    ));

    let mut installer = Installer::default();
    let mut install_io = FakeInstallIo::default();
    let receipt = installer
        .install(InstallAuthority::for_test(), &mut install_io, &install, 8)
        .unwrap();
    assert_eq!(receipt.table_publications, 1);
    assert!(
        install_io
            .events
            .iter()
            .any(|event| *event == ObservedIo::PublishMulti)
    );

    let mut ttbr0_root = [0xa5; LPAE_ENTRIES_PER_TABLE];
    let mut level2 = [0xa5; LPAE_ENTRIES_PER_TABLE];
    let mut level3 = [0xa5; 4 * LPAE_ENTRIES_PER_TABLE];
    let mut ttbr1_root = [0xa5; LPAE_ENTRIES_PER_TABLE];
    let mut buffers = LpaeMultiTableBuffers {
        physical_pages: table_pages,
        ttbr0_root: &mut ttbr0_root,
        level2: &mut level2,
        level3: &mut level3,
        ttbr1_root: &mut ttbr1_root,
    };
    assert_eq!(
        dma.lpae()
            .materialize_table_image(&mut buffers)
            .unwrap()
            .written_descriptors,
        (mappings.len() + 4 + 1) as u16
    );
    for mapping in &mappings {
        assert_eq!(
            walk_multi_lpae(
                &ttbr0_root,
                &level2,
                &level3,
                &level3_pages,
                mapping.iova_page
            ),
            Some(mapping.physical_page)
        );
    }
    assert_eq!(
        walk_multi_lpae(&ttbr0_root, &level2, &level3, &level3_pages, 0x8006_8000),
        None
    );
    assert_eq!(
        walk_multi_lpae(&ttbr0_root, &level2, &level3, &level3_pages, 0x8066_6000),
        None
    );
}

#[test]
fn htt_map_rejects_fewer_than_the_source_profile_reserved_pages() {
    let rx_ring = htt_rx_ring_plan();
    let required_pages = plan_htt_rx_profile(SUNFISH_QCACLD_CONFIG)
        .unwrap()
        .minimum_reserved_page_count()
        .unwrap() as usize;
    let mappings: Vec<_> = (0..required_pages - 1)
        .map(|index| LpaePageMapping {
            iova_page: 0x8000_0000 + index as u64 * SMMU_LPAE_PAGE_BYTES,
            physical_page: 0xa100_0000 + index as u64 * 2 * SMMU_LPAE_PAGE_BYTES,
        })
        .collect();
    let level3_pages = [0x9100_2000, 0x9100_3000, 0x9100_4000];
    let table_pages = LpaeMultiTablePages {
        ttbr0_root: 0x9100_0000,
        level2: 0x9100_1000,
        level3: &level3_pages,
        ttbr1_root: 0x9100_5000,
    };
    let route = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    assert_eq!(
        plan_wlan_htt_dma_translation(
            route,
            rx_ring,
            &mappings,
            table_pages,
            LpaeS1Config {
                input_address_bits: 39,
                output_address_bits: 40,
                asid: 5,
                table_walk_policy: TableWalkPolicy::NonCoherent,
                ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
                arm_ns_quirk: false,
            },
        ),
        Err(WlanDmaPlanError::HttPoolTooSmall)
    );
}

#[test]
fn table_publication_and_fault_handler_precede_context_enable() {
    let steps = plan().steps().to_vec();
    let publication = steps
        .iter()
        .position(|step| matches!(step, InstallStep::PublishPageTables { .. }))
        .unwrap();
    let invalidate = steps
        .iter()
        .position(|step| {
            matches!(
                step,
                InstallStep::Write {
                    offset: CB_TLBIALL,
                    ..
                }
            )
        })
        .unwrap();
    let sync_wait = steps
        .iter()
        .position(|step| {
            matches!(
                step,
                InstallStep::WaitForClear {
                    offset: CB_TLBSTATUS,
                    ..
                }
            )
        })
        .unwrap();
    let fault_bind = steps
        .iter()
        .position(|step| matches!(step, InstallStep::BindContextFaultHandler { .. }))
        .unwrap();
    let enable = steps
        .iter()
        .position(|step| {
            matches!(
                step,
                InstallStep::Write {
                    offset: CB_SCTLR,
                    value: 0x00c0_10e7,
                    ..
                }
            )
        })
        .unwrap();

    assert!(publication < invalidate);
    assert!(invalidate < sync_wait);
    assert!(sync_wait < fault_bind);
    assert!(fault_bind < enable);
}

fn ufs_dma_inputs() -> (
    sunfish_smmu::RoutePlan,
    DmaAperture,
    Vec<LpaePageMapping>,
    LpaeMultiTablePages<'static>,
    LpaeS1Config,
) {
    let route = plan_ufs_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    let aperture = DmaAperture {
        base: 0x8000_0000,
        bytes: 0x10_0000,
        exclusive: true,
    };
    let mappings = (0..(aperture.bytes / SMMU_LPAE_PAGE_BYTES) as usize)
        .map(|index| LpaePageMapping {
            iova_page: aperture.base + index as u64 * SMMU_LPAE_PAGE_BYTES,
            physical_page: 0xa000_0000 + index as u64 * SMMU_LPAE_PAGE_BYTES,
        })
        .collect();
    let table_pages = LpaeMultiTablePages {
        ttbr0_root: 0x9100_0000,
        level2: 0x9100_1000,
        level3: &UFS_LEVEL3_PAGES,
        ttbr1_root: 0x9100_3000,
    };
    let config = LpaeS1Config {
        input_address_bits: 39,
        output_address_bits: 40,
        asid: 5,
        table_walk_policy: TableWalkPolicy::NonCoherent,
        ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
        arm_ns_quirk: false,
    };
    (route, aperture, mappings, table_pages, config)
}

#[test]
fn ufs_dma_composition_covers_the_full_aperture_and_preserves_exact_route() {
    let (route, aperture, mappings, table_pages, config) = ufs_dma_inputs();
    let plan = plan_ufs_dma_translation(route, aperture, &mappings, table_pages, config).unwrap();
    assert_eq!(plan.route().id(), 0x300);
    assert_eq!(plan.route().mask(), 0);
    assert_eq!(plan.stream_match_slot(), 6);
    assert_eq!(plan.context_bank(), 4);
    assert_eq!(plan.aperture(), aperture);
    assert_eq!(plan.mappings().len(), 256);
    assert_eq!(plan.lpae().mapping_count(), mappings.len());
    assert_eq!(plan.context_registers().ttbr0, 0x9100_0000 | (5u64 << 48));

    let install = plan_ufs_smmu_install(&plan, 0x1234);
    let steps = install.steps();
    assert!(matches!(
        steps[0],
        InstallStep::Write {
            block: RegisterBlock::ContextBank,
            index: 4,
            offset: CB_SCTLR,
            value: 0,
            ..
        }
    ));
    assert!(matches!(
        steps[10],
        InstallStep::PublishMultiPageTables {
            ttbr0_root: 0x9100_0000,
            level2: 0x9100_1000,
            level3_pages: [0x9100_2000, 0, 0, 0, 0, 0, 0, 0],
            level3_page_count: 1,
            ttbr1_root: 0x9100_3000,
            page_bytes: 4096,
        }
    ));
    assert!(matches!(
        steps[15],
        InstallStep::Write {
            block: RegisterBlock::ContextBank,
            index: 4,
            offset: CB_SCTLR,
            value: 0x00c0_10e7,
            ..
        }
    ));
    assert!(matches!(
        steps[16],
        InstallStep::Write {
            block: RegisterBlock::Global0,
            offset,
            value: 0x0000_0304,
            ..
        } if offset == GR0_S2CR_BASE + 6 * 4
    ));
    assert!(matches!(
        steps[17],
        InstallStep::Write {
            block: RegisterBlock::Global0,
            offset,
            value: 0x8000_0300,
            ..
        } if offset == GR0_SMR_BASE + 6 * 4
    ));
}

#[test]
fn ufs_dma_composition_rejects_wrong_route_and_incomplete_or_nonexclusive_apertures() {
    let (route, aperture, mappings, table_pages, config) = ufs_dma_inputs();
    let wlan_route = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
    assert_eq!(
        plan_ufs_dma_translation(wlan_route, aperture, &mappings, table_pages, config),
        Err(qcom_ufs_dma::Error::RouteNotUfs)
    );
    assert_eq!(
        plan_ufs_dma_translation(route, aperture, &mappings[..255], table_pages, config),
        Err(qcom_ufs_dma::Error::MappingCountMismatch)
    );
    let mut nonexclusive = aperture;
    nonexclusive.exclusive = false;
    assert_eq!(
        plan_ufs_dma_translation(route, nonexclusive, &mappings, table_pages, config),
        Err(qcom_ufs_dma::Error::InvalidAperture)
    );
    let mut shifted = mappings.clone();
    shifted[0].iova_page += SMMU_LPAE_PAGE_BYTES;
    assert_eq!(
        plan_ufs_dma_translation(route, aperture, &shifted, table_pages, config),
        Err(qcom_ufs_dma::Error::MappingNotExact)
    );
}

fn exact_linux_ipa_pair_plan() -> sunfish_smmu_program::SmmuInstallPlan {
    let first = plan_ipa_ap_dma_route(true, true, true, 15, 79, 61, &[], &[6], &[4]).unwrap();
    let peer =
        sunfish_smmu::plan_ipa_linux_peer_dma_route(true, true, true, 15, 79, 61, &[], &[7], &[4])
            .unwrap();
    let tables = sunfish_smmu_lpae::plan_lpae_s1(
        &[LpaePageMapping {
            iova_page: 0x80000000,
            physical_page: 0xa3000000,
        }],
        LpaeTablePages {
            ttbr0_root: 0xa3100000,
            level2: 0xa3101000,
            level3: 0xa3102000,
            ttbr1_root: 0xa3103000,
        },
        LpaeS1Config {
            input_address_bits: 39,
            output_address_bits: 40,
            asid: 4,
            table_walk_policy: TableWalkPolicy::NonCoherent,
            ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
            arm_ns_quirk: false,
        },
    )
    .unwrap();
    sunfish_smmu_program::plan_linux_reference_install(first, &tables, 1)
        .with_captured_route_preimage(CapturedRoutePlan::for_test(
            first,
            RawRouteEntry { smr: 0, s2cr: 0 },
        ))
        .unwrap()
        .with_captured_context_preimage(ContextBankPreimage::for_test(4))
        .unwrap()
        .with_ipa_linux_peer_route(CapturedRoutePlan::for_test(
            peer,
            RawRouteEntry { smr: 0, s2cr: 0 },
        ))
        .unwrap()
}
#[test]
fn exact_ipa_both_route_preimages_precede_one_context_install_and_two_exact_matches() {
    let p = exact_linux_ipa_pair_plan();
    let mut io = FakeInstallIo::default();
    let r = Installer::default()
        .install(InstallAuthority::for_test(), &mut io, &p, 8)
        .unwrap();
    assert_eq!(
        (
            r.confirmed_writes,
            r.table_publications,
            r.fault_handler_bindings
        ),
        (17, 1, 1)
    );
    assert!(
        io.events[..r.precondition_reads as usize]
            .iter()
            .all(|e| matches!(e, ObservedIo::Read(..)))
    );
    let matches = io
        .events
        .iter()
        .filter_map(|e| match e {
            ObservedIo::Write32(RegisterBlock::Global0, 0, off, v)
                if [0x818, 0x81c].contains(off) =>
            {
                Some((*off, *v))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(matches, [(0x818, 0x80000520), (0x81c, 0x80000522)]);
    assert_eq!(
        &io.events[..4],
        &[
            ObservedIo::Read(RegisterBlock::Global0, 0, 0x818, 0),
            ObservedIo::Read(RegisterBlock::Global0, 0, 0xc18, 0),
            ObservedIo::Read(RegisterBlock::Global0, 0, 0x81c, 0),
            ObservedIo::Read(RegisterBlock::Global0, 0, 0xc1c, 0)
        ]
    );
}
#[test]
fn uncertain_peer_writes_quarantine_the_entire_two_entry_attempt_without_replay() {
    let p = exact_linux_ipa_pair_plan();
    let mut baseline = FakeInstallIo::default();
    Installer::default()
        .install(InstallAuthority::for_test(), &mut baseline, &p, 8)
        .unwrap();
    for at in baseline.events.len() - 2..baseline.events.len() {
        let mut io = FakeInstallIo {
            fail_at_event: Some(at),
            ..Default::default()
        };
        let mut once = Installer::default();
        assert!(matches!(
            once.install(InstallAuthority::for_test(), &mut io, &p, 8),
            Err(InstallError::Io {
                step_index: 18 | 19,
                ..
            })
        ));
        assert!(once.is_quarantined());
        let count = io.events.len();
        assert_eq!(count, at + 1);
        assert_eq!(
            once.install(InstallAuthority::for_test(), &mut io, &p, 8),
            Err(InstallError::Quarantined)
        );
        assert_eq!(io.events.len(), count);
    }
}
