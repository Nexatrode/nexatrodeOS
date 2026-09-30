use sel4_microkit::MessageLabel;

pub const REQUEST_RING: MessageLabel = 0x501;
pub const REQUEST_SMS: MessageLabel = 0x502;
pub const REPLY_OK: MessageLabel = 0x500;
pub const REPLY_BAD_REQUEST: MessageLabel = 0x5ff;
/// Max SMS body length carried in two message registers after the length word.
pub const SMS_MAX_BYTES: usize = 16;
/// Supervisor channel id on the client PD (`system.system`).
pub const CLIENT_CHANNEL: usize = 12;
