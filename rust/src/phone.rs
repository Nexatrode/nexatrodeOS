//! Phone display copy for the fake-modem alert shell.

/// Shared with `AppViewState::set_display_text` alphabet rules.
pub const PHONE_TEXT_CAPACITY: usize = 128;

/// Format the three-line Phone body: idle `IDLE\nRING 0\nSMS 0`, or
/// `ALERT\nRING N\nSMS M` when either count is non-zero.
pub fn format_phone_text(rings: u64, sms: u64, out: &mut [u8; PHONE_TEXT_CAPACITY]) -> Option<usize> {
    out.fill(0);
    let mut cursor = 0;
    let head = if rings == 0 && sms == 0 {
        b"IDLE\n".as_slice()
    } else {
        b"ALERT\n".as_slice()
    };
    if head.len() > PHONE_TEXT_CAPACITY {
        return None;
    }
    out[..head.len()].copy_from_slice(head);
    cursor += head.len();
    cursor = write_line(b"RING ", rings, out, cursor)?;
    cursor = write(out, cursor, b"\n")?;
    cursor = write_line(b"SMS ", sms, out, cursor)?;
    Some(cursor)
}

fn write(out: &mut [u8; PHONE_TEXT_CAPACITY], cursor: usize, bytes: &[u8]) -> Option<usize> {
    if cursor + bytes.len() > PHONE_TEXT_CAPACITY {
        return None;
    }
    out[cursor..cursor + bytes.len()].copy_from_slice(bytes);
    Some(cursor + bytes.len())
}

fn write_line(
    label: &[u8],
    value: u64,
    out: &mut [u8; PHONE_TEXT_CAPACITY],
    cursor: usize,
) -> Option<usize> {
    let cursor = write(out, cursor, label)?;
    write_decimal(value, out, cursor)
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
        let length = format_phone_text(0, 0, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"IDLE\nRING 0\nSMS 0");
        let length = format_phone_text(1, 1, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"ALERT\nRING 1\nSMS 1");
        let length = format_phone_text(12, 0, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"ALERT\nRING 12\nSMS 0");
    }

    #[test]
    fn formatted_text_fits_the_app_view_alphabet() {
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        for &(rings, sms) in &[(0, 0), (1, 1), (99, 7)] {
            let length = format_phone_text(rings, sms, &mut buffer).unwrap();
            assert!(length <= PHONE_TEXT_CAPACITY);
            assert!(
                buffer[..length]
                    .iter()
                    .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-'))
            );
        }
    }
}
