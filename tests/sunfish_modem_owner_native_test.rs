// Legacy boot fixtures and a kernel-API stub; the real Microkit API is also
// compiled into the target ELF. Native SMEM/lock/volatile/owner code is real.
#![allow(dead_code)]
#[path = "../rust/src/qcom_wlfw_bdf.rs"]
pub mod qcom_wlfw_bdf;
#[cfg(feature = "sunfish-modem-servreg-state-probe")]
pub mod pixel_probe_display {
    pub struct Display;

    impl Display {
        pub unsafe fn new(_: *mut u32) -> Self {
            Self
        }
        pub fn clear(&self) {}
        pub fn line(&self, _: usize, _: &str) {}
        pub fn hex(&self, _: usize, _: &str, _: u32) {}
        pub fn flush(&self) {}
    }
}
include!("sunfish_glink_channel_test.rs");
extern crate self as sel4_microkit;
#[path = "../rust/src/pixel_modem_owner.rs"]
pub mod pixel_modem_owner;
#[path = "../rust/src/pixel_modem_owner_gate.rs"]
pub mod pixel_modem_owner_gate;
#[path = "../rust/src/qcom_bcm.rs"]
pub mod qcom_bcm;
#[path = "../rust/src/qcom_bcm_vote.rs"]
pub mod qcom_bcm_vote;
#[path = "../rust/src/qcom_cmd_db.rs"]
pub mod qcom_cmd_db;
#[path = "../rust/src/qcom_glink_fifo.rs"]
pub mod qcom_glink_fifo;
#[path = "../rust/src/qcom_glink_native_io.rs"]
pub mod qcom_glink_native_io;
#[allow(dead_code)]
#[path = "../rust/src/qcom_wlan_power.rs"]
pub mod qcom_wlan_power;
#[path = "../rust/src/qcom_glink_qmi.rs"]
pub mod qcom_glink_qmi;
#[path = "../rust/src/qcom_ipa.rs"]
pub mod qcom_ipa;
#[path = "../rust/src/qcom_ipa_client.rs"]
pub mod qcom_ipa_client;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
pub mod qcom_ipa_service;
#[path = "../rust/src/qcom_modem_handlers.rs"]
pub mod qcom_modem_handlers;
#[path = "../rust/src/qcom_modem_owner.rs"]
pub mod qcom_modem_owner;
#[path = "../rust/src/qcom_modem_pools.rs"]
pub mod qcom_modem_pools;
#[path = "../rust/src/qcom_modem_services.rs"]
pub mod qcom_modem_services;
#[path = "../rust/src/qcom_msa.rs"]
pub mod qcom_msa;
#[path = "../rust/src/qcom_msa_handoff.rs"]
pub mod qcom_msa_handoff;
#[path = "../rust/src/qcom_msa_native.rs"]
pub mod qcom_msa_native;
#[path = "../rust/src/qcom_msm_bus_aggregate.rs"]
pub mod qcom_msm_bus_aggregate;
#[path = "../rust/src/qcom_qmi.rs"]
pub mod qcom_qmi;
#[path = "../rust/src/qcom_qmi_router.rs"]
pub mod qcom_qmi_router;
#[path = "../rust/src/qcom_qrtr_control.rs"]
pub mod qcom_qrtr_control;
#[path = "../rust/src/qcom_qrtr_tftp.rs"]
pub mod qcom_qrtr_tftp;
#[path = "../rust/src/qcom_rpmh_handoff.rs"]
pub mod qcom_rpmh_handoff;
#[path = "../rust/src/qcom_rpmh_read.rs"]
pub mod qcom_rpmh_read;
#[path = "../rust/src/qcom_rpmh_vote_aggregate.rs"]
pub mod qcom_rpmh_vote_aggregate;
#[path = "../rust/src/qcom_rpmh_vote_broker.rs"]
pub mod qcom_rpmh_vote_broker;
#[path = "../rust/src/qcom_rpmh_vrm.rs"]
pub mod qcom_rpmh_vrm;
#[path = "../rust/src/qcom_rpmh_vrm_command.rs"]
pub mod qcom_rpmh_vrm_command;
#[path = "../rust/src/qcom_rpmh_vrm_tcs.rs"]
pub mod qcom_rpmh_vrm_tcs;
#[path = "../rust/src/qcom_wlan_rpmh_map.rs"]
pub mod qcom_wlan_rpmh_map;
#[path = "../rust/src/qcom_wlan_rpmh_io.rs"]
#[allow(dead_code)]
pub mod qcom_wlan_rpmh_io;
#[path = "../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[path = "../rust/src/qcom_servreg_notif.rs"]
pub mod qcom_servreg_notif;
#[path = "../rust/src/qcom_servreg_notif_client.rs"]
pub mod qcom_servreg_notif_client;
#[path = "../rust/src/qcom_tftp.rs"]
pub mod qcom_tftp;
#[path = "../rust/src/qcom_ufs_host.rs"]
pub mod qcom_ufs_host;
#[path = "../rust/src/qcom_ufs_irq.rs"]
pub mod qcom_ufs_irq;
#[path = "../rust/src/qcom_ufs_protocol.rs"]
pub mod qcom_ufs_protocol;
#[path = "../rust/src/qcom_wlfw_client.rs"]
pub mod qcom_wlfw_client;
#[path = "../rust/src/sunfish_ipa_bus_vote.rs"]
pub mod sunfish_ipa_bus_vote;
#[path = "../rust/src/sunfish_ipa_post_init.rs"]
pub mod sunfish_ipa_post_init;
#[path = "../rust/src/sunfish_ipa_profile.rs"]
pub mod sunfish_ipa_profile;
#[path = "../rust/src/wlfw_protocol.rs"]
pub mod wlfw_protocol;

#[macro_export]
macro_rules! debug_println { ($($arg:tt)*) => { { let _ = format_args!($($arg)*); } }; }
#[derive(Clone, Copy)]
pub struct Channel(usize);
thread_local! {
    static KERNEL_CALLS: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
    static SCM_CALLS: std::cell::RefCell<Vec<[u64; 8]>> = const { std::cell::RefCell::new(Vec::new()) };
}
impl Channel {
    pub fn new(id: usize) -> Self {
        assert!(id < 5);
        Self(id)
    }
    pub fn index(&self) -> usize {
        self.0
    }
    pub fn irq_ack(&self) -> Result<(), qcom_modem_owner::Error> {
        assert!(self.0 < 2 || self.0 == qcom_ufs_irq::UFS_IRQ_CHANNEL_ID);
        KERNEL_CALLS.with(|calls| calls.borrow_mut().push(self.0));
        Ok(())
    }
    pub fn notify(&self) {
        assert_eq!(self.0, 2);
        KERNEL_CALLS.with(|calls| calls.borrow_mut().push(self.0));
    }
}

#[test]
fn owner_native_ufs_kernel_acknowledger_accepts_only_candidate_channel_four() {
    KERNEL_CALLS.with(|calls| calls.borrow_mut().clear());
    let mut ack = pixel_modem_owner::UfsKernelAcknowledger;
    assert!(qcom_ufs_irq::KernelAcknowledger::acknowledge(&mut ack, 3).is_err());
    qcom_ufs_irq::KernelAcknowledger::acknowledge(&mut ack, qcom_ufs_irq::UFS_IRQ_CHANNEL_ID)
        .unwrap();
    KERNEL_CALLS.with(|calls| assert_eq!(*calls.borrow(), [4]));
}
pub struct ChannelSet(u64);
impl ChannelSet {
    pub fn iter(&self) -> impl Iterator<Item = Channel> {
        let bits = self.0;
        (0..4).filter(move |id| bits & (1 << id) != 0).map(Channel)
    }
}

fn native_owner_checkpoint() -> (
    Vec<u8>,
    pixel_glink_channel::ChannelCheckpoint,
    qcom_modem_status::Item,
) {
    native_owner_checkpoint_for_generation(1)
}

fn native_owner_checkpoint_for_generation(
    generation: u64,
) -> (
    Vec<u8>,
    pixel_glink_channel::ChannelCheckpoint,
    qcom_modem_status::Item,
) {
    let (mut m, checkpoint, rx) = channel_fixture_for_generation(generation);
    let mut lock = 0;
    let mut window = unsafe { Window::new(m.as_ptr(), &mut lock) };
    struct NativeQueue(ChannelIo);
    impl pixel_glink_channel::Io for NativeQueue {
        fn byte(&mut self, index: usize, value: u8) {
            self.0.byte(index, value)
        }
        fn head(&mut self) {
            self.0.head()
        }
        fn notify(&mut self) {
            self.0.notify();
            // Publish two usable intents before the native capture observes
            // the peer head. No already-published bytes are changed later.
            unsafe {
                let m = std::slice::from_raw_parts_mut(self.0.base, SMEM_BYTES);
                w32(m, self.0.rx + 104, 128);
                w32(m, self.0.rx + 108, 0);
                w32(m, self.0.rx + 112, 4096);
                w32(m, self.0.rx + 116, 1);
            }
        }
    }
    let mut io = NativeQueue(ChannelIo::new(&mut m, &lock, rx.offset));
    let checkpoint = pixel_glink_channel::checkpoint_run(
        &mut window,
        checkpoint,
        &mut io,
        &mut Time {
            now: 0,
            step: 50000,
        },
        |_| {},
    )
    .unwrap();
    (m, checkpoint, rx)
}

fn simulated_scm(
    call: &qcom_modem_pools::Call,
    lifecycle: &mut dyn FnMut() -> Result<(), qcom_modem_handlers::Fault>,
) -> Result<[u64; 8], qcom_modem_handlers::Fault> {
    lifecycle()?;
    SCM_CALLS.with(|calls| calls.borrow_mut().push(*call.words()));
    Ok([0; 8])
}

struct CoordinatorReadyBackend;

impl sunfish_ipa_post_init::Backend for CoordinatorReadyBackend {
    type Error = ();

    fn apply_initial_bus_vote(
        &mut self,
        _generation: u64,
        _use_case: &'static sunfish_ipa_bus_vote::UseCase,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn establish_register_access(&mut self, _generation: u64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn initialize_fixed_registers(&mut self, _generation: u64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn validate_modem_sram_partition(&mut self, _generation: u64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn allocate_gsi_support_buffers(&mut self, _generation: u64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn initialize_interrupts(&mut self, _generation: u64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn register_gsi(&mut self, _generation: u64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn connect_ap_pipes(&mut self, _generation: u64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn capture_runtime_snapshot(
        &mut self,
        _generation: u64,
    ) -> Result<sunfish_ipa_profile::RuntimeSnapshot, Self::Error> {
        Ok(sunfish_ipa_profile::RuntimeSnapshot {
            shared_mem_baddr_units: 0x10,
            shared_mem_size_units: 0x400,
            apps_wan_cons_endpoint: Some(sunfish_ipa_profile::APPS_WAN_CONS_ENDPOINT),
            ipa_uc_loaded: false,
        })
    }
}

#[repr(align(4096))]
struct ScmScratch([u8; qcom_modem_pools::ARG_BYTES]);

fn owner_attach(
    m: &mut Vec<u8>,
    checkpoint: pixel_glink_channel::ChannelCheckpoint,
    lock: &mut u32,
    doorbell: &mut [u32; 4],
    scratch: &mut ScmScratch,
) -> Result<pixel_modem_owner::Runtime, qcom_modem_owner::Error> {
    let window = unsafe { Window::new(m.as_ptr(), lock) };
    let scm =
        unsafe { pixel_modem_owner::ScmResources::new(scratch.0.as_mut_ptr(), simulated_scm)? };
    unsafe {
        pixel_modem_owner::attach(
            window,
            checkpoint,
            m.as_ptr(),
            m.as_mut_ptr().add(PART),
            doorbell.as_mut_ptr(),
            m.as_mut_ptr().cast(),
            Some(scm),
            |_| {},
        )
    }
}

fn owner_attach_with_services<S: pixel_modem_owner::RuntimeServices>(
    m: &mut Vec<u8>,
    checkpoint: pixel_glink_channel::ChannelCheckpoint,
    lock: &mut u32,
    doorbell: &mut [u32; 4],
    scratch: &mut ScmScratch,
    services: S,
) -> Result<pixel_modem_owner::Runtime<S>, qcom_modem_owner::Error> {
    let window = unsafe { Window::new(m.as_ptr(), lock) };
    let scm =
        unsafe { pixel_modem_owner::ScmResources::new(scratch.0.as_mut_ptr(), simulated_scm)? };
    unsafe {
        pixel_modem_owner::attach_with_services(
            window,
            checkpoint,
            m.as_ptr(),
            m.as_mut_ptr().add(PART),
            doorbell.as_mut_ptr(),
            m.as_mut_ptr().cast(),
            Some(scm),
            services,
            |_| {},
        )
    }
}

#[test]
fn owner_native_ipa_server_rejects_a_readiness_permit_from_another_generation() {
    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let services = pixel_modem_owner::IpaServerServices::new(
        pixel_modem_owner::NoServices,
        qcom_ipa_service::ApReadyPermit::test_after_post_init(2),
    );
    let mut runtime = owner_attach_with_services(
        &mut m,
        checkpoint,
        &mut lock,
        &mut doorbell,
        &mut scratch,
        services,
    )
    .unwrap();

    SCM_CALLS.with(|calls| calls.borrow_mut().clear());
    assert_eq!(
        runtime.start(),
        Err(qcom_modem_owner::Error::IpaReadinessRequired)
    );
    SCM_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
}

#[test]
fn owner_native_ipa_server_waits_for_peer_hello_before_advertising() {
    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let services = pixel_modem_owner::IpaServerServices::new(
        pixel_modem_owner::NoServices,
        qcom_ipa_service::ApReadyPermit::test_after_post_init(1),
    );
    let mut runtime = owner_attach_with_services(
        &mut m,
        checkpoint,
        &mut lock,
        &mut doorbell,
        &mut scratch,
        services,
    )
    .unwrap();

    runtime.start().unwrap();
    let control = runtime.control_status();
    assert!(control.hello_sent);
    assert!(!control.peer_hello);
    assert_eq!(control.announced_services, 0);
}

#[test]
fn owner_native_prepared_ipa_and_servreg_session_attaches_without_re_registration() {
    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let prepared = pixel_modem_owner::IpaServerServices::prepare_before_mpss(
        pixel_modem_owner::NoServices,
        qcom_ipa_service::ApReadyPermit::test_after_post_init(1),
    )
    .unwrap();
    let window = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let scm =
        unsafe { pixel_modem_owner::ScmResources::new(scratch.0.as_mut_ptr(), simulated_scm) }
            .unwrap();
    let mut runtime = unsafe {
        pixel_modem_owner::attach_with_prepared_services(
            window,
            checkpoint,
            m.as_ptr(),
            m.as_mut_ptr().add(PART),
            doorbell.as_mut_ptr(),
            m.as_mut_ptr().cast(),
            Some(scm),
            prepared,
            |_| {},
        )
    }
    .unwrap();

    runtime.start().unwrap();
    let control = runtime.control_status();
    assert!(control.hello_sent);
    assert!(!control.peer_hello);
    assert_eq!(
        control.ready_services & ((1 << 2) | (1 << 4)),
        (1 << 2) | (1 << 4)
    );
    assert_eq!(control.ready_services & 0b11, 0);
    assert_eq!(control.announced_services, 0);
    assert_eq!(control.announced_services & 0b11, 0);
}

#[test]
fn prepared_modem_boot_coordinator_carries_post_init_generation_through_glink_attach() {
    let generation = 7;
    let mut runner = sunfish_ipa_post_init::Runner::new(generation);
    let ready = runner.run(&mut CoordinatorReadyBackend).unwrap();
    let prepared = pixel_modem_owner::PreparedModemBoot::after_ipa_post_init(
        pixel_modem_owner::NoServices,
        ready,
    )
    .unwrap();
    assert_eq!(prepared.generation(), generation);
    let mut handshake_smem = vec![0; qcom_modem_status::SMEM_BYTES];
    let mut handshake_doorbell = [0; 4];
    let handshake = unsafe {
        prepared.new_smp2p_handshake(
            handshake_smem.as_mut_ptr().add(PART),
            handshake_doorbell.as_mut_ptr(),
        )
    }
    .unwrap();
    assert_eq!(handshake.generation(), generation);

    let (mut m, checkpoint, _) = native_owner_checkpoint_for_generation(generation);
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let window = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let scm =
        unsafe { pixel_modem_owner::ScmResources::new(scratch.0.as_mut_ptr(), simulated_scm) }
            .unwrap();
    let mut runtime = unsafe {
        prepared.attach_after_glink(
            window,
            checkpoint,
            m.as_ptr(),
            m.as_mut_ptr().add(PART),
            doorbell.as_mut_ptr(),
            m.as_mut_ptr().cast(),
            Some(scm),
            |_| {},
        )
    }
    .unwrap();

    assert_eq!(runtime.generation(), generation);
    runtime.start().unwrap();
    assert!(runtime.control_status().hello_sent);
}

#[test]
fn prepared_modem_boot_rejects_a_checkpoint_from_another_generation_before_writes() {
    let mut runner = sunfish_ipa_post_init::Runner::new(7);
    let ready = runner.run(&mut CoordinatorReadyBackend).unwrap();
    let prepared = pixel_modem_owner::PreparedModemBoot::after_ipa_post_init(
        pixel_modem_owner::NoServices,
        ready,
    )
    .unwrap();
    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let before = m.clone();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let window = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let scm =
        unsafe { pixel_modem_owner::ScmResources::new(scratch.0.as_mut_ptr(), simulated_scm) }
            .unwrap();

    let result = unsafe {
        prepared.attach_after_glink(
            window,
            checkpoint,
            m.as_ptr(),
            m.as_mut_ptr().add(PART),
            doorbell.as_mut_ptr(),
            m.as_mut_ptr().cast(),
            Some(scm),
            |_| {},
        )
    };
    assert!(matches!(
        result,
        Err(qcom_modem_owner::Error::GenerationMismatch)
    ));
    assert_eq!(m, before);
    assert_eq!(lock, 0);
    assert_eq!(doorbell, [0; 4]);
}

#[test]
fn owner_native_prepared_ipa_session_composes_modem_init_client_before_control() {
    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let modem_client = pixel_modem_owner::IpaModemInitServices::new(
        pixel_modem_owner::NoServices,
        qcom_ipa_client::InitPermit::test_after_post_init(1),
    );
    let prepared = pixel_modem_owner::IpaServerServices::prepare_before_mpss(
        modem_client,
        qcom_ipa_service::ApReadyPermit::test_after_post_init(1),
    )
    .unwrap();
    let window = unsafe { Window::new(m.as_ptr(), &mut lock) };
    let scm =
        unsafe { pixel_modem_owner::ScmResources::new(scratch.0.as_mut_ptr(), simulated_scm) }
            .unwrap();
    let mut runtime = unsafe {
        pixel_modem_owner::attach_with_prepared_services(
            window,
            checkpoint,
            m.as_ptr(),
            m.as_mut_ptr().add(PART),
            doorbell.as_mut_ptr(),
            m.as_mut_ptr().cast(),
            Some(scm),
            prepared,
            |_| {},
        )
    }
    .unwrap();

    assert_eq!(
        runtime.ipa_client_state(),
        Some(qcom_ipa_client::State::Searching)
    );
    assert!(runtime.control_status().started);
    runtime.start().unwrap();
    assert_eq!(
        runtime.ipa_client_state(),
        Some(qcom_ipa_client::State::Searching)
    );
    assert_eq!(runtime.ipa_client_outcome(), None);
    SCM_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
}

#[test]
fn owner_native_ipa_modem_client_is_reserved_before_control_starts() {
    use qcom_ipa_client::State;

    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let services = pixel_modem_owner::IpaModemInitServices::new(
        pixel_modem_owner::NoServices,
        qcom_ipa_client::InitPermit::test_after_post_init(1),
    );
    let mut runtime = owner_attach_with_services(
        &mut m,
        checkpoint,
        &mut lock,
        &mut doorbell,
        &mut scratch,
        services,
    )
    .unwrap();

    assert_eq!(runtime.ipa_client_state(), Some(State::Searching));
    assert!(runtime.control_status().started);
    runtime.start().unwrap();
    assert_eq!(runtime.ipa_client_state(), Some(State::Searching));
    assert_eq!(runtime.ipa_client_outcome(), None);
    SCM_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
}

#[test]
fn owner_native_ipa_modem_client_rejects_a_stale_post_init_permit() {
    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let services = pixel_modem_owner::IpaModemInitServices::new(
        pixel_modem_owner::NoServices,
        qcom_ipa_client::InitPermit::test_after_post_init(2),
    );

    assert!(matches!(
        owner_attach_with_services(
            &mut m,
            checkpoint,
            &mut lock,
            &mut doorbell,
            &mut scratch,
            services,
        ),
        Err(qcom_modem_owner::Error::IpaReadinessRequired)
    ));
    assert_eq!(doorbell, [0; 4]);
    SCM_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
}

#[test]
fn owner_native_actual_handoff_transfers_cursor_then_runs_native_fifo() {
    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let before = m.clone();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let mut runtime =
        owner_attach(&mut m, checkpoint, &mut lock, &mut doorbell, &mut scratch).unwrap();
    assert_eq!(
        &m[DESCRIPTOR.offset + 8..DESCRIPTOR.offset + 12],
        &96u32.to_le_bytes()
    );
    for index in 0..m.len() {
        if !(DESCRIPTOR.offset + 8..DESCRIPTOR.offset + 12).contains(&index) {
            assert_eq!(m[index], before[index]);
        }
    }
    KERNEL_CALLS.with(|calls| calls.borrow_mut().clear());
    runtime.start().unwrap();
    assert!(scratch.0.iter().all(|byte| *byte == 0));
    KERNEL_CALLS.with(|calls| assert_eq!(*calls.borrow(), [1, 0, 2]));
    assert_eq!(
        &m[DESCRIPTOR.offset + 4..DESCRIPTOR.offset + 8],
        &144u32.to_le_bytes()
    );
    assert_eq!(
        &m[TX.offset + 72..TX.offset + 80],
        &[9, 0, 1, 0, 0, 0, 0, 0]
    );
    let hello: Vec<u8> = [
        1u32,
        2,
        1,
        0xffff_fffe,
        0,
        20,
        0,
        0xffff_fffe,
        2,
        0,
        0,
        0,
        0,
    ]
    .iter()
    .flat_map(|value| value.to_le_bytes())
    .collect();
    assert_eq!(&m[TX.offset + 88..TX.offset + 140], hello.as_slice());
    assert_eq!(
        &m[DESCRIPTOR.offset + 8..DESCRIPTOR.offset + 12],
        &120u32.to_le_bytes()
    );
    assert_eq!(doorbell, [0, 0, 0, 1 << 12]);
    assert_eq!(lock, 0);
    assert!(runtime.start().is_err());
    KERNEL_CALLS.with(|calls| assert_eq!(*calls.borrow(), [1, 0, 2]));
    drop(runtime);
}
#[test]
fn owner_native_bad_prelude_or_cursor_cannot_commit_a_tail() {
    for fault in 0..3 {
        let (mut m, checkpoint, rx) = native_owner_checkpoint();
        match fault {
            0 => m[rx.offset + 8 + 24 + 8] ^= 1,
            1 => w32(&mut m, DESCRIPTOR.offset + 12, u32::MAX),
            _ => w32(&mut m, DESCRIPTOR.offset + 16, 1),
        }
        let before = m.clone();
        let mut lock = 0;
        let mut doorbell = [0; 4];
        let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
        let result = owner_attach(&mut m, checkpoint, &mut lock, &mut doorbell, &mut scratch);
        assert!(result.is_err());
        assert_eq!(m, before);
        assert_eq!(doorbell, [0; 4]);
        assert_eq!(lock, 0);
    }
}
#[test]
fn owner_native_live_gate_stops_first_irq_work_after_ready_loss() {
    let (mut m, checkpoint, _) = native_owner_checkpoint();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let mut runtime =
        owner_attach(&mut m, checkpoint, &mut lock, &mut doorbell, &mut scratch).unwrap();
    let peer = smem_item(&m, 435).unwrap();
    w32(&mut m, peer.offset + 36, 1);
    let before = m.clone();
    KERNEL_CALLS.with(|calls| calls.borrow_mut().clear());
    assert!(runtime.start().is_err());
    assert_eq!(m, before);
    assert_eq!(doorbell, [0; 4]);
    KERNEL_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
    w32(&mut m, peer.offset + 36, 6);
    let restored = m.clone();
    runtime.notified(ChannelSet(3));
    KERNEL_CALLS.with(|calls| assert!(calls.borrow().is_empty()));
    assert_eq!(m, restored);
    drop(runtime);
}

#[test]
fn owner_native_initial_irq_unmask_waits_for_bounded_drain() {
    let (mut m, checkpoint, rx) = native_owner_checkpoint();
    // One initial intent record plus seventeen read notifications exceeds the
    // callback budget. Initial IRQs must stay masked across the local yield.
    for index in 0..17 {
        w32(&mut m, rx.offset + 120 + index * 8, 13);
        w32(&mut m, rx.offset + 124 + index * 8, 0);
    }
    w32(&mut m, DESCRIPTOR.offset + 12, 120 + 17 * 8);
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let mut runtime =
        owner_attach(&mut m, checkpoint, &mut lock, &mut doorbell, &mut scratch).unwrap();
    KERNEL_CALLS.with(|calls| calls.borrow_mut().clear());
    runtime.start().unwrap();
    KERNEL_CALLS.with(|calls| assert_eq!(*calls.borrow(), [2]));
    runtime.notified(ChannelSet(8));
    KERNEL_CALLS.with(|calls| assert_eq!(*calls.borrow(), [2, 1, 0, 2]));
}

#[test]
fn owner_native_irq_decodes_peer_hello_and_only_announces_bounded_servreg() {
    let (mut m, checkpoint, rx) = native_owner_checkpoint();
    let mut lock = 0;
    let mut doorbell = [0; 4];
    let mut scratch = ScmScratch([0; qcom_modem_pools::ARG_BYTES]);
    let mut runtime =
        owner_attach(&mut m, checkpoint, &mut lock, &mut doorbell, &mut scratch).unwrap();
    runtime.start().unwrap();
    let mut record = Vec::new();
    for value in [
        0x20009u32,
        1,
        52,
        0,
        1,
        2,
        0,
        0xffff_fffe,
        0,
        20,
        0xffff_ffff,
        0xffff_fffe,
        2,
        0,
        0,
        0,
        0,
        0,
    ] {
        record.extend(value.to_le_bytes());
    }
    assert_eq!(record.len(), 72);
    m[rx.offset + 120..rx.offset + 192].copy_from_slice(&record);
    w32(&mut m, rx.offset + 192, 0x20006);
    w32(&mut m, rx.offset + 196, 0);
    w32(&mut m, DESCRIPTOR.offset, 144); // Peer consumed the published HELLO.
    w32(&mut m, DESCRIPTOR.offset + 12, 200);
    KERNEL_CALLS.with(|calls| calls.borrow_mut().clear());
    // The initial post-ACK continuation is still pending and coalesces with IRQ0.
    runtime.notified(ChannelSet(9));
    let status = runtime.control_status();
    assert!(status.started && status.peer_hello && status.hello_sent);
    // Bit 2 is the bounded manifest-backed SERVREG map. Storage and heap
    // remain unavailable until their native data providers are attached.
    assert_eq!((status.ready_services, status.announced_services), (4, 4));
    assert_eq!(
        &m[DESCRIPTOR.offset + 4..DESCRIPTOR.offset + 8],
        &240u32.to_le_bytes() // Includes the bounded SERVREG announcement.
    );
    assert_eq!(
        &m[DESCRIPTOR.offset + 8..DESCRIPTOR.offset + 12],
        &200u32.to_le_bytes()
    );
    assert_eq!(&m[TX.offset + 144..TX.offset + 146], &5u16.to_le_bytes());
    assert_eq!(&m[TX.offset + 160..TX.offset + 162], &6u16.to_le_bytes());
    KERNEL_CALLS.with(|calls| assert_eq!(*calls.borrow(), [0, 2]));
    assert_eq!(lock, 0);
}
