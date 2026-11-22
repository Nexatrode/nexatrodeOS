#[path = "../src/chat_photo_loader.rs"]
mod chat_photo_loader;
use chat_photo_loader::{PHOTO_CHUNK, PHOTO_MAX, PhotoLoader, PhotoOutcome};

#[test]
fn a_claimed_photo_can_be_requested_again_after_leaving_and_returning() {
    let mut loader = PhotoLoader::default();
    loader.reset(1, 7, &[(4, 99)]);
    loader.set_visible(4, true);
    assert_eq!(loader.claim_visible(), Some((7, 99, 4)));
    assert_eq!(loader.claim_visible(), None);
    loader.set_visible(4, false);
    loader.set_visible(4, true);
    assert_eq!(loader.claim_visible(), Some((7, 99, 4)));
}

#[test]
fn completing_an_inflight_photo_clears_a_duplicate_return_request() {
    let mut loader = PhotoLoader::default();
    loader.reset(1, 7, &[(4, 99)]);
    loader.set_visible(4, true);
    assert_eq!(loader.claim_visible(), Some((7, 99, 4)));
    loader.set_visible(4, true); // row returned while the worker was busy
    loader.complete(4);
    assert_eq!(loader.claim_visible(), None);
    loader.set_visible(4, true);
    assert_eq!(loader.claim_visible(), Some((7, 99, 4)));
}

#[test]
fn large_photo_is_loaded_one_bounded_chunk_at_a_time() {
    let mut loader = PhotoLoader::default();
    loader.reset(7, 42, &[(3, 90)]);
    assert!(loader.matches(7, 42));
    assert!(!loader.matches(7, 43));
    loader.set_visible(3, true);
    assert_eq!(loader.next_request(), Some((42, 90, 0)));
    let first = vec![7u8; PHOTO_CHUNK];
    assert_eq!(
        loader.accept(Some((PHOTO_CHUNK + 2, &first))),
        PhotoOutcome::Pending
    );
    assert_eq!(loader.next_request(), Some((42, 90, 1)));
    match loader.accept(Some((PHOTO_CHUNK + 2, &[8, 9]))) {
        PhotoOutcome::Complete { row, bytes } => {
            assert_eq!(row, 3);
            assert_eq!(bytes.len(), PHOTO_CHUNK + 2);
            assert_eq!(&bytes[PHOTO_CHUNK..], &[8, 9]);
        }
        other => panic!("unexpected result: {other:?}"),
    }
    assert_eq!(loader.next_request(), None);
}

#[test]
fn switching_chat_discards_the_old_photo_and_skips_missing_preview_ids() {
    let mut loader = PhotoLoader::default();
    loader.reset(1, 7, &[(0, 10)]);
    loader.set_visible(0, true);
    assert_eq!(
        loader.accept(Some((PHOTO_CHUNK + 1, &vec![1; PHOTO_CHUNK]))),
        PhotoOutcome::Pending
    );
    loader.reset(2, 8, &[(5, 0), (6, 12)]);
    loader.set_visible(6, true);
    assert_eq!(loader.next_request(), Some((8, 12, 0)));
    assert_eq!(
        loader.accept(Some((1, &[4]))),
        PhotoOutcome::Complete {
            row: 6,
            bytes: vec![4]
        }
    );
    assert_eq!(loader.next_request(), None);
}

#[test]
fn bad_and_missing_chunks_fail_only_their_own_photo() {
    let mut loader = PhotoLoader::default();
    loader.reset(1, 3, &[(0, 11), (1, 12), (2, 13)]);
    for row in 0..3 {
        loader.set_visible(row, true);
    }
    assert_eq!(loader.accept(None), PhotoOutcome::Failed { row: 0 });
    assert_eq!(
        loader.accept(Some((PHOTO_MAX + 1, &[1]))),
        PhotoOutcome::Failed { row: 1 }
    );
    assert_eq!(
        loader.accept(Some((2, &[1]))),
        PhotoOutcome::Failed { row: 2 }
    );
    assert_eq!(loader.next_request(), None);
}

#[test]
fn changing_total_mid_photo_is_rejected() {
    let mut loader = PhotoLoader::default();
    loader.reset(1, 3, &[(4, 11)]);
    loader.set_visible(4, true);
    assert_eq!(
        loader.accept(Some((PHOTO_CHUNK + 1, &vec![1; PHOTO_CHUNK]))),
        PhotoOutcome::Pending
    );
    assert_eq!(
        loader.accept(Some((PHOTO_CHUNK + 2, &[9, 9]))),
        PhotoOutcome::Failed { row: 4 }
    );
}

#[test]
fn only_visible_photos_are_requested_and_offscreen_reads_restart() {
    let mut loader = PhotoLoader::default();
    loader.reset(3, 9, &[(0, 101), (1, 102)]);
    assert_eq!(loader.next_request(), None);
    loader.set_visible(1, true);
    assert_eq!(loader.next_request(), Some((9, 102, 0)));
    assert_eq!(
        loader.accept(Some((PHOTO_CHUNK + 1, &vec![3; PHOTO_CHUNK]))),
        PhotoOutcome::Pending
    );
    loader.set_visible(1, false);
    loader.set_visible(0, true);
    assert_eq!(loader.next_request(), Some((9, 101, 0)));
    loader.set_visible(0, false);
    assert_eq!(loader.next_request(), None);
    loader.set_visible(1, true);
    assert_eq!(loader.next_request(), Some((9, 102, 0)));
    assert_eq!(
        loader.accept(Some((PHOTO_CHUNK + 1, &vec![3; PHOTO_CHUNK]))),
        PhotoOutcome::Pending
    );
    assert_eq!(
        loader.accept(Some((PHOTO_CHUNK + 1, &[4]))),
        PhotoOutcome::Complete {
            row: 1,
            bytes: [vec![3; PHOTO_CHUNK], vec![4]].concat()
        }
    );
}

#[test]
fn background_reader_claims_only_visible_job_once() {
    let mut loader = PhotoLoader::default();
    loader.reset(9, 4, &[(0, 33), (1, 44)]);
    assert_eq!(loader.claim_visible(), None);
    loader.set_visible(1, true);
    assert_eq!(loader.claim_visible(), Some((4, 44, 1)));
    assert_eq!(loader.claim_visible(), None);
    loader.set_visible(0, true);
    assert_eq!(loader.claim_visible(), Some((4, 33, 0)));
}
