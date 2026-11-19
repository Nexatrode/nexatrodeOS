#![no_std]
#![no_main]
//! Native boot-input binding and Linux-reference IPA pre-firmware stages.
#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
use core::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "sunfish-native-startup-ipa-command")]
mod pixel_ipa_command;
#[cfg(feature = "sunfish-native-startup-ipa-firmware")]
mod pixel_ipa_firmware;
#[cfg(feature = "sunfish-native-startup-ipa-gsi")]
mod pixel_ipa_gsi;
#[cfg(feature = "sunfish-native-startup-ipa-receive")]
mod pixel_ipa_receive;
#[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
mod pixel_ipa_receive_memory;
#[cfg(feature = "sunfish-native-startup-ipa-tables")]
mod pixel_ipa_tables;
#[cfg(feature = "sunfish-native-startup-ipa-transfer")]
mod pixel_ipa_transfer;
#[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
mod pixel_linux_rmtfs;
#[cfg(feature = "sunfish-native-startup-mpss-auth")]
mod pixel_mpss_auth;
#[cfg(feature = "sunfish-native-startup-mpss-preauth")]
mod pixel_mpss_preauth;
#[cfg(feature = "sunfish-native-startup-mpss-runtime")]
mod pixel_mpss_runtime;
#[cfg(feature = "sunfish-native-startup-mpss-smem")]
mod pixel_mpss_smem;
#[cfg(feature = "sunfish-native-startup-ipa-firmware")]
mod pixel_pas_smc;
#[allow(dead_code)]
mod pixel_probe_display;
mod pixel_rpmh_bus;
#[cfg(feature = "sunfish-native-startup-ipa-domain")]
mod pixel_smmu_install;
mod private_inputs {
    include!(env!("SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH"));
}

#[cfg(any(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
static PRESCM_DIAGNOSTIC_OWNS_DISPLAY: AtomicBool = AtomicBool::new(false);
// Retain the link symbols required by the shared runtime .system profile.
// This is an inert function pointer: the diagnostic never calls MPSS auth.
#[cfg(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")]
#[used]
static RETAIN_AUTH_PROFILE_LAYOUT: fn(&Display, &mut Startup) = pixel_mpss_auth::start;

#[inline]
fn diagnostic_display_owned() -> bool {
    #[cfg(any(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
    if PRESCM_DIAGNOSTIC_OWNS_DISPLAY.load(Ordering::Acquire) {
        return true;
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    {
        return pixel_mpss_auth::diagnostics_owned();
    }
    #[cfg(not(feature = "sunfish-native-startup-mpss-runtime"))]
    {
        false
    }
}

use personal_os_rust_server::{
    qcom_bcm::Metadata,
    qcom_cmd_db::Database,
    qcom_modem_ram_storage::RamShadow,
    qcom_modem_storage::Storage,
    qcom_qrtr_tftp::Server,
    qcom_rpmh_read::BcmTargetName,
    sunfish_ipa_linux_power::{COUNT, PowerOnce, Report},
};
use pixel_probe_display::Display;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, memory_region_symbol, protection_domain,
};

struct Startup {
    #[cfg(feature = "sunfish-native-startup-mpss-auth")]
    _mpss_auth: Option<pixel_mpss_auth::Runtime>,
    #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
    _mpss_runtime: Option<pixel_mpss_runtime::RuntimeDomain>,
    #[cfg(feature = "sunfish-native-startup-mpss-preauth")]
    _modem_power: Option<pixel_mpss_preauth::Power>,
    #[cfg(feature = "sunfish-native-startup-mpss-smem")]
    _modem_smem_prepared: Option<personal_os_rust_server::sunfish_mpss_linux_smem::Prepared>,
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    _linux_rmtfs: Option<pixel_linux_rmtfs::Factory>,
    // Retain providers, the one-shot coordinator and all applied floors.
    _storage: Option<Storage<RamShadow<'static>>>,
    _tftp: Option<Server<'static>>,
    _power: PowerOnce,
    _report: Option<Report>,
    #[cfg(feature = "sunfish-native-startup-ipa-config")]
    _config: personal_os_rust_server::sunfish_ipa_linux_config::ConfigureOnce,
    #[cfg(feature = "sunfish-native-startup-ipa-config")]
    _config_report: Option<personal_os_rust_server::sunfish_ipa_linux_config::Report>,
    #[cfg(feature = "sunfish-native-startup-ipa-domain")]
    _domain: Option<pixel_smmu_install::Probe>,
    #[cfg(feature = "sunfish-native-startup-ipa-memory")]
    _memory: personal_os_rust_server::sunfish_ipa_linux_memory::CanariesOnce,
    #[cfg(feature = "sunfish-native-startup-ipa-memory")]
    _memory_report: Option<personal_os_rust_server::sunfish_ipa_linux_memory::Report>,
    #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
    _buffers: Option<
        personal_os_rust_server::sunfish_ipa_linux_buffers::Pool<
            personal_os_rust_server::sunfish_ipa_linux_buffers_native::NativeRam<'static>,
        >,
    >,
    #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
    _buffer_report: Option<personal_os_rust_server::sunfish_ipa_linux_buffers::Report>,
    #[cfg(feature = "sunfish-native-startup-ipa-resources")]
    _resources: personal_os_rust_server::sunfish_ipa_linux_resources::ConfigureOnce,
    #[cfg(feature = "sunfish-native-startup-ipa-resources")]
    _resource_report: Option<personal_os_rust_server::sunfish_ipa_linux_resources::Report>,
    #[cfg(feature = "sunfish-native-startup-ipa-irq")]
    _irq: personal_os_rust_server::sunfish_ipa_linux_irq::ArmOnce,
    #[cfg(feature = "sunfish-native-startup-ipa-irq")]
    _irq_report: Option<personal_os_rust_server::sunfish_ipa_linux_irq::Report>,
    #[cfg(feature = "sunfish-native-startup-ipa-irq")]
    _irq_notice: Option<personal_os_rust_server::sunfish_ipa_linux_irq::Notice>,
    #[cfg(feature = "sunfish-native-startup-ipa-irq")]
    _irq_notifications: u32,
    #[cfg(feature = "sunfish-native-startup-ipa-irq")]
    _irq_registers: Option<IrqRegisters>,
    #[cfg(feature = "sunfish-native-startup-ipa-irq")]
    _shared_pointer: Option<*const u8>,
    #[cfg(feature = "sunfish-native-startup-ipa-uc")]
    _uc: Option<personal_os_rust_server::sunfish_ipa_linux_uc::Runtime>,
    #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
    _firmware: Option<pixel_ipa_firmware::NativeLoader>,
    #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
    _firmware_rsc: Option<pixel_rpmh_bus::Controller>,
    #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
    _scm_hold: personal_os_rust_server::sunfish_scm_linux_power::HoldOnce,
    #[cfg(feature = "sunfish-native-startup-ipa-gsi")]
    _gsi: Option<personal_os_rust_server::sunfish_ipa_linux_gsi::SetupOnce>,
    #[cfg(feature = "sunfish-native-startup-ipa-gsi")]
    _gsi_stats: pixel_ipa_gsi::Stats,
    #[cfg(feature = "sunfish-native-startup-ipa-command")]
    _endpoint: Option<personal_os_rust_server::sunfish_ipa_linux_endpoint::ConfigureOnce>,
    #[cfg(feature = "sunfish-native-startup-ipa-command")]
    _command: Option<personal_os_rust_server::sunfish_ipa_linux_command::StartOnce>,
    #[cfg(feature = "sunfish-native-startup-ipa-command")]
    _command_stats: pixel_ipa_command::Stats,
    #[cfg(feature = "sunfish-native-startup-ipa-transfer")]
    _transfer: Option<personal_os_rust_server::sunfish_ipa_linux_transfer::TransferOnce>,
    #[cfg(feature = "sunfish-native-startup-ipa-transfer")]
    _transfer_stats: pixel_ipa_transfer::Stats,
    #[cfg(feature = "sunfish-native-startup-ipa-tables")]
    _tables: Option<personal_os_rust_server::sunfish_ipa_linux_table_transfer::TableOnce>,
    #[cfg(feature = "sunfish-native-startup-ipa-tables")]
    _table_stats: pixel_ipa_tables::Stats,
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    _receive_data: Option<
        personal_os_rust_server::sunfish_ipa_linux_receive_pool::DataPool<
            personal_os_rust_server::sunfish_ipa_linux_receive_native::NativeReceiveRam<'static>,
        >,
    >,
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    _receive_prepare: Option<personal_os_rust_server::sunfish_ipa_linux_receive_pool::Report>,
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    _receive_guards: Option<personal_os_rust_server::sunfish_ipa_linux_receive_pool::GuardReport>,
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    _receive_physical: Option<u64>,
    #[cfg(feature = "sunfish-native-startup-ipa-receive")]
    _receive_once: Option<pixel_ipa_receive::Once>,
    #[cfg(feature = "sunfish-native-startup-ipa-receive")]
    _receive_runtime: Option<pixel_ipa_receive::Runtime>,
    #[cfg(feature = "sunfish-native-startup-ipa-receive")]
    _receive_stats: pixel_ipa_receive::Stats,
    #[cfg(feature = "sunfish-native-startup-ipa-receive")]
    _receive_sink: pixel_ipa_receive::Sink,
}

fn word(base: *const u32, offset: usize) -> u32 {
    let value = unsafe { base.add(offset / 4).read_volatile() };
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    u32::from_le(value)
}
fn screen(d: &Display, status: &str) {
    d.clear();
    d.replace_line(0, "NATIVE IPA STARTUP");
    d.replace_line(1, status);
    d.replace_line(26, "RAM EFS ONLY");
    d.replace_line(27, "NO PAS OR MODEM START");
    d.replace_line(28, "NO DMA OR FLASH");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
fn metadata(dict: *const u32, base: *const u32) -> Result<[Metadata; COUNT], u32> {
    metadata_named(
        dict,
        base,
        BcmTargetName::IPA_PATHS.map(|name| name.as_str()),
    )
}
fn metadata_named<const N: usize>(
    dict: *const u32,
    base: *const u32,
    names: [&str; N],
) -> Result<[Metadata; N], u32> {
    let expected = (0x85f20000, 0x1078);
    let pointer = || (word(dict, 0xc), word(dict, 0x10));
    if pointer() != expected || pointer() != expected {
        return Err(1);
    }
    let mut bytes = [0; 0x2000];
    for i in 0..expected.1 as usize / 4 {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&word(base, i * 4).to_le_bytes());
    }
    for i in 0..expected.1 as usize / 4 {
        if bytes[i * 4..i * 4 + 4] != word(base, i * 4).to_le_bytes() {
            return Err(2);
        }
    }
    if pointer() != expected {
        return Err(3);
    }
    let db = Database::parse(&bytes[..expected.1 as usize]).map_err(|_| 4u32)?;
    let mut result = [None; N];
    for (i, name) in names.into_iter().enumerate() {
        let resource = db.lookup(name).map_err(|_| 0x10 + i as u32)?;
        if resource.table_slave != 5 {
            return Err(0x20 + i as u32);
        }
        result[i] = Some(Metadata::from_resource(&resource).map_err(|_| 0x30 + i as u32)?);
    }
    Ok(result.map(Option::unwrap))
}

#[protection_domain]
fn init() -> Startup {
    let d = unsafe { Display::new(memory_region_symbol!(splash_fb:*mut u32).as_ptr()) };
    let mut state = Startup {
        #[cfg(feature = "sunfish-native-startup-mpss-auth")]
        _mpss_auth: None,
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        _mpss_runtime: None,
        #[cfg(feature = "sunfish-native-startup-mpss-preauth")]
        _modem_power: None,
        #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
        _linux_rmtfs: None,
        #[cfg(feature = "sunfish-native-startup-mpss-smem")]
        _modem_smem_prepared: None,
        _storage: None,
        _tftp: None,
        _power: PowerOnce::new(),
        _report: None,
        #[cfg(feature = "sunfish-native-startup-ipa-config")]
        _config: personal_os_rust_server::sunfish_ipa_linux_config::ConfigureOnce::new(),
        #[cfg(feature = "sunfish-native-startup-ipa-config")]
        _config_report: None,
        #[cfg(feature = "sunfish-native-startup-ipa-domain")]
        _domain: None,
        #[cfg(feature = "sunfish-native-startup-ipa-memory")]
        _memory: personal_os_rust_server::sunfish_ipa_linux_memory::CanariesOnce::new(),
        #[cfg(feature = "sunfish-native-startup-ipa-memory")]
        _memory_report: None,
        #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
        _buffers: None,
        #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
        _buffer_report: None,
        #[cfg(feature = "sunfish-native-startup-ipa-resources")]
        _resources: personal_os_rust_server::sunfish_ipa_linux_resources::ConfigureOnce::new(),
        #[cfg(feature = "sunfish-native-startup-ipa-resources")]
        _resource_report: None,
        #[cfg(feature = "sunfish-native-startup-ipa-irq")]
        _irq: personal_os_rust_server::sunfish_ipa_linux_irq::ArmOnce::new(),
        #[cfg(feature = "sunfish-native-startup-ipa-irq")]
        _irq_report: None,
        #[cfg(feature = "sunfish-native-startup-ipa-irq")]
        _irq_notice: None,
        #[cfg(feature = "sunfish-native-startup-ipa-irq")]
        _irq_notifications: 0,
        #[cfg(feature = "sunfish-native-startup-ipa-irq")]
        _irq_registers: None,
        #[cfg(feature = "sunfish-native-startup-ipa-irq")]
        _shared_pointer: None,
        #[cfg(feature = "sunfish-native-startup-ipa-uc")]
        _uc: None,
        #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
        _firmware: None,
        #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
        _firmware_rsc: None,
        #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
        _scm_hold: personal_os_rust_server::sunfish_scm_linux_power::HoldOnce::new(),
        #[cfg(feature = "sunfish-native-startup-ipa-gsi")]
        _gsi: None,
        #[cfg(feature = "sunfish-native-startup-ipa-gsi")]
        _gsi_stats: pixel_ipa_gsi::Stats::default(),
        #[cfg(feature = "sunfish-native-startup-ipa-command")]
        _endpoint: None,
        #[cfg(feature = "sunfish-native-startup-ipa-command")]
        _command: None,
        #[cfg(feature = "sunfish-native-startup-ipa-command")]
        _command_stats: pixel_ipa_command::Stats::default(),
        #[cfg(feature = "sunfish-native-startup-ipa-transfer")]
        _transfer: None,
        #[cfg(feature = "sunfish-native-startup-ipa-transfer")]
        _transfer_stats: pixel_ipa_transfer::Stats::default(),
        #[cfg(feature = "sunfish-native-startup-ipa-tables")]
        _tables: None,
        #[cfg(feature = "sunfish-native-startup-ipa-tables")]
        _table_stats: pixel_ipa_tables::Stats::default(),
        #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
        _receive_data: None,
        #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
        _receive_prepare: None,
        #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
        _receive_guards: None,
        #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
        _receive_physical: None,
        #[cfg(feature = "sunfish-native-startup-ipa-receive")]
        _receive_once: None,
        #[cfg(feature = "sunfish-native-startup-ipa-receive")]
        _receive_runtime: None,
        #[cfg(feature = "sunfish-native-startup-ipa-receive")]
        _receive_stats: pixel_ipa_receive::Stats::default(),
        #[cfg(feature = "sunfish-native-startup-ipa-receive")]
        _receive_sink: pixel_ipa_receive::Sink::default(),
    };
    let rsc = memory_region_symbol!(apps_rsc:*mut u32).as_ptr();
    let dict = memory_region_symbol!(cmd_db_dictionary:*mut u32)
        .as_ptr()
        .cast_const();
    let cmd_db = memory_region_symbol!(cmd_db_snapshot:*mut u32)
        .as_ptr()
        .cast_const();
    let ipa = memory_region_symbol!(ipa_regs:*mut u32)
        .as_ptr()
        .cast_const();
    let gsi = memory_region_symbol!(gsi_regs:*mut u32)
        .as_ptr()
        .cast_const();
    screen(&d, "VERIFY PRIVATE INPUTS");
    let pointers = [
        memory_region_symbol!(efs_shadow1:*mut u8).as_ptr(),
        memory_region_symbol!(efs_shadow2:*mut u8).as_ptr(),
        memory_region_symbol!(efs_shadow3:*mut u8).as_ptr(),
        memory_region_symbol!(efs_shadow4:*mut u8).as_ptr(),
    ];
    if pointers.map(|p| p as usize) != [0x74001000, 0x74203000, 0x74405000, 0x74607000]
        || rsc as usize != 0x40000000
        || dict as usize != 0x41000000
        || cmd_db as usize != 0x41001000
        || ipa as usize != 0x44000000
        || gsi as usize != 0x44100000
    {
        screen(&d, "MAPPING STOP");
        return state;
    }
    // SAFETY: actual CapDL audit proves four unique cached RW/NX frame runs,
    // with guard holes and no aliases; this one PD calls init exactly once.
    // Only CPU private RAM is exposed here, never modem-owned SCM buffers.
    let shadows = core::array::from_fn(|i| unsafe {
        core::slice::from_raw_parts_mut(pointers[i], if i == 3 { 0x20000 } else { 0x200000 })
    });
    // The private artifact is selected only after host serial/product checks.
    // It does not detect hardware identity; that check is repeated before boot.
    let prepared = match private_inputs::inputs(1).prepare(private_inputs::DEVICE_BINDING, shadows)
    {
        Ok(p) => p,
        Err(_) => {
            screen(&d, "PRIVATE INPUT STOP");
            return state;
        }
    };
    state._storage = Some(prepared.storage);
    #[cfg(feature = "sunfish-tftp-wlan-mux")]
    let tftp_server = Server::new_bounded(1, 0, 0x5000, 0x5100, 0x51ff, &[], prepared.wlan);
    #[cfg(not(feature = "sunfish-tftp-wlan-mux"))]
    let tftp_server = Server::new(1, 0, 0x5000, 0x5100, &[], prepared.wlan);
    state._tftp = match tftp_server {
        Ok(s) => Some(s),
        Err(_) => {
            screen(&d, "TFTP INPUT STOP");
            return state;
        }
    };
    screen(&d, "RESOLVE IPA CMD DB");
    let metadata = match metadata(dict, cmd_db) {
        Ok(m) => m,
        Err(code) => {
            screen(&d, "CMD DB STOP");
            d.hex(3, "ERROR", code);
            d.flush();
            return state;
        }
    };
    screen(&d, "READ IPA BASELINE");
    // SAFETY: this cold standalone image exclusively owns Apps DRV2; actual
    // CapDL grants RW only on page 0 and RO on the following two pages. The
    // controller and every partial floor are retained until physical reboot.
    let mut controller = unsafe { pixel_rpmh_bus::Controller::new(rsc) };
    let report = state._power.enable(
        &mut controller,
        metadata,
        65536,
        || {
            for _ in 0..256 {
                core::hint::spin_loop();
            }
        },
        |i, name| {
            d.replace_line(1, "APPLY IPA FLOOR");
            d.replace_line(2, name.as_str());
            d.hex(3, "FLOOR INDEX", i as u32);
            d.flush();
        },
    );
    state._report = Some(report);
    screen(
        &d,
        if report.error.is_none() {
            "IPA POWER FLOORS HELD"
        } else {
            "IPA POWER STOP"
        },
    );
    d.hex(3, "SUBMITTED", report.submitted as u32);
    d.hex(4, "COMPLETED", report.completed as u32);
    if let Some(error) = report.error {
        d.hex(5, "POWER ERROR", error as u32 + 1);
        if let Some(name) = report.failed_resource {
            d.replace_line(6, name.as_str());
        }
        d.flush();
        return state;
    }
    for (i, value) in report.observed.into_iter().enumerate() {
        d.hex(
            7 + i,
            report.resources[i].map_or("NOT APPLIED", |n| n.as_str()),
            value,
        );
    }
    d.flush();
    // Correct Linux apertures: IPA=0x01e40000, GSI=0x01e04000. Unlike the
    // downstream profile, GSI is BELOW IPA and uses v4.0 table EE0 offsets.
    let mut samples = [[0; 4]; 2];
    for (pass, values) in samples.iter_mut().enumerate() {
        for (i, (base, off)) in [(ipa, 0x210), (ipa, 0x54), (gsi, 0x1f000), (gsi, 0x1f040)]
            .into_iter()
            .enumerate()
        {
            d.replace_line(1, "READ IPA GSI STATE");
            d.hex(19, "READ PASS", pass as u32 + 1);
            d.hex(20, "READ OFFSET", off as u32);
            d.flush();
            values[i] = word(base, off);
        }
    }
    d.replace_line(
        1,
        if samples[0] != samples[1] {
            "PRESTART STATE CHANGED"
        } else if samples[1][0] == 0 || samples[1][0] == u32::MAX || samples[1][1] & 0xffff < 0x400
        {
            "PRESTART STATE INVALID"
        } else {
            "PRESTART SNAPSHOT PASS"
        },
    );
    for (i, label) in ["IPA FLAVOR", "IPA SHMEM", "GSI STATUS", "GSI PARAM2"]
        .into_iter()
        .enumerate()
    {
        d.hex(21 + i, label, samples[1][i]);
    }
    d.replace_line(25, "INPUTS AND FLOORS ONLY");
    d.flush();
    #[cfg(feature = "sunfish-native-startup-ipa-config")]
    {
        let configured = configure_hardware(&d, &mut state, ipa, gsi, report);
        #[cfg(feature = "sunfish-native-startup-ipa-domain")]
        if configured {
            // SAFETY: the combined system's actual CapDL audit proves the
            // SMMU/table/data/IRQ mappings in addition to the retained private
            // providers and bounded IPA/RSC mappings. No DMA, GSI, PAS or
            // modem/WLAN engine has been started. This is the sole install
            // call; retain its adapter and every partial write until reboot.
            let domain = unsafe { pixel_smmu_install::run(d.pixels_ptr()) };
            let integrated =
                domain.installed() && verify_configuration_retained(&d, &state, ipa, gsi);
            if integrated {
                d.replace_line(0, "NATIVE IPA PRE DMA");
                d.replace_line(1, "INTEGRATED PRE DMA PASS");
                d.hex(10, "POWER COMPLETED", report.completed as u32);
                d.hex(
                    11,
                    "HW COMPLETED",
                    state._config_report.as_ref().unwrap().completed as u32,
                );
                d.flush();
            }
            state._domain = Some(domain);
            #[cfg(feature = "sunfish-native-startup-ipa-memory")]
            if integrated {
                configure_local_memory(&d, &mut state, ipa, gsi);
                #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
                if state._memory_report.is_some_and(|r| {
                    r.error.is_none()
                        && r.completed
                            == personal_os_rust_server::sunfish_ipa_linux_memory::COMPLETE
                        && r.verified_words == 25
                }) {
                    prepare_buffers(&d, &mut state, ipa, gsi);
                    #[cfg(feature = "sunfish-native-startup-ipa-resources")]
                    if state._buffer_report.is_some_and(|r| {
                        r.error.is_none()
                            && r.initialized
                                == personal_os_rust_server::sunfish_ipa_linux_buffers::COMPLETE
                            && r.verified
                                == personal_os_rust_server::sunfish_ipa_linux_buffers::COMPLETE
                            && r.checked_bytes == 0xf000
                    }) {
                        configure_resources(&d, &mut state, ipa, gsi);
                        #[cfg(feature = "sunfish-native-startup-ipa-irq")]
                        if state._resource_report.is_some_and(|r| {
                            r.error.is_none()
                                && r.completed == personal_os_rust_server::sunfish_ipa_linux_resources::COMPLETE
                        }) {
                            configure_irq(&d, &mut state, ipa, gsi);
                            #[cfg(feature = "sunfish-native-startup-ipa-shared")]
                            if state._irq_report.is_some_and(|r| r.error.is_none()) {
                                if let Some(domain)=state._domain.as_ref().filter(|d|d.retained_quiet()) {
                                    domain.show_shared_summary();
                                    #[cfg(feature = "sunfish-native-startup-ipa-uc")]
                                    install_uc_runtime(&d, &mut state);
                                }
                            }
                        }
                    }
                }
            }
        }
        #[cfg(not(feature = "sunfish-native-startup-ipa-domain"))]
        let _ = configured;
    }
    #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
    if state._uc.is_some() {
        pixel_ipa_firmware::start(&d, &mut state, controller, dict, cmd_db);
    }
    #[cfg(feature = "sunfish-native-startup-ipa-gsi")]
    pixel_ipa_gsi::start(&d, &mut state);
    #[cfg(feature = "sunfish-native-startup-ipa-command")]
    pixel_ipa_command::start(&d, &mut state);
    #[cfg(feature = "sunfish-native-startup-ipa-transfer")]
    pixel_ipa_transfer::start(&d, &mut state);
    #[cfg(feature = "sunfish-native-startup-ipa-tables")]
    pixel_ipa_tables::start(&d, &mut state);
    #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
    pixel_ipa_receive_memory::prepare(&d, &mut state);
    #[cfg(feature = "sunfish-native-startup-ipa-receive")]
    pixel_ipa_receive::start(&d, &mut state);
    #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
    prepare_linux_rmtfs(&d, &mut state);
    #[cfg(all(
        feature = "sunfish-native-startup-mpss-smem",
        not(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")
    ))]
    pixel_mpss_smem::prepare(&d, &mut state);
    #[cfg(all(
        feature = "sunfish-native-startup-mpss-preauth",
        not(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")
    ))]
    pixel_mpss_preauth::start(&d, &mut state, dict, cmd_db);
    #[cfg(all(
        feature = "sunfish-native-startup-mpss-auth",
        not(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")
    ))]
    pixel_mpss_auth::start(&d, &mut state);
    #[cfg(all(
        feature = "sunfish-native-startup-mpss-runtime",
        not(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")
    ))]
    if pixel_mpss_auth::runtime_ready(&state) {
        pixel_mpss_runtime::start(&d, &mut state);
    }
    state
}
#[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
fn show_linux_rmtfs(d: &Display, state: &Startup) {
    use pixel_linux_rmtfs::Phase;
    let Some(factory) = state._linux_rmtfs.as_ref() else {
        return;
    };
    d.clear();
    d.replace_line(0, "MODEM HOST RESOURCES");
    d.replace_line(
        1,
        match factory.phase() {
            Phase::New => "MAP AND COLD CONTRACT",
            Phase::Preparing => "ZERO PRIVATE BUFFER",
            Phase::Sharing => "SHARE RMTFS WITH MSS",
            Phase::Prepared => "HOST SERVICES PREPARED",
            Phase::Failed => "HOST RESOURCE STOP",
            #[cfg(feature = "sunfish-native-startup-mpss-auth")]
            Phase::Delegated => "PROVIDERS WITH MPSS OWNER",
        },
    );
    d.hex(3, "FACTORY ERROR", factory.error());
    if let Some(s) = factory.status() {
        d.hex(5, "ZEROED PAGES", s.zeroed_pages);
        d.hex(6, "ASSIGN ATTEMPTS", s.assignments_attempted);
        d.hex(7, "POOL PHASE", s.phase as u32);
    }
    let m = factory.monitor();
    d.hex(9, "MONITOR CALLS", m[0] as u32);
    d.hex(10, "SCM A0", m[1] as u32);
    d.hex(11, "SCM A1", m[2] as u32);
    d.hex(13, "RMTFS PA", 0xfde01000);
    d.hex(14, "RMTFS BYTES", 0x200000);
    let p = factory.providers();
    d.hex(16, "STORAGE RETAINED", u32::from(p[0]));
    d.hex(17, "TFTP RETAINED", u32::from(p[1]));
    d.hex(18, "SESSION PREPARED", u32::from(p[2]));
    d.hex(
        20,
        "IPA RX HEALTHY",
        u32::from(pixel_ipa_receive::healthy(state)),
    );
    d.replace_line(23, "MPSS NOT STARTED");
    d.replace_line(24, "RAM EFS ONLY");
    d.replace_line(25, "COLD BOOT REQUIRED");
    #[cfg(all(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic", not(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic")))]
    d.replace_line(27, "NO RMTFS ASSIGN");
    d.replace_line(28, "NO FLASH");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")]
fn share_checkpoint_label(checkpoint: personal_os_rust_server::sunfish_linux_rmtfs::ShareCheckpoint) -> &'static str {
    use personal_os_rust_server::sunfish_linux_rmtfs::ShareCheckpoint as C;
    match checkpoint {
        C::StorageValidationStart => "STORAGE CHECK",
        C::StorageValidationDone => "STORAGE CHECK OK",
        C::PoolValidationStart => "POOL CHECK",
        C::PoolValidationDone => "POOL CHECK OK",
        C::CallConstructionStart => "CALL BUILD",
        C::CallConstructionDone => "CALL BUILD OK",
        C::ScratchStageStart => "SCRATCH STAGE",
        C::ScratchStageDone => "SCRATCH STAGE OK",
        C::PostStageValidationStart => "POST STAGE CHECK",
        C::PostStageValidationDone => "POST CHECK OK",
        C::StorageFinalValidationStart => "FINAL OWNER CHECK",
        C::StorageFinalValidationDone => "FINAL OWNER OK",
        C::ReadyBeforeScm => "READY BEFORE SCM",
    }
}
#[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
fn prepare_linux_rmtfs(d: &Display, state: &mut Startup) {
    if !pixel_ipa_receive::healthy(state) || state._linux_rmtfs.is_some() {
        return;
    }
    if state._storage.is_none() || state._tftp.is_none() {
        return;
    }
    let storage = state._storage.take().unwrap();
    let tftp = state._tftp.take().unwrap();
    state._linux_rmtfs = Some(pixel_linux_rmtfs::Factory::new(storage, tftp));
    #[cfg(any(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic", feature = "sunfish-native-startup-mpss-runtime-share-trace"))]
    PRESCM_DIAGNOSTIC_OWNS_DISPLAY.store(true, Ordering::Release);
    #[cfg(feature = "sunfish-native-startup-mpss-runtime-share-trace")]
    pixel_linux_rmtfs::set_diagnostic_display(d.pixels_ptr());
    show_linux_rmtfs(d, state);
    // SAFETY: this opt-in composition is bootable only under the frozen cold
    // Sunfish/reference contract. CapDL proves exact exclusive mappings and
    // scratch/SMC authority. All IPA owners stay retained in `state`; no MPSS
    // start/reset capability is present. Host identity/power-off checks remain
    // mandatory external prerequisites; runtime flags do not prove them.
    unsafe {
        state._linux_rmtfs.as_mut().unwrap().begin();
    }
    for step in 0..514 {
        use pixel_linux_rmtfs::Phase;
        if !matches!(
            state._linux_rmtfs.as_ref().unwrap().phase(),
            Phase::Preparing | Phase::Sharing
        ) {
            break;
        }
        #[cfg(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")]
        if state._linux_rmtfs.as_ref().unwrap().phase() == Phase::Sharing {
            break;
        }
        state._linux_rmtfs.as_mut().unwrap().step();
        if step % 64 == 0 || state._linux_rmtfs.as_ref().unwrap().phase() != Phase::Preparing {
            show_linux_rmtfs(d, state);
        }
    }
    #[cfg(feature = "sunfish-native-startup-mpss-runtime-share-trace")]
    PRESCM_DIAGNOSTIC_OWNS_DISPLAY.store(false, Ordering::Release);
    #[cfg(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")]
    {
        #[cfg(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic")]
        {
            if state._linux_rmtfs.as_ref().unwrap().phase() != pixel_linux_rmtfs::Phase::Sharing {
                show_linux_rmtfs(d, state);
                d.replace_line(2, "SHARE NOT READY");
                d.flush();
                return;
            }
            pixel_linux_rmtfs::set_diagnostic_display(d.pixels_ptr());
            d.replace_line(2, "SHARE STEP ENTER");
            d.flush();
            state._linux_rmtfs.as_mut().unwrap().step();
            show_linux_rmtfs(d, state);
            d.replace_line(2, "SHARE STEP RETURN");
            d.hex(21, "KERNEL ERROR", pixel_linux_rmtfs::diagnostic_smc_error());
            d.replace_line(27, "STOP BEFORE PAS4");
            d.flush();
            return;
        }
        #[cfg(not(feature = "sunfish-native-startup-mpss-runtime-share-diagnostic"))]
        {
        let result = state._linux_rmtfs.as_mut().unwrap().preflight_share_with(|checkpoint| {
            d.replace_line(2, share_checkpoint_label(checkpoint));
            d.flush();
        });
        if result.is_err() {
            d.hex(2, "PREFLIGHT ERROR", state._linux_rmtfs.as_ref().unwrap().error());
            d.flush();
        }
        return;
        }
    }
    #[cfg(not(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic"))]
    {
    pixel_ipa_receive::poll_pending(state);
    if !pixel_ipa_receive::healthy(state) {
        state._linux_rmtfs.as_mut().unwrap().fault(0x20);
    }
    show_linux_rmtfs(d, state);
    }
}
#[cfg(feature = "sunfish-native-startup-ipa-irq")]
#[derive(Clone, Copy)]
struct IrqRegisters {
    ipa: *const u32,
    gsi: *const u32,
    shared: *const u8,
    pixels: *mut u32,
}
#[cfg(feature = "sunfish-native-startup-ipa-irq")]
struct IrqIo<'a> {
    registers: IrqRegisters,
    expected: personal_os_rust_server::sunfish_ipa_linux_config::Snapshot,
    domain: pixel_smmu_install::Access<'a>,
    pool: Option<
        &'a personal_os_rust_server::sunfish_ipa_linux_buffers::Pool<
            personal_os_rust_server::sunfish_ipa_linux_buffers_native::NativeRam<'static>,
        >,
    >,
    #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
    firmware: Option<&'a pixel_ipa_firmware::NativeLoader>,
}
#[cfg(feature = "sunfish-native-startup-ipa-firmware")]
impl IrqIo<'_> {
    fn retained_prerequisites(
        &self,
    ) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_irq::Error> {
        use personal_os_rust_server::{
            sunfish_ipa_linux_buffers as buffers, sunfish_ipa_linux_irq as irq,
            sunfish_ipa_linux_resources as resources,
        };
        if !self.domain.retained_quiet() {
            return Err(irq::Error::Domain);
        }
        if self
            .pool
            .and_then(|p| p.buffer(buffers::Kind::Zero))
            .is_none()
        {
            return Err(irq::Error::Buffers);
        }
        for _ in 0..2 {
            for spec in resources::PROFILE {
                if word(self.registers.ipa, spec.offset) != spec.value {
                    return Err(irq::Error::Resources);
                }
            }
        }
        Ok(())
    }
}
#[cfg(feature = "sunfish-native-startup-ipa-uc")]
impl personal_os_rust_server::sunfish_ipa_linux_uc::Io for IrqIo<'_> {
    fn guard(&mut self) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_uc::Error> {
        use personal_os_rust_server::{sunfish_ipa_linux_irq as irq, sunfish_ipa_linux_uc as uc};
        #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
        if let Some(loader) = self.firmware {
            if loader.report().phase
                != personal_os_rust_server::sunfish_ipa_linux_loader::Phase::AuthReturned
                || !loader.pas_report().passed()
            {
                return Err(uc::Error::Guard);
            }
            self.retained_prerequisites()
                .map_err(|_| uc::Error::Guard)?;
            return read_retained_post_auth_snapshot(
                self.registers.ipa,
                self.registers.gsi,
                self.expected,
            )
            .map_err(|_| uc::Error::Guard);
        }
        irq::Io::guard(self, irq::ENABLED).map_err(|_| uc::Error::Guard)
    }
    fn read(
        &mut self,
        register: personal_os_rust_server::sunfish_ipa_linux_irq::Read,
    ) -> Result<u32, personal_os_rust_server::sunfish_ipa_linux_uc::Error> {
        use personal_os_rust_server::{sunfish_ipa_linux_irq as irq, sunfish_ipa_linux_uc as uc};
        irq::Io::read(self, register).map_err(|_| uc::Error::Io)
    }
    fn write(
        &mut self,
        request: &personal_os_rust_server::sunfish_ipa_linux_uc::Write,
    ) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_uc::Error> {
        // SAFETY: the same audited uncached RW/NX IRQ page is retained.
        // Closed requests allow only W1C of a captured pending mask or
        // containment IRQ_EN=0; no IPA_IRQ_UC command register is selectable.
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            self.registers
                .ipa
                .cast_mut()
                .add(request.offset() / 4)
                .write_volatile(request.value().to_le());
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        Ok(())
    }
    fn shared(
        &mut self,
        field: personal_os_rust_server::sunfish_ipa_linux_irq::Shared,
    ) -> Result<u8, personal_os_rust_server::sunfish_ipa_linux_uc::Error> {
        use personal_os_rust_server::{sunfish_ipa_linux_irq as irq, sunfish_ipa_linux_uc as uc};
        irq::Io::shared(self, field).map_err(|_| uc::Error::Io)
    }
    fn acknowledge(&mut self) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_uc::Error> {
        use personal_os_rust_server::{sunfish_ipa_linux_irq as irq, sunfish_ipa_linux_uc as uc};
        Channel::new(irq::CHANNEL as usize)
            .irq_ack()
            .map_err(|_| uc::Error::Acknowledge)
    }
}
#[cfg(feature = "sunfish-native-startup-ipa-uc")]
fn install_uc_runtime(d: &Display, state: &mut Startup) {
    use personal_os_rust_server::sunfish_ipa_linux_uc::Runtime;
    d.clear();
    d.replace_line(0, "NATIVE IPA UC HANDLER");
    match Runtime::after_arming(&mut state._irq) {
        Ok(runtime) => {
            state._uc = Some(runtime);
            d.replace_line(1, "UC HANDLER ARMED");
        }
        Err(error) => {
            d.replace_line(1, "UC HANDOFF STOP");
            d.hex(3, "UC ERROR", error.code());
            contain_irq(state);
        }
    }
    d.replace_line(20, "POWER FLOORS REMAIN HELD");
    d.replace_line(21, "WAITING BEFORE FIRMWARE");
    d.replace_line(22, "PHYSICAL DELIVERY UNPROVEN");
    d.replace_line(26, "NO MODEM OR WLAN START");
    d.replace_line(27, "NO SMC OR FLASH");
    d.replace_line(28, "NO DMA OR ATS COMMAND");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
}
#[cfg(feature = "sunfish-native-startup-ipa-uc")]
fn dispatch_uc(state: &mut Startup, external_fault: bool) {
    let (Some(runtime), Some(registers), Some(expected), Some(domain)) = (
        state._uc.as_mut(),
        state._irq_registers,
        state._config_report.as_ref().and_then(|r| r.after),
        state
            ._domain
            .as_ref()
            .map(pixel_smmu_install::Access::Probe),
    ) else {
        contain_irq(state);
        return;
    };
    let mut io = IrqIo {
        registers,
        expected,
        domain,
        pool: state._buffers.as_ref(),
        #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
        firmware: state._firmware.as_ref(),
    };
    if external_fault {
        runtime.external_fault(&mut io);
    } else {
        let _ = runtime.notification(&mut io);
    }
    let report = runtime.report();
    if report.error.is_some() {
        if let Some(pool) = state._buffers.as_mut() {
            pool.quarantine();
        }
    }
    if diagnostic_display_owned() {
        return;
    }
    let d = unsafe { Display::new(registers.pixels) };
    d.replace_line(
        1,
        if report.error.is_some() {
            "UC HANDLER STOP"
        } else {
            "UC IRQ DISPATCHED"
        },
    );
    d.hex(3, "UC NOTICES", report.notifications);
    d.hex(4, "UC EVENTS", report.events);
    d.hex(5, "UC RESPONSES", report.responses);
    d.hex(6, "INIT OBSERVED", report.init_observed);
    d.hex(7, "CLEARS SENT", report.clear_submitted);
    d.hex(8, "CLEARS DONE", report.clear_completed);
    d.hex(9, "ACKS SENT", report.acknowledge_submitted);
    d.hex(10, "ACKS DONE", report.acknowledge_completed);
    if let Some(v) = report.last_status {
        d.hex(11, "LAST STATUS", v);
    }
    if let Some(v) = report.last_event {
        d.hex(12, "LAST EVENT", v as u32);
    }
    if let Some(v) = report.last_response {
        d.hex(13, "LAST RESPONSE", v as u32);
    }
    if let Some(error) = report.error {
        d.hex(14, "UC ERROR", error.code());
    }
    if let Some(error) = report.containment_error {
        d.hex(15, "MASK ERROR", error.code());
    }
    d.replace_line(17, "NO UC COMMAND SENT");
    d.replace_line(18, "NO READY TOKEN MINTED");
    d.flush();
    #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
    pixel_ipa_firmware::show_runtime_result(&d, state);
}
#[cfg(feature = "sunfish-native-startup-ipa-irq")]
impl personal_os_rust_server::sunfish_ipa_linux_irq::Io for IrqIo<'_> {
    fn guard(
        &mut self,
        enabled: u32,
    ) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_irq::Error> {
        use personal_os_rust_server::{
            sunfish_ipa_linux_buffers as buffers, sunfish_ipa_linux_irq as irq,
            sunfish_ipa_linux_resources as resources,
        };
        if !self.domain.retained_quiet() {
            return Err(irq::Error::Domain);
        }
        if self
            .pool
            .and_then(|p| p.buffer(buffers::Kind::Zero))
            .is_none()
        {
            return Err(irq::Error::Buffers);
        }
        read_retained_irq_snapshot(
            self.registers.ipa,
            self.registers.gsi,
            self.expected,
            enabled,
        )
        .map_err(|_| irq::Error::Config)?;
        for _ in 0..2 {
            for spec in resources::PROFILE {
                if word(self.registers.ipa, spec.offset) != spec.value {
                    return Err(irq::Error::Resources);
                }
            }
        }
        Ok(())
    }
    fn read(
        &mut self,
        register: personal_os_rust_server::sunfish_ipa_linux_irq::Read,
    ) -> Result<u32, personal_os_rust_server::sunfish_ipa_linux_irq::Error> {
        Ok(word(self.registers.ipa, register.offset()))
    }
    fn write(
        &mut self,
        request: &personal_os_rust_server::sunfish_ipa_linux_irq::Write,
    ) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_irq::Error> {
        use personal_os_rust_server::sunfish_ipa_linux_irq as irq;
        // SAFETY: the actual IRQ-profile CapDL audit grants one additional
        // uncached RW/NX IPA page. The closed request can write only IRQ_EN,
        // with values 0, 4 or 12; no IRQ_CLR, IPA_IRQ_UC or GSI write exists.
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            self.registers
                .ipa
                .cast_mut()
                .add(irq::ENABLE_OFFSET / 4)
                .write_volatile(request.value().to_le());
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        Ok(())
    }
    fn bind(&mut self) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_irq::Error> {
        use personal_os_rust_server::sunfish_ipa_linux_irq as irq;
        Channel::new(irq::CHANNEL as usize)
            .irq_ack()
            .map_err(|_| irq::Error::Binding)
    }
    fn shared(
        &mut self,
        field: personal_os_rust_server::sunfish_ipa_linux_irq::Shared,
    ) -> Result<u8, personal_os_rust_server::sunfish_ipa_linux_irq::Error> {
        use personal_os_rust_server::sunfish_ipa_linux_irq as irq;
        if !self.domain.retained_quiet() {
            return Err(irq::Error::Domain);
        }
        // SAFETY: the retained UC_SHARED SRAM mapping is present. Each closed
        // field names one defined u8; never load its reserved neighboring bytes.
        let value = unsafe { self.registers.shared.add(field.offset()).read_volatile() };
        unsafe {
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        Ok(value)
    }
}
#[cfg(feature = "sunfish-native-startup-ipa-irq")]
fn configure_irq(d: &Display, state: &mut Startup, ipa: *const u32, gsi: *const u32) {
    use personal_os_rust_server::sunfish_ipa_linux_irq as irq;
    d.clear();
    d.replace_line(0, "NATIVE IPA IRQ ARM");
    d.replace_line(1, "CHECK IRQ BASELINE");
    d.replace_line(26, "NO MODEM OR WLAN START");
    d.replace_line(27, "NO SMC OR FLASH");
    d.replace_line(28, "NO DMA OR ATS COMMAND");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
    let (Some(expected), Some(domain), Some(shared)) = (
        state._config_report.as_ref().and_then(|r| r.after),
        state
            ._domain
            .as_ref()
            .map(pixel_smmu_install::Access::Probe),
        state._shared_pointer,
    ) else {
        d.replace_line(1, "IRQ PRECONDITION STOP");
        d.flush();
        return;
    };
    let registers = IrqRegisters {
        ipa,
        gsi,
        shared,
        pixels: d.pixels_ptr(),
    };
    state._irq_registers = Some(registers);
    let mut io = IrqIo {
        registers,
        expected,
        domain,
        pool: state._buffers.as_ref(),
        #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
        firmware: None,
    };
    let report = state._irq.run(&mut io, |step| {
        d.replace_line(1, "ARM UC INTERRUPTS");
        d.hex(18, "ARM STEP", step as u32);
        d.hex(19, "WRITE VALUE", step.value());
        d.flush();
    });
    // A failed arm may have partially enabled an interrupt. Contain it with
    // IRQ_EN=0, retaining pending evidence and every mapping; never retry arm.
    if report.error.is_some() {
        state._irq_notice = Some(state._irq.notification(&mut io));
    }
    state._irq_report = Some(report);
    d.replace_line(
        1,
        if report.error.is_none() {
            "IPA UC IRQ ARM PASS"
        } else {
            "IPA UC IRQ ARM STOP"
        },
    );
    d.hex(2, "IRQ SUBMITTED", report.submitted as u32);
    d.hex(3, "IRQ COMPLETED", report.completed as u32);
    d.hex(4, "PHYSICAL IRQ", irq::IRQ);
    d.hex(5, "IRQ CHANNEL", irq::CHANNEL as u32);
    d.hex(6, "IRQ BOUND", report.bound as u32);
    if let Some(sample) = report.last {
        d.hex(7, "IRQ ENABLE", sample.enabled);
        d.hex(8, "IRQ STATUS", sample.status);
    }
    if let Some((global, context)) = domain.fault_status() {
        d.hex(9, "GLOBAL FSR", global);
        d.hex(10, "CONTEXT FSR", context);
    }
    d.hex(11, "EVENT MASK", irq::EVENT_MASK);
    d.hex(12, "RESPONSE MASK", irq::RESPONSE_MASK);
    d.hex(13, "IRQ NOTICES", state._irq_notifications);
    d.hex(20, "ALLOWED STATUS", irq::ALLOWED_DISABLED_STATUS);
    if let Some(error) = report.error {
        d.hex(14, "IRQ ARM ERROR", error.code());
        if let Some(n) = state._irq_notice {
            if let Some(s) = n.last {
                d.hex(15, "MASK READBACK", s.enabled);
                d.hex(16, "PENDING STATUS", s.status);
            }
        }
        d.hex(17, "IRQ CONTAINED", state._irq_notice.is_some() as u32);
        if let Some(pool) = state._buffers.as_mut() {
            pool.quarantine();
        }
    } else {
        d.replace_line(14, "CONFIG AND LIMITS RETAINED");
        d.replace_line(15, "PRIVATE BUFFERS RETAINED");
        d.replace_line(16, "UC EVENT AND RESPONSE ARMED");
        d.replace_line(17, "NO UC COMMAND SENT");
        d.replace_line(18, "WAITING BEFORE PAS");
        d.replace_line(19, "IRQ DELIVERY UNPROVEN");
    }
    d.flush();
}
#[cfg(feature = "sunfish-native-startup-ipa-irq")]
fn contain_irq(state: &mut Startup) {
    use personal_os_rust_server::sunfish_ipa_linux_irq as irq;
    let (Some(registers), Some(expected), Some(domain)) = (
        state._irq_registers,
        state._config_report.as_ref().and_then(|r| r.after),
        state
            ._domain
            .as_ref()
            .map(pixel_smmu_install::Access::Probe),
    ) else {
        return;
    };
    let n = state._irq.notification(&mut IrqIo {
        registers,
        expected,
        domain,
        pool: state._buffers.as_ref(),
        #[cfg(feature = "sunfish-native-startup-ipa-firmware")]
        firmware: state._firmware.as_ref(),
    });
    if state._irq_notice.is_none() {
        state._irq_notice = Some(n);
    }
    if let Some(pool) = state._buffers.as_mut() {
        pool.quarantine();
    }
    // The first notification is terminal; keep its evidence on repeat entry.
    if n.error == irq::Error::AlreadyStopped {
        return;
    }
    if diagnostic_display_owned() {
        return;
    }
    let d = unsafe { Display::new(registers.pixels) };
    d.replace_line(1, "IPA IRQ OBSERVED STOP");
    d.hex(13, "IRQ NOTICES", state._irq_notifications);
    d.hex(14, "NOTICE ERROR", n.error.code());
    if let Some(sample) = n.last {
        d.hex(15, "MASK READBACK", sample.enabled);
        d.hex(16, "PENDING STATUS", sample.status);
    }
    if let Some(event) = n.event {
        d.hex(17, "UC EVENT", event as u32);
    }
    if let Some(response) = n.response {
        d.hex(18, "UC RESPONSE", response as u32);
    }
    d.replace_line(20, "SOURCE AND IRQ MASKED");
    d.replace_line(21, "PENDING EVIDENCE RETAINED");
    d.flush();
}
#[cfg(feature = "sunfish-native-startup-ipa-resources")]
fn configure_resources(d: &Display, state: &mut Startup, ipa: *const u32, gsi: *const u32) {
    use personal_os_rust_server::{
        sunfish_ipa_linux_buffers as buffers, sunfish_ipa_linux_buffers_native::NativeRam,
        sunfish_ipa_linux_config as config, sunfish_ipa_linux_resources as resources,
    };
    struct Native<'a> {
        ipa: *const u32,
        gsi: *const u32,
        expected: config::Snapshot,
        domain: &'a pixel_smmu_install::Probe,
        pool: &'a buffers::Pool<NativeRam<'static>>,
    }
    impl resources::Io for Native<'_> {
        fn guard(&mut self) -> Result<(), resources::Error> {
            if !self.domain.retained_quiet() {
                return Err(resources::Error::Domain);
            }
            // The pool publishes metadata only after the full two-pass check;
            // any later SMMU notification quarantines that same retained owner.
            if self.pool.buffer(buffers::Kind::Zero).is_none() {
                return Err(resources::Error::Buffers);
            }
            read_retained_snapshot(self.ipa, self.gsi, self.expected)
                .map_err(|_| resources::Error::Config)
        }
        fn read(&mut self, register: resources::Register) -> Result<u32, resources::Error> {
            Ok(word(self.ipa, register.offset()))
        }
        fn write(&mut self, request: &resources::Write) -> Result<(), resources::Error> {
            // SAFETY: actual CapDL grants uncached RW/NX for IPA page zero.
            // The closed request names only one of seven source-derived
            // resource-limit words in that page, after all concrete guards.
            // GSI, IPA IRQ/endpoint pages and every other IPA page remain RO.
            unsafe {
                core::arch::asm!("dsb sy", options(nostack, preserves_flags));
                self.ipa
                    .cast_mut()
                    .add(request.register().offset() / 4)
                    .write_volatile(request.value().to_le());
                core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            }
            Ok(())
        }
    }
    d.clear();
    d.replace_line(0, "NATIVE IPA RESOURCES");
    d.replace_line(1, "CHECK RESOURCE BASELINE");
    d.replace_line(26, "NO MODEM OR WLAN START");
    d.replace_line(27, "NO SMC OR FLASH");
    d.replace_line(28, "NO DMA OR ATS COMMAND");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
    let (Some(expected), Some(domain), Some(pool)) = (
        state._config_report.as_ref().and_then(|r| r.after),
        state._domain.as_ref(),
        state._buffers.as_ref(),
    ) else {
        d.replace_line(1, "RESOURCE PRECONDITION STOP");
        d.flush();
        return;
    };
    let report = state._resources.run(
        &mut Native {
            ipa,
            gsi,
            expected,
            domain,
            pool,
        },
        expected.guards[0],
        |register, value| {
            d.replace_line(1, "WRITE RESOURCE LIMIT");
            d.replace_line(18, register.label());
            d.hex(19, "WRITE OFFSET", register.offset() as u32);
            d.hex(20, "WRITE VALUE", value);
            d.flush();
        },
    );
    d.replace_line(
        1,
        if report.error.is_none() {
            "IPA RESOURCE LIMITS PASS"
        } else {
            "IPA RESOURCE LIMITS STOP"
        },
    );
    d.hex(2, "RSRC SUBMITTED", report.submitted as u32);
    d.hex(3, "RSRC COMPLETED", report.completed as u32);
    if let Some(values) = report.after {
        for (index, spec) in resources::PROFILE.into_iter().enumerate() {
            d.hex(4 + index, spec.register.label(), values[index]);
        }
    }
    if let Some(topology) = report.topology {
        d.hex(11, "TX COUNT", topology.tx_count as u32);
        d.hex(12, "RX COUNT", topology.rx_count as u32);
        d.hex(13, "RX BASE", topology.rx_base as u32);
        d.hex(14, "AVAILABLE EPS", topology.available);
        d.hex(15, "DEFINED EPS", topology.defined);
    }
    if let Some((global, context)) = domain.fault_status() {
        d.hex(16, "GLOBAL FSR", global);
        d.hex(17, "CONTEXT FSR", context);
    }
    if let Some(error) = report.error {
        d.hex(21, "RESOURCE ERROR", error.code());
        if let Some(register) = report.failed {
            d.hex(22, "FAILED OFFSET", register.offset() as u32);
        }
        if let Some(pool) = state._buffers.as_mut() {
            pool.quarantine();
        }
    } else {
        d.replace_line(18, "CONFIG RETAINED");
        d.replace_line(19, "SEVEN FIXED LIMITS");
        d.replace_line(20, "EIGHT ENDPOINTS VALID");
        d.replace_line(21, "HASH TABLE CONFIG SKIPPED");
        d.replace_line(22, "IPA INTERRUPTS DISABLED");
        d.replace_line(23, "BUFFERS RETAINED");
    }
    state._resource_report = Some(report);
    d.flush();
}
#[cfg(feature = "sunfish-native-startup-ipa-buffers")]
fn prepare_buffers(d: &Display, state: &mut Startup, ipa: *const u32, gsi: *const u32) {
    use personal_os_rust_server::sunfish_ipa_linux_buffers as buffers;
    d.clear();
    d.replace_line(0, "NATIVE IPA BUFFERS");
    d.replace_line(1, "CLAIM PRIVATE POOL");
    d.replace_line(26, "NO MODEM OR WLAN START");
    d.replace_line(27, "NO SMC OR FLASH");
    d.replace_line(28, "NO DMA OR ATS COMMAND");
    d.replace_line(29, "RESTART WHEN DONE");
    d.flush();
    let Some(expected) = state._config_report.as_ref().and_then(|r| r.after) else {
        return;
    };
    let Some(domain) = state._domain.as_mut() else {
        return;
    };
    let ram = match domain.take_private_pool() {
        Ok(ram) => ram,
        Err(error) => {
            d.replace_line(1, "BUFFER CLAIM STOP");
            d.hex(20, "BUFFER ERROR", error.code());
            d.flush();
            return;
        }
    };
    let physical = ram.physical_base();
    let mut pool = match buffers::Pool::new(ram) {
        Ok(pool) => pool,
        Err(error) => {
            d.replace_line(1, "BUFFER LAYOUT STOP");
            d.hex(20, "BUFFER ERROR", error.code());
            d.flush();
            return;
        }
    };
    let report = pool.prepare(
        || {
            if !domain.retained_quiet() {
                return Err(buffers::Error::Domain);
            }
            read_retained_snapshot(ipa, gsi, expected).map_err(|_| buffers::Error::Config)
        },
        |phase, index, buffer| {
            d.replace_line(
                1,
                match phase {
                    buffers::Phase::Initialize => "INITIALIZE PRIVATE RAM",
                    buffers::Phase::VerifyFirst => "VERIFY BUFFER PASS ONE",
                    buffers::Phase::VerifySecond => "VERIFY BUFFER PASS TWO",
                },
            );
            d.hex(18, "BUFFER INDEX", index as u32);
            d.hex(19, "POOL OFFSET", buffer.offset() as u32);
            d.flush();
        },
    );
    d.replace_line(
        1,
        if report.error.is_none() {
            "IPA BUFFER PREP PASS"
        } else {
            "IPA BUFFER PREP STOP"
        },
    );
    d.hex(2, "BUFFER COUNT", buffers::BUFFER_COUNT as u32);
    d.hex(3, "INITIALIZED", report.initialized as u32);
    d.hex(4, "VERIFIED", report.verified as u32);
    d.hex(5, "CHECKED BYTES", report.checked_bytes as u32);
    d.hex(6, "POOL IOVA", buffers::POOL_IOVA as u32);
    d.hex(7, "POOL PA LOW", physical as u32);
    d.hex(8, "USED BYTES", pool.plan().used_bytes() as u32);
    d.hex(9, "RESERVED BYTES", pool.plan().reserved_bytes() as u32);
    d.hex(10, "REMAINING BYTES", pool.plan().remaining_bytes() as u32);
    d.hex(11, "ZERO BYTES", buffers::ZERO_BYTES as u32);
    d.hex(12, "TABLE BYTES", buffers::TABLE_BYTES as u32);
    d.hex(13, "FILTER BITMAP", buffers::FILTER_BITMAP as u32);
    d.hex(14, "CMD POOL BYTES", buffers::COMMAND_POOL_BYTES as u32);
    if let Some((global, context)) = domain.fault_status() {
        d.hex(15, "GLOBAL FSR", global);
        d.hex(16, "CONTEXT FSR", context);
    }
    d.hex(
        17,
        "TABLE IOVA",
        pool.plan().find(buffers::Kind::Table).unwrap().iova() as u32,
    );
    if let Some(index) = report.failed_buffer {
        d.hex(18, "FAILED BUFFER", index as u32);
    }
    if let Some(offset) = report.failed_offset {
        d.hex(19, "FAILED OFFSET", offset as u32);
    }
    if let Some(error) = report.error {
        d.hex(20, "BUFFER ERROR", error.code());
        d.hex(24, "BUFFER PHASE", report.phase as u32);
        if error == buffers::Error::Readback {
            d.hex(21, "EXPECTED WORD", report.expected_word);
            d.hex(22, "ACTUAL WORD", report.actual_word);
        }
    } else {
        d.replace_line(20, "CONFIG RETAINED");
        d.replace_line(21, "EIGHT AP RING BUFFERS");
        d.replace_line(22, "ZERO AND TABLE BUILT");
        d.replace_line(23, "CPU READBACK ONLY");
    }
    state._buffer_report = Some(report);
    state._buffers = Some(pool);
    d.flush();
}
#[cfg(feature = "sunfish-native-startup-ipa-memory")]
fn configure_local_memory(d: &Display, state: &mut Startup, ipa: *const u32, gsi: *const u32) {
    use personal_os_rust_server::{
        sunfish_ipa_linux_config as config, sunfish_ipa_linux_memory as mem,
    };
    struct Native<'a> {
        ipa: *const u32,
        gsi: *const u32,
        shared: *mut u32,
        domain: &'a pixel_smmu_install::Probe,
    }
    impl config::Io for Native<'_> {
        fn read(&mut self, r: config::Read) -> Result<u32, config::Error> {
            Ok(word(
                if r.is_gsi() { self.gsi } else { self.ipa },
                r.offset(),
            ))
        }
        fn write(&mut self, _: &config::Write) -> Result<(), config::Error> {
            // The primitive configuration is complete; this adapter cannot
            // replay its writes when a guard changes during SRAM setup.
            Err(config::Error::Io)
        }
    }
    impl mem::Io for Native<'_> {
        fn domain_quiet(&mut self) -> Result<(), mem::Error> {
            if self.domain.retained_quiet() {
                Ok(())
            } else {
                Err(mem::Error::Domain)
            }
        }
        fn read_canary(&mut self, request: mem::CanaryWord) -> Result<u32, mem::Error> {
            if request.offset() % 4 != 0 || request.offset() >= mem::SHARED_BYTES {
                return Err(mem::Error::Io);
            }
            Ok(word(self.shared.cast_const(), request.offset()))
        }
        fn write_canary(&mut self, request: mem::CanaryWord) -> Result<(), mem::Error> {
            if request.offset() % 4 != 0 || request.offset() >= mem::SHARED_BYTES {
                return Err(mem::Error::Io);
            }
            // SAFETY: the image's actual CapDL audit binds this exclusive
            // uncached RW/NX 8-KiB aperture to PA 0x01e47000. The request
            // names one of the 25 fixed source-defined canary words. CPU
            // aligned volatile accesses avoid SIMD on Device memory. No
            // payload memory, DMA command or other MMIO is writable here.
            unsafe {
                core::arch::asm!("dsb sy", options(nostack, preserves_flags));
                self.shared
                    .add(request.offset() / 4)
                    .write_volatile(request.value().to_le());
                core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            }
            Ok(())
        }
    }
    d.replace_line(0, "NATIVE IPA LOCAL MEMORY");
    d.replace_line(1, "CHECK IPA SRAM MAPPING");
    for row in 12..=24 {
        if row != 15 && row != 16 {
            d.replace_line(row, "");
        }
    }
    d.flush();
    let shared = memory_region_symbol!(ipa_shared: *mut u32).as_ptr();
    #[cfg(feature = "sunfish-native-startup-ipa-irq")]
    {
        state._shared_pointer = Some(shared.cast_const().cast());
    }
    if shared as usize != 0x44200000 {
        d.replace_line(1, "IPA SRAM MAPPING STOP");
        d.flush();
        return;
    }
    let Some(expected) = state._config_report.as_ref().and_then(|r| r.after) else {
        d.replace_line(1, "IPA CONFIG REPORT STOP");
        d.flush();
        return;
    };
    let Some(domain) = state._domain.as_ref() else {
        d.replace_line(1, "IPA DOMAIN REPORT STOP");
        d.flush();
        return;
    };
    let report = state._memory.run(
        &mut Native {
            ipa,
            gsi,
            shared,
            domain,
        },
        expected,
        |request| {
            d.replace_line(1, "WRITE IPA CANARY");
            d.hex(18, "CANARY INDEX", request.index() as u32);
            d.hex(19, "CANARY OFFSET", request.offset() as u32);
            d.flush();
        },
    );
    state._memory_report = Some(report);
    d.replace_line(
        1,
        if report.error.is_none() {
            "IPA CANARY SETUP PASS"
        } else {
            "IPA CANARY SETUP STOP"
        },
    );
    d.hex(12, "CANARY SENT", report.submitted);
    d.hex(13, "CANARY DONE", report.completed);
    d.hex(14, "CANARY WORDS", report.verified_words as u32);
    if let Some((global, context)) = domain.fault_status() {
        d.hex(15, "GLOBAL FSR", global);
        d.hex(16, "CONTEXT FSR", context);
    }
    d.hex(17, "LAST READ", report.last_read);
    if let Some(failed) = report.failed {
        d.hex(18, "CANARY INDEX", failed.index() as u32);
        d.hex(19, "CANARY OFFSET", failed.offset() as u32);
    }
    if let Some(layout) = report.geometry {
        d.hex(21, "REPORTED BYTES", layout.reported_bytes as u32);
        d.hex(22, "SRAM OFFSET", layout.offset as u32);
        d.hex(23, "USABLE BYTES", layout.usable_bytes as u32);
    }
    if let Some(error) = report.error {
        d.hex(20, "MEMORY ERROR", error.code());
        if let mem::Error::Config(source) = error {
            d.hex(24, "CONFIG ERROR", source as u32 + 1);
        }
    } else {
        d.replace_line(20, "CONFIG RETAINED");
    }
    d.replace_line(25, "25 FIXED CANARY WORDS");
    d.flush();
}
#[cfg(feature = "sunfish-native-startup-ipa-domain")]
fn read_retained_snapshot(
    ipa: *const u32,
    gsi: *const u32,
    expected: personal_os_rust_server::sunfish_ipa_linux_config::Snapshot,
) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_config::Error> {
    read_retained_irq_snapshot(ipa, gsi, expected, 0)
}
#[cfg(feature = "sunfish-native-startup-ipa-domain")]
fn read_retained_irq_snapshot(
    ipa: *const u32,
    gsi: *const u32,
    expected: personal_os_rust_server::sunfish_ipa_linux_config::Snapshot,
    enabled: u32,
) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_config::Error> {
    use personal_os_rust_server::sunfish_ipa_linux_config as config;
    struct ReadOnly {
        ipa: *const u32,
        gsi: *const u32,
    }
    impl config::Io for ReadOnly {
        fn read(&mut self, r: config::Read) -> Result<u32, config::Error> {
            Ok(word(
                if r.is_gsi() { self.gsi } else { self.ipa },
                r.offset(),
            ))
        }
        fn write(&mut self, _: &config::Write) -> Result<(), config::Error> {
            Err(config::Error::Io)
        }
    }
    config::verify_retained_uc_irq(&mut ReadOnly { ipa, gsi }, expected, enabled)
}
#[cfg(feature = "sunfish-native-startup-ipa-firmware")]
fn read_retained_post_auth_snapshot(
    ipa: *const u32,
    gsi: *const u32,
    expected: personal_os_rust_server::sunfish_ipa_linux_config::Snapshot,
) -> Result<(), personal_os_rust_server::sunfish_ipa_linux_config::Error> {
    use personal_os_rust_server::sunfish_ipa_linux_config as config;
    struct ReadOnly {
        ipa: *const u32,
        gsi: *const u32,
    }
    impl config::Io for ReadOnly {
        fn read(&mut self, r: config::Read) -> Result<u32, config::Error> {
            Ok(word(
                if r.is_gsi() { self.gsi } else { self.ipa },
                r.offset(),
            ))
        }
        fn write(&mut self, _: &config::Write) -> Result<(), config::Error> {
            Err(config::Error::Io)
        }
    }
    config::verify_retained_after_auth(&mut ReadOnly { ipa, gsi }, expected)
}
#[cfg(feature = "sunfish-native-startup-ipa-domain")]
fn verify_configuration_retained(
    d: &Display,
    state: &Startup,
    ipa: *const u32,
    gsi: *const u32,
) -> bool {
    use personal_os_rust_server::sunfish_ipa_linux_config as config;
    d.replace_line(1, "VERIFY IPA CONFIG");
    d.flush();
    let result = state
        ._config_report
        .as_ref()
        .and_then(|r| r.after)
        .ok_or(config::Error::Readback)
        .and_then(|expected| read_retained_snapshot(ipa, gsi, expected));
    if let Err(error) = result {
        d.replace_line(1, "RETAINED IPA CONFIG STOP");
        d.hex(20, "CONFIG ERROR", error as u32 + 1);
        d.flush();
        false
    } else {
        true
    }
}
#[cfg(feature = "sunfish-native-startup-ipa-config")]
fn configure_hardware(
    d: &Display,
    state: &mut Startup,
    ipa: *const u32,
    gsi: *const u32,
    power: Report,
) -> bool {
    use personal_os_rust_server::sunfish_ipa_linux_config as config;
    struct Native<'a> {
        ipa: *const u32,
        gsi: *const u32,
        display: &'a Display,
        first_reads: usize,
    }
    impl config::Io for Native<'_> {
        fn read(&mut self, r: config::Read) -> Result<u32, config::Error> {
            // Flush a checkpoint before each newly accessed register. Later
            // samples use the same bounded set; avoid hundreds of full-frame
            // flushes between the guarded write/readback steps.
            if self.first_reads < config::COUNT + 6 {
                self.display.replace_line(
                    2,
                    if r.is_gsi() {
                        "READ GSI BASELINE"
                    } else {
                        "READ IPA BASELINE"
                    },
                );
                self.display.hex(3, "READ OFFSET", r.offset() as u32);
                self.display.flush();
                self.first_reads += 1;
            }
            Ok(word(
                if r.is_gsi() { self.gsi } else { self.ipa },
                r.offset(),
            ))
        }
        fn write(&mut self, request: &config::Write) -> Result<(), config::Error> {
            // SAFETY: selected opt-in image maps only IPA page 0 RW; all nine
            // closed-enum registers lie within it. One PD owns this cold-boot
            // attempt. GSI, IRQ registers and all other IPA pages remain RO.
            // No borrowed RAM aliases or DMA descriptors are involved.
            unsafe {
                core::arch::asm!("dsb sy", options(nostack, preserves_flags));
                self.ipa
                    .cast_mut()
                    .add(request.register().offset() / 4)
                    .write_volatile(request.value().to_le());
                core::arch::asm!("dsb sy", options(nostack, preserves_flags));
            }
            Ok(())
        }
    }
    screen(d, "CHECK IPA CONFIG BASELINE");
    let report = state._config.run(
        &mut Native {
            ipa,
            gsi,
            display: d,
            first_reads: 0,
        },
        power.error.is_none() && power.submitted == 0x7ff && power.completed == 0x7ff,
        |register, value| {
            d.replace_line(1, "WRITE IPA HW CONFIG");
            d.replace_line(2, register.label());
            d.hex(3, "WRITE OFFSET", register.offset() as u32);
            d.hex(4, "WRITE VALUE", value);
            d.flush();
        },
    );
    state._config_report = Some(report);
    screen(
        d,
        if report.error.is_none() {
            "IPA HW CONFIG PASS"
        } else {
            "IPA HW CONFIG STOP"
        },
    );
    d.hex(3, "HW SUBMITTED", report.submitted as u32);
    d.hex(4, "HW COMPLETED", report.completed as u32);
    if let Some(error) = report.error {
        d.hex(5, "HW ERROR", error as u32 + 1);
    }
    if let Some(failed) = report.failed {
        d.replace_line(6, failed.label());
    }
    if let Some(last) = report.after {
        for (i, spec) in config::PROFILE.into_iter().enumerate() {
            d.hex(7 + i, spec.register.label(), last.config[i]);
        }
        for (i, label) in [
            "IPA FLAVOR",
            "IPA SHMEM",
            "AGGR ACTIVE",
            "IPA IRQ ENABLE",
            "GSI STATUS",
            "GSI PARAM2",
        ]
        .into_iter()
        .enumerate()
        {
            d.hex(17 + i, label, last.guards[i]);
        }
    }
    d.replace_line(24, "PRIMITIVE CONFIG ONLY");
    d.replace_line(25, "NO ENDPOINT OR IRQ ENABLE");
    d.flush();
    report.error.is_none() && report.submitted == 0x1ff && report.completed == 0x1ff
}
impl Handler for Startup {
    type Error = Infallible;
    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
        if self._mpss_runtime.is_some() {
            pixel_mpss_runtime::notified(self, channels);
            return Ok(());
        }
        #[cfg(feature = "sunfish-native-startup-mpss-auth")]
        if pixel_mpss_auth::started() {
            pixel_mpss_auth::notified(self, channels);
            #[cfg(feature = "sunfish-native-startup-mpss-runtime")]
            if pixel_mpss_auth::runtime_ready(self) {
                let d = self
                    ._irq_registers
                    .map(|registers| unsafe { Display::new(registers.pixels) });
                if let Some(d) = d {
                    pixel_mpss_runtime::start(&d, self);
                }
            }
            return Ok(());
        }
        #[cfg(feature = "sunfish-native-startup-linux-rmtfs")]
        if pixel_ipa_receive::notified(self, channels) {
            if self._linux_rmtfs.is_some() {
                if !pixel_ipa_receive::healthy(self) {
                    if self._linux_rmtfs.as_ref().unwrap().phase()
                        != pixel_linux_rmtfs::Phase::Failed
                    {
                        self._linux_rmtfs.as_mut().unwrap().fault(0x20);
                    }
                    #[cfg(feature = "sunfish-native-startup-mpss-smem")]
                    pixel_mpss_smem::fault(self, 0x20);
                    #[cfg(feature = "sunfish-native-startup-mpss-preauth")]
                    pixel_mpss_preauth::fault(self, 0x20);
                }
                if let Some(registers) = self._irq_registers {
                    #[cfg(not(feature = "sunfish-native-startup-mpss-smem"))]
                    show_linux_rmtfs(&unsafe { Display::new(registers.pixels) }, self);
                    #[cfg(all(
                        feature = "sunfish-native-startup-mpss-smem",
                        not(feature = "sunfish-native-startup-mpss-preauth")
                    ))]
                    pixel_mpss_smem::show(&unsafe { Display::new(registers.pixels) }, self);
                    #[cfg(all(
                        feature = "sunfish-native-startup-mpss-preauth",
                        feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic"
                    ))]
                    if !pixel_mpss_auth::diagnostics_owned() && pixel_mpss_preauth::started() {
                        pixel_mpss_preauth::show(&unsafe { Display::new(registers.pixels) }, self);
                    }
                    #[cfg(all(
                        feature = "sunfish-native-startup-mpss-preauth",
                        feature = "sunfish-native-startup-mpss-runtime",
                        not(feature = "sunfish-native-startup-mpss-runtime-gate-diagnostic")
                    ))]
                    if !pixel_mpss_auth::diagnostics_owned()
                        && pixel_mpss_preauth::started()
                    {
                        pixel_mpss_preauth::show(&unsafe { Display::new(registers.pixels) }, self);
                    }
                    #[cfg(all(
                        feature = "sunfish-native-startup-mpss-preauth",
                        not(feature = "sunfish-native-startup-mpss-runtime")
                    ))]
                    if pixel_mpss_preauth::started() {
                        pixel_mpss_preauth::show(&unsafe { Display::new(registers.pixels) }, self);
                    }
                }
            }
            return Ok(());
        }
        #[cfg(all(
            feature = "sunfish-native-startup-ipa-receive",
            not(feature = "sunfish-native-startup-linux-rmtfs")
        ))]
        if pixel_ipa_receive::notified(self, channels) {
            return Ok(());
        }
        #[cfg(feature = "sunfish-native-startup-ipa-gsi")]
        if channels.contains(Channel::new(pixel_ipa_gsi::CHANNEL)) {
            #[cfg(not(feature = "sunfish-native-startup-ipa-command"))]
            pixel_ipa_gsi::late_notification(self);
            #[cfg(all(
                feature = "sunfish-native-startup-ipa-command",
                not(feature = "sunfish-native-startup-ipa-transfer")
            ))]
            pixel_ipa_command::late_notification(self);
            #[cfg(all(
                feature = "sunfish-native-startup-ipa-transfer",
                not(feature = "sunfish-native-startup-ipa-tables")
            ))]
            pixel_ipa_transfer::late_notification(self);
            #[cfg(feature = "sunfish-native-startup-ipa-tables")]
            pixel_ipa_tables::late_notification(self);
            if channels
                .iter()
                .all(|c| c == Channel::new(pixel_ipa_gsi::CHANNEL))
            {
                return Ok(());
            }
        }
        #[cfg(feature = "sunfish-native-startup-ipa-irq")]
        {
            // Distinguish the IPA IRQ from the retained SMMU fault lines.
            // The opt-in UC runtime drains/ACKs a pure IPA notification.
            // Legacy probes still contain every notification. Any foreign
            // or co-delivered fault takes priority and remains unacknowledged.
            let has_ipa = channels.iter().any(|c| {
                c == Channel::new(personal_os_rust_server::sunfish_ipa_linux_irq::CHANNEL as usize)
            });
            let ipa_only = has_ipa
                && channels.iter().all(|c| {
                    c == Channel::new(
                        personal_os_rust_server::sunfish_ipa_linux_irq::CHANNEL as usize,
                    )
                });
            if has_ipa {
                self._irq_notifications = self._irq_notifications.saturating_add(1);
            }
            #[cfg(feature = "sunfish-native-startup-ipa-uc")]
            dispatch_uc(self, !ipa_only);
            #[cfg(not(feature = "sunfish-native-startup-ipa-uc"))]
            contain_irq(self);
            if ipa_only {
                return Ok(());
            }
        }
        #[cfg(feature = "sunfish-native-startup-ipa-buffers")]
        if let Some(buffers) = &mut self._buffers {
            buffers.quarantine();
        }
        #[cfg(feature = "sunfish-native-startup-ipa-receive-memory")]
        if let Some(data) = &mut self._receive_data {
            data.quarantine();
        }
        #[cfg(feature = "sunfish-native-startup-ipa-domain")]
        if let Some(domain) = &mut self._domain {
            // Shared handler latches the fault and retains masked IRQs and
            // all mappings. It never retries or continues startup.
            return domain.notified(channels);
        }
        let _ = channels;
        Ok(())
    }
    #[cfg(feature = "sunfish-native-startup-ipa-irq")]
    fn run(&mut self) -> Result<sel4_microkit::Never, Self::Error> {
        // No framework startup ACK: each actual IRQ capability is armed only
        // by its concrete setup step. Terminal notifications stay masked.
        loop {
            match sel4_microkit::ipc::recv() {
                sel4_microkit::ipc::Event::Notified(channels) => self.notified(channels)?,
                sel4_microkit::ipc::Event::Protected(_, _)
                | sel4_microkit::ipc::Event::Fault(_, _) => {
                    panic!("unexpected event in isolated IPA IRQ probe")
                }
            }
        }
    }
    fn protected(&mut self, _: Channel, _: MessageInfo) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
