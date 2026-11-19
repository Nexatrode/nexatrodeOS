//! Concrete cold native RMTFS mapping, SCM and retained preboot providers.
//! The opt-in auth entry consumes the cold lifecycle before PAS/reset. The
//! cold source convention is an explicit boot contract, never inferred from
//! absent services or SMP2P.
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};
#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
use core::sync::atomic::AtomicUsize;
use personal_os_rust_server::{
    qcom_glink_qmi::{Config, Session},
    qcom_modem_handlers::Fault,
    qcom_modem_ram_storage::RamShadow,
    qcom_modem_services::Service,
    qcom_modem_storage::Storage,
    qcom_qmi_router::Address,
    qcom_qrtr_tftp::Server as Tftp,
    qcom_scm_assign as scm, sunfish_linux_rmtfs as pool, sunfish_linux_rmtfs_native as native,
    sunfish_linux_rmtfs_server as storage,
};
use sel4_microkit::{memory_region_symbol, var};
const GENERATION: u64 = 1;
const BUFFER_VA: u64 = 0x77001000;
const SCRATCH_VA: u64 = 0x77301000;
// 0=unclaimed; 1=reviewed cold pre-MPSS lifecycle; 2=terminal;
// 3=AUTH submitted (may execute); 4=actual AUTH receipt; 5=actual ready receipt.
// Only the opt-in auth entry can leave the cold phase; no reset retry exists.
static COLD: AtomicU8 = AtomicU8::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);
static X0: AtomicU64 = AtomicU64::new(u64::MAX);
static X1: AtomicU64 = AtomicU64::new(u64::MAX);
#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
static DIAGNOSTIC_PIXELS: AtomicUsize = AtomicUsize::new(0);
#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
static DIAGNOSTIC_SMC_ERROR: AtomicU64 = AtomicU64::new(u64::MAX);
#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
pub(super) fn diagnostic_smc_error() -> u32 {
    DIAGNOSTIC_SMC_ERROR.load(Ordering::Acquire) as u32
}
#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
pub(super) fn set_diagnostic_display(pixels: *mut u32) {
    DIAGNOSTIC_PIXELS.store(pixels as usize, Ordering::Release);
}
#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
fn show_scm_checkpoint(label: &str) {
    let pixels = DIAGNOSTIC_PIXELS.load(Ordering::Acquire) as *mut u32;
    if !pixels.is_null() {
        let d = unsafe { super::pixel_probe_display::Display::new(pixels) };
        d.replace_line(2, label);
        d.flush();
    }
}
#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
fn show_scm_return(error: u32, a0: u64, a1: u64) {
    let pixels = DIAGNOSTIC_PIXELS.load(Ordering::Acquire) as *mut u32;
    if !pixels.is_null() {
        let d = unsafe { super::pixel_probe_display::Display::new(pixels) };
        d.replace_line(2, "SCM CALL RETURN");
        d.hex(3, "KERNEL ERROR", error);
        d.hex(10, "SCM A0", a0 as u32);
        d.hex(11, "SCM A1", a1 as u32);
        d.flush();
    }
}
#[cfg(feature = "sunfish-native-startup-mpss-smem")]
pub(super) fn preboot_generation(generation: u64) -> bool {
    generation == GENERATION && COLD.load(Ordering::Acquire) == 1 && mappings().is_ok()
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn retained_generation(g: u64) -> bool {
    g == GENERATION && matches!(COLD.load(Ordering::Acquire), 1 | 3 | 4 | 5) && mappings().is_ok()
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn lifecycle(
    g: u64,
) -> Option<personal_os_rust_server::sunfish_mpss_linux_smp2p::Lifecycle> {
    use personal_os_rust_server::sunfish_mpss_linux_smp2p::Lifecycle as L;
    if g != GENERATION || mappings().is_err() {
        return None;
    }
    match COLD.load(Ordering::Acquire) {
        1 => Some(L::PreAuth),
        3 => Some(L::AuthSubmitted),
        4 => Some(L::AuthReturned),
        5 => Some(L::Running),
        _ => None,
    }
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn auth_returned(
    r: &personal_os_rust_server::sunfish_mpss_linux_loader::AuthReturned,
) -> Result<(), Fault> {
    if r.generation() != GENERATION
        || mappings().is_err()
        || COLD
            .compare_exchange(3, 4, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        COLD.store(2, Ordering::Release);
        return Err(Fault::Uncertain);
    }
    Ok(())
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn running(
    a: &personal_os_rust_server::sunfish_mpss_linux_loader::AuthReturned,
    r: &personal_os_rust_server::sunfish_mpss_linux_smp2p::ReadyObservation,
) -> Result<(), Fault> {
    if a.generation() != GENERATION
        || r.generation() != GENERATION
        || mappings().is_err()
        || COLD
            .compare_exchange(4, 5, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        COLD.store(2, Ordering::Release);
        return Err(Fault::Uncertain);
    }
    Ok(())
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
pub(super) fn abort() {
    COLD.store(2, Ordering::Release);
}
pub(super) type NativeServer = storage::Server<'static, native::Native<Gate, Smc>>;
type Resources = pool::Resources<'static, native::Native<Gate, Smc>>;

fn mappings() -> Result<pool::Mappings, Fault> {
    let buffer = memory_region_symbol!(native_rmtfs:*mut u8).as_ptr() as u64;
    let scratch = memory_region_symbol!(rmtfs_scm:*mut u8).as_ptr() as u64;
    let bp = *var!(native_rmtfs_paddr:usize=0) as u64;
    let sp = *var!(rmtfs_scm_paddr:usize=0) as u64;
    if buffer != BUFFER_VA || scratch != SCRATCH_VA || bp != pool::BUFFER_PA {
        return Err(Fault::Failed);
    }
    let make = |va, pa, bytes| pool::Mapping {
        virtual_address: va,
        physical_address: pa,
        bytes,
        writable: true,
        executable: false,
        cached: false,
    };
    pool::Mappings {
        buffer: make(buffer, bp, pool::BUFFER_BYTES),
        scratch: make(scratch, sp, pool::PAGE),
    }
    .validate()
}
pub(super) struct Gate {
    maps: pool::Mappings,
    failed: bool,
}
impl native::Gate for Gate {
    fn observe(&mut self) -> Result<pool::Observation, Fault> {
        if self.failed {
            return Err(Fault::Uncertain);
        }
        self.failed = true;
        let phase = COLD.load(Ordering::Acquire);
        let allowed = phase == 1;
        #[cfg(feature = "sunfish-native-startup-mpss-auth")]
        let allowed = allowed || matches!(phase, 3 | 4 | 5);
        if !allowed || mappings() != Ok(self.maps) {
            COLD.store(2, Ordering::Release);
            return Err(Fault::Uncertain);
        }
        self.failed = false;
        Ok(pool::Observation {
            generation: GENERATION,
            identity: 1,
            ready: true,
            mappings: self.maps,
            // True only before the actual AUTH boundary consumes the trusted
            // cold-preboot contract; never inferred from an MMIO sample.
            modem_stopped: phase == 1,
            initial_hlos: phase == 1,
        })
    }
}
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
impl native::AuthGate for Gate {
    fn submit_mpss_auth(
        &mut self,
        g: u64,
        r: &personal_os_rust_server::sunfish_mpss_linux_loader::Request,
    ) -> Result<(), Fault> {
        if self.failed {
            abort();
            return Err(Fault::Uncertain);
        }
        self.failed = true;
        if g != GENERATION
            || r.kind() != personal_os_rust_server::sunfish_mpss_linux_loader::Kind::AuthAndReset
            || r.words() != &[0x42000205, 1, 4, 0, 0, 0, 0, 0]
            || mappings() != Ok(self.maps)
            || COLD
                .compare_exchange(1, 3, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            abort();
            return Err(Fault::Uncertain);
        }
        // Consume reset provenance before the caller enters the monitor.
        self.failed = false;
        Ok(())
    }
}
#[cfg(all(test, mpss_auth_root_tests))]
#[path = "../../tests/pixel_mpss_auth_lifecycle_cases.rs"]
mod auth_root_tests;
pub(super) struct Smc {
    maps: pool::Mappings,
}
fn barrier() {
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
}
struct Monitor;
impl scm::Transport for Monitor {
    fn invoke(&mut self, w: [u64; 8]) -> Result<[u64; 8], Fault> {
        CALLS.fetch_add(1, Ordering::Relaxed);
        let args = sel4::sys::seL4_ARM_SMCContext {
            x0: w[0],
            x1: w[1],
            x2: w[2],
            x3: w[3],
            x4: w[4],
            x5: w[5],
            x6: w[6],
            x7: w[7],
        };
        let mut r = sel4::sys::seL4_ARM_SMCContext::default();
        barrier();
        #[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
        show_scm_checkpoint("SCM CALL ENTER");
        let e = sel4::with_ipc_buffer_mut(|i| i.inner_mut().seL4_ARM_SMC_Call(7, &args, &mut r));
        barrier();
        #[cfg(any(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
        {
            DIAGNOSTIC_SMC_ERROR.store(e as u64, Ordering::Release);
            show_scm_return(e as u32, r.x0, r.x1);
        }
        if e != 0 {
            return Err(Fault::Uncertain);
        }
        X0.store(r.x0, Ordering::Relaxed);
        X1.store(r.x1, Ordering::Relaxed);
        Ok([r.x0, r.x1, r.x2, r.x3, r.x4, r.x5, r.x6, r.x7])
    }
}
struct Check(pool::Mappings);
impl scm::Lifecycle for Check {
    fn revalidate(&mut self) -> Result<(), Fault> {
        if COLD.load(Ordering::Acquire) == 1 && mappings() == Ok(self.0) {
            Ok(())
        } else {
            Err(Fault::Uncertain)
        }
    }
}
struct Delay;
impl scm::Delay for Delay {
    fn wait_ms(&mut self, ms: u32) -> Result<(), Fault> {
        let hz: u64;
        let start: u64;
        unsafe {
            core::arch::asm!("mrs {v}, cntfrq_el0",v=out(reg)hz,options(nostack,preserves_flags));
            core::arch::asm!("isb","mrs {v}, cntpct_el0",v=out(reg)start,options(nostack,preserves_flags));
        }
        if !(1_000_000..=1_000_000_000).contains(&hz) || ms != scm::BUSY_WAIT_MS {
            return Err(Fault::Uncertain);
        }
        let ticks = (hz * u64::from(ms) + 999) / 1000;
        for _ in 0..50_000_000 {
            let now: u64;
            unsafe {
                core::arch::asm!("isb","mrs {v}, cntpct_el0",v=out(reg)now,options(nostack,preserves_flags));
            }
            if now.wrapping_sub(start) >= ticks {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(Fault::Uncertain)
    }
}
impl native::Smc for Smc {
    fn invoke(&mut self, c: &pool::Call) -> Result<[u64; 8], Fault> {
        let expected = pool::Call::new(self.maps)?;
        if c.words() != expected.words() || c.bytes() != expected.bytes() {
            return Err(Fault::Failed);
        }
        scm::invoke_checked(&mut Monitor, &mut Delay, &mut Check(self.maps), *c.words())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Phase {
    New,
    Preparing,
    Sharing,
    Prepared,
    Failed,
    #[cfg(feature = "sunfish-native-startup-mpss-auth")]
    Delegated,
}
pub(super) struct Factory {
    storage: Option<Storage<RamShadow<'static>>>,
    tftp: Option<Tftp<'static>>,
    resources: Option<Resources>,
    server: Option<NativeServer>,
    session: Option<Session>,
    unattached: Option<(Gate, Smc)>,
    phase: Phase,
    error: u32,
}

/// Read-only evidence for every requirement checked before the runtime RMTFS
/// handoff. `None` means the owner or profile is absent, never a pass.
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RuntimeBootReport {
    pub phase: Phase,
    pub server_present: bool,
    pub server_failed: Option<bool>,
    pub session_present: bool,
    pub tftp_provider_present: bool,
}

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimeBootField {
    Phase,
    ServerPresent,
    ServerFailed,
    SessionPresent,
    TftpProviderPresent,
}

impl Factory {
    #[cfg(feature = "sunfish-native-startup-mpss-auth")]
    pub(super) fn take_boot_parts(
        &mut self,
    ) -> Result<(NativeServer, Session, Tftp<'static>), Fault> {
        if self.phase != Phase::Prepared
            || self.server.as_ref().is_none_or(|s| s.report().failed)
            || self
                .session
                .as_ref()
                .is_none_or(|s| !s.transport_is_pristine())
            || self.tftp.is_none()
            || !preboot_generation(GENERATION)
        {
            self.fault(0x30);
            return Err(Fault::Uncertain);
        }
        self.phase = Phase::Delegated;
        Ok((
            self.server.take().unwrap(),
            self.session.take().unwrap(),
            self.tftp.take().unwrap(),
        ))
    }

    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub(super) fn runtime_boot_report(&self) -> RuntimeBootReport {
        self.runtime_boot_report_with(|_, _| {})
    }

    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub(super) fn runtime_boot_report_with(
        &self,
        mut checkpoint: impl FnMut(RuntimeBootField, Option<u32>),
    ) -> RuntimeBootReport {
        checkpoint(RuntimeBootField::Phase, None);
        let phase = self.phase;
        checkpoint(RuntimeBootField::Phase, Some(phase as u32));

        checkpoint(RuntimeBootField::ServerPresent, None);
        let server_present = self.server.is_some();
        checkpoint(
            RuntimeBootField::ServerPresent,
            Some(u32::from(server_present)),
        );

        checkpoint(RuntimeBootField::ServerFailed, None);
        let server_failed = self.server.as_ref().map(|server| server.report().failed);
        checkpoint(
            RuntimeBootField::ServerFailed,
            Some(server_failed.map(u32::from).unwrap_or(u32::MAX)),
        );

        checkpoint(RuntimeBootField::SessionPresent, None);
        let session_present = self.session.is_some();
        checkpoint(
            RuntimeBootField::SessionPresent,
            Some(u32::from(session_present)),
        );

        checkpoint(RuntimeBootField::TftpProviderPresent, None);
        let tftp_provider_present = self.tftp.is_some();
        checkpoint(
            RuntimeBootField::TftpProviderPresent,
            Some(u32::from(tftp_provider_present)),
        );

        RuntimeBootReport {
            phase,
            server_present,
            server_failed,
            session_present,
            tftp_provider_present,
        }
    }

    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub(super) fn runtime_profile_report(
        &self,
    ) -> Option<personal_os_rust_server::qcom_glink_qmi::RuntimeProfileReport> {
        self.session
            .as_ref()
            .map(Session::runtime_profile_report)
    }

    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    pub(super) fn runtime_profile_report_with(
        &self,
        checkpoint: impl FnMut(
            personal_os_rust_server::qcom_glink_qmi::RuntimeProfileField,
            Option<u32>,
        ),
    ) -> Option<personal_os_rust_server::qcom_glink_qmi::RuntimeProfileReport> {
        self.session
            .as_ref()
            .map(|session| session.runtime_profile_report_with(checkpoint))
    }

    /// Runtime handoff keeps the immutable TFTP provider outside QRTR and
    /// transfers only the real RMTFS server plus the constrained session.
    #[cfg(all(
        feature = "sunfish-native-startup-mpss-runtime",
        not(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic")
    ))]
    pub(super) fn take_runtime_boot_parts(&mut self) -> Result<(NativeServer, Session), Fault> {
        if self.phase != Phase::Prepared
            || self.server.as_ref().is_none_or(|s| s.report().failed)
            || self.session.as_ref().is_none_or(|s| {
                let r = s.runtime_profile_report();
                !r.remote_storage_registered
                    || r.remote_storage_ready
                    || r.service_registry_registered
                    || r.ipa_server_registered
                    || !r.linux_ipa_client_enabled
                    || r.tftp_port.is_some()
                    || r.tftp_server_prepared
            })
            || self.tftp.is_none()
            || !preboot_generation(GENERATION)
        {
            self.fault(0x31);
            return Err(Fault::Uncertain);
        }
        self.phase = Phase::Delegated;
        Ok((self.server.take().unwrap(), self.session.take().unwrap()))
    }
    #[cfg(all(feature = "sunfish-native-startup-mpss-runtime", feature = "sunfish-tftp-wlan-mux",
        not(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic")))]
    pub(super) fn take_runtime_wlan_boot_parts(&mut self) -> Result<(NativeServer, Session, Tftp<'static>), Fault> {
        let (server, session) = self.take_runtime_boot_parts()?;
        // take_runtime_boot_parts has already verified that this unique owner exists.
        Ok((server, session, self.tftp.take().unwrap()))
    }
    pub(super) fn new(storage: Storage<RamShadow<'static>>, tftp: Tftp<'static>) -> Self {
        Self {
            storage: Some(storage),
            tftp: Some(tftp),
            resources: None,
            server: None,
            session: None,
            unattached: None,
            phase: Phase::New,
            error: 0,
        }
    }
    pub(super) fn phase(&self) -> Phase {
        self.phase
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")]
    pub(super) fn preflight_share_with(
        &mut self,
        checkpoint: impl FnMut(pool::ShareCheckpoint),
    ) -> Result<(), Fault> {
        if self.phase != Phase::Sharing {
            self.fault(0x0a);
            return Err(Fault::Uncertain);
        }
        let Some(resources) = self.resources.as_mut() else {
            self.fault(0x0a);
            return Err(Fault::Uncertain);
        };
        let result = resources.preflight_share_with(checkpoint);
        if result.is_err() {
            self.fault(0x0a);
        }
        result
    }
    pub(super) fn error(&self) -> u32 {
        self.error
    }
    pub(super) fn status(&self) -> Option<pool::Status> {
        self.resources
            .as_ref()
            .map(|r| r.status().0)
            .or_else(|| self.server.as_ref().map(|s| s.resource_status().0))
    }
    pub(super) fn monitor(&self) -> [u64; 3] {
        [
            CALLS.load(Ordering::Relaxed),
            X0.load(Ordering::Relaxed),
            X1.load(Ordering::Relaxed),
        ]
    }
    pub(super) fn providers(&self) -> [bool; 3] {
        [
            self.server.as_ref().is_some_and(|s| !s.report().failed),
            self.tftp.as_ref().is_some_and(Tftp::image_available),
            self.session.is_some(),
        ]
    }
    pub(super) fn fault(&mut self, error: u32) {
        COLD.store(2, Ordering::Release);
        self.phase = Phase::Failed;
        self.error = error;
        if let Some(s) = self.server.as_mut() {
            s.fault();
        }
    }
    /// # Safety
    /// This exact, hash-checked standalone image was started once on the bound
    /// Sunfish after >=10 seconds fully powered off, through the pinned common
    /// ABL/secure-firmware handoff audited against the Linux reference (which
    /// additionally passes through U-Boot; this image boots directly). No MPSS was started by earlier
    /// host code; the fixed reservation follows Linux's fresh HLOS convention.
    /// Retain the audited frames, scratch and SMC cap until reboot, including
    /// faults; no concurrent/cached host user, alias or teardown is permitted.
    /// Host identity/cold-boot checks are external preconditions, not proved by
    /// setvars, local phase flags, or the source archive. Call only in init.
    pub(super) unsafe fn begin(&mut self) {
        if self.phase != Phase::New
            || COLD
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            self.fault(1);
            return;
        }
        let Ok(maps) = mappings() else {
            self.fault(2);
            return;
        };
        if self
            .storage
            .as_ref()
            .is_none_or(|s| s.configuration().generation != GENERATION)
            || self.tftp.as_ref().is_none_or(|s| {
                !s.image_available()
                    || s.active_transfers() != 0
                    || s.route_binding() != (1, 0, 0x5000)
                    || s.next_transfer_port() != 0x5100
            })
        {
            self.fault(3);
            return;
        }
        let gate = Gate {
            maps,
            failed: false,
        };
        let smc = Smc { maps };
        // SAFETY: caller's concrete cold contract plus exact injected geometry;
        // the CapDL validator checks all frame aliases, attrs and guard holes.
        let io = match unsafe { native::Native::new(gate, smc, maps) } {
            Ok(io) => io,
            Err((g, s, _)) => {
                self.unattached = Some((g, s));
                self.fault(4);
                return;
            }
        };
        let p = pool::Pool::attach(GENERATION, maps, io);
        self.resources = Some(Resources::attach(p, self.storage.take().unwrap()));
        if self.resources.as_ref().unwrap().status().2 {
            self.fault(5);
            return;
        }
        self.phase = Phase::Preparing;
    }
    /// One page of preparation, or the single logical SCM transaction followed
    /// by local service preparation. No MPSS/PAS4 or transport publication.
    pub(super) fn step(&mut self) {
        match self.phase {
            Phase::Preparing => match self.resources.as_mut().unwrap().prepare_step() {
                Ok(true) => self.phase = Phase::Sharing,
                Ok(false) => {}
                Err(_) => self.fault(6),
            },
            Phase::Sharing => {
                if self.resources.as_mut().unwrap().share().is_err() {
                    self.fault(7);
                    return;
                }
                self.server = Some(storage::Server::attach(
                    storage::Configuration {
                        generation: GENERATION,
                        local: Address {
                            node: 1,
                            port: 0x4000,
                        },
                        peer_node: 0,
                    },
                    self.resources.take().unwrap(),
                ));
                if self.server.as_ref().unwrap().report().failed {
                    self.fault(8);
                    return;
                }
                // Actual providers are retained here before marking endpoints
                // prepared. The future authenticated FIFO attach must still
                // bind/activate this same server before control publication.
                #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
                let prepared = Session::native_runtime_profile(
                    Config {
                        local_node: 1,
                        peer_node: 0,
                        local_channel: 1,
                        remote_channel: 2,
                    },
                    GENERATION,
                );
                #[cfg(not(feature = "sunfish-native-startup-mpss-runtime"))]
                let prepared = (|| {
                    let mut s = Session::new(
                        Config {
                            local_node: 1,
                            peer_node: 0,
                            local_channel: 1,
                            remote_channel: 2,
                        },
                        GENERATION,
                    )?;
                    for (kind, port) in [
                        (Service::RemoteStorage, 0x4000),
                        (Service::ServiceRegistry, 0x4003),
                        (Service::IpaServer, 0x4002),
                    ] {
                        s.register(GENERATION, kind, port)?;
                        s.prepare_service_available(GENERATION, kind)?;
                    }
                    s.enable_linux_ipa(GENERATION, 0x4004)?;
                    s.prepare_tftp_server(GENERATION, 0x5000)?;
                    Ok::<_, personal_os_rust_server::qcom_glink_qmi::Error>(s)
                })();
                match prepared {
                    Ok(s) => {
                        self.session = Some(s);
                        self.phase = Phase::Prepared
                    }
                    Err(_) => self.fault(9),
                }
            }
            Phase::New | Phase::Prepared | Phase::Failed => {}
            #[cfg(feature = "sunfish-native-startup-mpss-auth")]
            Phase::Delegated => {}
        }
    }
}
