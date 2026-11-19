//! Exact parser for the observed QRTR NEW_SERVER frame plus GLINK controls.
//! It is deliberately independent of MMIO and does not authorize consumption.

pub const PREQUERY_BYTES: usize = 88;
pub const FRAME_BYTES: usize = 72;
const SERVICE: u32 = 0x2b;
pub const EXPECTED_INSTANCE: u32 = 0x0000_1202;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Server {
    pub service: u32,
    pub instance: u32,
    pub node: u32,
    pub port: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prequery {
    pub server: Server,
    pub returned_intent: u32,
    pub requested_intent_size: u32,
}

/// Decode the four NEW_SERVER identity words from a bounded snapshot without
/// asserting that the surrounding packet or identity is valid.
pub fn observed_server(bytes: &[u8; PREQUERY_BYTES], captured: usize) -> Option<Server> {
    if captured < 68 {
        return None;
    }
    Some(Server {
        service: word(bytes, 13),
        instance: word(bytes, 14),
        node: word(bytes, 15),
        port: word(bytes, 16),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    TooShort,
    GlinkHeader,
    GlinkIntent,
    GlinkLength,
    GlinkReserved,
    GlinkPadding,
    GlinkControls,
    QrtrHeader,
    ServerService,
    ServerInstance,
    ServerNode,
    ServerPort,
}

impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::TooShort => 0x10,
            Self::GlinkHeader => 1,
            Self::GlinkIntent => 2,
            Self::GlinkLength => 3,
            Self::GlinkReserved => 4,
            Self::GlinkPadding => 5,
            Self::GlinkControls => 6,
            Self::QrtrHeader => 7,
            Self::ServerService => 8,
            Self::ServerInstance => 9,
            Self::ServerNode => 10,
            Self::ServerPort => 11,
        }
    }
}

fn word(bytes: &[u8], index: usize) -> u32 {
    u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
}

pub fn parse(bytes: &[u8; PREQUERY_BYTES], captured: usize) -> Result<Prequery, Error> {
    if captured < PREQUERY_BYTES {
        return Err(Error::TooShort);
    }
    if bytes[..4] != [9, 0, 2, 0] {
        return Err(Error::GlinkHeader);
    }
    if bytes[4..8] != [3, 0, 0, 0] {
        return Err(Error::GlinkIntent);
    }
    if bytes[8..12] != [52, 0, 0, 0] {
        return Err(Error::GlinkLength);
    }
    if bytes[12..16] != [0; 4] {
        return Err(Error::GlinkReserved);
    }
    if bytes[68..FRAME_BYTES].iter().any(|byte| *byte != 0) {
        return Err(Error::GlinkPadding);
    }
    if bytes[72..88] != [14, 0, 2, 0, 1, 0, 0, 0, 7, 0, 2, 0, 60, 0, 0, 0] {
        return Err(Error::GlinkControls);
    }

    // QRTR v1 NEW_SERVER: exact control header and the one peer service
    // announcement observed on Sunfish. Other control records remain closed.
    let payload = &bytes[16..68];
    let mut header = [0; 9];
    for (index, value) in header.iter_mut().enumerate() {
        *value = word(payload, index);
    }
    if header != [1, 4, 0, 0xffff_fffe, 0, 20, 1, 0xffff_fffe, 4] {
        return Err(Error::QrtrHeader);
    }
    let server = Server {
        service: word(payload, 9),
        instance: word(payload, 10),
        node: word(payload, 11),
        port: word(payload, 12),
    };
    if server.service != SERVICE {
        return Err(Error::ServerService);
    }
    if server.instance != EXPECTED_INSTANCE {
        return Err(Error::ServerInstance);
    }
    if server.node != 0 {
        return Err(Error::ServerNode);
    }
    if server.port != 1 {
        return Err(Error::ServerPort);
    }

    Ok(Prequery {
        server,
        returned_intent: 1,
        requested_intent_size: 60,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed_prequery() -> [u8; PREQUERY_BYTES] {
        let mut bytes = [0; PREQUERY_BYTES];
        bytes[..16].copy_from_slice(&[9, 0, 2, 0, 3, 0, 0, 0, 52, 0, 0, 0, 0, 0, 0, 0]);
        let words = [
            1,
            4,
            0,
            0xffff_fffe,
            0,
            20,
            1,
            0xffff_fffe,
            4,
            SERVICE,
            EXPECTED_INSTANCE,
            0,
            1,
        ];
        for (index, value) in words.into_iter().enumerate() {
            let at = 16 + index * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[72..88].copy_from_slice(&[14, 0, 2, 0, 1, 0, 0, 0, 7, 0, 2, 0, 60, 0, 0, 0]);
        bytes
    }

    #[test]
    fn accepts_the_captured_new_server_and_control_pair() {
        let bytes = observed_prequery();
        assert_eq!(
            observed_server(&bytes, bytes.len()),
            Some(Server {
                service: SERVICE,
                instance: EXPECTED_INSTANCE,
                node: 0,
                port: 1,
            })
        );
        assert_eq!(
            parse(&bytes, bytes.len()),
            Ok(Prequery {
                server: Server {
                    service: SERVICE,
                    instance: EXPECTED_INSTANCE,
                    node: 0,
                    port: 1,
                },
                returned_intent: 1,
                requested_intent_size: 60,
            })
        );
    }

    #[test]
    fn rejects_short_or_changed_glink_qrtr_and_control_fields() {
        let bytes = observed_prequery();
        assert_eq!(parse(&bytes, PREQUERY_BYTES - 1), Err(Error::TooShort));
        assert_eq!(Error::TooShort.code(), 0x10);
        let mut old_assumption = bytes;
        old_assumption[56..60].copy_from_slice(&0x0001_2102u32.to_le_bytes());
        assert_eq!(
            parse(&old_assumption, old_assumption.len()),
            Err(Error::ServerInstance)
        );
        for (at, value, error, code) in [
            (4, 4, Error::GlinkIntent, 2),
            (20, 5, Error::QrtrHeader, 7),
            (52, 0x2c, Error::ServerService, 8),
            (56, 0x03, Error::ServerInstance, 9),
            (60, 0x01, Error::ServerNode, 10),
            (64, 0x02, Error::ServerPort, 11),
            (76, 2, Error::GlinkControls, 6),
            (84, 64, Error::GlinkControls, 6),
        ] {
            let mut changed = observed_prequery();
            changed[at] = value;
            assert_eq!(parse(&changed, changed.len()), Err(error), "offset {at}");
            assert_eq!(error.code(), code, "offset {at}");
        }
    }
}
