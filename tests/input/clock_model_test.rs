#![allow(dead_code)]

#[path = "../../rust/src/clock_display.rs"]
mod clock_display;

#[cfg(test)]
mod host_tests {
    use super::clock_display::{
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
        assert_eq!(&formatted_clock(951_868_800), b"2000 03 01\n00 00 00");
        assert_eq!(&formatted_clock(1_790_553_600), b"2026 09 28\n00 00 00");
    }
}
