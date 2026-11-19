#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_board.rs"]
mod ath10k_board;

use ath10k_board::{
    BoardFileError as Error, BoardIeKind, SnocQmiBoardNames, find_board_data,
    find_board_data_with_fallback, find_snoc_qmi_board_data,
};
use std::vec::Vec;

const BOARD_IE: u32 = 0;
const BOARD_EXT_IE: u32 = 1;
const NAME_IE: u32 = 0;
const DATA_IE: u32 = 1;

fn push_ie(out: &mut Vec<u8>, id: u32, payload: &[u8]) {
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn board_ie(name: &[u8], data: &[u8]) -> Vec<u8> {
    let mut nested = Vec::new();
    push_ie(&mut nested, NAME_IE, name);
    push_ie(&mut nested, DATA_IE, data);
    nested
}

fn board_file(entries: &[(u32, &[u8])]) -> Vec<u8> {
    let mut out = b"QCA-ATH10K-BOARD\0".to_vec();
    while out.len() % 4 != 0 {
        out.push(0);
    }
    for (kind, section) in entries {
        push_ie(&mut out, *kind, section);
    }
    out
}

#[test]
fn finds_exact_name_and_returns_borrowed_data() {
    let data = [0x11, 0x22, 0x33, 0x44, 0x55];
    let file = board_file(&[(BOARD_IE, &board_ie(b"bus=snoc,board", &data))]);
    let found = find_board_data(&file, b"bus=snoc,board", BoardIeKind::Board).unwrap();
    assert_eq!(found.candidate_index, 0);
    assert_eq!(found.bytes, data);
    assert_eq!(found.kind, BoardIeKind::Board);
    assert!(std::ptr::eq(
        found.bytes.as_ptr(),
        file.windows(data.len())
            .find(|w| *w == data)
            .unwrap()
            .as_ptr()
    ));
}

#[test]
fn honors_candidate_priority_not_container_order() {
    let older = board_ie(b"fallback", b"fallback-data");
    let preferred = board_ie(b"preferred", b"preferred-data");
    let file = board_file(&[(BOARD_IE, &older), (BOARD_IE, &preferred)]);
    let found =
        find_board_data_with_fallback(&file, &[b"preferred", b"fallback"], BoardIeKind::Board)
            .unwrap();
    assert_eq!(found.candidate_index, 0);
    assert_eq!(found.bytes, b"preferred-data");
}

#[test]
fn requires_board_name_before_data_element() {
    let mut section = Vec::new();
    push_ie(&mut section, DATA_IE, b"wrong-order");
    push_ie(&mut section, NAME_IE, b"sunfish");
    push_ie(&mut section, DATA_IE, b"right-order");
    let file = board_file(&[(BOARD_IE, &section)]);
    let found = find_board_data(&file, b"sunfish", BoardIeKind::Board).unwrap();
    assert_eq!(found.bytes, b"right-order");
}

#[test]
fn board_and_extended_board_data_are_distinct() {
    let board = board_ie(b"sunfish", b"board-data");
    let extended = board_ie(b"sunfish", b"extended-data");
    let file = board_file(&[(BOARD_IE, &board), (BOARD_EXT_IE, &extended)]);
    assert_eq!(
        find_board_data(&file, b"sunfish", BoardIeKind::Board)
            .unwrap()
            .bytes,
        b"board-data"
    );
    assert_eq!(
        find_board_data(&file, b"sunfish", BoardIeKind::BoardExt)
            .unwrap()
            .bytes,
        b"extended-data"
    );
}

#[test]
fn qmi_snoc_names_match_pinned_hex_and_variant_order() {
    let names = SnocQmiBoardNames::new(0x18, 0x22, Some(b"google_sunfish")).unwrap();
    assert_eq!(
        names.as_bytes(),
        [
            b"bus=snoc,qmi-board-id=18,qmi-chip-id=22,variant=google_sunfish".as_slice(),
            b"bus=snoc,qmi-board-id=18,qmi-chip-id=22".as_slice(),
            b"bus=snoc,qmi-board-id=18".as_slice(),
        ]
    );
}

#[test]
fn qmi_snoc_fallback_uses_board_only_after_both_full_id_names() {
    let section = board_ie(b"bus=snoc,qmi-board-id=18", b"fallback-data");
    let file = board_file(&[(BOARD_IE, &section)]);
    let found = find_snoc_qmi_board_data(&file, 0x18, 0x22, Some(b"google_sunfish")).unwrap();
    assert_eq!(found.candidate_index, 2);
    assert_eq!(found.bytes, b"fallback-data");
}

#[test]
fn absent_variant_keeps_the_source_duplicate_first_candidate() {
    let names = SnocQmiBoardNames::new(0, 0, None).unwrap();
    assert_eq!(
        names.as_bytes()[0],
        b"bus=snoc,qmi-board-id=0,qmi-chip-id=0"
    );
    assert_eq!(names.as_bytes()[0], names.as_bytes()[1]);
}

#[test]
fn rejects_short_or_wrong_magic_files() {
    assert_eq!(
        find_board_data(b"QCA", b"sunfish", BoardIeKind::Board),
        Err(Error::FileTooShort)
    );
    let mut bad = board_file(&[]);
    bad[0] = b'X';
    assert_eq!(
        find_board_data(&bad, b"sunfish", BoardIeKind::Board),
        Err(Error::InvalidMagic)
    );
}

#[test]
fn malformed_outer_and_nested_lengths_fail_closed() {
    let mut outer = board_file(&[(BOARD_IE, &board_ie(b"sunfish", b"ok"))]);
    let outer_len = 20;
    outer[outer_len + 4..outer_len + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        find_board_data(&outer, b"sunfish", BoardIeKind::Board),
        Err(Error::IePayloadTruncated)
    );

    let mut nested = board_file(&[(BOARD_IE, &board_ie(b"sunfish", b"ok"))]);
    let inner_len_offset = outer_len + 8 + 8 + 8 + 4;
    nested[inner_len_offset..inner_len_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        find_board_data(&nested, b"sunfish", BoardIeKind::Board),
        Err(Error::IePayloadTruncated)
    );
}

#[test]
fn ignores_only_the_source_defined_short_final_tail() {
    let section = board_ie(b"sunfish", b"payload");
    let mut file = board_file(&[(BOARD_IE, &section)]);
    file.extend_from_slice(b"tail");
    assert_eq!(
        find_board_data(&file, b"sunfish", BoardIeKind::Board)
            .unwrap()
            .bytes,
        b"payload"
    );
}

#[test]
fn rejects_empty_names_and_overlong_variant() {
    let file = board_file(&[]);
    assert_eq!(
        find_board_data(&file, b"", BoardIeKind::Board),
        Err(Error::EmptyName)
    );
    assert_eq!(
        SnocQmiBoardNames::new(1, 2, Some(&[b'x'; 32])),
        Err(Error::InvalidVariant)
    );
}

#[test]
fn no_implicit_generic_fallback_is_performed() {
    let section = board_ie(b"bus=snoc,board=generic", b"generic-data");
    let file = board_file(&[(BOARD_IE, &section)]);
    assert_eq!(
        find_snoc_qmi_board_data(&file, 0x18, 0x22, Some(b"google_sunfish")),
        Err(Error::NoMatchingEntry)
    );
}
