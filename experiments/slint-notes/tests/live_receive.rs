#[path = "../src/live_receive.rs"]
mod live_receive;

#[test]
fn receive_result_keeps_stable_owner_and_body() {
    let mut bytes = [0; 4096];
    let len = live_receive::encode(&mut bytes, 2, 91, "hello").unwrap();
    let event = live_receive::decode(&bytes[..len]).unwrap();
    assert_eq!((event.status, event.owner, event.text), (2, 91, "hello"));
    assert_eq!(live_receive::chat_for_owner(&[7, 91, 8], event.owner), Some(1));
    assert_eq!(live_receive::chat_for_owner(&[91, 7, 8], event.owner), Some(0));
    assert_eq!(live_receive::chat_for_owner(&[7, 8], event.owner), None);
}

#[test]
fn malformed_or_unowned_message_is_rejected() {
    let mut bytes = [0; 4096];
    assert!(live_receive::encode(&mut bytes, 2, 0, "hello").is_none());
    assert!(live_receive::encode(&mut bytes, 2, 1, &"x".repeat(2049)).is_none());
    let len = live_receive::encode(&mut bytes, 2, 1, "hello").unwrap();
    assert!(live_receive::decode(&bytes[..len - 1]).is_none());
    bytes[len - 1] = 255;
    assert!(live_receive::decode(&bytes[..len]).is_none());
}
