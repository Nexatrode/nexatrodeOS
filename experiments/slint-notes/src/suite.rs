//! Volatile interaction model for the native Slint app suite. This deliberately
//! makes no storage, RTC, modem, or network claim.

use alloc::{
    rc::Rc,
    string::{String, ToString},
};
use core::cell::RefCell;
use slint::ComponentHandle;

use crate::NotesWindow;

/// Matches the guest agent SQLCipher key and official archive passphrase file.
const DATABASE_PASSPHRASE: &str = "qemu-agentstore-test-key-v1";

fn apply_passphrase(current: &str, key: &str) -> String {
    match key {
        "SHIFT" | "?123" | "ABC" => String::from(current),
        "CLEAR" => String::new(),
        "DELETE" => {
            let mut value = String::from(current);
            value.pop();
            value
        }
        "SPACE" => {
            if current.len() >= 128 {
                String::from(current)
            } else {
                let mut value = String::from(current);
                value.push(' ');
                value
            }
        }
        character if character.len() == 1 && character.as_bytes()[0].is_ascii_graphic() => {
            if current.len() >= 128 {
                String::from(current)
            } else {
                let mut value = String::from(current);
                value.push_str(character);
                value
            }
        }
        _ => String::from(current),
    }
}

fn apply_key(current: &str, key: &str, limit: usize, multiline: bool) -> String {
    let mut next = String::from(current);
    match key {
        "DELETE" => {
            next.pop();
        }
        "SPACE" if next.len() < limit => next.push(' '),
        "ENTER" if multiline && next.len() < limit => next.push('\n'),
        _ if key.len() == 1
            && next.len() < limit
            && (key.as_bytes()[0].is_ascii_uppercase() || key.as_bytes()[0].is_ascii_digit()) =>
        {
            next.push_str(key)
        }
        _ => {}
    }
    next
}

fn apply_contact_name(current: &str, key: &str) -> String {
    if key == "DELETE"
        || key == "SPACE"
        || (key.len() == 1 && key.as_bytes()[0].is_ascii_uppercase())
    {
        apply_key(current, key, 40, false)
    } else {
        String::from(current)
    }
}

fn apply_contact_phone(current: &str, key: &str) -> String {
    if key == "DELETE" || (key.len() == 1 && key.as_bytes()[0].is_ascii_digit()) {
        apply_key(current, key, 24, false)
    } else {
        String::from(current)
    }
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
        if let Some(digit) = key
            .as_bytes()
            .first()
            .filter(|_| key.len() == 1)
            .and_then(|byte| byte.checked_sub(b'0'))
            .filter(|digit| *digit <= 9)
        {
            if self.error {
                *self = Self::default();
            }
            let next = if self.replace_entry {
                Some(digit as i64)
            } else {
                self.entry
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(digit as i64))
            };
            if let Some(value) = next.filter(|value| *value <= 999_999_999) {
                self.entry = value;
                self.replace_entry = false;
            }
            return self.entry.to_string();
        }
        if self.error {
            return String::from("ERROR");
        }
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

/// Baked-in clearnet test wallet. UTM's live scan overwrites this once the
/// wallet PD publishes an address. Pixel and iPhone show the same Receive QR
/// from this constant because those images do not have the wallet service yet.
pub const TEST_WALLET_ADDRESS: &str = "4A1ZVG4SrnpMgcnr3EDTHz2rzprHZm9pq7sZKj3dNL6L5j9CFcVd3RK3yZFJm3XWW3W2YtCAUxb7yBJEvsnicvriSZVA5nq";

pub fn present_test_wallet(ui: &NotesWindow) {
    let addr = TEST_WALLET_ADDRESS;
    ui.set_wallet_address(addr.into());
    let short = alloc::format!("{}…{}", &addr[..8], &addr[addr.len() - 6..]);
    ui.set_wallet_address_short(short.into());
    if let Some(image) = crate::qr::qr_image(addr) {
        ui.set_wallet_address_qr(image);
        ui.set_wallet_address_qr_visible(true);
    }
}

/// Send address is target 11, amount is target 12. Returns the replacement
/// field text, or `None` when the key does not change that field.
pub fn edit_wallet_field(target: i32, value: &str, key: &str) -> Option<String> {
    if target != 11 && target != 12 {
        return None;
    }
    if matches!(key, "ENTER" | "\n" | "SPACE" | "SHIFT" | "?123" | "ABC") {
        return None;
    }
    let mut next = if target == 12 && value == "0.000000000001" {
        String::new()
    } else {
        String::from(value)
    };
    match key {
        "DELETE" | "\u{8}" => {
            next.pop();
        }
        "CLEAR" => next.clear(),
        "." if target == 12 => {
            if !next.contains('.') && next.len() < 20 {
                next.push('.');
            }
        }
        k if target == 12 && k.len() == 1 && k.as_bytes()[0].is_ascii_digit() && next.len() < 20 => {
            next.push_str(k);
        }
        k if target == 11
            && k.len() == 1
            && k.as_bytes()[0].is_ascii_alphanumeric()
            && next.len() < 106 =>
        {
            next.push_str(k);
        }
        _ => return None,
    }
    Some(next)
}

pub fn apply_wallet_key(ui: &NotesWindow, key: &str) -> bool {
    if ui.get_page() != 16 || ui.get_wallet_tab() != 2 {
        return false;
    }
    let target = ui.get_edit_target();
    if key == "ENTER" || key == "\n" {
        if target == 11 || target == 12 {
            ui.set_edit_target(0);
            return true;
        }
        return false;
    }
    let current = if target == 11 {
        ui.get_wallet_send_dest().to_string()
    } else if target == 12 {
        ui.get_wallet_send_amount().to_string()
    } else {
        return false;
    };
    let Some(next) = edit_wallet_field(target, &current, key) else {
        return key == "SPACE" || key == "SHIFT" || key == "?123" || key == "ABC";
    };
    ui.set_wallet_fee_ready(false);
    ui.set_wallet_send_status("".into());
    if target == 11 {
        ui.set_wallet_send_dest(next.into());
    } else {
        ui.set_wallet_send_max(false);
        ui.set_wallet_send_amount(next.into());
    }
    true
}

fn escape_copy(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => {}
            other if other.is_ascii() => escaped.push(other),
            _ => {}
        }
    }
    escaped
}

fn emit_copy_line(line: &str) {
    #[cfg(target_os = "none")]
    sel4_microkit::debug_println!("{}", line);
    #[cfg(not(target_os = "none"))]
    {
        let _ = line;
    }
}

/// Same serial clipboard marker the UTM guest prints for the Mac helper.
pub fn copy_wallet_address(ui: &NotesWindow) {
    let addr = ui.get_wallet_address().to_string();
    if addr.is_empty() {
        ui.set_wallet_sync("Address not ready yet".into());
        return;
    }
    let escaped = escape_copy(&addr);
    emit_copy_line(&alloc::format!("NEXATRODE_COPY|BEGIN|{}", escaped.len()));
    let mut start = 0;
    while start < escaped.len() {
        let mut end = (start + 80).min(escaped.len());
        while end > start && !escaped.is_char_boundary(end) {
            end -= 1;
        }
        emit_copy_line(&alloc::format!("NEXATRODE_COPY|DATA|{}", &escaped[start..end]));
        start = end;
    }
    emit_copy_line("NEXATRODE_COPY|END");
    ui.set_wallet_sync("Address copied".into());
}

pub fn wire(ui: &NotesWindow) {
    wire_with_notes(
        ui,
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
    // UTM has a real wallet service; never initialize its receive view from
    // the shared demo address, even before the first frame is displayed.
    #[cfg(not(feature = "utm-pd"))]
    present_test_wallet(ui);
    let weak = ui.as_weak();
    ui.on_save_note(move |_| {
        if let Some(ui) = weak.upgrade() {
            notes_save(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_save_contact(move |_, _| {
        if let Some(ui) = weak.upgrade() {
            ui.set_contact_status("SAVE TAP RECEIVED".into());
        }
    });

    let weak = ui.as_weak();
    ui.on_save_calendar(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.set_calendar_status("SAVE TAP RECEIVED".into());
        }
    });

    let weak = ui.as_weak();
    ui.on_key_tapped(move |key| {
        if let Some(ui) = weak.upgrade() {
            let key = key.as_str();
            if apply_wallet_key(&ui, key) {
                return;
            }
            match ui.get_edit_target() {
                1 => notes_key(&ui, key),
                2 => {
                    ui.set_contact_name(
                        apply_contact_name(ui.get_contact_name().as_str(), key).into(),
                    );
                    ui.set_contact_status("VOLATILE · NOT SAVED".into());
                }
                3 => {
                    ui.set_contact_phone(
                        apply_contact_phone(ui.get_contact_phone().as_str(), key).into(),
                    );
                    ui.set_contact_status("VOLATILE · NOT SAVED".into());
                }
                4 => {
                    ui.set_calendar_item(
                        apply_key(ui.get_calendar_item().as_str(), key, 21, false).into(),
                    );
                    ui.set_calendar_status("VOLATILE · NOT SAVED".into());
                }
                5 => ui.set_message_peer(
                    apply_key(ui.get_message_peer().as_str(), key, 32, false).into(),
                ),
                6 => ui.set_message_body(
                    apply_key(ui.get_message_body().as_str(), key, 128, true).into(),
                ),
                10 => {
                    ui.set_message_search(
                        apply_key(ui.get_message_search().as_str(), key, 40, false).into(),
                    );
                    ui.invoke_filter_message_chats();
                }
                13 => {
                    if key == "ENTER" {
                        ui.set_edit_target(0);
                        ui.invoke_save_profile();
                    } else if key == "DELETE"
                        || key == "SPACE"
                        || (key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric())
                    {
                        ui.set_message_profile_name(
                            apply_key(ui.get_message_profile_name().as_str(), key, 48, false)
                                .into(),
                        );
                    }
                }
                _ => {}
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_join_key_tapped(move |key| {
        if let Some(ui) = weak.upgrade() {
            match ui.get_edit_target() {
                7 => {
                    let current = ui.get_message_join_uri().to_string();
                    let next = match key.as_str() {
                        "SHIFT" | "?123" | "ABC" => current,
                        "CLEAR" => String::new(),
                        "DELETE" => {
                            let mut value = current;
                            value.pop();
                            value
                        }
                        "SPACE" => current,
                        character
                            if character.len() == 1
                                && character.as_bytes()[0].is_ascii_graphic() =>
                        {
                            if current.len() >= 2048 {
                                current
                            } else {
                                let mut value = current;
                                value.push_str(character);
                                value
                            }
                        }
                        _ => current,
                    };
                    ui.set_message_join_uri(next.into());
                    ui.set_message_join_status("INVITATION ENTERED · TAP JOIN LINK".into());
                }
                8 => {
                    ui.set_db_passphrase_current(
                        apply_passphrase(ui.get_db_passphrase_current().as_str(), key.as_str())
                            .into(),
                    );
                    ui.set_db_unlocked(false);
                    ui.set_db_unlock_status("LOCKED · ENTER PASSPHRASE".into());
                }
                9 => {
                    ui.set_db_passphrase_new(
                        apply_passphrase(ui.get_db_passphrase_new().as_str(), key.as_str()).into(),
                    );
                }
                _ => {}
            }
        }
    });

    let session_key = Rc::new(RefCell::new(String::from(DATABASE_PASSPHRASE)));

    let weak = ui.as_weak();
    let unlock_key = session_key.clone();
    ui.on_unlock_database_passphrase(move || {
        if let Some(ui) = weak.upgrade() {
            let current = ui.get_db_passphrase_current().to_string();
            if current == *unlock_key.borrow() {
                ui.set_db_unlocked(true);
                ui.set_db_unlock_status("UNLOCKED · MATCHES PROFILE KEY".into());
                ui.set_settings_status("DATABASE · UNLOCKED".into());
                ui.set_edit_target(0);
            } else if current.is_empty() {
                ui.set_db_unlocked(false);
                ui.set_db_unlock_status("LOCKED · ENTER PASSPHRASE".into());
            } else {
                ui.set_db_unlocked(false);
                ui.set_db_unlock_status("WRONG PASSPHRASE".into());
            }
        }
    });

    let weak = ui.as_weak();
    let change_key = session_key.clone();
    ui.on_change_database_passphrase(move || {
        if let Some(ui) = weak.upgrade() {
            let current = ui.get_db_passphrase_current().to_string();
            let next = ui.get_db_passphrase_new().to_string();
            if current != *change_key.borrow() {
                ui.set_db_unlocked(false);
                ui.set_db_unlock_status("CHANGE NEEDS CURRENT PASSPHRASE".into());
                return;
            }
            if next.is_empty() {
                ui.set_db_unlock_status("ENTER A NEW PASSPHRASE".into());
                return;
            }
            if next == current {
                ui.set_db_unlock_status("NEW PASSPHRASE MUST DIFFER".into());
                return;
            }
            // Portable archive rekey is host-side (`simplex_archive.py rekey`).
            // The live guest agent keeps the profile key until an export/import cycle.
            *change_key.borrow_mut() = next.clone();
            ui.set_db_unlocked(true);
            ui.set_db_unlock_status("CHANGED · EXPORT ARCHIVE TO APPLY ON PHONE".into());
            ui.set_db_export_status("REKEY VIA OFFICIAL ZIP · PASSPHRASE OUTSIDE".into());
            ui.set_settings_status("DATABASE · PASSPHRASE CHANGED".into());
            ui.set_db_passphrase_current(next.into());
            ui.set_db_passphrase_new("".into());
            ui.set_edit_target(0);
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
                "CLEAR" => {
                    ui.set_phone_number("".into());
                    ui.set_phone_status("IDLE".into());
                }
                "CALL" => ui.set_phone_status("NO MODEM · DEMO ONLY".into()),
                digit if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => {
                    let mut number = ui.get_phone_number().to_string();
                    if number.len() < 20 {
                        number.push_str(digit);
                    }
                    ui.set_phone_number(number.into());
                    ui.set_phone_status("DIAL".into());
                }
                _ => {}
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_copy_wallet_address(move || {
        if let Some(ui) = weak.upgrade() {
            copy_wallet_address(&ui);
        }
    });
    let weak = ui.as_weak();
    ui.on_prepare_wallet_send(move || {
        if let Some(ui) = weak.upgrade() {
            // Phones have the Send form and no wallet service. Do not invent a fee.
            // UTM replaces this callback with the live prepare path.
            let dest = ui.get_wallet_send_dest().to_string();
            let amount = ui.get_wallet_send_amount().to_string();
            ui.set_wallet_fee_ready(false);
            if dest.is_empty() {
                ui.set_wallet_send_status("Enter an address".into());
            } else if amount.is_empty() || amount == "0" || amount == "0.0" || amount == "0." {
                ui.set_wallet_send_status("Enter an amount".into());
            } else {
                ui.set_wallet_send_status("No wallet service on this phone".into());
            }
        }
    });
    let weak = ui.as_weak();
    ui.on_confirm_wallet_send(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_edit_target(0);
            ui.set_wallet_fee_ready(false);
            if ui.get_wallet_send_dest().is_empty() || ui.get_wallet_send_amount().is_empty() {
                ui.set_wallet_send_status("Preview the fee first".into());
            } else {
                ui.set_wallet_send_status("No wallet service on this phone".into());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        apply_contact_name, apply_contact_phone, apply_key, edit_wallet_field, Calculator,
    };

    #[test]
    fn calculator_arithmetic_and_zero_division() {
        let mut calc = Calculator::default();
        for key in ["7", "+", "5"] {
            calc.press(key);
        }
        assert_eq!(calc.press("="), "12");
        for key in ["C", "9", "÷", "0"] {
            calc.press(key);
        }
        assert_eq!(calc.press("="), "ERROR");
        assert_eq!(calc.press("C"), "0");
    }

    #[test]
    fn wallet_send_fields_accept_address_and_amount_keys() {
        assert_eq!(
            edit_wallet_field(11, "4A", "1").as_deref(),
            Some("4A1")
        );
        assert_eq!(edit_wallet_field(11, "4A", " ").as_deref(), None);
        assert_eq!(edit_wallet_field(11, "4A", "DELETE").as_deref(), Some("4"));
        assert_eq!(edit_wallet_field(12, "0.00", "1").as_deref(), Some("0.001"));
        assert_eq!(edit_wallet_field(12, "0.00", ".").as_deref(), Some("0.00"));
        assert_eq!(edit_wallet_field(12, "1", "A").as_deref(), None);
        assert_eq!(edit_wallet_field(12, "0.000000000001", "2").as_deref(), Some("2"));
        let long = "A".repeat(106);
        assert_eq!(edit_wallet_field(11, &long, "B").as_deref(), None);
    }

    #[test]
    fn keyboard_limits_and_deletes_without_split_utf8() {
        assert_eq!(apply_key("AB", "DELETE", 4, false), "A");
        assert_eq!(apply_key("AB", "ENTER", 4, false), "AB");
        assert_eq!(apply_key("AB", "ENTER", 4, true), "AB\n");
        assert_eq!(apply_key("ABCD", "E", 4, false), "ABCD");
        assert_eq!(apply_key("é", "DELETE", 4, false), "");
    }

    #[test]
    fn contact_fields_only_accept_their_vault_supported_characters() {
        assert_eq!(apply_contact_name("AL", "B"), "ALB");
        assert_eq!(apply_contact_name("AL", "5"), "AL");
        assert_eq!(apply_contact_phone("12", "3"), "123");
        assert_eq!(apply_contact_phone("12", "A"), "12");
    }
}
