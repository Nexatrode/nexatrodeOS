use sel4_microkit::MessageLabel;

pub const REQUEST_BYTES: MessageLabel = 0x401;
pub const REPLY_OK: MessageLabel = 0x400;
#[allow(dead_code)]
pub const REPLY_UNAVAILABLE: MessageLabel = 0x4ff;
pub const SAMPLE_BYTES: usize = 16;
#[allow(dead_code)]
pub const CLIENT_CHANNEL: usize = 9;
