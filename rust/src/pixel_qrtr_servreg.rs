//! One local SERVREG announcement, exact GET_DOMAIN_LIST responses, and one
//! bounded follow-up query. The probe never handles PFR, unrelated QMI
//! messages, any query beyond that one follow-up, WLAN, host DMA, intent
//! reuse, or retries.

use crate::{
    pixel_glink_control,
    pixel_glink_prepare::{DESCRIPTOR, PART, TX},
    pixel_glink_receive::{self as receive, FIFO_BYTES},
    pixel_glink_rx::Buffer,
    pixel_modem_status::{Clock, Window, sync},
    pixel_qrtr_discovery::{self as discovery, MAX_TX},
    pixel_qrtr_hello,
    pixel_qrtr_next::{self, DiscoveryCheckpoint},
    pixel_qrtr_prequery as prequery, pixel_qrtr_snapshot, pixel_servreg_wire as wire,
    pixel_smp2p_handshake::ReadyObservation,
};
use personal_os_rust_server::qcom_modem_status::{Error, Read};

const LOCAL_BUFFER_ID: u32 = 3;
const ANNOUNCE_REMOTE_INTENT: u32 = 1;
const REPLY_REMOTE_INTENT: u32 = 2;
#[cfg(not(feature = "sunfish-qrtr-servreg-prequery-check"))]
const LOCAL_QUERY_INTENT_ID: u32 = LOCAL_BUFFER_ID + 1;
const PREQUERY_BYTES: usize = prequery::PREQUERY_BYTES;
const WAIT_MS: u64 = 45_000;
const QUERY_GLINK_BYTES: usize = if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
    80
} else {
    pixel_qrtr_snapshot::FRAME_PREFIX_BYTES
};
const QUERY_DATAGRAM_BYTES: usize = if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
    wire::PDR_QUERY_BYTES
} else {
    wire::QUERY_BYTES
};
const QUERY_CHUNK_BYTES: usize = if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
    60
} else {
    wire::QUERY_BYTES
};
const RESPONSE_GLINK_BYTES: usize = wire::RESPONSE_GLINK_BYTES;
const RX_DUMP_BYTES: usize = pixel_qrtr_snapshot::SNAPSHOT_BYTES;
const RX_DUMP_WORDS: usize = RX_DUMP_BYTES / 4;
const RX_SETTLE_POLLS: usize = 100;
const RX_SETTLE_QUIET_POLLS: usize = 10;
const POST_REPLY_WINDOW_MS: u64 = 10_000;
const POST_REPLY_POLL_LIMIT: usize = 1_200;
const POST_REPLY_CAPTURE_BYTES: usize = 64;
const POST_REPLY_WORDS: usize = POST_REPLY_CAPTURE_BYTES / 4;
const FOLLOWUP_INTENT_ID: u32 = 5;
const FOLLOWUP_INTENT_BYTES: usize = 64;
const FOLLOWUP_QUERY_GLINK_BYTES: usize = 16 + FOLLOWUP_INTENT_BYTES;
const FOLLOWUP_CAPTURE_BYTES: usize = 96;
const FOLLOWUP_CAPTURE_WORDS: usize = FOLLOWUP_CAPTURE_BYTES / 4;
const FOLLOWUP_WAIT_MS: u64 = 10_000;
const FOLLOWUP_POLL_LIMIT: usize = 1_000;

/// Private receive copy retained through restart after the one fresh intent.
#[repr(C)]
struct FollowupBuffer {
    before: [u8; 16],
    data: [u8; 80],
    after: [u8; 16],
}

impl FollowupBuffer {
    const fn new() -> Self {
        Self {
            before: [0xa5; 16],
            data: [0; 80],
            after: [0x5a; 16],
        }
    }

    fn guards(&self) -> bool {
        (0..16).all(|i| unsafe {
            self.before.as_ptr().add(i).read_volatile() == 0xa5
                && self.after.as_ptr().add(i).read_volatile() == 0x5a
        })
    }

    fn copy_exact(&mut self, bytes: &[u8]) -> bool {
        if bytes.len() > self.data.len() {
            return false;
        }
        for i in 0..self.data.len() {
            let value = bytes.get(i).copied().unwrap_or(0);
            unsafe { self.data.as_mut_ptr().add(i).write_volatile(value) };
        }
        true
    }

    fn verified_exact(&self, bytes: &[u8]) -> bool {
        bytes.len() <= self.data.len()
            && self.guards()
            && (0..self.data.len()).all(|i| unsafe {
                self.data.as_ptr().add(i).read_volatile() == bytes.get(i).copied().unwrap_or(0)
            })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Report {
    pub error: Option<Error>,
    pub stage: u32,
    pub announced: bool,
    pub request_seen: bool,
    pub response_staged: bool,
    pub response_consumed: bool,
    pub guards: bool,
    pub byte_writes: u32,
    pub notifications: u32,
    pub tx_words: [u32; 4],
    pub rx_tail: u32,
    pub request_port: u32,
    pub transaction: u32,
    pub rx_available: u32,
    pub rx_head: u32,
    pub rx_captured: u32,
    pub rx_prefix_bytes: u32,
    pub rx_suffix_bytes: u32,
    pub rx_prequery_shape: bool,
    pub rx_prequery_error_code: u32,
    pub prequery_check_pass: bool,
    pub query_check_pass: bool,
    pub prequery_handled: bool,
    pub server_service: u32,
    pub server_instance: u32,
    pub server_node: u32,
    pub server_port: u32,
    pub returned_intent: u32,
    pub requested_intent_size: u32,
    pub query_intent_id: u32,
    pub rx_words: [u32; RX_DUMP_WORDS],
    pub post_rx_available: u32,
    pub post_rx_head: u32,
    pub post_rx_captured: u32,
    pub post_rx_words: [u32; POST_REPLY_WORDS],
    pub followup_controls_consumed: bool,
    pub followup_intent_offered: bool,
    pub followup_tx_consumed: bool,
    pub followup_packet_valid: bool,
    pub followup_control_valid: bool,
    pub followup_buffer_valid: bool,
    pub followup_tx_cursor_valid: bool,
    pub followup_tx_len: u32,
    pub followup_tx_head: u32,
    pub followup_chunk_bytes: u32,
    pub followup_left_bytes: u32,
    pub followup_words: [u32; FOLLOWUP_CAPTURE_WORDS],
    pub followup_query_valid: bool,
    pub followup_response_staged: bool,
    pub followup_response_consumed: bool,
    pub followup_done_consumed: bool,
    pub followup_done_intent: u32,
    pub followup_transaction: u32,
    pub followup_request_port: u32,
    pub elapsed_ms: u32,
}

impl Report {
    fn new() -> Self {
        Self {
            error: None,
            stage: 0,
            announced: false,
            request_seen: false,
            response_staged: false,
            response_consumed: false,
            guards: false,
            byte_writes: 0,
            notifications: 0,
            tx_words: [0; 4],
            rx_tail: 0,
            request_port: 0,
            transaction: 0,
            rx_available: 0,
            rx_head: 0,
            rx_captured: 0,
            rx_prefix_bytes: 0,
            rx_suffix_bytes: 0,
            rx_prequery_shape: false,
            rx_prequery_error_code: 0,
            prequery_check_pass: false,
            query_check_pass: false,
            prequery_handled: false,
            server_service: 0,
            server_instance: 0,
            server_node: 0,
            server_port: 0,
            returned_intent: 0,
            requested_intent_size: 0,
            query_intent_id: LOCAL_BUFFER_ID,
            rx_words: [0; RX_DUMP_WORDS],
            post_rx_available: 0,
            post_rx_head: 0,
            post_rx_captured: 0,
            post_rx_words: [0; POST_REPLY_WORDS],
            followup_controls_consumed: false,
            followup_intent_offered: false,
            followup_tx_consumed: false,
            followup_packet_valid: false,
            followup_control_valid: false,
            followup_buffer_valid: false,
            followup_tx_cursor_valid: false,
            followup_tx_len: 0,
            followup_tx_head: 0,
            followup_chunk_bytes: 0,
            followup_left_bytes: 0,
            followup_words: [0; FOLLOWUP_CAPTURE_WORDS],
            followup_query_valid: false,
            followup_response_staged: false,
            followup_response_consumed: false,
            followup_done_consumed: false,
            followup_done_intent: 0,
            followup_transaction: 0,
            followup_request_port: 0,
            elapsed_ms: 0,
        }
    }

    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.announced
            && self.request_seen
            && self.response_staged
            && self.response_consumed
            && self.guards
            && (!cfg!(feature = "sunfish-qrtr-servreg-pdr-followup")
                || (self.followup_packet_valid
                    && self.followup_query_valid
                    && self.followup_response_staged
                    && self.followup_response_consumed
                    && self.followup_done_consumed))
    }
}

fn tx_expected(m: &impl Read, expected: &[u8; MAX_TX], len: usize) -> Result<(), Error> {
    if len > MAX_TX || len > TX.size {
        return Err(Error::Bounds);
    }
    for i in 0..TX.size {
        let want = if i < len { expected[i] } else { 0 };
        if m.read8(TX.offset + i) != want {
            return Err(Error::Publication);
        }
    }
    Ok(())
}

fn state(
    m: &impl Read,
    base: &receive::Report,
    ready: &ReadyObservation,
    head: u32,
    tail: u32,
    last_tx: u32,
    last_rx: u32,
) -> Result<receive::Report, Error> {
    let s = receive::inspect_ref(m, ready)?;
    if s.inventory.items != base.inventory.items
        || s.inventory.directory.route != base.inventory.directory.route
    {
        return Err(Error::RouteChanged);
    }
    if s.words[1] != head
        || s.words[2] != tail
        || s.words[0] < last_tx
        || s.words[0] > head
        || s.words[3] < last_rx
        || s.words[3] as usize >= FIFO_BYTES
        || s.available as usize > FIFO_BYTES
        || s.words[4..] != [0; 4]
    {
        return Err(Error::Outbound);
    }
    Ok(s)
}

fn read_span(
    m: &impl Read,
    item: receive::Report,
    at: u32,
    n: usize,
) -> Result<[u8; RX_DUMP_BYTES], Error> {
    if n > RX_DUMP_BYTES || at as usize >= FIFO_BYTES || at as usize + n > 2 * FIFO_BYTES {
        return Err(Error::Bounds);
    }
    let rx = item.inventory.items[2].ok_or(Error::ItemSize)?;
    let mut one = [0; RX_DUMP_BYTES];
    let mut two = [0; RX_DUMP_BYTES];
    for i in 0..n {
        one[i] = m.read8(rx.offset + (at as usize + i) % FIFO_BYTES);
    }
    sync();
    for i in 0..n {
        two[i] = m.read8(rx.offset + (at as usize + i) % FIFO_BYTES);
    }
    if one != two {
        return Err(Error::Unstable);
    }
    Ok(one)
}

fn observe_u32(bytes: &[u8; RX_DUMP_BYTES], captured: usize, at: usize) -> Option<u32> {
    (at.checked_add(4)? <= captured)
        .then(|| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()))
}

fn parse_active_query(bytes: &[u8]) -> Option<wire::Query> {
    if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
        wire::parse_pdr_query(bytes)
    } else {
        wire::parse_query(bytes)
    }
}

fn record_rx(
    report: &mut Report,
    state: &receive::Report,
    bytes: &[u8; RX_DUMP_BYTES],
    captured: usize,
) {
    report.rx_available = state.available;
    report.rx_head = state.words[3];
    let copied = captured.min(report.rx_words.len() * 4);
    report.rx_captured = copied as u32;
    let split = pixel_qrtr_snapshot::split_snapshot(&bytes[..], copied);
    report.rx_prefix_bytes = split.prefix_bytes as u32;
    report.rx_suffix_bytes = split.opaque_suffix_bytes as u32;
    if let Some(server) = prequery::observed_server(bytes, captured) {
        report.server_service = server.service;
        report.server_instance = server.instance;
        report.server_node = server.node;
        report.server_port = server.port;
    }
    report.rx_prequery_error_code = match prequery::parse(bytes, captured) {
        Ok(_) => 0,
        Err(error) => error.code(),
    };
    report.rx_prequery_shape = report.rx_prequery_error_code == 0;
    report.rx_words.fill(0);
    for (index, word) in report.rx_words.iter_mut().enumerate() {
        if let Some(value) = observe_u32(bytes, copied, index * 4) {
            *word = value;
        }
    }
}

fn copy_tx(
    ap: *mut u8,
    expected: &mut [u8; MAX_TX],
    len: &mut usize,
    at: usize,
    bytes: &[u8],
) -> Result<(), Error> {
    if at != *len
        || at
            .checked_add(bytes.len())
            .is_none_or(|end| end > MAX_TX || end > TX.size)
    {
        return Err(Error::Bounds);
    }
    expected[at..at + bytes.len()].copy_from_slice(bytes);
    for (i, byte) in bytes.iter().copied().enumerate() {
        unsafe { ap.add(TX.offset - PART + at + i).write_volatile(byte) };
    }
    *len += bytes.len();
    Ok(())
}

fn write_tail(ap: *mut u8, tail: u32) {
    unsafe {
        ap.add(DESCRIPTOR.offset - PART + 8)
            .cast::<u32>()
            .write_volatile(tail.to_le())
    }
}

fn write_head(ap: *mut u8, head: u32) {
    unsafe {
        ap.add(DESCRIPTOR.offset - PART + 4)
            .cast::<u32>()
            .write_volatile(head.to_le())
    }
}

fn notify(doorbell: *mut u32) {
    unsafe { doorbell.add(3).write_volatile((1u32 << 12).to_le()) }
}

fn pause(clock: &mut impl Clock, frequency: u64) -> Result<(), Error> {
    let start = clock.now();
    for _ in 0..1_000_000 {
        if clock.now().wrapping_sub(start) >= frequency / 100 {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(Error::Clock)
}

fn elapsed(
    clock: &mut impl Clock,
    start: u64,
    frequency: u64,
    report: &mut Report,
) -> Result<(), Error> {
    let ticks = clock.now().wrapping_sub(start);
    report.elapsed_ms = (ticks.saturating_mul(1000) / frequency).min(u32::MAX as u64) as u32;
    if ticks >= frequency.saturating_mul(WAIT_MS) / 1000 {
        Err(Error::Timeout)
    } else {
        Ok(())
    }
}

fn wait_consumed(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &[u8; MAX_TX],
    len: usize,
    head: u32,
    tail: u32,
    mut last_tx: u32,
    last_rx: u32,
    clock: &mut impl Clock,
    start: u64,
    frequency: u64,
    report: &mut Report,
) -> Result<receive::Report, Error> {
    for poll in 0..512 {
        elapsed(clock, start, frequency, report)?;
        let observed = window.locked(|m| {
            let s = state(m, base, ready, head, tail, last_tx, last_rx)?;
            tx_expected(m, expected, len)?;
            Ok::<_, Error>(s)
        })?;
        last_tx = observed.words[0];
        report.tx_words.copy_from_slice(&observed.words[..4]);
        if observed.words[0] == head {
            return Ok(observed);
        }
        if poll % 32 == 0 {
            pause(clock, frequency)?;
        }
    }
    Err(Error::Timeout)
}

/// Passively observe the RX queue after the exact reply has been consumed.
/// This never advances RX tail, publishes TX data, or notifies the peer.
fn observe_after_reply(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &[u8; MAX_TX],
    len: usize,
    tx_head: u32,
    rx_tail: u32,
    last_tx: u32,
    last_rx: u32,
    clock: &mut impl Clock,
    frequency: u64,
    report: &mut Report,
) -> Result<(), Error> {
    report.post_rx_available = 0;
    report.post_rx_head = 0;
    report.post_rx_captured = 0;
    report.post_rx_words.fill(0);
    let started = clock.now();
    let mut last_seen_head = None;
    for _ in 0..POST_REPLY_POLL_LIMIT {
        if clock.now().wrapping_sub(started)
            >= frequency.saturating_mul(POST_REPLY_WINDOW_MS) / 1000
        {
            break;
        }
        let observed = window.locked(|m| {
            let s = state(m, base, ready, tx_head, rx_tail, last_tx, last_rx)?;
            tx_expected(m, expected, len)?;
            Ok::<_, Error>(s)
        })?;
        if observed.available > 0 && last_seen_head != Some(observed.words[3]) {
            let captured = (observed.available as usize).min(POST_REPLY_CAPTURE_BYTES);
            let (bytes, stable) = window.locked(|m| {
                tx_expected(m, expected, len)?;
                let before = state(
                    m,
                    base,
                    ready,
                    tx_head,
                    rx_tail,
                    observed.words[0],
                    observed.words[3],
                )?;
                if before.words[3] < observed.words[3] || before.available < captured as u32 {
                    return Err(Error::Unstable);
                }
                let bytes = read_span(m, before, rx_tail, captured)?;
                let after = state(
                    m,
                    base,
                    ready,
                    tx_head,
                    rx_tail,
                    before.words[0],
                    before.words[3],
                )?;
                if after.words[2] != rx_tail || after.available < captured as u32 {
                    return Err(Error::Unstable);
                }
                Ok::<_, Error>((bytes, after))
            })?;
            report.post_rx_available = stable.available;
            report.post_rx_head = stable.words[3];
            report.post_rx_captured = captured as u32;
            report.post_rx_words.fill(0);
            for (index, word) in report.post_rx_words.iter_mut().enumerate() {
                let at = index * 4;
                if at + 4 <= captured {
                    *word = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                }
            }
            last_seen_head = Some(stable.words[3]);
        }
        pause(clock, frequency)?;
    }
    Ok(())
}

/// Consume only the observed RX_DONE_W_REUSE for the second PDR response,
/// then passively snapshot the next RX prefix without advancing its tail.
fn consume_followup_reply_done(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &[u8; MAX_TX],
    len: usize,
    head: u32,
    tail: u32,
    last_tx: u32,
    last_rx: u32,
    ap: *mut u8,
    clock: &mut impl Clock,
    frequency: u64,
    report: &mut Report,
) -> Result<(), Error> {
    if !report.followup_response_consumed
        || report.post_rx_available < pixel_glink_control::RX_DONE_BYTES as u32
        || report.post_rx_captured < pixel_glink_control::RX_DONE_BYTES as u32
    {
        return Err(Error::Timeout);
    }

    let (checked, intent) = window.locked(|m| {
        tx_expected(m, expected, len)?;
        let before = state(m, base, ready, head, tail, last_tx, last_rx)?;
        if before.available < pixel_glink_control::RX_DONE_BYTES as u32 {
            return Err(Error::Unstable);
        }
        let bytes = read_span(m, before, tail, pixel_glink_control::RX_DONE_BYTES)?;
        let after = state(m, base, ready, head, tail, before.words[0], before.words[3])?;
        if after.words[2] != tail
            || after.words[3] < before.words[3]
            || after.available < pixel_glink_control::RX_DONE_BYTES as u32
        {
            return Err(Error::Unstable);
        }
        if report.post_rx_words[0] != u32::from_le_bytes(bytes[..4].try_into().unwrap())
            || report.post_rx_words[1] != u32::from_le_bytes(bytes[4..8].try_into().unwrap())
        {
            return Err(Error::Unstable);
        }
        let intent =
            pixel_glink_control::parse_pdr_reply_done(&bytes[..pixel_glink_control::RX_DONE_BYTES])
                .ok_or(Error::Payload)?;
        Ok::<_, Error>((after, intent))
    })?;

    let new_tail = tail
        .checked_add(pixel_glink_control::RX_DONE_BYTES as u32)
        .ok_or(Error::Bounds)?;
    write_tail(ap, new_tail);
    sync();
    let released = window.locked(|m| {
        tx_expected(m, expected, len)?;
        let s = state(
            m,
            base,
            ready,
            head,
            new_tail,
            checked.words[0],
            checked.words[3],
        )?;
        if s.words[2] != new_tail
            || s.words[3] < checked.words[3]
            || s.available < checked.available.saturating_sub(new_tail - tail)
        {
            return Err(Error::Unstable);
        }
        Ok::<_, Error>(s)
    })?;

    report.followup_done_consumed = true;
    report.followup_done_intent = intent;
    report.rx_tail = new_tail;
    observe_after_reply(
        window,
        base,
        ready,
        expected,
        len,
        head,
        new_tail,
        released.words[0],
        released.words[3],
        clock,
        frequency,
        report,
    )
}

fn exact_followup_control(words: &[u32; POST_REPLY_WORDS], captured: u32) -> bool {
    captured >= 16 && words[..4] == [0x0002_000e, 2, 0x0002_0007, FOLLOWUP_INTENT_BYTES as u32]
}

fn parse_followup_query_frame(bytes: &[u8]) -> Option<wire::Query> {
    if bytes.len() != FOLLOWUP_QUERY_GLINK_BYTES
        || bytes[..4] != [9, 0, 2, 0]
        || u32::from_le_bytes(bytes[4..8].try_into().ok()?) != FOLLOWUP_INTENT_ID
        || u32::from_le_bytes(bytes[8..12].try_into().ok()?) != FOLLOWUP_INTENT_BYTES as u32
        || u32::from_le_bytes(bytes[12..16].try_into().ok()?) != 0
        || bytes[16 + wire::PDDUMP_QUERY_BYTES..]
            .iter()
            .any(|byte| *byte != 0)
    {
        return None;
    }
    wire::parse_pddump_query(&bytes[16..16 + wire::PDDUMP_QUERY_BYTES])
}

fn followup_intent_packets() -> [u8; 24] {
    // The RX_INTENT_REQ names remote channel2; replies use this endpoint's
    // local channel1. The intent id is fresh after local intents3 and4.
    [
        5,
        0,
        1,
        0,
        1,
        0,
        0,
        0,
        FOLLOWUP_INTENT_BYTES as u8,
        0,
        0,
        0,
        5,
        0,
        0,
        0,
        8,
        0,
        1,
        0,
        1,
        0,
        0,
        0,
    ]
}

/// Answer only the exact post-PDR control pair observed on the device, then
/// leave the first resulting data packet unread for a bounded snapshot.
fn answer_followup_intent(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &mut [u8; MAX_TX],
    len: &mut usize,
    head: &mut u32,
    tail: &mut u32,
    last_tx: u32,
    last_rx: u32,
    ap: *mut u8,
    doorbell: *mut u32,
    private: &mut FollowupBuffer,
    clock: &mut impl Clock,
    start: u64,
    frequency: u64,
    report: &mut Report,
) -> Result<(), Error> {
    for (dst, src) in report
        .followup_words
        .iter_mut()
        .zip(report.post_rx_words.iter())
    {
        *dst = *src;
    }
    report.followup_control_valid =
        exact_followup_control(&report.post_rx_words, report.post_rx_captured);
    report.followup_buffer_valid = private.guards();
    report.followup_tx_len = *len as u32;
    report.followup_tx_head = *head;
    report.followup_tx_cursor_valid = report.followup_tx_len == report.followup_tx_head;
    if !report.followup_control_valid
        || report.post_rx_available < 16
        || !report.followup_buffer_valid
        || !report.followup_tx_cursor_valid
    {
        return Err(Error::Payload);
    }

    let intent = followup_intent_packets();
    copy_tx(ap, expected, len, *len, &intent)?;
    report.byte_writes += intent.len() as u32;
    sync();

    let checked = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        let s = state(m, base, ready, *head, *tail, last_tx, last_rx)?;
        if s.available < 16 {
            return Err(Error::Unstable);
        }
        let prefix = read_span(m, s, *tail, 16)?;
        if prefix[..16]
            != [
                14,
                0,
                2,
                0,
                2,
                0,
                0,
                0,
                7,
                0,
                2,
                0,
                FOLLOWUP_INTENT_BYTES as u8,
                0,
                0,
                0,
            ]
        {
            return Err(Error::Unstable);
        }
        Ok::<_, Error>(s)
    })?;

    let new_tail = tail.checked_add(16).ok_or(Error::Bounds)?;
    write_tail(ap, new_tail);
    sync();
    let released = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        let s = state(
            m,
            base,
            ready,
            *head,
            new_tail,
            checked.words[0],
            checked.words[3],
        )?;
        if s.words[3] < checked.words[3] || s.available < checked.available.saturating_sub(16) {
            return Err(Error::Unstable);
        }
        Ok::<_, Error>(s)
    })?;

    let new_head = *len as u32;
    write_head(ap, new_head);
    sync();
    let published = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        state(
            m,
            base,
            ready,
            new_head,
            new_tail,
            released.words[0],
            released.words[3],
        )
    })?;
    *head = new_head;
    *tail = new_tail;
    report.followup_controls_consumed = true;
    report.followup_intent_offered = true;
    report.notifications += 1;
    report.rx_tail = new_tail;
    report.tx_words.copy_from_slice(&published.words[..4]);
    notify(doorbell);
    sync();

    let consumed = wait_consumed(
        window,
        base,
        ready,
        expected,
        *len,
        new_head,
        new_tail,
        published.words[0],
        published.words[3],
        clock,
        start,
        frequency,
        report,
    )?;
    report.tx_words.copy_from_slice(&consumed.words[..4]);
    report.followup_tx_consumed = consumed.words[0] == new_head;
    if !report.followup_tx_consumed {
        return Err(Error::Timeout);
    }

    capture_followup_packet(
        window,
        base,
        ready,
        expected,
        *len,
        new_head,
        new_tail,
        consumed.words[0],
        consumed.words[3],
        private,
        clock,
        frequency,
        report,
    )?;
    if !report.followup_query_valid {
        return Err(Error::Payload);
    }
    answer_followup_query(
        window,
        base,
        ready,
        expected,
        len,
        head,
        tail,
        consumed.words[0],
        consumed.words[3],
        ap,
        doorbell,
        private,
        clock,
        start,
        frequency,
        report,
    )
}

fn capture_followup_packet(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &[u8; MAX_TX],
    len: usize,
    head: u32,
    tail: u32,
    last_tx: u32,
    mut last_rx: u32,
    private: &mut FollowupBuffer,
    clock: &mut impl Clock,
    frequency: u64,
    report: &mut Report,
) -> Result<(), Error> {
    let started = clock.now();
    report.post_rx_available = 0;
    report.post_rx_head = 0;
    report.post_rx_captured = 0;
    report.followup_words.fill(0);
    for _ in 0..FOLLOWUP_POLL_LIMIT {
        if clock.now().wrapping_sub(started) >= frequency.saturating_mul(FOLLOWUP_WAIT_MS) / 1000 {
            return Ok(());
        }
        let observed = window.locked(|m| {
            tx_expected(m, expected, len)?;
            state(m, base, ready, head, tail, last_tx, last_rx)
        })?;
        report.post_rx_available = observed.available;
        report.post_rx_head = observed.words[3];
        if observed.available > 0 {
            let captured = (observed.available as usize).min(FOLLOWUP_CAPTURE_BYTES);
            let (bytes, stable) = window.locked(|m| {
                tx_expected(m, expected, len)?;
                let before = state(
                    m,
                    base,
                    ready,
                    head,
                    tail,
                    observed.words[0],
                    observed.words[3],
                )?;
                if before.available < captured as u32 {
                    return Err(Error::Unstable);
                }
                let bytes = read_span(m, before, tail, captured)?;
                let after = state(m, base, ready, head, tail, before.words[0], before.words[3])?;
                if after.words[2] != tail || after.available < captured as u32 {
                    return Err(Error::Unstable);
                }
                Ok::<_, Error>((bytes, after))
            })?;
            report.post_rx_available = stable.available;
            report.post_rx_head = stable.words[3];
            report.post_rx_captured = captured as u32;
            for (index, word) in report.followup_words.iter_mut().enumerate() {
                let at = index * 4;
                if at + 4 <= captured {
                    *word = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                }
            }
            if captured >= FOLLOWUP_QUERY_GLINK_BYTES {
                let command = u16::from_le_bytes(bytes[0..2].try_into().unwrap());
                let channel = u16::from_le_bytes(bytes[2..4].try_into().unwrap());
                let intent_id = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                let chunk = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
                let left = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
                let frame_bytes = 16usize
                    .checked_add(chunk)
                    .and_then(|n| n.checked_add(7))
                    .map(|n| n & !7)
                    .ok_or(Error::Bounds)?;
                report.followup_chunk_bytes = chunk as u32;
                report.followup_left_bytes = left;
                if command == 9
                    && channel == 2
                    && intent_id == FOLLOWUP_INTENT_ID
                    && chunk == FOLLOWUP_INTENT_BYTES
                    && left == 0
                    && frame_bytes == FOLLOWUP_QUERY_GLINK_BYTES
                    && observed.available == FOLLOWUP_QUERY_GLINK_BYTES as u32
                    && captured >= FOLLOWUP_QUERY_GLINK_BYTES
                {
                    let payload = &bytes[16..16 + chunk];
                    if payload[wire::PDDUMP_QUERY_BYTES..]
                        .iter()
                        .any(|byte| *byte != 0)
                    {
                        return Err(Error::Payload);
                    }
                    if let Some(query) =
                        wire::parse_pddump_query(&payload[..wire::PDDUMP_QUERY_BYTES])
                    {
                        if !private.copy_exact(payload) || !private.verified_exact(payload) {
                            return Err(Error::Publication);
                        }
                        report.followup_packet_valid = true;
                        report.followup_query_valid = true;
                        report.followup_transaction = query.transaction as u32;
                        report.followup_request_port = query.remote_port;
                    }
                }
            }
            report.guards &= private.guards();
            if !report.guards {
                return Err(Error::Publication);
            }
            return Ok(());
        }
        last_rx = last_rx.max(observed.words[3]);
        pause(clock, frequency)?;
    }
    Ok(())
}

/// Consume and answer only the captured `tms/pddump_disabled` lookup, then
/// passively observe the RX queue without consuming another record.
fn answer_followup_query(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &mut [u8; MAX_TX],
    len: &mut usize,
    head: &mut u32,
    tail: &mut u32,
    last_tx: u32,
    last_rx: u32,
    ap: *mut u8,
    doorbell: *mut u32,
    private: &mut FollowupBuffer,
    clock: &mut impl Clock,
    start: u64,
    frequency: u64,
    report: &mut Report,
) -> Result<(), Error> {
    if !report.followup_packet_valid
        || !report.followup_query_valid
        || !private.guards()
        || !private.verified_exact(&private.data)
    {
        return Err(Error::Publication);
    }
    let query = wire::parse_pddump_query(&private.data[..wire::PDDUMP_QUERY_BYTES])
        .ok_or(Error::Payload)?;
    if query.transaction as u32 != report.followup_transaction
        || query.remote_port != report.followup_request_port
    {
        return Err(Error::Payload);
    }

    let query_state = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        let s = state(m, base, ready, *head, *tail, last_tx, last_rx)?;
        if s.available != FOLLOWUP_QUERY_GLINK_BYTES as u32 {
            return Err(Error::Unstable);
        }
        let bytes = read_span(m, s, *tail, FOLLOWUP_QUERY_GLINK_BYTES)?;
        if parse_followup_query_frame(&bytes[..FOLLOWUP_QUERY_GLINK_BYTES]) != Some(query)
            || bytes[16..FOLLOWUP_QUERY_GLINK_BYTES] != private.data[..FOLLOWUP_INTENT_BYTES]
            || !private.guards()
        {
            return Err(Error::Unstable);
        }
        Ok::<_, Error>(s)
    })?;

    let mut qrtr_response = [0; wire::PDR_EMPTY_RESPONSE_BYTES];
    wire::pdr_empty_response(query, &mut qrtr_response);
    let response = wire::glink_data_frame::<{ wire::PDR_EMPTY_RESPONSE_GLINK_BYTES }>(
        REPLY_REMOTE_INTENT,
        &qrtr_response,
    )
    .ok_or(Error::Bounds)?;
    let done = [6, 0, 1, 0, FOLLOWUP_INTENT_ID as u8, 0, 0, 0];
    copy_tx(ap, expected, len, *len, &done)?;
    copy_tx(ap, expected, len, *len, &response)?;
    report.byte_writes += (done.len() + response.len()) as u32;
    sync();

    let checked = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        let s = state(m, base, ready, *head, *tail, last_tx, last_rx)?;
        if s.words[1] != query_state.words[1]
            || s.words[2] != query_state.words[2]
            || s.words[3] < query_state.words[3]
            || s.available < FOLLOWUP_QUERY_GLINK_BYTES as u32
        {
            return Err(Error::Unstable);
        }
        let bytes = read_span(m, s, *tail, FOLLOWUP_QUERY_GLINK_BYTES)?;
        if parse_followup_query_frame(&bytes[..FOLLOWUP_QUERY_GLINK_BYTES]) != Some(query)
            || bytes[16..FOLLOWUP_QUERY_GLINK_BYTES] != private.data[..FOLLOWUP_INTENT_BYTES]
            || !private.verified_exact(&private.data)
        {
            return Err(Error::Unstable);
        }
        Ok::<_, Error>(s)
    })?;
    let new_tail = tail
        .checked_add(FOLLOWUP_QUERY_GLINK_BYTES as u32)
        .ok_or(Error::Bounds)?;
    write_tail(ap, new_tail);
    sync();
    let released = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        let s = state(m, base, ready, *head, new_tail, last_tx, checked.words[3])?;
        if s.words[1] != *head || s.words[2] != new_tail {
            return Err(Error::Unstable);
        }
        Ok::<_, Error>(s)
    })?;

    let new_head = *len as u32;
    write_head(ap, new_head);
    sync();
    let published = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        state(
            m,
            base,
            ready,
            new_head,
            new_tail,
            released.words[0],
            released.words[3],
        )
    })?;
    *head = new_head;
    *tail = new_tail;
    report.followup_response_staged = true;
    report.notifications += 1;
    report.rx_tail = new_tail;
    report.tx_words.copy_from_slice(&published.words[..4]);
    notify(doorbell);
    sync();

    let consumed = wait_consumed(
        window,
        base,
        ready,
        expected,
        *len,
        new_head,
        new_tail,
        published.words[0],
        published.words[3],
        clock,
        start,
        frequency,
        report,
    )?;
    report.tx_words.copy_from_slice(&consumed.words[..4]);
    report.followup_response_consumed = consumed.words[0] == new_head;
    report.guards &= private.guards() && private.verified_exact(&private.data);
    if !report.followup_response_consumed || !report.guards {
        return Err(Error::Timeout);
    }

    observe_after_reply(
        window,
        base,
        ready,
        expected,
        *len,
        new_head,
        new_tail,
        consumed.words[0],
        consumed.words[3],
        clock,
        frequency,
        report,
    )?;
    if cfg!(feature = "sunfish-qrtr-servreg-pdr-followup") {
        consume_followup_reply_done(
            window,
            base,
            ready,
            expected,
            *len,
            new_head,
            new_tail,
            consumed.words[0],
            consumed.words[3],
            ap,
            clock,
            frequency,
            report,
        )?;
    }
    Ok(())
}

fn pending_request(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &[u8; MAX_TX],
    len: usize,
    head: u32,
    tail: u32,
    last_tx: u32,
    mut last_rx: u32,
    clock: &mut impl Clock,
    start: u64,
    frequency: u64,
    report: &mut Report,
) -> Result<receive::Report, Error> {
    for _ in 0..4500 {
        elapsed(clock, start, frequency, report)?;
        let found = window.locked(|m| {
            let s = state(m, base, ready, head, tail, last_tx, last_rx)?;
            tx_expected(m, expected, len)?;
            if s.available == 0 {
                return Ok::<_, Error>(None);
            }
            if s.available != 8 {
                return Err(Error::Payload);
            }
            let b = read_span(m, s, tail, 8)?;
            if b[..8] != [7, 0, 2, 0, 52, 0, 0, 0] {
                return Err(Error::Payload);
            }
            Ok(Some(s))
        })?;
        if let Some(s) = found {
            report.tx_words.copy_from_slice(&s.words[..4]);
            report.rx_tail = tail;
            return Ok(s);
        }
        last_rx =
            last_rx.max(window.locked(|m| receive::inspect_ref(m, ready).map(|s| s.words[3]))?);
        pause(clock, frequency)?;
    }
    Err(Error::Timeout)
}

#[allow(clippy::too_many_arguments)]
#[cfg(not(feature = "sunfish-qrtr-servreg-prequery-check"))]
fn consume_prequery(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &mut [u8; MAX_TX],
    len: &mut usize,
    head: &mut u32,
    tail: &mut u32,
    last_tx: &mut u32,
    last_rx: &mut u32,
    snapshot: receive::Report,
    bytes: &[u8; RX_DUMP_BYTES],
    captured: usize,
    ap: *mut u8,
    doorbell: *mut u32,
    clock: &mut impl Clock,
    start: u64,
    frequency: u64,
    report: &mut Report,
) -> Result<(), Error> {
    let prequery = prequery::parse(bytes, captured).map_err(|_| Error::Payload)?;
    if snapshot.available < PREQUERY_BYTES as u32 {
        return Err(Error::Payload);
    }

    // Retire the just-filled local intent3, then answer the peer's exact
    // 60-byte request with a fresh 64-byte intent4.
    let done = [6, 0, 1, 0, LOCAL_BUFFER_ID as u8, 0, 0, 0];
    let (advertise, advertise_size) = discovery::Packet::Advertise(1).bytes();
    if advertise_size != 24 {
        return Err(Error::Bounds);
    }
    let new_tail = tail
        .checked_add(PREQUERY_BYTES as u32)
        .ok_or(Error::Bounds)?;
    window.locked(|m| {
        tx_expected(m, expected, *len)?;
        let current = state(m, base, ready, *head, *tail, *last_tx, *last_rx)?;
        if current.words[1] != snapshot.words[1]
            || current.words[2] != snapshot.words[2]
            || current.words[3] < new_tail
            || current.available < PREQUERY_BYTES as u32
        {
            return Err(Error::Unstable);
        }
        let copied = read_span(m, current, *tail, PREQUERY_BYTES)?;
        if copied[..PREQUERY_BYTES] != bytes[..PREQUERY_BYTES] {
            return Err(Error::Unstable);
        }
        Ok::<_, Error>(())
    })?;

    copy_tx(ap, expected, len, *len, &done)?;
    copy_tx(ap, expected, len, *len, &advertise[..advertise_size])?;
    report.byte_writes += (done.len() + advertise_size) as u32;
    sync();
    let new_head = *len as u32;

    write_tail(ap, new_tail);
    sync();
    let released = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        let s = state(m, base, ready, *head, new_tail, *last_tx, *last_rx)?;
        if s.words[1] != *head || s.words[2] != new_tail || s.words[3] < new_tail {
            return Err(Error::Unstable);
        }
        Ok::<_, Error>(s)
    })?;

    write_head(ap, new_head);
    sync();
    let published = window.locked(|m| {
        tx_expected(m, expected, *len)?;
        state(
            m,
            base,
            ready,
            new_head,
            new_tail,
            released.words[0],
            released.words[3],
        )
    })?;
    report.prequery_handled = true;
    report.server_service = prequery.server.service;
    report.server_instance = prequery.server.instance;
    report.server_node = prequery.server.node;
    report.server_port = prequery.server.port;
    report.returned_intent = prequery.returned_intent;
    report.requested_intent_size = prequery.requested_intent_size;
    report.query_intent_id = LOCAL_QUERY_INTENT_ID;
    report.notifications += 1;
    report.rx_tail = new_tail;
    report.rx_available = 0;
    report.rx_captured = 0;
    report.rx_prequery_shape = false;
    report.tx_words.copy_from_slice(&published.words[..4]);
    notify(doorbell);
    sync();

    let consumed = wait_consumed(
        window,
        base,
        ready,
        expected,
        *len,
        new_head,
        new_tail,
        published.words[0],
        published.words[3],
        clock,
        start,
        frequency,
        report,
    )?;
    *head = new_head;
    *tail = new_tail;
    *last_tx = consumed.words[0];
    *last_rx = consumed.words[3];
    report.tx_words.copy_from_slice(&consumed.words[..4]);
    Ok(())
}

fn incoming_query(
    window: &mut Window,
    base: &receive::Report,
    ready: &ReadyObservation,
    expected: &mut [u8; MAX_TX],
    len: &mut usize,
    head: &mut u32,
    tail: &mut u32,
    last_tx: &mut u32,
    last_rx: &mut u32,
    buffer: &mut Buffer,
    ap: *mut u8,
    doorbell: *mut u32,
    clock: &mut impl Clock,
    start: u64,
    frequency: u64,
    report: &mut Report,
) -> Result<(receive::Report, Option<wire::Query>, u32), Error> {
    #[cfg(feature = "sunfish-qrtr-servreg-prequery-check")]
    let query_intent_id = LOCAL_BUFFER_ID;
    #[cfg(not(feature = "sunfish-qrtr-servreg-prequery-check"))]
    let mut query_intent_id = LOCAL_BUFFER_ID;
    #[cfg(feature = "sunfish-qrtr-servreg-prequery-check")]
    let _ = (ap, doorbell);
    for _ in 0..4500 {
        elapsed(clock, start, frequency, report)?;
        let found = window.locked(|m| {
            let s = state(m, base, ready, *head, *tail, *last_tx, *last_rx)?;
            tx_expected(m, expected, *len)?;
            if s.available == 0 {
                return Ok::<_, Error>(None);
            }
            let captured = (s.available as usize).min(RX_DUMP_BYTES);
            let b = read_span(m, s, *tail, captured)?;
            Ok(Some((s, b, captured)))
        })?;
        if let Some((s, b, captured)) = found {
            record_rx(report, &s, &b, captured);
            if captured >= PREQUERY_BYTES {
                match prequery::parse(&b, captured) {
                    Ok(prequery) if !report.prequery_handled => {
                        report.server_service = prequery.server.service;
                        report.server_instance = prequery.server.instance;
                        report.server_node = prequery.server.node;
                        report.server_port = prequery.server.port;
                        report.returned_intent = prequery.returned_intent;
                        report.requested_intent_size = prequery.requested_intent_size;
                        #[cfg(feature = "sunfish-qrtr-servreg-prequery-check")]
                        {
                            report.prequery_check_pass = true;
                            report.rx_tail = *tail;
                            report.tx_words.copy_from_slice(&s.words[..4]);
                            return Ok((s, None, query_intent_id));
                        }
                        #[cfg(not(feature = "sunfish-qrtr-servreg-prequery-check"))]
                        {
                            consume_prequery(
                                window, base, ready, expected, len, head, tail, last_tx, last_rx,
                                s, &b, captured, ap, doorbell, clock, start, frequency, report,
                            )?;
                            query_intent_id = LOCAL_QUERY_INTENT_ID;
                            continue;
                        }
                    }
                    Ok(_) | Err(_) => {
                        return Err(Error::Payload);
                    }
                }
            }
            if s.available != QUERY_GLINK_BYTES as u32 {
                // A coalesced or unfamiliar packet can be followed by more
                // bytes. Keep RX tail fixed and sample until the queue is
                // quiet, so the raw diagnostic includes a complete next
                // record when it arrives promptly.
                let mut best_state = s;
                let mut best_bytes = b;
                let mut best_captured = captured;
                let mut stable_polls = 0;
                let mut observed_rx = *last_rx;
                for _ in 0..RX_SETTLE_POLLS {
                    elapsed(clock, start, frequency, report)?;
                    let current = window.locked(|m| {
                        let current = state(m, base, ready, *head, *tail, *last_tx, observed_rx)?;
                        tx_expected(m, expected, *len)?;
                        let copied = (current.available as usize).min(RX_DUMP_BYTES);
                        let bytes = read_span(m, current, *tail, copied)?;
                        Ok::<_, Error>((current, bytes, copied))
                    })?;
                    let (current_state, current_bytes, current_captured) = current;
                    if current_state.available < best_state.available
                        || current_captured < best_captured
                        || current_bytes[..best_captured] != best_bytes[..best_captured]
                    {
                        return Err(Error::Unstable);
                    }
                    observed_rx = observed_rx.max(current_state.words[3]);
                    if current_state.available > best_state.available {
                        best_state = current_state;
                        best_bytes = current_bytes;
                        best_captured = current_captured;
                        stable_polls = 0;
                    } else {
                        stable_polls += 1;
                    }
                    if stable_polls >= RX_SETTLE_QUIET_POLLS {
                        break;
                    }
                    pause(clock, frequency)?;
                }
                record_rx(report, &best_state, &best_bytes, best_captured);
                return Err(Error::Payload);
            }
            if b[0..4] != [9, 0, 2, 0]
                || u32::from_le_bytes(b[4..8].try_into().unwrap()) != query_intent_id
                || u32::from_le_bytes(b[8..12].try_into().unwrap()) != QUERY_CHUNK_BYTES as u32
                || u32::from_le_bytes(b[12..16].try_into().unwrap()) != 0
                || b[16 + QUERY_DATAGRAM_BYTES..16 + QUERY_CHUNK_BYTES]
                    .iter()
                    .any(|v| *v != 0)
                || b[16 + QUERY_CHUNK_BYTES..QUERY_GLINK_BYTES]
                    .iter()
                    .any(|v| *v != 0)
            {
                return Err(Error::Payload);
            }
            let datagram = &b[16..16 + QUERY_DATAGRAM_BYTES];
            let query = parse_active_query(datagram).ok_or(Error::Payload)?;
            if !buffer.copy_exact(datagram) || !buffer.verified_exact(datagram) {
                return Err(Error::Publication);
            }
            let query = query;
            report.tx_words.copy_from_slice(&s.words[..4]);
            report.rx_tail = *tail;
            report.query_intent_id = query_intent_id;
            return Ok((s, Some(query), query_intent_id));
        }
        *last_rx =
            (*last_rx).max(window.locked(|m| receive::inspect_ref(m, ready).map(|s| s.words[3]))?);
        pause(clock, frequency)?;
    }
    Err(Error::Timeout)
}

pub struct ServregOnce<'a, 'b, 'c> {
    checkpoint: DiscoveryCheckpoint<'a, 'b>,
    buffer: &'c mut Buffer,
    followup_buffer: FollowupBuffer,
    ap: *mut u8,
    doorbell: *mut u32,
    spent: bool,
}

impl<'a, 'b, 'c> ServregOnce<'a, 'b, 'c> {
    /// # Safety
    /// Same exclusive mappings/checkpoint requirements as the preceding QRTR
    /// probe. `buffer` is a fresh disjoint guarded 64-byte private receive area.
    pub unsafe fn new(
        checkpoint: DiscoveryCheckpoint<'a, 'b>,
        buffer: &'c mut Buffer,
        ap: *mut u8,
        doorbell: *mut u32,
    ) -> Self {
        Self {
            checkpoint,
            buffer,
            followup_buffer: FollowupBuffer::new(),
            ap,
            doorbell,
            spent: false,
        }
    }

    pub fn run(&mut self, window: &mut Window, clock: &mut impl Clock) -> Report {
        if self.spent {
            let mut r = Report::new();
            r.error = Some(Error::Outbound);
            return r;
        }
        self.spent = true;
        self.run_once(window, clock)
    }

    fn run_once(&mut self, window: &mut Window, clock: &mut impl Clock) -> Report {
        let mut r = Report::new();
        let frequency = clock.frequency();
        if !(1_000_000..=1_000_000_000).contains(&frequency) {
            r.error = Some(Error::Clock);
            return r;
        }
        let start = clock.now();
        let mut advanced = false;
        for _ in 0..10_000 {
            if clock.now() != start {
                advanced = true;
                break;
            }
        }
        if !advanced {
            r.error = Some(Error::Clock);
            return r;
        }

        let (hello, second, prior, wire_capture) = self.checkpoint.parts();
        let (send, hello_report, control) = hello.parts();
        let (completion, _) = send.parts();
        let (ready, base, _, channel_capture, _, first) = completion.parts();
        let frame_payload: [u8; 52] = match wire_capture.bytes[16..68].try_into() {
            Ok(p) => p,
            Err(_) => {
                r.error = Some(Error::Bounds);
                return r;
            }
        };
        let initial_request = discovery::service(&frame_payload);
        let check = (|| -> Result<(), Error> {
            if !hello_report.passed()
                || hello_report.capacity < 128
                || pixel_qrtr_hello::peer_capacity(channel_capture, ANNOUNCE_REMOTE_INTENT)? < 52
                || pixel_qrtr_hello::peer_capacity(channel_capture, REPLY_REMOTE_INTENT)?
                    < wire::RESPONSE_BYTES as u32
                || !prior.passed()
                || prior.words[0..3] != [168, 168, 656]
                || !(88..=96).contains(&prior.available)
                || !(72..=80).contains(&wire_capture.size)
                || control.size != 16
                || initial_request.is_err()
                || !first.verified(&control.first_payload)
                || !second.verified(&frame_payload)
                || !self.buffer.empty()
            {
                return Err(Error::Payload);
            }
            pixel_qrtr_next::packet(&wire_capture.bytes[..72])?;
            let mut expected = discovery::initial_tx();
            let mut len = 168usize;
            let head = 168u32;
            let mut tail = 656u32;
            let last_rx = prior.words[3];

            let before = window.locked(|m| {
                let s = state(m, base, ready, head, tail, prior.words[0], last_rx)?;
                tx_expected(m, &expected, len)?;
                let prefix = read_span(m, s, tail, 16)?;
                if prefix[..16] != control.bytes[..16] {
                    return Err(Error::Unstable);
                }
                let frame = read_span(m, s, 672, wire_capture.size)?;
                if frame[..wire_capture.size] != wire_capture.bytes[..wire_capture.size] {
                    return Err(Error::Unstable);
                }
                Ok::<_, Error>(s)
            })?;

            // Publish one server announcement through remote intent1 and
            // complete only the already-owned first peer packet (intent2).
            r.stage = 1;
            let announce =
                wire::glink_data_frame::<72>(ANNOUNCE_REMOTE_INTENT, &wire::announcement())
                    .ok_or(Error::Bounds)?;
            let done = [6, 0, 1, 0, 2, 0, 0, 0];
            let (ap, bell) = (self.ap, self.doorbell);
            let announce_at = len;
            copy_tx(ap, &mut expected, &mut len, announce_at, &announce)?;
            r.byte_writes += announce.len() as u32;
            let done_at = len;
            copy_tx(ap, &mut expected, &mut len, done_at, &done)?;
            r.byte_writes += done.len() as u32;
            let new_head = len as u32;
            sync();
            window.locked(|m| {
                tx_expected(m, &expected, len)?;
                let s = state(m, base, ready, head, 656, prior.words[0], last_rx)?;
                let after = state(m, base, ready, head, 656, s.words[0], last_rx)?;
                if s.words != after.words {
                    return Err(Error::Unstable);
                }
                Ok::<_, Error>(())
            })?;
            tail = 744;
            write_tail(ap, tail);
            sync();
            let released = window.locked(|m| {
                tx_expected(m, &expected, len)?;
                let s = state(m, base, ready, head, tail, before.words[0], last_rx)?;
                if s.words[3] != before.words[3]
                    || s.available != before.words[3].saturating_sub(tail)
                {
                    return Err(Error::Unstable);
                }
                Ok::<_, Error>(s)
            })?;
            write_head(ap, new_head);
            sync();
            let published = window.locked(|m| {
                tx_expected(m, &expected, len)?;
                state(m, base, ready, new_head, tail, released.words[0], last_rx)
            })?;
            r.announced = true;
            r.notifications = 1;
            r.tx_words.copy_from_slice(&published.words[..4]);
            r.rx_tail = tail;
            notify(bell);
            sync();
            let after_announce = wait_consumed(
                window,
                base,
                ready,
                &expected,
                len,
                new_head,
                tail,
                published.words[0],
                last_rx,
                clock,
                start,
                frequency,
                &mut r,
            )?;

            // Wait for the exact one-buffer request needed to receive a QMI
            // request. No other GLINK control/data shape is accepted.
            r.stage = 2;
            let pending = pending_request(
                window,
                base,
                ready,
                &expected,
                len,
                new_head,
                tail,
                after_announce.words[0],
                last_rx,
                clock,
                start,
                frequency,
                &mut r,
            )?;

            // Advertise one fresh 64-byte local intent, ID3, and consume only
            // the exact request7/remote2/size52 record.
            r.stage = 3;
            let adv = discovery::Packet::Advertise(0);
            let (adv_bytes, adv_size) = adv.bytes();
            if adv_size != 24 || !self.buffer.empty() {
                return Err(Error::Payload);
            }
            let adv_at = len;
            copy_tx(
                self.ap,
                &mut expected,
                &mut len,
                adv_at,
                &adv_bytes[..adv_size],
            )?;
            r.byte_writes += adv_size as u32;
            tail += 8;
            write_tail(self.ap, tail);
            sync();
            let checked = window.locked(|m| {
                tx_expected(m, &expected, len)?;
                let s = state(
                    m,
                    base,
                    ready,
                    new_head,
                    tail,
                    pending.words[0],
                    pending.words[3],
                )?;
                if s.words[3] != pending.words[3] || s.available != 0 {
                    return Err(Error::Unstable);
                }
                Ok::<_, Error>(s)
            })?;
            let mut head = len as u32;
            write_head(self.ap, head);
            sync();
            let published = window.locked(|m| {
                tx_expected(m, &expected, len)?;
                state(
                    m,
                    base,
                    ready,
                    head,
                    tail,
                    checked.words[0],
                    pending.words[3],
                )
            })?;
            r.notifications += 1;
            r.tx_words.copy_from_slice(&published.words[..4]);
            r.rx_tail = tail;
            notify(self.doorbell);
            sync();
            let after_adv = wait_consumed(
                window,
                base,
                ready,
                &expected,
                len,
                head,
                tail,
                published.words[0],
                published.words[3],
                clock,
                start,
                frequency,
                &mut r,
            )?;

            // Copy and parse one exact QMI request, double-reading the shared
            // FIFO and retaining a private guarded copy before any reply.
            r.stage = 4;
            let mut last_tx = after_adv.words[0];
            let mut last_rx = after_adv.words[3];
            let (query_state, query, query_intent_id) = incoming_query(
                window,
                base,
                ready,
                &mut expected,
                &mut len,
                &mut head,
                &mut tail,
                &mut last_tx,
                &mut last_rx,
                self.buffer,
                self.ap,
                self.doorbell,
                clock,
                start,
                frequency,
                &mut r,
            )?;
            if r.prequery_check_pass {
                return Ok(());
            }
            let query = query.ok_or(Error::Payload)?;
            r.request_seen = true;
            r.request_port = query.remote_port;
            r.transaction = query.transaction as u32;
            r.guards = self.buffer.guards()
                && first.verified(&control.first_payload)
                && second.verified(&frame_payload);
            if !r.guards {
                return Err(Error::Publication);
            }

            // The prequery-check image is diagnostic-only. Even if a direct
            // QMI request arrives without the expected NEW_SERVER prelude,
            // stop after parsing and validating its private copy; never
            // consume RX or publish a SERVREG response in this build.
            #[cfg(feature = "sunfish-qrtr-servreg-query-check")]
            {
                r.query_check_pass = true;
            }
            #[cfg(any(
                feature = "sunfish-qrtr-servreg-prequery-check",
                feature = "sunfish-qrtr-servreg-query-check"
            ))]
            return Err(Error::Payload);

            // Respond once from the published endpoint using remote intent2;
            // retire whichever validated local intent carried the QMI query.
            r.stage = 5;
            let mut response_frame = [0; RESPONSE_GLINK_BYTES];
            let response_frame_len = if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") {
                let mut qrtr_response = [0; wire::PDR_EMPTY_RESPONSE_BYTES];
                wire::pdr_empty_response(query, &mut qrtr_response);
                let frame = wire::glink_data_frame::<{ wire::PDR_EMPTY_RESPONSE_GLINK_BYTES }>(
                    REPLY_REMOTE_INTENT,
                    &qrtr_response,
                )
                .ok_or(Error::Bounds)?;
                response_frame[..frame.len()].copy_from_slice(&frame);
                frame.len()
            } else {
                let mut qrtr_response = [0; wire::RESPONSE_BYTES];
                wire::response(query, &mut qrtr_response);
                let frame = wire::glink_data_frame::<RESPONSE_GLINK_BYTES>(
                    REPLY_REMOTE_INTENT,
                    &qrtr_response,
                )
                .ok_or(Error::Bounds)?;
                response_frame.copy_from_slice(&frame);
                frame.len()
            };
            let data = &response_frame[..response_frame_len];
            let mut done = [6, 0, 1, 0, 0, 0, 0, 0];
            done[4..8].copy_from_slice(&query_intent_id.to_le_bytes());
            let done_at = len;
            copy_tx(self.ap, &mut expected, &mut len, done_at, &done)?;
            r.byte_writes += done.len() as u32;
            let data_at = len;
            copy_tx(self.ap, &mut expected, &mut len, data_at, &data)?;
            r.byte_writes += data.len() as u32;
            sync();
            let checked = window.locked(|m| {
                tx_expected(m, &expected, len)?;
                let s = state(m, base, ready, head, tail, last_tx, last_rx)?;
                if s.words != query_state.words || s.available != QUERY_GLINK_BYTES as u32 {
                    return Err(Error::Unstable);
                }
                let bytes = read_span(m, s, tail, QUERY_GLINK_BYTES)?;
                if parse_active_query(&bytes[16..16 + QUERY_DATAGRAM_BYTES]) != Some(query) {
                    return Err(Error::Unstable);
                }
                if !self
                    .buffer
                    .verified_exact(&bytes[16..16 + QUERY_DATAGRAM_BYTES])
                {
                    return Err(Error::Publication);
                }
                Ok::<_, Error>(())
            })?;
            let _ = checked;
            tail = tail
                .checked_add(QUERY_GLINK_BYTES as u32)
                .ok_or(Error::Bounds)?;
            write_tail(self.ap, tail);
            sync();
            let released = window.locked(|m| {
                tx_expected(m, &expected, len)?;
                let s = state(m, base, ready, head, tail, last_tx, last_rx)?;
                if s.words[3] != query_state.words[3] || s.available != 0 {
                    return Err(Error::Unstable);
                }
                Ok::<_, Error>(s)
            })?;
            head = len as u32;
            write_head(self.ap, head);
            sync();
            let published = window.locked(|m| {
                tx_expected(m, &expected, len)?;
                state(m, base, ready, head, tail, released.words[0], last_rx)
            })?;
            r.response_staged = true;
            r.notifications += 1;
            r.rx_tail = tail;
            r.tx_words.copy_from_slice(&published.words[..4]);
            notify(self.doorbell);
            sync();
            let final_state = wait_consumed(
                window,
                base,
                ready,
                &expected,
                len,
                head,
                tail,
                published.words[0],
                published.words[3],
                clock,
                start,
                frequency,
                &mut r,
            )?;
            r.tx_words.copy_from_slice(&final_state.words[..4]);
            r.response_consumed = final_state.words[0] == head;
            r.guards &= self
                .buffer
                .verified_exact(&self.buffer.bytes()[..QUERY_DATAGRAM_BYTES]);
            if cfg!(feature = "sunfish-qrtr-servreg-pdr-reply") && r.response_consumed {
                observe_after_reply(
                    window,
                    base,
                    ready,
                    &expected,
                    len,
                    head,
                    tail,
                    final_state.words[0],
                    final_state.words[3],
                    clock,
                    frequency,
                    &mut r,
                )?;
            }
            if cfg!(feature = "sunfish-qrtr-servreg-pdr-followup") && r.response_consumed {
                answer_followup_intent(
                    window,
                    base,
                    ready,
                    &mut expected,
                    &mut len,
                    &mut head,
                    &mut tail,
                    final_state.words[0],
                    final_state.words[3],
                    self.ap,
                    self.doorbell,
                    &mut self.followup_buffer,
                    clock,
                    start,
                    frequency,
                    &mut r,
                )?;
            }
            elapsed(clock, start, frequency, &mut r)?;
            Ok(())
        })();

        if let Err(e) = check {
            r.error = Some(e);
        }
        r
    }
}
