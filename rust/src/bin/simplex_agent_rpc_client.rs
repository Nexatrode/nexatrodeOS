#![no_std]
#![no_main]

use core::convert::Infallible;
use personal_os_rust_server::simplex_ipc::{AgentPage, Operation, Status};
use sel4_microkit::{
    Channel, ChannelSet, Handler, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const AGENT_CHANNEL: Channel = Channel::new(0);
const REQUEST_ID: u32 = 1;

#[protection_domain]
fn init() -> RpcClient {
    let mut page = memory_region_symbol!(simplex_ipc_page: *mut AgentPage);
    let page_ref = unsafe { page.as_mut() };
    if !page_ref.begin(Operation::ListConnections, REQUEST_ID, &[]) {
        debug_println!("SIMPLEXIPC|RUST_CLIENT|FAIL_ENCODE");
    } else {
        AGENT_CHANNEL.notify();
        debug_println!("SIMPLEXIPC|RUST_CLIENT|REQUEST_SENT");
    }
    RpcClient { page }
}

struct RpcClient {
    page: core::ptr::NonNull<AgentPage>,
}

impl Handler for RpcClient {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if !channels.contains(AGENT_CHANNEL) {
            debug_println!("SIMPLEXIPC|RUST_CLIENT|FAIL_UNEXPECTED_NOTIFICATION");
            return Ok(());
        }
        let page = unsafe { self.page.as_ref() };
        match page.response(REQUEST_ID) {
            Some((Status::Ok, payload)) if payload == [0, 0] => {
                debug_println!("SIMPLEXIPC|RUST_CLIENT|PASS_LIST_CONNECTIONS|0");
            }
            _ => debug_println!("SIMPLEXIPC|RUST_CLIENT|FAIL_RESPONSE"),
        }
        Ok(())
    }

    fn protected(
        &mut self,
        _channel: Channel,
        _request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        debug_println!("SIMPLEXIPC|RUST_CLIENT|FAIL_UNEXPECTED_PROTECTED_CALL");
        Ok(MessageInfo::new(0, 0))
    }
}
