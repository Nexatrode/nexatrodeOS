//! Conversion of the clock service's UTC snapshot for display.

pub fn display_time(snapshot: &[u8], transitions: &[u8]) -> Option<[u8; 5]> {
    if snapshot.len() != 19 || snapshot[4] != b' ' || snapshot[7] != b' '
        || snapshot[10] != b'\n' || snapshot[13] != b' ' || snapshot[16] != b' '
        || transitions.len() < 4 || (transitions.len() - 4) % 8 != 0 {
        return None;
    }
    fn number(bytes: &[u8]) -> Option<i64> {
        bytes.iter().try_fold(0i64, |n, b| {
            if b.is_ascii_digit() { Some(n * 10 + i64::from(b - b'0')) } else { None }
        })
    }
    let year = number(&snapshot[..4])?;
    let month = number(&snapshot[5..7])?;
    let day = number(&snapshot[8..10])?;
    let hour = number(&snapshot[11..13])?;
    let minute = number(&snapshot[14..16])?;
    let second = number(&snapshot[17..19])?;
    if !(1970..=2106).contains(&year) || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 { return None; }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if day < 1 || day > month_days[(month - 1) as usize] { return None; }
    // Gregorian date to Unix days (Howard Hinnant's civil calendar algorithm).
    let y = year - i64::from(month <= 2);
    let era = y / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let epoch = days * 86_400 + hour * 3_600 + minute * 60 + second;
    if !(0..=u32::MAX as i64).contains(&epoch) { return None; }
    let mut offset = i32::from_le_bytes(transitions[..4].try_into().ok()?);
    for entry in transitions[4..].chunks_exact(8) {
        let start = u32::from_le_bytes(entry[..4].try_into().ok()?);
        if i64::from(start) > epoch { break; }
        offset = i32::from_le_bytes(entry[4..].try_into().ok()?);
    }
    let local = (epoch + i64::from(offset)).rem_euclid(86_400);
    let h = local / 3_600;
    let m = local / 60 % 60;
    Some([b'0' + (h / 10) as u8, b'0' + (h % 10) as u8, b':', b'0' + (m / 10) as u8, b'0' + (m % 10) as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(base: i32, transitions: &[(u32, i32)]) -> std::vec::Vec<u8> {
        let mut bytes = base.to_le_bytes().to_vec();
        for (time, offset) in transitions {
            bytes.extend_from_slice(&time.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn displays_local_time_and_crosses_midnight() {
        let zone = table(-8 * 3600, &[]);
        assert_eq!(display_time(b"2026 11 22\n06 14 40", &zone), Some(*b"22:14"));
    }

    #[test]
    fn selects_offset_at_daylight_saving_transition() {
        // 2026-03-08 10:00 UTC, the spring transition in Los Angeles.
        let zone = table(-8 * 3600, &[(1_772_964_000, -7 * 3600)]);
        assert_eq!(display_time(b"2026 03 08\n09 59 59", &zone), Some(*b"01:59"));
        assert_eq!(display_time(b"2026 03 08\n10 00 00", &zone), Some(*b"03:00"));
    }

    #[test]
    fn rejects_missing_or_malformed_clock_data() {
        assert_eq!(display_time(b"", &table(0, &[])), None);
        assert_eq!(display_time(b"2026 99 01\n10 00 00", &table(0, &[])), None);
        assert_eq!(display_time(b"2026 01 01\n10 00 00", b""), None);
    }
}
