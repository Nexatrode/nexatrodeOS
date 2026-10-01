#[path = "../../rust/src/phone.rs"]
mod phone;

#[cfg(test)]
mod tests {
    use super::phone::{DIGIT_CAPACITY, PHONE_TEXT_CAPACITY, Phone, format_phone_text};

    #[test]
    fn idle_alert_dial_and_calling() {
        let mut buffer = [0u8; PHONE_TEXT_CAPACITY];
        let length = format_phone_text(0, 0, b"", false, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"IDLE\nRING 0\nSMS 0");

        let mut app = Phone::new();
        app.set_alerts(1, 1);
        assert_eq!(app.display(&mut buffer).unwrap(), b"ALERT\nRING 1\nSMS 1");
        for digit in [5u8, 5, 5, 1, 2, 3, 4] {
            assert!(app.press_digit(digit));
        }
        assert_eq!(
            app.display(&mut buffer).unwrap(),
            b"DIAL\n5551234\nRING 1 SMS 1"
        );
        assert!(app.press_call());
        assert_eq!(
            app.display(&mut buffer).unwrap(),
            b"CALLING\n5551234\nRING 1 SMS 1"
        );
        assert!(app.press_clear());
        assert_eq!(app.display(&mut buffer).unwrap(), b"ALERT\nRING 1\nSMS 1");
    }

    #[test]
    fn rejects_overflow_and_empty_call() {
        let mut app = Phone::new();
        assert!(!app.press_call());
        for _ in 0..DIGIT_CAPACITY {
            assert!(app.press_digit(1));
        }
        assert!(!app.press_digit(2));
    }
}
