//! Shared page for Messages → dhcp_client sealed UDP transmit (test-net merge).

pub const PAGE_SIZE: usize = 4096;
pub const MAX_DATAGRAM: usize = 128;
pub const NET_READY_MAGIC: u32 = u32::from_le_bytes(*b"NET1");

#[repr(C)]
pub struct MessagesNetTxPage {
    pub magic: u32,
    pub length: u32,
    pub payload: [u8; MAX_DATAGRAM],
}

impl MessagesNetTxPage {
    pub const fn new() -> Self {
        Self {
            magic: 0,
            length: 0,
            payload: [0; MAX_DATAGRAM],
        }
    }

    pub fn net_ready(&self) -> bool {
        self.magic == NET_READY_MAGIC
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
