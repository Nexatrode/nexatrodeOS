//! Concrete one-shot PAS4 start and delivered SMP2P-ready observation.
//! All earlier IPA/storage/power owners remain live. No GLINK/WLAN start here.
use super::*;
use core::{
    mem::ManuallyDrop,
    sync::atomic::{AtomicU8, AtomicU64, Ordering},
};
#[cfg(feature = "sunfish-native-startup-mpss-glink")]
use personal_os_rust_server::sunfish_mpss_live_domain::ReceiptHandoff;
use personal_os_rust_server::{
    qcom_glink_qmi::Session, qcom_modem_handlers::Fault, qcom_qrtr_tftp::Server as Tftp,
    qcom_scm_assign as smccc, sunfish_ipa_linux_pas::Resources, sunfish_ipa_linux_receive_start,
    sunfish_mpss_linux_boot as boot, sunfish_mpss_linux_loader as pas,
    sunfish_mpss_linux_memory::Window, sunfish_mpss_linux_power as power,
    sunfish_mpss_linux_smp2p as smp,
};
use sel4_microkit::var;
#[path = "pixel_mpss_auth_irq.rs"]
mod irq_policy;
#[path = "pixel_mpss_auth_diagnostic.rs"]
mod diagnostic;
use irq_policy::{SMP, SMP_CHANNEL, WDOG, WDOG_CHANNEL};
const GENERATION: u64 = 1;
#[cfg(not(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic"))]
const MODEM: &[u8] = include_bytes!(env!("SUNFISH_MPSS_FIRMWARE_PATH"));
static STARTED: AtomicU8 = AtomicU8::new(0);
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
static ENTRY_ATTEMPTED: AtomicU8 = AtomicU8::new(0);
static ERROR: AtomicU64 = AtomicU64::new(0);
static PENDING: AtomicU64 = AtomicU64::new(0);
static DEFERRED: AtomicU64 = AtomicU64::new(0);
static DIAGNOSTIC: diagnostic::Report = diagnostic::Report::new();
static SMP_NOTICES: AtomicU64 = AtomicU64::new(0);
static WDOG_NOTICES: AtomicU64 = AtomicU64::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);
static X0: AtomicU64 = AtomicU64::new(u64::MAX);
static X1: AtomicU64 = AtomicU64::new(u64::MAX);
static KERNEL_ERROR: AtomicU64 = AtomicU64::new(0);
pub(super) type NativeBoot = boot::Boot<
    'static,
    'static,
    Window<'static>,
    personal_os_rust_server::sunfish_linux_rmtfs_native::Native<
        pixel_linux_rmtfs::Gate,
        pixel_linux_rmtfs::Smc,
    >,
>;
#[derive(Clone, Copy, Eq, PartialEq)]
enum Phase {
    Preparing,
    Loading,
    WaitingReady,
    Ready,
    Failed,
    Delegated,
}
pub(super) struct Runtime {
    boot: Option<NativeBoot>,
    unstarted: Option<pixel_linux_rmtfs::NativeServer>,
    _unloaded: Option<ManuallyDrop<(pas::Image<'static>, Window<'static>)>>,
    _session: Option<ManuallyDrop<Session>>,
    _tftp: Option<ManuallyDrop<Tftp<'static>>>,
    auth: Option<ManuallyDrop<pas::AuthReturned>>,
    ready: Option<ManuallyDrop<smp::ReadyObservation>>,
    phase: Phase,
    error: u32,
    steps: u32,
    acks: u32,
    ready_irq: bool,
    cold_peer_verified: bool,
    hz: u64,
    since: u64,
}

#[cfg(feature = "sunfish-native-startup-mpss-glink")]
pub(super) struct RuntimeParts {
    pub boot: NativeBoot,
    pub auth: pas::AuthReturned,
    pub ready: smp::ReadyObservation,
    pub gate: pixel_smmu_install::RuntimeGate,
    pub session: Session,
    #[cfg(feature = "sunfish-tftp-wlan-mux")]
    pub firmware: Tftp<'static>,
    pub receive: pixel_ipa_receive::Runtime,
    pub hz: u64,
    pub pending: u64,
}

/// Consume the actual PAS-returned and delivered-ready receipts exactly once
/// and move the retained owners into the combined post-auth runtime.
///
/// All checks happen before extracting any owner. A failed handoff leaves the
/// auth runtime and its protected receipts in place; the caller must treat the
/// returned fault as terminal and retain the startup state until reboot.
#[cfg(feature = "sunfish-native-startup-mpss-glink")]
pub(super) fn take_runtime(s: &mut Startup) -> Result<RuntimeParts, Fault> {
    let ready = s._mpss_auth.as_ref().is_some_and(|r| {
        r.phase == Phase::Ready
            && r.ready_irq
            && r.cold_peer_verified
            && r.unstarted.is_none()
            && r._unloaded.is_none()
            && r.boot.as_ref().is_some_and(|b| {
                b.report().phase == boot::Phase::AuthReturned
                    && b.loader_report().phase == pas::Phase::AuthReturned
            })
            && r.auth
                .as_ref()
                .is_some_and(|a| a.generation() == GENERATION)
            && r.ready
                .as_ref()
                .is_some_and(|v| v.generation() == GENERATION)
            && r._session.is_some()
            && if cfg!(feature = "sunfish-tftp-wlan-mux") { r._tftp.is_some() } else { r._tftp.is_none() }
            && (1_000_000..=1_000_000_000).contains(&r.hz)
    });
    if !ready
        || !healthy(s)
        || s._receive_runtime.is_none()
        || pixel_linux_rmtfs::lifecycle(GENERATION) != Some(smp::Lifecycle::Running)
        || PENDING.load(Ordering::Acquire) & WDOG != 0
        || s._mpss_auth
            .as_ref()
            .and_then(|r| r._session.as_ref())
            .is_none_or(|session| {
                let profile = session.runtime_profile_report();
                !profile.remote_storage_registered
                    || profile.remote_storage_ready
                    || profile.service_registry_registered
                    || profile.ipa_server_registered
                    || !profile.linux_ipa_client_enabled
                    || profile.tftp_port.is_some()
                    || profile.tftp_server_prepared
            })
    {
        fault(s, 0x50);
        return Err(Fault::Uncertain);
    }

    // Make delegation terminal before moving any receipt or owner. This also
    // prevents a second call from creating another live runtime.
    s._mpss_auth.as_mut().unwrap().phase = Phase::Delegated;

    let mut handoff = {
        let r = s._mpss_auth.as_mut().unwrap();
        ReceiptHandoff::new(
            GENERATION,
            r.auth.take().map(ManuallyDrop::into_inner),
            r.ready.take().map(ManuallyDrop::into_inner),
        )
    };
    let (auth, ready) = match handoff.take(true) {
        Ok(receipts) => receipts,
        Err(_) => {
            let (auth, ready) = handoff.into_retained();
            let r = s._mpss_auth.as_mut().unwrap();
            r.auth = auth.map(ManuallyDrop::new);
            r.ready = ready.map(ManuallyDrop::new);
            fault(s, 0x51);
            return Err(Fault::Uncertain);
        }
    };

    let gate = match s._domain.as_mut().and_then(|d| d.take_runtime_gate().ok()) {
        Some(gate) => gate,
        None => {
            let r = s._mpss_auth.as_mut().unwrap();
            r.auth = Some(ManuallyDrop::new(auth));
            r.ready = Some(ManuallyDrop::new(ready));
            fault(s, 0x52);
            return Err(Fault::Uncertain);
        }
    };

    let r = s._mpss_auth.as_mut().unwrap();
    let parts = RuntimeParts {
        boot: r.boot.take().unwrap(),
        auth,
        ready,
        gate,
        session: ManuallyDrop::into_inner(r._session.take().unwrap()),
        #[cfg(feature = "sunfish-tftp-wlan-mux")]
        firmware: ManuallyDrop::into_inner(r._tftp.take().unwrap()),
        receive: s._receive_runtime.take().unwrap(),
        hz: r.hz,
        pending: PENDING.swap(0, Ordering::AcqRel) | DEFERRED.swap(0, Ordering::AcqRel),
    };
    Ok(parts)
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct Views {
    metadata: *mut u8,
    metadata_pa: u64,
    target: *mut u8,
}
fn views() -> Result<Views, pas::Error> {
    let metadata = memory_region_symbol!(mpss_pas_metadata:*mut u8).as_ptr();
    let target = memory_region_symbol!(mpss_target:*mut u8).as_ptr();
    let metadata_pa = *var!(mpss_pas_metadata_paddr:usize=0) as u64;
    if metadata as usize != 0x77501000
        || target as usize != 0x88000000
        || *var!(mpss_target_paddr:usize=0) as u64 != pas::BASE
        || metadata_pa % 4096 != 0
        || metadata_pa < 0x80000000
        || metadata_pa
            .checked_add(0x3000)
            .is_none_or(|end| end > 1u64 << 32)
    {
        return Err(pas::Error::Geometry);
    }
    Ok(Views {
        metadata,
        metadata_pa,
        target,
    })
}
fn counter() -> u64 {
    let v;
    unsafe {
        core::arch::asm!("isb","mrs {v}, cntpct_el0",v=out(reg)v,options(nostack,preserves_flags));
    }
    v
}
fn frequency() -> u64 {
    let v;
    unsafe {
        core::arch::asm!("mrs {v}, cntfrq_el0",v=out(reg)v,options(nostack,preserves_flags));
    }
    v
}
fn raw_badge() -> u64 {
    let (_, badge) =
        sel4::Cap::<sel4::cap_type::Endpoint>::from_bits(1)
            .nb_recv(sel4::Cap::<sel4::cap_type::Reply>::from_bits(4));
    badge
}
fn capture(b: u64) -> u64 {
    DIAGNOSTIC.observe(b);
    if b & SMP != 0 {
        SMP_NOTICES.fetch_add(1, Ordering::Relaxed);
    }
    if b & WDOG != 0 {
        WDOG_NOTICES.fetch_add(1, Ordering::Relaxed);
    }
    let (owned, rest) = irq_policy::split(b);
    PENDING.fetch_or(owned, Ordering::AcqRel);
    rest
}
/// Shared notification endpoint: MPSS and runtime GLINK bits are retained for their real owners,
/// never mistaken for an IPA fault or lost by the earlier polling helpers.
pub(super) fn receive_badge() -> u64 {
    capture(raw_badge()) | DEFERRED.swap(0, Ordering::AcqRel)
}
fn capture_during_call() -> bool {
    let rest = capture(raw_badge());
    DEFERRED.fetch_or(rest, Ordering::AcqRel);
    irq_policy::monitor_healthy(rest, PENDING.load(Ordering::Acquire))
}
pub(super) fn started() -> bool {
    STARTED.load(Ordering::Acquire) != 0
}
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
pub(super) fn diagnostics_owned() -> bool {
    ENTRY_ATTEMPTED.load(Ordering::Acquire) != 0
}
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
pub(super) fn runtime_ready(s: &Startup) -> bool {
    s._mpss_auth
        .as_ref()
        .is_some_and(|r| r.phase == Phase::Ready && r.ready_irq && r.cold_peer_verified)
}
fn show_loader_checkpoint(
    d: &Display,
    r: &Runtime,
    label: &str,
    detail: Option<(&str, u32)>,
) {
    d.replace_line(1, label);
    if let Some((field, value)) = detail {
        d.hex(2, field, value);
    }
    d.hex(3, "LOADER STEPS", r.steps);
    if let Some(b) = r.boot.as_ref() {
        let p = b.loader_report();
        d.hex(4, "LOAD PHASE", p.phase as u32);
        d.hex(5, "PAS ATTEMPTS", p.attempted_mask);
        d.hex(6, "PAS RETURNED", p.returned_mask);
        d.hex(7, "TARGET WRITES", p.target_writes as u32);
        d.hex(8, "TARGET VERIFIED", p.target_verified as u32);
        d.hex(9, "LOADER ERROR", p.error.map_or(0, |e| e as u32 + 1));
    }
    d.hex(10, "LOW LEVEL SMC", CALLS.load(Ordering::Relaxed) as u32);
    d.flush();
}
fn show_runtime_checkpoint(
    d: Option<&Display>,
    s: &Startup,
    label: &str,
    detail: Option<(&str, u32)>,
) {
    if let Some(d) = d {
        if let Some(r) = s._mpss_auth.as_ref() {
            show_loader_checkpoint(d, r, label, detail);
        }
    }
}
fn retained_domain_quiet(s: &Startup, d: Option<&Display>) -> bool {
    let Some(domain) = s._domain.as_ref() else {
        return false;
    };
    let Some(d) = d else {
        return domain.retained_quiet();
    };
    domain.retained_quiet_with(|stage, index| {
        use personal_os_rust_server::sunfish_smmu_native::RetainedDomainStage as Stage;
        let (label, field, value) = match stage {
            Stage::Entry => ("DOMAIN ENTRY", "DOMAIN INDEX", index as u32),
            Stage::GlobalFaultStatus => ("DOM FSR GLOBAL", "REG ADDRESS", 0x1500_0048),
            Stage::ContextControl => ("DOM CB CONTROL", "REG ADDRESS", 0x1508_4000),
            Stage::ContextControlValue => ("DOM CB CTRL OK", "CB SCTLR VALUE", index as u32),
            Stage::ContextFaultStatus => ("DOM FSR CONTEXT", "REG ADDRESS", 0x1508_4058),
            Stage::ContextFaultStatusValue => ("DOM CB FSR OK", "CB FSR VALUE", index as u32),
            Stage::FaultStatusRejected => ("FSR REJECTED", "CB FSR VALUE", index as u32),
            Stage::FaultStatusAccepted => ("FSR ACCEPTED", "CB FSR VALUE", index as u32),
            Stage::ReceiveRecheckStart => ("RX RECHECK", "DOMAIN INDEX", index as u32),
            Stage::AsidBank => ("ASID BANK", "DOMAIN INDEX", index as u32),
            Stage::RouteSlot => ("ROUTE SLOT", "DOMAIN INDEX", index as u32),
            Stage::Register => ("SMMU REGISTER", "REG INDEX", index as u32),
            Stage::TableSetup => ("TABLE SETUP", "SETUP STEP", index as u32),
            Stage::TablePage => ("TABLE PAGE", "PAGE INDEX", index as u32),
            Stage::Complete => ("DOMAIN DONE", "DOMAIN INDEX", index as u32),
        };
        if let Some(r) = s._mpss_auth.as_ref() {
            show_loader_checkpoint(d, r, label, Some((field, value)));
        }
    })
}
fn healthy_with_progress(s: &Startup, d: Option<&Display>) -> bool {
    show_runtime_checkpoint(d, s, "RX HEALTH CHECK", None);
    let receive = pixel_ipa_receive::healthy(s);
    show_runtime_checkpoint(d, s, "RX HEALTH", Some(("RX HEALTH", u32::from(receive))));
    if !receive {
        DIAGNOSTIC.fail(diagnostic::Failure::Receive);
        return false;
    }
    show_runtime_checkpoint(d, s, "DOMAIN CHECK", None);
    let domain = retained_domain_quiet(s, d);
    show_runtime_checkpoint(d, s, "DOMAIN RESULT", Some(("DOMAIN QUIET", u32::from(domain))));
    if !domain {
        DIAGNOSTIC.fail(diagnostic::Failure::Domain);
        return false;
    }
    show_runtime_checkpoint(d, s, "PREAUTH CHECK", None);
    let preauth = pixel_mpss_preauth::verified();
    show_runtime_checkpoint(d, s, "PREAUTH RESULT", Some(("PREAUTH OK", u32::from(preauth))));
    if !preauth {
        DIAGNOSTIC.fail(diagnostic::Failure::Preauth);
        return false;
    }
    show_runtime_checkpoint(d, s, "RMTFS CHECK", None);
    let retained = pixel_linux_rmtfs::retained_generation(GENERATION);
    show_runtime_checkpoint(d, s, "RMTFS RESULT", Some(("RMTFS GEN", u32::from(retained))));
    if !retained {
        DIAGNOSTIC.fail(diagnostic::Failure::Rmtfs);
    }
    retained
}
fn healthy(s: &Startup) -> bool {
    healthy_with_progress(s, None)
}

#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
fn show_gate_diagnostic(d: &Display, s: &Startup, stop_after_report: bool) -> bool {
    const UNAVAILABLE: u32 = u32::MAX;

    d.clear();
    d.replace_line(0, "MPSS RUNTIME GATE CHECK");
    d.replace_line(1, "AUTH ENTRY START");
    d.flush();
    d.replace_line(1, "AUTH ENTRY CHECKING");
    for (line, label) in [
        (2, "PREAUTH OK"),
        (3, "IPA RX HEALTH"),
        (4, "DOMAIN QUIET"),
        (5, "RMTFS GEN"),
        (6, "AUTH HEALTH"),
        (8, "FACTORY PHASE"),
        (9, "SERVER PRESENT"),
        (10, "SERVER FAILED"),
        (11, "SESSION PRESENT"),
        (12, "TFTP PROVIDER"),
        (14, "PROFILE PRESENT"),
        (15, "REMOTE REG"),
        (16, "REMOTE READY"),
        (17, "SERVREG REG"),
        (18, "IPA SERVER REG"),
        (19, "LINUX IPA"),
        (20, "TFTP PRESENT"),
        (21, "TFTP PORT VALUE"),
        (22, "TFTP PREPARED"),
        (23, "PREBOOT GEN"),
        (24, "PROFILE PASS"),
        (25, "HANDOFF PASS"),
    ] {
        d.hex(line, label, UNAVAILABLE);
    }
    d.replace_line(7, "FACTORY SNAPSHOT NOT READ");
    d.replace_line(13, "PROFILE READ NOT STARTED");
    d.replace_line(26, "STOP BEFORE TAKE/PAS4");
    d.replace_line(27, "NO RESET/GLINK/WLAN");
    d.replace_line(28, "NO FLASH/RETRY");
    d.replace_line(29, "FF=UNAVAILABLE");
    d.flush();

    d.replace_line(2, "CHECK PREAUTH VERIFIED");
    d.flush();
    let preauth_verified = pixel_mpss_preauth::verified();
    d.hex(2, "PREAUTH OK", u32::from(preauth_verified));
    d.flush();

    d.replace_line(3, "CHECK IPA RX HEALTH");
    d.flush();
    let ipa_rx_healthy = pixel_ipa_receive::healthy(s);
    d.hex(3, "IPA RX HEALTH", u32::from(ipa_rx_healthy));
    d.flush();

    d.replace_line(4, "CHECK DOMAIN QUIET");
    d.flush();
    let domain_quiet = s._domain.as_ref().is_some_and(|domain| {
        domain.retained_quiet_with(|stage, index| {
            use personal_os_rust_server::sunfish_smmu_native::RetainedDomainStage as Stage;
            let (label, detail, value) = match stage {
                Stage::Entry => ("DOMAIN OWNER", "DOMAIN INDEX", index as u32),
                Stage::GlobalFaultStatus => ("DOM FSR GLOBAL", "REG ADDRESS", 0x1500_0048),
                Stage::ContextControl => ("DOM CB CONTROL", "REG ADDRESS", 0x1508_4000),
                Stage::ContextControlValue => ("DOM CB CTRL OK", "CB SCTLR VALUE", index as u32),
                Stage::ContextFaultStatus => ("DOM FSR CONTEXT", "REG ADDRESS", 0x1508_4058),
                Stage::ContextFaultStatusValue => ("DOM CB FSR OK", "CB FSR VALUE", index as u32),
                Stage::FaultStatusRejected => ("FSR REJECTED", "CB FSR VALUE", index as u32),
                Stage::FaultStatusAccepted => ("FSR ACCEPTED", "CB FSR VALUE", index as u32),
                Stage::ReceiveRecheckStart => ("RECHECK START", "DOMAIN INDEX", index as u32),
                Stage::AsidBank => ("DOM ASID SCAN", "DOMAIN INDEX", index as u32),
                Stage::RouteSlot => ("DOM ROUTE SLOT", "DOMAIN INDEX", index as u32),
                Stage::Register => ("DOM REG CHECK", "DOMAIN INDEX", index as u32),
                Stage::TableSetup => ("DOM TABLE SETUP", "SETUP STEP", index as u32),
                Stage::TablePage => ("DOM TABLE PAGE", "DOMAIN INDEX", index as u32),
                Stage::Complete => ("DOMAIN READ PASS", "DOMAIN INDEX", index as u32),
            };
            d.replace_line(4, label);
            d.hex(5, detail, value);
            d.flush();
        })
    });
    d.hex(4, "DOMAIN QUIET", u32::from(domain_quiet));
    d.flush();

    d.replace_line(5, "CHECK RMTFS GENERATION");
    d.flush();
    let retained = pixel_linux_rmtfs::retained_generation(GENERATION);
    d.hex(5, "RMTFS GEN", u32::from(retained));
    d.flush();

    let auth_health = preauth_verified && ipa_rx_healthy && domain_quiet && retained;
    d.hex(6, "AUTH HEALTH", u32::from(auth_health));
    d.flush();
    if !auth_health {
        d.replace_line(1, "AUTH HEALTH STOP");
        d.replace_line(13, "PROFILE READ SKIPPED");
        d.hex(24, "PROFILE GATE PASS", 0);
        d.hex(25, "HANDOFF WOULD PASS", 0);
        d.flush();
        return false;
    }

    let Some(factory) = s._linux_rmtfs.as_ref() else {
        d.replace_line(1, "FACTORY UNAVAILABLE");
        d.replace_line(7, "FACTORY SNAPSHOT UNAVAILABLE");
        d.replace_line(13, "PROFILE READ UNAVAILABLE");
        d.hex(24, "PROFILE GATE PASS", 0);
        d.hex(25, "HANDOFF WOULD PASS", 0);
        d.flush();
        return false;
    };

    d.replace_line(7, "FACTORY SNAPSHOT START");
    d.flush();
    let factory_report = factory.runtime_boot_report_with(|field, value| {
        use pixel_linux_rmtfs::RuntimeBootField as Field;
        let (line, label) = match field {
            Field::Phase => (8, "FACTORY PHASE"),
            Field::ServerPresent => (9, "SERVER PRESENT"),
            Field::ServerFailed => (10, "SERVER FAILED"),
            Field::SessionPresent => (11, "SESSION PRESENT"),
            Field::TftpProviderPresent => (12, "TFTP PROVIDER"),
        };
        match value {
            None => {
                d.replace_line(line, match field {
                    Field::Phase => "CHECK FACTORY PHASE",
                    Field::ServerPresent => "CHECK SERVER PRESENT",
                    Field::ServerFailed => "CHECK SERVER FAILED",
                    Field::SessionPresent => "CHECK SESSION PRESENT",
                    Field::TftpProviderPresent => "CHECK TFTP PROVIDER",
                });
                d.flush();
            }
            Some(value) => {
            d.hex(line, label, value);
                d.flush();
            }
        }
    });

    d.replace_line(13, "PROFILE READ START");
    d.flush();
    let profile = factory.runtime_profile_report_with(|field, value| {
        use personal_os_rust_server::qcom_glink_qmi::RuntimeProfileField as Field;
        let (line, label) = match field {
            Field::RemoteStorageRegistered => (15, "REMOTE REG"),
            Field::RemoteStorageReady => (16, "REMOTE READY"),
            Field::ServiceRegistryRegistered => (17, "SERVREG REG"),
            Field::IpaServerRegistered => (18, "IPA SERVER REG"),
            Field::LinuxIpaClientEnabled => (19, "LINUX IPA"),
            Field::TftpPort => (20, "TFTP PORT"),
            Field::TftpServerPrepared => (22, "TFTP PREPARED"),
        };
        match value {
            None => {
                d.replace_line(line, match field {
                    Field::RemoteStorageRegistered => "CHECK REMOTE REG",
                    Field::RemoteStorageReady => "CHECK REMOTE READY",
                    Field::ServiceRegistryRegistered => "CHECK SERVREG REG",
                    Field::IpaServerRegistered => "CHECK IPA SERVER REG",
                    Field::LinuxIpaClientEnabled => "CHECK LINUX IPA",
                    Field::TftpPort => "CHECK TFTP PORT",
                    Field::TftpServerPrepared => "CHECK TFTP PREPARED",
                });
                d.flush();
            }
            Some(value) if field == Field::TftpPort => {
                d.hex(20, "TFTP PRESENT", u32::from(value != UNAVAILABLE));
                d.flush();
                d.hex(21, "TFTP PORT VALUE", value);
                d.flush();
            }
            Some(value) => {
                d.hex(line, label, value);
                d.flush();
            }
        }
    });
    d.replace_line(13, "PROFILE READ RETURNED");
    d.flush();
    d.hex(14, "PROFILE PRESENT", u32::from(profile.is_some()));
    d.flush();

    d.replace_line(23, "CHECK PREBOOT GENERATION");
    d.flush();
    let preboot = pixel_linux_rmtfs::preboot_generation(GENERATION);
    d.hex(23, "PREBOOT GEN", u32::from(preboot));
    d.flush();

    let profile_pass = profile.is_some_and(|p| {
        p.remote_storage_registered
            && !p.remote_storage_ready
            && !p.service_registry_registered
            && !p.ipa_server_registered
            && p.linux_ipa_client_enabled
            && p.tftp_port.is_none()
            && !p.tftp_server_prepared
    });
    let handoff_pass = factory_report.phase == pixel_linux_rmtfs::Phase::Prepared
        && factory_report.server_present
        && factory_report.server_failed == Some(false)
        && factory_report.session_present
        && profile_pass
        && factory_report.tftp_provider_present
        && preboot;
    d.hex(24, "PROFILE PASS", u32::from(profile_pass));
    d.flush();
    d.hex(25, "HANDOFF PASS", u32::from(handoff_pass));
    d.replace_line(
        1,
        if handoff_pass {
            if stop_after_report {
                "GATES PASS; STOP HERE"
            } else {
                "GATES PASS; PAS4 NEXT"
            }
        } else {
            "RUNTIME GATE STOP"
        },
    );
    d.replace_line(
        26,
        if stop_after_report {
            "STOP BEFORE TAKE/PAS4"
        } else if handoff_pass {
            "RUNTIME PARTS START"
        } else {
            "RETAIN OWNERS; NO RETRY"
        },
    );
    d.flush();
    handoff_pass
}

pub(super) fn fault(s: &mut Startup, code: u32) {
    let _ = ERROR.compare_exchange(0, u64::from(code), Ordering::AcqRel, Ordering::Acquire);
    pixel_linux_rmtfs::abort();
    if let Some(r) = s._mpss_auth.as_mut() {
        r.phase = Phase::Failed;
        if r.error == 0 {
            r.error = code;
        }
        if let Some(b) = r.boot.as_mut() {
            b.quarantine();
        }
        if let Some(server) = r.unstarted.as_mut() {
            server.fault();
        }
    }
    // No reset, ownership reclaim, IRQ clear or unvote. All providers stay.
}
pub(super) fn show(d: &Display, s: &Startup) {
    let Some(r) = s._mpss_auth.as_ref() else {
        if started() {
            d.clear();
            d.replace_line(0, "NATIVE MPSS PAS4 START");
            d.replace_line(1, "MPSS OWNER ATTACH STOP");
            d.hex(2, "NATIVE ERROR", ERROR.load(Ordering::Acquire) as u32);
            d.replace_line(4, "EARLIER OWNERS RETAINED");
            d.replace_line(6, "RESTART WHEN DONE");
            d.flush();
        }
        return;
    };
    d.clear();
    d.replace_line(0, "NATIVE MPSS PAS4 START");
    d.replace_line(
        1,
        match r.phase {
            Phase::Preparing => "VERIFY MODEM INPUT",
            Phase::Loading => "STAGE AND AUTHENTICATE",
            Phase::WaitingReady => "WAIT DELIVERED READY",
            Phase::Ready => "MPSS AUTH READY PASS",
            Phase::Delegated => "MPSS READY DELEGATED",
            Phase::Failed => "MPSS AUTH READY STOP",
        },
    );
    d.hex(2, "NATIVE ERROR", r.error);
    d.hex(3, "LOADER STEPS", r.steps);
    if let Some(b) = r.boot.as_ref() {
        let p = b.loader_report();
        d.hex(4, "LOAD PHASE", p.phase as u32);
        d.hex(5, "PAS ATTEMPTS", p.attempted_mask);
        d.hex(6, "PAS RETURNED", p.returned_mask);
        d.hex(7, "TARGET WRITES", p.target_writes as u32);
        d.hex(8, "TARGET VERIFIED", p.target_verified as u32);
        d.hex(9, "LOADER ERROR", p.error.map_or(0, |e| e as u32 + 1));
    }
    d.hex(10, "LOW LEVEL SMC", CALLS.load(Ordering::Relaxed) as u32);
    d.hex(11, "SMC X0", X0.load(Ordering::Relaxed) as u32);
    d.hex(12, "SMC X1", X1.load(Ordering::Relaxed) as u32);
    d.hex(
        13,
        "KERNEL ERROR",
        KERNEL_ERROR.load(Ordering::Relaxed) as u32,
    );
    d.hex(
        14,
        "SMP NOTICES",
        SMP_NOTICES.load(Ordering::Relaxed) as u32,
    );
    d.hex(15, "SMP ACKS", r.acks);
    d.hex(
        16,
        "WDOG NOTICES",
        WDOG_NOTICES.load(Ordering::Relaxed) as u32,
    );
    if let Some(p) = s._domain.as_ref().and_then(|d| d.modem_smp2p_report()) {
        d.hex(17, "SMP ERROR", p.error.map_or(0, |e| e as u32 + 1));
        d.hex(18, "READY SEEN", u32::from(p.ready_seen));
        d.hex(
            19,
            "PEER SLAVE",
            p.peer.and_then(|v| v.slave_kernel).unwrap_or(u32::MAX),
        );
        d.hex(20, "IPA CLOCK REPLIED", u32::from(p.power_notified));
    }
    d.hex(21, "AUTH RECEIPT", u32::from(r.auth.is_some()));
    d.hex(22, "READY IRQ PROOF", u32::from(r.ready_irq));
    d.hex(23, "READY RECEIPT", u32::from(r.ready.is_some()));
    if r.error == 0x30 {
        let (rx_error, rx_badge) = pixel_ipa_receive::diagnostic_snapshot(s);
        let seen = DIAGNOSTIC.seen();
        d.hex(24, "HEALTH FAIL", DIAGNOSTIC.failure() as u32);
        d.hex(25, "RX ERROR", rx_error);
        d.hex(26, "RX BADGE LOW", rx_badge as u32);
        d.hex(27, "IRQ SEEN LOW", seen as u32);
        d.hex(20, "IRQ SEEN HIGH", (seen >> 32) as u32);
    } else {
        d.replace_line(24, "ORIGINAL RMTFS RETAINED");
        d.replace_line(25, "NO GLINK OR WLAN START");
        d.replace_line(26, "POWER HOLDS UNTIL REBOOT");
        d.hex(27, "COLD PEER VERIFIED", u32::from(r.cold_peer_verified));
    }
    d.replace_line(28, "NO FLASH OR RESET RETRY");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
struct Environment<'a> {
    domain: &'a pixel_smmu_install::Probe,
    rx: &'a pixel_ipa_receive::Runtime,
    maps: Views,
    cold_peer_verified: bool,
}
impl Environment<'_> {
    fn check(&self, g: pas::Guard) -> Result<(), pas::Error> {
        if g.generation != GENERATION
            || views() != Ok(self.maps)
            || !self.domain.retained_quiet()
            || self.rx.error().is_some()
            || self.rx.startup_report().phase != sunfish_ipa_linux_receive_start::Phase::Armed
            || !pixel_smmu_install::preboot_faults_clear()
            || !capture_during_call()
            || g.auth_submitted && !self.cold_peer_verified
        {
            return Err(pas::Error::Guard);
        }
        match pixel_linux_rmtfs::lifecycle(g.generation) {
            // auth_submitted in Guard is intent until the actual AuthGate.
            Some(smp::Lifecycle::PreAuth) if PENDING.load(Ordering::Acquire) & SMP == 0 => Ok(()),
            Some(smp::Lifecycle::AuthSubmitted) if g.auth_submitted => Ok(()),
            _ => Err(pas::Error::Guard),
        }
    }
}
struct Monitor;
impl smccc::Transport for Monitor {
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
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        let e = sel4::with_ipc_buffer_mut(|i| i.inner_mut().seL4_ARM_SMC_Call(7, &args, &mut r));
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        KERNEL_ERROR.store(e as u64, Ordering::Relaxed);
        X0.store(r.x0, Ordering::Relaxed);
        X1.store(r.x1, Ordering::Relaxed);
        if e != 0 {
            return Err(Fault::Uncertain);
        }
        Ok([r.x0, r.x1, r.x2, r.x3, r.x4, r.x5, r.x6, r.x7])
    }
}
struct Delay;
impl smccc::Delay for Delay {
    fn wait_ms(&mut self, ms: u32) -> Result<(), Fault> {
        let hz = frequency();
        if !(1_000_000..=1_000_000_000).contains(&hz) || ms != smccc::BUSY_WAIT_MS {
            return Err(Fault::Uncertain);
        }
        let start = counter();
        let ticks = (hz * u64::from(ms) + 999) / 1000;
        for _ in 0..50_000_000 {
            if counter().wrapping_sub(start) >= ticks {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(Fault::Uncertain)
    }
}
struct Retry<'a, 'b> {
    env: &'a Environment<'b>,
    guard: pas::Guard,
}
impl smccc::Lifecycle for Retry<'_, '_> {
    fn revalidate(&mut self) -> Result<(), Fault> {
        self.env.check(self.guard).map_err(|_| Fault::Uncertain)
    }
}
struct Backend<'a> {
    resources: power::Resources<
        'a,
        personal_os_rust_server::sunfish_mpss_linux_power_native::Native<
            'static,
            pixel_mpss_preauth::Gate,
        >,
        pixel_rpmh_bus::Controller,
    >,
    env: Environment<'a>,
    guard: Option<pas::Guard>,
}
impl pas::Backend for Backend<'_> {
    type Lease = personal_os_rust_server::sunfish_scm_linux_power::Lease;
    fn guard(&mut self, g: pas::Guard) -> Result<(), pas::Error> {
        self.env.check(g)?;
        self.guard = Some(g);
        Ok(())
    }
    fn acquire(&mut self) -> Result<Self::Lease, pas::Error> {
        self.resources.acquire().map_err(|_| pas::Error::Acquire)
    }
    fn release(&mut self, l: Self::Lease) -> Result<(), pas::Error> {
        self.resources.release(l).map_err(|_| pas::Error::Release)
    }
    fn invoke(&mut self, r: &pas::Request) -> Result<[u64; 8], pas::Error> {
        let life = pixel_linux_rmtfs::lifecycle(GENERATION).ok_or(pas::Error::Guard)?;
        if r.kind() == pas::Kind::AuthAndReset {
            if life != smp::Lifecycle::AuthSubmitted {
                return Err(pas::Error::Guard);
            }
        } else if life != smp::Lifecycle::PreAuth {
            return Err(pas::Error::Guard);
        }
        let g = self.guard.ok_or(pas::Error::Guard)?;
        // Same pinned Linux SMCCC interrupted/busy mechanics as assignment;
        // the outer loader supplied one closed PAS4 request and holds the
        // actual modem/SCM lease across all bounded monitor continuations.
        smccc::invoke_checked(
            &mut Monitor,
            &mut Delay,
            &mut Retry {
                env: &self.env,
                guard: g,
            },
            *r.words(),
        )
        .map_err(|_| pas::Error::Transport)
    }
}
fn service(s: &mut Startup, display: Option<&Display>) -> Result<(), u32> {
    show_runtime_checkpoint(display, s, "RX PENDING START", None);
    pixel_ipa_receive::poll_pending(s);
    show_runtime_checkpoint(display, s, "RX PENDING DONE", None);
    if !healthy_with_progress(s, display) {
        return Err(0x30);
    }
    let pending = PENDING.load(Ordering::Acquire);
    if pending & WDOG != 0 {
        return Err(0x31);
    }
    let phase = pixel_linux_rmtfs::lifecycle(GENERATION).ok_or(0x32u32)?;
    if phase == smp::Lifecycle::PreAuth {
        if pending & SMP != 0 {
            return Err(0x33);
        }
        // The controller's actual cold poll rejects any existing peer item.
        // Repeat that locked directory check immediately before AUTH, after
        // the long target staging operation, rather than trusting an old bit.
        let at_auth = s
            ._mpss_auth
            .as_ref()
            .and_then(|r| r.boot.as_ref())
            .is_some_and(|b| b.loader_report().phase == pas::Phase::AuthAndReset);
        if at_auth {
            let p = s
                ._domain
                .as_mut()
                .ok_or(0x34u32)?
                .modem_smp2p_poll()
                .map_err(|_| 0x35u32)?;
            if p != smp::Poll::Waiting {
                return Err(0x39);
            }
            s._mpss_auth.as_mut().ok_or(0x37u32)?.cold_peer_verified = true;
        }
        return Ok(());
    }
    let p = s
        ._domain
        .as_mut()
        .ok_or(0x34u32)?
        .modem_smp2p_poll()
        .map_err(|_| 0x35u32)?;
    if irq_policy::ack_sample(pending, p) {
        let report = s
            ._domain
            .as_ref()
            .unwrap()
            .modem_smp2p_report()
            .ok_or(0x36u32)?;
        let r = s._mpss_auth.as_mut().ok_or(0x37u32)?;
        if irq_policy::ready_sample(&report) {
            r.ready_irq = true;
        }
        Channel::new(SMP_CHANNEL).irq_ack().map_err(|_| 0x38u32)?;
        r.acks = r.acks.saturating_add(1);
        PENDING.fetch_and(!SMP, Ordering::AcqRel);
    }
    Ok(())
}
pub(super) fn notified(s: &mut Startup, channels: ChannelSet) {
    let mut b = 0u64;
    for channel in channels.iter() {
        b |= 1u64 << channel.index();
    }
    let rest = capture(b);
    if rest != 0 {
        pixel_ipa_receive::delivered_badge(s, rest);
    }
    if let Err(e) = service(s, None) {
        fault(s, e);
    }
    if let Some(p) = s._irq_registers {
        show(&unsafe { Display::new(p.pixels) }, s);
    }
}
pub(super) fn start(d: &Display, s: &mut Startup) {
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    ENTRY_ATTEMPTED.store(1, Ordering::Release);
    start_runtime(d, s);
}

fn start_runtime(d: &Display, s: &mut Startup) {
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    if !show_gate_diagnostic(
        d,
        s,
        cfg!(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic"),
    ) {
        return;
    }
    #[cfg(not(feature = "sunfish-native-startup-mpss-runtime"))]
    if !pixel_mpss_preauth::verified() || !healthy(s) {
        return;
    }

    // Keep the diagnostic on the ordinary runtime entry/call path, but
    // compile out every take/PAS4 action from that image.
    #[cfg(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic")]
    return;

    #[cfg(not(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic"))]
    start_runtime_after_gate(d, s);
}

#[cfg(not(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic"))]
#[inline(never)]
fn start_runtime_after_gate(d: &Display, s: &mut Startup) {
    if STARTED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        fault(s, 1);
        show(d, s);
        return;
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    d.replace_line(1, "RUNTIME PARTS START");
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    d.flush();
    #[cfg(all(feature = "sunfish-native-startup-mpss-runtime", not(feature = "sunfish-tftp-wlan-mux")))]
    let parts = s
        ._linux_rmtfs
        .as_mut()
        .unwrap()
        .take_runtime_boot_parts()
        .map(|(server, session)| (server, session, None));
    #[cfg(all(feature = "sunfish-native-startup-mpss-runtime", feature = "sunfish-tftp-wlan-mux"))]
    let parts = s._linux_rmtfs.as_mut().unwrap().take_runtime_wlan_boot_parts()
        .map(|(server, session, tftp)| (server, session, Some(tftp)));
    #[cfg(not(feature = "sunfish-native-startup-mpss-runtime"))]
    let parts = s
        ._linux_rmtfs
        .as_mut()
        .unwrap()
        .take_boot_parts()
        .map(|(server, session, tftp)| (server, session, Some(tftp)));
    let Ok((server, session, tftp)) = parts else {
        fault(s, 2);
        show(d, s);
        return;
    };
    s._mpss_auth = Some(Runtime {
        boot: None,
        unstarted: Some(server),
        _unloaded: None,
        _session: Some(ManuallyDrop::new(session)),
        _tftp: tftp.map(ManuallyDrop::new),
        auth: None,
        ready: None,
        phase: Phase::Preparing,
        error: 0,
        steps: 0,
        acks: 0,
        ready_irq: false,
        cold_peer_verified: false,
        hz: frequency(),
        since: 0,
    });
    show(d, s);
    let setup = (|| {
        let maps = views().map_err(|_| 3u32)?;
        let image = pas::Image::private_reference(MODEM).map_err(|_| 4u32)?;
        // Actual CapDL validator must prove these three private metadata
        // pages and the exclusive reserved 8b000000/09800000 target, uncached
        // RW/NX, without aliases/TZMem registration. No ownership comes from
        // the numeric checks; the image/cold-boot contract is independent.
        let memory = unsafe {
            Window::new(
                maps.metadata,
                [
                    maps.metadata_pa,
                    maps.metadata_pa + 4096,
                    maps.metadata_pa + 8192,
                ],
                maps.target,
                pas::BASE,
                pas::BYTES,
            )
        }
        .map_err(|_| 5u32)?;
        let loader = match pas::Loader::new(GENERATION, image, memory) {
            Ok(l) => l,
            Err((i, m, _)) => {
                s._mpss_auth.as_mut().unwrap()._unloaded = Some(ManuallyDrop::new((i, m)));
                return Err(6);
            }
        };
        let hz = s._mpss_auth.as_ref().unwrap().hz;
        if !(1_000_000..=1_000_000_000).contains(&hz) {
            return Err(7);
        }
        let r = s._mpss_auth.as_mut().unwrap();
        r.boot = Some(boot::Boot::new(
            GENERATION,
            loader,
            r.unstarted.take().unwrap(),
        ));
        r.phase = Phase::Loading;
        // Bind/unmask only the source-backed shared SMP2P and watchdog IRQs.
        Channel::new(SMP_CHANNEL).irq_ack().map_err(|_| 8u32)?;
        Channel::new(WDOG_CHANNEL).irq_ack().map_err(|_| 9u32)?;
        Ok(maps)
    })();
    let maps = match setup {
        Ok(m) => m,
        Err(e) => {
            fault(s, e);
            show(d, s);
            return;
        }
    };
    show(d, s);
    for _ in 0..(pas::BYTES / pas::CHUNK * 4 + 1024) {
        let detailed = s._mpss_auth.as_ref().unwrap().steps < 16;
        if detailed {
            show_loader_checkpoint(
                d,
                s._mpss_auth.as_ref().unwrap(),
                "SERVICE START",
                None,
            );
        }
        if let Err(e) = service(s, detailed.then_some(d)) {
            fault(s, e);
            break;
        }
        if detailed {
            show_loader_checkpoint(
                d,
                s._mpss_auth.as_ref().unwrap(),
                "SERVICE RETURNED",
                None,
            );
            show_loader_checkpoint(
                d,
                s._mpss_auth.as_ref().unwrap(),
                "POWER RESOURCE",
                None,
            );
        }
        let step = (|| {
            let resources = power::Resources::new(
                s._modem_power.as_mut().unwrap(),
                &s._scm_hold,
                s._firmware_rsc.as_mut().unwrap(),
                65536,
            )
            .map_err(|_| pas::Error::Acquire)?;
            if detailed {
                show_loader_checkpoint(
                    d,
                    s._mpss_auth.as_ref().unwrap(),
                    "RESOURCES READY",
                    None,
                );
            }
            let env = Environment {
                domain: s._domain.as_ref().unwrap(),
                rx: s._receive_runtime.as_ref().unwrap(),
                maps,
                cold_peer_verified: s._mpss_auth.as_ref().unwrap().cold_peer_verified,
            };
            let mut b = Backend {
                resources,
                env,
                guard: None,
            };
            let runtime = s._mpss_auth.as_mut().unwrap();
            runtime.steps = runtime.steps.saturating_add(1);
            if detailed {
                show_loader_checkpoint(d, runtime, "LOADER STEP ENTER", None);
            }
            let result = runtime
                .boot
                .as_mut()
                .unwrap()
                .step(&mut b)
                .map_err(|_| pas::Error::Stage);
            if detailed {
                show_loader_checkpoint(d, runtime, "LOADER STEP RETURNED", None);
            }
            result
        })();
        match step {
            Ok(pas::Phase::AuthReturned) => {
                let r = s._mpss_auth.as_mut().unwrap();
                match r.boot.as_mut().unwrap().take_auth_returned() {
                    Ok(a) => {
                        r.auth = Some(ManuallyDrop::new(a));
                        if pixel_linux_rmtfs::auth_returned(r.auth.as_ref().unwrap()).is_err() {
                            fault(s, 0x40);
                            break;
                        }
                        let r = s._mpss_auth.as_mut().unwrap();
                        r.phase = Phase::WaitingReady;
                        r.since = counter();
                    }
                    Err(_) => fault(s, 0x41),
                }
                break;
            }
            Ok(_) => {}
            Err(_) => {
                fault(s, 0x42);
                break;
            }
        }
        // Keep progress updates at approximately the same target-byte cadence
        // when the opt-in work budget groups more bytes in each loader step.
        if s._mpss_auth.as_ref().unwrap().steps % (1024 / (pas::TARGET_CHUNK / pas::CHUNK)) as u32 == 0 {
            let r = s._mpss_auth.as_ref().unwrap();
            let p = r.boot.as_ref().unwrap().loader_report();
            d.hex(3, "LOADER STEPS", r.steps);
            d.hex(4, "LOAD PHASE", p.phase as u32);
            d.hex(7, "TARGET WRITES", p.target_writes as u32);
            d.hex(8, "TARGET VERIFIED", p.target_verified as u32);
            d.flush();
        }
    }
    if s._mpss_auth.as_ref().unwrap().phase == Phase::Loading {
        fault(s, 0x43);
    }
    if s._mpss_auth.as_ref().unwrap().phase != Phase::WaitingReady {
        show(d, s);
        return;
    }
    if pixel_mpss_preauth::verify_retained(s).is_err() {
        fault(s, 0x44);
        show(d, s);
        return;
    }
    show(d, s);
    for _ in 0..100_000_000u32 {
        if let Err(e) = service(s, None) {
            fault(s, e);
            break;
        }
        let r = s._mpss_auth.as_ref().unwrap();
        if counter().wrapping_sub(r.since) >= r.hz * 5 {
            fault(s, 0x45);
            break;
        }
        if r.ready_irq {
            let ready = s._domain.as_mut().unwrap().take_modem_ready();
            match ready {
                Ok(ready) => {
                    let r = s._mpss_auth.as_mut().unwrap();
                    r.ready = Some(ManuallyDrop::new(ready));
                    if pixel_linux_rmtfs::running(
                        r.auth.as_ref().unwrap(),
                        r.ready.as_ref().unwrap(),
                    )
                    .is_err()
                    {
                        fault(s, 0x46);
                        break;
                    }
                    if pixel_mpss_preauth::verify_retained(s).is_err() {
                        fault(s, 0x47);
                        break;
                    }
                    s._mpss_auth.as_mut().unwrap().phase = Phase::Ready;
                    break;
                }
                Err(_) => {
                    fault(s, 0x48);
                    break;
                }
            }
        }
        core::hint::spin_loop();
    }
    if s._mpss_auth.as_ref().unwrap().phase == Phase::WaitingReady {
        fault(s, 0x49);
    }
    show(d, s);
}
