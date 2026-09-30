#[path = "../../rust/src/phone.rs"]
mod phone;

#[cfg(test)]
mod tests {
    use super::phone::{PHONE_TEXT_CAPACITY, format_phone_text};

    #[test]
    fn idle_and_alert_lines() {
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        let length = format_phone_text(0, 0, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"IDLE\nRING 0\nSMS 0");
        let length = format_phone_text(1, 1, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"ALERT\nRING 1\nSMS 1");
        assert!(length <= PHONE_TEXT_CAPACITY);
        assert!(
            buffer[..length]
                .iter()
                .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-'))
        );
    }
}
