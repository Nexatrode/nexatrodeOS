//! Bounded multi-note model and canonical encrypted-blob payload.

use crate::app_view::{AppViewState, KEY_DOWN, KEY_ENTER, KEY_UP};

pub const MAX_NOTES: usize = 4;
pub const NOTE_TITLE_CAPACITY: usize = 21;
pub const NOTE_TEXT_CAPACITY: usize = 128;
pub const NOTES_BLOB_CAPACITY: usize =
    6 + MAX_NOTES * (1 + NOTE_TITLE_CAPACITY + 1 + NOTE_TEXT_CAPACITY);

const V1_MAGIC: [u8; 4] = *b"PNT1";
const MAGIC: [u8; 4] = *b"PNT2";
const KEY_F1: u16 = 59;
const KEY_F2: u16 = 60;
const KEY_F3: u16 = 61;
const KEY_F4: u16 = 62;
const KEY_F5: u16 = 63;
const KEY_F6: u16 = 64;
const KEY_F7: u16 = 65;
const KEY_INSERT: u16 = 110;
const KEY_DELETE: u16 = 111;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Note {
    title: [u8; NOTE_TITLE_CAPACITY],
    title_length: u8,
    text: [u8; NOTE_TEXT_CAPACITY],
    length: u8,
}

impl Note {
    const EMPTY: Self = Self {
        title: [0; NOTE_TITLE_CAPACITY],
        title_length: 0,
        text: [0; NOTE_TEXT_CAPACITY],
        length: 0,
    };

    fn from_parts(title: &[u8], text: &[u8]) -> Option<Self> {
        if title.len() > NOTE_TITLE_CAPACITY
            || text.len() > NOTE_TEXT_CAPACITY
            || !valid_title(title)
            || !valid_text(text)
        {
            return None;
        }
        let mut note = Self::EMPTY;
        note.title[..title.len()].copy_from_slice(title);
        note.title_length = title.len() as u8;
        note.text[..text.len()].copy_from_slice(text);
        note.length = text.len() as u8;
        Some(note)
    }

    fn title(&self) -> &[u8] {
        &self.title[..self.title_length as usize]
    }

    fn text(&self) -> &[u8] {
        &self.text[..self.length as usize]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Screen {
    Editor,
    List,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotesUpdate {
    pub changed: bool,
    pub persist: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotesApp {
    notes: [Note; MAX_NOTES],
    count: u8,
    selected: u8,
    screen: Screen,
    editing_title: bool,
    delete_armed: bool,
}

impl NotesApp {
    pub const fn new() -> Self {
        let mut notes = [Note::EMPTY; MAX_NOTES];
        notes[0] = note_one();
        Self {
            notes,
            count: 1,
            selected: 0,
            screen: Screen::Editor,
            editing_title: false,
            delete_armed: false,
        }
    }

    /// Decode the canonical `PNT2` collection. All entries are validated
    /// before the model becomes available to the UI.
    pub fn decode(payload: &[u8]) -> Option<Self> {
        if payload.len() < 6 || payload[..4] != MAGIC {
            return None;
        }
        let count = payload[4] as usize;
        let selected = payload[5] as usize;
        if count == 0 || count > MAX_NOTES || selected >= count {
            return None;
        }
        let mut app = Self::new();
        app.count = count as u8;
        app.selected = selected as u8;
        let mut cursor = 6;
        for index in 0..count {
            let title_length = *payload.get(cursor)? as usize;
            cursor += 1;
            let title_end = cursor.checked_add(title_length)?;
            let title = payload.get(cursor..title_end)?;
            cursor = title_end;
            let text_length = *payload.get(cursor)? as usize;
            cursor += 1;
            let text_end = cursor.checked_add(text_length)?;
            let text = payload.get(cursor..text_end)?;
            cursor = text_end;
            app.notes[index] = Note::from_parts(title, text)?;
        }
        if cursor != payload.len() {
            return None;
        }
        Some(app)
    }

    /// Read the immediately previous `PNT1` body-only collection and assign a
    /// title from each body's first line before it is rewritten as `PNT2`.
    pub fn from_v1_collection(payload: &[u8]) -> Option<Self> {
        if payload.len() < 6 || payload[..4] != V1_MAGIC {
            return None;
        }
        let count = payload[4] as usize;
        let selected = payload[5] as usize;
        if count == 0 || count > MAX_NOTES || selected >= count {
            return None;
        }
        let mut app = Self::new();
        app.count = count as u8;
        app.selected = selected as u8;
        let mut cursor = 6;
        for index in 0..count {
            let length = *payload.get(cursor)? as usize;
            cursor += 1;
            let end = cursor.checked_add(length)?;
            let text = payload.get(cursor..end)?;
            cursor = end;
            if !valid_text(text) {
                return None;
            }
            let mut title = [0; NOTE_TITLE_CAPACITY];
            let title_length = title_from_body(text, index, &mut title);
            app.notes[index] = Note::from_parts(&title[..title_length], text)?;
        }
        if cursor != payload.len() {
            return None;
        }
        Some(app)
    }

    /// Convert the oldest raw single-note payload to a one-note collection.
    pub fn from_legacy_text(text: &[u8]) -> Option<Self> {
        if !valid_text(text) {
            return None;
        }
        let mut app = Self::new();
        let mut title = [0; NOTE_TITLE_CAPACITY];
        let title_length = title_from_body(text, 0, &mut title);
        app.notes[0] = Note::from_parts(&title[..title_length], text)?;
        Some(app)
    }

    pub fn encode(&self, output: &mut [u8; NOTES_BLOB_CAPACITY]) -> Option<usize> {
        let count = self.count as usize;
        let selected = self.selected as usize;
        if count == 0 || count > MAX_NOTES || selected >= count {
            return None;
        }
        output.fill(0);
        output[..4].copy_from_slice(&MAGIC);
        output[4] = self.count;
        output[5] = self.selected;
        let mut cursor = 6;
        for note in self.notes.iter().take(count) {
            let title = note.title();
            let text = note.text();
            if title.len() > NOTE_TITLE_CAPACITY
                || text.len() > NOTE_TEXT_CAPACITY
                || !valid_title(title)
                || !valid_text(text)
            {
                return None;
            }
            output[cursor] = title.len() as u8;
            cursor += 1;
            output[cursor..cursor + title.len()].copy_from_slice(title);
            cursor += title.len();
            output[cursor] = text.len() as u8;
            cursor += 1;
            output[cursor..cursor + text.len()].copy_from_slice(text);
            cursor += text.len();
        }
        Some(cursor)
    }

    pub fn current_text(&self) -> &[u8] {
        self.notes[self.selected as usize].text()
    }

    pub fn current_title(&self) -> &[u8] {
        self.notes[self.selected as usize].title()
    }

    pub fn count(&self) -> usize {
        self.count as usize
    }

    pub fn selected(&self) -> usize {
        self.selected as usize
    }

    pub fn is_list_mode(&self) -> bool {
        self.screen == Screen::List
    }

    /// 0 is body editing, 1 is the list, and 2 is title editing.
    pub fn touch_mode(&self) -> u32 {
        if self.screen == Screen::List {
            1
        } else if self.editing_title {
            2
        } else {
            0
        }
    }

    pub fn render(&self, view: &mut AppViewState) {
        match self.screen {
            Screen::Editor if self.editing_title => self.render_title(view),
            Screen::Editor => self.render_editor(view),
            Screen::List => self.render_list(view),
        }
    }

    /// Apply an action to the model and app-owned render snapshot. Scrolling,
    /// field focus, list selection, and screen changes stay out of the vault.
    pub fn apply_key(&mut self, code: u16, value: u32, view: &mut AppViewState) -> NotesUpdate {
        if value != 1 && value != 2 {
            return unchanged();
        }
        match self.screen {
            Screen::Editor => self.apply_editor_key(code, value, view),
            Screen::List => self.apply_list_key(code, value, view),
        }
    }

    fn apply_editor_key(&mut self, code: u16, value: u32, view: &mut AppViewState) -> NotesUpdate {
        if code == KEY_F5 && value == 1 {
            self.screen = Screen::List;
            self.delete_armed = false;
            self.render_list(view);
            return changed(false);
        }
        if code == KEY_F6 && value == 1 {
            if self.editing_title {
                return unchanged();
            }
            self.editing_title = true;
            self.render_title(view);
            return changed(false);
        }
        if code == KEY_F7 && value == 1 {
            if !self.editing_title {
                return unchanged();
            }
            self.editing_title = false;
            self.render_editor(view);
            return changed(false);
        }
        if (code == KEY_UP || code == KEY_DOWN) && !self.editing_title {
            return if view.scroll_notes_by_key(code, value) {
                changed(false)
            } else {
                unchanged()
            };
        }
        if code == KEY_INSERT || code == KEY_DELETE {
            return unchanged();
        }

        let index = self.selected();
        if self.editing_title {
            if !view.apply_key_with_policy(code, value, NOTE_TITLE_CAPACITY, false) {
                return unchanged();
            }
            let title = &view.text[..view.length as usize];
            let Some(note) = Note::from_parts(title, self.notes[index].text()) else {
                return unchanged();
            };
            self.notes[index] = note;
        } else {
            if !view.apply_key_with_policy(code, value, NOTE_TEXT_CAPACITY, true) {
                return unchanged();
            }
            let text = &view.text[..view.length as usize];
            let Some(note) = Note::from_parts(self.notes[index].title(), text) else {
                return unchanged();
            };
            self.notes[index] = note;
        }
        changed(true)
    }

    fn apply_list_key(&mut self, code: u16, value: u32, view: &mut AppViewState) -> NotesUpdate {
        if code == KEY_UP || code == KEY_DOWN {
            let selected = self.selected();
            let next = match code {
                KEY_UP if selected > 0 => selected - 1,
                KEY_DOWN if selected + 1 < self.count() => selected + 1,
                _ => return unchanged(),
            };
            self.selected = next as u8;
            self.delete_armed = false;
            self.render_list(view);
            return changed(false);
        }
        if matches!(code, KEY_F5 | KEY_F1 | KEY_F2 | KEY_F3 | KEY_F4 | KEY_ENTER) && value == 1 {
            if let Some(index) = note_index_for_key(code, self.count(), self.selected()) {
                self.selected = index as u8;
            }
            self.screen = Screen::Editor;
            self.editing_title = false;
            self.delete_armed = false;
            self.render_editor(view);
            return changed(true);
        }
        if code == KEY_INSERT && value == 1 {
            if self.count() == MAX_NOTES {
                return unchanged();
            }
            let index = self.count();
            self.notes[index] = Note::EMPTY;
            self.count += 1;
            self.selected = index as u8;
            self.screen = Screen::Editor;
            self.editing_title = false;
            self.delete_armed = false;
            self.render_editor(view);
            return changed(true);
        }
        if code == KEY_DELETE && value == 1 {
            if !self.delete_armed {
                self.delete_armed = true;
                self.render_list(view);
                return changed(false);
            }
            self.delete_selected();
            self.delete_armed = false;
            self.render_list(view);
            return changed(true);
        }
        unchanged()
    }

    fn delete_selected(&mut self) {
        let count = self.count();
        let selected = self.selected();
        if count == 1 {
            self.notes[0] = note_one();
            self.selected = 0;
            return;
        }
        for index in selected..count - 1 {
            self.notes[index] = self.notes[index + 1];
        }
        self.notes[count - 1] = Note::EMPTY;
        self.count -= 1;
        if self.selected as usize >= self.count() {
            self.selected = self.count - 1;
        }
    }

    fn render_title(&self, view: &mut AppViewState) {
        let _ = view.publish_notes_title(self.current_title(), self.count(), self.selected());
    }

    fn render_editor(&self, view: &mut AppViewState) {
        let _ = view.publish_notes_editor(self.current_text(), self.count(), self.selected());
    }

    fn render_list(&self, view: &mut AppViewState) {
        let mut text = [0; crate::app_view::APP_VIEW_TEXT_CAPACITY];
        let mut length = 0;
        for index in 0..self.count() {
            if index > 0 {
                text[length] = b'\n';
                length += 1;
            }
            text[length] = b'1' + index as u8;
            text[length + 1] = b' ';
            length += 2;
            let title = self.notes[index].title();
            let preview = if title.is_empty() {
                b"UNTITLED".as_slice()
            } else {
                title
            };
            let copy_length = if preview.len() > 18 {
                15
            } else {
                preview.len()
            };
            text[length..length + copy_length].copy_from_slice(&preview[..copy_length]);
            length += copy_length;
            if preview.len() > copy_length {
                text[length..length + 3].copy_from_slice(b"...");
                length += 3;
            }
        }
        let _ = view.publish_notes_list(
            &text[..length],
            self.count(),
            self.selected(),
            self.delete_armed,
        );
    }
}

const fn note_one() -> Note {
    let mut note = Note::EMPTY;
    note.title[0] = b'N';
    note.title[1] = b'O';
    note.title[2] = b'T';
    note.title[3] = b'E';
    note.title[4] = b' ';
    note.title[5] = b'1';
    note.title_length = 6;
    note
}

fn title_from_body(body: &[u8], index: usize, output: &mut [u8; NOTE_TITLE_CAPACITY]) -> usize {
    let first_line = body.split(|byte| *byte == b'\n').next().unwrap_or_default();
    if !first_line.is_empty() {
        let length = first_line.len().min(NOTE_TITLE_CAPACITY);
        output[..length].copy_from_slice(&first_line[..length]);
        return length;
    }
    output[..5].copy_from_slice(b"NOTE ");
    output[5] = b'1' + index.min(MAX_NOTES - 1) as u8;
    6
}

fn valid_title(title: &[u8]) -> bool {
    title
        .iter()
        .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' '))
}

fn valid_text(text: &[u8]) -> bool {
    text.iter()
        .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n'))
}

fn note_index_for_key(code: u16, count: usize, selected: usize) -> Option<usize> {
    let index = match code {
        KEY_F1 => 0,
        KEY_F2 => 1,
        KEY_F3 => 2,
        KEY_F4 => 3,
        KEY_ENTER | KEY_F5 => return (selected < count).then_some(selected),
        _ => return None,
    };
    (index < count).then_some(index)
}

const fn unchanged() -> NotesUpdate {
    NotesUpdate {
        changed: false,
        persist: false,
    }
}

const fn changed(persist: bool) -> NotesUpdate {
    NotesUpdate {
        changed: true,
        persist,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::Ordering;

    #[test]
    fn legacy_and_pnt1_notes_migrate_to_titled_pnt2_collection() {
        let legacy = NotesApp::from_legacy_text(b"OLD\nNOTE").unwrap();
        assert_eq!(legacy.current_title(), b"OLD");
        assert_eq!(legacy.current_text(), b"OLD\nNOTE");

        let mut previous = NotesApp::new();
        previous.notes[0] = Note::from_parts(b"", b"FIRST\nLINE").unwrap();
        previous.count = 2;
        previous.notes[1] = Note::from_parts(b"", b"SECOND").unwrap();
        previous.selected = 1;
        let mut v1 = [0; 6 + MAX_NOTES * (1 + NOTE_TEXT_CAPACITY)];
        v1[..4].copy_from_slice(&V1_MAGIC);
        v1[4] = 2;
        v1[5] = 1;
        let mut cursor = 6;
        for note in previous.notes.iter().take(2) {
            v1[cursor] = note.length;
            cursor += 1;
            let body = note.text();
            v1[cursor..cursor + body.len()].copy_from_slice(body);
            cursor += body.len();
        }
        let migrated = NotesApp::from_v1_collection(&v1[..cursor]).unwrap();
        assert_eq!(migrated.current_title(), b"SECOND");
        assert_eq!(migrated.current_text(), b"SECOND");
    }

    #[test]
    fn titles_and_bodies_round_trip_independently() {
        let mut app = NotesApp::new();
        app.notes[0] = Note::from_parts(b"JOURNAL", b"FIRST\nBODY").unwrap();
        app.count = 2;
        app.notes[1] = Note::from_parts(b"SECOND", b"OTHER").unwrap();
        app.selected = 1;
        let mut encoded = [0; NOTES_BLOB_CAPACITY];
        let length = app.encode(&mut encoded).unwrap();
        assert_eq!(&encoded[..4], b"PNT2");
        assert_eq!(NotesApp::decode(&encoded[..length]), Some(app));
    }

    #[test]
    fn title_and_body_fields_edit_without_overwriting_each_other() {
        let mut app = NotesApp::new();
        let mut view = AppViewState::new();
        app.render_editor(&mut view);
        assert_eq!(app.apply_key(30, 1, &mut view), changed(true));
        assert_eq!(app.current_text(), b"A");

        assert_eq!(app.apply_key(KEY_F6, 1, &mut view), changed(false));
        assert!(view.notes_field != 0);
        assert_eq!(app.apply_key(37, 1, &mut view), changed(true)); // K
        assert_eq!(app.apply_key(30, 1, &mut view), changed(true)); // A
        assert_eq!(app.apply_key(3, 1, &mut view), changed(true)); // 2
        assert_eq!(app.current_title(), b"NOTE 1KA2");
        assert_eq!(app.current_text(), b"A");

        assert_eq!(app.apply_key(KEY_F7, 1, &mut view), changed(false));
        assert_eq!(&view.text[..view.length as usize], b"A");
        assert_eq!(app.apply_key(48, 1, &mut view), changed(true)); // B
        assert_eq!(app.current_text(), b"AB");
        assert_eq!(app.current_title(), b"NOTE 1KA2");
    }

    #[test]
    fn editor_lists_adds_selects_and_edits_independent_notes() {
        let mut app = NotesApp::new();
        let mut view = AppViewState::new();
        app.render_editor(&mut view);
        assert_eq!(app.apply_key(30, 1, &mut view), changed(true));
        assert_eq!(app.current_text(), b"A");

        assert_eq!(app.apply_key(KEY_F5, 1, &mut view), changed(false));
        assert!(app.is_list_mode());
        assert_eq!(&view.text[..view.length as usize], b"1 NOTE 1");
        assert_eq!(app.apply_key(KEY_INSERT, 1, &mut view), changed(true));
        assert_eq!(app.count(), 2);
        assert_eq!(app.selected(), 1);
        assert_eq!(app.current_text(), b"");
        assert!(!app.is_list_mode());

        assert_eq!(app.apply_key(48, 1, &mut view), changed(true)); // B
        assert_eq!(app.current_text(), b"B");
        assert_eq!(app.apply_key(KEY_F5, 1, &mut view), changed(false));
        assert_eq!(app.apply_key(KEY_F1, 1, &mut view), changed(true));
        assert_eq!(app.selected(), 0);
        assert_eq!(app.current_text(), b"A");
        assert_eq!(view.revision.load(Ordering::SeqCst) & 1, 0);
    }

    #[test]
    fn deletion_requires_confirmation_and_keeps_one_named_empty_note() {
        let mut app = NotesApp::new();
        let mut view = AppViewState::new();
        app.count = 2;
        app.notes[0] = Note::from_parts(b"FIRST", b"ONE").unwrap();
        app.notes[1] = Note::from_parts(b"SECOND", b"TWO").unwrap();
        app.selected = 1;
        app.screen = Screen::List;
        app.render_list(&mut view);

        assert_eq!(app.apply_key(KEY_DELETE, 1, &mut view), changed(false));
        assert!(view.notes_delete_armed != 0);
        assert_eq!(app.apply_key(KEY_DELETE, 1, &mut view), changed(true));
        assert_eq!(app.count(), 1);
        assert_eq!(app.current_text(), b"ONE");

        app.apply_key(KEY_DELETE, 1, &mut view);
        assert_eq!(app.apply_key(KEY_DELETE, 1, &mut view), changed(true));
        assert_eq!(app.count(), 1);
        assert_eq!(app.current_text(), b"");
        assert_eq!(app.current_title(), b"NOTE 1");
    }

    #[test]
    fn decoder_rejects_invalid_title_or_body_lengths_and_trailing_bytes() {
        assert!(NotesApp::decode(b"PNT2\0\0").is_none());
        assert!(NotesApp::decode(b"PNT2\x01\0\x01a\x00").is_none());
        assert!(NotesApp::from_legacy_text(&[b'A'; NOTE_TEXT_CAPACITY + 1]).is_none());
    }
}
