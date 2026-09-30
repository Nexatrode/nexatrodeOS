use sel4_microkit::MessageLabel;

pub const REQUEST_SET_ALERTS: MessageLabel = 0x601;
pub const REPLY_OK: MessageLabel = 0x600;
pub const REPLY_BAD_REQUEST: MessageLabel = 0x60f;
/// Supervisor channel id on the client PD (`system.system`).
pub const CLIENT_CHANNEL: usize = 13;
