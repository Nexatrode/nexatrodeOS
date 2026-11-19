//! Compile the actual adapter against a fake seL4 IPC boundary. This verifies
//! capability slot, all eight argument/reply registers and invocation errors;
//! it cannot establish the secure firmware's acceptance or memory behavior.
#![allow(non_camel_case_types, non_snake_case)]
extern crate self as personal_os_rust_server;
extern crate self as sel4;

#[path = "../rust/src/pixel_pas_metadata.rs"]
mod pixel_pas_metadata;
#[path = "../rust/src/pixel_pas_smc.rs"]
mod pixel_pas_smc;
#[path = "../rust/src/qcom_mdt.rs"]
mod qcom_mdt;
#[path = "../rust/src/qcom_pas_init.rs"]
mod qcom_pas_init;
#[path = "../rust/src/qcom_pas_mem_setup.rs"]
mod qcom_pas_mem_setup;
#[path = "../rust/src/qcom_pas_metadata.rs"]
mod qcom_pas_metadata;

use std::cell::RefCell;
thread_local! { static CALLS: RefCell<(Vec<[u64;8]>,Option<usize>)> = const { RefCell::new((Vec::new(),None)) }; }
pub mod sys {
    #[derive(Default)]
    pub struct seL4_ARM_SMCContext {
        pub x0: u64,
        pub x1: u64,
        pub x2: u64,
        pub x3: u64,
        pub x4: u64,
        pub x5: u64,
        pub x6: u64,
        pub x7: u64,
    }
    pub struct seL4_IPCBuffer;
    impl seL4_IPCBuffer {
        pub fn seL4_ARM_SMC_Call(
            &mut self,
            service: u64,
            a: &seL4_ARM_SMCContext,
            r: &mut seL4_ARM_SMCContext,
        ) -> u32 {
            assert_eq!(service, 7);
            super::CALLS.with_borrow_mut(|(calls, fault)| {
                let index = calls.len();
                calls.push([a.x0, a.x1, a.x2, a.x3, a.x4, a.x5, a.x6, a.x7]);
                *r = seL4_ARM_SMCContext {
                    x0: 0,
                    x1: u64::from(!matches!(index, 2 | 4)),
                    x2: 0x123456789abcdef0,
                    x3: 3,
                    x4: 4,
                    x5: 5,
                    x6: 6,
                    x7: 7,
                };
                if *fault == Some(index) { 6 } else { 0 }
            })
        }
    }
}
pub struct IpcBuffer(sys::seL4_IPCBuffer);
impl IpcBuffer {
    pub fn inner_mut(&mut self) -> &mut sys::seL4_IPCBuffer {
        &mut self.0
    }
}
pub fn with_ipc_buffer_mut<T>(f: impl FnOnce(&mut IpcBuffer) -> T) -> T {
    f(&mut IpcBuffer(sys::seL4_IPCBuffer))
}

fn prepared() -> qcom_pas_metadata::Prepared {
    qcom_pas_metadata::Prepared {
        metadata_physical: 0x80001000,
        metadata_length: 9100,
        program_headers: 31,
        loadable_segments: 29,
        linked_start: 0x8b000000,
        linked_end: 0x94800000,
    }
}
#[test]
fn auth_and_availability_envelopes_use_the_same_exact_smc_boundary() {
    use qcom_pas_init::{Request,Transport};
    CALLS.with_borrow_mut(|v| *v = (Vec::new(), None));
    let requests = [Request::auth_query(),Request::shutdown_query(),Request::modem_auth_reset()];
    let mut adapter=pixel_pas_smc::Smc;
    for request in &requests {
        let reply=adapter.invoke(request).unwrap();
        assert_eq!(reply[2],0x123456789abcdef0);
        assert_eq!(&reply[3..],&[3,4,5,6,7]);
    }
    CALLS.with_borrow(|(calls,_)| assert_eq!(*calls,requests.map(|r|*r.words()).to_vec()));
}
#[test]
fn actual_adapter_marshals_all_registers_through_only_microkit_smc_slot() {
    CALLS.with_borrow_mut(|v| *v = (Vec::new(), None));
    let mut s = qcom_pas_init::InitOnce::new(prepared()).unwrap();
    let mut io = pixel_pas_smc::Smc;
    s.check(&mut io, |_| {}).unwrap();
    s.initialize(&mut io, prepared(), |_| {}).unwrap();
    assert!(s.report().passed());
    CALLS.with_borrow(|(calls, _)| {
        assert_eq!(
            calls,
            &[
                [0xc2000601, 1, 0x82000601, 0, 0, 0, 0, 0],
                [0x42000601, 1, 0x02000201, 0, 0, 0, 0, 0],
                [0x42000201, 0x82, 4, 0x80001000, 0, 0, 0, 0]
            ]
        )
    });
    assert_eq!(
        s.report().replies[2].unwrap(),
        [0, 0, 0x123456789abcdef0, 3, 4, 5, 6, 7]
    );
}
#[test]
fn actual_invocation_error_never_publishes_success_like_reply_registers() {
    for at in 0..3 {
        CALLS.with_borrow_mut(|v| *v = (Vec::new(), Some(at)));
        let mut s = qcom_pas_init::InitOnce::new(prepared()).unwrap();
        let mut io = pixel_pas_smc::Smc;
        let result = s
            .check(&mut io, |_| {})
            .and_then(|()| s.initialize(&mut io, prepared(), |_| {}));
        assert_eq!(result, Err(qcom_pas_init::Error::Kernel));
        assert_eq!(s.report().kernel_error, Some(6));
        assert_eq!(s.report().replies[at], None);
        assert_eq!(s.report().returned_mask, (1 << at) - 1);
        CALLS.with_borrow(|(calls, _)| assert_eq!(calls.len(), at + 1));
    }
}

#[test]
fn actual_adapter_marshals_fixed_memory_setup_after_consumed_init_and_propagates_errors() {
    use qcom_pas_mem_setup::{Error, MemSetupOnce};
    // Simulate the user's reported post-INIT snapshot through the actual byte
    // adapter/inspector; this does not simulate secure firmware behavior.
    let data = std::fs::read(std::env::var("SUNFISH_MODEM_MDT_PATH").unwrap()).unwrap();
    let mut ram = [0u8; qcom_pas_metadata::MAPPING_SIZE];
    let mut buffer = unsafe { pixel_pas_metadata::Buffer::new(ram.as_mut_ptr()) };
    let prepared_buffer = qcom_pas_metadata::prepare(&mut buffer, 0x80000000, &data).unwrap();
    assert_eq!(prepared_buffer, prepared());
    ram[qcom_pas_metadata::PAGE..qcom_pas_metadata::PAGE + data.len()].fill(0);
    let consumed = qcom_pas_metadata::inspect(&mut buffer, 0x80000000, &data).unwrap();
    assert!(qcom_pas_mem_setup::observed_consumed_pattern(
        &consumed,
        prepared_buffer
    ));
    for fault in [None, Some(3), Some(4)] {
        CALLS.with_borrow_mut(|v| *v = (Vec::new(), fault));
        let mut io = pixel_pas_smc::Smc;
        let mut init = qcom_pas_init::InitOnce::new(prepared()).unwrap();
        init.check(&mut io, |_| {}).unwrap();
        init.initialize(&mut io, prepared(), |_| {}).unwrap();
        let mut mem = MemSetupOnce::after_init(init, &consumed).unwrap();
        let result = mem
            .check(&mut io, |_| {})
            .and_then(|()| mem.setup(&mut io, &consumed, |_| {}));
        if let Some(at) = fault {
            assert_eq!(result, Err(Error::Kernel));
            assert_eq!(mem.report().replies[at - 3], None);
            CALLS.with_borrow(|(calls, _)| assert_eq!(calls.len(), at + 1));
        } else {
            assert!(result.is_ok() && mem.report().passed());
            assert_eq!(
                mem.report().replies[1].unwrap(),
                [0, 0, 0x123456789abcdef0, 3, 4, 5, 6, 7]
            );
            CALLS.with_borrow(|(calls, _)| {
                assert_eq!(calls.len(), 5);
                assert_eq!(calls[3], [0x42000601, 1, 0x02000202, 0, 0, 0, 0, 0]);
                assert_eq!(calls[4], [0x42000202, 3, 4, 0x8b000000, 0x9800000, 0, 0, 0]);
            });
        }
    }
}
