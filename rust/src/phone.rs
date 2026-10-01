//! Phone dial pad and fake-modem alert shell.

/// Shared with `AppViewState::set_display_text` alphabet rules.
pub const PHONE_TEXT_CAPACITY: usize = 128;
pub const DIGIT_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phone {
    rings: u64,
    sms: u64,
    digits: [u8; DIGIT_CAPACITY],
    digit_length: usize,
    calling: bool,
}

impl Phone {
    pub const fn new() -> Self {
        Self {
            rings: 0,
            sms: 0,
            digits: [0; DIGIT_CAPACITY],
            digit_length: 0,
            calling: false,
        }
    }

    pub fn set_alerts(&mut self, rings: u64, sms: u64) {
        self.rings = rings;
        self.sms = sms;
    }

    pub fn display<'a>(&'a self, buffer: &'a mut [u8; PHONE_TEXT_CAPACITY]) -> Option<&'a [u8]> {
        let length = format_phone_text(
            self.rings,
            self.sms,
            &self.digits[..self.digit_length],
            self.calling,
            buffer,
        )?;
        Some(&buffer[..length])
    }

    pub fn press_digit(&mut self, digit: u8) -> bool {
        if digit > 9 {
            return false;
        }
        if self.calling {
            self.digits = [0; DIGIT_CAPACITY];
            self.digit_length = 0;
            self.calling = false;
        }
        if self.digit_length >= DIGIT_CAPACITY {
            return false;
        }
        self.digits[self.digit_length] = b'0' + digit;
        self.digit_length += 1;
        true
    }

    pub fn press_backspace(&mut self) -> bool {
        if self.digit_length == 0 {
            if self.calling {
                self.calling = false;
                return true;
            }
            return false;
        }
        self.calling = false;
        self.digit_length -= 1;
        self.digits[self.digit_length] = 0;
        true
    }

    pub fn press_clear(&mut self) -> bool {
        if self.digit_length == 0 && !self.calling {
            return false;
        }
        self.digits = [0; DIGIT_CAPACITY];
        self.digit_length = 0;
        self.calling = false;
        true
    }

    pub fn can_call(&self) -> bool {
        self.digit_length > 0 && !self.calling
    }

    pub fn digits(&self) -> &[u8] {
        &self.digits[..self.digit_length]
    }

    pub fn press_call(&mut self) -> bool {
        if !self.can_call() {
            return false;
        }
        self.calling = true;
        true
    }
}

impl Default for Phone {
    fn default() -> Self {
        Self::new()
    }
}

/// Format Phone body. Empty buffer uses idle/alert lines; digits use
/// `DIAL` / `CALLING` plus a compact `RING N SMS M` status line.
pub fn format_phone_text(
    rings: u64,
    sms: u64,
    digits: &[u8],
    calling: bool,
    out: &mut [u8; PHONE_TEXT_CAPACITY],
) -> Option<usize> {
    out.fill(0);
    if digits.is_empty() && !calling {
        return format_idle_or_alert(rings, sms, out);
    }
    let mut cursor = 0;
    let head = if calling {
        b"CALLING\n".as_slice()
    } else {
        b"DIAL\n".as_slice()
    };
    cursor = write(out, cursor, head)?;
    if !digits.is_empty() {
        cursor = write(out, cursor, digits)?;
    }
    cursor = write(out, cursor, b"\n")?;
    cursor = write(out, cursor, b"RING ")?;
    cursor = write_decimal(rings, out, cursor)?;
    cursor = write(out, cursor, b" SMS ")?;
    write_decimal(sms, out, cursor)
}

fn format_idle_or_alert(
    rings: u64,
    sms: u64,
    out: &mut [u8; PHONE_TEXT_CAPACITY],
) -> Option<usize> {
    let mut cursor = 0;
    let head = if rings == 0 && sms == 0 {
        b"IDLE\n".as_slice()
    } else {
        b"ALERT\n".as_slice()
    };
    cursor = write(out, cursor, head)?;
    cursor = write(out, cursor, b"RING ")?;
    cursor = write_decimal(rings, out, cursor)?;
    cursor = write(out, cursor, b"\n")?;
    cursor = write(out, cursor, b"SMS ")?;
    write_decimal(sms, out, cursor)
}

fn write(out: &mut [u8; PHONE_TEXT_CAPACITY], cursor: usize, bytes: &[u8]) -> Option<usize> {
    if cursor + bytes.len() > PHONE_TEXT_CAPACITY {
        return None;
    }
    out[cursor..cursor + bytes.len()].copy_from_slice(bytes);
    Some(cursor + bytes.len())
}

fn write_decimal(value: u64, out: &mut [u8; PHONE_TEXT_CAPACITY], cursor: usize) -> Option<usize> {
    let mut digits = [0u8; 20];
    let mut remaining = value;
    let mut count = 0;
    loop {
        digits[count] = b'0' + (remaining % 10) as u8;
        count += 1;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    if cursor + count > PHONE_TEXT_CAPACITY {
        return None;
    }
    for index in 0..count {
        out[cursor + index] = digits[count - 1 - index];
    }
    Some(cursor + count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_and_alert_lines() {
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        let length = format_phone_text(0, 0, b"", false, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"IDLE\nRING 0\nSMS 0");
        let length = format_phone_text(1, 1, b"", false, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"ALERT\nRING 1\nSMS 1");
    }

    #[test]
    fn dial_and_calling_keep_alert_counts() {
        let mut phone = Phone::new();
        phone.set_alerts(1, 1);
        assert!(phone.press_digit(5));
        assert!(phone.press_digit(5));
        assert!(phone.press_digit(5));
        assert!(phone.press_digit(1));
        assert!(phone.press_digit(2));
        assert!(phone.press_digit(3));
        assert!(phone.press_digit(4));
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        assert_eq!(
            phone.display(&mut buffer).unwrap(),
            b"DIAL\n5551234\nRING 1 SMS 1"
        );
        assert!(phone.press_call());
        assert_eq!(
            phone.display(&mut buffer).unwrap(),
            b"CALLING\n5551234\nRING 1 SMS 1"
        );
        assert!(!phone.press_call());
        assert!(phone.press_clear());
        assert_eq!(phone.display(&mut buffer).unwrap(), b"ALERT\nRING 1\nSMS 1");
    }

    #[test]
    fn digit_capacity_and_backspace() {
        let mut phone = Phone::new();
        for _ in 0..DIGIT_CAPACITY {
            assert!(phone.press_digit(1));
        }
        assert!(!phone.press_digit(2));
        assert!(phone.press_backspace());
        assert!(phone.press_digit(9));
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        let text = phone.display(&mut buffer).unwrap();
        assert!(text.starts_with(b"DIAL\n"));
        assert_eq!(text[5 + DIGIT_CAPACITY - 1], b'9');
    }

    #[test]
    fn digit_after_calling_starts_a_fresh_buffer() {
        let mut phone = Phone::new();
        assert!(phone.press_digit(5));
        assert!(phone.press_call());
        assert!(phone.press_digit(9));
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        assert_eq!(phone.display(&mut buffer).unwrap(), b"DIAL\n9\nRING 0 SMS 0");
    }

    #[test]
    fn formatted_text_fits_the_app_view_alphabet() {
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        for &(rings, sms, digits, calling) in &[
            (0u64, 0u64, b"".as_slice(), false),
            (1, 1, b"", false),
            (1, 1, b"5551234".as_slice(), false),
            (12, 0, b"9".as_slice(), true),
        ] {
            let length = format_phone_text(rings, sms, digits, calling, &mut buffer).unwrap();
            assert!(length <= PHONE_TEXT_CAPACITY);
            assert!(
                buffer[..length]
                    .iter()
                    .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-'))
            );
        }
    }
}
