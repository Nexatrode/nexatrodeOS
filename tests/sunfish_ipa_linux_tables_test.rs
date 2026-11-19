#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
pub mod sunfish_ipa_linux_config;
#[path = "../rust/src/sunfish_ipa_linux_memory.rs"]
pub mod sunfish_ipa_linux_memory;
#[path = "../rust/src/sunfish_ipa_linux_memsetup.rs"]
pub mod sunfish_ipa_linux_memsetup;
#[path = "../rust/src/sunfish_ipa_linux_tables.rs"]
pub mod sunfish_ipa_linux_tables;
use sunfish_ipa_linux_tables::*;
fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02x}")).collect()
}

#[test]
fn tables_match_original_linux_allocation_six_commands_and_doorbells() {
    let p = Plan::new(0x415).unwrap();
    let mut lines = vec![format!(
        "table {:08x} {}",
        p.table_iova(),
        hex(p.table_bytes())
    )];
    for (i, c) in p.commands().iter().enumerate() {
        lines.push(format!(
            "command {i} {:04x} {:08x} {} {}",
            c.opcode(),
            c.payload_iova(),
            hex(c.payload()),
            hex(c.tre())
        ));
    }
    for (o, v) in [p.channel_doorbell(), p.event_doorbell()] {
        lines.push(format!("write {o:05x} {v:08x}"));
    }
    assert_eq!(
        lines.join("\n") + "\n",
        std::fs::read_to_string(env!("SUNFISH_TABLES_C_REFERENCE")).unwrap()
    );
}
#[test]
fn tables_cover_exact_nonhashed_spans_preserve_all_other_memory() {
    use sunfish_ipa_linux_memory as m;
    let p = Plan::new(0x415).unwrap();
    let mut touched = [false; 0x2000];
    assert_eq!(p.commands().map(|c| c.opcode()), [7, 8, 3, 0x13, 4, 0x13]);
    assert_eq!(
        p.commands().map(|c| (c.local_offset(), c.size())),
        [
            (0x3a0, 120),
            (0x428, 120),
            (0x290, 24),
            (0x2a8, 96),
            (0x318, 24),
            (0x330, 96)
        ]
    );
    for c in p.commands() {
        for v in &mut touched[c.local_offset()..c.local_offset() + c.size()] {
            assert!(!*v);
            *v = true;
        }
    }
    assert_eq!(touched.iter().filter(|v| **v).count(), 480);
    for w in m::words() {
        assert!(touched[w.offset()..w.offset() + 4].iter().all(|v| !*v));
    }
    for r in m::REGIONS {
        if !["V4_FILTER", "V6_FILTER", "V4_ROUTE", "V6_ROUTE"].contains(&r.name) {
            assert!(touched[r.offset..r.offset + r.size].iter().all(|v| !*v));
        }
    }
    for offset in (0..0x2000).step_by(4) {
        assert_eq!(p.expected32(offset).is_some(), touched[offset]);
    }
    assert_eq!(p.expected32(0x290), Some(0x24));
    assert_eq!(p.expected32(0x294), Some(0));
    assert_eq!(p.expected32(0x298), Some(0x80002000));
    assert_eq!(p.expected32(0x2a8), Some(0));
    for offset in (0x3a0..0x418).step_by(8) {
        assert_eq!(p.expected32(offset), Some(0x80002000));
        assert_eq!(p.expected32(offset + 4), Some(0));
    }
    assert_eq!(p.expected32(0x291), None);
}
#[test]
fn tables_use_fresh_slots_after_memory_transaction_and_keep_hash_fields_zero() {
    let p = Plan::new(0x415).unwrap();
    let first = sunfish_ipa_linux_memsetup::Plan::new(0x415).unwrap();
    for (i, c) in p.commands().iter().enumerate() {
        assert_eq!(c.payload_offset(), 0x6080 + i * 32);
        assert_eq!(c.tre_offset(), 0x4040 + i * 16);
        assert_eq!(c.payload_iova(), 0x80000000 + c.payload_offset() as u64);
        assert_eq!(c.tre_iova(), 0x80000000 + c.tre_offset() as u64);
        for old in first.commands() {
            assert!(c.tre_offset() >= old.tre_offset() + 16);
            assert!(c.payload_offset() >= old.payload_offset() + 32);
        }
        if c.opcode() != 0x13 {
            assert!(c.payload()[..8].iter().all(|v| *v == 0));
            assert!(c.payload()[24..].iter().all(|v| *v == 0));
        }
    }
    assert_eq!(p.channel_doorbell(), (0x1e008, 0x800040a0));
    assert_eq!(p.event_doorbell(), (0x1e100, 0x80003010));
    assert_eq!(p.initial_event_producer(), first.first_unfilled_event());
    assert_eq!(p.first_unfilled_event(), 0x80003020);
}
#[test]
fn tables_reject_other_geometry_and_foreign_stale_or_extra_events() {
    for bit in 0..32 {
        assert!(Plan::new(0x415 ^ (1 << bit)).is_err());
    }
    let p = Plan::new(0x415).unwrap();
    let mut event = [0u8; 16];
    event[..8].copy_from_slice(&0x80004090u64.to_le_bytes());
    event[15] = 1;
    assert_eq!(
        p.completion(0x80003020, &event).unwrap().transfer,
        0x80004090
    );
    for v in [0, 0x80003000, 0x80003010, 0x80003030, u32::MAX] {
        assert_eq!(p.completion(v, &event), Err(Error::EventPointer));
    }
    for bit in 0..64 {
        let mut bad = event;
        bad[bit / 8] ^= 1 << (bit % 8);
        assert_eq!(p.completion(0x80003020, &bad), Err(Error::TransferPointer));
    }
    for channel in 0..=255 {
        let mut bad = event;
        bad[15] = channel;
        if channel != 1 {
            assert_eq!(p.completion(0x80003020, &bad), Err(Error::EventChannel));
        }
    }
}
