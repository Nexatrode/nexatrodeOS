#[path = "../../rust/src/settings.rs"]
mod settings;

#[cfg(test)]
mod tests {
    use super::settings::{SETTINGS_TEXT_CAPACITY, format_settings_text};

    #[test]
    fn zero_and_nonzero_alert_lines() {
        let mut buffer = [0u8; SETTINGS_TEXT_CAPACITY];
        let length = format_settings_text(0, 0, &mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"LOCAL ONLY\nVAULT LOCAL\nNO NET YET\nNO ALERTS YET");
        let length = format_settings_text(1, 1, &mut buffer).unwrap();
        assert_eq!(
            &buffer[..length],
            b"LOCAL ONLY\nVAULT LOCAL\nNO NET YET\nALERTS RING 1 SMS 1"
        );
        assert!(length <= SETTINGS_TEXT_CAPACITY);
        assert!(
            buffer[..length]
                .iter()
                .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-'))
        );
    }
}
