#![no_std]
#![no_main]

mod app_view;

use app_view::AppViewState;
use core::ptr::NonNull;
use personal_os_rust_server::settings::SETTINGS_TEXT;
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

#[protection_domain]
fn init() -> SettingsService {
    let view = memory_region_symbol!(settings_view: *mut AppViewState);
    unsafe { view.as_ptr().write(AppViewState::new()) };
    let mut service = SettingsService { view };
    if !service.publish() {
        debug_println!("UIAPP|SETTINGS|ERROR: invalid settings display snapshot");
    }
    debug_println!("UIAPP|SETTINGS|READY: local-only settings stub");
    service
}

struct SettingsService {
    view: NonNull<AppViewState>,
}

impl SettingsService {
    fn publish(&mut self) -> bool {
        unsafe { self.view.as_mut().set_display_text(SETTINGS_TEXT) }
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
        _channel: Channel,
        _request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        // Read-only stub: the trusted router never forwards input here.
        Ok(MessageInfo::new(0, 0))
    }
}
