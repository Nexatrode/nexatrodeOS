// SPDX-License-Identifier: BSD-2-Clause
//! Browser-exclusive slot 2 in an otherwise app-free disposable network image.
use core::ptr::NonNull;
use nexatrode_guest_https::{PollTransport, StreamError};
use sel4_microkit::{Channel, MessageInfo, get_mr, memory_region_symbol};
const NET: Channel = Channel::new(1);
#[repr(C)]
struct Page {
    magic: u32,
    request_len: u32,
    response_len: u32,
    slot: u32,
    ip: [u8; 4],
    port: u16,
    pad: u16,
    request: [u8; 128],
    response: [u8; 128],
}
pub struct Tcp {
    page: NonNull<Page>,
    base: *mut u8,
    wide: usize,
    literal_ip: Option<[u8;4]>,
}
impl Tcp {
    pub fn new() -> Self {
        let base = memory_region_symbol!(haskell_tcp_page:*mut u8).as_ptr();
        let page = unsafe { NonNull::new_unchecked(base.cast::<Page>().add(2)) };
        unsafe {
            page.as_ptr().write(Page {
                magic: 0x31504354,
                request_len: 0,
                response_len: 0,
                slot: 2,
                ip: [0; 4],
                port: 0,
                pad: 0,
                request: [0; 128],
                response: [0; 128],
            });
        }
        Self {
            page,
            base,
            wide: 0,
            literal_ip: None,
        }
    }
    fn p(&mut self) -> &mut Page {
        unsafe { self.page.as_mut() }
    }
    fn call(&mut self, label: u64) -> u64 {
        self.p().magic = 0x31504354;
        self.p().slot = 2;
        NET.pp_call(MessageInfo::new(label, 0)).label()
    }
    pub fn cancel(&mut self) {
        self.call(0x426);
    }
    pub fn dns_start(&mut self, host: &str) -> Result<bool, ()> {
        self.literal_ip=host.parse::<core::net::Ipv4Addr>().ok().map(|ip|ip.octets());
        if self.literal_ip.is_some(){return Ok(true);}
        let p = self.p();
        p.request[..host.len()].copy_from_slice(host.as_bytes());
        p.request_len = host.len() as u32;
        match self.call(0x440) {
            0x430 | 0x433 => Ok(true),
            0x431 | 0x435 | 0x437 => Ok(false),
            _ => Err(()),
        }
    }
    pub fn dns_poll(&mut self) -> Result<Option<[u8; 4]>, ()> {
        if self.literal_ip.is_some(){return Ok(self.literal_ip.take());}
        match self.call(0x441) {
            0x433 if self.p().response_len == 4 => {
                let mut ip = [0u8; 4];
                ip.copy_from_slice(&self.p().response[..4]);
                Ok(Some(ip))
            }
            0x431 | 0x435 | 0x437 => Ok(None),
            _ => Err(()),
        }
    }
    pub fn open(&mut self, ip: [u8; 4], port: u16) -> Result<(), ()> {
        self.p().ip = ip;
        self.p().port = port;
        match self.call(0x421) {
            0x430 | 0x432 => Ok(()),
            _ => Err(()),
        }
    }
    pub fn connected(&mut self) -> Result<bool, ()> {
        match self.call(0x422) {
            0x432 => Ok(true),
            0x431 | 0x435 => Ok(false),
            _ => Err(()),
        }
    }
}
impl PollTransport for Tcp {
    type Error = StreamError;
    fn try_write(&mut self, data: &[u8]) -> Result<Option<usize>, Self::Error> {
        let n = data.len().min(128);
        self.p().request[..n].copy_from_slice(&data[..n]);
        self.p().request_len = n as u32;
        match self.call(0x423) {
            0x430 => Ok(Some(n)),
            0x431 | 0x435 => Ok(None),
            _ => Err(StreamError::Unavailable),
        }
    }
    fn try_read(&mut self, out: &mut [u8]) -> Result<Option<usize>, Self::Error> {
        if self.wide == 0 {
            let reply = NET.pp_call(MessageInfo::new(0x428, 0));
            self.wide = if reply.label() == 0x438 {
                if reply.count() == 1 {
                    (get_mr(0) as usize).clamp(128, 2048)
                } else {
                    1024
                }
            } else {
                128
            };
        }
        let n = out.len().min(self.wide);
        self.p().request_len = n as u32;
        self.p().response_len = 0;
        match self.call(if n > 128 { 0x427 } else { 0x424 }) {
            0x433 => {
                let count = self.p().response_len as usize;
                if count > n {
                    return Err(StreamError::InvalidReply);
                }
                if n > 128 {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            self.base.add(2048),
                            out.as_mut_ptr(),
                            count,
                        );
                    }
                } else {
                    out[..count].copy_from_slice(&self.p().response[..count]);
                }
                Ok(if count == 0 { None } else { Some(count) })
            }
            0x431 | 0x435 => Ok(None),
            0x434 => Ok(Some(0)),
            _ => Err(StreamError::Unavailable),
        }
    }
}
