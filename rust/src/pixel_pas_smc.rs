//! Private standalone-PD SMC adapter. Microkit places its sole SMC cap at7.
//! No client IPC accepts raw registers; the core constructs sealed requests.
use personal_os_rust_server::qcom_pas_init::{Request, Transport};

pub struct Smc;
impl Transport for Smc {
    fn invoke(&mut self, request: &Request) -> Result<[u64; 8], u64> {
        let r = request.words();
        let args = sel4::sys::seL4_ARM_SMCContext {
            x0: r[0],
            x1: r[1],
            x2: r[2],
            x3: r[3],
            x4: r[4],
            x5: r[5],
            x6: r[6],
            x7: r[7],
        };
        let mut reply = sel4::sys::seL4_ARM_SMCContext::default();
        sync();
        let error = sel4::with_ipc_buffer_mut(|ipc| {
            ipc.inner_mut().seL4_ARM_SMC_Call(7, &args, &mut reply)
        });
        sync();
        if error != 0 {
            return Err(error as u64);
        }
        Ok([
            reply.x0, reply.x1, reply.x2, reply.x3, reply.x4, reply.x5, reply.x6, reply.x7,
        ])
    }
}
fn sync() {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}
