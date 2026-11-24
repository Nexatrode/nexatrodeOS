//! One document, one process generation. No vault capability or key is held here.
use core::ptr;
use sel4_microkit::{Channel, MessageInfo, get_mr, set_mr, debug_println};
use slint::SharedString;
use zeroize::Zeroizing;
const MAX: usize = 2048;
const LOAD: u64 = 0x7a0;
const SAVE: u64 = 0x7a1;
#[repr(C)] struct Page { length: u32, reserved: u32, text: [u8; MAX] }
const _: () = assert!(core::mem::size_of::<Page>() == 2056);
fn page() -> *mut Page { 0x6a000000 as *mut Page }
fn clear() { for i in 0..4096 { unsafe { ptr::write_volatile((page() as *mut u8).add(i), 0); } } }
pub struct Store { generation: u64, revision: u64 }
impl Store {
    pub fn new(generation: u64) -> Self { Self { generation, revision: 0 } }
    pub fn revision(&self) -> u64 { self.revision }
    pub fn snapshot(&self, text: &str) -> Result<(), u64> {
        clear();
        if text.len() > MAX { return Err(2); }
        for (i, byte) in text.as_bytes().iter().enumerate() {
            unsafe { ptr::write_volatile(ptr::addr_of_mut!((*page()).text).cast::<u8>().add(i), *byte); }
        }
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*page()).length), text.len() as u32); }
        Ok(())
    }
    pub fn load(&mut self) -> Result<SharedString, u64> {
        set_mr(0, self.generation);
        let reply = Channel::new(0).pp_call(MessageInfo::new(LOAD, 1));
        if reply.label() != 0 || reply.count() != 1 { clear(); return Err(reply.label()); }
        self.revision = get_mr(0);
        let length = unsafe { ptr::read_volatile(ptr::addr_of!((*page()).length)) } as usize;
        let reserved = unsafe { ptr::read_volatile(ptr::addr_of!((*page()).reserved)) };
        if length > MAX || reserved != 0 { clear(); return Err(2); }
        let mut bytes = Zeroizing::new([0u8; MAX]);
        for (i, byte) in bytes[..length].iter_mut().enumerate() {
            *byte = unsafe { ptr::read_volatile(ptr::addr_of!((*page()).text).cast::<u8>().add(i)) };
        }
        clear();
        core::str::from_utf8(&bytes[..length]).map(SharedString::from).map_err(|_| 2)
    }
    pub fn save(&mut self, text: &str) -> Result<(), u64> {
        self.snapshot(text)?;
        set_mr(0, self.generation); set_mr(1, self.revision);
        let reply = Channel::new(0).pp_call(MessageInfo::new(SAVE, 2));
        clear();
        if reply.label() != 0 || reply.count() != 1 { return Err(reply.label()); }
        self.revision = get_mr(0); Ok(())
    }
    pub fn check_isolation(&self) {
        for supplied in [self.generation.wrapping_add(1), self.generation - 1] {
            set_mr(0, supplied);
            assert_eq!(Channel::new(0).pp_call(MessageInfo::new(LOAD, 1)).label(), 1, "stale storage handle accepted");
            set_mr(0, supplied); set_mr(1, 0);
            assert_eq!(Channel::new(0).pp_call(MessageInfo::new(SAVE, 2)).label(), 1, "stale storage writer accepted");
        }
        // There is no namespace selector in this ABI. Extra arguments cannot
        // turn the Notes handle into a wallet/other-app handle.
        set_mr(0, self.generation); set_mr(1, 8);
        assert_eq!(Channel::new(0).pp_call(MessageInfo::new(LOAD, 2)).label(), 1);
        for (length, reserved) in [(MAX as u32 + 1, 0), (0, 1)] {
            clear();
            unsafe { (*page()).length = length; (*page()).reserved = reserved; }
            set_mr(0, self.generation); set_mr(1, u64::MAX);
            assert_eq!(Channel::new(0).pp_call(MessageInfo::new(SAVE, 2)).label(), 2, "malformed note accepted");
        }
        clear(); set_mr(0, self.generation); set_mr(1, u64::MAX);
        assert_eq!(Channel::new(0).pp_call(MessageInfo::new(SAVE, 2)).label(), 4, "stale document revision accepted");
        clear();
        debug_println!("PRIVATE_NOTES_APP|ISOLATION_CHECKED|generation={}", self.generation);
    }
}
