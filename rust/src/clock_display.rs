//! Small, allocation-free formatting helpers for the trusted RTC display.

pub const UTC_TIME_TEXT_LENGTH: usize = 8;

/// Format seconds since the Unix epoch as a 24-hour UTC clock face (`HH MM SS`).
pub fn format_utc_hms(unix_seconds: u32, output: &mut [u8; UTC_TIME_TEXT_LENGTH]) {
    let day_seconds = unix_seconds % 86_400;
    let hours = day_seconds / 3_600;
    let minutes = (day_seconds / 60) % 60;
    let seconds = day_seconds % 60;
    output[0] = b'0' + (hours / 10) as u8;
    output[1] = b'0' + (hours % 10) as u8;
    output[2] = b' ';
    output[3] = b'0' + (minutes / 10) as u8;
    output[4] = b'0' + (minutes % 10) as u8;
    output[5] = b' ';
    output[6] = b'0' + (seconds / 10) as u8;
    output[7] = b'0' + (seconds % 10) as u8;
}

#[cfg(test)]
mod tests {
    use super::{UTC_TIME_TEXT_LENGTH, format_utc_hms};

    fn formatted(seconds: u32) -> [u8; UTC_TIME_TEXT_LENGTH] {
        let mut output = [0; UTC_TIME_TEXT_LENGTH];
        format_utc_hms(seconds, &mut output);
        output
    }

    #[test]
    fn formats_utc_time_and_rolls_over_at_midnight() {
        assert_eq!(&formatted(0), b"00 00 00");
        assert_eq!(&formatted(45_296), b"12 34 56");
        assert_eq!(&formatted(86_399), b"23 59 59");
        assert_eq!(&formatted(86_400), b"00 00 00");
        assert_eq!(&formatted(86_401), b"00 00 01");
    }
}
