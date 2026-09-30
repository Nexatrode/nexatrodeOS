use sel4_microkit::MessageLabel;

pub const REQUEST_SET_ALERTS: MessageLabel = 0x701;
pub const REPLY_OK: MessageLabel = 0x700;
pub const REPLY_BAD_REQUEST: MessageLabel = 0x70f;
/// Supervisor channel id on the client PD (`system.system`).
pub const CLIENT_CHANNEL: usize = 14;
