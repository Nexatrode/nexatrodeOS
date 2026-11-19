//! Ordered, source-derived register plans for translated Sunfish IPA AP and
//! WCN WLAN routes.
//!
//! The values and context-bank programming order follow the pinned LineageOS
//! `arm-smmu.c` / `arm-smmu-regs.h` implementation at commit
//! `28f9290ae067d3e7857126eb6312720dbe9c7c80`. This describes a custom
//! stage-1-translated IPA AP context; the Linux `ipa_smmu_ap` DT node itself
//! uses stage-1 bypass. The module never accesses MMIO and does not authorize
//! executing these steps.
//! `LinuxReference` below is a separate profile from the working 7.1 kernel
//! at 2267e248204e7e051e0f7355298949a6440de579. It does not inherit downstream
//! S2CR/SCTLR shareability overrides or the downstream fault-stall policy.
//!
//! Before a real owner can execute this plan, it must independently establish
//! SMMU power and exclusive control, a fresh complete route snapshot, an
//! unused context bank and match slot, owned table/data pages, required table
//! cache maintenance, and a bound context fault handler. The two route-planner
//! booleans are caller assertions, not evidence of any of those conditions.
//! The pinned Linux driver writes ACTLR zero for a new zeroed software context
//! with no matching Sunfish `qcom,actlr` entry. That does not describe live
//! hardware or a reused context: the node has `qcom,skip-init`, and context
//! destruction does not clear the stored ACTLR. The named fresh-context
//! helpers below only select this source-derived value in a pure plan; they do
//! not establish context ownership or authorize executing it.
#![forbid(unsafe_code)]

use crate::qcom_ufs_dma::UfsDmaPlan;
use crate::sunfish_ipa_dma::IpaApDmaPlan;
use crate::sunfish_smmu::RoutePlan;
use crate::sunfish_smmu_lpae::{MAX_MULTI_LPAE_LEVEL3_TABLES, SMMU_LPAE_PAGE_BYTES};
use crate::sunfish_smmu_snapshot::{CapturedRoutePlan, ContextBankPreimage, RawRouteEntry};
use crate::sunfish_wlan_dma::{WlanCeDmaPlan, WlanHttDmaPlan};

pub const GR0_SMR_BASE: u32 = 0x800;
pub const GR0_S2CR_BASE: u32 = 0xc00;
pub const GR1_CBAR_BASE: u32 = 0x000;
pub const GR1_CBA2R_BASE: u32 = 0x800;

/// Source-derived ACTLR value for a newly zeroed, unmatched Sunfish WLAN
/// context, following the pinned QSMMUv500 driver path. Runtime ACTLR and
/// reused-context state remain unknown; use only after exclusive fresh-context
/// ownership has been established by a separate platform owner.
pub const SUNFISH_WLAN_FRESH_UNMATCHED_ACTLR: u32 = 0;

pub const CB_SCTLR: u32 = 0x000;
pub const CB_ACTLR: u32 = 0x004;
pub const CB_TTBCR2: u32 = 0x010;
pub const CB_TTBR0: u32 = 0x020;
pub const CB_TTBR1: u32 = 0x028;
pub const CB_TTBCR: u32 = 0x030;
pub const CB_MAIR0: u32 = 0x038;
pub const CB_MAIR1: u32 = 0x03c;
pub const CB_TLBIALL: u32 = 0x618;
pub const CB_S1_TLBIASID: u32 = 0x610;
pub const CB_TLBSYNC: u32 = 0x7f0;
pub const CB_TLBSTATUS: u32 = 0x7f4;

const CBA2R_RW64_64BIT: u32 = 1;
const CBAR_TYPE_S1_TRANS_S2_BYPASS: u32 = 1 << 16;
const CBAR_S1_BPSHCFG_NSH: u32 = 3 << 8;
const CBAR_S1_MEMATTR_WB: u32 = 0xf << 12;
const SMR_VALID: u32 = 1 << 31;
const SMR_MASK_SHIFT: u32 = 16;
const SMR_ID_MASK: u32 = 0x7fff;
const SMR_MASK_MASK: u32 = 0x7fff;
const S2CR_SHCFG_NSH: u32 = 3 << 8;
const S2CR_TYPE_TRANS: u32 = 0;

const SCTLR_CFCFG: u32 = 1 << 7;
const SCTLR_CFIE: u32 = 1 << 6;
const SCTLR_CFRE: u32 = 1 << 5;
const SCTLR_AFE: u32 = 1 << 2;
const SCTLR_TRE: u32 = 1 << 1;
const SCTLR_M: u32 = 1;
const SCTLR_S1_ASIDPNE: u32 = 1 << 12;
const SCTLR_SHCFG_NSH: u32 = 3 << 22;
const SCTLR_S1_TRANSLATION_ENABLED: u32 = SCTLR_CFCFG
    | SCTLR_CFIE
    | SCTLR_CFRE
    | SCTLR_AFE
    | SCTLR_TRE
    | SCTLR_M
    | SCTLR_S1_ASIDPNE
    | SCTLR_SHCFG_NSH;

// Qualcomm's matched driver uses this value for context-bank TLBSYNC writes.
const QCOM_TLBSYNC_WRITE_VALUE: u32 = u32::MAX;
const TLBSTATUS_SACTIVE: u32 = 1;

/// The register address spaces used by ARM SMMUv2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterBlock {
    /// SMMU global register space 0.
    Global0,
    /// SMMU global register space 1.
    Global1,
    /// One context-bank register window, selected by `index`.
    ContextBank,
}

/// Width of one SMMU register transfer. TTBR registers require 64-bit writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessWidth {
    U32,
    U64,
}

/// One action in the planned context setup. Wait and integration steps are
/// explicit so an eventual owner cannot silently skip synchronization or
/// page-table visibility work when translating this plan into hardware I/O.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallStep {
    Write {
        block: RegisterBlock,
        index: usize,
        offset: u32,
        width: AccessWidth,
        value: u64,
    },
    PublishPageTables {
        physical_pages: [u64; 4],
        page_bytes: u64,
    },
    PublishMultiPageTables {
        ttbr0_root: u64,
        level2: u64,
        level3_pages: [u64; MAX_MULTI_LPAE_LEVEL3_TABLES],
        level3_page_count: u8,
        ttbr1_root: u64,
        page_bytes: u64,
    },
    PublishRootPageTables {
        physical_pages: [u64; 7],
        page_bytes: u64,
    },
    PublishReceiveRootPageTables {
        physical_pages: [u64; 8],
        page_bytes: u64,
    },
    BindContextFaultHandler {
        context_bank: usize,
    },
    WaitForClear {
        block: RegisterBlock,
        index: usize,
        offset: u32,
        mask: u32,
    },
}

pub const SMMU_INSTALL_STEP_COUNT: usize = 18;
pub const MAX_SMMU_INSTALL_STEP_COUNT: usize = 20;
pub const IPA_AP_SMMU_INSTALL_STEP_COUNT: usize = SMMU_INSTALL_STEP_COUNT;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterProfile {
    Lineage,
    LinuxReference,
}

impl RegisterProfile {
    const fn s2cr_attributes(self) -> u32 {
        match self {
            Self::Lineage => S2CR_SHCFG_NSH,
            Self::LinuxReference => 0,
        }
    }
    const fn sctlr(self) -> u32 {
        match self {
            Self::Lineage => SCTLR_S1_TRANSLATION_ENABLED,
            Self::LinuxReference => {
                SCTLR_CFIE | SCTLR_CFRE | SCTLR_AFE | SCTLR_TRE | SCTLR_M | SCTLR_S1_ASIDPNE
            }
        }
    }
}

/// Ordered values for one custom translated SMMUv2 context and stream route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SmmuInstallPlan {
    profile: RegisterProfile,
    steps: [InstallStep; MAX_SMMU_INSTALL_STEP_COUNT],
    step_count: usize,
    peer_route_preimage: Option<RouteSlotPreimage>,
    route_preimage: Option<RouteSlotPreimage>,
    context_preimage: Option<ContextBankPreimage>,
}

/// Exact raw route-register values observed before planning a new slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteSlotPreimage {
    slot: usize,
    context_bank: usize,
    smr: u32,
    s2cr: u32,
    global_fsr: u32,
}

impl RouteSlotPreimage {
    pub const fn slot(self) -> usize {
        self.slot
    }

    pub const fn context_bank(self) -> usize {
        self.context_bank
    }

    pub const fn smr(self) -> u32 {
        self.smr
    }

    pub const fn s2cr(self) -> u32 {
        self.s2cr
    }

    pub const fn global_fsr(self) -> u32 {
        self.global_fsr
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutePreimageBindingError {
    SlotOutOfRange,
    SlotWasAlreadyValid,
    RouteWritesDoNotMatchCapture,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextPreimageBindingError {
    RoutePreimageRequired,
    ContextBankDoesNotMatchRoute,
    ContextSetupWritesDoNotMatchCapture,
}

impl SmmuInstallPlan {
    pub const fn profile(&self) -> RegisterProfile {
        self.profile
    }
    pub fn steps(&self) -> &[InstallStep] {
        &self.steps[..self.step_count]
    }
    pub const fn peer_route_preimage(&self) -> Option<RouteSlotPreimage> {
        self.peer_route_preimage
    }

    pub const fn route_preimage(&self) -> Option<RouteSlotPreimage> {
        self.route_preimage
    }

    pub const fn context_preimage(&self) -> Option<ContextBankPreimage> {
        self.context_preimage
    }

    /// Bind this install plan to the selected raw route-slot preimage.
    ///
    /// The candidate must still be invalid, and the plan must contain exactly
    /// the expected translated S2CR and final SMR writes for the captured
    /// `RoutePlan`. The executor compares both raw registers again before its
    /// first write. This is stale-state detection, not SMMU ownership proof.
    pub fn with_captured_route_preimage(
        mut self,
        captured: CapturedRoutePlan,
    ) -> Result<Self, RoutePreimageBindingError> {
        let route = captured.route();
        let slot = route.slot();
        if slot >= crate::sunfish_smmu::SUNFISH_MAX_SMR_SLOTS {
            return Err(RoutePreimageBindingError::SlotOutOfRange);
        }
        let raw: RawRouteEntry = captured.slot_preimage();
        if raw.smr & SMR_VALID != 0 {
            return Err(RoutePreimageBindingError::SlotWasAlreadyValid);
        }

        let expected_smr = SMR_VALID
            | (route.id() & SMR_ID_MASK)
            | ((route.mask() & SMR_MASK_MASK) << SMR_MASK_SHIFT);
        let expected_s2cr =
            (route.context_bank() as u32 & 0xff) | S2CR_TYPE_TRANS | self.profile.s2cr_attributes();
        let smr_offset = GR0_SMR_BASE + (slot as u32 * 4);
        let s2cr_offset = GR0_S2CR_BASE + (slot as u32 * 4);
        if !has_one_matching_write(self.steps(), s2cr_offset, expected_s2cr)
            || !has_one_matching_write(self.steps(), smr_offset, expected_smr)
        {
            return Err(RoutePreimageBindingError::RouteWritesDoNotMatchCapture);
        }

        self.route_preimage = Some(RouteSlotPreimage {
            slot,
            context_bank: route.context_bank(),
            smr: raw.smr,
            s2cr: raw.s2cr,
            global_fsr: captured.global_fsr(),
        });
        Ok(self)
    }

    /// Add the exact 0x522 peer after a captured exact 0x520 route. Keep one
    /// context setup, two distinct inactive slots, and strict whole-register
    /// readback. The executor must check both preimages before its first write.
    pub fn with_ipa_linux_peer_route(
        mut self,
        captured: CapturedRoutePlan,
    ) -> Result<Self, RoutePreimageBindingError> {
        let first = self
            .route_preimage
            .ok_or(RoutePreimageBindingError::RouteWritesDoNotMatchCapture)?;
        let peer = captured.route();
        let raw = captured.slot_preimage();
        if self.profile != RegisterProfile::LinuxReference
            || self.peer_route_preimage.is_some()
            || self.step_count != SMMU_INSTALL_STEP_COUNT
            || peer.slot() == first.slot
            || peer.slot() >= crate::sunfish_smmu::SUNFISH_MAX_SMR_SLOTS
            || peer.id() != 0x522
            || peer.mask() != 0
            || peer.context_bank() != first.context_bank
            || captured.global_fsr() != first.global_fsr
            || raw.is_valid()
            || !has_one_matching_write(
                self.steps(),
                GR0_SMR_BASE + first.slot as u32 * 4,
                SMR_VALID | 0x520,
            )
        {
            return Err(RoutePreimageBindingError::RouteWritesDoNotMatchCapture);
        }
        let at = self.step_count;
        self.steps[at] = write32(
            RegisterBlock::Global0,
            0,
            GR0_S2CR_BASE + peer.slot() as u32 * 4,
            first.context_bank as u32,
        );
        self.steps[at + 1] = write32(
            RegisterBlock::Global0,
            0,
            GR0_SMR_BASE + peer.slot() as u32 * 4,
            SMR_VALID | 0x522,
        );
        self.step_count += 2;
        self.peer_route_preimage = Some(RouteSlotPreimage {
            slot: peer.slot(),
            context_bank: first.context_bank,
            smr: raw.smr,
            s2cr: raw.s2cr,
            global_fsr: captured.global_fsr(),
        });
        Ok(self)
    }

    /// Bind the chosen context bank to its exact register preimage. This
    /// catches stale state before any table publication or MMIO write; it is
    /// not proof that the caller owns or may reconfigure the bank.
    pub fn with_captured_context_preimage(
        mut self,
        captured: ContextBankPreimage,
    ) -> Result<Self, ContextPreimageBindingError> {
        let route = self
            .route_preimage
            .ok_or(ContextPreimageBindingError::RoutePreimageRequired)?;
        if captured.context_bank() != route.context_bank() {
            return Err(ContextPreimageBindingError::ContextBankDoesNotMatchRoute);
        }

        let bank = captured.context_bank();
        let required_writes = [
            (
                RegisterBlock::Global1,
                GR1_CBAR_BASE + bank as u32 * 4,
                AccessWidth::U32,
                1,
            ),
            (
                RegisterBlock::Global1,
                GR1_CBA2R_BASE + bank as u32 * 4,
                AccessWidth::U32,
                1,
            ),
            (RegisterBlock::ContextBank, CB_SCTLR, AccessWidth::U32, 2),
            (RegisterBlock::ContextBank, CB_ACTLR, AccessWidth::U32, 1),
            (RegisterBlock::ContextBank, CB_TTBCR2, AccessWidth::U32, 1),
            (RegisterBlock::ContextBank, CB_TTBR0, AccessWidth::U64, 1),
            (RegisterBlock::ContextBank, CB_TTBR1, AccessWidth::U64, 1),
            (RegisterBlock::ContextBank, CB_TTBCR, AccessWidth::U32, 1),
            (RegisterBlock::ContextBank, CB_MAIR0, AccessWidth::U32, 1),
            (RegisterBlock::ContextBank, CB_MAIR1, AccessWidth::U32, 1),
        ];
        if required_writes.iter().any(|(block, offset, width, count)| {
            !has_bank_writes(self.steps(), *block, bank, *offset, *width, *count)
        }) {
            return Err(ContextPreimageBindingError::ContextSetupWritesDoNotMatchCapture);
        }

        // Reject setup writes to a different bank or to an unobserved
        // configuration register. The TLB invalidation/sync registers are
        // command registers, so they are intentionally not part of the
        // stateful preimage.
        for step in self.steps() {
            let InstallStep::Write {
                block,
                index,
                offset,
                width,
                ..
            } = step
            else {
                continue;
            };
            let allowed = match block {
                RegisterBlock::Global1 => required_writes.iter().any(
                    |(required_block, required_offset, required_width, _)| {
                        block == required_block
                            && *index == 0
                            && *offset == *required_offset
                            && *width == *required_width
                    },
                ),
                RegisterBlock::ContextBank => {
                    *index == bank
                        && (required_writes.iter().any(
                            |(required_block, required_offset, required_width, _)| {
                                block == required_block
                                    && *offset == *required_offset
                                    && *width == *required_width
                            },
                        ) || (*width == AccessWidth::U32
                            && (*offset == CB_TLBIALL
                                || *offset == CB_S1_TLBIASID
                                || *offset == CB_TLBSYNC)))
                }
                RegisterBlock::Global0 => true,
            };
            if !allowed {
                return Err(ContextPreimageBindingError::ContextSetupWritesDoNotMatchCapture);
            }
        }

        self.context_preimage = Some(captured);
        Ok(self)
    }
}

fn has_bank_writes(
    steps: &[InstallStep],
    block: RegisterBlock,
    bank: usize,
    offset: u32,
    width: AccessWidth,
    expected_count: usize,
) -> bool {
    let mut matches = 0;
    let expected_index = if block == RegisterBlock::Global1 {
        0
    } else {
        bank
    };
    for step in steps {
        if let InstallStep::Write {
            block: observed_block,
            index,
            offset: observed_offset,
            width: observed_width,
            ..
        } = step
        {
            if *observed_block == block && *index == expected_index && *observed_offset == offset {
                matches += 1;
                if *observed_width != width {
                    return false;
                }
            }
        }
    }
    matches == expected_count
}

fn has_one_matching_write(steps: &[InstallStep], offset: u32, expected: u32) -> bool {
    let mut matches = 0;
    for step in steps {
        if let InstallStep::Write {
            block: RegisterBlock::Global0,
            index: 0,
            offset: observed_offset,
            width: AccessWidth::U32,
            value,
        } = step
        {
            if *observed_offset == offset {
                matches += 1;
                if *value != u64::from(expected) {
                    return false;
                }
            }
        }
    }
    matches == 1
}

pub type IpaApSmmuInstallPlan = SmmuInstallPlan;
pub type WlanCeSmmuInstallPlan = SmmuInstallPlan;
pub type UfsSmmuInstallPlan = SmmuInstallPlan;

/// Plan a context-bank and exact-SID route install from one composed IPA DMA
/// plan. `context_actlr` must come from the target's validated per-device
/// ACTLR policy; it is an explicit input because the kernel derives it from
/// device-tree data rather than a universal SMMU default.
pub fn plan_ipa_ap_smmu_install(dma: &IpaApDmaPlan, context_actlr: u32) -> IpaApSmmuInstallPlan {
    plan_smmu_install(
        dma.route(),
        dma.context_registers(),
        dma.lpae().table_pages(),
        context_actlr,
    )
}

/// Plan the same ordered context setup for the masked WCN stream alias pair.
/// The source driver places the 15-bit SMR mask in bits 16..30, so the WLAN
/// `0x240/0x1` route encodes both IDs `0x240` and `0x241` in one entry. The
/// caller must supply ACTLR from a validated owned-context policy; no-match in
/// Linux DTS is not evidence that an inherited or reused bank currently reads zero.
pub fn plan_wlan_ce_smmu_install(dma: &WlanCeDmaPlan, context_actlr: u32) -> WlanCeSmmuInstallPlan {
    plan_smmu_install(
        dma.route(),
        dma.context_registers(),
        dma.lpae().table_pages(),
        context_actlr,
    )
}

/// Plan WLAN CE translation with the source-derived ACTLR for a fresh,
/// unmatched context. This is only a convenience for the pure plan: the caller
/// must still prove exclusive SMMU ownership, a fresh unused context bank and
/// slot, and all DMA/page-table ownership before any executor can be used.
pub fn plan_wlan_ce_smmu_install_for_fresh_unmatched_context(
    dma: &WlanCeDmaPlan,
) -> WlanCeSmmuInstallPlan {
    plan_wlan_ce_smmu_install(dma, SUNFISH_WLAN_FRESH_UNMATCHED_ACTLR)
}

/// Plan SMMU publication and route steps for the multi-L3 HTT mapping. The
/// level-3 frames are copied into a fixed-capacity install record, while the
/// page-table image itself remains a separate caller-owned operation.
pub fn plan_wlan_htt_smmu_install(
    dma: &WlanHttDmaPlan<'_>,
    context_actlr: u32,
) -> WlanCeSmmuInstallPlan {
    let lpae = dma.lpae();
    let table_pages = lpae.table_pages();
    let mut level3_pages = [0; MAX_MULTI_LPAE_LEVEL3_TABLES];
    for (output, input) in level3_pages
        .iter_mut()
        .zip(table_pages.level3.iter().copied())
    {
        *output = input;
    }
    plan_smmu_install_with_publication(
        RegisterProfile::Lineage,
        dma.route(),
        dma.context_registers(),
        context_actlr,
        InstallStep::PublishMultiPageTables {
            ttbr0_root: table_pages.ttbr0_root,
            level2: table_pages.level2,
            level3_pages,
            level3_page_count: table_pages.level3.len() as u8,
            ttbr1_root: table_pages.ttbr1_root,
            page_bytes: SMMU_LPAE_PAGE_BYTES,
        },
    )
}

/// Plan the multi-L3 HTT route with the same source-derived fresh-context
/// ACTLR policy as the CE-only route. No ownership evidence is produced.
pub fn plan_wlan_htt_smmu_install_for_fresh_unmatched_context(
    dma: &WlanHttDmaPlan<'_>,
) -> WlanCeSmmuInstallPlan {
    plan_wlan_htt_smmu_install(dma, SUNFISH_WLAN_FRESH_UNMATCHED_ACTLR)
}

/// Plan publication and ordered context/route installation for the UFS DMA
/// aperture. The level-3 frames are copied into a fixed-capacity install
/// record, and the route remains the exact singleton SID `0x300` route from
/// the composed DMA plan.
pub fn plan_ufs_smmu_install(dma: &UfsDmaPlan<'_>, context_actlr: u32) -> UfsSmmuInstallPlan {
    let lpae = dma.lpae();
    let table_pages = lpae.table_pages();
    let mut level3_pages = [0; MAX_MULTI_LPAE_LEVEL3_TABLES];
    for (output, input) in level3_pages
        .iter_mut()
        .zip(table_pages.level3.iter().copied())
    {
        *output = input;
    }
    plan_smmu_install_with_publication(
        RegisterProfile::Lineage,
        dma.route(),
        dma.context_registers(),
        context_actlr,
        InstallStep::PublishMultiPageTables {
            ttbr0_root: table_pages.ttbr0_root,
            level2: table_pages.level2,
            level3_pages,
            level3_page_count: table_pages.level3.len() as u8,
            ttbr1_root: table_pages.ttbr1_root,
            page_bytes: SMMU_LPAE_PAGE_BYTES,
        },
    )
}

fn plan_smmu_install(
    route: RoutePlan,
    context: crate::sunfish_smmu_lpae::LpaeContextRegisters,
    table_pages: crate::sunfish_smmu_lpae::LpaeTablePages,
    context_actlr: u32,
) -> SmmuInstallPlan {
    plan_smmu_install_with_publication(
        RegisterProfile::Lineage,
        route,
        context,
        context_actlr,
        InstallStep::PublishPageTables {
            physical_pages: [
                table_pages.ttbr0_root,
                table_pages.level2,
                table_pages.level3,
                table_pages.ttbr1_root,
            ],
            page_bytes: SMMU_LPAE_PAGE_BYTES,
        },
    )
}

/// Pinned Linux enables CONFIG_ARM_SMMU_MMU_500_CPRE_ERRATA. Its
/// arm_mmu500_reset clears ACTLR.CPRE (bit 1) for the MMU-500 prefetch errata.
/// Apply that target-bank operation while preserving all unrelated bits;
/// do not infer a zero-initialized ACTLR or reset retained device banks.
pub const fn linux_reference_actlr(observed: u32) -> u32 {
    observed & !(1 << 1)
}

/// Build a restricted translation-domain plan using the working Linux
/// reference's register policy and caller-owned materialized tables. The
/// supplied ACTLR is the captured value; only the pinned MMU-500 CPRE erratum
/// workaround is applied. No downstream fresh-context default is substituted.
/// This plan alone grants neither SMMU access nor device/DMA readiness.
pub fn plan_linux_reference_install(
    route: RoutePlan,
    tables: &crate::sunfish_smmu_lpae::LpaeS1Plan,
    context_actlr: u32,
) -> SmmuInstallPlan {
    let pages = tables.table_pages();
    plan_smmu_install_with_publication(
        RegisterProfile::LinuxReference,
        route,
        tables.context(),
        linux_reference_actlr(context_actlr),
        InstallStep::PublishPageTables {
            physical_pages: [
                pages.ttbr0_root,
                pages.level2,
                pages.level3,
                pages.ttbr1_root,
            ],
            page_bytes: SMMU_LPAE_PAGE_BYTES,
        },
    )
}

/// Same Linux register policy, with explicit publication of all seven owned
/// two-root table frames. This pure plan establishes no hardware authority.
pub fn plan_linux_reference_root_install(
    route: RoutePlan,
    tables: &crate::sunfish_smmu_lpae::LpaeRootS1Plan<'_>,
    context_actlr: u32,
) -> SmmuInstallPlan {
    plan_smmu_install_with_publication(
        RegisterProfile::LinuxReference,
        route,
        tables.context(),
        linux_reference_actlr(context_actlr),
        InstallStep::PublishRootPageTables {
            physical_pages: tables.table_pages().pages(),
            page_bytes: SMMU_LPAE_PAGE_BYTES,
        },
    )
}

/// Full exception-RX profile with all eight owned table frames explicit.
/// Register policy/order remains the same; this is not allocation authority.
pub fn plan_linux_reference_receive_root_install(
    route: RoutePlan,
    tables: &crate::sunfish_smmu_lpae::LpaeRootS1Plan<'_, 4>,
    context_actlr: u32,
) -> SmmuInstallPlan {
    plan_smmu_install_with_publication(
        RegisterProfile::LinuxReference,
        route,
        tables.context(),
        linux_reference_actlr(context_actlr),
        InstallStep::PublishReceiveRootPageTables {
            physical_pages: tables.table_pages().pages(),
            page_bytes: SMMU_LPAE_PAGE_BYTES,
        },
    )
}

fn plan_smmu_install_with_publication(
    profile: RegisterProfile,
    route: RoutePlan,
    context: crate::sunfish_smmu_lpae::LpaeContextRegisters,
    context_actlr: u32,
    table_publication: InstallStep,
) -> SmmuInstallPlan {
    let bank = route.context_bank();
    let slot = route.slot();
    let cbar = CBAR_TYPE_S1_TRANS_S2_BYPASS | CBAR_S1_BPSHCFG_NSH | CBAR_S1_MEMATTR_WB;
    let s2cr = ((bank as u32) & 0xff) | S2CR_TYPE_TRANS | profile.s2cr_attributes();
    let smr =
        SMR_VALID | (route.id() & SMR_ID_MASK) | ((route.mask() & SMR_MASK_MASK) << SMR_MASK_SHIFT);

    let initial_steps = [
        // Keep the bank disabled while its translation state is replaced.
        write32(RegisterBlock::ContextBank, bank, CB_SCTLR, 0),
        write32(
            RegisterBlock::Global1,
            0,
            GR1_CBA2R_BASE + (bank as u32 * 4),
            CBA2R_RW64_64BIT,
        ),
        write32(
            RegisterBlock::Global1,
            0,
            GR1_CBAR_BASE + (bank as u32 * 4),
            cbar,
        ),
        write32(RegisterBlock::ContextBank, bank, CB_TTBCR2, context.ttbcr2),
        write32(
            RegisterBlock::ContextBank,
            bank,
            CB_TTBCR,
            context.tcr as u32,
        ),
        write64(RegisterBlock::ContextBank, bank, CB_TTBR0, context.ttbr0),
        write64(RegisterBlock::ContextBank, bank, CB_TTBR1, context.ttbr1),
        write32(RegisterBlock::ContextBank, bank, CB_MAIR0, context.mair0),
        write32(RegisterBlock::ContextBank, bank, CB_MAIR1, context.mair1),
        write32(RegisterBlock::ContextBank, bank, CB_ACTLR, context_actlr),
        table_publication,
        // Invalidate this bank before enabling it; complete the sync before
        // exposing the route. A real owner must implement bounded polling.
        match profile {
            RegisterProfile::Lineage => write32(RegisterBlock::ContextBank, bank, CB_TLBIALL, 0),
            RegisterProfile::LinuxReference => write32(
                RegisterBlock::ContextBank,
                bank,
                CB_S1_TLBIASID,
                (context.ttbr0 >> 48) as u32,
            ),
        },
        write32(
            RegisterBlock::ContextBank,
            bank,
            CB_TLBSYNC,
            QCOM_TLBSYNC_WRITE_VALUE,
        ),
        InstallStep::WaitForClear {
            block: RegisterBlock::ContextBank,
            index: bank,
            offset: CB_TLBSTATUS,
            mask: TLBSTATUS_SACTIVE,
        },
        InstallStep::BindContextFaultHandler { context_bank: bank },
        write32(RegisterBlock::ContextBank, bank, CB_SCTLR, profile.sctlr()),
        // Match the Linux driver: configure the translation target first,
        // then make the stream-match entry valid as the final route write.
        write32(
            RegisterBlock::Global0,
            0,
            GR0_S2CR_BASE + (slot as u32 * 4),
            s2cr,
        ),
        write32(
            RegisterBlock::Global0,
            0,
            GR0_SMR_BASE + (slot as u32 * 4),
            smr,
        ),
    ];
    let mut steps = [initial_steps[0]; MAX_SMMU_INSTALL_STEP_COUNT];
    steps[..SMMU_INSTALL_STEP_COUNT].copy_from_slice(&initial_steps);
    SmmuInstallPlan {
        profile,
        steps,
        step_count: SMMU_INSTALL_STEP_COUNT,
        route_preimage: None,
        peer_route_preimage: None,
        context_preimage: None,
    }
}

const fn write32(block: RegisterBlock, index: usize, offset: u32, value: u32) -> InstallStep {
    InstallStep::Write {
        block,
        index,
        offset,
        width: AccessWidth::U32,
        value: value as u64,
    }
}

const fn write64(block: RegisterBlock, index: usize, offset: u32, value: u64) -> InstallStep {
    InstallStep::Write {
        block,
        index,
        offset,
        width: AccessWidth::U64,
        value,
    }
}
