#![no_std]
#![no_main]

mod input_protocol;

use core::{
    cell::UnsafeCell,
    ptr,
    ptr::NonNull,
    sync::atomic::{AtomicU32, Ordering},
};
use input_protocol::{
    ABS_X, ABS_Y, BTN_LEFT, DEVICE_KEYBOARD, DEVICE_TABLET, DISPLAY_HEIGHT, DISPLAY_WIDTH,
    EVENT_ABS, EVENT_CAPACITY, EVENT_KEY, EVENT_SYN, EventRecord, EventRing, FocusTarget, KEY_ESC,
    KEY_TAB, KeyOwners, NavigationAction, REPLY_ROUTED_EVENT_OK, REQUEST_ROUTED_EVENT, SYN_REPORT,
    focus_value, next_focus_target, onscreen_key_code, onscreen_keyboard_area,
    tablet_axis_to_pixel, touch_calculator_action, touch_contacts_action, touch_focus_target,
    touch_navigation_action, touch_notes_action, touch_notes_list_action, valid_event,
};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageRegisterValue, debug_println,
    memory_region_symbol, protection_domain, set_mr,
};

const INPUT_NOTIFICATION: Channel = Channel::new(0);
const NOTES_INPUT: Channel = Channel::new(2);
const CONTACTS_INPUT: Channel = Channel::new(3);
const GPU_FOCUS_NOTIFICATION: Channel = Channel::new(4);
const CALCULATOR_INPUT: Channel = Channel::new(5);

#[protection_domain]
fn init() -> InputService {
    let ring = memory_region_symbol!(input_handoff: *mut EventRing);
    let ui_focus = memory_region_symbol!(ui_focus: *mut AtomicU32);
    let notes_touch_mode = memory_region_symbol!(notes_touch_mode: *mut AtomicU32);
    unsafe {
        ui_focus
            .as_ref()
            .store(focus_value(FocusTarget::None), Ordering::Release)
    };
    debug_println!(
        "UIINPUT|READY: bounded event service capacity={} focus=none touch-surfaces=notes,contacts,calculator,clock",
        EVENT_CAPACITY
    );
    InputService {
        ring,
        ui_focus,
        notes_touch_mode,
        focus: FocusTarget::None,
        tablet_x: None,
        tablet_y: None,
        key_owners: KeyOwners::new(),
        next_sequence: 0,
        accepted: 0,
        forwarded: 0,
        unfocused: 0,
        rejected: 0,
        call_errors: 0,
        seen_dropped: 0,
    }
}

struct InputService {
    ring: NonNull<EventRing>,
    ui_focus: NonNull<AtomicU32>,
    notes_touch_mode: NonNull<AtomicU32>,
    focus: FocusTarget,
    tablet_x: Option<u32>,
    tablet_y: Option<u32>,
    key_owners: KeyOwners,
    next_sequence: MessageRegisterValue,
    accepted: u32,
    forwarded: u32,
    unfocused: u32,
    rejected: u32,
    call_errors: u32,
    seen_dropped: u32,
}

impl InputService {
    fn route_event(&mut self, event: EventRecord) {
        if !valid_event(event) {
            self.rejected = self.rejected.saturating_add(1);
            return;
        }
        self.accepted = self.accepted.saturating_add(1);

        if event.device == DEVICE_TABLET && event.event_type == EVENT_ABS {
            match event.code {
                ABS_X => self.tablet_x = Some(event.value),
                ABS_Y => self.tablet_y = Some(event.value),
                _ => {}
            }
        }

        // Tab is a system-level focus shortcut. Consume press, repeat, and
        // release so it cannot be typed into either app's private field.
        if event.device == input_protocol::DEVICE_KEYBOARD
            && event.event_type == input_protocol::EVENT_KEY
            && event.code == KEY_TAB
        {
            if event.value == 1 {
                self.set_focus(next_focus_target(self.focus));
                debug_println!("UIROUTER|FOCUS: {} key=TAB", focus_name(self.focus));
            }
            return;
        }

        // Escape returns from an app to the launcher. Consume all key states
        // so the navigation key can never become app text.
        if event.device == DEVICE_KEYBOARD && event.event_type == EVENT_KEY && event.code == KEY_ESC
        {
            if event.value == 1 && self.focus != FocusTarget::None {
                self.set_focus(FocusTarget::None);
                debug_println!("UIROUTER|NAV: HOME key=ESC");
            }
            return;
        }

        // The home screen's cards open apps. Once inside an app, the top strip
        // provides direct Home/Notes/Contacts navigation.
        if let (Some(x), Some(y)) = (self.tablet_x, self.tablet_y) {
            if self.focus == FocusTarget::None {
                if let Some(focus) = touch_focus_target(event, x, y) {
                    self.set_focus(focus);
                    debug_println!(
                        "UIROUTER|FOCUS: {} touch=({}, {})",
                        focus_name(focus),
                        tablet_axis_to_pixel(x, DISPLAY_WIDTH),
                        tablet_axis_to_pixel(y, DISPLAY_HEIGHT)
                    );
                }
            } else if event.device == DEVICE_TABLET
                && event.event_type == EVENT_KEY
                && event.code == BTN_LEFT
            {
                let pixel_x = tablet_axis_to_pixel(x, DISPLAY_WIDTH);
                let pixel_y = tablet_axis_to_pixel(y, DISPLAY_HEIGHT);
                if let Some(action) = touch_navigation_action(pixel_x, pixel_y) {
                    if event.value == 1 {
                        let focus = match action {
                            NavigationAction::Home => FocusTarget::None,
                            NavigationAction::Open(target) => target,
                        };
                        self.set_focus(focus);
                        debug_println!("UIROUTER|NAV: {} touch", screen_name(focus));
                    }
                    return;
                }
                if let Some(code) = touch_contacts_action(self.focus, pixel_x, pixel_y) {
                    if event.value == 1 {
                        self.dispatch_virtual_key(code);
                        debug_println!("UIROUTER|VKEY: CONTACTS code={}", code);
                    }
                    return;
                }
                let notes_mode = unsafe { self.notes_touch_mode.as_ref().load(Ordering::Acquire) };
                if let Some(code) =
                    touch_notes_list_action(self.focus, notes_mode == 1, pixel_x, pixel_y)
                {
                    if event.value == 1 {
                        self.dispatch_virtual_key(code);
                        debug_println!("UIROUTER|VKEY: NOTES row-code={}", code);
                    }
                    return;
                }
                if let Some(code) = touch_notes_action(self.focus, notes_mode, pixel_x, pixel_y) {
                    if event.value == 1 {
                        self.dispatch_virtual_key(code);
                        debug_println!("UIROUTER|VKEY: NOTES code={}", code);
                    }
                    return;
                }
            }

            if event.device == DEVICE_TABLET
                && event.event_type == EVENT_KEY
                && event.code == BTN_LEFT
                && onscreen_keyboard_area(tablet_axis_to_pixel(y, DISPLAY_HEIGHT))
            {
                if event.value == 1 {
                    let pixel_x = tablet_axis_to_pixel(x, DISPLAY_WIDTH);
                    let pixel_y = tablet_axis_to_pixel(y, DISPLAY_HEIGHT);
                    let code = if self.focus == FocusTarget::Calculator {
                        touch_calculator_action(self.focus, pixel_x, pixel_y)
                    } else {
                        onscreen_key_code(pixel_x, pixel_y)
                    };
                    if let Some(code) = code {
                        self.dispatch_virtual_key(code);
                        debug_println!("UIROUTER|VKEY: {} code={}", focus_name(self.focus), code);
                    }
                }
                return;
            }
        }

        let target = self.key_owners.target_for_event(event, self.focus);
        self.dispatch_event(target, event);
    }

    fn set_focus(&mut self, focus: FocusTarget) {
        if focus != FocusTarget::None && focus != self.focus {
            // Let the app restore its own view before the GPU reads its
            // snapshot. This matters when switching to an app without typing.
            self.dispatch_event(
                focus,
                EventRecord {
                    device: DEVICE_KEYBOARD,
                    event_type: EVENT_SYN,
                    code: SYN_REPORT,
                    reserved: 0,
                    value: 0,
                },
            );
        }
        self.focus = focus;
        unsafe {
            self.ui_focus
                .as_ref()
                .store(focus_value(focus), Ordering::Release)
        };
        GPU_FOCUS_NOTIFICATION.notify();
    }

    fn dispatch_virtual_key(&mut self, code: u16) {
        let press = EventRecord {
            device: DEVICE_KEYBOARD,
            event_type: EVENT_KEY,
            code,
            reserved: 0,
            value: 1,
        };
        let target = self.key_owners.target_for_event(press, self.focus);
        self.dispatch_event(target, press);

        let release = EventRecord { value: 0, ..press };
        let target = self.key_owners.target_for_event(release, self.focus);
        self.dispatch_event(target, release);
    }

    fn dispatch_event(&mut self, target: FocusTarget, event: EventRecord) {
        let channel = match target {
            FocusTarget::None => {
                self.unfocused = self.unfocused.saturating_add(1);
                return;
            }
            FocusTarget::Notes => NOTES_INPUT,
            FocusTarget::Contacts => CONTACTS_INPUT,
            FocusTarget::Calculator => CALCULATOR_INPUT,
            // The Clock screen is a read-only snapshot published by the RTC
            // service; no app endpoint receives its input events.
            FocusTarget::Clock => return,
        };

        self.next_sequence = self.next_sequence.saturating_add(1);
        if forward_event(channel, event, self.next_sequence) {
            self.forwarded = self.forwarded.saturating_add(1);
            if event.device == DEVICE_KEYBOARD && event.event_type == EVENT_KEY && event.value != 0
            {
                GPU_FOCUS_NOTIFICATION.notify();
            }
        } else {
            self.call_errors = self.call_errors.saturating_add(1);
            debug_println!(
                "UIROUTER|ERROR: {} endpoint rejected input",
                focus_name(target)
            );
        }
    }
}

impl Handler for InputService {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if !channels.contains(INPUT_NOTIFICATION) {
            if channels.contains(GPU_FOCUS_NOTIFICATION) {
                return Ok(());
            }
            debug_println!("UIINPUT|ERROR: unexpected notification");
            return Ok(());
        }

        let ring = unsafe { self.ring.as_ref() };
        let head = ring.head.load(Ordering::Acquire);
        let mut tail = ring.tail.load(Ordering::Relaxed);
        let pending = head.wrapping_sub(tail);
        if pending > EVENT_CAPACITY {
            self.rejected = self.rejected.saturating_add(pending);
            ring.tail.store(head, Ordering::Release);
            debug_println!("UIINPUT|ERROR: corrupt ring distance={}", pending);
            return Ok(());
        }

        for _ in 0..pending {
            let slot = (tail & (EVENT_CAPACITY - 1)) as usize;
            // The producer publishes head with release ordering after writing
            // the record; this acquire load makes the slot contents visible.
            let event = unsafe { ptr::read_volatile(ring.events[slot].get()) };
            self.route_event(event);
            tail = tail.wrapping_add(1);
        }

        ring.tail.store(tail, Ordering::Release);
        let dropped = ring.dropped.load(Ordering::Acquire);
        let newly_dropped = dropped.wrapping_sub(self.seen_dropped);
        self.seen_dropped = dropped;
        debug_println!(
            "UIROUTER|BATCH: accepted={} forwarded={} unfocused={} rejected={} call_errors={} dropped={}",
            self.accepted,
            self.forwarded,
            self.unfocused,
            self.rejected,
            self.call_errors,
            newly_dropped
        );
        Ok(())
    }

    fn protected(
        &mut self,
        _channel: Channel,
        _request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        // No application or general-purpose service can select its own focus.
        Ok(MessageInfo::new(0, 0))
    }
}

fn forward_event(channel: Channel, event: EventRecord, sequence: MessageRegisterValue) -> bool {
    let header = ((event.device as MessageRegisterValue) << 32)
        | ((event.event_type as MessageRegisterValue) << 16)
        | event.code as MessageRegisterValue;
    set_mr(0, header);
    set_mr(1, event.value as MessageRegisterValue);
    set_mr(2, sequence);
    let reply = channel.pp_call(MessageInfo::new(REQUEST_ROUTED_EVENT, 3));
    reply.label() == REPLY_ROUTED_EVENT_OK && reply.count() == 0
}

const fn focus_name(focus: FocusTarget) -> &'static str {
    match focus {
        FocusTarget::None => "none",
        FocusTarget::Notes => "NOTES",
        FocusTarget::Contacts => "CONTACTS",
        FocusTarget::Calculator => "CALCULATOR",
        FocusTarget::Clock => "CLOCK",
    }
}

const fn screen_name(focus: FocusTarget) -> &'static str {
    match focus {
        FocusTarget::None => "HOME",
        other => focus_name(other),
    }
}

const _: () = assert!(core::mem::size_of::<UnsafeCell<input_protocol::EventRecord>>() == 12);
const _: () = assert!(core::mem::size_of::<EventRing>() == 400);
const _: () = assert!(core::mem::align_of::<EventRing>() == 4);
const _: () = assert!(BTN_LEFT == 272 && DEVICE_TABLET == 1 && EVENT_ABS == 3);
