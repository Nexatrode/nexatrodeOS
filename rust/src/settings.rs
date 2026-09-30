//! Static Settings stub copy for the local-only device shell.

pub const SETTINGS_TEXT: &[u8] = b"LOCAL ONLY\nVAULT LOCAL\nNO NET YET\nNO ALERTS YET";
pub const SETTINGS_TEXT_LENGTH: usize = SETTINGS_TEXT.len();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_text_fits_the_app_view_alphabet() {
        assert!(SETTINGS_TEXT_LENGTH <= 128);
        assert!(
            SETTINGS_TEXT
                .iter()
                .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'\n' | b'-'))
        );
    }
}
