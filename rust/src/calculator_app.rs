#![no_std]
#![no_main]

mod app_view;
mod input_protocol;

use app_view::{AppViewState, KEY_BACKSPACE, KEY_ENTER};
use core::ptr::NonNull;
use input_protocol::{
    DEVICE_KEYBOARD, EVENT_KEY, KEY_F1, KEY_F2, KEY_F3, KEY_F4, KEY_F5, KEY_F6, KEY_F7,
    REPLY_ROUTED_EVENT_BAD, REPLY_ROUTED_EVENT_OK, REQUEST_ROUTED_EVENT, decode_routed_event,
};
use personal_os_rust_server::calculator::{Calculator, Operation};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageRegisterValue, debug_println,
    get_mr, memory_region_symbol, protection_domain,
};

const ROUTED_INPUT: Channel = Channel::new(2);

#[protection_domain]
fn init() -> CalculatorService {
    let view = memory_region_symbol!(calculator_view: *mut AppViewState);
    unsafe { view.as_ptr().write(AppViewState::new()) };
    let mut service = CalculatorService {
        view,
        calculator: Calculator::new(),
        last_sequence: 0,
        input_events: 0,
    };
    service.publish();
    debug_println!("UIAPP|CALCULATOR|READY: isolated integer calculator");
    service
}

struct CalculatorService {
    view: NonNull<AppViewState>,
    calculator: Calculator,
    last_sequence: MessageRegisterValue,
    input_events: u32,
}

impl CalculatorService {
    fn publish(&mut self) -> bool {
        unsafe {
            self.view
                .as_mut()
                .set_display_text(self.calculator.display())
        }
    }

    fn apply_key(&mut self, code: u16) -> bool {
        match code {
            2..=10 => self.calculator.press_digit((code - 1) as u8),
            11 => self.calculator.press_digit(0),
            KEY_F1 | 78 => self.calculator.press_operation(Operation::Add),
            KEY_F2 | 12 | 74 => self.calculator.press_operation(Operation::Subtract),
            KEY_F3 | 55 => self.calculator.press_operation(Operation::Multiply),
            KEY_F4 | 98 => self.calculator.press_operation(Operation::Divide),
            KEY_F5 | KEY_ENTER | 13 => self.calculator.equals(),
            KEY_F6 | 111 => {
                self.calculator.clear();
                true
            }
            KEY_F7 | KEY_BACKSPACE => self.calculator.backspace(),
            _ => false,
        }
    }
}

impl Handler for CalculatorService {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("UIAPP|CALCULATOR|ERROR: unexpected notification");
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
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
                debug_println!("UIAPP|CALCULATOR|ERROR: invalid calculator display snapshot");
                return Ok(MessageInfo::new(REPLY_ROUTED_EVENT_BAD, 0));
            }
        }
        Ok(MessageInfo::new(REPLY_ROUTED_EVENT_OK, 0))
    }
}
