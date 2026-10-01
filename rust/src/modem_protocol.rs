use sel4_microkit::MessageLabel;

pub const REQUEST_RING: MessageLabel = 0x501;
pub const REQUEST_SMS: MessageLabel = 0x502;
/// Outgoing digit string from the Phone dialer (length + two payload registers).
pub const REQUEST_DIAL: MessageLabel = 0x503;
pub const REPLY_OK: MessageLabel = 0x500;
pub const REPLY_BAD_REQUEST: MessageLabel = 0x5ff;
/// Max SMS body length carried in two message registers after the length word.
pub const SMS_MAX_BYTES: usize = 16;
/// Max dialed digit count; packed like SMS (length + two registers).
pub const DIAL_MAX_BYTES: usize = 16;
/// Supervisor channel id on the client PD (`system.system`) for RING/SMS.
pub const CLIENT_CHANNEL: usize = 12;
/// Phone PD channel id used for REQUEST_DIAL (`system.system`).
pub const PHONE_DIAL_CHANNEL: usize = 1;
