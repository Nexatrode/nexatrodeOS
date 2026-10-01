#![no_std]
#![no_main]

mod app_view;
mod input_protocol;
mod modem_protocol;
mod phone_protocol;

use app_view::{AppViewState, KEY_BACKSPACE, KEY_ENTER};
use core::ptr::NonNull;
use input_protocol::{
    DEVICE_KEYBOARD, EVENT_KEY, KEY_F5, KEY_F6, KEY_F7, REPLY_ROUTED_EVENT_BAD,
    REPLY_ROUTED_EVENT_OK, REQUEST_ROUTED_EVENT, decode_routed_event,
};
use modem_protocol::{
    DIAL_MAX_BYTES, REPLY_OK as MODEM_OK, REQUEST_DIAL, PHONE_DIAL_CHANNEL,
};
use personal_os_rust_server::phone::{PHONE_TEXT_CAPACITY, Phone};
use phone_protocol::{REPLY_BAD_REQUEST, REPLY_OK, REQUEST_SET_ALERTS};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageRegisterValue, debug_println,
    get_mr, memory_region_symbol, protection_domain, set_mr,
};

const SUPERVISOR: Channel = Channel::new(0);
const MODEM_DIAL: Channel = Channel::new(PHONE_DIAL_CHANNEL);
const ROUTED_INPUT: Channel = Channel::new(2);

#[protection_domain]
fn init() -> PhoneService {
    let view = memory_region_symbol!(phone_view: *mut AppViewState);
    unsafe { view.as_ptr().write(AppViewState::new()) };
    let mut service = PhoneService {
        view,
        phone: Phone::new(),
        last_sequence: 0,
        input_events: 0,
    };
    if !service.publish() {
        debug_println!("UIAPP|PHONE|ERROR: invalid phone display snapshot");
    }
    debug_println!("UIAPP|PHONE|READY: fake-modem phone stub");
    service
}

struct PhoneService {
    view: NonNull<AppViewState>,
    phone: Phone,
    last_sequence: MessageRegisterValue,
    input_events: u32,
}

impl PhoneService {
    fn publish(&mut self) -> bool {
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        let Some(text) = self.phone.display(&mut buffer) else {
            return false;
        };
        unsafe { self.view.as_mut().set_display_text(text) }
    }

    fn set_alerts(&mut self, rings: MessageRegisterValue, sms: MessageRegisterValue) -> MessageInfo {
        self.phone.set_alerts(rings as u64, sms as u64);
        if !self.publish() {
            debug_println!("UIAPP|PHONE|ERROR: alert display rejected");
            return MessageInfo::new(REPLY_BAD_REQUEST, 0);
        }
        debug_println!(
            "UIAPP|PHONE|ALERTS|RING={} SMS={}",
            rings,
            sms
        );
        MessageInfo::new(REPLY_OK, 0)
    }

    fn pack_dial_digits(&self) -> bool {
        let digits = self.phone.digits();
        if digits.is_empty() || digits.len() > DIAL_MAX_BYTES {
            return false;
        }
        if !digits.iter().all(|byte| (b'0'..=b'9').contains(byte)) {
            return false;
        }
        let mut words = [0u8; DIAL_MAX_BYTES];
        words[..digits.len()].copy_from_slice(digits);
        set_mr(0, digits.len() as MessageRegisterValue);
        set_mr(
            1,
            u64::from_le_bytes(words[..8].try_into().unwrap_or([0; 8])) as MessageRegisterValue,
        );
        set_mr(
            2,
            u64::from_le_bytes(words[8..].try_into().unwrap_or([0; 8])) as MessageRegisterValue,
        );
        true
    }

    fn place_call(&mut self) -> bool {
        if !self.phone.can_call() {
            return false;
        }
        if !self.pack_dial_digits() {
            return false;
        }
        let reply = MODEM_DIAL.pp_call(MessageInfo::new(REQUEST_DIAL, 3));
        if reply.label() != MODEM_OK || reply.count() != 0 {
            debug_println!("UIAPP|PHONE|ERROR: modem rejected dial");
            return false;
        }
        self.phone.press_call()
    }

    fn apply_key(&mut self, code: u16) -> bool {
        match code {
            2..=10 => self.phone.press_digit((code - 1) as u8),
            11 => self.phone.press_digit(0),
            KEY_F5 | KEY_ENTER | 13 => self.place_call(),
            KEY_F6 | 111 => self.phone.press_clear(),
            KEY_F7 | KEY_BACKSPACE => self.phone.press_backspace(),
            _ => false,
        }
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
        if channel == SUPERVISOR {
            if request.label() == REQUEST_SET_ALERTS && request.count() == 2 {
                return Ok(self.set_alerts(get_mr(0), get_mr(1)));
            }
            return Ok(MessageInfo::new(REPLY_BAD_REQUEST, 0));
        }
        if channel != ROUTED_INPUT
            || request.label() != REQUEST_ROUTED_EVENT
            || request.count() != 3
        {
            return Ok(MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0));
        }
        let Some((event, sequence)) =
            decode_routed_event(get_mr(0) as u64, get_mr(1) as u64, get_mr(2) as u64)
        else {
            return Ok(MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0));
        };
        if sequence as MessageRegisterValue <= self.last_sequence {
            return Ok(MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0));
        }
        self.last_sequence = sequence as MessageRegisterValue;
        self.input_events = self.input_events.saturating_add(1);

        if event.device == DEVICE_KEYBOARD && event.event_type == EVENT_KEY && event.value != 0 {
            if self.apply_key(event.code) && !self.publish() {
                debug_println!("UIAPP|PHONE|ERROR: invalid phone display snapshot");
                return Ok(MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0));
            }
        }
        Ok(MessageInfo::new(REPLY_ROUTED_EVENT_OK, 0))
    }
}
