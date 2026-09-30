//! Bounded handoff format and validation for untrusted virtual input events.
#![allow(dead_code)]

pub const EVENT_CAPACITY: u32 = 256;
pub const DEVICE_KEYBOARD: u16 = 0;
pub const DEVICE_TABLET: u16 = 1;

pub const EVENT_SYN: u16 = 0;
pub const EVENT_KEY: u16 = 1;
pub const EVENT_ABS: u16 = 3;
pub const SYN_REPORT: u16 = 0;
pub const ABS_X: u16 = 0;
pub const ABS_Y: u16 = 1;
pub const KEY_TAB: u16 = 15;
pub const KEY_ESC: u16 = 1;
pub const KEY_UP: u16 = 103;
pub const KEY_DOWN: u16 = 108;
pub const KEY_INSERT: u16 = 110;
pub const KEY_DELETE: u16 = 111;
pub const KEY_F1: u16 = 59;
pub const KEY_F2: u16 = 60;
pub const KEY_F3: u16 = 61;
pub const KEY_F4: u16 = 62;
pub const KEY_F5: u16 = 63;
pub const KEY_F6: u16 = 64;
pub const KEY_F7: u16 = 65;
pub const BTN_LEFT: u16 = 272;
pub const KEY_MAX: u16 = 0x2ff;
pub const TABLET_AXIS_MAX: u32 = 0x7fff;
pub const DISPLAY_WIDTH: u32 = 300;
pub const DISPLAY_HEIGHT: u32 = 400;
/// Known cursor park used by QEMU frame compares (footer chrome).
pub const POINTER_PARK_X: u32 = 290;
pub const POINTER_PARK_Y: u32 = 390;
pub const KEYBOARD_TOP: u32 = 220;
pub const KEYBOARD_BOTTOM: u32 = 360;
pub const FOCUS_NONE: u32 = 0;
pub const FOCUS_NOTES: u32 = 1;
pub const FOCUS_CONTACTS: u32 = 2;
pub const FOCUS_CALCULATOR: u32 = 3;
pub const FOCUS_CLOCK: u32 = 4;
pub const FOCUS_CALENDAR: u32 = 5;
pub const FOCUS_MESSAGES: u32 = 6;
pub const FOCUS_SETTINGS: u32 = 7;
pub const FOCUS_PHONE: u32 = 8;

pub const fn focus_value(focus: FocusTarget) -> u32 {
    match focus {
        FocusTarget::None => FOCUS_NONE,
        FocusTarget::Notes => FOCUS_NOTES,
        FocusTarget::Contacts => FOCUS_CONTACTS,
        FocusTarget::Calculator => FOCUS_CALCULATOR,
        FocusTarget::Clock => FOCUS_CLOCK,
        FocusTarget::Calendar => FOCUS_CALENDAR,
        FocusTarget::Messages => FOCUS_MESSAGES,
        FocusTarget::Settings => FOCUS_SETTINGS,
        FocusTarget::Phone => FOCUS_PHONE,
    }
}

pub const fn next_focus_target(focus: FocusTarget) -> FocusTarget {
    match focus {
        FocusTarget::None | FocusTarget::Settings => FocusTarget::Notes,
        FocusTarget::Notes => FocusTarget::Contacts,
        FocusTarget::Contacts => FocusTarget::Calculator,
        FocusTarget::Calculator => FocusTarget::Clock,
        FocusTarget::Clock => FocusTarget::Calendar,
        FocusTarget::Calendar => FocusTarget::Messages,
        FocusTarget::Messages => FocusTarget::Phone,
        FocusTarget::Phone => FocusTarget::Settings,
    }
}

pub const REQUEST_ROUTED_EVENT: u64 = 0x310;
pub const REPLY_ROUTED_EVENT_OK: u64 = 0x311;
pub const REPLY_ROUTED_EVENT_BAD: u64 = 0x312;

/// Shared tablet cursor published by the trusted router for the GPU client.
#[repr(C)]
pub struct UiPointerState {
    pub revision: core::sync::atomic::AtomicU32,
    pub x: u32,
    pub y: u32,
    pub visible: u32,
}

impl UiPointerState {
    pub const fn new() -> Self {
        Self {
            revision: core::sync::atomic::AtomicU32::new(0),
            x: 0,
            y: 0,
            visible: 0,
        }
    }

    pub fn publish(&mut self, x: u32, y: u32) -> bool {
        let x = x.min(DISPLAY_WIDTH.saturating_sub(1));
        let y = y.min(DISPLAY_HEIGHT.saturating_sub(1));
        if self.visible == 1 && self.x == x && self.y == y {
            return false;
        }
        let odd = self.revision.load(core::sync::atomic::Ordering::Relaxed).wrapping_add(1) | 1;
        self.revision
            .store(odd, core::sync::atomic::Ordering::SeqCst);
        self.x = x;
        self.y = y;
        self.visible = 1;
        self.revision
            .store(odd.wrapping_add(1), core::sync::atomic::Ordering::SeqCst);
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusTarget {
    None,
    Notes,
    Contacts,
    Calculator,
    Clock,
    Calendar,
    Messages,
    Settings,
    Phone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationAction {
    Home,
    Open(FocusTarget),
}

pub struct KeyOwners {
    owners: [FocusTarget; KEY_MAX as usize + 1],
}

impl KeyOwners {
    pub const fn new() -> Self {
        Self {
            owners: [FocusTarget::None; KEY_MAX as usize + 1],
        }
    }

    pub fn target_for_event(&mut self, event: EventRecord, focused: FocusTarget) -> FocusTarget {
        if event.device != DEVICE_KEYBOARD || event.event_type != EVENT_KEY {
            return focused;
        }
        let owner = &mut self.owners[event.code as usize];
        match event.value {
            1 => {
                if *owner == FocusTarget::None {
                    *owner = focused;
                }
                *owner
            }
            2 => *owner,
            0 => {
                let previous_owner = *owner;
                *owner = FocusTarget::None;
                previous_owner
            }
            _ => FocusTarget::None,
        }
    }
}

pub const fn tablet_axis_to_pixel(value: u32, pixel_count: u32) -> u32 {
    value * (pixel_count - 1) / TABLET_AXIS_MAX
}

pub const fn touch_focus_target(
    event: EventRecord,
    tablet_x: u32,
    tablet_y: u32,
) -> Option<FocusTarget> {
    if event.device != DEVICE_TABLET
        || event.event_type != EVENT_KEY
        || event.code != BTN_LEFT
        || event.value != 1
    {
        return None;
    }
    let x = tablet_axis_to_pixel(tablet_x, DISPLAY_WIDTH);
    let y = tablet_axis_to_pixel(tablet_y, DISPLAY_HEIGHT);
    match (x, y) {
        (12..=147, 50..=93) => Some(FocusTarget::Notes),
        (153..=288, 50..=93) => Some(FocusTarget::Contacts),
        (12..=147, 98..=141) => Some(FocusTarget::Calculator),
        (153..=288, 98..=141) => Some(FocusTarget::Clock),
        (12..=147, 146..=189) => Some(FocusTarget::Calendar),
        (153..=288, 146..=189) => Some(FocusTarget::Messages),
        (12..=147, 194..=219) => Some(FocusTarget::Phone),
        (153..=288, 194..=219) => Some(FocusTarget::Settings),
        _ => None,
    }
}

/// Map the visible app-page navigation strip to a home or app-switch action.
/// The launcher itself has no strip, so callers should use this only while an
/// app is open.
pub const fn touch_navigation_action(x: u32, y: u32) -> Option<NavigationAction> {
    if y < 12 || y > 43 {
        return None;
    }
    if x >= 12 && x <= 43 {
        Some(NavigationAction::Home)
    } else if x >= 46 && x <= 69 {
        Some(NavigationAction::Open(FocusTarget::Notes))
    } else if x >= 72 && x <= 95 {
        Some(NavigationAction::Open(FocusTarget::Contacts))
    } else if x >= 98 && x <= 121 {
        Some(NavigationAction::Open(FocusTarget::Calculator))
    } else if x >= 124 && x <= 147 {
        Some(NavigationAction::Open(FocusTarget::Clock))
    } else if x >= 150 && x <= 173 {
        Some(NavigationAction::Open(FocusTarget::Calendar))
    } else if x >= 176 && x <= 199 {
        Some(NavigationAction::Open(FocusTarget::Messages))
    } else if x >= 202 && x <= 225 {
        Some(NavigationAction::Open(FocusTarget::Phone))
    } else if x >= 228 && x <= 251 {
        Some(NavigationAction::Open(FocusTarget::Settings))
    } else {
        None
    }
}

/// Map the visible Calculator keypad to ordinary app key events. Gaps and
/// padding do not produce input, and the router calls this only for Calculator.
pub const fn touch_calculator_action(focus: FocusTarget, x: u32, y: u32) -> Option<u16> {
    if !matches!(focus, FocusTarget::Calculator) || x < 16 || x >= 284 {
        return None;
    }
    if x < 277 && y >= 330 && y < 352 {
        return Some(KEY_F5);
    }
    let row = if y >= 226 && y < 248 {
        0
    } else if y >= 252 && y < 274 {
        1
    } else if y >= 278 && y < 300 {
        2
    } else if y >= 304 && y < 326 {
        3
    } else {
        return None;
    };
    let column = (x - 16) / 67;
    let within = (x - 16) % 67;
    if column >= 4 || within >= 60 {
        return None;
    }
    match row {
        0 => match column {
            0 => Some(8),
            1 => Some(9),
            2 => Some(10),
            3 => Some(KEY_F4),
            _ => None,
        },
        1 => match column {
            0 => Some(5),
            1 => Some(6),
            2 => Some(7),
            3 => Some(KEY_F3),
            _ => None,
        },
        2 => match column {
            0 => Some(2),
            1 => Some(3),
            2 => Some(4),
            3 => Some(KEY_F2),
            _ => None,
        },
        3 => match column {
            0 => Some(11),
            1 => Some(KEY_F7),
            2 => Some(KEY_F6),
            3 => Some(KEY_F1),
            _ => None,
        },
        _ => None,
    }
}

/// Map the visible Contacts/Calendar browse controls to ordinary keyboard actions.
pub const fn touch_contacts_action(focus: FocusTarget, x: u32, y: u32) -> Option<u16> {
    if !matches!(focus, FocusTarget::Contacts | FocusTarget::Calendar) || y < 198 || y >= 218 {
        return None;
    }
    if x >= 20 && x < 104 {
        Some(KEY_UP)
    } else if x >= 108 && x < 184 {
        Some(KEY_DOWN)
    } else if x >= 188 && x < 283 {
        Some(KEY_DELETE)
    } else {
        None
    }
}

/// Map the visible Notes viewport controls to their ordinary keyboard actions.
pub const fn touch_notes_action(
    focus: FocusTarget,
    notes_mode: u32,
    x: u32,
    y: u32,
) -> Option<u16> {
    if !matches!(focus, FocusTarget::Notes) || y < 198 || y >= 218 {
        return None;
    }
    if x >= 20 && x < 68 {
        Some(KEY_F5)
    } else if notes_mode == 1 && x >= 72 && x < 120 {
        Some(KEY_UP)
    } else if notes_mode == 1 && x >= 124 && x < 172 {
        Some(KEY_DOWN)
    } else if notes_mode == 1 && x >= 176 && x < 224 {
        Some(KEY_INSERT)
    } else if notes_mode == 1 && x >= 228 && x < 283 {
        Some(KEY_DELETE)
    } else if x >= 72 && x < 120 {
        Some(KEY_F6)
    } else if x >= 124 && x < 172 {
        Some(KEY_F7)
    } else if notes_mode == 0 && x >= 176 && x < 224 {
        Some(KEY_UP)
    } else if notes_mode == 0 && x >= 228 && x < 283 {
        Some(KEY_DOWN)
    } else {
        None
    }
}

/// Map a Notes-list row tap to the corresponding ordinary function key.
pub const fn touch_notes_list_action(
    focus: FocusTarget,
    list_mode: bool,
    x: u32,
    y: u32,
) -> Option<u16> {
    if !matches!(focus, FocusTarget::Notes) || !list_mode || x < 20 || x >= 283 {
        return None;
    }
    if y < 130 || y >= 190 {
        return None;
    }
    let index = ((y - 130) / 15) as u16;
    Some(KEY_F1 + index)
}

pub const fn onscreen_keyboard_area(y: u32) -> bool {
    y >= KEYBOARD_TOP && y < KEYBOARD_BOTTOM
}

/// Return the evdev key represented by a visible touchscreen key.
pub const fn onscreen_key_code(x: u32, y: u32) -> Option<u16> {
    if y >= 226 && y < 246 && x >= 16 {
        let column = (x - 16) / 27;
        if column < 10 && x < 16 + column * 27 + 24 {
            return Some(column as u16 + 2); // 1..9, then 0
        }
    } else if y >= 248 && y < 268 && x >= 16 {
        let column = (x - 16) / 27;
        if column < 10 && x < 16 + column * 27 + 24 {
            return match column {
                0 => Some(16), // Q
                1 => Some(17), // W
                2 => Some(18), // E
                3 => Some(19), // R
                4 => Some(20), // T
                5 => Some(21), // Y
                6 => Some(22), // U
                7 => Some(23), // I
                8 => Some(24), // O
                9 => Some(25), // P
                _ => None,
            };
        }
    } else if y >= 270 && y < 290 && x >= 29 {
        let column = (x - 29) / 27;
        if column < 9 && x < 29 + column * 27 + 24 {
            return match column {
                0 => Some(30), // A
                1 => Some(31), // S
                2 => Some(32), // D
                3 => Some(33), // F
                4 => Some(34), // G
                5 => Some(35), // H
                6 => Some(36), // J
                7 => Some(37), // K
                8 => Some(38), // L
                _ => None,
            };
        }
    } else if y >= 292 && y < 312 && x >= 56 {
        let column = (x - 56) / 27;
        if column < 7 && x < 56 + column * 27 + 24 {
            return match column {
                0 => Some(44), // Z
                1 => Some(45), // X
                2 => Some(46), // C
                3 => Some(47), // V
                4 => Some(48), // B
                5 => Some(49), // N
                6 => Some(50), // M
                _ => None,
            };
        }
    } else if y >= 324 && y < 353 {
        if x >= 20 && x < 104 {
            return Some(57); // Space
        }
        if x >= 108 && x < 184 {
            return Some(28); // Enter
        }
        if x >= 188 && x < 283 {
            return Some(14); // Backspace
        }
    }
    None
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EventRecord {
    pub device: u16,
    pub event_type: u16,
    pub code: u16,
    pub reserved: u16,
    pub value: u32,
}

#[repr(C)]
pub struct EventRing {
    pub head: core::sync::atomic::AtomicU32,
    pub tail: core::sync::atomic::AtomicU32,
    pub dropped: core::sync::atomic::AtomicU32,
    pub reserved: u32,
    pub events: [core::cell::UnsafeCell<EventRecord>; EVENT_CAPACITY as usize],
}

pub const fn valid_event(event: EventRecord) -> bool {
    if event.reserved != 0 {
        return false;
    }

    match (event.device, event.event_type, event.code) {
        (DEVICE_KEYBOARD | DEVICE_TABLET, EVENT_SYN, SYN_REPORT) => event.value == 0,
        (DEVICE_KEYBOARD, EVENT_KEY, code) => code > 0 && code <= KEY_MAX && event.value <= 2,
        (DEVICE_TABLET, EVENT_KEY, BTN_LEFT) => event.value <= 1,
        (DEVICE_TABLET, EVENT_ABS, ABS_X | ABS_Y) => event.value <= TABLET_AXIS_MAX,
        _ => false,
    }
}

pub const fn decode_routed_event(
    header: u64,
    value: u64,
    sequence: u64,
) -> Option<(EventRecord, u64)> {
    if header >> 48 != 0 || value > u32::MAX as u64 || sequence == 0 {
        return None;
    }
    let event = EventRecord {
        device: (header >> 32) as u16,
        event_type: (header >> 16) as u16,
        code: header as u16,
        reserved: 0,
        value: value as u32,
    };
    if valid_event(event) {
        Some((event, sequence))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn event(device: u16, event_type: u16, code: u16, value: u32) -> EventRecord {
        EventRecord {
            device,
            event_type,
            code,
            reserved: 0,
            value,
        }
    }

    #[test]
    fn accepts_supported_keyboard_and_tablet_events() {
        assert!(valid_event(event(DEVICE_KEYBOARD, EVENT_KEY, 30, 1)));
        assert!(valid_event(event(DEVICE_KEYBOARD, EVENT_KEY, 30, 2)));
        assert!(valid_event(event(
            DEVICE_KEYBOARD,
            EVENT_SYN,
            SYN_REPORT,
            0
        )));
        assert!(valid_event(event(DEVICE_TABLET, EVENT_ABS, ABS_X, 12345)));
        assert!(valid_event(event(
            DEVICE_TABLET,
            EVENT_ABS,
            ABS_Y,
            TABLET_AXIS_MAX
        )));
        assert!(valid_event(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1)));
    }

    #[test]
    fn rejects_out_of_policy_values_and_device_code_combinations() {
        assert!(!valid_event(event(DEVICE_KEYBOARD, EVENT_KEY, 30, 3)));
        assert!(!valid_event(event(
            DEVICE_KEYBOARD,
            EVENT_KEY,
            KEY_MAX + 1,
            1
        )));
        assert!(!valid_event(event(
            DEVICE_TABLET,
            EVENT_ABS,
            ABS_X,
            TABLET_AXIS_MAX + 1
        )));
        assert!(!valid_event(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 2)));
        assert!(!valid_event(event(DEVICE_TABLET, EVENT_KEY, 273, 1)));
        assert!(!valid_event(event(DEVICE_KEYBOARD, EVENT_SYN, 1, 0)));
        assert!(!valid_event(event(9, EVENT_SYN, SYN_REPORT, 0)));
        assert!(!valid_event(EventRecord {
            reserved: 1,
            ..event(DEVICE_KEYBOARD, EVENT_KEY, 30, 1)
        }));
        assert!(!valid_event(event(9, EVENT_KEY, 30, 1)));
    }

    #[test]
    fn touch_hits_only_the_visible_app_surfaces_on_left_button_down() {
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 10_000, 6_000),
            Some(FocusTarget::Notes)
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 24_000, 6_000),
            Some(FocusTarget::Contacts)
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 10_000, 10_000),
            Some(FocusTarget::Calculator)
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 24_000, 10_000),
            Some(FocusTarget::Clock)
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 10_000, 14_000),
            Some(FocusTarget::Calendar)
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 24_000, 14_000),
            Some(FocusTarget::Messages)
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 10_000, 17_000),
            Some(FocusTarget::Phone)
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 24_000, 17_000),
            Some(FocusTarget::Settings)
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 0), 10_000, 10_000),
            None
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_KEYBOARD, EVENT_KEY, 30, 1), 10_000, 10_000),
            None
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 0, 100),
            None
        );
        assert_eq!(
            touch_focus_target(event(DEVICE_TABLET, EVENT_KEY, BTN_LEFT, 1), 24_000, 20_000),
            None
        );
    }

    #[test]
    fn tab_cycles_focus_between_workspaces_and_starts_at_notes() {
        assert_eq!(next_focus_target(FocusTarget::None), FocusTarget::Notes);
        assert_eq!(next_focus_target(FocusTarget::Notes), FocusTarget::Contacts);
        assert_eq!(
            next_focus_target(FocusTarget::Contacts),
            FocusTarget::Calculator
        );
        assert_eq!(
            next_focus_target(FocusTarget::Calculator),
            FocusTarget::Clock
        );
        assert_eq!(
            next_focus_target(FocusTarget::Clock),
            FocusTarget::Calendar
        );
        assert_eq!(
            next_focus_target(FocusTarget::Calendar),
            FocusTarget::Messages
        );
        assert_eq!(
            next_focus_target(FocusTarget::Messages),
            FocusTarget::Phone
        );
        assert_eq!(
            next_focus_target(FocusTarget::Phone),
            FocusTarget::Settings
        );
        assert_eq!(
            next_focus_target(FocusTarget::Settings),
            FocusTarget::Notes
        );
    }

    #[test]
    fn touchscreen_keyboard_maps_only_visible_key_faces() {
        assert!(onscreen_keyboard_area(230));
        assert!(!onscreen_keyboard_area(360));
        assert_eq!(onscreen_key_code(28, 236), Some(2)); // 1
        assert_eq!(onscreen_key_code(271, 236), Some(11)); // 0
        assert_eq!(onscreen_key_code(28, 258), Some(16)); // Q
        assert_eq!(onscreen_key_code(40, 280), Some(30)); // A
        assert_eq!(onscreen_key_code(176, 302), Some(48)); // B
        assert_eq!(onscreen_key_code(60, 337), Some(57)); // Space
        assert_eq!(onscreen_key_code(146, 337), Some(28)); // Enter
        assert_eq!(onscreen_key_code(228, 337), Some(14)); // Backspace
        assert_eq!(onscreen_key_code(40, 247), None); // gap between rows
        assert_eq!(onscreen_key_code(54, 280), None); // gap between keys
        assert_eq!(onscreen_key_code(10, 280), None); // outside the keyboard
    }

    #[test]
    fn app_navigation_maps_home_and_app_tabs() {
        assert_eq!(
            touch_navigation_action(30, 24),
            Some(NavigationAction::Home)
        );
        assert_eq!(
            touch_navigation_action(57, 24),
            Some(NavigationAction::Open(FocusTarget::Notes))
        );
        assert_eq!(
            touch_navigation_action(83, 24),
            Some(NavigationAction::Open(FocusTarget::Contacts))
        );
        assert_eq!(
            touch_navigation_action(109, 24),
            Some(NavigationAction::Open(FocusTarget::Calculator))
        );
        assert_eq!(
            touch_navigation_action(135, 24),
            Some(NavigationAction::Open(FocusTarget::Clock))
        );
        assert_eq!(
            touch_navigation_action(161, 24),
            Some(NavigationAction::Open(FocusTarget::Calendar))
        );
        assert_eq!(
            touch_navigation_action(187, 24),
            Some(NavigationAction::Open(FocusTarget::Messages))
        );
        assert_eq!(
            touch_navigation_action(213, 24),
            Some(NavigationAction::Open(FocusTarget::Phone))
        );
        assert_eq!(
            touch_navigation_action(239, 24),
            Some(NavigationAction::Open(FocusTarget::Settings))
        );
        assert_eq!(touch_navigation_action(44, 24), None);
        assert_eq!(touch_navigation_action(40, 50), None);
    }

    #[test]
    fn calculator_touch_maps_only_the_visible_key_faces() {
        assert_eq!(
            touch_calculator_action(FocusTarget::Calculator, 30, 237),
            Some(8)
        );
        assert_eq!(
            touch_calculator_action(FocusTarget::Calculator, 245, 237),
            Some(KEY_F4)
        );
        assert_eq!(
            touch_calculator_action(FocusTarget::Calculator, 30, 315),
            Some(11)
        );
        assert_eq!(
            touch_calculator_action(FocusTarget::Calculator, 180, 315),
            Some(KEY_F6)
        );
        assert_eq!(
            touch_calculator_action(FocusTarget::Calculator, 245, 315),
            Some(KEY_F1)
        );
        assert_eq!(
            touch_calculator_action(FocusTarget::Calculator, 150, 341),
            Some(KEY_F5)
        );
        assert_eq!(touch_calculator_action(FocusTarget::Notes, 30, 237), None);
        assert_eq!(
            touch_calculator_action(FocusTarget::Calculator, 80, 237),
            None
        );
        assert_eq!(
            touch_calculator_action(FocusTarget::Calculator, 150, 356),
            None
        );
    }

    #[test]
    fn contacts_touch_controls_map_only_the_visible_buttons() {
        assert_eq!(
            touch_contacts_action(FocusTarget::Contacts, 60, 208),
            Some(KEY_UP)
        );
        assert_eq!(
            touch_contacts_action(FocusTarget::Contacts, 146, 208),
            Some(KEY_DOWN)
        );
        assert_eq!(
            touch_contacts_action(FocusTarget::Contacts, 235, 208),
            Some(KEY_DELETE)
        );
        assert_eq!(touch_contacts_action(FocusTarget::Notes, 60, 208), None);
        assert_eq!(touch_contacts_action(FocusTarget::Contacts, 106, 208), None);
        assert_eq!(touch_contacts_action(FocusTarget::Contacts, 235, 218), None);
    }

    #[test]
    fn notes_touch_controls_map_only_the_visible_buttons() {
        assert_eq!(
            touch_notes_action(FocusTarget::Notes, 0, 44, 208),
            Some(KEY_F5)
        );
        assert_eq!(
            touch_notes_action(FocusTarget::Notes, 0, 96, 208),
            Some(KEY_F6)
        );
        assert_eq!(
            touch_notes_action(FocusTarget::Notes, 0, 148, 208),
            Some(KEY_F7)
        );
        assert_eq!(
            touch_notes_action(FocusTarget::Notes, 0, 200, 208),
            Some(KEY_UP)
        );
        assert_eq!(
            touch_notes_action(FocusTarget::Notes, 0, 255, 208),
            Some(KEY_DOWN)
        );
        assert_eq!(
            touch_notes_action(FocusTarget::Notes, 1, 200, 208),
            Some(KEY_INSERT)
        );
        assert_eq!(
            touch_notes_action(FocusTarget::Notes, 1, 255, 208),
            Some(KEY_DELETE)
        );
        assert_eq!(
            touch_notes_list_action(FocusTarget::Notes, true, 100, 152),
            Some(KEY_F2)
        );
        assert_eq!(
            touch_notes_action(FocusTarget::Notes, 1, 96, 208),
            Some(KEY_UP)
        );
        assert_eq!(touch_notes_action(FocusTarget::Notes, 2, 200, 208), None);
        assert_eq!(touch_notes_action(FocusTarget::Contacts, 0, 44, 208), None);
        assert_eq!(touch_notes_action(FocusTarget::Notes, 0, 70, 208), None);
        assert_eq!(touch_notes_action(FocusTarget::Notes, 0, 148, 218), None);
        assert_eq!(
            touch_notes_list_action(FocusTarget::Notes, false, 100, 152),
            None
        );
    }

    #[test]
    fn key_release_stays_with_the_app_that_received_keydown() {
        let mut owners = KeyOwners::new();
        let key_down = event(DEVICE_KEYBOARD, EVENT_KEY, 30, 1);
        let key_up = event(DEVICE_KEYBOARD, EVENT_KEY, 30, 0);
        let other_key_down = event(DEVICE_KEYBOARD, EVENT_KEY, 48, 1);

        assert_eq!(
            owners.target_for_event(key_down, FocusTarget::Notes),
            FocusTarget::Notes
        );
        assert_eq!(
            owners.target_for_event(key_up, FocusTarget::Contacts),
            FocusTarget::Notes
        );
        assert_eq!(
            owners.target_for_event(other_key_down, FocusTarget::Contacts),
            FocusTarget::Contacts
        );
        assert_eq!(
            owners.target_for_event(key_up, FocusTarget::Contacts),
            FocusTarget::None
        );
    }

    #[test]
    fn routed_event_decoder_rejects_malformed_wire_records() {
        let key_header = ((DEVICE_KEYBOARD as u64) << 32) | ((EVENT_KEY as u64) << 16) | 30;
        assert_eq!(
            decode_routed_event(key_header, 1, 42),
            Some((event(DEVICE_KEYBOARD, EVENT_KEY, 30, 1), 42))
        );
        assert_eq!(decode_routed_event(key_header | (1 << 48), 1, 42), None);
        assert_eq!(
            decode_routed_event(key_header, u32::MAX as u64 + 1, 42),
            None
        );
        assert_eq!(decode_routed_event(key_header, 1, 0), None);
        assert_eq!(decode_routed_event(key_header, 3, 42), None);
    }
}
