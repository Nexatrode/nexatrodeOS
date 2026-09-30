//! Settings display copy for the local-only device shell.

/// Shared with `AppViewState::set_display_text` alphabet rules.
pub const SETTINGS_TEXT_CAPACITY: usize = 128;

const PREFIX: &[u8] = b"LOCAL ONLY\nVAULT LOCAL\nNO NET YET\n";
const NO_ALERTS: &[u8] = b"NO ALERTS YET";

/// Format the four-line Settings body. Line 4 is `NO ALERTS YET` or
/// `ALERTS RING N SMS M` (uppercase digits only).
pub fn format_settings_text(rings: u64, sms: u64, out: &mut [u8; SETTINGS_TEXT_CAPACITY]) -> Option<usize> {
    out.fill(0);
    if PREFIX.len() > SETTINGS_TEXT_CAPACITY {
        return None;
    }
    out[..PREFIX.len()].copy_from_slice(PREFIX);
    let mut cursor = PREFIX.len();
    if rings == 0 && sms == 0 {
        if cursor + NO_ALERTS.len() > SETTINGS_TEXT_CAPACITY {
            return None;
        }
        out[cursor..cursor + NO_ALERTS.len()].copy_from_slice(NO_ALERTS);
        return Some(cursor + NO_ALERTS.len());
    }
    const HEAD: &[u8] = b"ALERTS RING ";
    const MID: &[u8] = b" SMS ";
    if cursor + HEAD.len() > SETTINGS_TEXT_CAPACITY {
        return None;
    }
    out[cursor..cursor + HEAD.len()].copy_from_slice(HEAD);
    cursor += HEAD.len();
    cursor = write_decimal(rings, out, cursor)?;
    if cursor + MID.len() > SETTINGS_TEXT_CAPACITY {
        return None;
    }
    out[cursor..cursor + MID.len()].copy_from_slice(MID);
    cursor += MID.len();
    cursor = write_decimal(sms, out, cursor)?;
    Some(cursor)
}

fn write_decimal(value: u64, out: &mut [u8; SETTINGS_TEXT_CAPACITY], cursor: usize) -> Option<usize> {
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
    if cursor + count > SETTINGS_TEXT_CAPACITY {
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
    fn zero_alerts_keeps_the_stub_line() {
        let mut buffer = [0u8; SETTINGS_TEXT_CAPACITY];
        let length = format_settings_text(0, 0, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"LOCAL ONLY\nVAULT LOCAL\nNO NET YET\nNO ALERTS YET");
    }

    #[test]
    fn non_zero_alerts_use_the_ring_sms_line() {
        let mut buffer = [0u8; SETTINGS_TEXT_CAPACITY];
        let length = format_settings_text(1, 1, &mut buffer).unwrap();
        assert_eq!(
            &buffer[..length],
            b"LOCAL ONLY\nVAULT LOCAL\nNO NET YET\nALERTS RING 1 SMS 1"
        );
        let length = format_settings_text(12, 0, &mut buffer).unwrap();
        assert_eq!(
            &buffer[..length],
            b"LOCAL ONLY\nVAULT LOCAL\nNO NET YET\nALERTS RING 12 SMS 0"
        );
    }

    #[test]
    fn formatted_text_fits_the_app_view_alphabet() {
        let mut buffer = [0u8; SETTINGS_TEXT_CAPACITY];
        for &(rings, sms) in &[(0, 0), (1, 1), (99, 7)] {
            let length = format_settings_text(rings, sms, &mut buffer).unwrap();
            assert!(length <= SETTINGS_TEXT_CAPACITY);
            assert!(
                buffer[..length]
                    .iter()
                    .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-'))
            );
        }
    }
}
