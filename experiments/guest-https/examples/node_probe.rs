//! Host-only validation probe. No wallet data is sent.
use nexatrode_guest_https::{request, PollTransport, Progress, Session, Transport};
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Socket(TcpStream);
impl Transport for Socket {
    type Error = std::io::Error;
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        // Match the maximum payload of the guest's TCP IPC page.
        for chunk in bytes.chunks(128) {
            self.0.write_all(chunk)?;
        }
        Ok(())
    }
    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Self::Error> {
        let count = bytes.len().min(128);
        self.0.read(&mut bytes[..count])
    }
}

struct PollSocket {
    socket: Socket,
    pause: bool,
}
impl PollTransport for PollSocket {
    type Error = std::io::Error;
    fn try_write(&mut self, bytes: &[u8]) -> Result<Option<usize>, Self::Error> {
        self.pause = !self.pause;
        if self.pause {
            return Ok(None);
        }
        let n = bytes.len().min(128);
        self.socket.0.write(&bytes[..n]).map(Some)
    }
    fn try_read(&mut self, bytes: &mut [u8]) -> Result<Option<usize>, Self::Error> {
        self.pause = !self.pause;
        if self.pause {
            return Ok(None);
        }
        self.socket.read(bytes).map(Some)
    }
}

fn complete(bytes: &[u8]) -> bool {
    let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let Ok(headers) = std::str::from_utf8(&bytes[..end]) else {
        return false;
    };
    let size = headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
    size.is_some_and(|size| bytes.len() - end - 4 >= size)
}

fn call(
    name: &str,
    now: u64,
    limit: usize,
) -> Result<Vec<u8>, nexatrode_guest_https::Error<std::io::Error>> {
    let socket = TcpStream::connect(("rpc.nexawal.com", 443)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    request(
        &mut Socket(socket),
        name,
        now,
        b"GET /get_info HTTP/1.1\r\nHost: rpc.nexawal.com\r\nConnection: close\r\n\r\n",
        limit,
        complete,
    )
}

fn poll_call(
    name: &str,
    now: u64,
    limit: usize,
) -> Result<Vec<u8>, nexatrode_guest_https::Error<std::io::Error>> {
    let socket = TcpStream::connect(("rpc.nexawal.com", 443)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    let mut io = PollSocket {
        socket: Socket(socket),
        pause: false,
    };
    let mut session = Session::new(
        name,
        now,
        b"GET /get_info HTTP/1.1\r\nHost: rpc.nexawal.com\r\nConnection: close\r\n\r\n",
        limit,
    )
    .unwrap();
    for _ in 0..100_000 {
        match session.poll(&mut io, complete)? {
            Progress::Pending => (),
            Progress::Complete(response) => return Ok(response),
        }
    }
    Err(nexatrode_guest_https::Error::IterationLimit)
}

fn main() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let response = call("rpc.nexawal.com", now, 32768).expect("validated node request failed");
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert!(response
        .windows(b"\"nettype\"".len())
        .any(|w| w == b"\"nettype\""));
    assert!(
        matches!(
            call("wrong.invalid", now, 32768),
            Err(nexatrode_guest_https::Error::Tls(_))
        ),
        "wrong hostname must fail TLS validation"
    );
    assert!(
        matches!(
            call("rpc.nexawal.com", 1, 32768),
            Err(nexatrode_guest_https::Error::Tls(_))
        ),
        "a certificate outside its validity period must fail"
    );
    assert!(
        matches!(
            call("rpc.nexawal.com", now, 16),
            Err(nexatrode_guest_https::Error::BufferLimit)
        ),
        "responses beyond the caller's limit must fail"
    );
    assert!(poll_call("rpc.nexawal.com", now, 32768)
        .unwrap()
        .starts_with(b"HTTP/1.1 200"));
    assert!(matches!(
        poll_call("wrong.invalid", now, 32768),
        Err(nexatrode_guest_https::Error::Tls(_))
    ));
    assert!(matches!(
        poll_call("rpc.nexawal.com", 1, 32768),
        Err(nexatrode_guest_https::Error::Tls(_))
    ));
    assert!(matches!(
        poll_call("rpc.nexawal.com", now, 16),
        Err(nexatrode_guest_https::Error::BufferLimit)
    ));
    println!("HTTPS_NODE_HOSTNAME_TIME_LIMIT_AND_POLL_PASS");
}
