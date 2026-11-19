//! Aligned host RAM fixture; numerical physical identities simulate a device.
#![allow(dead_code)]
use crate::{sunfish_ipa_shared_memory::SharedMemory, sunfish_ipa_smem_native::Window};

#[repr(C, align(4096))]
#[derive(Clone)]
pub struct Page(pub [u8; 4096]);
pub struct Fixture {
    pub smem: Vec<Page>,
    pub imem: Vec<Page>,
    pub lock: Box<Page>,
}
impl Fixture {
    pub fn new() -> Self {
        let mut f = Self {
            smem: vec![Page([0x5a; 4096]); 512],
            imem: vec![Page([0x77; 4096]); 2],
            lock: Box::new(Page([0; 4096])),
        };
        let p = 0x11d000;
        let t = 0x1ff000;
        f.smem[511].0.fill(0);
        for (at, value) in [
            (92, 12 << 16),
            (192, 1),
            (204, 0),
            (t, 0x434f5424),
            (t + 4, 1),
            (t + 8, 11),
            (t + 32, t as u32),
            (t + 36, 4096),
            (t + 44, 0xffffffff),
            (t + 32 + 7 * 48, p as u32),
            (t + 36 + 7 * 48, 0x7c000),
            (t + 44 + 7 * 48, 0x00010000),
            (t + 48 + 7 * 48, 32),
            (p, 0x54525024),
            (p + 4, 0x00010000),
            (p + 8, 0x7c000),
            (p + 12, 32),
            (p + 16, 0x7c000),
            (t + 32 + 11 * 48, 0x49494953),
            (t + 32 + 11 * 48 + 16, 621),
        ] {
            f.word(at, value);
        }
        f
    }
    pub fn word(&mut self, at: usize, value: u32) {
        self.smem[at / 4096].0[at % 4096..at % 4096 + 4].copy_from_slice(&value.to_le_bytes());
    }
    pub fn shared(&mut self) -> SharedMemory<'_> {
        let smem = self.smem.as_mut_ptr().cast::<u8>();
        // SAFETY: exclusive aligned host RAM simulates the exact views. This
        // fixture is never linked into or used to authorize a hardware image.
        unsafe {
            let window = Window::new(
                smem,
                0x86000000,
                smem.add(0x11d000).cast(),
                0x8611d000,
                self.lock.0.as_mut_ptr().cast(),
                0x01f43000,
            )
            .unwrap();
            SharedMemory::new(window, self.imem.as_ptr().cast(), 0x146a8000, 8192).unwrap()
        }
    }
}
