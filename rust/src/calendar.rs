//! Bounded reminder list for the Calendar workspace.

pub const REMINDER_CAPACITY: usize = 3;
pub const REMINDER_TITLE_CAPACITY: usize = 21;
pub const DAY_CAPACITY: usize = 4;
pub const TIME_CAPACITY: usize = 4;
pub const CALENDAR_BLOB_CAPACITY: usize = 6
    + REMINDER_CAPACITY * (1 + REMINDER_TITLE_CAPACITY + 1 + DAY_CAPACITY + 1 + TIME_CAPACITY);
const MAGIC_V1: [u8; 4] = *b"PCAL";
const MAGIC_V2: [u8; 4] = *b"PCL2";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalendarUpdate {
    pub changed: bool,
    pub persist: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditPhase {
    Browse,
    Title,
    Day,
    Time,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Calendar {
    titles: [[u8; REMINDER_TITLE_CAPACITY]; REMINDER_CAPACITY],
    lengths: [usize; REMINDER_CAPACITY],
    days: [[u8; DAY_CAPACITY]; REMINDER_CAPACITY],
    day_lengths: [usize; REMINDER_CAPACITY],
    times: [[u8; TIME_CAPACITY]; REMINDER_CAPACITY],
    time_lengths: [usize; REMINDER_CAPACITY],
    count: usize,
    selected: usize,
    phase: EditPhase,
}

impl Calendar {
    pub const fn new() -> Self {
        Self {
            titles: [[0; REMINDER_TITLE_CAPACITY]; REMINDER_CAPACITY],
            lengths: [0; REMINDER_CAPACITY],
            days: [[0; DAY_CAPACITY]; REMINDER_CAPACITY],
            day_lengths: [0; REMINDER_CAPACITY],
            times: [[0; TIME_CAPACITY]; REMINDER_CAPACITY],
            time_lengths: [0; REMINDER_CAPACITY],
            count: 0,
            selected: 0,
            phase: EditPhase::Browse,
        }
    }

    pub fn display<'a>(&'a self, buffer: &'a mut [u8; 128]) -> &'a [u8] {
        buffer.fill(0);
        match self.phase {
            EditPhase::Browse if self.count == 0 => {
                let empty = b"NO REMINDERS\nENTER TO ADD";
                buffer[..empty.len()].copy_from_slice(empty);
                &buffer[..empty.len()]
            }
            EditPhase::Browse => {
                let mut offset = 0;
                for index in 0..self.count {
                    if index > 0 {
                        offset = write(buffer, offset, b"\n");
                    }
                    let marker = if index == self.selected {
                        b"- "
                    } else {
                        b"  "
                    };
                    offset = write(buffer, offset, marker);
                    offset = write(buffer, offset, &self.titles[index][..self.lengths[index]]);
                    if self.day_lengths[index] > 0 || self.time_lengths[index] > 0 {
                        offset = write(buffer, offset, b"\n  ");
                        if self.day_lengths[index] == DAY_CAPACITY {
                            offset = write(buffer, offset, &self.days[index]);
                        } else {
                            offset = write(buffer, offset, b"----");
                        }
                        offset = write(buffer, offset, b" ");
                        if self.time_lengths[index] == TIME_CAPACITY {
                            offset = write(buffer, offset, &self.times[index]);
                        } else {
                            offset = write(buffer, offset, b"----");
                        }
                    }
                }
                &buffer[..offset]
            }
            EditPhase::Title => {
                let mut offset = write(buffer, 0, b"ADD REMINDER\n");
                offset = write(
                    buffer,
                    offset,
                    &self.titles[self.selected][..self.lengths[self.selected]],
                );
                &buffer[..offset]
            }
            EditPhase::Day => {
                let mut offset = write(buffer, 0, b"ADD DAY\n");
                offset = write(
                    buffer,
                    offset,
                    &self.titles[self.selected][..self.lengths[self.selected]],
                );
                offset = write(buffer, offset, b"\n");
                offset = write(
                    buffer,
                    offset,
                    &self.days[self.selected][..self.day_lengths[self.selected]],
                );
                &buffer[..offset]
            }
            EditPhase::Time => {
                let mut offset = write(buffer, 0, b"ADD TIME\n");
                offset = write(
                    buffer,
                    offset,
                    &self.titles[self.selected][..self.lengths[self.selected]],
                );
                offset = write(buffer, offset, b"\n");
                if self.day_lengths[self.selected] == DAY_CAPACITY {
                    offset = write(buffer, offset, &self.days[self.selected]);
                } else {
                    offset = write(buffer, offset, b"----");
                }
                offset = write(buffer, offset, b"\n");
                offset = write(
                    buffer,
                    offset,
                    &self.times[self.selected][..self.time_lengths[self.selected]],
                );
                &buffer[..offset]
            }
        }
    }

    pub fn decode(payload: &[u8]) -> Option<Self> {
        if payload.len() < 6 {
            return None;
        }
        let with_when = match payload[..4].try_into().ok()? {
            MAGIC_V2 => true,
            MAGIC_V1 => false,
            _ => return None,
        };
        let count = payload[4] as usize;
        let selected = payload[5] as usize;
        if count > REMINDER_CAPACITY
            || (count > 0 && selected >= count)
            || (count == 0 && selected != 0)
        {
            return None;
        }
        let mut calendar = Self::new();
        calendar.count = count;
        calendar.selected = selected;
        let mut cursor = 6;
        for index in 0..count {
            if cursor >= payload.len() {
                return None;
            }
            let length = payload[cursor] as usize;
            cursor += 1;
            if length == 0
                || length > REMINDER_TITLE_CAPACITY
                || cursor + length > payload.len()
            {
                return None;
            }
            let title = &payload[cursor..cursor + length];
            if title
                .iter()
                .any(|byte| !matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' '))
            {
                return None;
            }
            calendar.titles[index][..length].copy_from_slice(title);
            calendar.lengths[index] = length;
            cursor += length;

            if with_when {
                let day_length = *payload.get(cursor)? as usize;
                cursor += 1;
                if day_length != 0 && day_length != DAY_CAPACITY {
                    return None;
                }
                if day_length > 0 {
                    let day_end = cursor.checked_add(day_length)?;
                    let day = payload.get(cursor..day_end)?;
                    if !valid_day(day) {
                        return None;
                    }
                    calendar.days[index].copy_from_slice(day);
                    calendar.day_lengths[index] = day_length;
                    cursor = day_end;
                }
                let time_length = *payload.get(cursor)? as usize;
                cursor += 1;
                if time_length != 0 && time_length != TIME_CAPACITY {
                    return None;
                }
                if time_length > 0 {
                    let time_end = cursor.checked_add(time_length)?;
                    let time = payload.get(cursor..time_end)?;
                    if !valid_time(time) {
                        return None;
                    }
                    calendar.times[index].copy_from_slice(time);
                    calendar.time_lengths[index] = time_length;
                    cursor = time_end;
                }
            }
        }
        if cursor != payload.len() {
            return None;
        }
        Some(calendar)
    }

    pub fn encode(&self, output: &mut [u8; CALENDAR_BLOB_CAPACITY]) -> Option<usize> {
        if self.phase != EditPhase::Browse || self.count > REMINDER_CAPACITY {
            return None;
        }
        if self.count > 0 && self.selected >= self.count {
            return None;
        }
        output.fill(0);
        output[..4].copy_from_slice(&MAGIC_V2);
        output[4] = self.count as u8;
        output[5] = self.selected as u8;
        let mut cursor = 6;
        for index in 0..self.count {
            let length = self.lengths[index];
            let title = &self.titles[index][..length];
            if length == 0
                || length > REMINDER_TITLE_CAPACITY
                || title
                    .iter()
                    .any(|byte| !matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' '))
            {
                return None;
            }
            let day_length = self.day_lengths[index];
            let time_length = self.time_lengths[index];
            if day_length != 0 && (day_length != DAY_CAPACITY || !valid_day(&self.days[index])) {
                return None;
            }
            if time_length != 0
                && (time_length != TIME_CAPACITY || !valid_time(&self.times[index]))
            {
                return None;
            }
            output[cursor] = length as u8;
            cursor += 1;
            output[cursor..cursor + length].copy_from_slice(title);
            cursor += length;
            output[cursor] = day_length as u8;
            cursor += 1;
            if day_length > 0 {
                output[cursor..cursor + day_length].copy_from_slice(&self.days[index]);
                cursor += day_length;
            }
            output[cursor] = time_length as u8;
            cursor += 1;
            if time_length > 0 {
                output[cursor..cursor + time_length].copy_from_slice(&self.times[index]);
                cursor += time_length;
            }
        }
        Some(cursor)
    }

    pub fn apply_key(&mut self, code: u16, value: u32) -> CalendarUpdate {
        if value != 1 && value != 2 {
            return unchanged();
        }
        match code {
            KEY_ENTER => self.press_enter(),
            KEY_UP => bool_update(self.press_up(), false),
            KEY_DOWN => bool_update(self.press_down(), false),
            KEY_DELETE => bool_update(self.press_delete(), true),
            KEY_BACKSPACE => bool_update(self.press_backspace(), false),
            KEY_SPACE => bool_update(self.press_char(b' '), false),
            _ => {
                if let Some(letter) = evdev_letter(code) {
                    bool_update(self.press_char(letter), false)
                } else if let Some(digit) = evdev_digit(code) {
                    bool_update(self.press_char(digit), false)
                } else {
                    unchanged()
                }
            }
        }
    }

    fn press_enter(&mut self) -> CalendarUpdate {
        match self.phase {
            EditPhase::Browse => {
                if self.count < REMINDER_CAPACITY {
                    self.selected = self.count;
                    self.clear_slot(self.selected);
                    self.phase = EditPhase::Title;
                    return changed(false);
                }
                unchanged()
            }
            EditPhase::Title => {
                if self.lengths[self.selected] == 0 {
                    self.phase = EditPhase::Browse;
                    if self.selected == self.count && self.count > 0 {
                        self.selected = self.count - 1;
                    }
                    return changed(false);
                }
                self.phase = EditPhase::Day;
                changed(false)
            }
            EditPhase::Day => {
                let day_length = self.day_lengths[self.selected];
                if day_length != 0 && day_length != DAY_CAPACITY {
                    return unchanged();
                }
                if day_length == DAY_CAPACITY && !valid_day(&self.days[self.selected]) {
                    return unchanged();
                }
                self.phase = EditPhase::Time;
                changed(false)
            }
            EditPhase::Time => {
                let time_length = self.time_lengths[self.selected];
                if time_length != 0 && time_length != TIME_CAPACITY {
                    return unchanged();
                }
                if time_length == TIME_CAPACITY && !valid_time(&self.times[self.selected]) {
                    return unchanged();
                }
                if self.selected == self.count {
                    self.count += 1;
                }
                self.phase = EditPhase::Browse;
                changed(true)
            }
        }
    }

    fn press_up(&mut self) -> bool {
        if self.phase != EditPhase::Browse || self.count == 0 || self.selected == 0 {
            return false;
        }
        self.selected -= 1;
        true
    }

    fn press_down(&mut self) -> bool {
        if self.phase != EditPhase::Browse || self.count < 2 || self.selected + 1 >= self.count {
            return false;
        }
        self.selected += 1;
        true
    }

    fn press_delete(&mut self) -> bool {
        if self.phase != EditPhase::Browse || self.count == 0 {
            return false;
        }
        for index in self.selected..self.count - 1 {
            self.titles[index] = self.titles[index + 1];
            self.lengths[index] = self.lengths[index + 1];
            self.days[index] = self.days[index + 1];
            self.day_lengths[index] = self.day_lengths[index + 1];
            self.times[index] = self.times[index + 1];
            self.time_lengths[index] = self.time_lengths[index + 1];
        }
        let last = self.count - 1;
        self.clear_slot(last);
        self.count -= 1;
        if self.count == 0 {
            self.selected = 0;
        } else if self.selected >= self.count {
            self.selected = self.count - 1;
        }
        true
    }

    fn press_backspace(&mut self) -> bool {
        match self.phase {
            EditPhase::Browse => false,
            EditPhase::Title => {
                if self.lengths[self.selected] == 0 {
                    self.phase = EditPhase::Browse;
                    if self.selected == self.count && self.count > 0 {
                        self.selected = self.count - 1;
                    }
                    return true;
                }
                let length = self.lengths[self.selected] - 1;
                self.titles[self.selected][length] = 0;
                self.lengths[self.selected] = length;
                true
            }
            EditPhase::Day => {
                if self.day_lengths[self.selected] == 0 {
                    self.phase = EditPhase::Title;
                    return true;
                }
                let length = self.day_lengths[self.selected] - 1;
                self.days[self.selected][length] = 0;
                self.day_lengths[self.selected] = length;
                true
            }
            EditPhase::Time => {
                if self.time_lengths[self.selected] == 0 {
                    self.phase = EditPhase::Day;
                    return true;
                }
                let length = self.time_lengths[self.selected] - 1;
                self.times[self.selected][length] = 0;
                self.time_lengths[self.selected] = length;
                true
            }
        }
    }

    fn press_char(&mut self, character: u8) -> bool {
        match self.phase {
            EditPhase::Browse => false,
            EditPhase::Title => {
                if !matches!(character, b'A'..=b'Z' | b'0'..=b'9' | b' ') {
                    return false;
                }
                let length = self.lengths[self.selected];
                if length >= REMINDER_TITLE_CAPACITY {
                    return false;
                }
                self.titles[self.selected][length] = character;
                self.lengths[self.selected] = length + 1;
                true
            }
            EditPhase::Day => {
                if !character.is_ascii_digit() {
                    return false;
                }
                let length = self.day_lengths[self.selected];
                if length >= DAY_CAPACITY {
                    return false;
                }
                self.days[self.selected][length] = character;
                self.day_lengths[self.selected] = length + 1;
                true
            }
            EditPhase::Time => {
                if !character.is_ascii_digit() {
                    return false;
                }
                let length = self.time_lengths[self.selected];
                if length >= TIME_CAPACITY {
                    return false;
                }
                self.times[self.selected][length] = character;
                self.time_lengths[self.selected] = length + 1;
                true
            }
        }
    }

    fn clear_slot(&mut self, index: usize) {
        self.titles[index] = [0; REMINDER_TITLE_CAPACITY];
        self.lengths[index] = 0;
        self.days[index] = [0; DAY_CAPACITY];
        self.day_lengths[index] = 0;
        self.times[index] = [0; TIME_CAPACITY];
        self.time_lengths[index] = 0;
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self::new()
    }
}

const KEY_BACKSPACE: u16 = 14;
const KEY_SPACE: u16 = 57;
const KEY_ENTER: u16 = 28;
const KEY_UP: u16 = 103;
const KEY_DOWN: u16 = 108;
const KEY_DELETE: u16 = 111;

fn write(buffer: &mut [u8], offset: usize, bytes: &[u8]) -> usize {
    let end = (offset + bytes.len()).min(buffer.len());
    let copy = end - offset;
    buffer[offset..end].copy_from_slice(&bytes[..copy]);
    end
}

const fn unchanged() -> CalendarUpdate {
    CalendarUpdate {
        changed: false,
        persist: false,
    }
}

const fn changed(persist: bool) -> CalendarUpdate {
    CalendarUpdate {
        changed: true,
        persist,
    }
}

const fn bool_update(changed_flag: bool, persist: bool) -> CalendarUpdate {
    CalendarUpdate {
        changed: changed_flag,
        persist: changed_flag && persist,
    }
}

fn two_digits(bytes: &[u8]) -> Option<u8> {
    if bytes.len() != 2 {
        return None;
    }
    let tens = bytes[0].checked_sub(b'0')?;
    let ones = bytes[1].checked_sub(b'0')?;
    if tens > 9 || ones > 9 {
        return None;
    }
    Some(tens * 10 + ones)
}

fn valid_day(day: &[u8]) -> bool {
    if day.len() != DAY_CAPACITY || day.iter().any(|byte| !byte.is_ascii_digit()) {
        return false;
    }
    let Some(month) = two_digits(&day[..2]) else {
        return false;
    };
    let Some(dom) = two_digits(&day[2..]) else {
        return false;
    };
    (1..=12).contains(&month) && (1..=31).contains(&dom)
}

fn valid_time(time: &[u8]) -> bool {
    if time.len() != TIME_CAPACITY || time.iter().any(|byte| !byte.is_ascii_digit()) {
        return false;
    }
    let Some(hour) = two_digits(&time[..2]) else {
        return false;
    };
    let Some(minute) = two_digits(&time[2..]) else {
        return false;
    };
    hour <= 23 && minute <= 59
}

const fn evdev_letter(code: u16) -> Option<u8> {
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

const fn evdev_digit(code: u16) -> Option<u8> {
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

    fn finish_empty_when(calendar: &mut Calendar) {
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), changed(false)); // title -> day
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), changed(false)); // day -> time
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), changed(true)); // commit
    }

    #[test]
    fn adds_selects_deletes_and_round_trips_blob() {
        let mut calendar = Calendar::new();
        let mut buffer = [0u8; 128];
        assert_eq!(calendar.display(&mut buffer), b"NO REMINDERS\nENTER TO ADD");
        assert!(calendar.apply_key(KEY_ENTER, 1).changed);
        for code in [50u16, 18, 18, 20] {
            assert!(calendar.apply_key(code, 1).changed);
        }
        finish_empty_when(&mut calendar);
        assert_eq!(calendar.display(&mut buffer), b"- MEET");
        assert!(calendar.apply_key(KEY_ENTER, 1).changed);
        for code in [46u16, 30, 38] {
            assert!(calendar.apply_key(code, 1).changed);
        }
        finish_empty_when(&mut calendar);
        assert_eq!(calendar.display(&mut buffer), b"  MEET\n- CAL");
        assert!(calendar.apply_key(KEY_UP, 1).changed);
        assert_eq!(calendar.display(&mut buffer), b"- MEET\n  CAL");
        assert_eq!(calendar.apply_key(KEY_DELETE, 1), changed(true));
        assert_eq!(calendar.display(&mut buffer), b"- CAL");

        let mut encoded = [0; CALENDAR_BLOB_CAPACITY];
        let length = calendar.encode(&mut encoded).unwrap();
        assert_eq!(&encoded[..4], &MAGIC_V2);
        let decoded = Calendar::decode(&encoded[..length]).unwrap();
        assert_eq!(decoded.display(&mut buffer), b"- CAL");
    }

    #[test]
    fn day_and_time_round_trip_and_render() {
        let mut calendar = Calendar::new();
        let mut buffer = [0u8; 128];
        assert!(calendar.apply_key(KEY_ENTER, 1).changed);
        for code in [50u16, 18, 18, 20] {
            assert!(calendar.apply_key(code, 1).changed);
        }
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), changed(false)); // day
        for code in [11u16, 2, 11, 2] {
            // 0101
            assert!(calendar.apply_key(code, 1).changed);
        }
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), changed(false)); // time
        for code in [2u16, 3, 11, 11] {
            // 1200
            assert!(calendar.apply_key(code, 1).changed);
        }
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), changed(true));
        assert_eq!(calendar.display(&mut buffer), b"- MEET\n  0101 1200");

        let mut encoded = [0; CALENDAR_BLOB_CAPACITY];
        let length = calendar.encode(&mut encoded).unwrap();
        let decoded = Calendar::decode(&encoded[..length]).unwrap();
        assert_eq!(decoded.display(&mut buffer), b"- MEET\n  0101 1200");
    }

    #[test]
    fn decodes_pcal_blobs_with_empty_when() {
        let payload = b"PCAL\x01\x00\x04MEET";
        let decoded = Calendar::decode(payload).unwrap();
        let mut buffer = [0u8; 128];
        assert_eq!(decoded.display(&mut buffer), b"- MEET");
        let mut encoded = [0; CALENDAR_BLOB_CAPACITY];
        let length = decoded.encode(&mut encoded).unwrap();
        assert_eq!(&encoded[..4], &MAGIC_V2);
        assert_eq!(encoded[6 + 1 + 4], 0); // day_len
        assert_eq!(encoded[6 + 1 + 4 + 1], 0); // time_len
        let _ = length;
    }

    #[test]
    fn rejects_incomplete_or_invalid_day_and_time_on_enter() {
        let mut calendar = Calendar::new();
        calendar.apply_key(KEY_ENTER, 1);
        calendar.apply_key(50, 1); // M
        calendar.apply_key(KEY_ENTER, 1); // day
        calendar.apply_key(2, 1); // 1
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), unchanged()); // incomplete
        for code in [2u16, 3, 2] {
            // complete invalid-ish path to 1131 then fix: actually build 1301 invalid month
            let _ = code;
        }
        // Clear and type 1301 (invalid month)
        calendar.apply_key(KEY_BACKSPACE, 1);
        for code in [2u16, 4, 11, 2] {
            // 1301
            calendar.apply_key(code, 1);
        }
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), unchanged());
        // Fix to 0101
        for _ in 0..4 {
            calendar.apply_key(KEY_BACKSPACE, 1);
        }
        for code in [11u16, 2, 11, 2] {
            calendar.apply_key(code, 1);
        }
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), changed(false));
        calendar.apply_key(3, 1); // 2
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), unchanged()); // incomplete time
        for code in [5u16, 11, 11] {
            // 2400 invalid hour from 2...
            calendar.apply_key(code, 1);
        }
        // Now 2400
        assert_eq!(calendar.apply_key(KEY_ENTER, 1), unchanged());
    }

    #[test]
    fn decoder_rejects_trailing_bytes_and_bad_titles() {
        let mut calendar = Calendar::new();
        calendar.apply_key(KEY_ENTER, 1);
        calendar.apply_key(50, 1);
        finish_empty_when(&mut calendar);
        let mut encoded = [0; CALENDAR_BLOB_CAPACITY];
        let length = calendar.encode(&mut encoded).unwrap();
        assert!(Calendar::decode(&encoded[..length]).is_some());
        assert!(Calendar::decode(&encoded[..length + 1]).is_none());
        let mut bad = encoded;
        bad[7] = b'm';
        assert!(Calendar::decode(&bad[..length]).is_none());
    }
}
