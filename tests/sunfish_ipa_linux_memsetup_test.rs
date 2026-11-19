#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_config.rs"]
pub mod sunfish_ipa_linux_config;
#[path = "../rust/src/sunfish_ipa_linux_memory.rs"]
pub mod sunfish_ipa_linux_memory;
#[path = "../rust/src/sunfish_ipa_linux_memsetup.rs"]
pub mod sunfish_ipa_linux_memsetup;

use sunfish_ipa_linux_memsetup::*;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}

#[test]
fn memsetup_matches_compiled_linux_payloads_tres_and_doorbells() {
    let p = Plan::new(0x415).unwrap();
    let mut lines = vec![];
    for (i, c) in p.commands().iter().enumerate() {
        lines.push(format!(
            "command {i} {:04x} {:08x} {} {}",
            c.opcode(),
            c.payload_iova(),
            hex(c.payload()),
            hex(c.tre())
        ));
    }
    for (offset, value) in [
        p.channel_doorbell(),
        p.event_doorbell(),
        (p.processing_context().0, p.processing_context().1),
    ] {
        lines.push(format!("write {offset:05x} {value:08x}"));
    }
    let mut event = event(&p);
    event[8..10].copy_from_slice(&0x80a1u16.to_le_bytes());
    event[10] = 0x73;
    event[11] = 0x51;
    event[12..14].copy_from_slice(&0x8495u16.to_le_bytes());
    event[14] = 0x62;
    lines.push(format!(
        "event {:08x} {}",
        p.first_unfilled_event(),
        hex(&event)
    ));
    let parsed = p.completion(p.first_unfilled_event(), &event).unwrap();
    assert_eq!(
        (
            parsed.length,
            parsed.code,
            parsed.event_type,
            parsed.channel
        ),
        (0x80a1, 0x51, 0x62, 1)
    );
    let expected = std::fs::read_to_string(env!("SUNFISH_MEMSETUP_REFERENCE")).unwrap();
    assert_eq!(lines.join("\n") + "\n", expected);
}

#[test]
fn memsetup_rejects_any_other_shared_memory_geometry() {
    for bit in 0..32 {
        assert!(matches!(
            Plan::new(0x415 ^ (1 << bit)),
            Err(Error::Geometry)
        ));
    }
    for v in [0, u32::MAX, 0x400, 0x10000415, 0x00010415] {
        assert!(Plan::new(v).is_err());
    }
}

#[test]
fn memsetup_destinations_preserve_uc_tables_canaries_and_other_regions() {
    use sunfish_ipa_linux_buffers as b;
    use sunfish_ipa_linux_memory as m;
    let p = Plan::new(0x415).unwrap();
    let pool = b::Plan::new().unwrap();
    let mut touched = [false; 0x2000];
    for (i, c) in p.commands().iter().enumerate() {
        assert_eq!(c.payload_offset(), 0x6000 + i * 32);
        assert_eq!(c.tre_offset(), 0x4000 + i * 16);
        assert_eq!(c.payload_iova(), b::POOL_IOVA + c.payload_offset() as u64);
        assert_eq!(c.tre_iova(), b::POOL_IOVA + c.tre_offset() as u64);
        assert_eq!(c.payload_iova() & 31, 0);
        assert!(c.size() <= pool.find(b::Kind::Zero).unwrap().requested());
        for v in &mut touched[c.local_offset()..c.local_offset() + c.size()] {
            assert!(!*v);
            *v = true;
        }
        assert!(c.payload()[16..].iter().all(|b| *b == 0));
    }
    assert_eq!(
        touched.iter().filter(|v| **v).count(),
        0x140 + 0x200 + 0x200 + 0x140c
    );
    for w in m::words() {
        assert!(touched[w.offset()..w.offset() + 4].iter().all(|v| !*v));
    }
    for r in m::REGIONS {
        if !["MODEM_HEADER", "MODEM_PROC_CTX", "AP_PROC_CTX", "MODEM"].contains(&r.name) {
            assert!(touched[r.offset..r.offset + r.size].iter().all(|v| !*v));
        }
    }
    assert_eq!(p.processing_context(), (0x1e8, 0x5f0, 0x1ffff));
    assert_eq!(p.channel_doorbell(), (0x1e008, 0x80004040));
    assert_eq!(p.event_doorbell(), (0x1e100, 0x80003000));
}

fn event(p: &Plan) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&p.commands()[3].tre_iova().to_le_bytes());
    bytes[15] = 1;
    bytes
}

#[test]
fn memsetup_completion_rejects_foreign_pointers_channels_and_extra_events() {
    let p = Plan::new(0x415).unwrap();
    let bytes = event(&p);
    assert_eq!(
        p.completion(p.first_unfilled_event(), &bytes)
            .unwrap()
            .transfer,
        0x80004030
    );
    for bit in 0..32 {
        assert_eq!(
            p.completion(p.first_unfilled_event() ^ (1 << bit), &bytes),
            Err(Error::EventPointer)
        );
    }
    for bit in 0..64 {
        let mut bad = bytes;
        bad[bit / 8] ^= 1 << (bit % 8);
        assert_eq!(
            p.completion(p.first_unfilled_event(), &bad),
            Err(Error::TransferPointer)
        );
    }
    for channel in 0..=255 {
        let mut bad = bytes;
        bad[15] = channel;
        if channel != 1 {
            assert_eq!(
                p.completion(p.first_unfilled_event(), &bad),
                Err(Error::EventChannel)
            );
        }
    }
    for pointer in [
        p.event_iova() as u32,
        p.first_unfilled_event() + 16,
        0,
        u32::MAX,
    ] {
        assert_eq!(p.completion(pointer, &bytes), Err(Error::EventPointer));
    }
}

#[test]
fn memsetup_tx_event_fields_ignored_by_linux_stay_raw_diagnostics() {
    let p = Plan::new(0x415).unwrap();
    let mut bytes = event(&p);
    for code in 0..=255 {
        bytes[8..15].fill(code);
        let e = p.completion(p.first_unfilled_event(), &bytes).unwrap();
        assert_eq!(e.code, code);
        assert_eq!(e.event_type, code);
        assert_eq!(e.length, u16::from_le_bytes([code, code]));
    }
}
