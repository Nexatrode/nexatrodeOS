#![no_std]
#![no_main]

mod app_view;
mod phone_protocol;

use app_view::AppViewState;
use core::ptr::NonNull;
use personal_os_rust_server::phone::{PHONE_TEXT_CAPACITY, format_phone_text};
use phone_protocol::{REPLY_BAD_REQUEST, REPLY_OK, REQUEST_SET_ALERTS};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageRegisterValue, debug_println,
    get_mr, memory_region_symbol, protection_domain,
};

const SUPERVISOR: Channel = Channel::new(0);

#[protection_domain]
fn init() -> PhoneService {
    let view = memory_region_symbol!(phone_view: *mut AppViewState);
    unsafe { view.as_ptr().write(AppViewState::new()) };
    let mut service = PhoneService {
        view,
        rings: 0,
        sms: 0,
    };
    if !service.publish() {
        debug_println!("UIAPP|PHONE|ERROR: invalid phone display snapshot");
    }
    debug_println!("UIAPP|PHONE|READY: fake-modem phone stub");
    service
}

struct PhoneService {
    view: NonNull<AppViewState>,
    rings: MessageRegisterValue,
    sms: MessageRegisterValue,
}

impl PhoneService {
    fn publish(&mut self) -> bool {
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        let Some(length) = format_phone_text(self.rings as u64, self.sms as u64, &mut buffer) else {
            return false;
        };
        unsafe { self.view.as_mut().set_display_text(&buffer[..length]) }
    }

    fn set_alerts(&mut self, rings: MessageRegisterValue, sms: MessageRegisterValue) -> MessageInfo {
        self.rings = rings;
        self.sms = sms;
        if !self.publish() {
            debug_println!("UIAPP|PHONE|ERROR: alert display rejected");
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        debug_println!(
            "UIAPP|PHONE|ALERTS|RING={} SMS={}",
            self.rings,
            self.sms
        );
        MessageInfo::new(REPLY_OK, 0)
    }
}

impl Handler for PhoneService {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("UIAPP|PHONE|ERROR: unexpected notification");
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel != SUPERVISOR {
            return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
        }
        if request.label() == REQUEST_SET_ALERTS && request.count() == 2 {
            return Ok(self.set_alerts(get_mr(0), get_mr(1)));
        }
        // Read-only for other callers; input router never forwards keys here.
        Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0))
    }
}
