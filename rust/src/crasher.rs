#![no_std]
#![no_main]
#![feature(used_with_arg)]

mod vault_protocol;

use core::sync::atomic::{AtomicUsize, Ordering};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, protection_domain, set_mr,
};
use vault_protocol::{
    CRASHER_CHANNEL as VAULT, DENIED_WRITE_VALUE, REPLY_DENIED as VAULT_DENIED,
    REQUEST_READ as VAULT_READ, REQUEST_WRITE as VAULT_WRITE,
};

const CRASH_TRIGGER: Channel = Channel::new(0);
const SERVER_PRIVATE_ADDRESS: usize = 0x4000_0000;
static CRASH_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

#[protection_domain]
fn init() -> RustCrasher {
    CRASH_ATTEMPTS.store(0, Ordering::Relaxed);
    RustCrasher
}

struct RustCrasher;

impl Handler for RustCrasher {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if !channels.contains(CRASH_TRIGGER) {
            debug_println!("FAIL: unexpected crasher notification");
            return Ok(());
        }

        let attempt = CRASH_ATTEMPTS
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        debug_println!("CRASHER: attempt {}; private state initialized", attempt);

        let read_reply = VAULT.pp_call(MessageInfo::new(VAULT_READ, 0));
        let read_denied = read_reply.label() == VAULT_DENIED && read_reply.count() == 0;
        debug_println!(
            "{}vault denied untrusted child read",
            if read_denied { "PASS: " } else { "FAIL: " }
        );

        set_mr(0, 0);
        set_mr(1, DENIED_WRITE_VALUE);
        let write_reply = VAULT.pp_call(MessageInfo::new(VAULT_WRITE, 2));
        let write_denied = write_reply.label() == VAULT_DENIED && write_reply.count() == 0;
        debug_println!(
            "{}vault denied untrusted child write",
            if write_denied { "PASS: " } else { "FAIL: " }
        );

        #[cfg(feature = "budget-pressure")]
        {
            debug_println!("CRASHER: CPU budget pressure test started");
            loop {
                core::hint::spin_loop();
            }
        }

        #[cfg(not(feature = "budget-pressure"))]
        {
            debug_println!("CRASHER: attempting forbidden write at 0x40000000");
            trigger_forbidden_write();
            debug_println!("FAIL: forbidden write unexpectedly succeeded");
            Ok(())
        }
    }
}

#[inline(never)]
fn trigger_forbidden_write() {
    // This address is intentionally not a Rust pointer: the crasher has no
    // mapping there. Emit the AArch64 store directly to test kernel isolation.
    unsafe {
        core::arch::asm!(
            "str xzr, [{address}]",
            address = in(reg) SERVER_PRIVATE_ADDRESS,
            options(nostack, preserves_flags)
        );
    }
}

// The Rust runtime initializes process-global hooks on its first entry. After
// a fault, the supervisor restarts here instead of re-running that initializer.
#[unsafe(no_mangle)]
pub extern "C" fn rust_crasher_restart_entry() -> ! {
    let attempt = CRASH_ATTEMPTS
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    debug_println!(
        "CRASHER: restart attempt {}; private state retained",
        attempt
    );
    trigger_forbidden_write();
    debug_println!("FAIL: forbidden write unexpectedly succeeded");
    loop {
        core::hint::spin_loop();
    }
}

#[used(linker)]
#[unsafe(link_section = ".data")]
static RUST_CRASHER_RESTART_ENTRY_REFERENCE: extern "C" fn() -> ! = rust_crasher_restart_entry;
