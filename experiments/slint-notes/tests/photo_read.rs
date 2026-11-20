#[path = "../src/photo_read.rs"]
mod photo_read;
use photo_read::{CHUNK, read_photo};

#[test]
fn reads_all_chunks_in_order_and_validates_lengths() {
    let source = vec![0x5a; CHUNK * 2 + 17];
    let mut out = vec![0; source.len()];
    let mut seen = Vec::new();
    let n = read_photo(7, 9, &mut out, |owner, id, part, dst| {
        assert_eq!((owner, id), (7, 9));
        seen.push(part);
        let start = part * CHUNK;
        let end = source.len().min(start + CHUNK);
        dst[..end - start].copy_from_slice(&source[start..end]);
        Some((source.len(), end - start))
    });
    assert_eq!(n, Some(source.len()));
    assert_eq!(out, source);
    assert_eq!(seen, [0, 1, 2]);
}

#[test]
fn rejects_invalid_owners_ids_and_incomplete_or_changed_data() {
    let mut out = vec![0; CHUNK * 2];
    assert_eq!(read_photo(0, 9, &mut out, |_, _, _, _| panic!()), None);
    assert_eq!(read_photo(7, 0, &mut out, |_, _, _, _| panic!()), None);
    assert_eq!(
        read_photo(7, 9, &mut out, |_, _, part, _| {
            if part == 0 {
                Some((CHUNK + 1, CHUNK))
            } else {
                None
            }
        }),
        None
    );
    assert_eq!(
        read_photo(7, 9, &mut out, |_, _, part, _| {
            if part == 0 {
                Some((CHUNK + 1, CHUNK))
            } else {
                Some((CHUNK + 2, 1))
            }
        }),
        None
    );
    assert_eq!(
        read_photo(7, 9, &mut out, |_, _, _, _| Some((CHUNK + 1, 1))),
        None
    );
    assert_eq!(
        read_photo(7, 9, &mut out, |_, _, _, _| Some((CHUNK * 2 + 1, CHUNK))),
        None
    );
}
