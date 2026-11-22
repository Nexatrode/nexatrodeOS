use personal_os_slint_notes_spike::photo_preview::decode_jpeg_rgba;

#[test]
fn decodes_large_phone_photo_to_bounded_preview() {
    let jpeg = include_bytes!("fixtures/large-preview.jpg");
    let (width, height, pixels) = decode_jpeg_rgba(jpeg).expect("large photo preview");
    assert_eq!((width, height), (269, 480));
    assert_eq!(pixels.len(), width * height * 4);
}

#[test]
fn rejects_truncated_or_non_jpeg_data() {
    assert!(decode_jpeg_rgba(b"not a jpeg").is_none());
    assert!(decode_jpeg_rgba(&include_bytes!("fixtures/large-preview.jpg")[..32]).is_none());
}

#[cfg(feature = "utm-photo-worker")]
#[test]
fn worker_result_is_bounded_and_validated() {
    use personal_os_slint_notes_spike::photo_ipc::{
        HEADER, MAX_RGBA, encode_preview, preview_parts,
    };
    let jpeg = include_bytes!("fixtures/large-preview.jpg");
    let mut output = vec![0; HEADER + MAX_RGBA];
    let len = encode_preview(jpeg, &mut output).expect("worker preview");
    let (width, height, pixels) = preview_parts(&output[..len]).expect("valid worker result");
    assert_eq!((width, height), (269, 480));
    assert_eq!(pixels.len(), width * height * 4);
    output[..4].copy_from_slice(&481u32.to_le_bytes());
    assert!(preview_parts(&output[..len]).is_none());
    assert!(preview_parts(&[0; 7]).is_none());
}

#[cfg(feature = "utm-photo-worker")]
#[test]
fn completed_preview_only_updates_the_matching_conversation_and_transcript() {
    use personal_os_slint_notes_spike::photo_ipc::preview_matches;
    assert!(preview_matches(7, 4, 7, 4, true));
    assert!(!preview_matches(7, 4, 7, 5, true));
    assert!(!preview_matches(7, 4, 8, 4, true));
    assert!(!preview_matches(7, 4, 7, 4, false));
}

#[test]
fn rejects_images_beyond_the_worker_memory_budget() {
    let jpeg = include_bytes!("fixtures/over-budget.jpg");
    assert!(decode_jpeg_rgba(jpeg).is_none());
}

#[cfg(feature = "utm-photo-worker")]
#[test]
fn reader_request_rejects_invalid_owners_and_identifiers() {
    use personal_os_slint_notes_spike::photo_reader_ipc::{parse_request, request};
    assert_eq!(parse_request(&request(123, 456)), Some((123, 456)));
    assert_eq!(parse_request(&request(0, 456)), None);
    assert_eq!(parse_request(&request(u64::MAX, 456)), None);
    assert_eq!(parse_request(&request(123, 0)), None);
    assert_eq!(parse_request(&[0; 15]), None);
}
