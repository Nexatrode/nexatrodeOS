//! Bounded native MMIO/table adapter for a separate Sunfish SMMU experiment.
//!
//! GR0, GR1 and context bank 4 are writable; other banks are read-only. Writes are restricted to one
//! previously invalid route and that bank's planned registers. The caller
//! owns the exact uncached mappings, table frames, data pages and fault IRQ.
//! Default builds cannot mint the install executor's authority token.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::sunfish_smmu_exec::InstallIo;
#[cfg(feature = "sunfish-smmu-install-probe")]
use crate::sunfish_smmu_exec::{InstallReceipt, Installer};
use crate::sunfish_smmu_lpae::{
    LPAE_ENTRIES_PER_TABLE, LpaeS1Plan, LpaeTableBuffers, LpaeTableLevel, LpaeTablePages,
};
use crate::sunfish_smmu_program::{
    self as regs, AccessWidth, InstallStep, RegisterBlock, RegisterProfile, SmmuInstallPlan,
};
use crate::sunfish_smmu_snapshot::SmmuReadIo;
use core::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};

/// Compare one complete, exclusively retained Normal-NC table RAM page.
/// This is not an MMIO accessor, an atomic snapshot or a validation cache.
///
/// # Safety
/// Null and misaligned pointers are rejected without access. Otherwise,
/// `pointer` must retain a readable4KiB page with no concurrent CPU/device
/// writer. The caller proves Normal-NC mapping and retains the
/// surrounding full-domain validation and barriers. Each pair stays in-page.
#[cfg(feature = "sunfish-smmu-normal-nc-table-pairs")]
#[inline(never)]
unsafe fn verify_normal_nc_table_pairs(
    pointer: *const u64,
    expected: &[u64; LPAE_ENTRIES_PER_TABLE],
) -> Result<(), Error> {
    if pointer.is_null() || pointer as usize % 16 != 0 { return Err(Error::Tables); }
    const { assert!(LPAE_ENTRIES_PER_TABLE == 512); }
    for (pair, words) in expected.as_chunks::<2>().0.iter().enumerate() {
        #[cfg(target_endian = "little")]
        let value = words[0] as u128 | ((words[1] as u128) << 64);
        #[cfg(target_endian = "big")]
        let value = ((words[0] as u128) << 64) | words[1] as u128;
        // SAFETY: alignment checked above; pair0..255 spans exactly4096 bytes.
        let actual = unsafe { pointer.cast::<u128>().add(pair).read_volatile() };
        if actual != value { return Err(Error::Tables); }
    }
    Ok(())
}

pub const PAGE_BYTES: usize = 4096;
/// The stable cold census observed banks 0..3 retained and bank 4 eligible.
/// Every boot still verifies no route reference, SCTLR.M=0 and no ASID conflict.
pub const CONTEXT_BANK: usize = 4;
pub const CONTEXT_OFFSET: u32 = 0x80000 + CONTEXT_BANK as u32 * PAGE_BYTES as u32;
pub const REGISTER_PHYSICAL: [u64; 5] = [
    0x1500_0000,
    0x1500_1000,
    0x1508_4000,
    0x1508_0000,
    0x1508_5000,
];
pub const REGISTER_BYTES: [usize; 5] = [0x1000, 0x1000, 0x1000, 0x4000, 0x38000];
const FSR_FORMAT_MASK: u32 = 3 << 9;
const PHYSICAL_PAGE_MASK: u64 = ((1u64 << 40) - 1) & !0xfff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Mapping,
    Layout,
    State,
    Register,
    Tables,
    DataPage,
    SharedMemory(crate::sunfish_ipa_shared_memory::Error),
    Fault(u32),
    FaultBinding,
    ContextReferenced {
        slot: usize,
    },
    ContextActive {
        sctlr: u32,
    },
    AsidConflict {
        bank: usize,
        asid: u16,
    },
    Readback {
        offset: u32,
        expected: u64,
        actual: u64,
    },
}

/// Progress markers for the retained, read-only IPA receive-domain audit.
/// They are used by the MPSS runtime diagnostic to distinguish a slow scan
/// from a stop at a particular MMIO or retained-table read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetainedDomainStage {
    Entry,
    GlobalFaultStatus,
    ContextControl,
    ContextControlValue,
    ContextFaultStatus,
    ContextFaultStatusValue,
    FaultStatusRejected,
    FaultStatusAccepted,
    ReceiveRecheckStart,
    AsidBank,
    RouteSlot,
    Register,
    TableSetup,
    TablePage,
    Complete,
}

#[cfg(feature = "sunfish-smmu-install-probe")]
pub(crate) struct ExperimentGate {
    _private: (),
}

pub struct Native<'a, F: FnMut(usize) -> Result<(), Error>> {
    registers: [*mut u8; 5],
    table_pointers: [*mut u64; 4],
    table_physical: [u64; 4],
    // Additional exclusively retained frames for the opt-in two-root plan:
    // second L2, second L3, third L3. The four-page API stays unchanged.
    root_extra: Option<([*mut u64; 3], [u64; 3])>,
    receive_table: Option<(*mut u64, u64)>,
    // Immutable CPU-owned expectation, derived from the validated canonical
    // receive plan before installation. Never populated from live table RAM.
    #[cfg(feature = "sunfish-smmu-retained-table-expectation")]
    receive_table_expectation: Option<[[u64; LPAE_ENTRIES_PER_TABLE]; 8]>,
    receive: Option<ReceiveBacking<'a>>,
    receive_handoff_attempted: bool,
    installation_completed: bool,
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    modem_quarantined: bool,
    shared_memory: Option<crate::sunfish_ipa_shared_memory::SharedMemory<'a>>,
    data_pages: &'a [u64],
    fault: &'a AtomicBool,
    bind_fault: F,
    slot: Option<usize>,
    peer_slot: Option<usize>,
    plan: Option<SmmuInstallPlan>,
    next_step: usize,
    staged_context: Option<crate::sunfish_smmu_lpae::LpaeContextRegisters>,
    tables_staged: bool,
    selected: bool,
    failed: bool,
    progress: fn(usize, u32),
    _exclusive: PhantomData<&'a mut [u8]>,
}
#[derive(Clone, Copy)]
enum RootBacking {
    Private,
    Shared,
    Receive,
}
struct ReceiveBacking<'a> {
    memory: Option<crate::sunfish_ipa_linux_receive_native::NativeReceiveRam<'a>>,
    physical: [u64; 512],
    span: (usize, usize),
}
#[cfg(test)]
#[path = "../../tests/sunfish_ipa_shared_fixture.rs"]
mod shared_fixture;

impl<'a, F: FnMut(usize) -> Result<(), Error>> Native<'a, F> {
    /// # Safety
    /// Retain uncached NX mappings of the five exact physical register spans
    /// with RW rights only for GR0, GR1 and CB4, and exclusive RW/NX mappings
    /// of all four table frames for this lifetime, with no
    /// writable/cached aliases or concurrent SMMU writer. `data_pages` names
    /// exclusively retained allocator frames, not firmware-supplied addresses.
    /// The Microkit image/capability audit must establish these bindings and
    /// ownership of context-bank 4's and the global fault IRQs (GIC 132/97, level high).
    /// `bind_fault` arms that real IRQ and its retained fault handler; it may
    /// not acknowledge a software-only substitute as a hardware binding.
    pub unsafe fn new(
        registers: [*mut u8; 5],
        register_physical: [u64; 5],
        table_pointers: [*mut u64; 4],
        table_physical: [u64; 4],
        data_pages: &'a [u64],
        fault: &'a AtomicBool,
        bind_fault: F,
    ) -> Result<Self, Error> {
        if register_physical != REGISTER_PHYSICAL
            || registers
                .iter()
                .any(|p| p.is_null() || *p as usize % PAGE_BYTES != 0)
            || table_pointers
                .iter()
                .any(|p| p.is_null() || *p as usize % PAGE_BYTES != 0)
            || data_pages.is_empty()
            || data_pages.len() > 128
        {
            return Err(Error::Mapping);
        }
        let valid_page = |p: u64| p >= 0x8000_0000 && p < 1u64 << 40 && p % PAGE_BYTES as u64 == 0;
        for (i, page) in table_physical.iter().copied().enumerate() {
            if !valid_page(page)
                || table_physical[..i].contains(&page)
                || data_pages.contains(&page)
            {
                return Err(Error::Mapping);
            }
        }
        for (i, page) in data_pages.iter().copied().enumerate() {
            if !valid_page(page) || data_pages[..i].contains(&page) {
                return Err(Error::Mapping);
            }
        }
        let all_pointers = [
            registers[0] as usize,
            registers[1] as usize,
            registers[2] as usize,
            registers[3] as usize,
            registers[4] as usize,
            table_pointers[0] as usize,
            table_pointers[1] as usize,
            table_pointers[2] as usize,
            table_pointers[3] as usize,
        ];
        for (i, pointer) in all_pointers.iter().enumerate() {
            let bytes = REGISTER_BYTES.get(i).copied().unwrap_or(PAGE_BYTES);
            let end = pointer.checked_add(bytes).ok_or(Error::Mapping)?;
            for (j, other) in all_pointers[..i].iter().enumerate() {
                let other_bytes = REGISTER_BYTES.get(j).copied().unwrap_or(PAGE_BYTES);
                let other_end = other.checked_add(other_bytes).ok_or(Error::Mapping)?;
                if *pointer < other_end && *other < end {
                    return Err(Error::Mapping);
                }
            }
        }
        let native = Self {
            registers,
            table_pointers,
            table_physical,
            root_extra: None,
            receive_table: None,
            #[cfg(feature = "sunfish-smmu-retained-table-expectation")]
            receive_table_expectation: None,
            receive: None,
            receive_handoff_attempted: false,
            installation_completed: false,
            #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
            modem_quarantined: false,
            shared_memory: None,
            data_pages,
            fault,
            bind_fault,
            slot: None,
            peer_slot: None,
            plan: None,
            next_step: 0,
            staged_context: None,
            tables_staged: false,
            selected: false,
            failed: false,
            progress: |_, _| {},
            _exclusive: PhantomData,
        };
        let id0 = native.load32(0, 0x20);
        let id1 = native.load32(0, 0x24);
        if id0 & (1 << 27) == 0
            || id0 & (1 << 26) == 0
            || id0 & (1 << 30) == 0
            || (id0 >> 9) & 0xf != 15
            || id0 & (1 << 8) != 0
            || id0 & 0xff == 0
            || id0 & 0xff > 128
            || id1 & (1 << 31) != 0
            || (id1 >> 28) & 7 != 6
            || id1 & 0xff != 61
            || (id1 >> 16) & 0xff != 0
            || native.load32(0, 0) & 1 != 0
        {
            return Err(Error::Layout);
        }
        Ok(native)
    }

    fn load32(&self, page: usize, offset: u32) -> u32 {
        // SAFETY: callers restrict page/offset to retained aligned mappings.
        unsafe {
            self.registers[page]
                .add(offset as usize)
                .cast::<u32>()
                .read_volatile()
        }
    }

    pub fn fault_status(&self) -> (u32, u32) {
        (self.global_fault_status(), self.context_fault_status())
    }

    pub fn global_fault_status(&self) -> u32 {
        self.load32(0, 0x48)
    }

    pub fn context_fault_status(&self) -> u32 {
        self.load32(2, 0x58)
    }

    pub fn context_control(&self) -> u32 {
        self.load32(2, 0x00)
    }

    pub fn set_progress(&mut self, progress: fn(usize, u32)) {
        self.progress = progress;
    }

    fn check(&mut self) -> Result<(), Error> {
        if self.failed || self.fault.load(Ordering::Acquire) {
            self.failed = true;
            return Err(Error::State);
        }
        let (global, context) = self.fault_status();
        if global != 0 || context & !FSR_FORMAT_MASK != 0 {
            self.failed = true;
            return Err(Error::Fault(if global != 0 { global } else { context }));
        }
        Ok(())
    }

    /// Copy a validated table image into uncached frames with aligned volatile
    /// stores/readback. Ordinary memset/SIMD writes are not used on these pages.
    pub fn stage_tables(&mut self, plan: &LpaeS1Plan) -> Result<(), Error> {
        self.check()?;
        if self.tables_staged || self.selected || self.root_extra.is_some() {
            return Err(Error::State);
        }
        let pages = LpaeTablePages {
            ttbr0_root: self.table_physical[0],
            level2: self.table_physical[1],
            level3: self.table_physical[2],
            ttbr1_root: self.table_physical[3],
        };
        if plan.table_pages() != pages {
            return Err(Error::Tables);
        }
        let id2 = self.load32(0, 0x28);
        let width = |encoding: u32| -> Option<u8> {
            match encoding {
                0 => Some(32),
                1 => Some(36),
                2 => Some(40),
                3 => Some(42),
                4 => Some(44),
                5 => Some(48),
                6 => Some(52),
                _ => None,
            }
        };
        let input_bits = width((id2 >> 8) & 0xf).ok_or(Error::Layout)?;
        let ipa_bits = width(id2 & 0xf).ok_or(Error::Layout)?;
        let physical_bits = width((id2 >> 4) & 0xf).ok_or(Error::Layout)?;
        let context = plan.context();
        let table_output_bits = width(context.ttbcr2 & 7).ok_or(Error::Tables)?;
        if id2 & (1 << 12) == 0
            || 64 - (context.tcr & 0x3f) > input_bits as u64
            || table_output_bits > ipa_bits
            || self
                .table_physical
                .iter()
                .chain(self.data_pages.iter())
                .any(|p| *p >= 1u64 << physical_bits)
        {
            return Err(Error::Tables);
        }
        for write in plan.descriptor_writes() {
            if write.table == LpaeTableLevel::Level3
                && !self
                    .data_pages
                    .contains(&(write.value & PHYSICAL_PAGE_MASK))
            {
                return Err(Error::DataPage);
            }
        }
        let mut image = [[0u64; LPAE_ENTRIES_PER_TABLE]; 4];
        let [root, l2, l3, upper] = &mut image;
        plan.materialize_table_image(&mut LpaeTableBuffers {
            physical_pages: pages,
            ttbr0_root: root,
            level2: l2,
            level3: l3,
            ttbr1_root: upper,
        })
        .map_err(|_| Error::Tables)?;
        for (page, values) in image.iter().enumerate() {
            for (index, value) in values.iter().copied().enumerate() {
                // SAFETY: constructor retains the exact exclusive 4-KiB frame.
                unsafe {
                    self.table_pointers[page].add(index).write_volatile(value);
                }
            }
        }
        sync();
        for (page, values) in image.iter().enumerate() {
            for (index, value) in values.iter().copied().enumerate() {
                // SAFETY: same bounded frame and aligned index as publication.
                if unsafe { self.table_pointers[page].add(index).read_volatile() } != value {
                    self.failed = true;
                    return Err(Error::Tables);
                }
            }
        }
        self.tables_staged = true;
        self.staged_context = Some(plan.context());
        Ok(())
    }

    /// Stage exactly seven owned frames for two roots / three leaf regions.
    /// Only retained private data frames can be mapped by this experiment.
    ///
    /// # Safety
    /// `extra` names three exclusive uncached RW/NX frame mappings with the
    /// supplied allocator identities, retained for this adapter's lifetime.
    /// No other CPU/device writer may access any table frame. Actual CapDL
    /// must establish these bindings, no aliases, and reserved-memory exclusion.
    pub unsafe fn stage_root_tables(
        &mut self,
        plan: &crate::sunfish_smmu_lpae::LpaeRootS1Plan<'_>,
        extra: [*mut u64; 3],
        physical: [u64; 3],
    ) -> Result<(), Error> {
        // SAFETY: caller retains the same exact table frames; private-only
        // output validation in the common path preserves this contract.
        unsafe {
            self.stage_root_tables_inner::<3, 3, 7>(plan, extra, physical, RootBacking::Private)
        }
    }

    /// Consume and retain the real views before the first allocation write.
    /// No shared item is returned as an independently forgeable mapping grant.
    pub fn attach_shared_memory(
        &mut self,
        memory: crate::sunfish_ipa_shared_memory::SharedMemory<'a>,
    ) -> Result<(), Error> {
        self.check()?;
        if self.tables_staged || self.selected || self.shared_memory.is_some() {
            return Err(Error::State);
        }
        for (start, bytes) in memory.virtual_spans() {
            let end = start.checked_add(bytes).ok_or(Error::Mapping)?;
            for (pointer, length) in self
                .registers
                .iter()
                .enumerate()
                .map(|(i, p)| (*p as usize, REGISTER_BYTES[i]))
                .chain(
                    self.table_pointers
                        .iter()
                        .map(|p| (*p as usize, PAGE_BYTES)),
                )
            {
                if start < pointer + length && pointer < end {
                    return Err(Error::Mapping);
                }
            }
        }
        if self
            .table_physical
            .iter()
            .chain(self.data_pages.iter())
            .any(|p| crate::sunfish_ipa_shared_memory::SharedMemory::excludes_private_page(*p))
        {
            return Err(Error::Mapping);
        }
        self.shared_memory = Some(memory);
        match self.shared_memory.as_mut().unwrap().prepare() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.failed = true;
                Err(Error::SharedMemory(error))
            }
        }
    }
    pub fn shared_report(&self) -> Option<crate::sunfish_ipa_smem::Report> {
        self.shared_memory.as_ref().map(|m| m.report())
    }
    pub fn shared_item_physical(&self) -> Option<u64> {
        self.shared_memory.as_ref().and_then(|m| m.item_physical())
    }
    /// Extend the actual retained IPA allocation after the installed domain.
    /// Only a Window constructed with the audited modem subview can attach.
    /// The installation write path stays consumed. Post-install operations
    /// revalidate the retained domain and have their own terminal latch.
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    fn with_retained_modem<T>(
        &mut self,
        action: impl FnOnce(
            &mut crate::sunfish_ipa_shared_memory::SharedMemory<'a>,
        ) -> Result<T, crate::sunfish_ipa_shared_memory::Error>,
    ) -> Result<T, Error> {
        if self.modem_quarantined {
            return Err(Error::State);
        }
        // Retain the quarantine on errors and unwind, including any uncertain
        // child operation. Never reopen the install executor's `failed` latch.
        self.modem_quarantined = true;
        self.retained_receive_domain()?;
        let result = action(self.shared_memory.as_mut().ok_or(Error::State)?)
            .map_err(Error::SharedMemory)?;
        self.retained_receive_domain()?;
        self.modem_quarantined = false;
        Ok(result)
    }
    #[cfg(feature = "sunfish-native-startup-mpss-glink")]
    pub(crate) fn with_glink<T>(
        &mut self,
        action: impl FnOnce(
            &mut crate::sunfish_ipa_shared_memory::Smp<'a>,
        ) -> Result<T, crate::sunfish_mpss_linux_smp2p::GlinkError>,
    ) -> Result<T, Error> {
        self.with_retained_modem(|memory| memory.glink(action))
    }
    #[cfg(feature = "sunfish-tftp-barrier-domain-observer")]
    pub(crate) fn with_glink_observed<T>(
        &mut self,
        action: impl FnOnce(
            &mut crate::sunfish_ipa_shared_memory::Smp<'a>,
        ) -> Result<T, crate::sunfish_mpss_linux_smp2p::GlinkError>,
        mut observe: impl FnMut(usize),
        #[cfg(feature="sunfish-wrq-domain-costs")]
        mut stage:impl FnMut(usize,RetainedDomainStage,usize),
    ) -> Result<T, Error> {
        if self.modem_quarantined { return Err(Error::State); }
        self.modem_quarantined = true;
        observe(0);
        #[cfg(not(feature="sunfish-wrq-domain-costs"))]
        let checked = self.retained_receive_domain();
        #[cfg(feature="sunfish-wrq-domain-costs")]
        let checked = self.retained_receive_domain_with(|s,i|stage(0,s,i));
        observe(1);
        checked?;
        let result = self.shared_memory.as_mut().ok_or(Error::State)?
            .glink(action).map_err(Error::SharedMemory)?;
        observe(2);
        #[cfg(not(feature="sunfish-wrq-domain-costs"))]
        let checked = self.retained_receive_domain();
        #[cfg(feature="sunfish-wrq-domain-costs")]
        let checked = self.retained_receive_domain_with(|s,i|stage(1,s,i));
        observe(3);
        checked?;
        self.modem_quarantined = false;
        Ok(result)
    }
    #[cfg(feature = "sunfish-native-startup-mpss-glink")]
    pub(crate) fn quarantine_glink(&mut self) {
        self.modem_quarantined = true;
    }
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub fn attach_modem_smem(
        &mut self,
        generation: u64,
        gate: fn(u64) -> Result<(), crate::sunfish_mpss_linux_smem::Error>,
    ) -> Result<(), Error> {
        self.with_retained_modem(|memory| memory.attach_modem(generation, gate))
    }
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub fn modem_smem_report(&self) -> Option<crate::sunfish_mpss_linux_smem::Report> {
        self.shared_memory.as_ref().and_then(|m| m.modem_report())
    }
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub fn modem_smem_step(&mut self) -> Result<crate::sunfish_mpss_linux_smem::Phase, Error> {
        self.with_retained_modem(|memory| memory.modem_step())
    }
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    pub fn take_modem_smem_prepared(
        &mut self,
    ) -> Result<crate::sunfish_mpss_linux_smem::Prepared, Error> {
        self.with_retained_modem(|memory| memory.take_modem_prepared())
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub fn activate_modem_smp2p(
        &mut self,
        prepared: crate::sunfish_mpss_linux_smem::Prepared,
        signal: crate::sunfish_mpss_linux_smp2p_native::Platform<'a>,
    ) -> Result<(), Error> {
        // Retain before checking the outer owner, including an earlier fault.
        let signal = core::mem::ManuallyDrop::new(signal);
        self.with_retained_modem(|memory| {
            memory.activate_smp2p(prepared, core::mem::ManuallyDrop::into_inner(signal))
        })
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub fn modem_smp2p_report(&self) -> Option<crate::sunfish_mpss_linux_smp2p::Report> {
        self.shared_memory.as_ref().and_then(|m| m.smp2p_report())
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub fn modem_smp2p_poll(&mut self) -> Result<crate::sunfish_mpss_linux_smp2p::Poll, Error> {
        self.with_retained_modem(|memory| memory.smp2p_poll())
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smp2p")]
    pub fn take_mpss_ready_observation(
        &mut self,
    ) -> Result<crate::sunfish_mpss_linux_smp2p::ReadyObservation, Error> {
        self.with_retained_modem(|memory| memory.take_mpss_ready_observation())
    }
    pub fn shared_mappings(
        &mut self,
    ) -> Result<[crate::sunfish_smmu_lpae::LpaePageMapping; 37], Error> {
        self.check()?;
        self.shared_memory
            .as_ref()
            .ok_or(Error::State)?
            .mappings(self.data_pages)
            .map_err(Error::SharedMemory)
    }
    /// Retain the actual sole CPU backend before validating or staging the
    /// receive mapping. Its unsafe constructor's CapDL ownership contract
    /// still applies; numerical frame identities alone are insufficient.
    pub fn attach_receive_memory(
        &mut self,
        memory: crate::sunfish_ipa_linux_receive_native::NativeReceiveRam<'a>,
    ) -> Result<(), Error> {
        self.check()?;
        if self.tables_staged
            || self.selected
            || self.receive.is_some()
            || self.shared_memory.is_none()
        {
            return Err(Error::State);
        }
        let (physical, span) = memory.identity();
        self.receive = Some(ReceiveBacking {
            memory: Some(memory),
            physical,
            span,
        });
        let result = (|| {
            if physical.iter().any(|p| {
                self.table_physical.contains(p)
                    || self.data_pages.contains(p)
                    || crate::sunfish_ipa_shared_memory::SharedMemory::excludes_private_page(*p)
            }) {
                return Err(Error::Mapping);
            }
            let (start, bytes) = span;
            let end = start.checked_add(bytes).ok_or(Error::Mapping)?;
            for (pointer, length) in self
                .registers
                .iter()
                .enumerate()
                .map(|(i, p)| (*p as usize, REGISTER_BYTES[i]))
                .chain(
                    self.table_pointers
                        .iter()
                        .map(|p| (*p as usize, PAGE_BYTES)),
                )
                .chain(self.shared_memory.iter().flat_map(|m| m.virtual_spans()))
            {
                let other_end = pointer.checked_add(length).ok_or(Error::Mapping)?;
                if start < other_end && pointer < end {
                    return Err(Error::Mapping);
                }
            }
            self.receive_mapping_inner()?;
            Ok(())
        })();
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn receive_mapping_inner(
        &self,
    ) -> Result<crate::sunfish_ipa_linux_receive_domain::Mapping, Error> {
        self.receive_mapping_inner_with(&mut |_, _| {})
    }
    #[inline(never)]
    fn receive_mapping_inner_with(
        &self,
        progress: &mut impl FnMut(RetainedDomainStage, usize),
    ) -> Result<crate::sunfish_ipa_linux_receive_domain::Mapping, Error> {
        progress(RetainedDomainStage::TableSetup, 7);
        let shared = self
            .shared_memory
            .as_ref()
            .ok_or(Error::State)?;
        progress(RetainedDomainStage::TableSetup, 8);
        let shared = shared
            .mappings(self.data_pages)
            .map_err(Error::SharedMemory)?;
        progress(RetainedDomainStage::TableSetup, 9);
        let receive = self.receive.as_ref().ok_or(Error::State)?;
        progress(RetainedDomainStage::TableSetup, 10);
        let mapping = crate::sunfish_ipa_linux_receive_domain::Mapping::new_with_progress(
            &shared, &receive.physical,
            &mut |step| progress(RetainedDomainStage::TableSetup, step),
        ).map_err(|_| Error::DataPage)?;
        progress(RetainedDomainStage::TableSetup, 11);
        Ok(mapping)
    }
    pub fn receive_mappings(
        &mut self,
    ) -> Result<crate::sunfish_ipa_linux_receive_domain::Mapping, Error> {
        self.check()?;
        self.receive_mapping_inner()
    }
    /// # Safety
    /// Retain four exclusive uncached RW/NX extra frames: second L2, command L3, shared L3, receive L3.
    /// With pair reads enabled, all eight frames (including Native::new's four) must be Normal-NC
    /// and have no concurrent CPU/device writer throughout retention. CapDL must prove identities,
    /// reserved-memory exclusion and absence of aliases. Receive RAM and shared owner stay retained.
    pub unsafe fn stage_receive_root_tables(
        &mut self,
        plan: &crate::sunfish_smmu_lpae::LpaeRootS1Plan<'_, 4>,
        extra: [*mut u64; 4],
        physical: [u64; 4],
    ) -> Result<(), Error> {
        // SAFETY: caller retains the full exact set of four extra frames.
        let result = unsafe {
            self.stage_root_tables_inner::<4, 4, 8>(plan, extra, physical, RootBacking::Receive)
        };
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    /// Stage the real shared pages only while retaining their live owner.
    /// # Safety
    /// The extra table-frame contract is identical to stage_root_tables.
    pub unsafe fn stage_shared_root_tables(
        &mut self,
        plan: &crate::sunfish_smmu_lpae::LpaeRootS1Plan<'_>,
        extra: [*mut u64; 3],
        physical: [u64; 3],
    ) -> Result<(), Error> {
        // SAFETY: caller retains the extra table mappings, and the live owner
        // check in the common path admits only its exact shared pages.
        unsafe {
            self.stage_root_tables_inner::<3, 3, 7>(plan, extra, physical, RootBacking::Shared)
        }
    }
    unsafe fn stage_root_tables_inner<
        const LEAVES: usize,
        const EXTRA: usize,
        const TABLES: usize,
    >(
        &mut self,
        plan: &crate::sunfish_smmu_lpae::LpaeRootS1Plan<'_, LEAVES>,
        extra: [*mut u64; EXTRA],
        physical: [u64; EXTRA],
        backing: RootBacking,
    ) -> Result<(), Error> {
        self.check()?;
        if self.tables_staged || self.selected || self.root_extra.is_some() {
            return Err(Error::State);
        }
        if !matches!((LEAVES, EXTRA, TABLES), (3, 3, 7) | (4, 4, 8))
            || matches!(backing, RootBacking::Receive) != (LEAVES == 4)
            || matches!(backing, RootBacking::Receive) != self.receive.is_some()
        {
            return Err(Error::State);
        }
        let expected_pages: crate::sunfish_smmu_lpae::LpaeRootTablePages<LEAVES> =
            crate::sunfish_smmu_lpae::LpaeRootTablePages {
                ttbr0_root: self.table_physical[0],
                level2: [self.table_physical[1], physical[0]],
                level3: core::array::from_fn(|n| match n {
                    0 => self.table_physical[2],
                    1 => physical[1],
                    2 if LEAVES == 4 => physical[3],
                    _ => physical[2],
                }),
                ttbr1_root: self.table_physical[3],
            };
        let all_physical: [u64; TABLES] = core::array::from_fn(|n| match n {
            0 => expected_pages.ttbr0_root,
            1 | 2 => expected_pages.level2[n - 1],
            n if n < 3 + LEAVES => expected_pages.level3[n - 3],
            _ => expected_pages.ttbr1_root,
        });
        if plan.table_pages() != expected_pages {
            return Err(Error::Tables);
        }
        for (i, page) in all_physical.iter().copied().enumerate() {
            if page < 0x8000_0000
                || page >= 1u64 << 40
                || page & 4095 != 0
                || all_physical[..i].contains(&page)
                || self.data_pages.contains(&page)
                || self
                    .receive
                    .as_ref()
                    .is_some_and(|r| r.physical.contains(&page))
                || (self.shared_memory.is_some()
                    && crate::sunfish_ipa_shared_memory::SharedMemory::excludes_private_page(page))
            {
                return Err(Error::Mapping);
            }
        }
        for (i, pointer) in extra.iter().copied().enumerate() {
            let address = pointer as usize;
            let end = address.checked_add(PAGE_BYTES).ok_or(Error::Mapping)?;
            if address == 0 || address % PAGE_BYTES != 0 {
                return Err(Error::Mapping);
            }
            for (other, bytes) in self
                .registers
                .iter()
                .enumerate()
                .map(|(i, p)| (*p as usize, REGISTER_BYTES[i]))
                .chain(
                    self.table_pointers
                        .iter()
                        .map(|p| (*p as usize, PAGE_BYTES)),
                )
                .chain(extra[..i].iter().map(|p| (*p as usize, PAGE_BYTES)))
                .chain(self.shared_memory.iter().flat_map(|m| m.virtual_spans()))
                .chain(self.receive.iter().map(|r| r.span))
            {
                let other_end = other.checked_add(bytes).ok_or(Error::Mapping)?;
                if address < other_end && other < end {
                    return Err(Error::Mapping);
                }
            }
        }
        match backing {
            RootBacking::Private => {
                if plan
                    .mappings()
                    .iter()
                    .any(|mapping| !self.data_pages.contains(&mapping.physical_page))
                {
                    return Err(Error::DataPage);
                }
            }
            RootBacking::Shared => {
                self.shared_memory
                    .as_ref()
                    .ok_or(Error::State)?
                    .validate_mappings(plan.mappings(), self.data_pages)
                    .map_err(Error::SharedMemory)?;
            }
            RootBacking::Receive => {
                let actual = self.receive_mapping_inner()?;
                let canonical = actual
                    .plan(crate::sunfish_smmu_lpae::LpaeRootTablePages {
                        ttbr0_root: expected_pages.ttbr0_root,
                        level2: expected_pages.level2,
                        level3: [
                            self.table_physical[2],
                            physical[1],
                            physical[3],
                            physical[2],
                        ],
                        ttbr1_root: expected_pages.ttbr1_root,
                    })
                    .map_err(|_| Error::Tables)?;
                // Compare complete metadata and context/attribute policy,
                // not just an address list supplied by the caller.
                if plan.mappings() != canonical.mappings() || plan.context() != canonical.context()
                {
                    return Err(Error::DataPage);
                }
                let mut expected = [[0u64; LPAE_ENTRIES_PER_TABLE]; 8];
                canonical
                    .materialize_table_image(canonical.table_pages(), &mut expected)
                    .map_err(|_| Error::Tables)?;
                let mut supplied = [[0u64; LPAE_ENTRIES_PER_TABLE]; TABLES];
                plan.materialize_table_image(plan.table_pages(), &mut supplied)
                    .map_err(|_| Error::Tables)?;
                if supplied.as_slice() != expected.as_slice() {
                    return Err(Error::Tables);
                }
            }
        }
        let id2 = self.load32(0, 0x28);
        let width = |value| match value {
            0 => Some(32),
            1 => Some(36),
            2 => Some(40),
            3 => Some(42),
            4 => Some(44),
            5 => Some(48),
            6 => Some(52),
            _ => None,
        };
        let input_bits = width((id2 >> 8) & 15).ok_or(Error::Layout)?;
        let ipa_bits = width(id2 & 15).ok_or(Error::Layout)?;
        let physical_bits = width((id2 >> 4) & 15).ok_or(Error::Layout)?;
        let context = plan.context();
        let output_bits = width(context.ttbcr2 & 7).ok_or(Error::Tables)?;
        if id2 & (1 << 12) == 0
            || 64 - (context.tcr & 63) > input_bits
            || output_bits > ipa_bits
            || all_physical
                .iter()
                .chain(self.data_pages.iter())
                .chain(plan.mappings().iter().map(|mapping| &mapping.physical_page))
                .any(|page| *page >= 1u64 << physical_bits)
        {
            return Err(Error::Tables);
        }
        let mut image = [[0u64; LPAE_ENTRIES_PER_TABLE]; TABLES];
        plan.materialize_table_image(plan.table_pages(), &mut image)
            .map_err(|_| Error::Tables)?;
        let pointers: [*mut u64; TABLES] = core::array::from_fn(|n| match n {
            0 => self.table_pointers[0],
            1 => self.table_pointers[1],
            2 => extra[0],
            3 => self.table_pointers[2],
            4 => extra[1],
            5 if LEAVES == 4 => extra[3],
            5 => extra[2],
            6 if LEAVES == 4 => extra[2],
            _ => self.table_pointers[3],
        });
        // Consume/retain all selected frames before the first potentially
        // uncertain publication. No failed table write is retried or freed.
        self.root_extra = Some((
            [extra[0], extra[1], extra[2]],
            [physical[0], physical[1], physical[2]],
        ));
        if LEAVES == 4 {
            self.receive_table = Some((extra[3], physical[3]));
        }
        for (p, values) in pointers.iter().zip(image.iter()) {
            for (i, value) in values.iter().copied().enumerate() {
                // SAFETY: all exact, exclusive 4-KiB frame mappings checked above.
                unsafe { p.add(i).write_volatile(value) };
            }
        }
        sync();
        for (p, values) in pointers.iter().zip(image.iter()) {
            for (i, value) in values.iter().copied().enumerate() {
                // SAFETY: same retained, bounded scalar mapping as publication.
                if unsafe { p.add(i).read_volatile() } != value {
                    self.failed = true;
                    return Err(Error::Tables);
                }
            }
        }
        #[cfg(feature = "sunfish-smmu-retained-table-expectation")]
        if matches!(backing, RootBacking::Receive) {
            // The supplied image was compared to the independently rebuilt
            // canonical image above, then published and read back in full.
            // All geometry/ownership inputs are private or immutably borrowed
            // for this adapter's lifetime. Only the live table RAM can change.
            self.receive_table_expectation = Some(core::array::from_fn(|page| image[page]));
        }
        self.tables_staged = true;
        self.staged_context = Some(context);
        Ok(())
    }

    /// Restrict the adapter before executing the plan. No MMIO write occurs.
    pub fn select(&mut self, plan: &SmmuInstallPlan) -> Result<(), Error> {
        self.check()?;
        if self.selected || !self.tables_staged || plan.profile() != RegisterProfile::LinuxReference
        {
            return Err(Error::State);
        }
        let route = plan.route_preimage().ok_or(Error::State)?;
        if route.context_bank() != CONTEXT_BANK
            || plan
                .context_preimage()
                .is_none_or(|p| p.context_bank() != CONTEXT_BANK)
            || route.slot() >= (self.load32(0, 0x20) & 0xff) as usize
        {
            return Err(Error::Mapping);
        }
        // Even invalid SMRs can retain a translation reference; never steal it.
        for slot in 0..(self.load32(0, 0x20) & 0xff) {
            let s2cr = self.load32(0, 0xc00 + slot * 4);
            if (s2cr >> 16) & 3 == 0 && s2cr & 0xff == CONTEXT_BANK as u32 {
                return Err(Error::ContextReferenced {
                    slot: slot as usize,
                });
            }
        }
        let context = self.staged_context.ok_or(Error::Tables)?;
        if self.root_extra.is_some() {
            if !plan.steps().contains(&self.root_publication()?) {
                return Err(Error::Tables);
            }
        }
        if self.receive_table.is_some() {
            let peer = plan.peer_route_preimage().ok_or(Error::Mapping)?;
            for (slot, value) in [(route.slot(), 0x80000520), (peer.slot(), 0x80000522)] {
                if !plan.steps().contains(&InstallStep::Write {
                    block: RegisterBlock::Global0,
                    index: 0,
                    offset: regs::GR0_SMR_BASE + slot as u32 * 4,
                    width: AccessWidth::U32,
                    value,
                }) {
                    return Err(Error::Mapping);
                }
            }
        }
        let before = plan.context_preimage().ok_or(Error::State)?;
        let sctlr = self.load32(2, regs::CB_SCTLR);
        if sctlr & 1 != 0 {
            return Err(Error::ContextActive { sctlr });
        }
        let asid = (context.ttbr0 >> 48) as u16;
        // Match Linux's ASID=CB index policy. Never reuse a live ASID from a
        // retained bootloader context, including an enabled TTBR1 context.
        if asid != CONTEXT_BANK as u16 || (context.ttbr1 >> 48) as u16 != asid {
            return Err(Error::Tables);
        }
        self.check_asid(asid)?;
        for (offset, width, value) in [
            (regs::CB_TTBCR2, AccessWidth::U32, context.ttbcr2 as u64),
            (regs::CB_TTBCR, AccessWidth::U32, context.tcr as u32 as u64),
            (regs::CB_TTBR0, AccessWidth::U64, context.ttbr0),
            (regs::CB_TTBR1, AccessWidth::U64, context.ttbr1),
            (regs::CB_MAIR0, AccessWidth::U32, context.mair0 as u64),
            (regs::CB_MAIR1, AccessWidth::U32, context.mair1 as u64),
            (
                regs::CB_ACTLR,
                AccessWidth::U32,
                regs::linux_reference_actlr(before.actlr()) as u64,
            ),
        ] {
            if !plan.steps().contains(&InstallStep::Write {
                block: RegisterBlock::ContextBank,
                index: CONTEXT_BANK,
                offset,
                width,
                value,
            }) {
                return Err(Error::Tables);
            }
        }
        if let Some(peer) = plan.peer_route_preimage() {
            if peer.context_bank() != CONTEXT_BANK
                || peer.slot() == route.slot()
                || peer.slot() >= (self.load32(0, 0x20) & 0xff) as usize
            {
                return Err(Error::Mapping);
            }
            self.peer_slot = Some(peer.slot());
        }
        self.slot = Some(route.slot());
        self.plan = Some(*plan);
        self.selected = true;
        Ok(())
    }

    fn receive_table_pages(
        &self,
    ) -> Result<crate::sunfish_smmu_lpae::LpaeRootTablePages<4>, Error> {
        let (_, extra) = self.root_extra.ok_or(Error::Tables)?;
        let (_, receive) = self.receive_table.ok_or(Error::Tables)?;
        Ok(crate::sunfish_smmu_lpae::LpaeRootTablePages {
            ttbr0_root: self.table_physical[0],
            level2: [self.table_physical[1], extra[0]],
            level3: [self.table_physical[2], extra[1], receive, extra[2]],
            ttbr1_root: self.table_physical[3],
        })
    }
    fn root_publication(&self) -> Result<InstallStep, Error> {
        let (_, extra) = self.root_extra.ok_or(Error::Tables)?;
        if self.receive_table.is_some() {
            if self.receive.is_none() {
                return Err(Error::State);
            }
            Ok(InstallStep::PublishReceiveRootPageTables {
                physical_pages: self.receive_table_pages()?.pages(),
                page_bytes: PAGE_BYTES as u64,
            })
        } else {
            Ok(InstallStep::PublishRootPageTables {
                physical_pages: [
                    self.table_physical[0],
                    self.table_physical[1],
                    extra[0],
                    self.table_physical[2],
                    extra[1],
                    extra[2],
                    self.table_physical[3],
                ],
                page_bytes: PAGE_BYTES as u64,
            })
        }
    }

    /// Recheck the actual programmed route/context and all eight table
    /// images, even after the install executor has consumed its write path.
    /// This is a retained-domain observation, not a DMA/packet Ready token.
    pub fn retained_receive_domain(&self) -> Result<(), Error> {
        self.retained_receive_domain_with(|_, _| {})
    }

    /// Same read-only revalidation with bounded diagnostic progress. The
    /// callback runs before each potentially blocking MMIO/table-read group.
    pub fn retained_receive_domain_with(
        &self,
        mut progress: impl FnMut(RetainedDomainStage, usize),
    ) -> Result<(), Error> {
        progress(RetainedDomainStage::Entry, 0);
        if !self.installation_completed
            || !self.tables_staged
            || !self.selected
            || self.fault.load(Ordering::Acquire)
        {
            return Err(Error::State);
        }
        let plan = self.plan.as_ref().ok_or(Error::State)?;
        if self.next_step != plan.steps().len() {
            return Err(Error::State);
        }
        progress(RetainedDomainStage::GlobalFaultStatus, 0);
        let global = self.load32(0, 0x48);
        progress(RetainedDomainStage::ContextControl, 0);
        let _context_control = self.load32(2, 0x00);
        progress(
            RetainedDomainStage::ContextControlValue,
            _context_control as usize,
        );
        progress(RetainedDomainStage::ContextFaultStatus, 0);
        let context = self.load32(2, 0x58);
        progress(
            RetainedDomainStage::ContextFaultStatusValue,
            context as usize,
        );
        if global != 0 || context & !FSR_FORMAT_MASK != 0 {
            return Err(Error::Fault(if global != 0 { global } else { context }));
        }
        self.check_asid_with(CONTEXT_BANK as u16, |bank| {
            progress(RetainedDomainStage::AsidBank, bank)
        })?;
        let slots = (self.load32(0, 0x20) & 0xff) as usize;
        for slot in 0..slots {
            progress(RetainedDomainStage::RouteSlot, slot);
            let s2cr = self.load32(0, 0xc00 + slot as u32 * 4);
            if (s2cr >> 16) & 3 == 0
                && s2cr & 0xff == CONTEXT_BANK as u32
                && Some(slot) != self.slot
                && Some(slot) != self.peer_slot
            {
                return Err(Error::ContextReferenced { slot });
            }
        }
        for (at, step) in plan.steps().iter().enumerate() {
            if let InstallStep::Write {
                block,
                index,
                offset,
                width,
                value,
            } = *step
            {
                // Installation first disables the bank, then enables it.
                // Observe the final planned value of each stable register,
                // rather than requiring superseded intermediate values.
                if plan.steps()[at + 1..].iter().any(|later| {
                    matches!(*later,
                    InstallStep::Write{block:b,index:i,offset:o,width:w,..}
                    if (b,i,o,w)==(block,index,offset,width))
                }) {
                    continue;
                }
                if block == RegisterBlock::ContextBank
                    && [regs::CB_S1_TLBIASID, regs::CB_TLBSYNC].contains(&offset)
                {
                    continue;
                }
                progress(RetainedDomainStage::Register, at);
                let page = self.resolve(block, index, offset, width == AccessWidth::U64, false)?;
                let actual = match width {
                    AccessWidth::U32 => self.load32(page, offset) as u64,
                    // SAFETY: selected plan, retained aligned bank and bounded offset.
                    AccessWidth::U64 => unsafe {
                        self.registers[page]
                            .add(offset as usize)
                            .cast::<u64>()
                            .read_volatile()
                    },
                };
                let mask = if width == AccessWidth::U32 {
                    readback_mask(block, offset) as u64
                } else {
                    u64::MAX
                };
                if (actual ^ value) & mask != 0 {
                    return Err(Error::Readback {
                        offset,
                        expected: value,
                        actual,
                    });
                }
            }
        }
        progress(RetainedDomainStage::TableSetup, 0);
        self.verify_receive_tables_with(&mut progress)?;
        progress(RetainedDomainStage::Complete, 0);
        Ok(())
    }

    // Keep the eight-table image out of the entry frame. In particular, the
    // caller can report early domain progress before this workspace is needed.
    #[inline(never)]
    fn verify_receive_tables_with(
        &self,
        progress: &mut impl FnMut(RetainedDomainStage, usize),
    ) -> Result<(), Error> {
        // The caller reports step 0 before entering this function. These
        // checkpoints bracket preparation without skipping any validation.
        progress(RetainedDomainStage::TableSetup, 1);
        #[cfg(not(feature = "sunfish-smmu-retained-table-expectation"))]
        let mapping = self.receive_mapping_inner_with(progress)?;
        progress(RetainedDomainStage::TableSetup, 2);
        #[cfg(not(feature = "sunfish-smmu-retained-table-expectation"))]
        let pages = self.receive_table_pages()?;
        progress(RetainedDomainStage::TableSetup, 3);
        #[cfg(not(feature = "sunfish-smmu-retained-table-expectation"))]
        let canonical = mapping.plan(pages).map_err(|_| Error::Tables)?;
        progress(RetainedDomainStage::TableSetup, 4);
        #[cfg(not(feature = "sunfish-smmu-retained-table-expectation"))]
        let mut image = [0u64; LPAE_ENTRIES_PER_TABLE];
        #[cfg(feature = "sunfish-smmu-retained-table-expectation")]
        let expectation = self.receive_table_expectation.as_ref().ok_or(Error::Tables)?;
        let (extra, _) = self.root_extra.ok_or(Error::Tables)?;
        let (receive, _) = self.receive_table.ok_or(Error::Tables)?;
        let pointers = [
            self.table_pointers[0],
            self.table_pointers[1],
            extra[0],
            self.table_pointers[2],
            extra[1],
            receive,
            extra[2],
            self.table_pointers[3],
        ];
        progress(RetainedDomainStage::TableSetup, 5);
        sync();
        progress(RetainedDomainStage::TableSetup, 6);
        for (page, pointer) in pointers.iter().enumerate() {
            progress(RetainedDomainStage::TablePage, page);
            #[cfg(not(feature = "sunfish-smmu-retained-table-expectation"))]
            canonical.materialize_table_page(pages, page, &mut image).map_err(|_| Error::Tables)?;
            #[cfg(not(feature = "sunfish-smmu-retained-table-expectation"))]
            let values = &image;
            #[cfg(feature = "sunfish-smmu-retained-table-expectation")]
            let values = &expectation[page];
            #[cfg(feature = "sunfish-smmu-normal-nc-table-pairs")]
            // SAFETY: these are the eight exclusively owned, page-aligned table
            // RAM frames; the opt-in SDK/mapping audit proves Normal-NC memory.
            unsafe { verify_normal_nc_table_pairs(*pointer, values)?; }
            #[cfg(all(feature = "sunfish-smmu-grouped-table-check", not(feature = "sunfish-smmu-normal-nc-table-pairs")))]
            {
                const GROUP_WORDS: usize = 32;
                const { assert!(LPAE_ENTRIES_PER_TABLE % GROUP_WORDS == 0); }
                for (group, words) in values.as_chunks::<GROUP_WORDS>().0.iter().enumerate() {
                    for (offset, &expected) in words.iter().enumerate() {
                        // SAFETY: each complete group lies within the retained table frame.
                        if unsafe { pointer.add(group * GROUP_WORDS + offset).read_volatile() } != expected {
                            return Err(Error::Tables);
                        }
                    }
                }
            }
            #[cfg(all(not(feature = "sunfish-smmu-grouped-table-check"), not(feature = "sunfish-smmu-normal-nc-table-pairs")))]
            for (n, value) in values.iter().enumerate() {
                // SAFETY: these exact eight frames were retained before staging.
                if unsafe { pointer.add(n).read_volatile() } != *value {
                    return Err(Error::Tables);
                }
            }
        }
        sync();
        Ok(())
    }
    /// One transfer of the sole CPU writer after real install completion and
    /// retained-domain revalidation. Every failed handoff is consumed too.
    pub fn take_receive_memory(
        &mut self,
    ) -> Result<crate::sunfish_ipa_linux_receive_native::NativeReceiveRam<'a>, Error> {
        if self.receive_handoff_attempted {
            return Err(Error::State);
        }
        self.receive_handoff_attempted = true;
        self.retained_receive_domain()?;
        self.receive
            .as_mut()
            .and_then(|r| r.memory.take())
            .ok_or(Error::State)
    }

    fn bank_address(bank: usize, offset: u32) -> Result<(usize, u32), Error> {
        if bank >= 61 || offset >= PAGE_BYTES as u32 || offset % 4 != 0 {
            return Err(Error::Register);
        }
        Ok(if bank < CONTEXT_BANK {
            (3, bank as u32 * PAGE_BYTES as u32 + offset)
        } else if bank == CONTEXT_BANK {
            (2, offset)
        } else {
            (
                4,
                (bank - CONTEXT_BANK - 1) as u32 * PAGE_BYTES as u32 + offset,
            )
        })
    }

    fn check_asid(&self, asid: u16) -> Result<(), Error> {
        self.check_asid_with(asid, |_| {})
    }

    fn check_asid_with(
        &self,
        asid: u16,
        mut progress: impl FnMut(usize),
    ) -> Result<(), Error> {
        for bank in 0..61 {
            progress(bank);
            if bank == CONTEXT_BANK {
                continue;
            }
            let (page, at) = Self::bank_address(bank, regs::CB_SCTLR)?;
            if self.load32(page, at) & 1 == 0 {
                continue;
            }
            // Conservatively reject a matching ASID in either TTBR. This does
            // not depend on another context's translation stage or EPD bits.
            for offset in [regs::CB_TTBR0 + 4, regs::CB_TTBR1 + 4] {
                let (page, at) = Self::bank_address(bank, offset)?;
                if (self.load32(page, at) >> 16) as u16 == asid {
                    return Err(Error::AsidConflict { bank, asid });
                }
            }
        }
        Ok(())
    }

    fn expected_step(&self) -> Result<InstallStep, Error> {
        self.plan
            .as_ref()
            .and_then(|plan| plan.steps().get(self.next_step))
            .copied()
            .ok_or(Error::State)
    }

    fn expect_write(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), Error> {
        if self.expected_step()?
            != (InstallStep::Write {
                block,
                index,
                offset,
                width,
                value,
            })
        {
            self.failed = true;
            return Err(Error::State);
        }
        Ok(())
    }

    fn resolve(
        &self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        wide: bool,
        write: bool,
    ) -> Result<usize, Error> {
        if offset % if wide { 8 } else { 4 } != 0
            || offset as usize + if wide { 8 } else { 4 } > PAGE_BYTES
        {
            return Err(Error::Register);
        }
        let page = match (block, index) {
            (RegisterBlock::Global0, 0) => 0,
            (RegisterBlock::Global1, 0) => 1,
            (RegisterBlock::ContextBank, CONTEXT_BANK) => 2,
            _ => return Err(Error::Register),
        };
        if write {
            let slot = self.slot.ok_or(Error::State)? as u32;
            let allowed = match page {
                0 => {
                    !wide
                        && core::iter::once(slot)
                            .chain(self.peer_slot.map(|s| s as u32))
                            .any(|slot| {
                                [
                                    regs::GR0_SMR_BASE + slot * 4,
                                    regs::GR0_S2CR_BASE + slot * 4,
                                ]
                                .contains(&offset)
                            })
                }
                1 => {
                    !wide
                        && [
                            regs::GR1_CBAR_BASE + CONTEXT_BANK as u32 * 4,
                            regs::GR1_CBA2R_BASE + CONTEXT_BANK as u32 * 4,
                        ]
                        .contains(&offset)
                }
                _ if wide => [regs::CB_TTBR0, regs::CB_TTBR1].contains(&offset),
                _ => [
                    regs::CB_SCTLR,
                    regs::CB_ACTLR,
                    regs::CB_TTBCR2,
                    regs::CB_TTBCR,
                    regs::CB_MAIR0,
                    regs::CB_MAIR1,
                    regs::CB_S1_TLBIASID,
                    regs::CB_TLBSYNC,
                ]
                .contains(&offset),
            };
            if !allowed {
                return Err(Error::Register);
            }
        }
        Ok(page)
    }

    #[cfg(feature = "sunfish-smmu-install-probe")]
    pub fn install_experiment(
        &mut self,
        plan: &SmmuInstallPlan,
        polls: u16,
    ) -> Result<InstallReceipt, crate::sunfish_smmu_exec::InstallError<Error>> {
        let authority =
            crate::sunfish_smmu_exec::InstallAuthority::from_experiment_gate(ExperimentGate {
                _private: (),
            });
        // A fresh installer does not permit this adapter to replay: select and
        // this consumed state remain one-shot even after any executor failure.
        if !self.selected || self.failed || self.plan != Some(*plan) {
            return Err(crate::sunfish_smmu_exec::InstallError::Quarantined);
        }
        let result = Installer::default().install(authority, self, plan, polls);
        self.installation_completed = result.is_ok() && self.next_step == plan.steps().len();
        self.failed = true;
        result
    }
}

impl<F: FnMut(usize) -> Result<(), Error>> SmmuReadIo for Native<'_, F> {
    type Error = Error;
    fn read32(&mut self, offset: u32) -> Result<u32, Error> {
        let (page, local) = if offset < 0x1000 {
            (0, offset)
        } else if offset < 0x2000 {
            (1, offset - 0x1000)
        } else if (0x80000..0xbd000).contains(&offset) {
            Self::bank_address(((offset - 0x80000) / 0x1000) as usize, offset % 0x1000)?
        } else {
            return Err(Error::Register);
        };
        if local % 4 != 0 {
            return Err(Error::Register);
        }
        Ok(self.load32(page, local))
    }
}

/// Keep checks on the programmed address/ASID controls. Sunfish's NS view
/// forces TCR2.TBI0/TBI1 high, as in the working Linux capture. ARM IHI0062D.c
/// 16.5.39 says those bits affect address translation operations only. This
/// adapter supports no ATS and its layout requires ID0.ATOSNS=1. Do not mask
/// other fields or transfer these exceptions to a different register block.
fn readback_mask(block: RegisterBlock, offset: u32) -> u32 {
    match (block, offset) {
        (RegisterBlock::ContextBank, regs::CB_SCTLR) => !(1 << 12),
        (RegisterBlock::ContextBank, regs::CB_TTBCR2) => !((1 << 5) | (1 << 6)),
        _ => u32::MAX,
    }
}

impl<F: FnMut(usize) -> Result<(), Error>> InstallIo for Native<'_, F> {
    type Error = Error;
    fn read32(&mut self, block: RegisterBlock, index: usize, offset: u32) -> Result<u32, Error> {
        let page = self.resolve(block, index, offset, false, false)?;
        let value = self.load32(page, offset);
        if let Ok(InstallStep::WaitForClear {
            block: waiting_block,
            index: waiting_index,
            offset: waiting_offset,
            mask,
        }) = self.expected_step()
        {
            if (block, index, offset) == (waiting_block, waiting_index, waiting_offset)
                && value & mask == 0
            {
                self.next_step += 1;
            }
        }
        Ok(value)
    }
    fn write32(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        value: u32,
    ) -> Result<(), Error> {
        self.check()?;
        self.expect_write(block, index, offset, AccessWidth::U32, value as u64)?;
        let page = self.resolve(block, index, offset, false, true)?;
        // Recheck every retained bank immediately before a TLBI or enable.
        if page == 2
            && (offset == regs::CB_S1_TLBIASID || (offset == regs::CB_SCTLR && value & 1 != 0))
        {
            self.check_asid(CONTEXT_BANK as u16)?;
        }
        (self.progress)(self.next_step, offset);
        // SAFETY: resolve restricts this to the owned route/bank in one page.
        unsafe {
            self.registers[page]
                .add(offset as usize)
                .cast::<u32>()
                .write_volatile(value);
        }
        sync();
        if page == 2 && [regs::CB_S1_TLBIASID, regs::CB_TLBSYNC].contains(&offset) {
            self.next_step += 1;
            return Ok(());
        }
        let actual = self.load32(page, offset);
        // The working NS view reads SCTLR 0x67 after Linux writes 0x1067.
        // ASIDPNE is not used as ownership evidence; ASIDs are separately checked.
        // Its NS readback may be RAZ/WI on this implementation.
        let mask = readback_mask(block, offset);
        if (actual ^ value) & mask != 0 {
            self.failed = true;
            return Err(Error::Readback {
                offset,
                expected: value as u64,
                actual: actual as u64,
            });
        }
        self.next_step += 1;
        Ok(())
    }
    fn write64(
        &mut self,
        block: RegisterBlock,
        index: usize,
        offset: u32,
        value: u64,
    ) -> Result<(), Error> {
        self.check()?;
        self.expect_write(block, index, offset, AccessWidth::U64, value)?;
        let page = self.resolve(block, index, offset, true, true)?;
        (self.progress)(self.next_step, offset);
        // SAFETY: aligned TTBR within the exclusively mapped bank page.
        let pointer = unsafe { self.registers[page].add(offset as usize).cast::<u64>() };
        unsafe {
            pointer.write_volatile(value);
        }
        sync();
        let actual = unsafe { pointer.read_volatile() };
        if actual != value {
            self.failed = true;
            return Err(Error::Readback {
                offset,
                expected: value,
                actual,
            });
        }
        self.next_step += 1;
        Ok(())
    }
    fn publish_page_tables(
        &mut self,
        physical_pages: [u64; 4],
        page_bytes: u64,
    ) -> Result<(), Error> {
        self.check()?;
        if self.expected_step()?
            != (InstallStep::PublishPageTables {
                physical_pages,
                page_bytes,
            })
        {
            self.failed = true;
            return Err(Error::Tables);
        }
        if self.root_extra.is_some()
            || !self.tables_staged
            || physical_pages != self.table_physical
            || page_bytes != PAGE_BYTES as u64
        {
            return Err(Error::Tables);
        }
        sync();
        self.next_step += 1;
        Ok(())
    }
    fn publish_multi_page_tables(
        &mut self,
        root: u64,
        l2: u64,
        l3: &[u64],
        upper: u64,
        bytes: u64,
    ) -> Result<(), Error> {
        if l3.len() != 1 {
            return Err(Error::Tables);
        }
        self.publish_page_tables([root, l2, l3[0], upper], bytes)
    }
    fn bind_context_fault_handler(&mut self, bank: usize) -> Result<(), Error> {
        self.check()?;
        if self.expected_step()? != (InstallStep::BindContextFaultHandler { context_bank: bank }) {
            self.failed = true;
            return Err(Error::FaultBinding);
        }
        if bank != CONTEXT_BANK {
            return Err(Error::FaultBinding);
        }
        if let Err(error) = (self.bind_fault)(bank) {
            self.failed = true;
            return Err(error);
        }
        self.next_step += 1;
        Ok(())
    }
    fn publish_root_page_tables(
        &mut self,
        physical_pages: [u64; 7],
        page_bytes: u64,
    ) -> Result<(), Error> {
        self.check()?;
        let (_, extra) = self.root_extra.ok_or(Error::Tables)?;
        let expected = [
            self.table_physical[0],
            self.table_physical[1],
            extra[0],
            self.table_physical[2],
            extra[1],
            extra[2],
            self.table_physical[3],
        ];
        if self.expected_step()?
            != (InstallStep::PublishRootPageTables {
                physical_pages,
                page_bytes,
            })
            || !self.tables_staged
            || physical_pages != expected
            || page_bytes != PAGE_BYTES as u64
        {
            self.failed = true;
            return Err(Error::Tables);
        }
        sync();
        self.next_step += 1;
        Ok(())
    }
    fn publish_receive_root_page_tables(
        &mut self,
        physical_pages: [u64; 8],
        page_bytes: u64,
    ) -> Result<(), Error> {
        self.check()?;
        let request = InstallStep::PublishReceiveRootPageTables {
            physical_pages,
            page_bytes,
        };
        if self.expected_step()? != request
            || self.root_publication()? != request
            || !self.tables_staged
            || self.receive.as_ref().is_none_or(|r| r.memory.is_none())
        {
            self.failed = true;
            return Err(Error::Tables);
        }
        sync();
        self.next_step += 1;
        Ok(())
    }
}

fn sync() {
    #[cfg(target_arch = "aarch64")]
    // SAFETY: only memory ordering, no address or privilege-state changes.
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(mpss_glink_transport_tests)]
    mod glink_transport {
        use super::*;
        include!("../../tests/sunfish_mpss_linux_transport_native_cases.rs");
    }
    #[cfg(all(
        feature = "sunfish-smmu-install-probe",
        feature = "sunfish-native-startup-mpss-smp2p"
    ))]
    mod mpss_postinstall {
        use super::*;
        include!("../../tests/sunfish_mpss_postinstall_cases.rs");
    }
    use crate::sunfish_smmu_exec::InstallAuthority;
    use crate::sunfish_smmu_lpae::{
        self, LpaePageMapping, LpaeS1Config, RingMemoryAttribute, TableWalkPolicy,
    };
    use crate::sunfish_smmu_program::plan_linux_reference_install;
    use crate::sunfish_smmu_snapshot::{
        capture, capture_context_bank_preimage, plan_wlan_alias_route_from_snapshot,
    };

    #[repr(align(4096))]
    struct Page([u64; 512]);
    #[repr(align(4096))]
    struct RegisterSpan([u64; 0x3a000 / 8]);
    struct Fixture {
        registers: [Box<RegisterSpan>; 5],
        tables: [Page; 4],
        data: [u64; 1],
        fault: AtomicBool,
    }
    fn bind(bank: usize) -> Result<(), Error> {
        if bank == CONTEXT_BANK {
            Ok(())
        } else {
            Err(Error::FaultBinding)
        }
    }
    impl Fixture {
        fn new() -> Self {
            let mut fixture = Self {
                registers: core::array::from_fn(|_| Box::new(RegisterSpan([0; 0x3a000 / 8]))),
                tables: core::array::from_fn(|_| Page([0xa5; 512])),
                data: [0xa300_0000],
                fault: AtomicBool::new(false),
            };
            fixture.set(0, 0x20, 0x4c017e4f);
            fixture.set(0, 0x24, 0x6000003d);
            fixture.set(0, 0x28, 0x1222);
            fixture.set(0, 0, 0x00281c06);
            fixture.set(2, 4, 1);
            fixture.set(2, 0x58, 0x400);
            fixture
        }
        fn set(&mut self, page: usize, offset: usize, value: u32) {
            let pointer = self.registers[page].0.as_mut_ptr().cast::<u8>();
            // SAFETY: fixture pages are aligned, owned RAM with bounded offsets.
            unsafe {
                pointer.add(offset).cast::<u32>().write(value);
            }
        }
        fn native(&mut self) -> Native<'_, fn(usize) -> Result<(), Error>> {
            let pointers = self
                .registers
                .each_mut()
                .map(|page| page.0.as_mut_ptr().cast::<u8>());
            let tables = self.tables.each_mut().map(|page| page.0.as_mut_ptr());
            // SAFETY: tests substitute exclusive aligned RAM register/table spans. This
            // is an MMIO simulation, not evidence of a real Sunfish capability.
            unsafe {
                Native::new(
                    pointers,
                    REGISTER_PHYSICAL,
                    tables,
                    [0xa310_0000, 0xa310_1000, 0xa310_2000, 0xa310_3000],
                    &self.data,
                    &self.fault,
                    bind as fn(usize) -> Result<(), Error>,
                )
            }
            .unwrap()
        }
        fn native_with_pages<'a>(
            &'a mut self,
            data: &'a [u64],
        ) -> Native<'a, fn(usize) -> Result<(), Error>> {
            // SAFETY: same exclusive RAM-backed simulation as native(), with
            // independently supplied retained allocator frame identities.
            unsafe {
                Native::new(
                    self.registers.each_mut().map(|p| p.0.as_mut_ptr().cast()),
                    REGISTER_PHYSICAL,
                    self.tables.each_mut().map(|p| p.0.as_mut_ptr()),
                    [0xa3100000, 0xa3101000, 0xa3102000, 0xa3103000],
                    data,
                    &self.fault,
                    bind as fn(usize) -> Result<(), Error>,
                )
            }
            .unwrap()
        }
    }
    fn root_pages() -> sunfish_smmu_lpae::LpaeRootTablePages {
        sunfish_smmu_lpae::LpaeRootTablePages {
            ttbr0_root: 0xa3100000,
            level2: [0xa3101000, 0xa3104000],
            level3: [0xa3102000, 0xa3105000, 0xa3106000],
            ttbr1_root: 0xa3103000,
        }
    }
    fn root_config() -> LpaeS1Config {
        LpaeS1Config {
            input_address_bits: 39,
            output_address_bits: 40,
            asid: 4,
            table_walk_policy: TableWalkPolicy::NonCoherent,
            ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
            arm_ns_quirk: false,
        }
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    fn receive_pages() -> crate::sunfish_smmu_lpae::LpaeRootTablePages<4> {
        let p = root_pages();
        crate::sunfish_smmu_lpae::LpaeRootTablePages {
            ttbr0_root: p.ttbr0_root,
            level2: p.level2,
            level3: [p.level3[0], p.level3[1], 0xa3107000, p.level3[2]],
            ttbr1_root: p.ttbr1_root,
        }
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    fn receive_ram() -> Vec<Page> {
        (0..512).map(|_| Page([0xa5; 512])).collect()
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    fn receive_physical() -> [u64; 512] {
        core::array::from_fn(|n| 0xa4000000 + n as u64 * 4096)
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    fn attach_receive<'a>(
        fixture: &'a mut Fixture,
        shared: &'a mut super::shared_fixture::Fixture,
        data: &'a [u64],
        ram: &'a mut [Page],
    ) -> Native<'a, fn(usize) -> Result<(), Error>> {
        let mut n = fixture.native_with_pages(data);
        n.attach_shared_memory(shared.shared()).unwrap();
        // SAFETY: exclusive aligned contiguous host RAM; physical IDs are
        // simulation fixtures, not real Sunfish ownership or mappings.
        let memory = unsafe {
            crate::sunfish_ipa_linux_receive_native::NativeReceiveRam::new(
                ram.as_mut_ptr().cast(),
                &receive_physical(),
            )
        }
        .unwrap();
        n.attach_receive_memory(memory).unwrap();
        n
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    fn stage_receive(
        n: &mut Native<'_, fn(usize) -> Result<(), Error>>,
        extra: &mut [Page; 4],
    ) -> SmmuInstallPlan {
        let snap = capture(n).unwrap();
        let [first, peer] =
            crate::sunfish_smmu_snapshot::plan_ipa_linux_exact_routes_from_snapshot(
                true,
                true,
                &snap,
                &[4],
            )
            .unwrap();
        let context = capture_context_bank_preimage(n, &snap, 4).unwrap();
        let mapping = n.receive_mappings().unwrap();
        let tables = mapping.plan(receive_pages()).unwrap();
        unsafe {
            n.stage_receive_root_tables(
                &tables,
                extra.each_mut().map(|p| p.0.as_mut_ptr()),
                [0xa3104000, 0xa3105000, 0xa3106000, 0xa3107000],
            )
        }
        .unwrap();
        crate::sunfish_smmu_program::plan_linux_reference_receive_root_install(
            first.route(),
            &tables,
            context.actlr(),
        )
        .with_captured_route_preimage(first)
        .unwrap()
        .with_captured_context_preimage(context)
        .unwrap()
        .with_ipa_linux_peer_route(peer)
        .unwrap()
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    #[test]
    fn native_receive_install_and_real_ram_handoff_preserve_retained_banks_and_tail_holes() {
        use crate::sunfish_ipa_linux_buffers::Memory;
        let mut f = Fixture::new();
        let mut shared = super::shared_fixture::Fixture::new();
        f.set(3, 2 * 4096 + regs::CB_SCTLR as usize, 1);
        f.set(3, 2 * 4096 + regs::CB_TTBR0 as usize + 4, 2 << 16);
        f.set(3, 3 * 4096 + regs::CB_SCTLR as usize, 1);
        f.set(3, 3 * 4096 + regs::CB_TTBR0 as usize + 4, 3 << 16);
        f.set(0, 0x810, 0x800f0300);
        f.set(0, 0xc10, 2);
        f.set(0, 0x814, 0x84400800);
        f.set(0, 0xc14, 3);
        let before_low = f.registers[3].0.to_vec();
        let before_high = f.registers[4].0.to_vec();
        let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
        let mut ram = receive_ram();
        let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
        let plan = stage_receive(&mut n, &mut extra);
        assert_eq!(
            plan.steps()
                .iter()
                .filter(|s| matches!(s, InstallStep::PublishReceiveRootPageTables { .. }))
                .count(),
            1
        );
        n.select(&plan).unwrap();
        let receipt = n.install_experiment(&plan, 8).unwrap();
        assert_eq!(receipt.table_publications, 1);
        assert_eq!(receipt.confirmed_writes, 17);
        assert!(n.failed && n.installation_completed);
        n.retained_receive_domain().unwrap();
        for index in 0..512 {
            let value = unsafe { n.receive_table.unwrap().0.add(index).read_volatile() };
            if index < 502 {
                assert_eq!(value & PHYSICAL_PAGE_MASK, receive_physical()[index]);
            } else {
                assert_eq!(value, 0);
            }
        }
        let mut memory = n.take_receive_memory().unwrap();
        assert_eq!(memory.physical_base(), 0xa4000000);
        assert_eq!(memory.dma_page(501), Some(0xa41f5000));
        assert_eq!(memory.dma_page(502), None);
        memory.write32(0, 0x12345678).unwrap();
        assert_eq!(memory.read32(0), Ok(0x12345678));
        assert!(matches!(n.take_receive_memory(), Err(Error::State)));
        n.retained_receive_domain().unwrap();
        drop(memory);
        drop(n);
        assert_eq!(f.registers[3].0.to_vec(), before_low);
        assert_eq!(f.registers[4].0.to_vec(), before_high);
        assert_eq!(shared.imem[0].0, [0x77; 4096]);
        assert_eq!(shared.imem[1].0, [0x77; 4096]);
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    #[test]
    fn native_receive_canonical_map_and_attributes_cannot_be_substituted_before_stores() {
        for fault in 0..5 {
            let mut f = Fixture::new();
            let mut shared = super::shared_fixture::Fixture::new();
            let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
            let mut ram = receive_ram();
            let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
            let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
            let mapping = n.receive_mappings().unwrap();
            let mut maps = *mapping.mappings();
            let mut config = root_config();
            match fault {
                0 => maps[256].physical_page = 0xa5000000,
                1 => maps[535].physical_page = receive_physical()[502],
                2 => config.ring_memory_attribute = RingMemoryAttribute::NormalWriteBack,
                3 => config.arm_ns_quirk = true,
                _ => config.asid = 5,
            }
            let tables = crate::sunfish_smmu_lpae::plan_lpae_s1_receive_roots(
                &maps,
                receive_pages(),
                config,
            )
            .unwrap();
            assert!(
                unsafe {
                    n.stage_receive_root_tables(
                        &tables,
                        extra.each_mut().map(|p| p.0.as_mut_ptr()),
                        [0xa3104000, 0xa3105000, 0xa3106000, 0xa3107000],
                    )
                }
                .is_err()
            );
            assert!(n.failed && !n.tables_staged && n.root_extra.is_none());
            assert!(n.receive.as_ref().unwrap().memory.is_some());
            for p in n.table_pointers {
                assert!(
                    unsafe { core::slice::from_raw_parts(p, 512) }
                        .iter()
                        .all(|v| *v == 0xa5)
                );
            }
            assert!(extra.iter().all(|p| p.0.iter().all(|v| *v == 0xa5)));
        }
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    #[test]
    fn native_receive_extra_table_aliases_include_unmapped_tail_and_cpu_span() {
        for fault in 0..5 {
            let mut f = Fixture::new();
            let mut shared = super::shared_fixture::Fixture::new();
            let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
            let mut ram = receive_ram();
            let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
            let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
            let mapping = n.receive_mappings().unwrap();
            let mut pages = receive_pages();
            let mut physical = [0xa3104000, 0xa3105000, 0xa3106000, 0xa3107000];
            let mut pointers = extra.each_mut().map(|p| p.0.as_mut_ptr());
            match fault {
                0 | 1 => {
                    physical[3] = receive_physical()[if fault == 0 { 502 } else { 511 }];
                    pages.level3[2] = physical[3];
                }
                2 => pointers[3] = n.receive.as_ref().unwrap().span.0 as *mut u64,
                3 => pointers[3] = n.table_pointers[0],
                _ => pointers[3] = pointers[0],
            }
            // The generic encoder cannot know about unmapped CPU tail pages;
            // the actual RAM owner must exclude those frame aliases too.
            let tables = crate::sunfish_smmu_lpae::plan_lpae_s1_receive_roots(
                mapping.mappings(),
                pages,
                root_config(),
            )
            .unwrap();
            assert!(unsafe { n.stage_receive_root_tables(&tables, pointers, physical) }.is_err());
            assert!(n.failed && !n.tables_staged && n.root_extra.is_none());
            assert!(n.receive.as_ref().unwrap().memory.is_some());
            assert!(extra.iter().all(|p| p.0.iter().all(|v| *v == 0xa5)));
        }
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    #[test]
    fn native_receive_handoff_rechecks_every_table_word_route_context_and_fault_state() {
        for fault in 0..16 {
            let mut f = Fixture::new();
            let mut shared = super::shared_fixture::Fixture::new();
            let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
            let mut ram = receive_ram();
            let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
            let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
            let plan = stage_receive(&mut n, &mut extra);
            n.select(&plan).unwrap();
            n.install_experiment(&plan, 8).unwrap();
            n.retained_receive_domain().unwrap();
            // Simulated asynchronous corruption through host RAM, never MMIO.
            unsafe {
                match fault {
                    0 => n.registers[2]
                        .add(regs::CB_TTBR0 as usize)
                        .cast::<u64>()
                        .write_volatile(0),
                    1 => n.registers[2]
                        .add(regs::CB_SCTLR as usize)
                        .cast::<u32>()
                        .write_volatile(0),
                    2 => n.registers[0]
                        .add(0x800 + n.slot.unwrap() * 4)
                        .cast::<u32>()
                        .write_volatile(0),
                    3 => n.receive_table.unwrap().0.add(511).write_volatile(3),
                    4 => n.table_pointers[0].add(1).write_volatile(3),
                    5 => n.table_pointers[3].add(511).write_volatile(3),
                    6 => n.registers[0].add(0x48).cast::<u32>().write_volatile(2),
                    7 => n.registers[2].add(0x58).cast::<u32>().write_volatile(0x402),
                    8 => n.fault.store(true, Ordering::Release),
                    9 => {
                        n.registers[3].add(2 * 4096).cast::<u32>().write_volatile(1);
                        n.registers[3]
                            .add(2 * 4096 + regs::CB_TTBR0 as usize + 4)
                            .cast::<u32>()
                            .write_volatile(4 << 16);
                    }
                    10 => n.registers[0]
                        .add(0xc00 + 10 * 4)
                        .cast::<u32>()
                        .write_volatile(4),
                    11 => n.table_pointers[1].add(511).write_volatile(3),
                    12 => n.table_pointers[2].add(511).write_volatile(3),
                    _ => n.root_extra.unwrap().0[fault - 13]
                        .add(511)
                        .write_volatile(3),
                }
            }
            assert!(n.retained_receive_domain().is_err(), "fault {fault}");
            assert!(n.take_receive_memory().is_err());
            assert!(n.receive.as_ref().unwrap().memory.is_some());
            assert!(matches!(n.take_receive_memory(), Err(Error::State)));
        }
    }
    #[cfg(feature = "sunfish-smmu-normal-nc-table-pairs")]
    #[test]
    fn normal_nc_table_pairs_check_every_bit_and_reject_misalignment() {
        let mut page = Page(core::array::from_fn(|i| (i as u64).wrapping_mul(0x123456789abcdef) ^ u64::MAX));
        let expected = page.0;
        // Exclusive host RAM models a page; no concurrent writer or device I/O.
        assert_eq!(unsafe { verify_normal_nc_table_pairs(page.0.as_ptr(), &expected) }, Ok(()));
        for word in 0..512 {
            for bit in 0..64 {
                page.0[word] ^= 1u64 << bit;
                assert_eq!(unsafe { verify_normal_nc_table_pairs(page.0.as_ptr(), &expected) }, Err(Error::Tables), "word {word}, bit {bit}");
                page.0[word] ^= 1u64 << bit;
            }
        }
        assert_eq!(unsafe { verify_normal_nc_table_pairs(page.0.as_ptr().add(1), &expected) }, Err(Error::Tables));
        assert_eq!(unsafe { verify_normal_nc_table_pairs(page.0.as_ptr(), &expected) }, Ok(()));
    }

    #[cfg(feature = "sunfish-smmu-install-probe")]
    #[test]
    fn native_receive_revalidation_rejects_corruption_at_every_table_word() {
        let mut f = Fixture::new();
        let mut shared = super::shared_fixture::Fixture::new();
        let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
        let mut ram = receive_ram();
        let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
        let plan = stage_receive(&mut n, &mut extra);
        n.select(&plan).unwrap();
        n.install_experiment(&plan, 8).unwrap();
        n.retained_receive_domain().unwrap();
        let root = n.root_extra.unwrap().0;
        let pointers = [n.table_pointers[0], n.table_pointers[1], root[0],
            n.table_pointers[2], root[1], n.receive_table.unwrap().0,
            root[2], n.table_pointers[3]];
        for (page, pointer) in pointers.iter().enumerate() {
            for word in 0..LPAE_ENTRIES_PER_TABLE {
                // SAFETY: synchronous mutation of exclusive host fixture RAM.
                // Flip an occupied or empty descriptor, then restore it.
                unsafe {
                    let at = pointer.add(word);
                    let original = at.read_volatile();
                    at.write_volatile(original ^ 1);
                    assert_eq!(n.retained_receive_domain(), Err(Error::Tables),
                        "page {page}, word {word}");
                    at.write_volatile(original);
                }
            }
        }
        n.retained_receive_domain().unwrap();
    }

    #[cfg(all(feature = "sunfish-smmu-install-probe",
              feature = "sunfish-smmu-retained-table-expectation"))]
    #[test]
    fn native_receive_revalidation_requires_prepared_expectation() {
        let mut f = Fixture::new();
        let mut shared = super::shared_fixture::Fixture::new();
        let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
        let mut ram = receive_ram();
        let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
        let plan = stage_receive(&mut n, &mut extra);
        n.select(&plan).unwrap();
        n.install_experiment(&plan, 8).unwrap();
        n.retained_receive_domain().unwrap();
        n.receive_table_expectation = None;
        assert_eq!(n.retained_receive_domain(), Err(Error::Tables));
        assert!(n.take_receive_memory().is_err());
    }

    #[cfg(feature = "sunfish-smmu-install-probe")]
    #[test]
    fn native_receive_fault_during_publication_consumes_install_and_retains_ram() {
        use core::sync::atomic::AtomicUsize;
        static FAULT_POINTER: AtomicUsize = AtomicUsize::new(0);
        static TRIGGER: AtomicUsize = AtomicUsize::new(usize::MAX);
        fn fault_after_disable(step: usize, _: u32) {
            if step == TRIGGER.load(Ordering::Acquire) {
                let p = FAULT_POINTER.load(Ordering::Acquire);
                // SAFETY: the synchronous test retains this exact AtomicBool
                // until installation returns and the pointer is cleared.
                unsafe { (*(p as *const AtomicBool)).store(true, Ordering::Release) };
            }
        }
        let mut f = Fixture::new();
        let mut shared = super::shared_fixture::Fixture::new();
        let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
        let mut ram = receive_ram();
        let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
        let plan = stage_receive(&mut n, &mut extra);
        n.select(&plan).unwrap();
        let publication = plan
            .steps()
            .iter()
            .position(|s| matches!(s, InstallStep::PublishReceiveRootPageTables { .. }))
            .unwrap();
        assert!(matches!(
            plan.steps()[publication - 1],
            InstallStep::Write { .. }
        ));
        TRIGGER.store(publication - 1, Ordering::Release);
        FAULT_POINTER.store(n.fault as *const AtomicBool as usize, Ordering::Release);
        n.set_progress(fault_after_disable);
        let result = n.install_experiment(&plan, 8);
        FAULT_POINTER.store(0, Ordering::Release);
        assert!(matches!(
            result,
            Err(crate::sunfish_smmu_exec::InstallError::Io {
                operation: crate::sunfish_smmu_exec::Operation::PublishReceiveRootPageTables,
                ..
            })
        ));
        assert!(!n.installation_completed && n.failed && n.next_step == publication);
        assert!(n.receive.as_ref().unwrap().memory.is_some());
        n.fault.store(false, Ordering::Release);
        assert!(matches!(
            n.install_experiment(&plan, 8),
            Err(crate::sunfish_smmu_exec::InstallError::Quarantined)
        ));
        assert!(n.take_receive_memory().is_err());
    }
    #[cfg(feature = "sunfish-smmu-install-probe")]
    #[test]
    fn native_receive_metadata_without_ram_both_routes_or_install_cannot_handoff() {
        let mut f = Fixture::new();
        let mut shared = super::shared_fixture::Fixture::new();
        let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
        let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut n = f.native_with_pages(&data);
        n.attach_shared_memory(shared.shared()).unwrap();
        let supplied = crate::sunfish_ipa_linux_receive_domain::Mapping::new(
            &n.shared_mappings().unwrap(),
            &receive_physical(),
        )
        .unwrap();
        let supplied_plan = supplied.plan(receive_pages()).unwrap();
        assert_eq!(
            unsafe {
                n.stage_receive_root_tables(
                    &supplied_plan,
                    extra.each_mut().map(|p| p.0.as_mut_ptr()),
                    [0xa3104000, 0xa3105000, 0xa3106000, 0xa3107000],
                )
            },
            Err(Error::State)
        );
        assert!(n.receive.is_none() && n.root_extra.is_none());
        drop(n);

        let mut f = Fixture::new();
        let mut shared = super::shared_fixture::Fixture::new();
        let mut ram = receive_ram();
        let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
        let full = stage_receive(&mut n, &mut extra);
        let snap = capture(&mut n).unwrap();
        let [first, _] = crate::sunfish_smmu_snapshot::plan_ipa_linux_exact_routes_from_snapshot(
            true,
            true,
            &snap,
            &[4],
        )
        .unwrap();
        let context = capture_context_bank_preimage(&mut n, &snap, 4).unwrap();
        let mapping = n.receive_mappings().unwrap();
        let tables = mapping.plan(receive_pages()).unwrap();
        let missing_peer = crate::sunfish_smmu_program::plan_linux_reference_receive_root_install(
            first.route(),
            &tables,
            context.actlr(),
        )
        .with_captured_route_preimage(first)
        .unwrap()
        .with_captured_context_preimage(context)
        .unwrap();
        assert_eq!(n.select(&missing_peer), Err(Error::Mapping));
        assert!(!n.selected && n.next_step == 0);
        n.select(&full).unwrap();
        assert!(matches!(n.take_receive_memory(), Err(Error::State)));
        assert!(n.receive.as_ref().unwrap().memory.is_some());
        // The premature claim is consumed even if installation later succeeds.
        n.install_experiment(&full, 8).unwrap();
        assert!(matches!(n.take_receive_memory(), Err(Error::State)));
        assert!(n.receive.as_ref().unwrap().memory.is_some());
    }
    #[test]
    fn native_real_shared_install_retains_owner_and_preserves_foreign_banks() {
        let mut fixture = Fixture::new();
        let before_low = fixture.registers[3].0.to_vec();
        let before_high = fixture.registers[4].0.to_vec();
        let mut shared = super::shared_fixture::Fixture::new();
        let data: [u64; 32] = core::array::from_fn(|i| 0xa3000000 + i as u64 * 4096);
        let mut extra: [Page; 3] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut native = fixture.native_with_pages(&data);
        let snap = capture(&mut native).unwrap();
        let [first, peer] =
            crate::sunfish_smmu_snapshot::plan_ipa_linux_exact_routes_from_snapshot(
                true,
                true,
                &snap,
                &[4],
            )
            .unwrap();
        let context = capture_context_bank_preimage(&mut native, &snap, 4).unwrap();
        native.attach_shared_memory(shared.shared()).unwrap();
        let maps = native.shared_mappings().unwrap();
        let tables =
            sunfish_smmu_lpae::plan_lpae_s1_roots(&maps, root_pages(), root_config()).unwrap();
        unsafe {
            native.stage_shared_root_tables(
                &tables,
                extra.each_mut().map(|p| p.0.as_mut_ptr()),
                [0xa3104000, 0xa3105000, 0xa3106000],
            )
        }
        .unwrap();
        let plan = crate::sunfish_smmu_program::plan_linux_reference_root_install(
            first.route(),
            &tables,
            context.actlr(),
        )
        .with_captured_route_preimage(first)
        .unwrap()
        .with_captured_context_preimage(context)
        .unwrap()
        .with_ipa_linux_peer_route(peer)
        .unwrap();
        native.select(&plan).unwrap();
        let receipt = crate::sunfish_smmu_exec::Installer::default()
            .install(InstallAuthority::for_test(), &mut native, &plan, 8)
            .unwrap();
        assert_eq!(receipt.confirmed_writes, 17);
        assert_eq!(
            native.load32(0, 0x800 + first.slot() as u32 * 4),
            0x80000520
        );
        assert_eq!(native.load32(0, 0x800 + peer.slot() as u32 * 4), 0x80000522);
        assert_eq!(native.load32(2, 0), 0x1067);
        let report = native.shared_report().unwrap();
        assert_eq!((report.submitted, report.completed), (15, 15));
        assert!(report.release_sent && report.published);
        assert_eq!(native.shared_item_physical(), Some(0x8611d030));
        drop(native);
        assert_eq!(fixture.registers[3].0.to_vec(), before_low);
        assert_eq!(fixture.registers[4].0.to_vec(), before_high);
        let mask = ((1u64 << 40) - 1) & !4095;
        assert_eq!(
            fixture.tables[2].0[(0x146a8000u64 >> 12 & 511) as usize] & mask,
            0x146a8000
        );
        assert_eq!(
            extra[2].0[(0x8611d000u64 >> 12 & 511) as usize] & mask,
            0x8611d000
        );
        assert_eq!(shared.imem[0].0, [0x77; 4096]);
        assert_eq!(shared.imem[1].0, [0x77; 4096]);
    }
    #[test]
    fn shared_addresses_without_live_owner_or_with_substituted_page_are_rejected() {
        for mode in 0..3 {
            let mut fixture = Fixture::new();
            let mut shared = super::shared_fixture::Fixture::new();
            let data: [u64; 32] = core::array::from_fn(|i| 0xa3000000 + i as u64 * 4096);
            let mut extra: [Page; 3] = core::array::from_fn(|_| Page([0xa5; 512]));
            let mut native = fixture.native_with_pages(&data);
            let mut maps: [LpaePageMapping; 37] = core::array::from_fn(|i| {
                let (iova_page, physical_page) = if i < 2 {
                    (0x146a8000 + i as u64 * 4096, 0x146a8000 + i as u64 * 4096)
                } else if i < 34 {
                    (0x80000000 + (i - 2) as u64 * 4096, data[i - 2])
                } else {
                    (
                        0x8611d000 + (i - 34) as u64 * 4096,
                        0x8611d000 + (i - 34) as u64 * 4096,
                    )
                };
                LpaePageMapping {
                    iova_page,
                    physical_page,
                }
            });
            if mode > 0 {
                native.attach_shared_memory(shared.shared()).unwrap();
            }
            if mode == 1 {
                maps[0].physical_page = 0xa5000000;
            }
            let tables =
                sunfish_smmu_lpae::plan_lpae_s1_roots(&maps, root_pages(), root_config()).unwrap();
            let result = unsafe {
                if mode == 2 {
                    native.stage_root_tables(
                        &tables,
                        extra.each_mut().map(|p| p.0.as_mut_ptr()),
                        [0xa3104000, 0xa3105000, 0xa3106000],
                    )
                } else {
                    native.stage_shared_root_tables(
                        &tables,
                        extra.each_mut().map(|p| p.0.as_mut_ptr()),
                        [0xa3104000, 0xa3105000, 0xa3106000],
                    )
                }
            };
            assert_eq!(
                result,
                Err(if mode == 0 {
                    Error::State
                } else if mode == 1 {
                    Error::SharedMemory(crate::sunfish_ipa_shared_memory::Error::Geometry)
                } else {
                    Error::DataPage
                })
            );
            assert!(native.root_extra.is_none() && !native.tables_staged);
            assert_eq!(native.load32(2, 0), 0);
            assert_eq!(native.load32(0, 0x800), 0);
            assert!(extra.iter().all(|p| p.0 == [0xa5; 512]));
        }
    }
    #[test]
    fn failed_shared_allocation_quarantines_adapter_and_retains_its_report() {
        let mut fixture = Fixture::new();
        let mut shared = super::shared_fixture::Fixture::new();
        shared.word(0x1ff000 + 32 + 7 * 48 + 16, 64);
        let data: [u64; 32] = core::array::from_fn(|i| 0xa3000000 + i as u64 * 4096);
        let mut native = fixture.native_with_pages(&data);
        assert!(native.attach_shared_memory(shared.shared()).is_err());
        assert!(native.shared_memory.is_some() && native.failed);
        assert_eq!(native.shared_mappings(), Err(Error::State));
        let r = native.shared_report().unwrap();
        assert!(r.consumed && r.release_sent);
        assert_eq!((r.submitted, r.completed), (0, 0));
        assert_eq!(native.load32(2, 0), 0);
        assert_eq!(native.load32(0, 0x800), 0);
    }
    #[test]
    fn shared_views_cannot_alias_extra_table_virtual_or_reserved_physical_pages() {
        for physical_alias in [false, true] {
            let mut fixture = Fixture::new();
            let mut shared = super::shared_fixture::Fixture::new();
            let smem_pointer = shared.smem.as_mut_ptr().cast::<u64>();
            let data: [u64; 32] = core::array::from_fn(|i| 0xa3000000 + i as u64 * 4096);
            let mut extra: [Page; 3] = core::array::from_fn(|_| Page([0xa5; 512]));
            let mut native = fixture.native_with_pages(&data);
            native.attach_shared_memory(shared.shared()).unwrap();
            let maps = native.shared_mappings().unwrap();
            let mut physical = root_pages();
            if physical_alias {
                physical.level2[1] = 0x86000000;
            }
            let plan =
                sunfish_smmu_lpae::plan_lpae_s1_roots(&maps, physical, root_config()).unwrap();
            let mut pointers = extra.each_mut().map(|p| p.0.as_mut_ptr());
            if !physical_alias {
                pointers[0] = smem_pointer;
            }
            assert_eq!(
                unsafe {
                    native.stage_shared_root_tables(
                        &plan,
                        pointers,
                        [physical.level2[1], physical.level3[1], physical.level3[2]],
                    )
                },
                Err(Error::Mapping)
            );
            assert!(native.root_extra.is_none() && !native.tables_staged);
            assert!(extra.iter().all(|p| p.0 == [0xa5; 512]));
        }
    }
    #[test]
    fn private_root_staging_checks_width_of_unmapped_retained_private_pages() {
        let mut fixture = Fixture::new();
        fixture.set(0, 0x28, 0x1202);
        let data: [u64; 38] = core::array::from_fn(|i| {
            if i == 37 {
                1u64 << 36
            } else {
                0xa3000000 + i as u64 * 4096
            }
        });
        let maps: [LpaePageMapping; 37] = core::array::from_fn(|i| LpaePageMapping {
            iova_page: if i < 2 {
                0x146a8000 + i as u64 * 4096
            } else if i < 34 {
                0x80000000 + (i - 2) as u64 * 4096
            } else {
                0x8611d000 + (i - 34) as u64 * 4096
            },
            physical_page: data[i],
        });
        let plan =
            sunfish_smmu_lpae::plan_lpae_s1_roots(&maps, root_pages(), root_config()).unwrap();
        let mut extra: [Page; 3] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut native = fixture.native_with_pages(&data);
        assert_eq!(
            unsafe {
                native.stage_root_tables(
                    &plan,
                    extra.each_mut().map(|p| p.0.as_mut_ptr()),
                    [0xa3104000, 0xa3105000, 0xa3106000],
                )
            },
            Err(Error::Tables)
        );
        assert!(native.root_extra.is_none() && !native.tables_staged);
        assert!(extra.iter().all(|p| p.0 == [0xa5; 512]));
    }
    #[test]
    fn native_seven_frame_private_root_install_preserves_every_foreign_bank() {
        let mut fixture = Fixture::new();
        let before_low = fixture.registers[3].0.to_vec();
        let before_high = fixture.registers[4].0.to_vec();
        let data: [u64; 37] = core::array::from_fn(|i| 0xa3000000 + i as u64 * 4096);
        let maps: [LpaePageMapping; 37] = core::array::from_fn(|i| LpaePageMapping {
            iova_page: if i < 2 {
                0x146a8000 + i as u64 * 4096
            } else if i < 34 {
                0x80000000 + (i - 2) as u64 * 4096
            } else {
                0x8611d000 + (i - 34) as u64 * 4096
            },
            physical_page: data[i],
        });
        let tables =
            sunfish_smmu_lpae::plan_lpae_s1_roots(&maps, root_pages(), root_config()).unwrap();
        let mut extra: [Page; 3] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut native = fixture.native_with_pages(&data);
        let snap = capture(&mut native).unwrap();
        let [first, peer] =
            crate::sunfish_smmu_snapshot::plan_ipa_linux_exact_routes_from_snapshot(
                true,
                true,
                &snap,
                &[4],
            )
            .unwrap();
        let context = capture_context_bank_preimage(&mut native, &snap, 4).unwrap();
        unsafe {
            native.stage_root_tables(
                &tables,
                extra.each_mut().map(|p| p.0.as_mut_ptr()),
                [0xa3104000, 0xa3105000, 0xa3106000],
            )
        }
        .unwrap();
        let plan = crate::sunfish_smmu_program::plan_linux_reference_root_install(
            first.route(),
            &tables,
            context.actlr(),
        )
        .with_captured_route_preimage(first)
        .unwrap()
        .with_captured_context_preimage(context)
        .unwrap()
        .with_ipa_linux_peer_route(peer)
        .unwrap();
        native.select(&plan).unwrap();
        let receipt = crate::sunfish_smmu_exec::Installer::default()
            .install(InstallAuthority::for_test(), &mut native, &plan, 8)
            .unwrap();
        assert_eq!(receipt.confirmed_writes, 17);
        assert_eq!(receipt.table_publications, 1);
        // This RAM-backed register retains ASIDPNE; actual MMU-500 readback
        // may report that RAZ bit as zero (the measured hardware value is 67).
        assert_eq!(native.load32(2, 0), 0x1067);
        assert_eq!(
            native.load32(0, 0x800 + first.slot() as u32 * 4),
            0x80000520
        );
        assert_eq!(native.load32(0, 0x800 + peer.slot() as u32 * 4), 0x80000522);
        assert_eq!(
            unsafe {
                core::slice::from_raw_parts(native.registers[3].cast::<u64>(), before_low.len())
            },
            before_low.as_slice()
        );
        assert_eq!(
            unsafe {
                core::slice::from_raw_parts(native.registers[4].cast::<u64>(), before_high.len())
            },
            before_high.as_slice()
        );
        assert!(matches!(
            unsafe {
                native.stage_root_tables(
                    &tables,
                    extra.each_mut().map(|p| p.0.as_mut_ptr()),
                    [0xa3104000, 0xa3105000, 0xa3106000],
                )
            },
            Err(Error::State)
        ));
    }
    #[test]
    fn root_staging_rejects_unowned_data_and_virtual_alias_before_touching_frames() {
        let maps = [
            LpaePageMapping {
                iova_page: 0x146a8000,
                physical_page: 0xa3000000,
            },
            LpaePageMapping {
                iova_page: 0x80000000,
                physical_page: 0xa3001000,
            },
            LpaePageMapping {
                iova_page: 0x8611d000,
                physical_page: 0xa3002000,
            },
        ];
        let tables =
            sunfish_smmu_lpae::plan_lpae_s1_roots(&maps, root_pages(), root_config()).unwrap();
        let mut fixture = Fixture::new();
        let mut extra: [Page; 3] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut native = fixture.native();
        assert_eq!(
            unsafe {
                native.stage_root_tables(
                    &tables,
                    extra.each_mut().map(|p| p.0.as_mut_ptr()),
                    [0xa3104000, 0xa3105000, 0xa3106000],
                )
            },
            Err(Error::DataPage)
        );
        let mut pointers = extra.each_mut().map(|p| p.0.as_mut_ptr());
        pointers[0] = native.table_pointers[0];
        assert_eq!(
            unsafe {
                native.stage_root_tables(&tables, pointers, [0xa3104000, 0xa3105000, 0xa3106000])
            },
            Err(Error::Mapping)
        );
        assert!(!native.tables_staged && native.root_extra.is_none());
        for page in native.table_pointers {
            assert!(
                unsafe { core::slice::from_raw_parts(page, 512) }
                    .iter()
                    .all(|v| *v == 0xa5)
            );
        }
        assert!(extra.iter().all(|p| p.0.iter().all(|v| *v == 0xa5)));
        assert_eq!(native.next_step, 0);
    }
    #[test]
    fn root_plan_cannot_substitute_an_unstaged_extra_table_before_mmio() {
        let data = [0xa3000000, 0xa3001000, 0xa3002000];
        let maps = [
            LpaePageMapping {
                iova_page: 0x146a8000,
                physical_page: data[0],
            },
            LpaePageMapping {
                iova_page: 0x80000000,
                physical_page: data[1],
            },
            LpaePageMapping {
                iova_page: 0x8611d000,
                physical_page: data[2],
            },
        ];
        let mut fixture = Fixture::new();
        let mut extra: [Page; 3] = core::array::from_fn(|_| Page([0xa5; 512]));
        let mut native = fixture.native_with_pages(&data);
        let snap = capture(&mut native).unwrap();
        let route = plan_wlan_alias_route_from_snapshot(true, true, &snap, &[4]).unwrap();
        let context = capture_context_bank_preimage(&mut native, &snap, 4).unwrap();
        let original =
            sunfish_smmu_lpae::plan_lpae_s1_roots(&maps, root_pages(), root_config()).unwrap();
        unsafe {
            native.stage_root_tables(
                &original,
                extra.each_mut().map(|p| p.0.as_mut_ptr()),
                [0xa3104000, 0xa3105000, 0xa3106000],
            )
        }
        .unwrap();
        let mut forged = root_pages();
        forged.level2[1] = 0xa3200000;
        let other = sunfish_smmu_lpae::plan_lpae_s1_roots(&maps, forged, root_config()).unwrap();
        let plan = crate::sunfish_smmu_program::plan_linux_reference_root_install(
            route.route(),
            &other,
            context.actlr(),
        )
        .with_captured_route_preimage(route)
        .unwrap()
        .with_captured_context_preimage(context)
        .unwrap();
        assert_eq!(native.select(&plan), Err(Error::Tables));
        assert_eq!(native.next_step, 0);
        assert_eq!(native.load32(2, 0), 0);
    }
    fn tables(asid: u16, physical: u64) -> LpaeS1Plan {
        sunfish_smmu_lpae::plan_lpae_s1(
            &[LpaePageMapping {
                iova_page: 0x8000_0000,
                physical_page: physical,
            }],
            LpaeTablePages {
                ttbr0_root: 0xa310_0000,
                level2: 0xa310_1000,
                level3: 0xa310_2000,
                ttbr1_root: 0xa310_3000,
            },
            LpaeS1Config {
                input_address_bits: 39,
                output_address_bits: 40,
                asid,
                table_walk_policy: TableWalkPolicy::NonCoherent,
                ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
                arm_ns_quirk: false,
            },
        )
        .unwrap()
    }
    fn staged_plan<F: FnMut(usize) -> Result<(), Error>>(
        native: &mut Native<'_, F>,
    ) -> SmmuInstallPlan {
        let snapshot = capture(native).unwrap();
        let route =
            plan_wlan_alias_route_from_snapshot(true, true, &snapshot, &[CONTEXT_BANK]).unwrap();
        let context = capture_context_bank_preimage(native, &snapshot, CONTEXT_BANK).unwrap();
        let tables = tables(CONTEXT_BANK as u16, 0xa300_0000);
        native.stage_tables(&tables).unwrap();
        let plan = plan_linux_reference_install(route.route(), &tables, context.actlr())
            .with_captured_route_preimage(route)
            .unwrap()
            .with_captured_context_preimage(context)
            .unwrap();
        plan
    }

    fn prepare<F: FnMut(usize) -> Result<(), Error>>(
        native: &mut Native<'_, F>,
    ) -> SmmuInstallPlan {
        let plan = staged_plan(native);
        native.select(&plan).unwrap();
        plan
    }

    fn exact_ipa_plan<F: FnMut(usize) -> Result<(), Error>>(
        native: &mut Native<'_, F>,
    ) -> SmmuInstallPlan {
        let snap = capture(native).unwrap();
        let [first, peer] =
            crate::sunfish_smmu_snapshot::plan_ipa_linux_exact_routes_from_snapshot(
                true,
                true,
                &snap,
                &[4],
            )
            .unwrap();
        let context = capture_context_bank_preimage(native, &snap, 4).unwrap();
        let tables = tables(4, 0xa3000000);
        native.stage_tables(&tables).unwrap();
        plan_linux_reference_install(first.route(), &tables, context.actlr())
            .with_captured_route_preimage(first)
            .unwrap()
            .with_captured_context_preimage(context)
            .unwrap()
            .with_ipa_linux_peer_route(peer)
            .unwrap()
    }
    #[test]
    fn changed_peer_preimage_stops_exact_pair_before_any_mmio_write() {
        let mut fixture = Fixture::new();
        let mut native = fixture.native();
        let plan = exact_ipa_plan(&mut native);
        native.select(&plan).unwrap();
        let peer = plan.peer_route_preimage().unwrap();
        unsafe {
            native.registers[0]
                .add((0x800 + peer.slot() * 4) as usize)
                .cast::<u32>()
                .write(0x80000522);
        }
        let result = crate::sunfish_smmu_exec::Installer::default().install(
            InstallAuthority::for_test(),
            &mut native,
            &plan,
            8,
        );
        assert!(
            matches!(result,Err(crate::sunfish_smmu_exec::InstallError::RoutePreconditionMismatch{slot,..}) if slot==peer.slot())
        );
        assert_eq!(native.next_step, 0);
        assert_eq!(native.load32(2, 0), 0);
        assert_eq!(native.load32(0, 0xc00), 0);
    }
    #[test]
    fn exact_pair_rejects_masked_primary_duplicate_peer_and_different_bank() {
        let mut fixture = Fixture::new();
        let mut native = fixture.native();
        let snap = capture(&mut native).unwrap();
        let [first, peer] =
            crate::sunfish_smmu_snapshot::plan_ipa_linux_exact_routes_from_snapshot(
                true,
                true,
                &snap,
                &[4],
            )
            .unwrap();
        let masked = crate::sunfish_smmu_snapshot::plan_ipa_linux_dma_route_from_snapshot(
            true,
            true,
            &snap,
            &[4],
        )
        .unwrap();
        let tables = tables(4, 0xa3000000);
        let p = plan_linux_reference_install(masked.route(), &tables, 1)
            .with_captured_route_preimage(masked)
            .unwrap();
        assert!(p.with_ipa_linux_peer_route(peer).is_err());
        let [_, wrong_bank] =
            crate::sunfish_smmu_snapshot::plan_ipa_linux_exact_routes_from_snapshot(
                true,
                true,
                &snap,
                &[5],
            )
            .unwrap();
        let p = plan_linux_reference_install(first.route(), &tables, 1)
            .with_captured_route_preimage(first)
            .unwrap();
        assert!(p.with_ipa_linux_peer_route(wrong_bank).is_err());
        let p = p.with_ipa_linux_peer_route(peer).unwrap();
        assert!(p.with_ipa_linux_peer_route(peer).is_err());
        assert_eq!(p.steps().len(), 20);
        assert_eq!(p.route_preimage().unwrap().context_bank(), 4);
    }

    #[test]
    fn native_ipa_pair_installs_only_the_32_owned_pool_pages_and_preserves_neighbors() {
        let mut fixture = Fixture::new();
        fixture.set(0, 0x810, 0x800f0300);
        fixture.set(0, 0xc10, 2);
        fixture.set(0, 0x814, 0x84400800);
        fixture.set(0, 0xc14, 3);
        let data: [u64; 32] = core::array::from_fn(|i| 0xa3000000 + i as u64 * 4096);
        let pointers = fixture
            .registers
            .each_mut()
            .map(|p| p.0.as_mut_ptr().cast::<u8>());
        let table_pointers = fixture.tables.each_mut().map(|p| p.0.as_mut_ptr());
        // RAM-backed MMIO simulator only; no device or real authority.
        let mut native = unsafe {
            Native::new(
                pointers,
                REGISTER_PHYSICAL,
                table_pointers,
                [0xa3100000, 0xa3101000, 0xa3102000, 0xa3103000],
                &data,
                &fixture.fault,
                bind as fn(usize) -> Result<(), Error>,
            )
        }
        .unwrap();
        let snap = capture(&mut native).unwrap();
        let [route, peer] =
            crate::sunfish_smmu_snapshot::plan_ipa_linux_exact_routes_from_snapshot(
                true,
                true,
                &snap,
                &[4],
            )
            .unwrap();
        let context = capture_context_bank_preimage(&mut native, &snap, 4).unwrap();
        let maps: [LpaePageMapping; 32] = core::array::from_fn(|i| LpaePageMapping {
            iova_page: 0x80000000 + i as u64 * 4096,
            physical_page: data[i],
        });
        let pt = sunfish_smmu_lpae::plan_lpae_s1(
            &maps,
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
        native.stage_tables(&pt).unwrap();
        let plan = plan_linux_reference_install(route.route(), &pt, context.actlr())
            .with_captured_route_preimage(route)
            .unwrap()
            .with_captured_context_preimage(context)
            .unwrap()
            .with_ipa_linux_peer_route(peer)
            .unwrap();
        native.select(&plan).unwrap();
        let receipt = crate::sunfish_smmu_exec::Installer::default()
            .install(InstallAuthority::for_test(), &mut native, &plan, 8)
            .unwrap();
        assert_eq!(receipt.confirmed_writes, 17);
        assert_eq!(
            native.load32(0, 0x800 + route.slot() as u32 * 4),
            0x80000520
        );
        assert_eq!(native.load32(0, 0x800 + peer.slot() as u32 * 4), 0x80000522);
        assert_eq!(native.load32(0, 0xc00 + peer.slot() as u32 * 4), 4);
        assert_eq!(
            (native.load32(0, 0x810), native.load32(0, 0xc10)),
            (0x800f0300, 2)
        );
        assert_eq!(
            (native.load32(0, 0x814), native.load32(0, 0xc14)),
            (0x84400800, 3)
        );
        for i in 0..512 {
            let leaf = unsafe { native.table_pointers[2].add(i).read() };
            if i < 32 {
                assert_eq!(leaf & PHYSICAL_PAGE_MASK, data[i]);
                assert_ne!(leaf & 1, 0);
                assert_eq!(leaf & (3 << 53), 3 << 53);
            } else {
                assert_eq!(leaf, 0);
            }
        }
        assert_eq!(native.fault_status(), (0, 0x400));
    }

    #[test]
    fn native_smmu_readback_accepts_measured_tbi_but_rejects_address_and_asid_drift() {
        let mask = readback_mask(RegisterBlock::ContextBank, regs::CB_TTBCR2);
        assert_eq!((0x38012 ^ 0x38072) & mask, 0);
        for actual in [0x38073, 0x38062, 0x30072, 0x38472, 0x80038072] {
            assert_ne!((0x38012 ^ actual) & mask, 0);
        }
        assert_ne!(
            (0x38012 ^ 0x38072) & readback_mask(RegisterBlock::Global1, 0x10),
            0
        );
        let sctlr = readback_mask(RegisterBlock::ContextBank, regs::CB_SCTLR);
        assert_eq!((0x1067 ^ 0x67) & sctlr, 0);
        assert_ne!((0x1067 ^ 0x66) & sctlr, 0);
    }

    #[test]
    fn native_smmu_executes_bound_table_and_register_plan_with_format_only_fsr() {
        let mut fixture = Fixture::new();
        let mut native = fixture.native();
        let plan = prepare(&mut native);
        let receipt = crate::sunfish_smmu_exec::Installer::default()
            .install(InstallAuthority::for_test(), &mut native, &plan, 8)
            .unwrap();
        assert_eq!(receipt.confirmed_writes, 15);
        assert_eq!(receipt.fault_handler_bindings, 1);
        assert_eq!(native.load32(0, 0x800), 0x80010240);
        assert_eq!(native.load32(0, 0xc00), CONTEXT_BANK as u32);
        assert_eq!(native.load32(2, 0), 0x1067);
        assert_eq!(native.load32(2, 4), 1);
        assert_eq!(native.load32(2, regs::CB_S1_TLBIASID), CONTEXT_BANK as u32);
        assert_eq!(native.load32(2, regs::CB_TLBIALL), 0);
        assert_eq!(native.fault_status(), (0, 0x400));
    }

    #[test]
    fn native_smmu_preserves_the_actual_abl_ufs_and_display_routes_and_banks() {
        let mut fixture = Fixture::new();
        fixture.set(0, 0x800 + 4 * 4, 0x800f0300);
        fixture.set(0, 0xc00 + 4 * 4, 2);
        fixture.set(0, 0x800 + 5 * 4, 0x84400800);
        fixture.set(0, 0xc00 + 5 * 4, 3);
        fixture.set(1, 3 * 4, 0x1f000);
        fixture.set(3, 3 * PAGE_BYTES, 0xe0);
        fixture.set(1, 2 * 4, 0x1f000);
        fixture.set(3, 2 * PAGE_BYTES, 0xe0);
        fixture.set(3, 2 * PAGE_BYTES + 0x20, 0xdead0000);
        let before = fixture.registers[3].0[..CONTEXT_BANK * PAGE_BYTES / 8].to_vec();
        let mut native = fixture.native();
        let plan = prepare(&mut native);
        crate::sunfish_smmu_exec::Installer::default()
            .install(InstallAuthority::for_test(), &mut native, &plan, 8)
            .unwrap();
        assert_eq!(native.load32(0, 0x800 + 4 * 4), 0x800f0300);
        assert_eq!(native.load32(0, 0xc00 + 4 * 4), 2);
        assert_eq!(native.load32(1, 2 * 4), 0x1f000);
        assert_eq!(native.load32(0, 0x800 + 5 * 4), 0x84400800);
        assert_eq!(native.load32(0, 0xc00 + 5 * 4), 3);
        assert_eq!(native.load32(1, 3 * 4), 0x1f000);
        drop(native);
        assert_eq!(
            &fixture.registers[3].0[..CONTEXT_BANK * PAGE_BYTES / 8],
            before
        );
    }

    #[test]
    fn native_smmu_replaces_the_measured_disabled_state_and_applies_cpre_workaround() {
        let mut fixture = Fixture::new();
        fixture.set(0, 0x28, 0x5555);
        fixture.set(2, 0, 0x2a944312);
        fixture.set(2, 4, 3);
        fixture.set(2, 0x58, 0);
        fixture.set(2, 0x24, 0x5b919038);
        fixture.set(2, 0x2c, 0xe58ea767);
        let mut native = fixture.native();
        let plan = prepare(&mut native);
        crate::sunfish_smmu_exec::Installer::default()
            .install(InstallAuthority::for_test(), &mut native, &plan, 8)
            .unwrap();
        assert_eq!(native.load32(2, 4), 1);
        assert_eq!(native.load32(2, 0), 0x1067);
        assert_eq!(native.load32(2, 0x24) >> 16, CONTEXT_BANK as u32);
        assert_eq!(native.load32(2, 0x2c) >> 16, CONTEXT_BANK as u32);
    }

    #[test]
    fn native_smmu_refuses_an_enabled_unreferenced_candidate_before_mmio_writes() {
        let mut fixture = Fixture::new();
        fixture.set(2, 0, 0x67);
        let mut native = fixture.native();
        let plan = staged_plan(&mut native);
        assert_eq!(
            native.select(&plan),
            Err(Error::ContextActive { sctlr: 0x67 })
        );
        assert_eq!(native.load32(2, 0), 0x67);
        assert_eq!(native.load32(0, 0x800), 0);
    }

    #[test]
    fn native_smmu_checks_id2_translation_format_before_touching_tables() {
        for id2 in [0x222, 0x1221, 0x1122, 0x122f] {
            let mut fixture = Fixture::new();
            fixture.set(0, 0x28, id2);
            let mut native = fixture.native();
            assert!(
                native
                    .stage_tables(&tables(CONTEXT_BANK as u16, 0xa300_0000))
                    .is_err()
            );
            drop(native);
            assert!(fixture.tables.iter().all(|page| page.0 == [0xa5; 512]));
        }
    }

    #[test]
    fn native_smmu_refuses_unowned_data_before_touching_table_frames() {
        let mut fixture = Fixture::new();
        let mut native = fixture.native();
        assert_eq!(
            native.stage_tables(&tables(3, 0xa400_0000)),
            Err(Error::DataPage)
        );
        drop(native);
        assert!(fixture.tables.iter().all(|page| page.0 == [0xa5; 512]));
    }

    #[test]
    fn native_smmu_staged_context_cannot_be_replaced_by_a_different_asid_plan() {
        let mut fixture = Fixture::new();
        let mut native = fixture.native();
        let snapshot = capture(&mut native).unwrap();
        let route =
            plan_wlan_alias_route_from_snapshot(true, true, &snapshot, &[CONTEXT_BANK]).unwrap();
        let context = capture_context_bank_preimage(&mut native, &snapshot, CONTEXT_BANK).unwrap();
        native.stage_tables(&tables(3, 0xa300_0000)).unwrap();
        let different =
            plan_linux_reference_install(route.route(), &tables(4, 0xa300_0000), context.actlr())
                .with_captured_route_preimage(route)
                .unwrap()
                .with_captured_context_preimage(context)
                .unwrap();
        assert_eq!(native.select(&different), Err(Error::Tables));
        assert_eq!(native.load32(0, 0x800), 0);
        assert_eq!(native.load32(2, 0), 0);
    }

    #[test]
    fn native_smmu_refuses_an_active_retained_banks_matching_ttbr1_asid() {
        let mut fixture = Fixture::new();
        fixture.set(4, (60 - CONTEXT_BANK - 1) * PAGE_BYTES, 1);
        fixture.set(
            4,
            (60 - CONTEXT_BANK - 1) * PAGE_BYTES + 0x2c,
            (CONTEXT_BANK as u32) << 16,
        );
        let mut native = fixture.native();
        let snapshot = capture(&mut native).unwrap();
        let route =
            plan_wlan_alias_route_from_snapshot(true, true, &snapshot, &[CONTEXT_BANK]).unwrap();
        let context = capture_context_bank_preimage(&mut native, &snapshot, CONTEXT_BANK).unwrap();
        let tables = tables(CONTEXT_BANK as u16, 0xa300_0000);
        native.stage_tables(&tables).unwrap();
        let plan = plan_linux_reference_install(route.route(), &tables, context.actlr())
            .with_captured_route_preimage(route)
            .unwrap()
            .with_captured_context_preimage(context)
            .unwrap();
        assert_eq!(
            native.select(&plan),
            Err(Error::AsidConflict {
                bank: 60,
                asid: CONTEXT_BANK as u16
            })
        );
        assert_eq!(native.load32(2, 0), 0);
        assert_eq!(native.load32(0, 0x800), 0);
    }

    #[test]
    fn native_smmu_out_of_order_enable_is_terminal_without_a_register_write() {
        let mut fixture = Fixture::new();
        let mut native = fixture.native();
        prepare(&mut native);
        assert_eq!(
            native.write32(RegisterBlock::ContextBank, CONTEXT_BANK, 0, 0x1067),
            Err(Error::State)
        );
        assert_eq!(native.load32(2, 0), 0);
        assert_eq!(
            native.write32(RegisterBlock::ContextBank, CONTEXT_BANK, 0, 0),
            Err(Error::State)
        );
    }

    #[test]
    fn native_smmu_fault_binding_failure_keeps_translation_and_route_disabled() {
        let mut fixture = Fixture::new();
        let mut native = fixture.native();
        native.bind_fault = (|_| Err(Error::FaultBinding)) as fn(usize) -> Result<(), Error>;
        let plan = prepare(&mut native);
        assert!(
            crate::sunfish_smmu_exec::Installer::default()
                .install(InstallAuthority::for_test(), &mut native, &plan, 8)
                .is_err()
        );
        assert_eq!(native.load32(2, 0), 0);
        assert_eq!(native.load32(0, 0x800), 0);
    }
}
