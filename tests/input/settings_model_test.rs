#![allow(dead_code)]

#[path = "../../rust/src/settings.rs"]
mod settings;

#[cfg(test)]
mod host_tests {
    use super::settings::{SETTINGS_TEXT, SETTINGS_TEXT_LENGTH};

    #[test]
    fn stub_copy_is_stable_and_displayable() {
        assert_eq!(SETTINGS_TEXT, b"LOCAL ONLY\nVAULT LOCAL\nNO NET YET");
        assert_eq!(SETTINGS_TEXT_LENGTH, SETTINGS_TEXT.len());
        assert!(SETTINGS_TEXT_LENGTH <= 128);
        assert!(
            SETTINGS_TEXT
                .iter()
                .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-'))
        );
    }
}
