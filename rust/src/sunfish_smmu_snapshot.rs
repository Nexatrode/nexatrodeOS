//! Complete, read-only Apps-SMMU route-table capture for Sunfish.
//!
//! The snapshot reads every implemented stream-match entry and its paired
//! S2CR, including invalid SMRs. It does not write hardware, prove exclusive
//! ownership, or prove that an object remains fresh after capture. A live
//! owner must call it immediately before planning and installation while
//! holding the platform's SMMU serialization/authority gate.
#![forbid(unsafe_code)]

use crate::sunfish_smmu::{
    ObservedRoute, RouteError, RoutePlan, SUNFISH_MAX_CONTEXT_BANKS, SUNFISH_MAX_SMR_SLOTS,
    plan_ipa_ap_dma_route, plan_ipa_linux_dma_route, plan_ipa_linux_peer_dma_route,
    plan_wlan_alias_route,
};

pub const SMMU_MMIO_BYTES: u32 = 0x0010_0000;
pub const SMMU_ID0: u32 = 0x020;
pub const SMMU_ID1: u32 = 0x024;
/// Global fault status register visible through this SMMU security bank.
pub const SMMU_GLOBAL_FSR: u32 = 0x048;
pub const SMMU_SMR_BASE: u32 = 0x800;
pub const SMMU_S2CR_BASE: u32 = 0xc00;
pub const SMMU_GR1_CBAR_BASE: u32 = 0x000;
pub const SMMU_GR1_CBA2R_BASE: u32 = 0x800;
pub const SMMU_CB_SCTLR: u32 = 0x000;
pub const SMMU_CB_ACTLR: u32 = 0x004;
pub const SMMU_CB_TTBCR2: u32 = 0x010;
pub const SMMU_CB_TTBR0: u32 = 0x020;
pub const SMMU_CB_TTBR1: u32 = 0x028;
pub const SMMU_CB_TTBCR: u32 = 0x030;
pub const SMMU_CB_MAIR0: u32 = 0x038;
pub const SMMU_CB_MAIR1: u32 = 0x03c;
pub const SMMU_CB_FSR: u32 = 0x058;

const ID0_SMS: u32 = 1 << 27;
const ID0_NUMSIDB_SHIFT: u32 = 9;
const ID0_NUMSIDB_MASK: u32 = 0xf;
const ID0_NUMSMRG_MASK: u32 = 0xff;
const ID1_PAGESIZE_64K: u32 = 1 << 31;
const ID1_NUMPAGENDXB_SHIFT: u32 = 28;
const ID1_NUMPAGENDXB_MASK: u32 = 0x7;
const ID1_NUMCB_MASK: u32 = 0xff;
const SMR_VALID: u32 = 1 << 31;
const SMR_ID_MASK: u32 = 0x7fff;
const SMR_MASK_SHIFT: u32 = 16;
const S2CR_TYPE_SHIFT: u32 = 16;
const S2CR_CBNDX_MASK: u32 = 0xff;

/// Read-only access to 32-bit SMMU registers. A hardware implementation must
/// validate its mapped aperture and serialize this capture against every
/// possible SMMU writer; this trait alone grants neither property.
pub trait SmmuReadIo {
    type Error;

    fn read32(&mut self, offset: u32) -> Result<u32, Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawRouteEntry {
    pub smr: u32,
    pub s2cr: u32,
}

impl RawRouteEntry {
    const EMPTY: Self = Self { smr: 0, s2cr: 0 };

    pub const fn is_valid(self) -> bool {
        self.smr & SMR_VALID != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError<E> {
    Io { offset: u32, source: E },
    IndexedStreamModeUnsupported,
    InvalidStreamMatchCount(u32),
    InvalidStreamIdWidth(u32),
    InvalidContextBankCount(u32),
    ContextBankOutOfRange { bank: usize, count: usize },
    RegisterWindowOutOfRange,
    InvalidStreamId { slot: usize, value: u32 },
    InvalidS2crType { slot: usize, value: u32 },
    InvalidContextBank { slot: usize, value: usize },
}

/// Exact pre-install values for every context-bank register changed by the
/// SMMU plan, plus FSR so an already-latched fault can be rejected before
/// writes. This is stale-state evidence only; it does not establish ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextBankPreimage {
    context_bank: usize,
    cbar: u32,
    cba2r: u32,
    sctlr: u32,
    actlr: u32,
    ttbcr2: u32,
    ttbr0_low: u32,
    ttbr0_high: u32,
    ttbr1_low: u32,
    ttbr1_high: u32,
    ttbcr: u32,
    mair0: u32,
    mair1: u32,
    fsr: u32,
}

impl ContextBankPreimage {
    pub const fn context_bank(self) -> usize {
        self.context_bank
    }
    pub const fn cbar(self) -> u32 {
        self.cbar
    }
    pub const fn cba2r(self) -> u32 {
        self.cba2r
    }
    pub const fn sctlr(self) -> u32 {
        self.sctlr
    }
    pub const fn actlr(self) -> u32 {
        self.actlr
    }
    pub const fn ttbcr2(self) -> u32 {
        self.ttbcr2
    }
    pub const fn ttbr0_low(self) -> u32 {
        self.ttbr0_low
    }
    pub const fn ttbr0_high(self) -> u32 {
        self.ttbr0_high
    }
    pub const fn ttbr1_low(self) -> u32 {
        self.ttbr1_low
    }
    pub const fn ttbr1_high(self) -> u32 {
        self.ttbr1_high
    }
    pub const fn ttbcr(self) -> u32 {
        self.ttbcr
    }
    pub const fn mair0(self) -> u32 {
        self.mair0
    }
    pub const fn mair1(self) -> u32 {
        self.mair1
    }
    pub const fn fsr(self) -> u32 {
        self.fsr
    }

    #[cfg(test)]
    #[allow(dead_code)] // Host tests include this module directly by path.
    pub(crate) const fn for_test(context_bank: usize) -> Self {
        Self {
            context_bank,
            cbar: 0,
            cba2r: 0,
            sctlr: 0,
            actlr: 0,
            ttbcr2: 0,
            ttbr0_low: 0,
            ttbr0_high: 0,
            ttbr1_low: 0,
            ttbr1_high: 0,
            ttbcr: 0,
            mair0: 0,
            mair1: 0,
            fsr: 0,
        }
    }

    #[cfg(test)]
    #[allow(dead_code)] // Used by the standalone executor integration test.
    pub(crate) const fn with_fsr_for_test(mut self, fsr: u32) -> Self {
        self.fsr = fsr;
        self
    }

    #[cfg(test)]
    #[allow(dead_code)] // Used by the standalone executor integration test.
    pub(crate) const fn with_sctlr_for_test(mut self, sctlr: u32) -> Self {
        self.sctlr = sctlr;
        self
    }
}

/// Capture a chosen bank's preimage while the caller holds the same SMMU
/// serialization gate through planning and installation.
pub fn capture_context_bank_preimage<I: SmmuReadIo>(
    io: &mut I,
    snapshot: &RouteTableSnapshot,
    context_bank: usize,
) -> Result<ContextBankPreimage, SnapshotError<I::Error>> {
    if context_bank >= snapshot.context_bank_count {
        return Err(SnapshotError::ContextBankOutOfRange {
            bank: context_bank,
            count: snapshot.context_bank_count,
        });
    }

    let bank = context_bank as u32;
    let bank_index = bank
        .checked_mul(4)
        .ok_or(SnapshotError::RegisterWindowOutOfRange)?;
    let cbar_offset = snapshot
        .page_size_bytes
        .checked_add(bank_index)
        .ok_or(SnapshotError::RegisterWindowOutOfRange)?;
    let cba2r_offset = snapshot
        .page_size_bytes
        .checked_add(SMMU_GR1_CBA2R_BASE)
        .and_then(|base| base.checked_add(bank_index))
        .ok_or(SnapshotError::RegisterWindowOutOfRange)?;
    let base = snapshot
        .context_bank_base
        .checked_add(
            bank.checked_mul(snapshot.page_size_bytes)
                .ok_or(SnapshotError::RegisterWindowOutOfRange)?,
        )
        .ok_or(SnapshotError::RegisterWindowOutOfRange)?;
    let reg = |offset: u32| {
        base.checked_add(offset)
            .ok_or(SnapshotError::RegisterWindowOutOfRange)
    };

    Ok(ContextBankPreimage {
        context_bank,
        cbar: read(io, cbar_offset)?,
        cba2r: read(io, cba2r_offset)?,
        sctlr: read(io, reg(SMMU_CB_SCTLR)?)?,
        actlr: read(io, reg(SMMU_CB_ACTLR)?)?,
        ttbcr2: read(io, reg(SMMU_CB_TTBCR2)?)?,
        ttbr0_low: read(io, reg(SMMU_CB_TTBR0)?)?,
        ttbr0_high: read(io, reg(SMMU_CB_TTBR0 + 4)?)?,
        ttbr1_low: read(io, reg(SMMU_CB_TTBR1)?)?,
        ttbr1_high: read(io, reg(SMMU_CB_TTBR1 + 4)?)?,
        ttbcr: read(io, reg(SMMU_CB_TTBCR)?)?,
        mair0: read(io, reg(SMMU_CB_MAIR0)?)?,
        mair1: read(io, reg(SMMU_CB_MAIR1)?)?,
        fsr: read(io, reg(SMMU_CB_FSR)?)?,
    })
}

/// A complete immutable copy of the Sunfish stream-match routing table.
/// Construction is only available through [`capture`], which reads the
/// hardware-reported number of entries in full. The raw S2CR is retained even
/// when its SMR is invalid so a future installer can compare its preimage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteTableSnapshot {
    id0: u32,
    id1: u32,
    global_fsr: u32,
    stream_id_width: u8,
    slot_count: usize,
    context_bank_count: usize,
    page_size_bytes: u32,
    context_bank_base: u32,
    entries: [RawRouteEntry; SUNFISH_MAX_SMR_SLOTS],
}

/// A candidate route together with the exact SMR/S2CR preimage that selected
/// its slot. The preimage is retained for an eventual compare-before-write
/// owner; this value itself is not hardware authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapturedRoutePlan {
    route: RoutePlan,
    slot_preimage: RawRouteEntry,
    global_fsr: u32,
}

impl CapturedRoutePlan {
    pub const fn route(self) -> RoutePlan {
        self.route
    }

    pub const fn slot_preimage(self) -> RawRouteEntry {
        self.slot_preimage
    }

    pub const fn global_fsr(self) -> u32 {
        self.global_fsr
    }

    pub const fn slot(self) -> usize {
        self.route.slot()
    }

    pub const fn context_bank(self) -> usize {
        self.route.context_bank()
    }

    #[cfg(test)]
    #[allow(dead_code)] // Host tests include this module directly by path.
    pub(crate) const fn for_test(route: RoutePlan, slot_preimage: RawRouteEntry) -> Self {
        Self {
            route,
            slot_preimage,
            global_fsr: 0,
        }
    }

    #[cfg(test)]
    #[allow(dead_code)] // Used by the standalone executor integration test.
    pub(crate) const fn with_global_fsr_for_test(mut self, global_fsr: u32) -> Self {
        self.global_fsr = global_fsr;
        self
    }
}

impl RouteTableSnapshot {
    pub const fn id0(self) -> u32 {
        self.id0
    }

    pub const fn id1(self) -> u32 {
        self.id1
    }

    pub const fn global_fsr(self) -> u32 {
        self.global_fsr
    }

    pub const fn stream_id_width(self) -> u8 {
        self.stream_id_width
    }

    pub const fn slot_count(self) -> usize {
        self.slot_count
    }

    pub const fn context_bank_count(self) -> usize {
        self.context_bank_count
    }

    pub const fn page_size_bytes(self) -> u32 {
        self.page_size_bytes
    }

    pub const fn context_bank_base(self) -> u32 {
        self.context_bank_base
    }

    pub fn raw_entry(&self, slot: usize) -> Option<RawRouteEntry> {
        (slot < self.slot_count).then(|| self.entries[slot])
    }

    /// Return the active routes decoded from the complete raw table.
    pub fn observed_routes(&self) -> ObservedRoutes<'_> {
        ObservedRoutes {
            snapshot: self,
            next_slot: 0,
        }
    }

    /// Return the SMR slots that were invalid in this capture. This is an
    /// observation only; it becomes a usable slot only under an independent
    /// exclusive-owner gate and an immediate pre-write readback.
    pub fn inactive_slots(&self) -> InactiveSlots<'_> {
        InactiveSlots {
            snapshot: self,
            next_slot: 0,
        }
    }
}

pub struct ObservedRoutes<'a> {
    snapshot: &'a RouteTableSnapshot,
    next_slot: usize,
}

impl Iterator for ObservedRoutes<'_> {
    type Item = ObservedRoute;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next_slot < self.snapshot.slot_count {
            let slot = self.next_slot;
            self.next_slot += 1;
            let raw = self.snapshot.entries[slot];
            if !raw.is_valid() {
                continue;
            }
            let s2cr_type = (raw.s2cr >> S2CR_TYPE_SHIFT) & 3;
            let context_bank = if s2cr_type == 0 {
                (raw.s2cr & S2CR_CBNDX_MASK) as usize
            } else {
                // Bypass/fault S2CRs do not consume a context bank. Zero is
                // conservative for the legacy planner's bank-reference set.
                0
            };
            return Some(ObservedRoute {
                slot,
                id: raw.smr & SMR_ID_MASK,
                mask: (raw.smr >> SMR_MASK_SHIFT) & SMR_ID_MASK,
                context_bank,
            });
        }
        None
    }
}

pub struct InactiveSlots<'a> {
    snapshot: &'a RouteTableSnapshot,
    next_slot: usize,
}

impl Iterator for InactiveSlots<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next_slot < self.snapshot.slot_count {
            let slot = self.next_slot;
            self.next_slot += 1;
            if !self.snapshot.entries[slot].is_valid() {
                return Some(slot);
            }
        }
        None
    }
}

/// Read and validate the complete Sunfish stream-match table.
///
/// Register offsets and ID decoding match the existing Sunfish SMMU probe and
/// the pinned Linux `arm-smmu.c` definitions. Every ID0.NUMSMRG pair is read,
/// bounded by the supported aperture, not just valid rows or selected SIDs.
/// The caller must separately
/// establish exclusive control and recheck the chosen preimage immediately
/// before any write.
pub fn capture<I: SmmuReadIo>(io: &mut I) -> Result<RouteTableSnapshot, SnapshotError<I::Error>> {
    let id0 = read(io, SMMU_ID0)?;
    let id1 = read(io, SMMU_ID1)?;
    let global_fsr = read(io, SMMU_GLOBAL_FSR)?;

    if id0 & ID0_SMS == 0 {
        return Err(SnapshotError::IndexedStreamModeUnsupported);
    }
    let slot_count = (id0 & ID0_NUMSMRG_MASK) as usize;
    if slot_count == 0 || slot_count > SUNFISH_MAX_SMR_SLOTS {
        return Err(SnapshotError::InvalidStreamMatchCount(slot_count as u32));
    }
    let stream_id_width = ((id0 >> ID0_NUMSIDB_SHIFT) & ID0_NUMSIDB_MASK) as u8;
    if stream_id_width == 0 || stream_id_width > 15 {
        return Err(SnapshotError::InvalidStreamIdWidth(u32::from(
            stream_id_width,
        )));
    }
    let context_bank_count = (id1 & ID1_NUMCB_MASK) as usize;
    if context_bank_count == 0 || context_bank_count > SUNFISH_MAX_CONTEXT_BANKS {
        return Err(SnapshotError::InvalidContextBankCount(
            context_bank_count as u32,
        ));
    }

    let page_size_bytes = if id1 & ID1_PAGESIZE_64K != 0 {
        0x1_0000
    } else {
        0x1000
    };
    let page_count = 1u32 << (((id1 >> ID1_NUMPAGENDXB_SHIFT) & ID1_NUMPAGENDXB_MASK) + 1);
    let context_bank_base = page_count
        .checked_mul(page_size_bytes)
        .ok_or(SnapshotError::RegisterWindowOutOfRange)?;
    let context_bank_end = context_bank_base
        .checked_add((context_bank_count as u32) * page_size_bytes)
        .ok_or(SnapshotError::RegisterWindowOutOfRange)?;
    let cbar_end = page_size_bytes
        .checked_add((context_bank_count as u32) * 4)
        .ok_or(SnapshotError::RegisterWindowOutOfRange)?;
    let route_end = SMMU_S2CR_BASE
        .checked_add((slot_count as u32) * 4)
        .ok_or(SnapshotError::RegisterWindowOutOfRange)?;
    if context_bank_end > SMMU_MMIO_BYTES
        || cbar_end > SMMU_MMIO_BYTES
        || route_end > SMMU_MMIO_BYTES
    {
        return Err(SnapshotError::RegisterWindowOutOfRange);
    }

    let width_mask = (1u32 << stream_id_width) - 1;
    let mut entries = [RawRouteEntry::EMPTY; SUNFISH_MAX_SMR_SLOTS];
    for (slot, entry) in entries.iter_mut().take(slot_count).enumerate() {
        let smr_offset = SMMU_SMR_BASE + slot as u32 * 4;
        let s2cr_offset = SMMU_S2CR_BASE + slot as u32 * 4;
        entry.smr = read(io, smr_offset)?;
        entry.s2cr = read(io, s2cr_offset)?;
        if entry.is_valid() {
            let id = entry.smr & SMR_ID_MASK;
            let mask = (entry.smr >> SMR_MASK_SHIFT) & SMR_ID_MASK;
            if id & !width_mask != 0 || mask & !width_mask != 0 {
                return Err(SnapshotError::InvalidStreamId {
                    slot,
                    value: entry.smr,
                });
            }
            let s2cr_type = (entry.s2cr >> S2CR_TYPE_SHIFT) & 3;
            if s2cr_type == 3 {
                return Err(SnapshotError::InvalidS2crType {
                    slot,
                    value: s2cr_type,
                });
            }
            if s2cr_type == 0 {
                let context_bank = (entry.s2cr & S2CR_CBNDX_MASK) as usize;
                if context_bank >= context_bank_count {
                    return Err(SnapshotError::InvalidContextBank {
                        slot,
                        value: context_bank,
                    });
                }
            }
        }
    }

    Ok(RouteTableSnapshot {
        id0,
        id1,
        global_fsr,
        stream_id_width,
        slot_count,
        context_bank_count,
        page_size_bytes,
        context_bank_base,
        entries,
    })
}

/// Plan the WLAN alias route from a captured complete table, deriving both
/// occupied routes and inactive SMR slots from the raw registers. Context-bank
/// availability remains a separate explicit input because route-table capture
/// alone cannot prove that an unreferenced bank is safe to reuse.
pub fn plan_wlan_alias_route_from_snapshot(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    snapshot: &RouteTableSnapshot,
    verified_free_context_banks: &[usize],
) -> Result<CapturedRoutePlan, RouteError> {
    plan_alias_route_from_snapshot(
        smmu_control_delegated,
        ownership_transferred,
        snapshot,
        verified_free_context_banks,
        RouteProfile::Wlan,
        None,
    )
}

/// Complete-table mainline IPA two-SID route; no context is considered free
/// if any S2CR references it, including an invalid SMR's retained S2CR.
pub fn plan_ipa_linux_dma_route_from_snapshot(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    snapshot: &RouteTableSnapshot,
    verified_free_context_banks: &[usize],
) -> Result<CapturedRoutePlan, RouteError> {
    plan_alias_route_from_snapshot(
        smmu_control_delegated,
        ownership_transferred,
        snapshot,
        verified_free_context_banks,
        RouteProfile::MaskedIpa,
        None,
    )
}

/// Two exact mask-zero matches from the working DT, captured before any write.
/// Both share one context and have distinct inactive slots. This does not use
/// the failed masked-pair optimization or prove acceptance by hardware.
pub fn plan_ipa_linux_exact_routes_from_snapshot(
    control: bool,
    ownership: bool,
    snapshot: &RouteTableSnapshot,
    candidates: &[usize],
) -> Result<[CapturedRoutePlan; 2], RouteError> {
    let first = plan_alias_route_from_snapshot(
        control,
        ownership,
        snapshot,
        candidates,
        RouteProfile::IpaPrimary,
        None,
    )?;
    let second = plan_alias_route_from_snapshot(
        control,
        ownership,
        snapshot,
        &[first.context_bank()],
        RouteProfile::IpaPeer,
        Some(first.slot()),
    )?;
    Ok([first, second])
}
#[derive(Clone, Copy)]
enum RouteProfile {
    Wlan,
    MaskedIpa,
    IpaPrimary,
    IpaPeer,
}

fn plan_alias_route_from_snapshot(
    smmu_control_delegated: bool,
    ownership_transferred: bool,
    snapshot: &RouteTableSnapshot,
    verified_free_context_banks: &[usize],
    profile: RouteProfile,
    excluded_slot: Option<usize>,
) -> Result<CapturedRoutePlan, RouteError> {
    let mut observed = [ObservedRoute {
        slot: 0,
        id: 0,
        mask: 0,
        context_bank: 0,
    }; SUNFISH_MAX_SMR_SLOTS];
    let mut observed_count = 0;
    for route in snapshot.observed_routes() {
        observed[observed_count] = route;
        observed_count += 1;
    }

    let mut free_slots = [0usize; SUNFISH_MAX_SMR_SLOTS];
    let mut free_slot_count = 0;
    for slot in snapshot.inactive_slots() {
        if excluded_slot == Some(slot) {
            continue;
        }
        free_slots[free_slot_count] = slot;
        free_slot_count += 1;
    }

    // A translation S2CR can retain a context-bank reference while its SMR
    // is invalid (for example, during a route transition). Do not let a
    // caller-supplied candidate overwrite that bank merely because it is not
    // present in `observed_routes()`, which intentionally includes only valid
    // SMRs.
    let mut unreferenced_context_banks = [0usize; SUNFISH_MAX_CONTEXT_BANKS];
    let mut unreferenced_context_bank_count = 0;
    for bank in verified_free_context_banks.iter().copied() {
        if bank >= snapshot.context_bank_count {
            return Err(RouteError::InvalidHardwareBounds);
        }
        let referenced_by_translation =
            snapshot.entries[..snapshot.slot_count].iter().any(|entry| {
                (entry.s2cr >> S2CR_TYPE_SHIFT) & 3 == 0
                    && (entry.s2cr & S2CR_CBNDX_MASK) as usize == bank
            });
        if !referenced_by_translation {
            if !unreferenced_context_banks[..unreferenced_context_bank_count].contains(&bank) {
                unreferenced_context_banks[unreferenced_context_bank_count] = bank;
                unreferenced_context_bank_count += 1;
            }
        }
    }

    let planner = match profile {
        RouteProfile::Wlan => plan_wlan_alias_route,
        RouteProfile::MaskedIpa => plan_ipa_linux_dma_route,
        RouteProfile::IpaPrimary => plan_ipa_ap_dma_route,
        RouteProfile::IpaPeer => plan_ipa_linux_peer_dma_route,
    };
    let route = planner(
        smmu_control_delegated,
        ownership_transferred,
        true,
        snapshot.stream_id_width,
        snapshot.slot_count,
        snapshot.context_bank_count,
        &observed[..observed_count],
        &free_slots[..free_slot_count],
        &unreferenced_context_banks[..unreferenced_context_bank_count],
    )?;
    let slot_preimage = snapshot
        .raw_entry(route.slot())
        .expect("route planner only selects an implemented slot");
    Ok(CapturedRoutePlan {
        route,
        slot_preimage,
        global_fsr: snapshot.global_fsr,
    })
}

fn read<I: SmmuReadIo>(io: &mut I, offset: u32) -> Result<u32, SnapshotError<I::Error>> {
    if offset
        .checked_add(4)
        .is_none_or(|end| end > SMMU_MMIO_BYTES)
    {
        return Err(SnapshotError::RegisterWindowOutOfRange);
    }
    io.read32(offset)
        .map_err(|source| SnapshotError::Io { offset, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::vec::Vec;

    const ID0_VALUE: u32 = (15 << ID0_NUMSIDB_SHIFT) | ID0_SMS | 78;
    const ID1_VALUE: u32 = 61; // 4 KiB pages, two global pages, 61 context banks.

    #[derive(Default)]
    struct MockSmmu {
        registers: BTreeMap<u32, u32>,
        reads: Vec<u32>,
        fail_at: Option<u32>,
    }

    impl MockSmmu {
        fn sunfish() -> Self {
            let mut mock = Self::default();
            mock.registers.insert(SMMU_ID0, ID0_VALUE);
            mock.registers.insert(SMMU_ID1, ID1_VALUE);
            mock
        }

        fn set_route(&mut self, slot: usize, smr: u32, s2cr: u32) {
            self.registers.insert(SMMU_SMR_BASE + slot as u32 * 4, smr);
            self.registers
                .insert(SMMU_S2CR_BASE + slot as u32 * 4, s2cr);
        }
    }

    impl SmmuReadIo for MockSmmu {
        type Error = u32;

        fn read32(&mut self, offset: u32) -> Result<u32, Self::Error> {
            self.reads.push(offset);
            if self.fail_at == Some(offset) {
                return Err(offset);
            }
            Ok(self.registers.get(&offset).copied().unwrap_or(0))
        }
    }

    #[test]
    fn captures_every_smr_s2cr_pair_including_invalid_slot_preimages() {
        let mut io = MockSmmu::sunfish();
        io.registers.insert(SMMU_GLOBAL_FSR, 0x12);
        io.set_route(0, SMR_VALID | 0x0340 | (0xf << SMR_MASK_SHIFT), 0);
        io.registers.insert(SMMU_S2CR_BASE + 6 * 4, 0xdead_beef);

        let snapshot = capture(&mut io).unwrap();
        assert_eq!(snapshot.slot_count(), 78);
        assert_eq!(snapshot.context_bank_count(), 61);
        assert_eq!(snapshot.stream_id_width(), 15);
        assert_eq!(snapshot.page_size_bytes(), 0x1000);
        assert_eq!(snapshot.context_bank_base(), 0x2000);
        assert_eq!(snapshot.global_fsr(), 0x12);
        assert_eq!(snapshot.raw_entry(6).unwrap().smr, 0);
        assert_eq!(snapshot.raw_entry(6).unwrap().s2cr, 0xdead_beef);
        assert_eq!(snapshot.observed_routes().count(), 1);
        assert_eq!(snapshot.inactive_slots().count(), 77);
        assert_eq!(io.reads.len(), 3 + 2 * 78);
        assert!(io.reads.contains(&SMMU_GLOBAL_FSR));
        for slot in 0..78 {
            assert!(io.reads.contains(&(SMMU_SMR_BASE + slot * 4)));
            assert!(io.reads.contains(&(SMMU_S2CR_BASE + slot * 4)));
        }
    }

    #[test]
    fn linux_79_group_handoff_keeps_the_final_route_in_conflict_detection() {
        let mut io = MockSmmu::sunfish();
        io.registers.insert(SMMU_ID0, (ID0_VALUE & !0xff) | 79);
        io.set_route(78, SMR_VALID | 0x240 | (1 << SMR_MASK_SHIFT), 3);
        let snapshot = capture(&mut io).unwrap();
        assert_eq!(snapshot.slot_count(), 79);
        assert_eq!(snapshot.raw_entry(78).unwrap().s2cr, 3);
        assert_eq!(snapshot.observed_routes().last().unwrap().slot, 78);
        assert_eq!(io.reads.len(), 3 + 2 * 79);
        assert!(plan_wlan_alias_route_from_snapshot(true, true, &snapshot, &[2]).is_err());
    }

    #[test]
    fn captures_context_configuration_registers_and_fault_status_for_one_bank() {
        let mut io = MockSmmu::sunfish();
        let snapshot = capture(&mut io).unwrap();
        io.reads.clear();

        let expected = [
            (0x1014, 0x11), // GR1.CBAR[5]
            (0x1814, 0x12), // GR1.CBA2R[5]
            (0x7000, 0x13), // SCTLR
            (0x7004, 0x14), // ACTLR
            (0x7010, 0x15), // TTBCR2
            (0x7020, 0x16), // TTBR0 low
            (0x7024, 0x17), // TTBR0 high
            (0x7028, 0x18), // TTBR1 low
            (0x702c, 0x19), // TTBR1 high
            (0x7030, 0x1a), // TTBCR
            (0x7038, 0x1b), // MAIR0
            (0x703c, 0x1c), // MAIR1
            (0x7058, 0),    // FSR
        ];
        for (offset, value) in expected {
            io.registers.insert(offset, value);
        }

        let preimage = capture_context_bank_preimage(&mut io, &snapshot, 5).unwrap();
        assert_eq!(preimage.context_bank(), 5);
        assert_eq!(preimage.cbar(), 0x11);
        assert_eq!(preimage.cba2r(), 0x12);
        assert_eq!(preimage.sctlr(), 0x13);
        assert_eq!(preimage.actlr(), 0x14);
        assert_eq!(preimage.ttbcr2(), 0x15);
        assert_eq!(preimage.ttbr0_low(), 0x16);
        assert_eq!(preimage.ttbr0_high(), 0x17);
        assert_eq!(preimage.ttbr1_low(), 0x18);
        assert_eq!(preimage.ttbr1_high(), 0x19);
        assert_eq!(preimage.ttbcr(), 0x1a);
        assert_eq!(preimage.mair0(), 0x1b);
        assert_eq!(preimage.mair1(), 0x1c);
        assert_eq!(preimage.fsr(), 0);
        assert_eq!(io.reads, expected.map(|(offset, _)| offset));
    }

    #[test]
    fn context_preimage_capture_rejects_unimplemented_bank_before_reading() {
        let mut io = MockSmmu::sunfish();
        let snapshot = capture(&mut io).unwrap();
        io.reads.clear();
        assert_eq!(
            capture_context_bank_preimage(&mut io, &snapshot, 61),
            Err(SnapshotError::ContextBankOutOfRange {
                bank: 61,
                count: 61,
            })
        );
        assert!(io.reads.is_empty());
    }

    #[test]
    fn context_preimage_capture_reports_the_failing_register_offset() {
        let mut io = MockSmmu::sunfish();
        let snapshot = capture(&mut io).unwrap();
        io.reads.clear();
        io.fail_at = Some(0x7028);
        assert_eq!(
            capture_context_bank_preimage(&mut io, &snapshot, 5),
            Err(SnapshotError::Io {
                offset: 0x7028,
                source: 0x7028,
            })
        );
        assert_eq!(io.reads.last(), Some(&0x7028));
    }

    #[test]
    fn exact_ipa_pair_has_distinct_mask_zero_slots_one_context_and_requires_both_free() {
        let mut io = MockSmmu::sunfish();
        let snap = capture(&mut io).unwrap();
        let [first, peer] =
            plan_ipa_linux_exact_routes_from_snapshot(true, true, &snap, &[4]).unwrap();
        assert_ne!(first.slot(), peer.slot());
        assert_eq!(first.context_bank(), peer.context_bank());
        assert_eq!(
            (
                first.route().id(),
                first.route().mask(),
                peer.route().id(),
                peer.route().mask()
            ),
            (0x520, 0, 0x522, 0)
        );
        for slot in 0..snap.slot_count() {
            if slot != 6 {
                io.set_route(slot, 0x80000100 + slot as u32, 3);
            }
        }
        let snap = capture(&mut io).unwrap();
        assert_eq!(
            plan_ipa_linux_exact_routes_from_snapshot(true, true, &snap, &[4]),
            Err(RouteError::NoVerifiedFreeSlot)
        );
        io.set_route(6, 0x80000522, 3);
        io.set_route(7, 0, 3);
        io.set_route(8, 0, 3);
        let snap = capture(&mut io).unwrap();
        assert_eq!(
            plan_ipa_linux_exact_routes_from_snapshot(true, true, &snap, &[4]),
            Err(RouteError::RouteOverlap)
        );
    }

    #[test]
    fn linux_ipa_snapshot_rejects_second_sid_overlap_and_invalid_smr_bank_reference() {
        let mut io = MockSmmu::sunfish();
        io.set_route(7, 0, 4); // invalid stream match still retains CB4
        let snap = capture(&mut io).unwrap();
        let route = plan_ipa_linux_dma_route_from_snapshot(true, true, &snap, &[4, 5]).unwrap();
        assert_eq!(route.context_bank(), 5);
        assert_eq!(route.route().matched_sids(), [0x520, 0x522]);
        assert_eq!(
            plan_ipa_linux_dma_route_from_snapshot(true, true, &snap, &[4]),
            Err(RouteError::NoVerifiedFreeContextBank)
        );
        io.set_route(9, 0x80000522, 3);
        let snap = capture(&mut io).unwrap();
        assert_eq!(
            plan_ipa_linux_dma_route_from_snapshot(true, true, &snap, &[5]),
            Err(RouteError::RouteOverlap)
        );
    }

    #[test]
    fn planner_derives_overlap_and_free_slot_from_complete_snapshot() {
        let mut io = MockSmmu::sunfish();
        io.registers.insert(SMMU_GLOBAL_FSR, 0x02);
        io.set_route(0, SMR_VALID | 0x0340 | (0xf << SMR_MASK_SHIFT), 0);
        io.set_route(3, SMR_VALID | 0x0540, 1 << S2CR_TYPE_SHIFT);
        io.registers.insert(SMMU_S2CR_BASE + 4, 0xdead_beef);
        let snapshot = capture(&mut io).unwrap();

        let plan = plan_wlan_alias_route_from_snapshot(true, true, &snapshot, &[4]).unwrap();
        assert_eq!(plan.slot(), 1);
        assert_eq!(plan.context_bank(), 4);
        assert_eq!(plan.slot_preimage().smr, 0);
        assert_eq!(plan.slot_preimage().s2cr, 0xdead_beef);
        assert_eq!(plan.global_fsr(), 0x02);

        let mut overlap = MockSmmu::sunfish();
        overlap.set_route(13, SMR_VALID | 0x0240 | (1 << SMR_MASK_SHIFT), 6);
        let snapshot = capture(&mut overlap).unwrap();
        assert_eq!(
            plan_wlan_alias_route_from_snapshot(true, true, &snapshot, &[4]),
            Err(RouteError::RouteOverlap)
        );
    }

    #[test]
    fn planner_skips_context_banks_referenced_by_invalid_stream_matches() {
        let mut io = MockSmmu::sunfish();
        // SMR is invalid, but the translation S2CR still points at CB4.
        io.set_route(7, 0, 4);
        let snapshot = capture(&mut io).unwrap();

        let plan = plan_wlan_alias_route_from_snapshot(true, true, &snapshot, &[4, 5]).unwrap();
        assert_eq!(plan.context_bank(), 5);
        assert_eq!(
            plan_wlan_alias_route_from_snapshot(true, true, &snapshot, &[4]),
            Err(RouteError::NoVerifiedFreeContextBank)
        );
    }

    #[test]
    fn refuses_incomplete_or_incompatible_hardware_geometry() {
        let mut indexed = MockSmmu::sunfish();
        indexed.registers.insert(SMMU_ID0, ID0_VALUE & !ID0_SMS);
        assert_eq!(
            capture(&mut indexed),
            Err(SnapshotError::IndexedStreamModeUnsupported)
        );

        let mut wrong_smr_count = MockSmmu::sunfish();
        wrong_smr_count
            .registers
            .insert(SMMU_ID0, (ID0_VALUE & !0xff) | 129);
        assert_eq!(
            capture(&mut wrong_smr_count),
            Err(SnapshotError::InvalidStreamMatchCount(129))
        );

        let mut oversized_context_window = MockSmmu::sunfish();
        oversized_context_window
            .registers
            .insert(SMMU_ID1, (7 << ID1_NUMPAGENDXB_SHIFT) | 61);
        assert_eq!(
            capture(&mut oversized_context_window),
            Err(SnapshotError::RegisterWindowOutOfRange)
        );
    }

    #[test]
    fn fails_closed_on_bad_route_metadata_or_io_error() {
        let mut bad_type = MockSmmu::sunfish();
        bad_type.set_route(5, SMR_VALID | 0x240, 3 << S2CR_TYPE_SHIFT);
        assert_eq!(
            capture(&mut bad_type),
            Err(SnapshotError::InvalidS2crType { slot: 5, value: 3 })
        );

        let mut bad_cb = MockSmmu::sunfish();
        bad_cb.set_route(4, SMR_VALID | 0x240, 61);
        assert_eq!(
            capture(&mut bad_cb),
            Err(SnapshotError::InvalidContextBank { slot: 4, value: 61 })
        );

        let mut io_error = MockSmmu::sunfish();
        io_error.fail_at = Some(SMMU_S2CR_BASE + 9 * 4);
        assert_eq!(
            capture(&mut io_error),
            Err(SnapshotError::Io {
                offset: SMMU_S2CR_BASE + 9 * 4,
                source: SMMU_S2CR_BASE + 9 * 4,
            })
        );
    }

    #[test]
    fn fails_closed_if_global_fault_status_cannot_be_read() {
        let mut io = MockSmmu::sunfish();
        io.fail_at = Some(SMMU_GLOBAL_FSR);

        assert_eq!(
            capture(&mut io),
            Err(SnapshotError::Io {
                offset: SMMU_GLOBAL_FSR,
                source: SMMU_GLOBAL_FSR,
            })
        );
        assert_eq!(io.reads, [SMMU_ID0, SMMU_ID1, SMMU_GLOBAL_FSR]);
    }
}
