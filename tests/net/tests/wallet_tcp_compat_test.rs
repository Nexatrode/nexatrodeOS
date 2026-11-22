//! Exercise the real wallet reader against legacy and wide replies, without a guest.
#![allow(dead_code)]
extern crate self as nexatrode_guest_https;
extern crate self as sel4;
extern crate self as sel4_microkit;

use std::sync::Mutex;

#[path = "../../../rust/src/wallet_tcp.rs"]
mod wallet_tcp;

#[derive(Debug)]
pub enum StreamError {
    Unavailable,
    InvalidReply,
}
pub trait PollTransport {
    type Error;
    fn try_read(&mut self, dst: &mut [u8]) -> Result<Option<usize>, Self::Error>;
    fn try_write(&mut self, src: &[u8]) -> Result<Option<usize>, Self::Error>;
}

#[repr(align(4096))]
struct Region([u8; 4096]);
static mut REGION: Region = Region([0; 4096]);
#[no_mangle]
static microkit_pps: usize = 0xf;
#[macro_export]
macro_rules! memory_region_symbol {
    ($id:ident: *mut $ty:ty) => {{
        unsafe {
            core::ptr::NonNull::<$ty>::new_unchecked(
                core::ptr::addr_of_mut!(crate::REGION.0).cast(),
            )
        }
    }};
}
#[macro_export]
macro_rules! debug_println { ($($arg:tt)*) => {{ let _ = format_args!($($arg)*); }}; }
pub fn r#yield() {}

#[derive(Default)]
struct Service {
    supports_wide: bool,
    advertised_capacity: Option<usize>,
    queries: usize,
    ordinary_reads: usize,
    wide_reads: usize,
    oversize: bool,
    wake_reads: usize,
}
static SERVICE: Mutex<Service> = Mutex::new(Service {
    supports_wide: false,
    advertised_capacity: None,
    queries: 0,
    ordinary_reads: 0,
    wide_reads: 0,
    oversize: false,
    wake_reads: 0,
});

pub struct MessageInfo(u64, usize);
impl MessageInfo {
    pub fn new(label: u64, count: usize) -> Self {
        Self(label, count)
    }
    pub fn label(&self) -> u64 {
        self.0
    }
    pub fn count(&self) -> usize { self.1 }
}
pub fn set_mr(index: usize, value: u64) {
    assert_eq!((index, value), (0, 1));
}
pub fn get_mr(index: usize) -> u64 {
    assert_eq!(index, 0);
    SERVICE.lock().unwrap().advertised_capacity.expect("legacy reply has no MR") as u64
}
#[derive(Clone, Copy)]
pub struct Channel(usize);
impl Channel {
    pub const fn new(id: usize) -> Self {
        Self(id)
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn pp_call(self, msg: MessageInfo) -> MessageInfo {
        let mut service = SERVICE.lock().unwrap();
        let slot = if self.index() == wallet_tcp::WALLET_DHCP_CHANNEL.index() {
            2
        } else {
            3
        };
        if msg.0 == 0x428 {
            assert_eq!(slot, 2);
            service.queries += 1;
            return MessageInfo(
                if service.supports_wide { 0x438 } else { 0x436 },
                usize::from(service.advertised_capacity.is_some()),
            );
        }
        let region = unsafe { &mut *core::ptr::addr_of_mut!(REGION.0) };
        let page = slot * 280;
        let count = u32::from_ne_bytes(region[page + 4..page + 8].try_into().unwrap()) as usize;
        if msg.count() != 0 {
            assert_eq!((slot, msg.label(), msg.count()), (2, 0x427, 1));
            service.wake_reads += 1;
        }
        let offset = match msg.0 {
            0x424 => {
                service.ordinary_reads += 1;
                assert!(count <= 128);
                page + 152
            }
            0x427 => {
                assert!(slot == 2 && service.supports_wide && count <= service.advertised_capacity.unwrap_or(1024));
                service.wide_reads += 1;
                2048
            }
            label => panic!("unexpected request {label:#x}"),
        };
        let advertised = count as u32 + u32::from(service.oversize);
        region[page + 8..page + 12].copy_from_slice(&advertised.to_ne_bytes());
        for (i, byte) in region[offset..offset + count].iter_mut().enumerate() {
            *byte = (i ^ 0xba) as u8;
        }
        MessageInfo(0x433, 0)
    }
}

fn main() {
    scan_preflight_sends_persistent_http_with_exact_body_length();
    let enabled = cfg!(feature = "wallet-wide-rx");
    for (capable, advertised_capacity, limit) in [
        (false, None, 128), (true, None, 1024), (true, Some(2048), 2048),
        (true, Some(8192), 2048), (true, Some(0), 128),
    ] {
        *SERVICE.lock().unwrap() = Service {
            supports_wide: capable,
            advertised_capacity,
            ..Service::default()
        };
        let mut client = wallet_tcp::WalletTcp::map();
        let expected = if enabled { limit } else { 128 };
        for _ in 0..2 {
            let mut out = [0xee; 2049];
            assert_eq!(client.try_read(&mut out).unwrap(), Some(expected));
            for (i, byte) in out[..expected].iter().enumerate() {
                assert_eq!(*byte, (i ^ 0xba) as u8);
            }
            assert_eq!(out[expected], 0xee);
        }
        assert_eq!(client.try_read(&mut [0; 64]).unwrap(), Some(64));
        assert_eq!(
            wallet_tcp::WalletTcp::map_worker()
                .try_read(&mut [0; 2048])
                .unwrap(),
            Some(128)
        );
        {
            let service = SERVICE.lock().unwrap();
            assert_eq!(service.queries, usize::from(enabled));
            assert_eq!(service.wide_reads, if expected > 128 { 2 } else { 0 });
            assert_eq!(service.ordinary_reads + service.wide_reads, 4);
        }
        assert_eq!(SERVICE.lock().unwrap().wake_reads, 0);
        assert_eq!(client.with_range_rx(|tcp| tcp.try_read(&mut [0; 2048])).unwrap(), Some(expected));
        assert_eq!(SERVICE.lock().unwrap().wake_reads, usize::from(expected > 128));
        client.try_read(&mut [0; 2048]).unwrap();
        assert_eq!(SERVICE.lock().unwrap().wake_reads, usize::from(expected > 128));
        SERVICE.lock().unwrap().oversize = true;
        let mut out = [0xee; 2048];
        assert!(matches!(
            client.try_read(&mut out),
            Err(StreamError::InvalidReply)
        ));
        assert!(out.iter().all(|b| *b == 0xee));
    }
}

fn scan_preflight_sends_persistent_http_with_exact_body_length() {
    let body = br#"{"method":"get_info"}"#;
    let mut out = [0; 256];
    let len = wallet_tcp::build_scan_preflight_http("node:443", body, &mut out).unwrap();
    assert_eq!(&out[..len], b"POST /json_rpc HTTP/1.1\r\nHost: node:443\r\nContent-Type: application/json\r\nContent-Length: 21\r\nAccept-Encoding: identity\r\n\r\n{\"method\":\"get_info\"}");
    assert!(wallet_tcp::build_scan_preflight_http("node:443", body, &mut out[..len-1]).is_none());
    let old = wallet_tcp::build_json_rpc_http("node:443", body, &mut out).unwrap();
    assert!(out[..old].windows(17).any(|s| s == b"Connection: close"));
}
