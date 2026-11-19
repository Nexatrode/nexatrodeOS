//! Test-only access to sealed tokens challenges the real native whitelist.
//! Production callers cannot create any of these requests.
use super::*;
#[test]
fn glink_memory_native_whitelist_rejects_forged_cursor_and_cross_item_stores() {
    for mode in 0..11 {
        crate::with_link_memory(|c, p| {
            let before = p.read(memory::PART, memory::PART_BYTES);
            let payload = c.parts().unwrap().1;
            payload.claim_glink().unwrap();
            let mut r = GlinkWrite {
                kind: Kind::TxHead,
                offset: 1,
                bytes: &[],
                value: 8,
            };
            match mode {
                0 => r.offset = 0,
                1 => r.offset = 2,
                2 => r.offset = usize::MAX,
                3 => r.value = 1,
                4 => r.value = TX.size as u32,
                5 => r.bytes = b"12345678",
                6 => {
                    r.kind = Kind::RxTail;
                    r.offset = 2;
                }
                7 => {
                    r.kind = Kind::Tx;
                    r.offset = TX.size;
                    r.bytes = b"12345678";
                }
                8 => {
                    r.kind = Kind::Tx;
                    r.offset = usize::MAX;
                    r.bytes = b"12345678";
                }
                9 => {
                    r.kind = Kind::Tx;
                    r.offset = 0;
                }
                _ => {
                    payload
                        .bind_glink_rx(&GlinkRx {
                            item: Item {
                                offset: memory::PART + memory::FINAL_FREE + 16,
                                size: 4096,
                            },
                        })
                        .unwrap();
                    r.kind = Kind::RxTail;
                    r.offset = 2;
                    r.value = 4096;
                }
            }
            assert!(payload.glink_store(&r).is_err(), "mode {mode}");
            assert_eq!(
                payload.glink_store(&GlinkWrite {
                    kind: Kind::TxHead,
                    offset: 1,
                    bytes: &[],
                    value: 8
                }),
                Err(GlinkError::Consumed)
            );
            assert_eq!(p.read(memory::PART, memory::PART_BYTES), before);
        });
    }
}
#[test]
fn glink_memory_native_bind_checks_extent_and_never_replaces_it() {
    for mode in 0..6 {
        crate::with_link_memory(|c, p| {
            let before = p.read(memory::PART, memory::PART_BYTES);
            let payload = c.parts().unwrap().1;
            let mut item = Item {
                offset: memory::PART + memory::FINAL_FREE + 16,
                size: 4096,
            };
            match mode {
                0 => item.offset = memory::PART,
                1 => item.offset += 1,
                2 => item.size = 7,
                3 => item.size = 4097,
                4 => item.offset = usize::MAX,
                _ => {
                    payload.bind_glink_rx(&GlinkRx { item }).unwrap();
                    item.offset += 8;
                }
            }
            assert!(payload.bind_glink_rx(&GlinkRx { item }).is_err());
            assert_eq!(payload.glink_descriptor(0), Err(GlinkError::Consumed));
            assert_eq!(p.read(memory::PART, memory::PART_BYTES), before);
        });
    }
}

#[cfg(feature = "sunfish-glink-aligned-fifo-reads")]
#[test]
fn aligned_fifo_reads_preserve_bytes_and_never_touch_outside_the_span() {
    use std::cell::RefCell;
    struct Checked {
        bytes: [u8; 64],
        start: usize,
        end: usize,
        reads: RefCell<Vec<(usize, usize)>>,
    }
    impl crate::qcom_modem_status::Read for Checked {
        fn read8(&self, at: usize) -> u8 {
            assert!(self.start <= at && at < self.end);
            self.reads.borrow_mut().push((at, 1));
            self.bytes[at]
        }
        fn read_le32(&self, at: usize) -> u32 {
            assert_eq!(at % 4, 0);
            assert!(self.start <= at && at + 4 <= self.end);
            self.reads.borrow_mut().push((at, 4));
            u32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap())
        }
    }
    for start in 0..16 {
        for len in 0..=48 {
            if start + len > 64 { continue; }
            let io = Checked { bytes: core::array::from_fn(|i| (i as u8).wrapping_mul(37)),
                start, end: start + len, reads: RefCell::new(Vec::new()) };
            let mut actual = vec![0; len];
            read_fifo_span(&io, start, &mut actual);
            assert_eq!(actual, io.bytes[start..start + len], "start={start} len={len}");
            let reads = io.reads.borrow();
            let accessed: usize = reads.iter().map(|(_, size)| size).sum();
            assert_eq!(accessed, len);
            if start % 4 == 0 && len >= 4 {
                assert!(reads.iter().any(|(_, size)| *size == 4));
            }
        }
    }
}
