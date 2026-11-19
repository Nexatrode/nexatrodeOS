//! One native SMMU context/route install experiment. No radio or PAS access.
//! Optional IPA profiles: two SIDs share private RAM; a seven-table root
//! experiment covers 37 pages in three IOVA regions. A separately selected
//! shared profile retains real IMEM/SMEM backing. Neither path issues DMA.
use crate::pixel_probe_display::Display;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use personal_os_rust_server::{
    sunfish_smmu_lpae::{self, LpaePageMapping},
    sunfish_smmu_native::{self as native, Error, Native},
    sunfish_smmu_snapshot::{self as snapshot, SmmuReadIo},
};
#[cfg(not(feature = "sunfish-native-startup-ipa-receive-memory"))]
use personal_os_rust_server::{
    sunfish_smmu_lpae::{LpaeS1Config, LpaeTablePages, RingMemoryAttribute, TableWalkPolicy},
    sunfish_smmu_program::plan_linux_reference_install,
};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
};

const IPA_PROFILE: bool = cfg!(feature = "sunfish-smmu-ipa-install-probe");
const ROOT_PROFILE: bool = cfg!(feature = "sunfish-smmu-ipa-root-install-probe");
const SHARED_PROFILE: bool = cfg!(feature = "sunfish-smmu-ipa-shared-install-probe");
const RECEIVE_PROFILE: bool = cfg!(feature = "sunfish-native-startup-ipa-receive-memory");
const DATA_PAGE_COUNT: usize = if SHARED_PROFILE {
    32
} else if ROOT_PROFILE {
    37
} else if IPA_PROFILE {
    32
} else {
    1
};
const MAPPING_PAGE_COUNT: usize = if ROOT_PROFILE { 37 } else { DATA_PAGE_COUNT };
const ROUTE_ID: u32 = if IPA_PROFILE { 0x520 } else { 0x240 };
const ROUTE_MASK: u32 = if IPA_PROFILE { 0 } else { 1 };
// Initialized once before Native borrows it. Frame identity/retention is
// established by actual CapDL, not by this CPU-owned array of addresses.
static mut DATA_PAGES: [u64; DATA_PAGE_COUNT] = [0; DATA_PAGE_COUNT];
static FAULT: AtomicBool = AtomicBool::new(false);
static FRAMEBUFFER: AtomicUsize = AtomicUsize::new(0);
macro_rules! physical_symbol {
    ($name:ident) => {
        #[used]
        #[unsafe(no_mangle)]
        static mut $name: u64 = 0;
    };
}
physical_symbol!(smmu_gr0_paddr);
physical_symbol!(smmu_gr1_paddr);
physical_symbol!(smmu_context_paddr);
physical_symbol!(smmu_cb_low_paddr);
physical_symbol!(smmu_cb_high_paddr);
physical_symbol!(smmu_root_paddr);
physical_symbol!(smmu_l2_paddr);
physical_symbol!(smmu_l3_paddr);
physical_symbol!(smmu_upper_paddr);
physical_symbol!(smmu_data_paddr);
#[cfg(feature = "sunfish-smmu-ipa-root-install-probe")]
physical_symbol!(smmu_l2_peer_paddr);
#[cfg(feature = "sunfish-smmu-ipa-root-install-probe")]
physical_symbol!(smmu_l3_peer_paddr);
#[cfg(feature = "sunfish-smmu-ipa-root-install-probe")]
physical_symbol!(smmu_l3_shared_paddr);
#[cfg(feature = "sunfish-smmu-ipa-shared-install-probe")]
physical_symbol!(ipa_smem_before_paddr);
#[cfg(feature = "sunfish-smmu-ipa-shared-install-probe")]
physical_symbol!(ipa_smem_metadata_paddr);
#[cfg(feature = "sunfish-smmu-ipa-shared-install-probe")]
physical_symbol!(ipa_smem_lock_paddr);
#[cfg(feature = "sunfish-smmu-ipa-shared-install-probe")]
physical_symbol!(ipa_imem_paddr);
#[cfg(feature = "sunfish-native-startup-mpss-smem")]
physical_symbol!(mpss_smem_payload_paddr);
#[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
physical_symbol!(smmu_l3_receive_paddr);
#[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
physical_symbol!(ipa_receive_data_paddr);

pub(crate) type Adapter = Native<'static, fn(usize) -> Result<(), Error>>;
#[cfg(feature = "sunfish-native-startup-mpss-glink")]
pub(crate) type RuntimeGate =
    personal_os_rust_server::sunfish_mpss_live_domain::NativeGate<Adapter>;
pub(crate) struct Probe {
    #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
    pool_claimed: bool,
    installed: bool,
    display: Display,
    adapter: Option<Adapter>,
}

/// Short-lived view of the same installed mapping owner before or after the
/// authenticated runtime takes it from `Probe`. The Gate variant never
/// exposes the adapter borrow beyond a closure and latches any conflict.
#[derive(Clone, Copy)]
pub(crate) enum Access<'a> {
    Probe(&'a Probe),
    #[cfg(feature = "sunfish-native-startup-mpss-glink")]
    Borrowed(&'a Adapter),
}

impl Access<'_> {
    pub(crate) fn retained_quiet(self) -> bool {
        match self {
            Self::Probe(probe) => probe.retained_quiet(),
            #[cfg(feature = "sunfish-native-startup-mpss-glink")]
            Self::Borrowed(adapter) => adapter.retained_receive_domain().is_ok(),
        }
    }

    pub(crate) fn fault_status(self) -> Option<(u32, u32)> {
        match self {
            Self::Probe(probe) => probe.fault_status(),
            #[cfg(feature = "sunfish-native-startup-mpss-glink")]
            Self::Borrowed(adapter) => Some(adapter.fault_status()),
        }
    }
}

fn checkpoint(step: usize, offset: u32) {
    let pointer = FRAMEBUFFER.load(Ordering::Relaxed) as *mut u32;
    // SAFETY: init publishes this PD's retained splash mapping before use.
    let display = unsafe { Display::new(pointer) };
    display.hex(18, "WRITE STEP", step as u32);
    display.hex(19, "REGISTER", offset);
    display.flush();
    debug_println!("SMMU-INSTALL|WRITE|step={}|offset={:x}", step, offset);
}

fn show_error(display: &Display, error: Error) {
    let label = match error {
        Error::Mapping => "MAPPING ERROR",
        Error::Layout => "LAYOUT ERROR",
        Error::State => "STATE GUARD",
        Error::Register => "REGISTER GUARD",
        Error::Tables => "TABLE GUARD",
        Error::DataPage => "DATA PAGE GUARD",
        Error::SharedMemory(error) => {
            debug_println!("SMMU-INSTALL|SHARED|{:?}", error);
            use personal_os_rust_server::{
                sunfish_ipa_shared_memory::Error as Shared, sunfish_ipa_smem::Error as Smem,
            };
            match error {
                Shared::Mapping => "SHARED MAP ID GUARD",
                Shared::Geometry => "SHARED GEOMETRY GUARD",
                #[cfg(feature = "sunfish-native-startup-mpss-glink")]
                Shared::Glink(_) => "GLINK SHARED MEMORY GUARD",
                Shared::NotAllocated => "SMEM NOT ALLOCATED",
                Shared::Allocation(error) => match error {
                    Smem::Consumed => "SMEM ONE SHOT GUARD",
                    Smem::LockBusy => "SMEM LOCK BUSY",
                    Smem::LockLost => "SMEM LOCK LOST",
                    Smem::Directory(error) => {
                        display.hex(22, "SMEM DIR ERROR", error.code());
                        "SMEM DIRECTORY GUARD"
                    }
                    Smem::Unstable => "SMEM SNAPSHOT CHANGED",
                    Smem::Target => "SMEM TARGET GUARD",
                    Smem::ItemLimit => "SMEM ITEM LIMIT",
                    Smem::TableDescriptor => "SMEM TOC GUARD",
                    Smem::ForeignRange => "SMEM FOREIGN RANGE",
                    Smem::ForeignOverlap => "SMEM FOREIGN OVERLAP",
                    Smem::Store { step, .. } => {
                        display.hex(23, "SMEM STORE STEP", step as u32);
                        "SMEM STORE STOP"
                    }
                    Smem::Readback { step } => {
                        display.hex(23, "SMEM STORE STEP", step as u32);
                        "SMEM READBACK STOP"
                    }
                    Smem::Lookup => "SMEM LOOKUP STOP",
                },
            }
        }
        Error::Fault(code) => {
            display.hex(22, "FAULT CODE", code);
            "FAULT PRESENT"
        }
        Error::FaultBinding => "IRQ BIND ERROR",
        Error::ContextReferenced { slot } => {
            display.hex(22, "REFERENCE SLOT", slot as u32);
            "CONTEXT REFERENCED"
        }
        Error::ContextActive { sctlr } => {
            display.hex(22, "LIVE SCTLR", sctlr);
            "CONTEXT ACTIVE"
        }
        Error::AsidConflict { bank, asid } => {
            display.hex(22, "CONFLICT BANK", bank as u32);
            display.hex(23, "CONFLICT ASID", asid as u32);
            "ASID CONFLICT"
        }
        Error::Readback {
            offset,
            expected,
            actual,
        } => {
            display.hex(21, "READBACK OFFSET", offset);
            display.hex(22, "EXPECTED LOW", expected as u32);
            display.hex(23, "ACTUAL LOW", actual as u32);
            display.hex(24, "ACTUAL HIGH", (actual >> 32) as u32);
            "READBACK ERROR"
        }
    };
    display.replace_line(17, label);
}

fn show_route_error(
    display: &Display,
    captured: &snapshot::RouteTableSnapshot,
    error: personal_os_rust_server::sunfish_smmu::RouteError,
) {
    use personal_os_rust_server::sunfish_smmu::RouteError;
    let label = match error {
        RouteError::NoVerifiedFreeSlot => "NO FREE ROUTE SLOT",
        RouteError::NoVerifiedFreeContextBank => "NO FREE CONTEXT BANK",
        RouteError::RouteOverlap => "TARGET ROUTE OVERLAP",
        RouteError::ObservedRouteOutOfRange => "OBSERVED ROUTE RANGE",
        RouteError::InvalidHardwareBounds => "HARDWARE BOUNDS",
        _ => "ROUTE PLANNER GUARD",
    };
    display.replace_line(17, label);
    display.hex(18, "ROUTE ERROR", error as u32);
    display.hex(19, "GLOBAL FSR", captured.global_fsr());
    let mut matches = 0;
    let mut first_match = None;
    let mut bank_refs = 0;
    let mut first_ref = None;
    for slot in 0..captured.slot_count() {
        let raw = captured.raw_entry(slot).unwrap();
        let id = raw.smr & 0x7fff;
        let mask = (raw.smr >> 16) & 0x7fff;
        if raw.is_valid()
            && (((id ^ ROUTE_ID) & !mask & !ROUTE_MASK & 0x7fff) == 0
                || (IPA_PROFILE && ((id ^ 0x522) & !mask & 0x7fff) == 0))
        {
            matches += 1;
            if first_match.is_none() {
                first_match = Some((slot, raw));
            }
        }
        if (raw.s2cr >> 16) & 3 == 0 && raw.s2cr & 0xff == native::CONTEXT_BANK as u32 {
            bank_refs += 1;
            if first_ref.is_none() {
                first_ref = Some((slot, raw));
            }
        }
    }
    display.hex(5, "FREE SLOTS", captured.inactive_slots().count() as u32);
    display.hex(6, "TARGET MATCHES", matches);
    display.hex(
        7,
        "FIRST MATCH",
        first_match.map_or(u32::MAX, |(slot, _)| slot as u32),
    );
    if let Some((_, raw)) = first_match {
        display.hex(8, "MATCH SMR", raw.smr);
        display.hex(9, "MATCH S2CR", raw.s2cr);
    }
    display.hex(10, "BANK REF COUNT", bank_refs);
    display.hex(
        11,
        "FIRST BANK REF",
        first_ref.map_or(u32::MAX, |(slot, _)| slot as u32),
    );
    if let Some((_, raw)) = first_ref {
        display.hex(12, "BANK REF SMR", raw.smr);
        display.hex(13, "BANK REF S2CR", raw.s2cr);
    }
    debug_println!("SMMU-INSTALL|ROUTE|{:?}", error);
}

fn bind_fault(bank: usize) -> Result<(), Error> {
    if bank != native::CONTEXT_BANK {
        return Err(Error::FaultBinding);
    }
    Channel::new(0).irq_ack().map_err(|_| Error::FaultBinding)?;
    Channel::new(1).irq_ack().map_err(|_| Error::FaultBinding)
}

fn stage(display: &Display, label: &str) {
    display.clear();
    display.replace_line(
        0,
        if SHARED_PROFILE {
            "IPA REAL SHARED MAPS"
        } else if ROOT_PROFILE {
            "IPA ROOT MAP TEST"
        } else if IPA_PROFILE {
            "NATIVE IPA DMA DOMAIN"
        } else {
            "NATIVE SMMU INSTALL"
        },
    );
    display.replace_line(1, label);
    display.replace_line(
        25,
        if SHARED_PROFILE {
            "REAL IMEM SMEM BACKING"
        } else if ROOT_PROFILE {
            "ALL MAPS PRIVATE RAM"
        } else if IPA_PROFILE {
            "32 PRIVATE RAM PAGES"
        } else {
            "ONE PRIVATE DATA PAGE"
        },
    );
    display.replace_line(28, "NO DMA OR ATS COMMAND");
    display.replace_line(26, "NO MODEM OR WLAN START");
    display.replace_line(27, "NO SMC OR FLASH");
    display.replace_line(29, "RESTART WHEN DONE");
    display.flush();
}

/// # Safety
/// The caller owns the retained splash mapping and the exact audited SMMU
/// apertures, table/data frames and fault IRQs for this one cold attempt.
pub(crate) unsafe fn run(framebuffer: *mut u32) -> Probe {
    FRAMEBUFFER.store(framebuffer as usize, Ordering::Relaxed);
    // SAFETY: fixed splash region retained by this PD's audited system.
    let display = unsafe { Display::new(framebuffer) };
    stage(&display, "MAPPING CHECK");
    let registers = [
        memory_region_symbol!(smmu_gr0: *mut u8).as_ptr(),
        memory_region_symbol!(smmu_gr1: *mut u8).as_ptr(),
        memory_region_symbol!(smmu_context: *mut u8).as_ptr(),
        memory_region_symbol!(smmu_cb_low: *mut u8).as_ptr(),
        memory_region_symbol!(smmu_cb_high: *mut u8).as_ptr(),
    ];
    let tables = [
        memory_region_symbol!(smmu_root: *mut u64).as_ptr(),
        memory_region_symbol!(smmu_l2: *mut u64).as_ptr(),
        memory_region_symbol!(smmu_l3: *mut u64).as_ptr(),
        memory_region_symbol!(smmu_upper: *mut u64).as_ptr(),
    ];
    #[cfg(feature = "sunfish-smmu-ipa-root-install-probe")]
    let extra_tables = [
        memory_region_symbol!(smmu_l2_peer: *mut u64).as_ptr(),
        memory_region_symbol!(smmu_l3_peer: *mut u64).as_ptr(),
        memory_region_symbol!(smmu_l3_shared: *mut u64).as_ptr(),
    ];
    #[cfg(not(feature = "sunfish-smmu-ipa-root-install-probe"))]
    let extra_tables = [core::ptr::null_mut::<u64>(); 3];
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    let receive_table = memory_region_symbol!(smmu_l3_receive:*mut u64).as_ptr();
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    let receive_data = memory_region_symbol!(ipa_receive_data:*mut u32).as_ptr();
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    if receive_table as usize != 0x7100f000 || receive_data as usize != 0x76001000 {
        stage(&display, "RX MAPPING STOP");
        return Probe {
            pool_claimed: false,
            installed: false,
            display,
            adapter: None,
        };
    }
    if registers.map(|p| p as usize) != [0x70000000, 0x70001000, 0x70002000, 0x73000000, 0x73010000]
        || tables.map(|p| p as usize) != [0x71001000, 0x71003000, 0x71005000, 0x71007000]
        || memory_region_symbol!(smmu_data: *mut u8).as_ptr() as usize != 0x72001000
        || (ROOT_PROFILE
            && extra_tables.map(|p| p as usize) != [0x71009000, 0x7100b000, 0x7100d000])
    {
        stage(&display, "MAPPING STOP");
        return Probe {
            #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
            pool_claimed: false,
            installed: false,
            display,
            adapter: None,
        };
    }
    // SAFETY: Microkit patches these retained scalar symbols before PD entry;
    // no code writes them afterward. The data frame is held for this boot.
    let (physical, table_physical, data) = unsafe {
        (
            [
                core::ptr::read_volatile(&raw const smmu_gr0_paddr),
                core::ptr::read_volatile(&raw const smmu_gr1_paddr),
                core::ptr::read_volatile(&raw const smmu_context_paddr),
                core::ptr::read_volatile(&raw const smmu_cb_low_paddr),
                core::ptr::read_volatile(&raw const smmu_cb_high_paddr),
            ],
            [
                core::ptr::read_volatile(&raw const smmu_root_paddr),
                core::ptr::read_volatile(&raw const smmu_l2_paddr),
                core::ptr::read_volatile(&raw const smmu_l3_paddr),
                core::ptr::read_volatile(&raw const smmu_upper_paddr),
            ],
            {
                let base = core::ptr::read_volatile(&raw const smmu_data_paddr);
                for i in 0..DATA_PAGE_COUNT {
                    (&raw mut DATA_PAGES)
                        .cast::<u64>()
                        .add(i)
                        .write(base.checked_add(i as u64 * 4096).unwrap_or(0));
                }
                core::slice::from_raw_parts((&raw const DATA_PAGES).cast::<u64>(), DATA_PAGE_COUNT)
            },
        )
    };
    #[cfg(feature = "sunfish-smmu-ipa-root-install-probe")]
    let extra_physical = unsafe {
        [
            core::ptr::read_volatile(&raw const smmu_l2_peer_paddr),
            core::ptr::read_volatile(&raw const smmu_l3_peer_paddr),
            core::ptr::read_volatile(&raw const smmu_l3_shared_paddr),
        ]
    };
    #[cfg(not(feature = "sunfish-smmu-ipa-root-install-probe"))]
    let extra_physical = [0; 3];
    if IPA_PROFILE {
        stage(&display, "ZERO PRIVATE DMA RAM");
        // SAFETY: the audited region is an exclusive contiguous private pool
        // uncached RW/NX allocation. Use aligned volatile stores rather than
        // SIMD memset on the Device mapping, and verify before any SMMU write.
        let ram = 0x72001000 as *mut u32;
        for i in 0..DATA_PAGE_COUNT * 1024 {
            unsafe { ram.add(i).write_volatile(0) };
        }
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        for i in 0..DATA_PAGE_COUNT * 1024 {
            if unsafe { ram.add(i).read_volatile() } != 0 {
                stage(&display, "PRIVATE RAM STOP");
                return Probe {
                    #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
                    pool_claimed: false,
                    installed: false,
                    display,
                    adapter: None,
                };
            }
        }
    }
    stage(&display, "READING IDS");
    // SAFETY: the build validates exclusive physical/virtual mappings, frames,
    // guard holes and fault IRQs against the actual CapDL model. This image
    // has one PD and no SMC capability. The standalone image has no radio
    // mappings; the integrated image retains only its bounded IPA/RSC maps.
    let mut adapter = match unsafe {
        Native::new(
            registers,
            physical,
            tables,
            table_physical,
            data,
            &FAULT,
            bind_fault as fn(usize) -> Result<(), Error>,
        )
    } {
        Ok(adapter) => adapter,
        Err(error) => {
            stage(&display, "LAYOUT STOP");
            show_error(&display, error);
            display.flush();
            debug_println!("SMMU-INSTALL|LAYOUT|{:?}", error);
            return Probe {
                #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
                pool_claimed: false,
                installed: false,
                display,
                adapter: None,
            };
        }
    };
    adapter.set_progress(checkpoint);
    stage(&display, "CAPTURING ROUTES");
    let result = (|| {
        display.replace_line(20, "CAPTURE ROUTES");
        let captured = snapshot::capture(&mut adapter).map_err(|_| Error::Layout)?;
        display.hex(2, "ID0", captured.id0());
        display.hex(3, "ID1", captured.id1());
        display.hex(4, "SMR COUNT", captured.slot_count() as u32);
        display.hex(10, "ID7", SmmuReadIo::read32(&mut adapter, 0x3c)?);
        display.hex(11, "SMMU ACR", SmmuReadIo::read32(&mut adapter, 0x10)?);
        display.hex(16, "CANDIDATE BANK", native::CONTEXT_BANK as u32);
        display.hex(21, "ID2", SmmuReadIo::read32(&mut adapter, 0x28)?);
        display.replace_line(20, "PLAN ROUTE");
        display.flush();
        let selected = if IPA_PROFILE {
            snapshot::plan_ipa_linux_exact_routes_from_snapshot(
                true,
                true,
                &captured,
                &[native::CONTEXT_BANK],
            )
            .map(|[first, peer]| (first, Some(peer)))
        } else {
            snapshot::plan_wlan_alias_route_from_snapshot(
                true,
                true,
                &captured,
                &[native::CONTEXT_BANK],
            )
            .map(|first| (first, None))
        };
        let (route, peer) = selected.map_err(|error| {
            show_route_error(&display, &captured, error);
            Error::State
        })?;
        display.replace_line(20, "CAPTURE CONTEXT");
        let context =
            snapshot::capture_context_bank_preimage(&mut adapter, &captured, native::CONTEXT_BANK)
                .map_err(|_| Error::Layout)?;
        display.hex(5, "SELECTED SLOT", route.slot() as u32);
        display.hex(6, "CONTEXT BANK", native::CONTEXT_BANK as u32);
        display.hex(7, "BEFORE SCTLR", context.sctlr());
        display.hex(8, "BEFORE ACTLR", context.actlr());
        display.hex(9, "BEFORE FSR", context.fsr());
        display.replace_line(1, "STAGING TABLES");
        display.replace_line(20, "STAGE TABLES");
        display.flush();
        #[cfg(feature = "sunfish-smmu-ipa-shared-install-probe")]
        let mappings: [LpaePageMapping; MAPPING_PAGE_COUNT] = {
            use personal_os_rust_server::{
                sunfish_ipa_shared_memory::SharedMemory, sunfish_ipa_smem_native::Window,
            };
            display.replace_line(1, "ALLOCATE SMEM ITEM 497");
            display.flush();
            // SAFETY: this opt-in profile's actual CapDL binds the complete
            // uncached RO SMEM view, single RW metadata page, real lock3 and
            // RO IMEM reservation. No payload is read, zeroed or written.
            let memory = unsafe {
                #[cfg(not(feature = "sunfish-native-startup-mpss-smem"))]
                let window = Window::new(
                    memory_region_symbol!(ipa_smem_before: *mut u8)
                        .as_ptr()
                        .cast_const(),
                    core::ptr::read_volatile(&raw const ipa_smem_before_paddr),
                    memory_region_symbol!(ipa_smem_metadata: *mut u32).as_ptr(),
                    core::ptr::read_volatile(&raw const ipa_smem_metadata_paddr),
                    memory_region_symbol!(ipa_smem_lock: *mut u32).as_ptr(),
                    core::ptr::read_volatile(&raw const ipa_smem_lock_paddr),
                )
                .map_err(|_| Error::Mapping)?;
                #[cfg(feature = "sunfish-native-startup-mpss-smem")]
                let window = Window::new_with_modem_pages(
                    memory_region_symbol!(ipa_smem_before: *mut u8)
                        .as_ptr()
                        .cast_const(),
                    core::ptr::read_volatile(&raw const ipa_smem_before_paddr),
                    memory_region_symbol!(ipa_smem_metadata: *mut u32).as_ptr(),
                    core::ptr::read_volatile(&raw const ipa_smem_metadata_paddr),
                    memory_region_symbol!(ipa_smem_lock: *mut u32).as_ptr(),
                    core::ptr::read_volatile(&raw const ipa_smem_lock_paddr),
                    memory_region_symbol!(mpss_smem_payload: *mut u32).as_ptr(),
                    core::ptr::read_volatile(&raw const mpss_smem_payload_paddr),
                )
                .map_err(|_| Error::Mapping)?;
                SharedMemory::new(
                    window,
                    memory_region_symbol!(ipa_imem: *mut u8)
                        .as_ptr()
                        .cast_const(),
                    core::ptr::read_volatile(&raw const ipa_imem_paddr),
                    8192,
                )
                .map_err(Error::SharedMemory)?
            };
            adapter.attach_shared_memory(memory)?;
            adapter.shared_mappings()?
        };
        #[cfg(not(feature = "sunfish-smmu-ipa-shared-install-probe"))]
        let mappings: [LpaePageMapping; MAPPING_PAGE_COUNT] =
            core::array::from_fn(|i| LpaePageMapping {
                // This opt-in test exercises future shared-memory IOVA
                // geometry using ONLY distinct allocator-owned private RAM.
                // It maps neither actual IMEM nor an SMEM allocation.
                iova_page: if ROOT_PROFILE && i < 2 {
                    0x146a8000 + i as u64 * 4096
                } else if ROOT_PROFILE && i >= 34 {
                    0x8611d000 + (i - 34) as u64 * 4096
                } else {
                    0x8000_0000 + (i - if ROOT_PROFILE { 2 } else { 0 }) as u64 * 4096
                },
                physical_page: data[i],
            });
        #[cfg(not(feature = "sunfish-native-startup-ipa-receive-memory"))]
        let config = LpaeS1Config {
            input_address_bits: 39,
            output_address_bits: 40,
            asid: native::CONTEXT_BANK as u16,
            table_walk_policy: TableWalkPolicy::NonCoherent,
            ring_memory_attribute: RingMemoryAttribute::NormalNonCacheable,
            arm_ns_quirk: false,
        };
        #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
        let initial_plan = {
            use personal_os_rust_server::sunfish_ipa_linux_receive_native::NativeReceiveRam;
            let (base, leaf) = unsafe {
                (
                    core::ptr::read_volatile(&raw const ipa_receive_data_paddr),
                    core::ptr::read_volatile(&raw const smmu_l3_receive_paddr),
                )
            };
            let physical: [u64; 512] =
                core::array::from_fn(|i| base.checked_add(i as u64 * 4096).unwrap_or(0));
            // SAFETY: the receive profile's actual CapDL must establish the
            // sole full 2-MiB uncached RW/NX allocation and all 512 frames.
            // No receive channel/doorbell is issued by this memory stage.
            let ram = unsafe { NativeReceiveRam::new(receive_data, &physical) }
                .map_err(|_| Error::Mapping)?;
            adapter.attach_receive_memory(ram)?;
            let full = adapter.receive_mappings()?;
            if mappings.iter().any(|m| !full.mappings().contains(m)) {
                return Err(Error::DataPage);
            }
            let tables = full
                .plan(sunfish_smmu_lpae::LpaeRootTablePages {
                    ttbr0_root: table_physical[0],
                    level2: [table_physical[1], extra_physical[0]],
                    level3: [
                        table_physical[2],
                        extra_physical[1],
                        leaf,
                        extra_physical[2],
                    ],
                    ttbr1_root: table_physical[3],
                })
                .map_err(|_| Error::Tables)?;
            // SAFETY: all eight table frames are actual exclusive allocations,
            // separately audited from the retained command and receive RAM.
            unsafe {
                adapter.stage_receive_root_tables(
                    &tables,
                    [
                        extra_tables[0],
                        extra_tables[1],
                        extra_tables[2],
                        receive_table,
                    ],
                    [
                        extra_physical[0],
                        extra_physical[1],
                        extra_physical[2],
                        leaf,
                    ],
                )
            }?;
            personal_os_rust_server::sunfish_smmu_program::plan_linux_reference_receive_root_install(
                route.route(),
                &tables,
                context.actlr(),
            )
        };
        #[cfg(not(feature = "sunfish-native-startup-ipa-receive-memory"))]
        let initial_plan = if ROOT_PROFILE {
            let tables = sunfish_smmu_lpae::plan_lpae_s1_roots(
                &mappings,
                sunfish_smmu_lpae::LpaeRootTablePages {
                    ttbr0_root: table_physical[0],
                    level2: [table_physical[1], extra_physical[0]],
                    level3: [table_physical[2], extra_physical[1], extra_physical[2]],
                    ttbr1_root: table_physical[3],
                },
                config,
            )
            .map_err(|_| Error::Tables)?;
            // SAFETY: this opt-in image's actual CapDL proves all seven
            // exclusive uncached table frames, guards and allocator identities.
            unsafe {
                if SHARED_PROFILE {
                    adapter.stage_shared_root_tables(&tables, extra_tables, extra_physical)?;
                } else {
                    adapter.stage_root_tables(&tables, extra_tables, extra_physical)?;
                }
            }
            personal_os_rust_server::sunfish_smmu_program::plan_linux_reference_root_install(
                route.route(),
                &tables,
                context.actlr(),
            )
        } else {
            let tables = sunfish_smmu_lpae::plan_lpae_s1(
                &mappings,
                LpaeTablePages {
                    ttbr0_root: table_physical[0],
                    level2: table_physical[1],
                    level3: table_physical[2],
                    ttbr1_root: table_physical[3],
                },
                config,
            )
            .map_err(|_| Error::Tables)?;
            adapter.stage_tables(&tables)?;
            plan_linux_reference_install(route.route(), &tables, context.actlr())
        };
        let mut plan = initial_plan
            .with_captured_route_preimage(route)
            .map_err(|_| Error::State)?
            .with_captured_context_preimage(context)
            .map_err(|_| Error::State)?;
        if let Some(peer) = peer {
            plan = plan
                .with_ipa_linux_peer_route(peer)
                .map_err(|_| Error::State)?;
        }
        display.replace_line(20, "SELECT PLAN");
        adapter.select(&plan)?;
        display.replace_line(1, "INSTALLING CONTEXT");
        display.replace_line(20, "EXECUTE PLAN");
        display.flush();
        match adapter.install_experiment(&plan, 1024) {
            Ok(receipt) => {
                display.hex(10, "CONFIRMED WRITES", receipt.confirmed_writes as u32);
                display.hex(11, "TLB WAIT READS", receipt.wait_reads as u32);
            }
            Err(error) => {
                use personal_os_rust_server::sunfish_smmu_exec::InstallError;
                match &error {
                    InstallError::Io {
                        step_index, source, ..
                    } => {
                        display.hex(18, "FAILED STEP", *step_index as u32);
                        show_error(&display, *source);
                    }
                    InstallError::WaitTimedOut {
                        step_index,
                        last_value,
                        ..
                    } => {
                        display.hex(18, "TIMEOUT STEP", *step_index as u32);
                        display.hex(22, "WAIT VALUE", *last_value);
                    }
                    InstallError::ContextTranslationAlreadyEnabled {
                        context_bank,
                        sctlr,
                    } => {
                        display.replace_line(17, "CONTEXT ACTIVE");
                        display.hex(22, "ACTIVE BANK", *context_bank as u32);
                        display.hex(23, "LIVE SCTLR", *sctlr);
                    }
                    _ => display.replace_line(17, "EXECUTOR GUARD"),
                }
                debug_println!("SMMU-INSTALL|EXEC|{:?}", error);
                return Err(Error::State);
            }
        }
        display.replace_line(20, "FINAL READBACK");
        let smr = SmmuReadIo::read32(&mut adapter, 0x800 + route.slot() as u32 * 4)?;
        let s2cr = SmmuReadIo::read32(&mut adapter, 0xc00 + route.slot() as u32 * 4)?;
        let sctlr = SmmuReadIo::read32(&mut adapter, native::CONTEXT_OFFSET)?;
        let (global, fsr) = adapter.fault_status();
        let actlr = SmmuReadIo::read32(&mut adapter, native::CONTEXT_OFFSET + 4)?;
        display.hex(17, "AFTER ACTLR", actlr);
        display.hex(12, "AFTER SMR", smr);
        display.hex(13, "AFTER S2CR", s2cr);
        display.hex(14, "AFTER SCTLR", sctlr);
        display.hex(15, "GLOBAL FSR", global);
        display.hex(16, "CONTEXT FSR", fsr);
        if smr != (0x80000000 | ROUTE_MASK << 16 | ROUTE_ID)
            || s2cr != native::CONTEXT_BANK as u32
            || sctlr & 1 == 0
            || actlr
                != personal_os_rust_server::sunfish_smmu_program::linux_reference_actlr(
                    context.actlr(),
                )
            || global != 0
            || fsr & !(3 << 9) != 0
        {
            return Err(Error::State);
        }
        if let Some(peer) = peer {
            let peer_smr = SmmuReadIo::read32(&mut adapter, 0x800 + peer.slot() as u32 * 4)?;
            let peer_s2cr = SmmuReadIo::read32(&mut adapter, 0xc00 + peer.slot() as u32 * 4)?;
            for row in 7..=9 {
                display.replace_line(row, "");
            }
            display.hex(7, "PEER SLOT", peer.slot() as u32);
            display.hex(8, "PEER SMR", peer_smr);
            display.hex(9, "PEER S2CR", peer_s2cr);
            if peer_smr != 0x80000522 || peer_s2cr != native::CONTEXT_BANK as u32 {
                return Err(Error::State);
            }
        }
        if IPA_PROFILE {
            display.hex(22, "PRIVATE PAGES", DATA_PAGE_COUNT as u32);
            display.hex(23, "PRIVATE BYTES", (DATA_PAGE_COUNT * 4096) as u32);
            display.hex(24, "POOL PA LOW", data[0] as u32);
        }
        if ROOT_PROFILE {
            display.hex(2, "ROOT MASK", 5);
            display.hex(3, "TABLE FRAMES", if RECEIVE_PROFILE { 8 } else { 7 });
            display.hex(4, "LEAF TABLES", if RECEIVE_PROFILE { 4 } else { 3 });
            display.replace_line(
                20,
                if SHARED_PROFILE {
                    "REAL IMEM SMEM MAPPED"
                } else {
                    "IMEM SMEM NOT MAPPED"
                },
            );
        }
        Ok(())
    })();
    let installed = result.is_ok();
    match result {
        Ok(()) => display.replace_line(
            1,
            if SHARED_PROFILE {
                "SHARED MAP INSTALL PASS"
            } else if ROOT_PROFILE {
                "ROOT MAP INSTALL PASS"
            } else if IPA_PROFILE {
                "IPA DOMAIN INSTALL PASS"
            } else {
                "INSTALL READBACK PASS"
            },
        ),
        Err(error) => {
            display.replace_line(1, "INSTALL STOPPED");
            if let Some(report) = adapter.shared_report() {
                display.hex(2, "SMEM SENT", report.submitted as u32);
                display.hex(3, "SMEM DONE", report.completed as u32);
                display.hex(4, "SMEM PUBLISHED", report.published as u32);
                display.hex(10, "SMEM PRE SCANS", report.pre_scans as u32);
                display.hex(11, "SMEM POST SCANS", report.post_scans as u32);
                display.hex(12, "SMEM RELEASE", report.release_sent as u32);
                display.hex(13, "SMEM ITEM LIMIT", report.item_limit as u32);
            }
            let (global, context) = adapter.fault_status();
            display.hex(15, "GLOBAL FSR", global);
            display.hex(16, "CONTEXT FSR", context);
            if error != Error::State {
                show_error(&display, error);
            }
            debug_println!("SMMU-INSTALL|STOP|{:?}", error);
        }
    }
    display.flush();
    Probe {
        #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
        pool_claimed: false,
        installed,
        display,
        adapter: Some(adapter),
    }
}

impl Probe {
    /// Transfer the sole installed SMMU/SMEM owner into the post-auth gate.
    /// The `Option::take` is the one-shot boundary; no adapter or writer is
    /// cloned and no reference to this Probe is retained by the new owner.
    #[cfg(feature = "sunfish-native-startup-mpss-glink")]
    pub(crate) fn take_runtime_gate(&mut self) -> Result<RuntimeGate, Error> {
        if !self.retained_quiet() {
            return Err(Error::State);
        }
        self.adapter
            .take()
            .map(personal_os_rust_server::sunfish_mpss_live_domain::NativeGate::new)
            .ok_or(Error::State)
    }

    #[cfg(feature = "sunfish-native-startup-mpss-auth")]
    pub(crate) fn modem_smp2p_poll(
        &mut self,
    ) -> Result<personal_os_rust_server::sunfish_mpss_linux_smp2p::Poll, Error> {
        if !self.retained_quiet() {
            return Err(Error::State);
        }
        self.adapter
            .as_mut()
            .ok_or(Error::State)?
            .modem_smp2p_poll()
    }
    #[cfg(feature = "sunfish-native-startup-mpss-auth")]
    pub(crate) fn take_modem_ready(
        &mut self,
    ) -> Result<personal_os_rust_server::sunfish_mpss_linux_smp2p::ReadyObservation, Error> {
        if !self.retained_quiet() {
            return Err(Error::State);
        }
        self.adapter
            .as_mut()
            .ok_or(Error::State)?
            .take_mpss_ready_observation()
    }
    #[cfg(feature = "sunfish-native-startup-mpss-preauth")]
    pub(crate) fn activate_modem_smp2p(
        &mut self,
        prepared: personal_os_rust_server::sunfish_mpss_linux_smem::Prepared,
        signal: personal_os_rust_server::sunfish_mpss_linux_smp2p_native::Platform<'static>,
    ) -> Result<(), Error> {
        let retained = core::mem::ManuallyDrop::new(signal);
        if !self.retained_quiet() {
            return Err(Error::State);
        }
        self.adapter
            .as_mut()
            .ok_or(Error::State)?
            .activate_modem_smp2p(prepared, core::mem::ManuallyDrop::into_inner(retained))
    }
    #[cfg(feature = "sunfish-native-startup-mpss-preauth")]
    pub(crate) fn modem_smp2p_report(
        &self,
    ) -> Option<personal_os_rust_server::sunfish_mpss_linux_smp2p::Report> {
        self.adapter.as_ref().and_then(|a| a.modem_smp2p_report())
    }
    #[cfg(feature = "sunfish-native-startup-ipa-shared")]
    pub(crate) fn show_shared_summary(&self) {
        let Some(adapter) = self.adapter.as_ref() else {
            return;
        };
        let Some(report) = adapter.shared_report() else {
            return;
        };
        let Some(item) = adapter.shared_item_physical() else {
            return;
        };
        let d = &self.display;
        d.clear();
        d.replace_line(0, "NATIVE IPA SHARED MAPS");
        d.replace_line(1, "IPA SHARED SETUP PASS");
        for (row, label, value) in [
            (2, "ROOT MASK", 5),
            (3, "TABLE FRAMES", if RECEIVE_PROFILE { 8 } else { 7 }),
            (4, "LEAF TABLES", if RECEIVE_PROFILE { 4 } else { 3 }),
            (5, "IMEM PA", 0x146a8000),
            (6, "IMEM BYTES", 8192),
            (7, "SMEM ITEM PA", item as u32),
            (8, "SMEM BYTES", 8192),
            (9, "SMEM MAP PA", item as u32 & !4095),
            (10, "SMEM MAP PAGES", 3),
            (11, "SMEM SENT", report.submitted as u32),
            (12, "SMEM DONE", report.completed as u32),
            (13, "SMEM RELEASE", report.release_sent as u32),
            (14, "PRIVATE PAGES", 32),
            (15, "SID 520 BANK", 4),
            (16, "SID 522 BANK", 4),
        ] {
            d.hex(row, label, value);
        }
        let (global, context) = adapter.fault_status();
        d.hex(17, "GLOBAL FSR", global);
        d.hex(18, "CONTEXT FSR", context);
        d.replace_line(19, "POWER CONFIG CANARIES PASS");
        d.replace_line(20, "BUFFERS LIMITS IRQ PASS");
        d.replace_line(21, "LIVE SMEM OWNER RETAINED");
        d.replace_line(22, "REAL IMEM SMEM MAPPED");
        d.replace_line(23, "NO SHARED PAYLOAD ACCESS");
        d.replace_line(26, "NO MODEM OR WLAN START");
        d.replace_line(27, "NO SMC OR FLASH");
        d.replace_line(28, "NO DMA OR ATS COMMAND");
        d.replace_line(29, "RESTART WHEN DONE");
        d.flush();
    }
    #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
    pub(crate) fn take_private_pool(
        &mut self,
    ) -> Result<
        personal_os_rust_server::sunfish_ipa_linux_buffers_native::NativeRam<'static>,
        personal_os_rust_server::sunfish_ipa_linux_buffers::Error,
    > {
        use personal_os_rust_server::{
            sunfish_ipa_linux_buffers as buffers, sunfish_ipa_linux_buffers_native::NativeRam,
        };
        if self.pool_claimed {
            return Err(buffers::Error::AlreadyStarted);
        }
        self.pool_claimed = true;
        if !IPA_PROFILE || !self.retained_quiet() {
            return Err(buffers::Error::Domain);
        }
        // SAFETY: the actual CapDL audit proves the sole uncached RW/NX
        // 128-KiB mapping and its ordered physical frames. This Probe retains
        // the exact installed IOVA 0x80000000 plan and both routes; no method
        // retargets them. Earlier scalar zero/readback references have ended.
        // The one-shot claim transfers the sole CPU writer into the retained
        // Pool owner; fault handling keeps all frames and mappings retained.
        unsafe {
            let physical =
                core::slice::from_raw_parts((&raw const DATA_PAGES).cast::<u64>(), DATA_PAGE_COUNT);
            NativeRam::new(0x72001000 as *mut u32, physical).map_err(buffers::Error::Memory)
        }
    }
    pub(crate) fn installed(&self) -> bool {
        self.installed && !FAULT.load(Ordering::Acquire)
    }
    #[cfg(feature = "sunfish-native-startup-ipa-memory")]
    pub(crate) fn retained_quiet(&self) -> bool {
        self.retained_quiet_with(|_, _| {})
    }

    #[cfg(feature = "sunfish-native-startup-ipa-memory")]
    pub(crate) fn retained_quiet_with(
        &self,
        mut progress: impl FnMut(
            personal_os_rust_server::sunfish_smmu_native::RetainedDomainStage,
            usize,
        ),
    ) -> bool {
        use personal_os_rust_server::sunfish_smmu_native::RetainedDomainStage as Stage;

        progress(Stage::Entry, 0);
        if !self.installed() {
            return false;
        }
        let Some(adapter) = self.adapter.as_ref() else {
            return false;
        };
        progress(Stage::GlobalFaultStatus, 0);
        let global = adapter.global_fault_status();
        progress(Stage::ContextControl, 0);
        let context_control = adapter.context_control();
        progress(Stage::ContextControlValue, context_control as usize);
        progress(Stage::ContextFaultStatus, 0);
        let context = adapter.context_fault_status();
        progress(Stage::ContextFaultStatusValue, context as usize);
        if global != 0 || context & !(3 << 9) != 0 {
            progress(Stage::FaultStatusRejected, context as usize);
            return false;
        }
        progress(Stage::FaultStatusAccepted, context as usize);
        if !RECEIVE_PROFILE {
            return true;
        }
        progress(Stage::ReceiveRecheckStart, 0);
        adapter
            .retained_receive_domain_with(&mut progress)
            .is_ok()
    }
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    pub(crate) fn take_receive_memory(
        &mut self,
    ) -> Result<
        personal_os_rust_server::sunfish_ipa_linux_receive_native::NativeReceiveRam<'static>,
        Error,
    > {
        if !self.installed() || !RECEIVE_PROFILE {
            return Err(Error::State);
        }
        self.adapter
            .as_mut()
            .ok_or(Error::State)?
            .take_receive_memory()
    }
    #[cfg(feature = "sunfish-native-startup-ipa-memory")]
    pub(crate) fn fault_status(&self) -> Option<(u32, u32)> {
        self.adapter.as_ref().map(|adapter| adapter.fault_status())
    }
}

impl Probe {
    #[cfg(feature = "sunfish-native-startup-mpss-smem")]
    pub(crate) fn attach_modem_smem(
        &mut self,
        generation: u64,
        gate: fn(u64) -> Result<(), personal_os_rust_server::sunfish_mpss_linux_smem::Error>,
    ) -> Result<(), Error> {
        if !self.retained_quiet() {
            return Err(Error::State);
        }
        self.adapter
            .as_mut()
            .ok_or(Error::State)?
            .attach_modem_smem(generation, gate)
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smem")]
    pub(crate) fn modem_smem_report(
        &self,
    ) -> Option<personal_os_rust_server::sunfish_mpss_linux_smem::Report> {
        self.adapter.as_ref().and_then(|a| a.modem_smem_report())
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smem")]
    pub(crate) fn modem_smem_step(
        &mut self,
    ) -> Result<personal_os_rust_server::sunfish_mpss_linux_smem::Phase, Error> {
        if !self.retained_quiet() {
            return Err(Error::State);
        }
        self.adapter.as_mut().ok_or(Error::State)?.modem_smem_step()
    }
    #[cfg(feature = "sunfish-native-startup-mpss-smem")]
    pub(crate) fn take_modem_smem_prepared(
        &mut self,
    ) -> Result<personal_os_rust_server::sunfish_mpss_linux_smem::Prepared, Error> {
        if !self.retained_quiet() {
            return Err(Error::State);
        }
        self.adapter
            .as_mut()
            .ok_or(Error::State)?
            .take_modem_smem_prepared()
    }
    pub(crate) fn stop_on_fault(&mut self) {
        FAULT.store(true, Ordering::Release);
        #[cfg(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic")]
        if super::diagnostic_display_owned() {
            return;
        }
        self.display.replace_line(1, "FAULT IRQ STOPPED");
        if let Some(adapter) = &self.adapter {
            let (global, context) = adapter.fault_status();
            self.display.hex(15, "GLOBAL FSR", global);
            self.display.hex(16, "CONTEXT FSR", context);
        }
        self.display.flush();
        // Keep the delivered fault IRQ masked and every mapping retained.
    }
}

/// Read-only delegation for the serialized pre-auth allocation loop. The
/// actual Probe and its mappings stay retained; this uses no Rust reference
/// into its mutably borrowed adapter and performs no SMMU writes/clears.
#[cfg(feature = "sunfish-native-startup-mpss-smem")]
pub(crate) fn preboot_faults_clear() -> bool {
    if FAULT.load(Ordering::Acquire) {
        return false;
    }
    // memory_region_symbol! defines its injection cell, so invoke it only in
    // run(). Read those same existing cells here without defining duplicates.
    unsafe extern "C" {
        static smmu_gr0: usize;
        static smmu_context: usize;
    }
    let gr0 = unsafe { core::ptr::read_volatile(&raw const smmu_gr0) as *const u32 };
    let cb = unsafe { core::ptr::read_volatile(&raw const smmu_context) as *const u32 };
    if gr0 as usize != 0x70000000 || cb as usize != 0x70002000 {
        return false;
    }
    unsafe {
        if core::ptr::read_volatile(&raw const smmu_gr0_paddr) != 0x15000000
            || core::ptr::read_volatile(&raw const smmu_context_paddr) != 0x15084000
        {
            return false;
        }
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        let global = gr0.add(0x48 / 4).read_volatile();
        let context = cb.add(0x58 / 4).read_volatile();
        global == 0 && context & !(3 << 9) == 0
    }
}
impl Handler for Probe {
    type Error = Infallible;
    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Infallible> {
        self.stop_on_fault();
        Ok(())
    }
    fn protected(
        &mut self,
        _channel: Channel,
        _message: MessageInfo,
    ) -> Result<MessageInfo, Infallible> {
        Ok(MessageInfo::new(0, 0))
    }
}
