//! Bounded read-only pending control-message diagnostic. Never consumes RX,
//! accepts a channel, sends a response or grants transport-write authority.
use crate::{
    pixel_glink_receive::{self as receive, Header},
    pixel_glink_version,
    pixel_modem_status::sync,
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};

pub const MAX_BYTES: usize = 128;
pub const MAX_PACKETS: usize = MAX_BYTES / 8;
pub const MAX_NAME: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Packet {
    pub header: Header,
    pub offset: usize,
    pub name: [u8; MAX_NAME],
    pub name_len: usize,
}
const EMPTY: Packet = Packet {
    header: Header {
        command: 0,
        param1: 0,
        param2: 0,
    },
    offset: 0,
    name: [0; MAX_NAME],
    name_len: 0,
};
#[derive(Clone, Copy, Debug)]
pub struct Parsed {
    pub packets: [Packet; MAX_PACKETS],
    pub count: usize,
    pub parsed_bytes: usize,
    pub error: Option<Error>,
    pub unknown_command: bool,
    pub ack_in_prefix: bool,
}
pub fn parse(bytes: &[u8]) -> Parsed {
    let mut r = Parsed {
        packets: [EMPTY; MAX_PACKETS],
        count: 0,
        parsed_bytes: 0,
        error: None,
        unknown_command: false,
        ack_in_prefix: false,
    };
    if bytes.len() > MAX_BYTES || bytes.len() % 8 != 0 {
        r.error = Some(Error::Bounds);
        return r;
    }
    while r.parsed_bytes < bytes.len() {
        let at = r.parsed_bytes;
        let b = &bytes[at..at + 8];
        let header = Header {
            command: u16::from_le_bytes(b[..2].try_into().unwrap()),
            param1: u16::from_le_bytes(b[2..4].try_into().unwrap()),
            param2: u32::from_le_bytes(b[4..8].try_into().unwrap()),
        };
        let mut packet = Packet {
            header,
            offset: at,
            ..EMPTY
        };
        let size = match header.command {
            0 | 1 | 13 => 8,
            2 => {
                let n = header.param2 as usize;
                if header.param1 == 0 || n < 2 || n > MAX_NAME {
                    r.error = Some(Error::Payload);
                    break;
                }
                let size = (8 + n + 7) & !7;
                if size > bytes.len() - at {
                    r.error = Some(Error::Bounds);
                    break;
                }
                let name = &bytes[at + 8..at + 8 + n];
                if name[n - 1] != 0 || name[..n - 1].iter().any(|v| !(0x21..=0x7e).contains(v)) {
                    r.error = Some(Error::Payload);
                    break;
                }
                packet.name[..n - 1].copy_from_slice(&name[..n - 1]);
                packet.name_len = n - 1;
                size
            }
            _ => {
                r.unknown_command = true;
                break;
            }
        };
        r.packets[r.count] = packet;
        r.count += 1;
        r.parsed_bytes += size;
        if header == pixel_glink_version::ACK {
            r.ack_in_prefix = true
        }
    }
    r
}

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub bytes: [u8; MAX_BYTES],
    pub captured: usize,
    pub available: u32,
    pub words: [u32; 8],
    pub parsed: Parsed,
}
/// Caller holds the directory lock. FIFO producers are not serialized by that
/// lock: compare two bounded byte copies, descriptor/identity and ready state.
/// Only the private one-shot experiment invokes this after its fixed send.
pub(crate) fn read(
    m: &impl Read,
    baseline: &receive::Report,
    ready: &ReadyObservation,
    last_tail: u32,
    last_head: u32,
) -> Result<Snapshot, Error> {
    let before = pixel_glink_version::sample(m, baseline, ready, last_tail, last_head)?;
    let rx = before.inventory.items[2].ok_or(Error::ItemSize)?;
    let captured = (before.available as usize).min(MAX_BYTES);
    let mut bytes = [0; MAX_BYTES];
    let mut second = [0; MAX_BYTES];
    for (i, v) in bytes[..captured].iter_mut().enumerate() {
        *v = m.read8(rx.offset + (before.words[2] as usize + i) % receive::FIFO_BYTES)
    }
    sync();
    for (i, v) in second[..captured].iter_mut().enumerate() {
        *v = m.read8(rx.offset + (before.words[2] as usize + i) % receive::FIFO_BYTES)
    }
    if bytes != second {
        return Err(Error::Unstable);
    }
    sync();
    let after = pixel_glink_version::sample(m, baseline, ready, before.words[0], before.words[3])?;
    if before.words != after.words
        || before.available != after.available
        || before.header != after.header
        || before.inventory.items != after.inventory.items
        || before.inventory.directory != after.inventory.directory
    {
        return Err(Error::Unstable);
    }
    Ok(Snapshot {
        bytes,
        captured,
        available: after.available,
        words: after.words,
        parsed: parse(&bytes[..captured]),
    })
}
