//! Volatile interaction model for the native Slint app suite. This deliberately
//! makes no storage, RTC, modem, or network claim.

use alloc::{rc::Rc, string::{String, ToString}};
use core::cell::RefCell;
use slint::ComponentHandle;

use crate::NotesWindow;

fn apply_key(current: &str, key: &str, limit: usize, multiline: bool) -> String {
    let mut next = String::from(current);
    match key {
        "DELETE" => { next.pop(); }
        "SPACE" if next.len() < limit => next.push(' '),
        "ENTER" if multiline && next.len() < limit => next.push('\n'),
        _ if key.len() == 1 && next.len() < limit &&
            (key.as_bytes()[0].is_ascii_uppercase() || key.as_bytes()[0].is_ascii_digit()) => next.push_str(key),
        _ => {}
    }
    next
}

#[derive(Default)]
struct Calculator {
    entry: i64,
    accumulator: Option<i64>,
    operation: Option<char>,
    replace_entry: bool,
    error: bool,
}

impl Calculator {
    fn press(&mut self, key: &str) -> String {
        if key == "C" {
            *self = Self::default();
            return String::from("0");
        }
        if let Some(digit) = key.as_bytes().first().filter(|_| key.len() == 1)
            .and_then(|byte| byte.checked_sub(b'0')).filter(|digit| *digit <= 9)
        {
            if self.error { *self = Self::default(); }
            let next = if self.replace_entry { Some(digit as i64) } else {
                self.entry.checked_mul(10).and_then(|value| value.checked_add(digit as i64))
            };
            if let Some(value) = next.filter(|value| *value <= 999_999_999) {
                self.entry = value;
                self.replace_entry = false;
            }
            return self.entry.to_string();
        }
        if self.error { return String::from("ERROR"); }
        if key == "=" || matches!(key, "+" | "−" | "×" | "÷") {
            if let (Some(left), Some(op)) = (self.accumulator, self.operation) {
                let result = match op {
                    '+' => left.checked_add(self.entry),
                    '−' => left.checked_sub(self.entry),
                    '×' => left.checked_mul(self.entry),
                    '÷' if self.entry != 0 => left.checked_div(self.entry),
                    _ => None,
                };
                let Some(result) = result else {
                    self.error = true;
                    return String::from("ERROR");
                };
                self.entry = result;
            }
            self.accumulator = if key == "=" { None } else { Some(self.entry) };
            self.operation = if key == "=" { None } else { key.chars().next() };
            self.replace_entry = true;
        }
        self.entry.to_string()
    }
}

pub fn wire(ui: &NotesWindow) {
    wire_with_notes(ui,
        |ui, key| {
            ui.set_note(apply_key(ui.get_note().as_str(), key, 128, true).into());
            ui.set_status("VOLATILE · NOT SAVED".into());
        },
        |ui| ui.set_status("HELD IN MEMORY ONLY".into()),
    );
}

/// The device shell can delegate Notes edits to a separate protection domain.
/// Other workspaces remain local until their services are split out.
pub fn wire_with_notes<K, S>(ui: &NotesWindow, notes_key: K, notes_save: S)
where
    K: Fn(&NotesWindow, &str) + 'static,
    S: Fn(&NotesWindow) + 'static,
{
    let weak = ui.as_weak();
    ui.on_save_note(move |_| {
        if let Some(ui) = weak.upgrade() {
            notes_save(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_key_tapped(move |key| {
        if let Some(ui) = weak.upgrade() {
            let key = key.as_str();
            match ui.get_edit_target() {
                1 => notes_key(&ui, key),
                2 => ui.set_contact_name(apply_key(ui.get_contact_name().as_str(), key, 40, false).into()),
                3 => ui.set_contact_phone(apply_key(ui.get_contact_phone().as_str(), key, 24, false).into()),
                4 => ui.set_calendar_item(apply_key(ui.get_calendar_item().as_str(), key, 128, false).into()),
                5 => ui.set_message_peer(apply_key(ui.get_message_peer().as_str(), key, 32, false).into()),
                6 => ui.set_message_body(apply_key(ui.get_message_body().as_str(), key, 128, true).into()),
                _ => {}
            }
        }
    });

    let weak = ui.as_weak();
    let calculator = Rc::new(RefCell::new(Calculator::default()));
    ui.on_calculator_key(move |key| {
        if let Some(ui) = weak.upgrade() {
            let display = calculator.borrow_mut().press(key.as_str());
            ui.set_calc_display(display.into());
        }
    });

    let weak = ui.as_weak();
    ui.on_action(move |key| {
        if let Some(ui) = weak.upgrade() {
            match key.as_str() {
                "CLEAR" => { ui.set_phone_number("".into()); ui.set_phone_status("IDLE".into()); }
                "CALL" => ui.set_phone_status("NO MODEM · DEMO ONLY".into()),
                digit if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => {
                    let mut number = ui.get_phone_number().to_string();
                    if number.len() < 20 { number.push_str(digit); }
                    ui.set_phone_number(number.into());
                    ui.set_phone_status("DIAL".into());
                }
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{Calculator, apply_key};

    #[test]
    fn calculator_arithmetic_and_zero_division() {
        let mut calc = Calculator::default();
        for key in ["7", "+", "5"] { calc.press(key); }
        assert_eq!(calc.press("="), "12");
        for key in ["C", "9", "÷", "0"] { calc.press(key); }
        assert_eq!(calc.press("="), "ERROR");
        assert_eq!(calc.press("C"), "0");
    }

    #[test]
    fn keyboard_limits_and_deletes_without_split_utf8() {
        assert_eq!(apply_key("AB", "DELETE", 4, false), "A");
        assert_eq!(apply_key("AB", "ENTER", 4, false), "AB");
        assert_eq!(apply_key("AB", "ENTER", 4, true), "AB\n");
        assert_eq!(apply_key("ABCD", "E", 4, false), "ABCD");
        assert_eq!(apply_key("é", "DELETE", 4, false), "");
    }
}
