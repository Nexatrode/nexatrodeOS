#![no_std]
#![no_main]

use core::ptr;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageLabel, MessageRegisterValue,
    debug_println, get_mr, memory_region_symbol, protection_domain, set_mr,
};

// These labels intentionally match the wire protocol in src/protocol.h. This
// service is called by the Rust supervisor over a Microkit protected call.
const CLIENT: Channel = Channel::new(0);
const REQUEST_ADD: MessageLabel = 1;
const REQUEST_COUNT: MessageLabel = 2;
const REQUEST_GUARD: MessageLabel = 3;
const REPLY_OK: MessageLabel = 0x100;
const REPLY_BAD_REQUEST: MessageLabel = 0x101;
const REPLY_OVERFLOW: MessageLabel = 0x102;
const GUARD_VALUE: MessageRegisterValue = 0x51A7E123;

#[protection_domain]
fn init() -> RustServer {
    let state = memory_region_symbol!(server_private_start: *mut ServerState).as_ptr();
    // Microkit maps this writable region only into this protection domain.
    unsafe {
        ptr::write_volatile(
            state,
            ServerState {
                guard: GUARD_VALUE,
                completed_additions: 0,
            },
        );
    }
    debug_println!("SERVER: ready; state page mapped only in server");
    RustServer { state }
}

#[repr(C)]
struct ServerState {
    guard: MessageRegisterValue,
    completed_additions: MessageRegisterValue,
}

struct RustServer {
    state: *mut ServerState,
}

impl RustServer {
    fn guard(&self) -> MessageRegisterValue {
        // The pointer comes from Microkit's setvar_vaddr mapping above.
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.state).guard)) }
    }

    fn completed_additions(&self) -> MessageRegisterValue {
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.state).completed_additions)) }
    }

    fn record_addition(&mut self) {
        let next = self.completed_additions().wrapping_add(1);
        unsafe {
            ptr::write_volatile(ptr::addr_of_mut!((*self.state).completed_additions), next);
        }
    }
}

impl Handler for RustServer {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("FAIL: unexpected Rust server notification");
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel != CLIENT {
            return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
        }

        match (request.label(), request.count()) {
            (REQUEST_COUNT, 0) => {
                set_mr(0, self.completed_additions());
                Ok(MessageInfo::new(REPLY_OK, 1))
            }
            (REQUEST_GUARD, 0) => {
                set_mr(0, self.guard());
                Ok(MessageInfo::new(REPLY_OK, 1))
            }
            (REQUEST_ADD, 2) => {
                let left = get_mr(0);
                let right = get_mr(1);
                match left.checked_add(right) {
                    Some(sum) => {
                        self.record_addition();
                        set_mr(0, sum);
                        Ok(MessageInfo::new(REPLY_OK, 1))
                    }
                    None => Ok(MessageInfo::new(REPLY_OVERFLOW, 0)),
                }
            }
            _ => Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0)),
        }
    }
}
