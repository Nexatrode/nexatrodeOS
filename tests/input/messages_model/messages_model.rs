#![allow(dead_code)]

// Path-include the shared codec + Messages model so `#[cfg(test)]` modules
// inside those files run under a host Cargo package with `aes-gcm-siv`.
#[path = "../../../rust/src/record_crypto.rs"]
mod record_crypto;
#[path = "../../../rust/src/message_seal.rs"]
mod message_seal;
#[path = "../../../rust/src/vault_crypto.rs"]
mod vault_crypto;
#[path = "../../../rust/src/vault_record.rs"]
mod vault_record;
#[path = "../../../rust/src/vault_blob_record.rs"]
mod vault_blob_record;
#[path = "../../../rust/src/messages.rs"]
mod messages;

#[path = "../../../rust/src/vault_history.rs"]
mod vault_history;

#[path = "../../../rust/src/simplex_ipc.rs"]
mod simplex_ipc;

#[test]
fn peer_text_survives_fragment_failure_retry_and_restart_as_one_literal_row() {
    use vault_history::{decode_event, store_peer_line, Event, MessageKey, PeerVersion};

    let key = MessageKey { source: 15, number: 47, part: 1 };
    let version = PeerVersion { key, shared_id: "abc=", revision: (47, 1), deleted: false };
    let text = ["界🍎".repeat(1000), "\r\n\x1eR\x1frender literally ✓".into()].concat();
    let mut line = "iPhone: 12:23\x1f".to_owned();
    messages::write_direct_text(&mut line, "abc=", &text).unwrap();

    let mut records: Vec<Vec<u8>> = Vec::new();
    let error = store_peer_line(version, &line, |event| {
        if records.len() == 1 { return Err(()); }
        records.push(event.to_vec());
        Ok(records.len() as u64)
    });
    assert!(error.is_err());
    assert!(records.iter().all(|bytes| matches!(decode_event(bytes), Some(Event::Fragment(_)))));
    store_peer_line(version, &line, |event| {
        records.push(event.to_vec());
        Ok(records.len() as u64)
    }).unwrap();

    let Some(Event::Peer(row)) = decode_event(records.last().unwrap()) else { panic!("missing row commit") };
    let mut restored = row.first.to_string();
    for id in row.fragment_ids() {
        let Some(Event::Fragment(tail)) = decode_event(&records[id as usize - 1]) else { panic!("missing fragment") };
        restored.push_str(tail);
    }
    let (_, stamped) = restored.split_once(": ").unwrap();
    let (id, encoded) = messages::split_direct_text(messages::split_message_time(stamped).1).unwrap();
    let mut body = String::new();
    messages::decode_direct_text(encoded, &mut body).unwrap();
    assert_eq!(body, text);
    assert_eq!(id, "abc=");
    assert_eq!(row.version.key, key);
    let edit = MessageKey { number: 48, ..key };
    assert!(row.version.accepts(edit, id));
    assert!(!row.version.accepts(MessageKey { source: 16, ..edit }, id));
    assert!(!row.version.accepts(edit, "unrelated"));
    assert!(!row.version.accepts(key, id));
    let edited = PeerVersion { revision: (48, 1), ..row.version };
    assert!(!edited.accepts(edit, id));
    assert!(edited.accepts(MessageKey { part: 2, ..edit }, id));
    let deleted = PeerVersion { deleted: true, ..edited };
    assert!(!deleted.accepts(MessageKey { number: 49, ..edit }, id));
}

#[test]
fn direct_first_page_preserves_target_and_handles_utf8_split_across_pages() {
    let text = format!("{}🍎{}", "a".repeat(2999), "β".repeat(2000));
    let mut frame = vec![8];
    frame.extend_from_slice(&7_u64.to_le_bytes());
    frame.extend_from_slice(&121_u64.to_le_bytes());
    frame.extend_from_slice(&2_u16.to_le_bytes());
    frame.extend_from_slice(b"\x04\0ab=/");
    frame.extend_from_slice(&(text.len() as u32).to_le_bytes());
    frame.extend_from_slice(&3000_u16.to_le_bytes());
    frame.extend_from_slice(&text.as_bytes()[..3000]);

    let decoded = simplex_ipc::DirectFrame::parse(&frame, vault_history::DIRECT_TEXT_LIMIT).unwrap();
    assert_eq!((decoded.kind, decoded.handle, decoded.message, decoded.part, decoded.id), (8, 7, 121, 2, "ab=/"));
    assert_eq!(decoded.total, text.len());
    assert!(std::str::from_utf8(decoded.first).is_err());
    let restored = [decoded.first, &text.as_bytes()[3000..]].concat();
    assert_eq!(std::str::from_utf8(&restored).unwrap(), text);
    assert!(simplex_ipc::DirectFrame::parse(&frame[..frame.len() - 1], vault_history::DIRECT_TEXT_LIMIT).is_none());
}
