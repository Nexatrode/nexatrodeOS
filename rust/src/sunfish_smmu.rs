//! Host-testable Sunfish SMMU route planning primitives.
//!
//! This module only validates a proposed stream-match route against a supplied
//! read-only snapshot and explicitly verified free resources. It supports the
//! WCN3998 alias pair, a singleton UFS SID route, and the Sunfish IPA AP DMA
//! SID. It performs no MMIO, DMA allocation, route replacement, or ownership
//! inference.

/// Architected bounded capacity, not the implemented count. Older Sunfish
/// handoffs exposed 78 groups; the working 7.1 Linux boot reports 79. Every
/// capture and plan must use its own live ID0.NUMSMRG count within this limit.
pub const SUNFISH_MAX_SMR_SLOTS: usize = 128;
pub const SUNFISH_MAX_CONTEXT_BANKS: usize = 61;
/// Apps SMMU stream ID used by the IPA AP DMA context in Sunfish DTS.
pub const SUNFISH_IPA_AP_DMA_SID: u32 = 0x520;

/// ARM SMMUv2 context-bank fault register offsets used by the Sunfish Linux
/// driver. These are offsets within a context-bank register window, not
/// physical addresses.
pub const CONTEXT_FSR_OFFSET: u32 = 0x58;
pub const CONTEXT_FAR_OFFSET: u32 = 0x60;
pub const CONTEXT_FSYNR0_OFFSET: u32 = 0x68;

const FSR_MULTI: u32 = 1 << 31;
const FSR_SS: u32 = 1 << 30;
const FSR_UUT: u32 = 1 << 8;
const FSR_ASF: u32 = 1 << 7;
const FSR_TLBLKF: u32 = 1 << 6;
const FSR_TLBMCF: u32 = 1 << 5;
const FSR_EF: u32 = 1 << 4;
const FSR_PF: u32 = 1 << 3;
const FSR_AFF: u32 = 1 << 2;
const FSR_TF: u32 = 1 << 1;
const FSR_FAULT: u32 = FSR_MULTI
    | FSR_SS
    | FSR_UUT
    | FSR_ASF
    | FSR_TLBLKF
    | FSR_TLBMCF
    | FSR_EF
    | FSR_PF
    | FSR_AFF
    | FSR_TF;
const FSYNR0_WNR: u32 = 1 << 4;

/// A read-only snapshot of the context-bank FSR and FSYNR0 registers.
///
/// Construct this from captured register values. It deliberately has no MMIO
/// access, polling, or fault-clearing behavior; FSR is write-one-to-clear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextFaultSnapshot {
    pub fsr: u32,
    pub fsynr0: u32,
}

/// Decoded ARM SMMUv2 context-bank fault bits, preserving the raw snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextFaultStatus {
    pub raw_fsr: u32,
    pub raw_fsynr0: u32,
    pub has_fault: bool,
    pub multiple: bool,
    pub stalled: bool,
    pub unexpected_upstream: bool,
    pub address_size: bool,
    pub tlb_lock: bool,
    pub tlb_multihit: bool,
    pub external: bool,
    pub permission: bool,
    pub access_flag: bool,
    pub translation: bool,
    pub write: bool,
}

impl ContextFaultSnapshot {
    pub const fn decode(self) -> ContextFaultStatus {
        ContextFaultStatus {
            raw_fsr: self.fsr,
            raw_fsynr0: self.fsynr0,
            has_fault: self.fsr & FSR_FAULT != 0,
            multiple: self.fsr & FSR_MULTI != 0,
            stalled: self.fsr & FSR_SS != 0,
            unexpected_upstream: self.fsr & FSR_UUT != 0,
            address_size: self.fsr & FSR_ASF != 0,
            tlb_lock: self.fsr & FSR_TLBLKF != 0,
            tlb_multihit: self.fsr & FSR_TLBMCF != 0,
            external: self.fsr & FSR_EF != 0,
            permission: self.fsr & FSR_PF != 0,
            access_flag: self.fsr & FSR_AFF != 0,
            translation: self.fsr & FSR_TF != 0,
            write: self.fsynr0 & FSYNR0_WNR != 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservedRoute {
    pub slot: usize,
    pub id: u32,
    pub mask: u32,
    pub context_bank: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutePlan {
    slot: usize,
    id: u32,
    mask: u32,
    context_bank: usize,
    matched_sids: [u32; 2],
    matched_sid_count: usize,
}

impl RoutePlan {
    pub const fn slot(self) -> usize {
        self.slot
    }

    pub const fn id(self) -> u32 {
        self.id
    }

    pub const fn mask(self) -> u32 {
        self.mask
    }

    pub const fn context_bank(self) -> usize {
        self.context_bank
    }

    pub const fn matched_sids(self) -> [u32; 2] {
        self.matched_sids
    }

    pub const fn matched_sid_count(self) -> usize {
        self.matched_sid_count
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnbindAction {
    KeepSharedRoute,
    ReleaseRoute,
}

/// Models the capability references to the single masked WLAN route.
///
/// A real seL4 integration must couple these transitions to validated SID/CB
/// caps and hardware ownership. This value is only the tested bookkeeping
/// policy; it never programs the SMMU.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WlanAliasBindings {
    slot: usize,
    context_bank: usize,
    bound_sids: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteError {
    RouteSnapshotNotFreshAndComplete,
    ControlNotDelegated,
    OwnershipNotTransferred,
    InvalidStreamIdWidth,
    InvalidHardwareBounds,
    CandidateOutOfRange,
    AliasSetInvalid,
    ObservedSlotOutOfRange,
    DuplicateObservedSlot,
    ObservedRouteOutOfRange,
    RouteOverlap,
    NoVerifiedFreeSlot,
    NoVerifiedFreeContextBank,
    InvalidRoutePlan,
    SidNotInAliasSet,
    SidAlreadyBound,
    SidNotBound,
    ContextBankMismatch,
}

impl WlanAliasBindings {
    pub fn from_plan(plan: RoutePlan) -> Result<Self, RouteError> {
        if plan.slot >= SUNFISH_MAX_SMR_SLOTS
            || plan.context_bank >= SUNFISH_MAX_CONTEXT_BANKS
            || plan.id != 0x240
            || plan.mask != 1
            || plan.matched_sids != [0x240, 0x241]
            || plan.matched_sid_count != 2
        {
            return Err(RouteError::InvalidRoutePlan);
        }
        Ok(Self {
            slot: plan.slot,
            context_bank: plan.context_bank,
            bound_sids: 0,
        })
    }

    /// Record one SID capability binding to the planned context bank.
    pub fn bind(&mut self, sid: u32, context_bank: usize) -> Result<(), RouteError> {
        let bit = sid_bit(sid).ok_or(RouteError::SidNotInAliasSet)?;
        if context_bank != self.context_bank {
            return Err(RouteError::ContextBankMismatch);
        }
        if self.bound_sids & bit != 0 {
            return Err(RouteError::SidAlreadyBound);
        }
        self.bound_sids |= bit;
        Ok(())
    }

    /// Remove one SID capability reference and say whether hardware teardown
    /// may be considered. The caller still needs its ownership/serialization
    /// proof before applying that action to registers.
    pub fn unbind(&mut self, sid: u32) -> Result<UnbindAction, RouteError> {
        let bit = sid_bit(sid).ok_or(RouteError::SidNotInAliasSet)?;
        if self.bound_sids & bit == 0 {
            return Err(RouteError::SidNotBound);
        }
        self.bound_sids &= !bit;
        if self.bound_sids == 0 {
            Ok(UnbindAction::ReleaseRoute)
        } else {
            Ok(UnbindAction::KeepSharedRoute)
        }
    }

    pub const fn slot(self) -> usize {
        self.slot
    }

    pub const fn context_bank(self) -> usize {
        self.context_bank
    }

    pub const fn is_route_referenced(self) -> bool {
        self.bound_sids != 0
    }
}

const fn sid_bit(sid: u32) -> Option<u8> {
    match sid {
        0x240 => Some(1),
        0x241 => Some(2),
        _ => None,
    }
}

/// Plan one masked route for the two known WCN3998 stream IDs (0x240/0x241).
///
/// The SIDs remain full hardware identities. `slot` is selected independently
/// from the bounded SMR/S2CR table, and both alias SIDs intentionally share it.
/// Callers must supply a fresh complete route table and establish that
/// ownership is delegated and the listed free resources are authoritative
/// before using this plan for any later operation. A targeted SID readback is
/// not a complete route table.
pub fn plan_wlan_alias_route(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    route_snapshot_fresh_and_complete: bool,
    stream_id_width: u8,
    smr_slot_count: usize,
    context_bank_count: usize,
    observed: &[ObservedRoute],
    verified_free_slots: &[usize],
    verified_free_context_banks: &[usize],
) -> Result<RoutePlan, RouteError> {
    plan_route(
        smmu_control_delegated,
        ownership_transferred,
        route_snapshot_fresh_and_complete,
        stream_id_width,
        smr_slot_count,
        context_bank_count,
        observed,
        verified_free_slots,
        verified_free_context_banks,
        0x240,
        0x1,
        [0x240, 0x241],
        2,
    )
}

/// Plan one exact translated route for the Sunfish UFS stream ID `0x300`.
///
/// Callers must provide a fresh complete route table; a targeted readback is
/// insufficient. This never removes or replaces an existing stream match. In particular,
/// the saved `0x300/0xf` ABL route overlaps this plan and must first be retired
/// by a separately authorized owner, then absent from a fresh route snapshot.
pub fn plan_ufs_route(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    route_snapshot_fresh_and_complete: bool,
    stream_id_width: u8,
    smr_slot_count: usize,
    context_bank_count: usize,
    observed: &[ObservedRoute],
    verified_free_slots: &[usize],
    verified_free_context_banks: &[usize],
) -> Result<RoutePlan, RouteError> {
    plan_route(
        smmu_control_delegated,
        ownership_transferred,
        route_snapshot_fresh_and_complete,
        stream_id_width,
        smr_slot_count,
        context_bank_count,
        observed,
        verified_free_slots,
        verified_free_context_banks,
        u32::from(crate::qcom_ufs_platform::SMMU_STREAM_ID),
        0,
        [u32::from(crate::qcom_ufs_platform::SMMU_STREAM_ID), 0],
        1,
    )
}

/// Plan one exact translated Apps-SMMU route for the IPA AP DMA master.
///
/// Sunfish DTS binds `ipa_smmu_ap` to SID `0x520`, with no stream-mask bits.
/// Lineage configures this context for stage-1 bypass, which is not suitable
/// for an isolated seL4 ring pool. This planner requires a fresh complete
/// snapshot and explicitly delegated control/ownership, selects only verified
/// free resources, and refuses any existing match that overlaps this SID. It
/// does not program the route or build the context-bank page tables.
pub fn plan_ipa_ap_dma_route(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    route_snapshot_fresh_and_complete: bool,
    stream_id_width: u8,
    smr_slot_count: usize,
    context_bank_count: usize,
    observed: &[ObservedRoute],
    verified_free_slots: &[usize],
    verified_free_context_banks: &[usize],
) -> Result<RoutePlan, RouteError> {
    plan_route(
        smmu_control_delegated,
        ownership_transferred,
        route_snapshot_fresh_and_complete,
        stream_id_width,
        smr_slot_count,
        context_bank_count,
        observed,
        verified_free_slots,
        verified_free_context_banks,
        SUNFISH_IPA_AP_DMA_SID,
        0,
        [SUNFISH_IPA_AP_DMA_SID, 0],
        1,
    )
}

/// The working mainline SM7150 DT binds IPA SIDs 0x520 and 0x522 to one
/// domain. Mask bit 1 combines exactly those two identities, excluding
/// 0x521/0x523. This profile is separate from the legacy single-0x520 route.
/// All ownership, completeness, overlap and free-resource checks still apply.
#[allow(clippy::too_many_arguments)]
pub fn plan_ipa_linux_dma_route(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    route_snapshot_fresh_and_complete: bool,
    stream_id_width: u8,
    smr_slot_count: usize,
    context_bank_count: usize,
    observed: &[ObservedRoute],
    verified_free_slots: &[usize],
    verified_free_context_banks: &[usize],
) -> Result<RoutePlan, RouteError> {
    plan_route(
        smmu_control_delegated,
        ownership_transferred,
        route_snapshot_fresh_and_complete,
        stream_id_width,
        smr_slot_count,
        context_bank_count,
        observed,
        verified_free_slots,
        verified_free_context_banks,
        0x520,
        0x2,
        [0x520, 0x522],
        2,
    )
}

/// Second exact SID of the pinned mainline IPA domain. A two-entry caller
/// supplies the same unused context as the primary 0x520 entry and a distinct
/// unused slot. No mask bits or runtime SID choice are accepted here.
#[allow(clippy::too_many_arguments)]
pub fn plan_ipa_linux_peer_dma_route(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    route_snapshot_fresh_and_complete: bool,
    stream_id_width: u8,
    smr_slot_count: usize,
    context_bank_count: usize,
    observed: &[ObservedRoute],
    verified_free_slots: &[usize],
    verified_free_context_banks: &[usize],
) -> Result<RoutePlan, RouteError> {
    plan_route(
        smmu_control_delegated,
        ownership_transferred,
        route_snapshot_fresh_and_complete,
        stream_id_width,
        smr_slot_count,
        context_bank_count,
        observed,
        verified_free_slots,
        verified_free_context_banks,
        0x522,
        0,
        [0x522, 0],
        1,
    )
}

#[allow(clippy::too_many_arguments)]
fn plan_route(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    route_snapshot_fresh_and_complete: bool,
    stream_id_width: u8,
    smr_slot_count: usize,
    context_bank_count: usize,
    observed: &[ObservedRoute],
    verified_free_slots: &[usize],
    verified_free_context_banks: &[usize],
    route_id: u32,
    route_mask: u32,
    matched_sids: [u32; 2],
    matched_sid_count: usize,
) -> Result<RoutePlan, RouteError> {
    if !route_snapshot_fresh_and_complete {
        return Err(RouteError::RouteSnapshotNotFreshAndComplete);
    }
    if !smmu_control_delegated {
        return Err(RouteError::ControlNotDelegated);
    }
    if !ownership_transferred {
        return Err(RouteError::OwnershipNotTransferred);
    }
    if stream_id_width == 0 || stream_id_width > 32 {
        return Err(RouteError::InvalidStreamIdWidth);
    }
    if smr_slot_count == 0
        || smr_slot_count > SUNFISH_MAX_SMR_SLOTS
        || context_bank_count == 0
        || context_bank_count > SUNFISH_MAX_CONTEXT_BANKS
    {
        return Err(RouteError::InvalidHardwareBounds);
    }
    if verified_free_slots
        .iter()
        .any(|slot| *slot >= smr_slot_count)
        || verified_free_context_banks
            .iter()
            .any(|bank| *bank >= context_bank_count)
    {
        return Err(RouteError::InvalidHardwareBounds);
    }

    let width_mask = if stream_id_width == 32 {
        u32::MAX
    } else {
        (1u32 << stream_id_width) - 1
    };
    if route_id & !width_mask != 0 || route_mask & !width_mask != 0 {
        return Err(RouteError::CandidateOutOfRange);
    }
    if matched_sid_count == 0
        || matched_sid_count > matched_sids.len()
        || matched_sids[..matched_sid_count]
            .iter()
            .enumerate()
            .any(|(index, sid)| {
                sid & !width_mask != 0
                    || !matches(*sid, route_id, route_mask, width_mask)
                    || matched_sids[..index].contains(sid)
            })
    {
        return Err(RouteError::AliasSetInvalid);
    }
    // The route must cover exactly the explicitly declared SID set.
    if expansion_count(route_mask) != matched_sid_count {
        return Err(RouteError::AliasSetInvalid);
    }

    for (index, route) in observed.iter().enumerate() {
        if route.slot >= smr_slot_count {
            return Err(RouteError::ObservedSlotOutOfRange);
        }
        if observed[..index]
            .iter()
            .any(|previous| previous.slot == route.slot)
        {
            return Err(RouteError::DuplicateObservedSlot);
        }
        if route.id & !width_mask != 0 || route.mask & !width_mask != 0 {
            return Err(RouteError::ObservedRouteOutOfRange);
        }
        if route.context_bank >= context_bank_count {
            return Err(RouteError::ObservedRouteOutOfRange);
        }
        if overlaps(route_id, route_mask, route.id, route.mask, width_mask) {
            return Err(RouteError::RouteOverlap);
        }
    }

    let slot = verified_free_slots
        .iter()
        .copied()
        .filter(|slot| *slot < smr_slot_count)
        .find(|slot| !observed.iter().any(|route| route.slot == *slot))
        .ok_or_else(|| {
            if verified_free_slots
                .iter()
                .any(|slot| *slot >= smr_slot_count)
            {
                RouteError::InvalidHardwareBounds
            } else {
                RouteError::NoVerifiedFreeSlot
            }
        })?;
    let context_bank = verified_free_context_banks
        .iter()
        .copied()
        .filter(|bank| *bank < context_bank_count)
        .find(|bank| !observed.iter().any(|route| route.context_bank == *bank))
        .ok_or_else(|| {
            if verified_free_context_banks
                .iter()
                .any(|bank| *bank >= context_bank_count)
            {
                RouteError::InvalidHardwareBounds
            } else {
                RouteError::NoVerifiedFreeContextBank
            }
        })?;

    Ok(RoutePlan {
        slot,
        id: route_id,
        mask: route_mask,
        context_bank,
        matched_sids,
        matched_sid_count,
    })
}

const fn matches(sid: u32, route_id: u32, mask: u32, width_mask: u32) -> bool {
    ((sid ^ route_id) & !mask & width_mask) == 0
}

const fn overlaps(id_a: u32, mask_a: u32, id_b: u32, mask_b: u32, width_mask: u32) -> bool {
    let significant = !(mask_a | mask_b) & width_mask;
    ((id_a ^ id_b) & significant) == 0
}

const fn expansion_count(mask: u32) -> usize {
    1usize << mask.count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_fault_decoder_uses_arm_smmuv2_bit_positions() {
        let permission_write = ContextFaultSnapshot {
            fsr: 1 << 3,
            fsynr0: 1 << 4,
        }
        .decode();
        assert!(permission_write.has_fault);
        assert!(permission_write.permission);
        assert!(permission_write.write);
        assert!(!permission_write.external);
        assert!(!permission_write.translation);

        let external_read = ContextFaultSnapshot {
            fsr: 1 << 4,
            fsynr0: 0,
        }
        .decode();
        assert!(external_read.external);
        assert!(!external_read.permission);
        assert!(!external_read.write);
    }

    #[test]
    fn context_fault_decoder_preserves_all_known_fault_classes_and_raw_values() {
        let fsr = FSR_MULTI
            | FSR_SS
            | FSR_UUT
            | FSR_ASF
            | FSR_TLBLKF
            | FSR_TLBMCF
            | FSR_EF
            | FSR_PF
            | FSR_AFF
            | FSR_TF;
        let decoded = ContextFaultSnapshot {
            fsr,
            fsynr0: u32::MAX,
        }
        .decode();
        assert_eq!(decoded.raw_fsr, fsr);
        assert_eq!(decoded.raw_fsynr0, u32::MAX);
        assert!(decoded.has_fault);
        assert!(decoded.multiple);
        assert!(decoded.stalled);
        assert!(decoded.unexpected_upstream);
        assert!(decoded.address_size);
        assert!(decoded.tlb_lock);
        assert!(decoded.tlb_multihit);
        assert!(decoded.external);
        assert!(decoded.permission);
        assert!(decoded.access_flag);
        assert!(decoded.translation);
        assert!(decoded.write);
    }

    #[test]
    fn context_fault_decoder_does_not_invent_faults_from_reserved_bits() {
        let decoded = ContextFaultSnapshot {
            fsr: 1 << 20,
            fsynr0: 0,
        }
        .decode();
        assert!(!decoded.has_fault);
        assert_eq!(decoded.raw_fsr, 1 << 20);
    }

    #[test]
    fn ufs_route_is_one_exact_sid_and_uses_verified_resources() {
        let observed = [
            ObservedRoute {
                slot: 0,
                id: 0x200,
                mask: 0x0f,
                context_bank: 0,
            },
            ObservedRoute {
                slot: 1,
                id: 0x500,
                mask: 0,
                context_bank: 1,
            },
        ];
        let plan = plan_ufs_route(true, true, true, 15, 78, 61, &observed, &[2, 3], &[4]).unwrap();
        assert_eq!((plan.id(), plan.mask()), (0x300, 0));
        assert_eq!(plan.matched_sids(), [0x300, 0]);
        assert_eq!(plan.matched_sid_count(), 1);
        assert_eq!((plan.slot(), plan.context_bank()), (2, 4));
    }

    #[test]
    fn ufs_route_rejects_saved_broad_match_and_missing_ownership() {
        let observed = [ObservedRoute {
            slot: 4,
            id: 0x300,
            mask: 0x0f,
            context_bank: 2,
        }];
        assert_eq!(
            plan_ufs_route(true, true, true, 15, 78, 61, &observed, &[6], &[4]),
            Err(RouteError::RouteOverlap)
        );
        assert_eq!(
            plan_ufs_route(false, true, true, 15, 78, 61, &[], &[6], &[4]),
            Err(RouteError::ControlNotDelegated)
        );
        assert_eq!(
            plan_ufs_route(true, false, true, 15, 78, 61, &[], &[6], &[4]),
            Err(RouteError::OwnershipNotTransferred)
        );
    }

    #[test]
    fn linux_ipa_pair_matches_exactly_520_and_522_and_shares_one_context() {
        let p = plan_ipa_linux_dma_route(true, true, true, 15, 79, 61, &[], &[6], &[4]).unwrap();
        assert_eq!(
            (p.id(), p.mask(), p.slot(), p.context_bank()),
            (0x520, 2, 6, 4)
        );
        assert_eq!(p.matched_sids(), [0x520, 0x522]);
        assert_eq!(p.matched_sid_count(), 2);
        for sid in 0..1 << 15 {
            assert_eq!(
                (sid ^ p.id()) & !p.mask() == 0,
                sid == 0x520 || sid == 0x522
            );
        }
        for sid in [0x520, 0x522] {
            let existing = [ObservedRoute {
                slot: 7,
                id: sid,
                mask: 0,
                context_bank: 3,
            }];
            assert_eq!(
                plan_ipa_linux_dma_route(true, true, true, 15, 79, 61, &existing, &[6], &[4]),
                Err(RouteError::RouteOverlap)
            );
        }
        let neighbor = [ObservedRoute {
            slot: 7,
            id: 0x521,
            mask: 0,
            context_bank: 3,
        }];
        assert!(
            plan_ipa_linux_dma_route(true, true, true, 15, 79, 61, &neighbor, &[6], &[4]).is_ok()
        );
        assert_eq!(
            plan_ipa_linux_dma_route(false, true, true, 15, 79, 61, &[], &[6], &[4]),
            Err(RouteError::ControlNotDelegated)
        );
        assert_eq!(
            plan_ipa_linux_dma_route(true, false, true, 15, 79, 61, &[], &[6], &[4]),
            Err(RouteError::OwnershipNotTransferred)
        );
        assert_eq!(
            plan_ipa_linux_dma_route(true, true, false, 15, 79, 61, &[], &[6], &[4]),
            Err(RouteError::RouteSnapshotNotFreshAndComplete)
        );
    }

    #[test]
    fn ipa_ap_dma_uses_one_exact_translated_sid_route() {
        let observed = [ObservedRoute {
            slot: 0,
            id: 0x240,
            mask: 1,
            context_bank: 0,
        }];
        let plan = plan_ipa_ap_dma_route(true, true, true, 15, 78, 61, &observed, &[2, 3], &[4, 5])
            .unwrap();
        assert_eq!(SUNFISH_IPA_AP_DMA_SID, 0x520);
        assert_eq!((plan.id(), plan.mask()), (0x520, 0));
        assert_eq!(plan.matched_sids(), [0x520, 0]);
        assert_eq!(plan.matched_sid_count(), 1);
        assert_eq!((plan.slot(), plan.context_bank()), (2, 4));
    }

    #[test]
    fn ipa_ap_route_rejects_linux_bypass_overlap_and_missing_ownership() {
        let broad = [ObservedRoute {
            slot: 8,
            id: 0x500,
            mask: 0x3f,
            context_bank: 7,
        }];
        assert_eq!(
            plan_ipa_ap_dma_route(true, true, true, 15, 78, 61, &broad, &[9], &[10]),
            Err(RouteError::RouteOverlap)
        );
        assert_eq!(
            plan_ipa_ap_dma_route(false, true, true, 15, 78, 61, &[], &[9], &[10]),
            Err(RouteError::ControlNotDelegated)
        );
        assert_eq!(
            plan_ipa_ap_dma_route(true, false, true, 15, 78, 61, &[], &[9], &[10]),
            Err(RouteError::OwnershipNotTransferred)
        );
    }

    #[test]
    fn refuses_targeted_or_stale_route_snapshot() {
        assert_eq!(
            plan_wlan_alias_route(false, false, false, 15, 78, 61, &[], &[6], &[4]),
            Err(RouteError::RouteSnapshotNotFreshAndComplete)
        );
    }

    #[test]
    fn high_sid_alias_pair_uses_one_bounded_hardware_slot() {
        let plan = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
        assert_eq!(plan.slot(), 6);
        assert_ne!(plan.slot() as u32, plan.matched_sids()[0]);
        assert_eq!(plan.matched_sids(), [0x240, 0x241]);
        assert_eq!(plan.matched_sid_count(), 2);
        assert_eq!((plan.id(), plan.mask(), plan.context_bank()), (0x240, 1, 4));
    }

    #[test]
    fn rejects_out_of_range_hardware_slot() {
        assert_eq!(
            plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[79], &[4]),
            Err(RouteError::InvalidHardwareBounds)
        );
    }

    #[test]
    fn sunfish_last_smr_slot_is_77() {
        let plan = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[77], &[4]).unwrap();
        assert_eq!(plan.slot(), 77);
        assert_eq!(
            plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[78], &[4]),
            Err(RouteError::InvalidHardwareBounds)
        );
    }

    #[test]
    fn rejects_route_overlap_before_selecting_slot() {
        let observed = [ObservedRoute {
            slot: 2,
            id: 0x240,
            mask: 1,
            context_bank: 3,
        }];
        assert_eq!(
            plan_wlan_alias_route(true, true, true, 15, 78, 61, &observed, &[6], &[4]),
            Err(RouteError::RouteOverlap)
        );
    }

    #[test]
    fn refuses_when_no_free_slot_is_explicitly_verified() {
        assert_eq!(
            plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[], &[4]),
            Err(RouteError::NoVerifiedFreeSlot)
        );
    }

    #[test]
    fn refuses_context_bank_already_in_use() {
        let observed = [ObservedRoute {
            slot: 2,
            id: 0x100,
            mask: 0,
            context_bank: 4,
        }];
        assert_eq!(
            plan_wlan_alias_route(true, true, true, 15, 78, 61, &observed, &[6], &[4]),
            Err(RouteError::NoVerifiedFreeContextBank)
        );
    }

    #[test]
    fn rejects_hardware_geometry_larger_than_the_supported_register_aperture() {
        assert_eq!(
            plan_wlan_alias_route(true, true, true, 15, 129, 61, &[], &[6], &[4]),
            Err(RouteError::InvalidHardwareBounds)
        );
        assert_eq!(
            plan_wlan_alias_route(true, true, true, 15, 130, 61, &[], &[6], &[4]),
            Err(RouteError::InvalidHardwareBounds)
        );
    }

    #[test]
    fn refuses_without_both_ownership_gates() {
        assert_eq!(
            plan_wlan_alias_route(false, true, true, 15, 78, 61, &[], &[6], &[4]),
            Err(RouteError::ControlNotDelegated)
        );
        assert_eq!(
            plan_wlan_alias_route(true, false, true, 15, 78, 61, &[], &[6], &[4]),
            Err(RouteError::OwnershipNotTransferred)
        );
    }

    #[test]
    fn shared_alias_route_lives_until_last_sid_cap_is_unbound() {
        let plan = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
        let mut bindings = WlanAliasBindings::from_plan(plan).unwrap();
        bindings.bind(0x240, 4).unwrap();
        bindings.bind(0x241, 4).unwrap();
        assert!(bindings.is_route_referenced());
        assert_eq!(bindings.slot(), 6);
        assert_eq!(bindings.unbind(0x240), Ok(UnbindAction::KeepSharedRoute));
        assert!(bindings.is_route_referenced());
        assert_eq!(bindings.unbind(0x241), Ok(UnbindAction::ReleaseRoute));
        assert!(!bindings.is_route_referenced());
    }

    #[test]
    fn alias_binding_rejects_duplicate_sid_or_different_context_bank() {
        let plan = plan_wlan_alias_route(true, true, true, 15, 78, 61, &[], &[6], &[4]).unwrap();
        let mut bindings = WlanAliasBindings::from_plan(plan).unwrap();
        bindings.bind(0x240, 4).unwrap();
        assert_eq!(bindings.bind(0x240, 4), Err(RouteError::SidAlreadyBound));
        assert_eq!(
            bindings.bind(0x241, 5),
            Err(RouteError::ContextBankMismatch)
        );
        assert_eq!(bindings.unbind(0x241), Err(RouteError::SidNotBound));
    }
}
