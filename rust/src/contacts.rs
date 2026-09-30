//! Bounded Contacts data model and canonical blob codec.

use crate::app_view::{KEY_BACKSPACE, KEY_ENTER, KEY_SPACE, evdev_letter};

pub const MAX_CONTACTS: usize = 4;
pub const NAME_CAPACITY: usize = 12;
pub const PHONE_CAPACITY: usize = 16;
pub const KEY_CAPACITY: usize = 8;
pub const CONTACTS_BLOB_CAPACITY: usize =
    5 + MAX_CONTACTS * (3 + NAME_CAPACITY + PHONE_CAPACITY + KEY_CAPACITY);
const MAGIC_V1: [u8; 4] = *b"PCT1";
const MAGIC_V2: [u8; 4] = *b"PCT2";

const KEY_UP: u16 = 103;
const KEY_DOWN: u16 = 108;
const KEY_DELETE: u16 = 111;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Contact {
    name: [u8; NAME_CAPACITY],
    name_length: u8,
    phone: [u8; PHONE_CAPACITY],
    phone_length: u8,
    key: [u8; KEY_CAPACITY],
    key_length: u8,
}

impl Contact {
    const EMPTY: Self = Self {
        name: [0; NAME_CAPACITY],
        name_length: 0,
        phone: [0; PHONE_CAPACITY],
        phone_length: 0,
        key: [0; KEY_CAPACITY],
        key_length: 0,
    };

    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_length as usize]
    }

    pub fn phone(&self) -> &[u8] {
        &self.phone[..self.phone_length as usize]
    }

    pub fn key(&self) -> &[u8] {
        &self.key[..self.key_length as usize]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditMode {
    Browse,
    Name,
    Phone,
    Key,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContactUpdate {
    pub changed: bool,
    pub persist: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContactsApp {
    entries: [Contact; MAX_CONTACTS],
    count: u8,
    selected: u8,
    mode: EditMode,
    draft_name: [u8; NAME_CAPACITY],
    draft_name_length: u8,
    draft_phone: [u8; PHONE_CAPACITY],
    draft_phone_length: u8,
    draft_key: [u8; KEY_CAPACITY],
    draft_key_length: u8,
}

impl ContactsApp {
    pub const fn new() -> Self {
        Self {
            entries: [Contact::EMPTY; MAX_CONTACTS],
            count: 0,
            selected: 0,
            mode: EditMode::Browse,
            draft_name: [0; NAME_CAPACITY],
            draft_name_length: 0,
            draft_phone: [0; PHONE_CAPACITY],
            draft_phone_length: 0,
            draft_key: [0; KEY_CAPACITY],
            draft_key_length: 0,
        }
    }

    pub fn count(&self) -> usize {
        self.count as usize
    }

    pub fn selected(&self) -> Option<&Contact> {
        self.entries
            .get(self.selected as usize)
            .filter(|_| self.count > 0)
    }

    /// Import an older app blob that stored only a single name.
    pub fn from_legacy_name(name: &[u8]) -> Option<Self> {
        if name.len() > NAME_CAPACITY || name.iter().any(|byte| !matches!(byte, b'A'..=b'Z' | b' '))
        {
            return None;
        }
        let mut app = Self::new();
        if !name.is_empty() {
            let mut contact = Contact::EMPTY;
            contact.name[..name.len()].copy_from_slice(name);
            contact.name_length = name.len() as u8;
            app.entries[0] = contact;
            app.count = 1;
        }
        Some(app)
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        if payload.len() < 5 {
            return None;
        }
        let with_key = match payload[..4].try_into().ok()? {
            MAGIC_V2 => true,
            MAGIC_V1 => false,
            _ => return None,
        };
        let count = payload[4] as usize;
        if count > MAX_CONTACTS {
            return None;
        }
        let mut app = Self::new();
        let mut cursor = 5;
        for index in 0..count {
            let name_length = *payload.get(cursor)? as usize;
            cursor += 1;
            if name_length == 0 || name_length > NAME_CAPACITY {
                return None;
            }
            let name_end = cursor.checked_add(name_length)?;
            let name = payload.get(cursor..name_end)?;
            if name.iter().any(|byte| !matches!(byte, b'A'..=b'Z' | b' ')) {
                return None;
            }
            cursor = name_end;

            let phone_length = *payload.get(cursor)? as usize;
            cursor += 1;
            if phone_length > PHONE_CAPACITY {
                return None;
            }
            let phone_end = cursor.checked_add(phone_length)?;
            let phone = payload.get(cursor..phone_end)?;
            if phone.iter().any(|byte| !byte.is_ascii_digit()) {
                return None;
            }
            cursor = phone_end;

            let mut key_length = 0;
            let mut key: &[u8] = &[];
            if with_key {
                key_length = *payload.get(cursor)? as usize;
                cursor += 1;
                if key_length > KEY_CAPACITY {
                    return None;
                }
                let key_end = cursor.checked_add(key_length)?;
                key = payload.get(cursor..key_end)?;
                if key.iter().any(|byte| !is_fingerprint_byte(*byte)) {
                    return None;
                }
                cursor = key_end;
            }

            let mut contact = Contact::EMPTY;
            contact.name[..name_length].copy_from_slice(name);
            contact.name_length = name_length as u8;
            contact.phone[..phone_length].copy_from_slice(phone);
            contact.phone_length = phone_length as u8;
            contact.key[..key_length].copy_from_slice(key);
            contact.key_length = key_length as u8;
            app.entries[index] = contact;
        }
        if cursor != payload.len() {
            return None;
        }
        app.count = count as u8;
        Some(app)
    }

    pub fn encode(&self, output: &mut [u8; CONTACTS_BLOB_CAPACITY]) -> Option<usize> {
        let count = self.count as usize;
        if count > MAX_CONTACTS || self.selected as usize >= MAX_CONTACTS {
            return None;
        }
        output.fill(0);
        output[..4].copy_from_slice(&MAGIC_V2);
        output[4] = self.count;
        let mut cursor = 5;
        for contact in self.entries.iter().take(count) {
            let name = contact.name();
            let phone = contact.phone();
            let key = contact.key();
            if name.is_empty()
                || name.len() > NAME_CAPACITY
                || name.iter().any(|byte| !matches!(byte, b'A'..=b'Z' | b' '))
                || phone.len() > PHONE_CAPACITY
                || phone.iter().any(|byte| !byte.is_ascii_digit())
                || key.len() > KEY_CAPACITY
                || key.iter().any(|byte| !is_fingerprint_byte(*byte))
            {
                return None;
            }
            output[cursor] = name.len() as u8;
            cursor += 1;
            output[cursor..cursor + name.len()].copy_from_slice(name);
            cursor += name.len();
            output[cursor] = phone.len() as u8;
            cursor += 1;
            output[cursor..cursor + phone.len()].copy_from_slice(phone);
            cursor += phone.len();
            output[cursor] = key.len() as u8;
            cursor += 1;
            output[cursor..cursor + key.len()].copy_from_slice(key);
            cursor += key.len();
        }
        Some(cursor)
    }

    /// Apply a key to the list/editor. The second result reports a committed
    /// contact-list change that must be written to the encrypted vault.
    pub fn apply_key(&mut self, code: u16, value: u32) -> ContactUpdate {
        if value != 1 && value != 2 {
            return unchanged();
        }
        match self.mode {
            EditMode::Browse => self.apply_browse_key(code),
            EditMode::Name => self.apply_name_key(code),
            EditMode::Phone => self.apply_phone_key(code),
            EditMode::Key => self.apply_fingerprint_key(code),
        }
    }

    fn apply_browse_key(&mut self, code: u16) -> ContactUpdate {
        let count = self.count as usize;
        match code {
            KEY_ENTER if count < MAX_CONTACTS => {
                self.draft_name.fill(0);
                self.draft_name_length = 0;
                self.draft_phone.fill(0);
                self.draft_phone_length = 0;
                self.draft_key.fill(0);
                self.draft_key_length = 0;
                self.mode = EditMode::Name;
                changed(false)
            }
            KEY_UP if count > 0 => {
                self.selected = if self.selected == 0 {
                    self.count - 1
                } else {
                    self.selected - 1
                };
                changed(false)
            }
            KEY_DOWN if count > 0 => {
                self.selected = (self.selected + 1) % self.count;
                changed(false)
            }
            KEY_DELETE if count > 0 => {
                let selected = self.selected as usize;
                for index in selected..count - 1 {
                    self.entries[index] = self.entries[index + 1];
                }
                self.entries[count - 1] = Contact::EMPTY;
                self.count -= 1;
                self.selected = if self.count == 0 {
                    0
                } else {
                    (self.selected as usize).min(self.count as usize - 1) as u8
                };
                changed(true)
            }
            _ => unchanged(),
        }
    }

    fn apply_name_key(&mut self, code: u16) -> ContactUpdate {
        if code == KEY_ENTER {
            if self.draft_name_length == 0 {
                return unchanged();
            }
            self.mode = EditMode::Phone;
            return changed(false);
        }
        if code == KEY_BACKSPACE {
            if self.draft_name_length == 0 {
                self.mode = EditMode::Browse;
            } else {
                self.draft_name_length -= 1;
                self.draft_name[self.draft_name_length as usize] = 0;
            }
            return changed(false);
        }
        let character = if code == KEY_SPACE {
            Some(b' ')
        } else {
            evdev_letter(code)
        };
        let Some(character) = character else {
            return unchanged();
        };
        let length = self.draft_name_length as usize;
        if length == NAME_CAPACITY {
            return unchanged();
        }
        self.draft_name[length] = character;
        self.draft_name_length += 1;
        changed(false)
    }

    fn apply_phone_key(&mut self, code: u16) -> ContactUpdate {
        if code == KEY_ENTER {
            if self.draft_name_length == 0 || self.count as usize == MAX_CONTACTS {
                return unchanged();
            }
            self.mode = EditMode::Key;
            changed(false)
        } else if code == KEY_BACKSPACE {
            if self.draft_phone_length == 0 {
                self.mode = EditMode::Name;
            } else {
                self.draft_phone_length -= 1;
                self.draft_phone[self.draft_phone_length as usize] = 0;
            }
            changed(false)
        } else if let Some(digit) = evdev_digit(code) {
            let length = self.draft_phone_length as usize;
            if length == PHONE_CAPACITY {
                return unchanged();
            }
            self.draft_phone[length] = digit;
            self.draft_phone_length += 1;
            changed(false)
        } else {
            unchanged()
        }
    }

    fn apply_fingerprint_key(&mut self, code: u16) -> ContactUpdate {
        if code == KEY_ENTER {
            let count = self.count as usize;
            let name_length = self.draft_name_length as usize;
            if name_length == 0 || count == MAX_CONTACTS {
                return unchanged();
            }
            let mut contact = Contact::EMPTY;
            contact.name[..name_length].copy_from_slice(&self.draft_name[..name_length]);
            contact.name_length = name_length as u8;
            let phone_length = self.draft_phone_length as usize;
            contact.phone[..phone_length].copy_from_slice(&self.draft_phone[..phone_length]);
            contact.phone_length = phone_length as u8;
            let key_length = self.draft_key_length as usize;
            contact.key[..key_length].copy_from_slice(&self.draft_key[..key_length]);
            contact.key_length = key_length as u8;
            self.entries[count] = contact;
            self.count += 1;
            self.selected = self.count - 1;
            self.mode = EditMode::Browse;
            changed(true)
        } else if code == KEY_BACKSPACE {
            if self.draft_key_length == 0 {
                self.mode = EditMode::Phone;
            } else {
                self.draft_key_length -= 1;
                self.draft_key[self.draft_key_length as usize] = 0;
            }
            changed(false)
        } else if let Some(character) = fingerprint_character(code) {
            let length = self.draft_key_length as usize;
            if length == KEY_CAPACITY {
                return unchanged();
            }
            self.draft_key[length] = character;
            self.draft_key_length += 1;
            changed(false)
        } else {
            unchanged()
        }
    }

    /// Format a read-only snapshot for the trusted display process.
    pub fn render(&self, output: &mut [u8; 128]) -> usize {
        let mut writer = SnapshotWriter::new(output);
        match self.mode {
            EditMode::Browse if self.count == 0 => {
                writer.text(b"NO CONTACTS\nENTER TO ADD");
            }
            EditMode::Browse => {
                writer.text(b"CONTACT ");
                writer.digit(self.selected + 1);
                writer.text(b" OF ");
                writer.digit(self.count);
                writer.text(b"\nNAME ");
                let contact = &self.entries[self.selected as usize];
                writer.text(contact.name());
                writer.text(b"\nPHONE ");
                writer.text(contact.phone());
                writer.text(b"\nKEY ");
                if contact.key_length == 0 {
                    writer.text(b"-");
                } else {
                    writer.text(contact.key());
                }
            }
            EditMode::Name | EditMode::Phone => {
                writer.text(if self.mode == EditMode::Name {
                    b"ADD NAME\nNAME "
                } else {
                    b"ADD PHONE\nNAME "
                });
                writer.text(&self.draft_name[..self.draft_name_length as usize]);
                writer.text(b"\nPHONE ");
                writer.text(&self.draft_phone[..self.draft_phone_length as usize]);
            }
            EditMode::Key => {
                writer.text(b"ADD KEY\nNAME ");
                writer.text(&self.draft_name[..self.draft_name_length as usize]);
                writer.text(b"\nPHONE ");
                writer.text(&self.draft_phone[..self.draft_phone_length as usize]);
                writer.text(b"\nKEY ");
                writer.text(&self.draft_key[..self.draft_key_length as usize]);
            }
        }
        writer.length
    }
}

const fn unchanged() -> ContactUpdate {
    ContactUpdate {
        changed: false,
        persist: false,
    }
}

const fn changed(persist: bool) -> ContactUpdate {
    ContactUpdate {
        changed: true,
        persist,
    }
}

const fn is_fingerprint_byte(byte: u8) -> bool {
    matches!(byte, b'0'..=b'9' | b'A'..=b'F')
}

fn fingerprint_character(code: u16) -> Option<u8> {
    if let Some(digit) = evdev_digit(code) {
        return Some(digit);
    }
    match evdev_letter(code) {
        Some(letter @ b'A'..=b'F') => Some(letter),
        _ => None,
    }
}

fn evdev_digit(code: u16) -> Option<u8> {
    match code {
        2 => Some(b'1'),
        3 => Some(b'2'),
        4 => Some(b'3'),
        5 => Some(b'4'),
        6 => Some(b'5'),
        7 => Some(b'6'),
        8 => Some(b'7'),
        9 => Some(b'8'),
        10 => Some(b'9'),
        11 => Some(b'0'),
        _ => None,
    }
}

struct SnapshotWriter<'a> {
    output: &'a mut [u8; 128],
    length: usize,
}

impl SnapshotWriter<'_> {
    fn new(output: &mut [u8; 128]) -> SnapshotWriter<'_> {
        output.fill(0);
        SnapshotWriter { output, length: 0 }
    }

    fn text(&mut self, text: &[u8]) {
        let available = self.output.len() - self.length;
        let count = text.len().min(available);
        self.output[self.length..self.length + count].copy_from_slice(&text[..count]);
        self.length += count;
    }

    fn digit(&mut self, digit: u8) {
        self.output[self.length] = b'0' + digit;
        self.length += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(app: &mut ContactsApp, code: u16) -> ContactUpdate {
        app.apply_key(code, 1)
    }

    fn finish_empty_key(app: &mut ContactsApp) -> ContactUpdate {
        // Phone Enter moves to Key; Key Enter commits.
        assert_eq!(key(app, KEY_ENTER), changed(false));
        key(app, KEY_ENTER)
    }

    #[test]
    fn adds_multiple_contacts_and_round_trips_canonical_blob() {
        let mut app = ContactsApp::new();
        assert_eq!(key(&mut app, KEY_ENTER), changed(false));
        key(&mut app, 30); // A
        key(&mut app, KEY_ENTER);
        for code in [6, 6, 6, 5] {
            // 5554
            key(&mut app, code);
        }
        assert_eq!(finish_empty_key(&mut app), changed(true));
        key(&mut app, KEY_ENTER);
        key(&mut app, 48); // B
        key(&mut app, KEY_ENTER);
        key(&mut app, 9); // 8
        key(&mut app, 7); // 6
        assert_eq!(key(&mut app, KEY_ENTER), changed(false)); // to Key
        key(&mut app, 30); // A
        key(&mut app, 48); // B
        key(&mut app, 2); // 1
        key(&mut app, 3); // 2
        assert_eq!(key(&mut app, KEY_ENTER), changed(true));
        assert_eq!(app.count(), 2);
        assert_eq!(app.selected().unwrap().name(), b"B");
        assert_eq!(app.selected().unwrap().phone(), b"86");
        assert_eq!(app.selected().unwrap().key(), b"AB12");
        assert_eq!(app.entries[0].key(), b"");

        let mut encoded = [0; CONTACTS_BLOB_CAPACITY];
        let length = app.encode(&mut encoded).unwrap();
        assert_eq!(&encoded[..4], &MAGIC_V2);
        let decoded = ContactsApp::decode(&encoded[..length]).unwrap();
        assert_eq!(decoded.count(), 2);
        assert_eq!(decoded.entries[0].name(), b"A");
        assert_eq!(decoded.entries[0].phone(), b"5554");
        assert_eq!(decoded.entries[0].key(), b"");
        assert_eq!(decoded.entries[1].name(), b"B");
        assert_eq!(decoded.entries[1].phone(), b"86");
        assert_eq!(decoded.entries[1].key(), b"AB12");
    }

    #[test]
    fn decodes_pct1_blobs_with_empty_fingerprints() {
        let payload = b"PCT1\x01\x01A\x045554";
        let decoded = ContactsApp::decode(payload).unwrap();
        assert_eq!(decoded.count(), 1);
        assert_eq!(decoded.entries[0].name(), b"A");
        assert_eq!(decoded.entries[0].phone(), b"5554");
        assert_eq!(decoded.entries[0].key(), b"");

        let mut encoded = [0; CONTACTS_BLOB_CAPACITY];
        let length = decoded.encode(&mut encoded).unwrap();
        assert_eq!(&encoded[..4], &MAGIC_V2);
        let round_trip = ContactsApp::decode(&encoded[..length]).unwrap();
        assert_eq!(round_trip.entries[0].key(), b"");
    }

    #[test]
    fn list_selection_deletion_and_legacy_migration_work() {
        let mut app = ContactsApp::from_legacy_name(b"OLD NAME").unwrap();
        assert_eq!(app.count(), 1);
        assert_eq!(app.selected().unwrap().name(), b"OLD NAME");
        key(&mut app, KEY_ENTER);
        key(&mut app, 48);
        key(&mut app, KEY_ENTER);
        assert_eq!(finish_empty_key(&mut app), changed(true));
        assert_eq!(app.count(), 2);
        key(&mut app, KEY_UP);
        assert_eq!(app.selected().unwrap().name(), b"OLD NAME");
        key(&mut app, KEY_DELETE);
        assert_eq!(app.count(), 1);
        assert_eq!(app.selected().unwrap().name(), b"B");
    }

    #[test]
    fn decoder_rejects_invalid_lengths_bytes_and_trailing_data() {
        let mut app = ContactsApp::new();
        key(&mut app, KEY_ENTER);
        key(&mut app, 30);
        key(&mut app, KEY_ENTER);
        key(&mut app, 2);
        assert_eq!(finish_empty_key(&mut app), changed(true));
        let mut encoded = [0; CONTACTS_BLOB_CAPACITY];
        let length = app.encode(&mut encoded).unwrap();
        assert!(ContactsApp::decode(&encoded[..length]).is_some());

        let mut bad_name_length = encoded;
        bad_name_length[5] = 0xff;
        assert!(ContactsApp::decode(&bad_name_length[..length]).is_none());
        let mut bad_phone = encoded;
        let phone_offset = 5 + 1 + 1 + 1;
        bad_phone[phone_offset] = b'X';
        assert!(ContactsApp::decode(&bad_phone[..length]).is_none());
        assert!(ContactsApp::decode(&encoded[..length + 1]).is_none());
    }

    #[test]
    fn fingerprint_rejects_non_hex_letters_and_caps_at_eight() {
        let mut app = ContactsApp::new();
        key(&mut app, KEY_ENTER);
        key(&mut app, 30); // A
        key(&mut app, KEY_ENTER);
        key(&mut app, KEY_ENTER); // phone empty -> key
        assert!(!key(&mut app, 34).changed); // G rejected
        for code in [30, 48, 46, 32, 18, 33, 2, 3] {
            // A B C D E F 1 2
            assert!(key(&mut app, code).changed);
        }
        assert!(!key(&mut app, 4).changed); // capacity full
        assert_eq!(&app.draft_key[..8], b"ABCDEF12");
    }

    #[test]
    fn rendered_contacts_are_bounded_and_include_selection_details() {
        let mut app = ContactsApp::from_legacy_name(b"ALICE").unwrap();
        let mut output = [0; 128];
        let length = app.render(&mut output);
        assert_eq!(&output[..length], b"CONTACT 1 OF 1\nNAME ALICE\nPHONE \nKEY -");
        assert!(!app.apply_key(1, 0).changed);
    }
}
