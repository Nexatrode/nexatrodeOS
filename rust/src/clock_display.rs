//! Small, allocation-free formatting helpers for the trusted RTC display.

pub const UTC_DATE_TEXT_LENGTH: usize = 10;
pub const UTC_TIME_TEXT_LENGTH: usize = 8;
pub const UTC_CLOCK_TEXT_LENGTH: usize = UTC_DATE_TEXT_LENGTH + 1 + UTC_TIME_TEXT_LENGTH;

/// Format seconds since the Unix epoch as UTC date and 24-hour time.
///
/// Layout: `YYYY MM DD\nHH MM SS` (17 bytes). The graphics client renders the
/// date and time on separate rows; the newline is a stable field separator.
pub fn format_utc_clock(unix_seconds: u32, output: &mut [u8; UTC_CLOCK_TEXT_LENGTH]) {
    let day_seconds = unix_seconds % 86_400;
    let (year, month, day) = civil_from_unix_days(unix_seconds / 86_400);
    write_u32_digits(year, &mut output[0..4]);
    output[4] = b' ';
    write_u32_digits(month, &mut output[5..7]);
    output[7] = b' ';
    write_u32_digits(day, &mut output[8..10]);
    output[10] = b'\n';
    let hours = day_seconds / 3_600;
    let minutes = (day_seconds / 60) % 60;
    let seconds = day_seconds % 60;
    write_u32_digits(hours, &mut output[11..13]);
    output[13] = b' ';
    write_u32_digits(minutes, &mut output[14..16]);
    output[16] = b' ';
    write_u32_digits(seconds, &mut output[17..19]);
}

/// Format seconds since the Unix epoch as a 24-hour UTC clock face (`HH MM SS`).
pub fn format_utc_hms(unix_seconds: u32, output: &mut [u8; UTC_TIME_TEXT_LENGTH]) {
    let day_seconds = unix_seconds % 86_400;
    let hours = day_seconds / 3_600;
    let minutes = (day_seconds / 60) % 60;
    let seconds = day_seconds % 60;
    write_u32_digits(hours, &mut output[0..2]);
    output[2] = b' ';
    write_u32_digits(minutes, &mut output[3..5]);
    output[5] = b' ';
    write_u32_digits(seconds, &mut output[6..8]);
}

fn write_u32_digits(value: u32, digits: &mut [u8]) {
    let mut remaining = value;
    for slot in digits.iter_mut().rev() {
        *slot = b'0' + (remaining % 10) as u8;
        remaining /= 10;
    }
}

/// Convert a proleptic Gregorian day count since 1970-01-01 into UTC Y/M/D.
///
/// Uses Howard Hinnant's public-domain civil-from-days algorithm.
fn civil_from_unix_days(unix_days: u32) -> (u32, u32, u32) {
    let z = unix_days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m, d)
}

#[cfg(test)]
mod tests {
    use super::{
        UTC_CLOCK_TEXT_LENGTH, UTC_TIME_TEXT_LENGTH, format_utc_clock, format_utc_hms,
    };

    fn formatted_time(seconds: u32) -> [u8; UTC_TIME_TEXT_LENGTH] {
        let mut output = [0; UTC_TIME_TEXT_LENGTH];
        format_utc_hms(seconds, &mut output);
        output
    }

    fn formatted_clock(seconds: u32) -> [u8; UTC_CLOCK_TEXT_LENGTH] {
        let mut output = [0; UTC_CLOCK_TEXT_LENGTH];
        format_utc_clock(seconds, &mut output);
        output
    }

    #[test]
    fn formats_utc_time_and_rolls_over_at_midnight() {
        assert_eq!(&formatted_time(0), b"00 00 00");
        assert_eq!(&formatted_time(45_296), b"12 34 56");
        assert_eq!(&formatted_time(86_399), b"23 59 59");
        assert_eq!(&formatted_time(86_400), b"00 00 00");
        assert_eq!(&formatted_time(86_401), b"00 00 01");
    }

    #[test]
    fn formats_utc_date_and_time() {
        assert_eq!(&formatted_clock(0), b"1970 01 01\n00 00 00");
        assert_eq!(&formatted_clock(45_296), b"1970 01 01\n12 34 56");
        assert_eq!(&formatted_clock(86_399), b"1970 01 01\n23 59 59");
        assert_eq!(&formatted_clock(86_400), b"1970 01 02\n00 00 00");
        // 2000-03-01 00:00:00 UTC
        assert_eq!(&formatted_clock(951_868_800), b"2000 03 01\n00 00 00");
        // 2026-09-28 00:00:00 UTC
        assert_eq!(&formatted_clock(1_790_553_600), b"2026 09 28\n00 00 00");
    }
}
