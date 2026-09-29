//! Small app-owned text snapshots exposed to the trusted display client.

use core::sync::atomic::{AtomicU32, Ordering};

/// Bounded byte capacity for a private app text snapshot. Notes use the full
/// buffer; the legacy packed-value migration and Contacts field stay at 12.
pub const APP_VIEW_TEXT_CAPACITY: usize = 128;
pub const LEGACY_TEXT_CAPACITY: usize = 12;
pub const KEY_BACKSPACE: u16 = 14;
pub const KEY_SPACE: u16 = 57;
pub const KEY_ENTER: u16 = 28;
pub const KEY_UP: u16 = 103;
pub const KEY_DOWN: u16 = 108;

/// C-compatible app view model. Each app can write only its own mapped page;
/// the graphics client maps both snapshots read-only.
#[repr(C)]
pub struct AppViewState {
    pub revision: AtomicU32,
    pub length: u32,
    pub text: [u8; APP_VIEW_TEXT_CAPACITY],
    /// First wrapped Notes row visible in the GPU client; zero for Contacts.
    pub scroll_start_line: u32,
    /// Notes-only presentation metadata consumed by the input router/GPU client.
    pub notes_mode: u32,
    pub selected_note: u32,
    pub note_count: u32,
    pub notes_delete_armed: u32,
    /// 1 while the Notes title field is displayed; otherwise 0.
    pub notes_field: u32,
}

impl AppViewState {
    pub const fn new() -> Self {
        Self {
            revision: AtomicU32::new(0),
            length: 0,
            text: [0; APP_VIEW_TEXT_CAPACITY],
            scroll_start_line: 0,
            notes_mode: 0,
            selected_note: 0,
            note_count: 1,
            notes_delete_armed: 0,
            notes_field: 0,
        }
    }

    /// Apply a bounded set of evdev key presses to this app's visible text.
    /// Returns true only when the view changed.
    #[cfg(test)]
    pub fn apply_key(&mut self, code: u16, value: u32) -> bool {
        self.apply_key_with_policy(code, value, APP_VIEW_TEXT_CAPACITY, true)
    }

    /// Apply a key while enforcing the owning app's field size and newline
    /// policy. Notes opt into multiline input; Contacts remains a short name.
    pub fn apply_key_with_policy(
        &mut self,
        code: u16,
        value: u32,
        maximum_length: usize,
        multiline: bool,
    ) -> bool {
        if value != 1 && value != 2 {
            return false;
        }

        if maximum_length > APP_VIEW_TEXT_CAPACITY {
            return false;
        }
        let length = self.length as usize;
        if length > maximum_length {
            return false;
        }
        let mut next_text = self.text;
        let next_length = if code == KEY_BACKSPACE {
            if length == 0 {
                return false;
            }
            next_text[length - 1] = 0;
            length - 1
        } else if code == KEY_ENTER && multiline {
            if length == maximum_length {
                return false;
            }
            next_text[length] = b'\n';
            length + 1
        } else if let Some(character) = evdev_letter(code) {
            if length == maximum_length {
                return false;
            }
            next_text[length] = character;
            length + 1
        } else if let Some(character) = evdev_digit(code) {
            if length == maximum_length {
                return false;
            }
            next_text[length] = character;
            length + 1
        } else if code == KEY_SPACE {
            if length == maximum_length {
                return false;
            }
            next_text[length] = b' ';
            length + 1
        } else {
            return false;
        };

        self.scroll_start_line = if multiline {
            maximum_scroll_start(&next_text[..next_length]) as u32
        } else {
            0
        };
        let odd_revision = self.revision.load(Ordering::Relaxed).wrapping_add(1) | 1;
        self.revision.store(odd_revision, Ordering::SeqCst);
        self.length = next_length as u32;
        self.text = next_text;
        self.revision
            .store(odd_revision.wrapping_add(1), Ordering::SeqCst);
        true
    }

    /// Move the Notes viewport without changing or persisting note contents.
    pub fn scroll_notes_by_key(&mut self, code: u16, value: u32) -> bool {
        if value != 1 && value != 2 {
            return false;
        }
        let maximum = maximum_scroll_start(&self.text[..self.length as usize]) as u32;
        let next = match code {
            KEY_UP if self.scroll_start_line > 0 => self.scroll_start_line - 1,
            KEY_DOWN if self.scroll_start_line < maximum => self.scroll_start_line + 1,
            _ => return false,
        };
        if next == self.scroll_start_line {
            return false;
        }
        let odd_revision = self.revision.load(Ordering::Relaxed).wrapping_add(1) | 1;
        self.revision.store(odd_revision, Ordering::SeqCst);
        self.scroll_start_line = next;
        self.revision
            .store(odd_revision.wrapping_add(1), Ordering::SeqCst);
        true
    }

    /// Encode this prototype field into the current vault's 64-bit value.
    #[cfg(test)]
    pub fn packed_value(&self) -> Option<u64> {
        let length = self.length as usize;
        if length > LEGACY_TEXT_CAPACITY {
            return None;
        }
        let mut packed = 0u64;
        for (index, character) in self.text[..length].iter().copied().enumerate() {
            let symbol = match character {
                b'A'..=b'Z' => u64::from(character - b'A' + 1),
                b' ' => 27,
                _ => return None,
            };
            packed |= symbol << (index * 5);
        }
        Some(packed)
    }

    /// Restore a canonical vault value. Zero symbols mark the end; malformed
    /// values are rejected rather than displayed as arbitrary memory bytes.
    pub fn restore_packed(&mut self, packed: u64) -> bool {
        let mut next_text = [0; APP_VIEW_TEXT_CAPACITY];
        let mut next_length = 0;
        let mut ended = false;
        for index in 0..LEGACY_TEXT_CAPACITY {
            let symbol = ((packed >> (index * 5)) & 0x1f) as u8;
            if symbol == 0 {
                ended = true;
                continue;
            }
            if ended {
                return false;
            }
            next_text[index] = match symbol {
                1..=26 => b'A' + symbol - 1,
                27 => b' ',
                _ => return false,
            };
            next_length += 1;
        }
        if packed >> (LEGACY_TEXT_CAPACITY * 5) != 0 {
            return false;
        }
        self.text = next_text;
        self.length = next_length;
        true
    }

    /// Restore this prototype field from its new byte-oriented blob payload.
    #[cfg(test)]
    pub fn restore_text_bytes(&mut self, text: &[u8]) -> bool {
        self.restore_text_bytes_with_policy(text, APP_VIEW_TEXT_CAPACITY, true)
    }

    #[cfg(test)]
    pub fn restore_text_bytes_with_policy(
        &mut self,
        text: &[u8],
        maximum_length: usize,
        multiline: bool,
    ) -> bool {
        if maximum_length > APP_VIEW_TEXT_CAPACITY
            || text.len() > maximum_length
            || text.iter().any(|character| {
                !matches!(character, b'A'..=b'Z' | b'0'..=b'9' | b' ')
                    && !(multiline && *character == b'\n')
            })
        {
            return false;
        }
        self.text.fill(0);
        self.text[..text.len()].copy_from_slice(text);
        self.length = text.len() as u32;
        self.scroll_start_line = if multiline {
            maximum_scroll_start(text) as u32
        } else {
            0
        };
        true
    }

    /// Publish a Contacts display snapshot after validating its printable
    /// alphabet and updating the shared-page sequence counter.
    pub fn set_display_text(&mut self, text: &[u8]) -> bool {
        if text.len() > APP_VIEW_TEXT_CAPACITY
            || text
                .iter()
                .any(|byte| !matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-'))
        {
            return false;
        }
        if self.length as usize == text.len() && &self.text[..text.len()] == text {
            return true;
        }
        let odd_revision = self.revision.load(Ordering::Relaxed).wrapping_add(1) | 1;
        self.revision.store(odd_revision, Ordering::SeqCst);
        self.text.fill(0);
        self.text[..text.len()].copy_from_slice(text);
        self.length = text.len() as u32;
        self.scroll_start_line = 0;
        self.revision
            .store(odd_revision.wrapping_add(1), Ordering::SeqCst);
        true
    }

    /// Publish the currently selected note into the read-only render snapshot.
    pub fn publish_notes_editor(
        &mut self,
        text: &[u8],
        note_count: usize,
        selected_note: usize,
    ) -> bool {
        self.publish_notes_state(text, false, false, note_count, selected_note, false)
    }

    /// Publish the selected note's title into the editor snapshot.
    pub fn publish_notes_title(
        &mut self,
        title: &[u8],
        note_count: usize,
        selected_note: usize,
    ) -> bool {
        self.publish_notes_state(title, false, true, note_count, selected_note, false)
    }

    /// Publish a bounded note-list snapshot and its selection metadata.
    pub fn publish_notes_list(
        &mut self,
        text: &[u8],
        note_count: usize,
        selected_note: usize,
        delete_armed: bool,
    ) -> bool {
        self.publish_notes_state(text, true, false, note_count, selected_note, delete_armed)
    }

    fn publish_notes_state(
        &mut self,
        text: &[u8],
        list_mode: bool,
        title_mode: bool,
        note_count: usize,
        selected_note: usize,
        delete_armed: bool,
    ) -> bool {
        if text.len() > APP_VIEW_TEXT_CAPACITY
            || note_count == 0
            || note_count > 4
            || selected_note >= note_count
            || text
                .iter()
                .any(|byte| !matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'.'))
        {
            return false;
        }
        let odd_revision = self.revision.load(Ordering::Relaxed).wrapping_add(1) | 1;
        self.revision.store(odd_revision, Ordering::SeqCst);
        self.text.fill(0);
        self.text[..text.len()].copy_from_slice(text);
        self.length = text.len() as u32;
        self.scroll_start_line = if list_mode {
            0
        } else {
            maximum_scroll_start(text) as u32
        };
        self.notes_mode = u32::from(list_mode);
        self.selected_note = selected_note as u32;
        self.note_count = note_count as u32;
        self.notes_delete_armed = u32::from(delete_armed);
        self.notes_field = u32::from(title_mode);
        self.revision
            .store(odd_revision.wrapping_add(1), Ordering::SeqCst);
        true
    }
}

fn maximum_scroll_start(text: &[u8]) -> usize {
    wrapped_line_count(text).saturating_sub(3)
}

fn wrapped_line_count(text: &[u8]) -> usize {
    let mut lines = 1;
    let mut column = 0;
    for &character in text {
        if character == b'\n' {
            lines += 1;
            column = 0;
        } else {
            if column == 21 {
                lines += 1;
                column = 0;
            }
            column += 1;
        }
    }
    lines
}

pub(crate) const fn evdev_letter(code: u16) -> Option<u8> {
    match code {
        30 => Some(b'A'),
        48 => Some(b'B'),
        46 => Some(b'C'),
        32 => Some(b'D'),
        18 => Some(b'E'),
        33 => Some(b'F'),
        34 => Some(b'G'),
        35 => Some(b'H'),
        23 => Some(b'I'),
        36 => Some(b'J'),
        37 => Some(b'K'),
        38 => Some(b'L'),
        50 => Some(b'M'),
        49 => Some(b'N'),
        24 => Some(b'O'),
        25 => Some(b'P'),
        16 => Some(b'Q'),
        19 => Some(b'R'),
        31 => Some(b'S'),
        20 => Some(b'T'),
        22 => Some(b'U'),
        47 => Some(b'V'),
        17 => Some(b'W'),
        45 => Some(b'X'),
        21 => Some(b'Y'),
        44 => Some(b'Z'),
        _ => None,
    }
}

pub(crate) const fn evdev_digit(code: u16) -> Option<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_pressed_letters_and_ignores_key_release() {
        let mut view = AppViewState::new();
        assert!(view.apply_key(30, 1)); // A
        assert!(!view.apply_key(30, 0));
        assert!(view.apply_key(48, 1)); // B
        assert_eq!(view.length, 2);
        assert_eq!(&view.text[..2], b"AB");
        assert_eq!(view.revision.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn backspace_and_capacity_are_bounded() {
        let mut view = AppViewState::new();
        assert!(!view.apply_key(KEY_BACKSPACE, 1));
        for _ in 0..APP_VIEW_TEXT_CAPACITY {
            assert!(view.apply_key(30, 1));
        }
        assert!(!view.apply_key(48, 1));
        assert!(view.apply_key(KEY_BACKSPACE, 1));
        assert_eq!(view.length as usize, APP_VIEW_TEXT_CAPACITY - 1);
        assert_eq!(view.text[APP_VIEW_TEXT_CAPACITY - 1], 0);
    }

    #[test]
    fn notes_enter_adds_a_new_line_and_backspace_removes_it() {
        let mut view = AppViewState::new();
        assert!(view.apply_key(30, 1)); // A
        assert!(view.apply_key(KEY_ENTER, 1));
        assert!(view.apply_key(48, 1)); // B
        assert_eq!(&view.text[..view.length as usize], b"A\nB");
        assert!(view.apply_key(KEY_BACKSPACE, 1));
        assert_eq!(&view.text[..view.length as usize], b"A\n");
        assert!(view.apply_key(KEY_BACKSPACE, 1));
        assert_eq!(&view.text[..view.length as usize], b"A");
    }

    #[test]
    fn notes_scroll_changes_only_the_viewport_and_new_edits_return_to_bottom() {
        let mut view = AppViewState::new();
        let value = b"ONE\nTWO\nTHREE\nFOUR";
        assert!(view.restore_text_bytes(value));
        assert_eq!(view.scroll_start_line, 1);
        assert!(view.scroll_notes_by_key(KEY_UP, 1));
        assert_eq!(view.scroll_start_line, 0);
        assert_eq!(&view.text[..value.len()], value);
        assert!(!view.scroll_notes_by_key(KEY_UP, 1));
        assert!(!view.scroll_notes_by_key(KEY_DOWN, 0));
        assert!(view.scroll_notes_by_key(KEY_DOWN, 1));
        assert_eq!(view.scroll_start_line, 1);
        assert!(view.apply_key(48, 1)); // B
        assert_eq!(view.scroll_start_line, 1);
    }

    #[test]
    fn contacts_policy_rejects_enter_and_caps_names_at_twelve_symbols() {
        let mut view = AppViewState::new();
        assert!(!view.apply_key_with_policy(KEY_ENTER, 1, LEGACY_TEXT_CAPACITY, false));
        for _ in 0..LEGACY_TEXT_CAPACITY {
            assert!(view.apply_key_with_policy(30, 1, LEGACY_TEXT_CAPACITY, false));
        }
        assert!(!view.apply_key_with_policy(48, 1, LEGACY_TEXT_CAPACITY, false));
        assert_eq!(view.length as usize, LEGACY_TEXT_CAPACITY);
    }

    #[test]
    fn restores_and_preserves_multiline_text_bytes() {
        let mut view = AppViewState::new();
        let value = b"FIRST LINE\nSECOND LINE";
        assert!(view.restore_text_bytes(value));
        assert_eq!(&view.text[..view.length as usize], value);
        assert_eq!(view.packed_value(), None);
        assert!(!view.restore_text_bytes_with_policy(value, LEGACY_TEXT_CAPACITY, false));
    }

    #[test]
    fn rejects_non_text_keys_and_releases() {
        let mut view = AppViewState::new();
        assert!(!view.apply_key(1, 1));
        assert!(!view.apply_key(KEY_SPACE, 0));
        assert_eq!(view.length, 0);
    }

    #[test]
    fn packed_value_round_trips_the_full_prototype_field() {
        let mut view = AppViewState::new();
        for code in [30, KEY_SPACE, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37] {
            assert!(view.apply_key(code, 1));
        }
        let packed = view.packed_value().unwrap();
        let mut restored = AppViewState::new();
        assert!(restored.restore_packed(packed));
        assert_eq!(restored.length as usize, LEGACY_TEXT_CAPACITY);
        assert_eq!(&restored.text[..LEGACY_TEXT_CAPACITY], b"A BCDEFGHIJK");
        assert_eq!(restored.packed_value(), Some(packed));
    }

    #[test]
    fn packed_value_preserves_trailing_spaces_and_rejects_noncanonical_data() {
        let mut view = AppViewState::new();
        assert!(view.apply_key(30, 1));
        assert!(view.apply_key(KEY_SPACE, 1));
        let packed = view.packed_value().unwrap();
        let mut restored = AppViewState::new();
        assert!(restored.restore_packed(packed));
        assert_eq!(restored.length, 2);
        assert_eq!(&restored.text[..2], b"A ");
        assert!(!restored.restore_packed(28));
        assert!(!restored.restore_packed(1 | (2 << 10)));
    }
}
