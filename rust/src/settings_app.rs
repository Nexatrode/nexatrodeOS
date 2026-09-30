#![no_std]
#![no_main]

mod app_view;
mod settings_protocol;

use app_view::AppViewState;
use core::ptr::NonNull;
use personal_os_rust_server::settings::{SETTINGS_TEXT_CAPACITY, format_settings_text};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageRegisterValue, debug_println,
    get_mr, memory_region_symbol, protection_domain,
};
use settings_protocol::{REPLY_BAD_REQUEST, REPLY_OK, REQUEST_SET_ALERTS};

const SUPERVISOR: Channel = Channel::new(0);

#[protection_domain]
fn init() -> SettingsService {
    let view = memory_region_symbol!(settings_view: *mut AppViewState);
    unsafe { view.as_ptr().write(AppViewState::new()) };
    let mut service = SettingsService {
        view,
        rings: 0,
        sms: 0,
    };
    if !service.publish() {
        debug_println!("UIAPP|SETTINGS|ERROR: invalid settings display snapshot");
    }
    debug_println!("UIAPP|SETTINGS|READY: local-only settings stub");
    service
}

struct SettingsService {
    view: NonNull<AppViewState>,
    rings: MessageRegisterValue,
    sms: MessageRegisterValue,
}

impl SettingsService {
    fn publish(&mut self) -> bool {
        let mut buffer = [0u8; SETTINGS_TEXT_CAPACITY];
        let Some(length) = format_settings_text(self.rings as u64, self.sms as u64, &mut buffer)
        else {
            return false;
        };
        unsafe { self.view.as_mut().set_display_text(&buffer[..length]) }
    }

    fn set_alerts(&mut self, rings: MessageRegisterValue, sms: MessageRegisterValue) -> MessageInfo {
        self.rings = rings;
        self.sms = sms;
        if !self.publish() {
            debug_println!("UIAPP|SETTINGS|ERROR: alert display rejected");
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        debug_println!(
            "UIAPP|SETTINGS|ALERTS|RING={} SMS={}",
            self.rings,
            self.sms
        );
        MessageInfo::new(REPLY_OK, 0)
    }
}

impl Handler for SettingsService {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("UIAPP|SETTINGS|ERROR: unexpected notification");
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
