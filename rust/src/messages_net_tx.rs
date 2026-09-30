//! Shared page for Messages → dhcp_client sealed UDP/TLS transmit (test-net merge).

pub const PAGE_SIZE: usize = 4096;
pub const MAX_DATAGRAM: usize = 128;
pub const NET_READY_MAGIC: u32 = u32::from_le_bytes(*b"NET1");
/// Also queue the datagram on the open guest TLS session (`MSG TLS TX`).
pub const TX_FLAG_TLS: u16 = 0x1;

/// Default SLIRP host gateway used by `./dev test-net` + toy_router.
pub const DEMO_PEER_IP: [u8; 4] = [10, 0, 2, 2];
pub const DEMO_PEER_PORT: u16 = 1237;

#[repr(C)]
pub struct MessagesNetTxPage {
    pub magic: u32,
    pub length: u32,
    /// IPv4 octets (a.b.c.d). All-zero means “use demo peer”.
    pub dest_ip: [u8; 4],
    /// UDP destination port (host endian). Zero means “use demo peer”.
    pub dest_port: u16,
    pub reserved: u16,
    pub payload: [u8; MAX_DATAGRAM],
}

impl MessagesNetTxPage {
    pub const fn new() -> Self {
        Self {
            magic: 0,
            length: 0,
            dest_ip: [0; 4],
            dest_port: 0,
            reserved: 0,
            payload: [0; MAX_DATAGRAM],
        }
    }

    pub fn net_ready(&self) -> bool {
        self.magic == NET_READY_MAGIC
    }

    pub fn set_peer(&mut self, ipv4: [u8; 4], port: u16) {
        self.dest_ip = ipv4;
        self.dest_port = port;
        self.reserved = 0;
    }

    pub fn set_demo_peer(&mut self) {
        self.set_peer(DEMO_PEER_IP, DEMO_PEER_PORT);
    }

    /// Demo peer over UDP plus TLS when the guest TLS session is up.
    pub fn set_demo_peer_tls(&mut self) {
        self.set_peer(DEMO_PEER_IP, DEMO_PEER_PORT);
        self.reserved = TX_FLAG_TLS;
    }

    /// Resolved destination for dhcp_client. Empty IP or port 0 → demo host gateway.
    pub fn peer(&self) -> ([u8; 4], u16) {
        if self.dest_ip == [0, 0, 0, 0] || self.dest_port == 0 {
            (DEMO_PEER_IP, DEMO_PEER_PORT)
        } else {
            (self.dest_ip, self.dest_port)
        }
    }

    pub fn set_datagram(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() > MAX_DATAGRAM {
            return false;
        }
        self.payload[..bytes.len()].copy_from_slice(bytes);
        self.payload[bytes.len()..].fill(0);
        self.length = bytes.len() as u32;
        true
    }

    pub fn datagram(&self) -> Option<&[u8]> {
        let length = self.length as usize;
        if length == 0 || length > MAX_DATAGRAM {
            None
        } else {
            Some(&self.payload[..length])
        }
    }
}

const _: () = assert!(core::mem::size_of::<MessagesNetTxPage>() <= PAGE_SIZE);
const _: () = assert!(core::mem::offset_of!(MessagesNetTxPage, payload) == 16);
