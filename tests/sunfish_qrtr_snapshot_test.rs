#[path = "../rust/src/pixel_qrtr_snapshot.rs"]
mod pixel_qrtr_snapshot;

fn snapshot_with_frame72() -> [u8; 88] {
    let mut bytes = [0; 88];
    bytes[..16].copy_from_slice(&[9, 0, 2, 0, 2, 0, 0, 0, 52, 0, 0, 0, 0, 0, 0, 0]);
    for (index, byte) in bytes[16..72].iter_mut().enumerate() {
        *byte = index as u8;
    }
    // The splitter keeps these GLINK control records outside the QRTR frame;
    // the SERVREG probe validates their exact shape before consuming them.
    bytes[72..88].copy_from_slice(&[0x0e, 0, 2, 0, 1, 0, 0, 0, 0x07, 0, 2, 0, 0x3c, 0, 0, 0]);
    bytes
}

#[test]
fn bounded_snapshot_splits_72_byte_prefix_from_16_opaque_bytes() {
    let bytes = snapshot_with_frame72();
    let before = bytes;
    let split = pixel_qrtr_snapshot::split_snapshot(&bytes, bytes.len());

    assert_eq!(split.prefix_bytes, 72);
    assert_eq!(split.opaque_suffix_bytes, 16);
    assert_eq!(&bytes[72..], &before[72..]);
    assert_eq!(bytes, before);
}

#[test]
fn prefix_contents_do_not_change_boundaries_or_mutate_snapshot() {
    let mut bytes = snapshot_with_frame72();
    bytes[0] = 0xff;
    let before = bytes;
    let split = pixel_qrtr_snapshot::split_snapshot(&bytes, bytes.len());

    assert_eq!(split.prefix_bytes, 72);
    assert_eq!(split.opaque_suffix_bytes, 16);
    assert_eq!(bytes, before);
}

#[test]
fn short_snapshot_has_no_suffix_or_complete_frame_classification() {
    let bytes = snapshot_with_frame72();
    let split = pixel_qrtr_snapshot::split_snapshot(&bytes, 64);

    assert_eq!(split.prefix_bytes, 64);
    assert_eq!(split.opaque_suffix_bytes, 0);
}

#[test]
fn snapshots_are_bounded_to_the_frame_and_one_opaque_suffix() {
    let bytes = [0xaa; 128];
    let split = pixel_qrtr_snapshot::split_snapshot(&bytes, bytes.len());

    assert_eq!(split.prefix_bytes, 72);
    assert_eq!(split.opaque_suffix_bytes, 16);
    assert_eq!(bytes, [0xaa; 128]);
}
