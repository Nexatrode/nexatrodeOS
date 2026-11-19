//! Numeric root lifecycle with the exact root Gate, actual sealed loader/ready
//! receipts, and modeled symbol injection. No fixed VA dereference or SMC/IRQ.
include!("sunfish_mpss_linux_glink_test.rs");
extern crate self as personal_os_rust_server;
extern crate self as sel4;
extern crate self as sel4_microkit;
#[allow(dead_code)]
#[path = "../rust/src/pixel_linux_rmtfs.rs"]
mod pixel_linux_rmtfs;
#[path = "../rust/src/pixel_mpss_auth_irq.rs"]
mod pixel_mpss_auth_irq;
#[path = "../rust/src/sunfish_mpss_live_domain.rs"]
pub mod sunfish_mpss_live_domain;
#[path = "../rust/src/qcom_scm_assign.rs"]
pub mod qcom_scm_assign;
pub static BAD_ROOT_MAP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub struct Symbol(*mut u8);
impl Symbol {
    pub fn as_ptr(self) -> *mut u8 {
        self.0
    }
}
pub fn root_symbol(name: &str) -> Symbol {
    Symbol(match name {
        "native_rmtfs" => 0x77001000,
        "rmtfs_scm" => 0x77301000,
        _ => panic!("unexpected root view"),
    } as *mut u8)
}
pub fn root_physical(name: &str) -> &'static usize {
    match name {
        "native_rmtfs_paddr" => {
            if BAD_ROOT_MAP.load(std::sync::atomic::Ordering::Acquire) {
                &0xfde02000
            } else {
                &0xfde01000
            }
        }
        "rmtfs_scm_paddr" => &0x81000000,
        _ => panic!("unexpected physical injection"),
    }
}
#[macro_export]
macro_rules! memory_region_symbol {
    ($name:ident:*mut u8) => {
        $crate::root_symbol(stringify!($name))
    };
}
#[macro_export]
macro_rules! var {
    ($name:ident:usize=0) => {
        $crate::root_physical(stringify!($name))
    };
}
pub mod sys {
    #[allow(non_camel_case_types)]
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
}
pub struct Ipc;
impl Ipc {
    pub fn inner_mut(&mut self) -> &mut Self {
        self
    }
    #[allow(non_snake_case)]
    pub fn seL4_ARM_SMC_Call(
        &mut self,
        _: u64,
        _: &sys::seL4_ARM_SMCContext,
        _: &mut sys::seL4_ARM_SMCContext,
    ) -> u64 {
        panic!("host root test must not invoke SMC")
    }
}
pub fn with_ipc_buffer_mut<T>(_: impl FnOnce(&mut Ipc) -> T) -> T {
    panic!("host root test must not access IPC buffer")
}
