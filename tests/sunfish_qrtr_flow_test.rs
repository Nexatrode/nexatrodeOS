#[path = "../rust/src/qcom_qrtr_flow.rs"]
mod flow;
use flow::{Destination, Error, Table};

const A: Destination = Destination { node: 0, port: 18 };
const B: Destination = Destination { node: 0, port: 19 };

#[test]
fn linux_watermarks_and_resume_are_per_destination() {
    let mut table = Table::<8>::new(1).unwrap();
    assert!(!table.contains(A));
    for count in 1..=10 {
        let sent = table.reserve(A).unwrap();
        assert!(table.contains(A));
        assert_eq!(sent.destination(), A);
        assert_eq!(sent.sequence(), count);
        assert_eq!(sent.confirm_rx(), count == 5);
        table.finish_after_glink(&sent).unwrap();
    }
    assert_eq!(table.reserve(A), Err(Error::Blocked));
    let other = table.reserve(B).unwrap();
    assert!(!other.confirm_rx());
    table.finish_after_glink(&other).unwrap();
    assert!(table.resume(A).unwrap());
    let sent = table.reserve(A).unwrap();
    assert!(!sent.confirm_rx());
}

#[test]
fn proven_unpublished_confirmation_is_requested_again() {
    let mut table = Table::<8>::new(1).unwrap();
    for _ in 0..4 {
        let sent = table.reserve(A).unwrap();
        table.finish_after_glink(&sent).unwrap();
    }
    let confirmation = table.reserve(A).unwrap();
    assert!(confirmation.confirm_rx());
    table.fail_unpublished(&confirmation).unwrap();
    assert_eq!(table.fail_unpublished(&confirmation), Err(Error::Stale));
    let retry = table.reserve(A).unwrap();
    assert!(retry.confirm_rx());
    table.finish_after_glink(&retry).unwrap();
    assert!(!table.reserve(A).unwrap().confirm_rx());
}

#[test]
fn partial_or_uncertain_publication_quarantines_without_replay() {
    let mut table = Table::<4>::new(1).unwrap();
    let pending = table.reserve(A).unwrap();
    table.fail_uncertain(&pending).unwrap();
    assert_eq!(table.reserve(A), Err(Error::Quarantined));
    assert_eq!(table.resume(A), Err(Error::Quarantined));
    assert_eq!(table.finish_after_glink(&pending), Err(Error::Quarantined));
}

#[test]
fn closed_port_cannot_receive_stale_credit_or_be_reused() {
    let mut table = Table::<4>::new(1).unwrap();
    let pending = table.reserve(A).unwrap();
    assert_eq!(table.close(A), Err(Error::Blocked));
    table.finish_after_glink(&pending).unwrap();
    table.close(A).unwrap();
    assert!(!table.resume(A).unwrap());
    assert_eq!(table.reserve(A), Err(Error::Closed));
    table.close(B).unwrap();
    assert_eq!(table.reserve(B), Err(Error::Closed));
}

#[test]
fn tokens_are_bound_to_their_owner_and_generation() {
    let mut first = Table::<4>::new(1).unwrap();
    let mut second = Table::<4>::new(1).unwrap();
    let mut third = Table::<4>::new(2).unwrap();
    let token = first.reserve(A).unwrap();
    second.reserve(A).unwrap();
    third.reserve(A).unwrap();
    assert_eq!(second.finish_after_glink(&token), Err(Error::Stale));
    assert_eq!(third.finish_after_glink(&token), Err(Error::Stale));
    first.finish_after_glink(&token).unwrap();
    assert_eq!(first.finish_after_glink(&token), Err(Error::Stale));
}

#[test]
fn bounded_capacity_and_unmatched_resume_fail_closed() {
    let mut table = Table::<1>::new(1).unwrap();
    assert!(!table.resume(B).unwrap());
    let token = table.reserve(A).unwrap();
    assert_eq!(table.reserve(B), Err(Error::Capacity));
    table.finish_after_glink(&token).unwrap();
    assert_eq!(table.reserve(B), Err(Error::Capacity));
    assert!(matches!(Table::<1>::new(0), Err(Error::InvalidGeneration)));
}

#[test]
fn resume_does_not_complete_or_release_an_outstanding_glink_send() {
    let mut table = Table::<2>::new(3).unwrap();
    let pending = table.reserve(A).unwrap();
    assert_eq!(table.usage(), (1, 1));
    assert!(table.resume(A).unwrap());
    assert_eq!(table.close(A), Err(Error::Blocked));
    table.finish_after_glink(&pending).unwrap();
    assert_eq!(table.usage(), (1, 0));
    table.close(A).unwrap();
}

#[test]
fn invalid_destinations_never_consume_the_bounded_table() {
    let mut table = Table::<1>::new(1).unwrap();
    for destination in [Destination { node: u32::MAX, port: 1 },
        Destination { node: 0, port: 0 }, Destination { node: 0, port: 0xffff_fffe }] {
        assert_eq!(table.reserve(destination), Err(Error::InvalidDestination));
        assert_eq!(table.close(destination), Err(Error::InvalidDestination));
    }
    assert!(table.reserve(A).is_ok());
}

#[test]
fn first_storage_reply_to_each_observed_peer_does_not_request_resume() {
    let mut table = Table::<16>::new(1).unwrap();
    for port in 10..=19 {
        let token = table.reserve(Destination { node: 0, port }).unwrap();
        assert!(!token.confirm_rx());
        table.finish_after_glink(&token).unwrap();
    }
}

#[test]
fn legacy_sender_shares_destination_counter_with_reference_sends() {
    let mut table = Table::<8>::new(1).unwrap();
    for _ in 0..4 {
        let token = table.reserve(A).unwrap();
        assert!(!token.confirm_rx());
        table.finish_after_glink(&token).unwrap();
    }
    let legacy = table.reserve_with_legacy_confirmation(A, true).unwrap();
    assert!(legacy.confirm_rx());
    table.finish_after_glink(&legacy).unwrap();
    assert!(table.resume(A).unwrap());
    let next = table.reserve(A).unwrap();
    assert!(!next.confirm_rx());
}
