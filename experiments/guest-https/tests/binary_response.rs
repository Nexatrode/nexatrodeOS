use nexatrode_guest_https::binary_response::{BinaryResponse, RangeError};
use nexatrode_guest_https::PlaintextSink;

fn parse(fragments: &[&[u8]], limit: usize) -> Result<Vec<u8>, RangeError> {
    let mut reply = BinaryResponse::new(limit);
    for fragment in fragments {
        reply.append(fragment);
    }
    reply.finish()
}

#[test]
fn receives_exact_binary_content_across_all_split_points() {
    let data = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n\0\r\n\xffx";
    for n in 0..data.len() {
        assert_eq!(parse(&[&data[..n], &data[n..]], 5).unwrap(), b"\0\r\n\xffx");
    }
    let mut reply = BinaryResponse::new(5);
    reply.append(&data[..data.len() - 1]);
    assert!(!reply.complete());
}

#[test]
fn decodes_chunked_data_trailers_and_partial_delimiters() {
    let data = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n2;ext=x\r\n\0\xff\r\n3\r\n\r\na\r\n0\r\nX-End: yes\r\n\r\n";
    let mut reply = BinaryResponse::new(5);
    for b in data {
        reply.append(core::slice::from_ref(b));
    }
    assert!(reply.complete());
    assert_eq!(reply.finish().unwrap(), b"\0\xff\r\na");
}

#[test]
fn fails_closed_on_conflicts_oversize_truncation_and_extra_bytes() {
    for header in [
        "Content-Length: 2\r\nContent-Length: 2",
        "Transfer-Encoding: chunked\r\nContent-Length: 2",
        "Transfer-Encoding: gzip",
        "Content-Length: +2",
        "Content-Encoding: gzip",
        "Bogus",
        "Content-Length : 2",
        "Content-Length: 2\r\n x",
    ] {
        let response = format!("HTTP/1.1 200 OK\r\n{header}\r\n\r\nok");
        assert_eq!(
            parse(&[response.as_bytes()], 16),
            Err(RangeError::Framing),
            "{header}"
        );
    }
    assert_eq!(
        parse(&[b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n"], 4),
        Err(RangeError::TooLarge)
    );
    assert_eq!(
        parse(&[b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok!"], 4),
        Err(RangeError::Framing)
    );
    assert_eq!(
        parse(
            &[b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nok!\r\n3\r\n"],
            4
        ),
        Err(RangeError::TooLarge)
    );
    assert_eq!(
        parse(
            &[b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab!!"],
            4
        ),
        Err(RangeError::Framing)
    );
    assert_eq!(
        parse(&[b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nxy"], 4),
        Err(RangeError::Truncated)
    );
}

#[test]
fn non_success_completes_at_authenticated_headers_and_is_typed() {
    let mut reply = BinaryResponse::new(4);
    reply.append(b"HTTP/1.1 429 slow\r\nRetry-After: 12\r\n");
    assert!(!reply.complete());
    reply.append(b"\r\n<body>");
    assert!(reply.complete());
    assert_eq!(reply.finish(), Err(RangeError::Http(429)));
    assert!(RangeError::Http(429).rate_limited());
    assert!(RangeError::Http(503).transient());
    assert!(RangeError::Http(404).unsupported());
    assert!(!RangeError::Http(403).transient());
}

#[test]
fn informational_headers_do_not_end_response_or_count_as_body() {
    let mut reply = BinaryResponse::new(4);
    reply.append(b"HTTP/1.1 103 Early Hints\r\nLink: </a>\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nab");
    assert!(!reply.complete());
    assert_eq!(reply.body_progress(), 2);
    reply.append(b"cd");
    assert_eq!(reply.body_progress(), 4);
    assert_eq!(reply.finish().unwrap(), b"abcd");
}

#[test]
fn rejects_oversize_headers_trailers_and_chunk_lines() {
    let repeated = vec![b'x'; 8193];
    assert_eq!(
        parse(&[b"HTTP/1.1 200 OK\r\nX-Z: ", &repeated], 4),
        Err(RangeError::Framing)
    );
    let begin = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
    assert_eq!(
        parse(&[begin, b"1;", &repeated], 4),
        Err(RangeError::Framing)
    );
    assert_eq!(
        parse(&[begin, b"0\r\nX-T: ", &repeated], 4),
        Err(RangeError::Framing)
    );
}

#[test]
fn only_fully_framed_persistent_http11_replies_can_reuse_tls() {
    for (head, tail, reuse) in [
        ("HTTP/1.1 200 OK", "Content-Length: 2\r\n\r\nok", true),
        ("HTTP/1.1 200 OK", "Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n", true),
        ("HTTP/1.1 200 OK", "Connection: keep-alive, CLOSE\r\nContent-Length: 0\r\n\r\n", false),
        ("HTTP/1.0 200 OK", "Content-Length: 0\r\n\r\n", false),
        ("HTTP/1.1 200 OK", "Content-Length: 2\r\n\r\no", false),
        ("HTTP/1.1 200 OK", "Content-Length: 2\r\n\r\nok!", false),
        ("HTTP/1.1 429 Slow Down", "Content-Length: 0\r\n\r\n", false),
    ] {
        let mut reply = BinaryResponse::new(32);
        reply.append(format!("{head}\r\n{tail}").as_bytes());
        assert_eq!(reply.can_reuse(), reuse, "{head} {tail}");
    }
}

#[test]
fn preflight_accepts_only_one_persistent_framed_reply_for_handoff() {
    use nexatrode_guest_https::binary_response::BufferedRpcResponse;
    let message = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n";
    for n in 1..message.len() {
        let mut response = BufferedRpcResponse::new(16);
        assert!(!response.complete(&message[..n], |_| true));
        assert!(response.complete(message, |_| false));
        assert_eq!(response.finish().unwrap(), (b"{}".to_vec(), true));
    }
    for invalid in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}x".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
        b"HTTP/1.1 200 OK\r\nContent-Length: 513\r\n\r\n{}",
        b"HTTP/1.1 503 Busy\r\n\r\n",
    ] {
        let mut response = BufferedRpcResponse::new(512);
        assert!(response.complete(invalid, |_| true));
        assert!(response.finish().is_err());
    }
    let mut reply = BufferedRpcResponse::new(16);
    assert!(reply.complete(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\n{}", |_| false));
    assert_eq!(reply.finish().unwrap(), (b"{}".to_vec(), false));
    let mut old = BufferedRpcResponse::new(16);
    assert!(old.complete(b"HTTP/1.0 200 OK\r\n\r\n{}", |_| true));
    assert!(old.finish().is_err());
}
