// Compile the production wallet range driver against a deterministic TCP slot.
extern crate alloc;
extern crate self as walletcore_core;
use nexatrode_guest_https::{FailureKind, PollTransport, StreamError};

pub const MAX_RANGE_BODY: usize = 2 * 1024 * 1024;

// This transport-only test never receives daemon plaintext. The decoder and
// partial-body transitions are exercised by walletcore's bulk_scan fixture.
#[derive(Debug, PartialEq, Eq)]
pub struct RangeBody(Vec<u8>);
impl RangeBody {
    pub fn new(_: u64, _: usize) -> Self {
        Self(Vec::new())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn reserve(&mut self, additional: usize) -> Result<(), ()> {
        self.0.try_reserve_exact(additional).map_err(|_| ())
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), ()> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct NodeEndpoint {
    pub port: u16,
}
impl NodeEndpoint {
    pub fn host(&self) -> &str {
        "rpc.nexawal.com"
    }
    pub fn authority(&self) -> &str {
        "rpc.nexawal.com:443"
    }
}

mod wallet_tcp {
    use super::*;
    #[derive(Default)]
    pub struct WalletTcp {
        pub resets: usize,
        pub releases: usize,
        pub connects: usize,
        pub writes: usize,
        pub reads: usize,
        pub connected: bool,
        pub refuse_reset: bool,
        pub refuse_open: bool,
    }
    impl WalletTcp {
        pub fn reset(&mut self) -> Result<(), ()> {
            self.resets += 1;
            if self.refuse_reset {
                Err(())
            } else {
                Ok(())
            }
        }
        pub fn open_fresh(&mut self, _: [u8; 4], _: u16) -> Result<(), ()> {
            self.connects += 1;
            if self.refuse_open {
                Err(())
            } else {
                Ok(())
            }
        }
        pub fn with_range_rx<T>(&mut self, poll: impl FnOnce(&mut Self) -> T) -> T { poll(self) }
        pub fn poll_connected(&mut self) -> Result<bool, ()> {
            Ok(self.connected)
        }
        pub fn release(&mut self) -> Result<(), ()> {
            self.releases += 1;
            Ok(())
        }
    }
    impl PollTransport for WalletTcp {
        type Error = StreamError;
        fn try_write(&mut self, _: &[u8]) -> Result<Option<usize>, Self::Error> {
            self.writes += 1;
            Ok(Some(1))
        }
        fn try_read(&mut self, _: &mut [u8]) -> Result<Option<usize>, Self::Error> {
            self.reads += 1;
            Ok(None)
        }
    }
}

#[allow(unexpected_cfgs, reason = "wallet-range-profile is a feature of the seL4 wallet crate")]
#[path = "../../../rust/src/wallet_range_fetch.rs"]
mod driver;
use driver::{RangeError, RangeFetch};
use wallet_tcp::WalletTcp;
fn start(tcp: &mut WalletTcp) -> RangeFetch {
    RangeFetch::start(
        tcp,
        NodeEndpoint { port: 443 },
        [127, 0, 0, 1],
        12,
        8,
        b"test",
        Some(1_800_000_000),
        100,
        &mut None,
    )
    .unwrap()
}

#[test]
fn unfinished_preflight_session_opens_a_fresh_connection() {
    let peer = NodeEndpoint { port: 443 };
    let ip = [127, 0, 0, 1];
    let time = 1_800_000_000;
    let session = nexatrode_guest_https::Session::new(peer.host(), time, b"metadata", 1024).unwrap();
    let mut idle = Some(driver::RangeConnection::from_preflight(session, peer, ip));
    let mut tcp = WalletTcp::default();
    let fetch = RangeFetch::start(&mut tcp, peer, ip, 12, 8, b"range", Some(time), 100, &mut idle).unwrap();
    assert!(!fetch.reused && idle.is_none());
    assert_eq!((tcp.resets, tcp.connects), (1, 1));
}

#[test]
fn priming_leaves_a_fresh_handshake_for_ordinary_polling() {
    let mut tcp = WalletTcp { connected: true, ..Default::default() };
    let mut fetch = start(&mut tcp);
    assert!(!fetch.reused);
    for now in 101..105 {
        assert_eq!(fetch.prime_request(&mut tcp, now), Ok(false));
    }
    assert_eq!((tcp.reads, tcp.writes, fetch.body_progress()), (0, 0, 0));
    for now in 105..110 {
        fetch.step(&mut tcp, now, &mut None).unwrap();
    }
    assert!(tcp.writes > 0);
}

#[test]
fn timeout_cancels_exactly_once_and_retains_original_range() {
    let mut tcp = WalletTcp::default();
    let mut f = start(&mut tcp);
    assert_eq!((f.start_height, f.count), (12, 8));
    assert_eq!(f.partial_body().unwrap().len(), 0);
    assert_eq!(f.step(&mut tcp, 101, &mut None), Ok(None));
    assert_eq!(f.step(&mut tcp, 30_100, &mut None), Err(RangeError::Timeout));
    assert_eq!(tcp.resets, 2);
    f.cancel(&mut tcp);
    assert_eq!(f.step(&mut tcp, 30_101, &mut None), Err(RangeError::Inactive));
    assert!(f.partial_body().is_none());
    assert_eq!(tcp.resets, 2);
}

#[test]
fn clock_and_node_are_validated_before_touching_slot() {
    let mut tcp = WalletTcp::default();
    let err = RangeFetch::start(
        &mut tcp,
        NodeEndpoint { port: 443 },
        [1; 4],
        2,
        4,
        b"req",
        None,
        100,
        &mut None,
    )
    .err();
    assert_eq!(err, Some(RangeError::Tls(FailureKind::Clock)));
    assert!(RangeFetch::start(
        &mut tcp,
        NodeEndpoint { port: 80 },
        [1; 4],
        2,
        4,
        b"req",
        Some(1_800_000_000),
        100,
        &mut None,
    )
    .is_err());
    assert_eq!(tcp.resets, 0);
    tcp.refuse_reset = true;
    assert_eq!(
        RangeFetch::start(
            &mut tcp,
            NodeEndpoint { port: 443 },
            [1; 4],
            2,
            4,
            b"req",
            Some(1_800_000_000),
            100,
            &mut None,
        )
        .err(),
        Some(RangeError::Connect)
    );
    assert_eq!(tcp.connects, 0);
}

#[test]
fn limits_tls_polls_per_call_and_stalls_even_if_parent_keeps_ticking() {
    let mut tcp = WalletTcp {
        connected: true,
        ..Default::default()
    };
    let mut f = start(&mut tcp);
    for ms in 101..105 {
        let before = tcp.writes + tcp.reads;
        assert_eq!(f.step(&mut tcp, ms, &mut None), Ok(None));
        assert!(tcp.writes + tcp.reads - before <= 64);
    }
    assert!(f.wire_progress() > 0);
    assert_eq!(f.body_progress(), 0);
    assert_eq!(f.step(&mut tcp, 150_100, &mut None), Err(RangeError::Timeout));
    assert_eq!(tcp.resets, 2);
}
