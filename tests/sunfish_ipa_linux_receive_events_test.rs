#[path = "../rust/src/sunfish_ipa_linux_buffers.rs"]
pub mod sunfish_ipa_linux_buffers;
#[path = "../rust/src/sunfish_ipa_linux_endpoint.rs"]
pub mod sunfish_ipa_linux_endpoint;
#[path = "../rust/src/sunfish_ipa_linux_gsi.rs"]
pub mod sunfish_ipa_linux_gsi;
#[path = "../rust/src/sunfish_ipa_linux_receive.rs"]
pub mod sunfish_ipa_linux_receive;
#[path = "../rust/src/sunfish_ipa_linux_receive_events.rs"]
pub mod sunfish_ipa_linux_receive_events;

use sunfish_ipa_linux_receive::{self as rx, Plan};
use sunfish_ipa_linux_receive_events::*;

#[derive(Default)]
struct Fake {
    producer: u32,
    events: Vec<u32>,
    tres: Vec<u32>,
    calls: usize,
    event_reads: usize,
    stores: usize,
    reads: usize,
    releases: usize,
    acquires: usize,
    generated: usize,
    fail: Option<usize>,
    torn: Option<usize>,
    suppress: Option<usize>,
    arrive_on_event_read: Option<usize>,
    writes: Vec<(usize, u32)>,
}
impl Fake {
    fn new() -> Self {
        let plan = Plan::new().unwrap();
        let mut this = Self {
            producer: plan.event_iova() as u32,
            events: vec![0; rx::RING_COUNT * 4],
            tres: vec![0; rx::RING_COUNT * 4],
            ..Self::default()
        };
        for n in 0..rx::BUFFER_COUNT {
            let tre = plan.buffer(n).unwrap().tre();
            for (i, chunk) in tre.chunks_exact(4).enumerate() {
                this.tres[4 * n + i] = u32::from_le_bytes(chunk.try_into().unwrap());
            }
        }
        this
    }
    fn step(&mut self) -> Result<(), Error> {
        self.calls += 1;
        if self.fail == Some(self.calls) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
    fn event(&mut self, slot: usize, raw: [u8; 16]) {
        for (n, chunk) in raw.chunks_exact(4).enumerate() {
            self.events[slot * 4 + n] = u32::from_le_bytes(chunk.try_into().unwrap());
        }
    }
    fn push(&mut self, count: usize) {
        for n in self.generated..self.generated + count {
            self.event(n % 256, event(n));
        }
        self.generated += count;
        self.producer = 0x80008000 + ((self.generated % 256) * 16) as u32;
    }
    fn tre(&self, slot: usize) -> [u8; 16] {
        let mut raw = [0; 16];
        for (n, chunk) in raw.chunks_exact_mut(4).enumerate() {
            chunk.copy_from_slice(&self.tres[slot * 4 + n].to_le_bytes());
        }
        raw
    }
}
fn event(n: usize) -> [u8; 16] {
    let length = if n % 17 == 0 {
        0
    } else if n % 19 == 0 {
        8128
    } else {
        (n * 73 + 19) % 8129
    };
    let mut raw = [0; 16];
    raw[..8].copy_from_slice(&(0x80009000u64 + ((n % 256) * 16) as u64).to_le_bytes());
    raw[8..10].copy_from_slice(&(length as u16).to_le_bytes());
    raw[10] = (n * 3) as u8;
    raw[11] = (n * 17) as u8;
    raw[12..14].copy_from_slice(&((n * 37) as u16).to_le_bytes());
    raw[14] = (n * 29) as u8;
    raw[15] = 2;
    raw
}
impl Io for Fake {
    fn producer(&mut self) -> Result<u32, Error> {
        self.step()?;
        Ok(self.producer)
    }
    fn acquire(&mut self) -> Result<(), Error> {
        self.acquires += 1;
        self.step()
    }
    fn event32(&mut self, offset: usize) -> Result<u32, Error> {
        assert!((0x8000..0x9000).contains(&offset) && offset % 4 == 0);
        self.step()?;
        self.event_reads += 1;
        if self.arrive_on_event_read == Some(self.event_reads) {
            self.push(3);
        }
        let value = self.events[(offset - 0x8000) / 4];
        Ok(if self.torn == Some(self.event_reads) {
            value ^ 1
        } else {
            value
        })
    }
    fn return_events(&mut self, request: &Store) -> Result<(), Error> {
        assert_eq!(request.offset(), EVENT_DOORBELL);
        assert!((0x80008000..0x80009000).contains(&request.value()));
        assert_eq!(request.value() % 16, 0);
        self.writes.push((request.offset(), request.value()));
        self.step()
    }
    fn write_tre(&mut self, request: &Store) -> Result<(), Error> {
        assert!((0x9000..0xa000).contains(&request.offset()) && request.offset() % 4 == 0);
        self.stores += 1;
        if self.suppress != Some(self.stores) {
            self.tres[(request.offset() - 0x9000) / 4] = request.value();
        }
        // Model the uncertain operation: store may happen before Err.
        self.step()
    }
    fn read_tre(&mut self, offset: usize) -> Result<u32, Error> {
        assert!((0x9000..0xa000).contains(&offset) && offset % 4 == 0);
        self.reads += 1;
        self.step()?;
        Ok(self.tres[(offset - 0x9000) / 4])
    }
    fn release(&mut self) -> Result<(), Error> {
        self.releases += 1;
        self.step()
    }
    fn publish(&mut self, request: &Store) -> Result<(), Error> {
        assert_eq!(request.offset(), CHANNEL_DOORBELL);
        assert!((0x80009000..0x8000a000).contains(&request.value()));
        assert_eq!(request.value() % 16, 0);
        self.writes.push((request.offset(), request.value()));
        self.step()
    }
}
fn queue() -> Queue {
    Queue::new(Plan::new().unwrap()).unwrap()
}
fn hex(value: &str) -> usize {
    usize::from_str_radix(value, 16).unwrap()
}
fn bytes(value: &str) -> [u8; 16] {
    assert_eq!(value.len(), 32);
    core::array::from_fn(|n| hex(&value[n * 2..n * 2 + 2]) as u8)
}

#[test]
fn rxevents_original_linux_receive_update_credit_and_repost_wrap() {
    let reference = include_str!(env!("SUNFISH_RECEIVE_EVENTS_C"));
    let mut q = queue();
    let mut io = Fake::new();
    let mut total = 0;
    let mut wrapped_credit = false;
    let mut wrapped_publish = false;
    for block in reference.split("batch ").skip(1) {
        let rows: Vec<Vec<&str>> = block
            .lines()
            .map(|r| r.split_whitespace().collect())
            .collect();
        let count: usize = rows[0][0].parse().unwrap();
        io.producer = hex(rows[0][1]) as u32;
        let mut completions = vec![];
        let mut reposts = vec![];
        let mut writes = vec![];
        for row in rows.iter().skip(1) {
            match row[0] {
                "event" => io.event((hex(row[1]) - 0x8000) / 16, bytes(row[2])),
                "completed" => completions.push(row),
                "repost" => reposts.push((hex(row[1]), bytes(row[2]))),
                "write" => writes.push((hex(row[1]), hex(row[2]) as u32)),
                "total" => assert_eq!(row[1].parse::<usize>().unwrap(), total),
                other => panic!("unexpected C output: {other}"),
            }
        }
        assert_eq!(completions.len(), count);
        assert_eq!(reposts.len(), count);
        let before = io.writes.len();
        assert_eq!(q.drain(&mut io), Ok(count));
        for (n, c) in completions.iter().enumerate() {
            let front = q.front().unwrap().unwrap();
            assert_eq!(front.buffer(), c[1].parse::<usize>().unwrap());
            assert_eq!(front.transfer_iova(), hex(c[2]) as u64);
            assert_eq!(front.length(), c[3].parse::<usize>().unwrap());
            assert_eq!(front.code(), c[4].parse::<u8>().unwrap());
            assert_eq!(front.kind(), c[5].parse::<u8>().unwrap());
            assert_eq!(front.event_index(), (total + n) % 256);
            let stores = io.stores;
            assert_eq!(
                q.consume_and_recycle(&mut io, |v| {
                    assert_eq!(v, front);
                    Ok(())
                }),
                Ok(Some(front))
            );
            assert_eq!(io.stores, stores + 4);
            assert_eq!(io.tre((reposts[n].0 - 0x9000) / 16), reposts[n].1);
        }
        assert_eq!(&io.writes[before..], &writes);
        wrapped_credit |= writes.contains(&(EVENT_DOORBELL, 0x80008ff0));
        wrapped_publish |= writes.contains(&(CHANNEL_DOORBELL, 0x80009000));
        assert_eq!(q.front(), Ok(None));
        assert_eq!(q.report().in_flight, 251);
        assert_eq!(q.report().held, 0);
        total += count;
    }
    assert!(total > 2000 && wrapped_credit && wrapped_publish);
    assert_eq!(q.report().events, total as u64);
    assert_eq!(q.report().recycled, total as u64);
    assert_eq!(q.report().descriptor_words_done, (4 * total) as u64);
    assert_eq!(q.report().descriptor_words_checked, (8 * total) as u64);
    assert_eq!(q.report().error, None);
}

#[test]
fn rxevents_producer_bounds_alignment_and_outstanding_limit() {
    for p in [
        0,
        0x7fffffff,
        0x80007ff0,
        0x80008001,
        0x8000800f,
        0x80009000,
        u32::MAX,
    ] {
        let mut q = queue();
        let mut io = Fake::new();
        io.producer = p;
        assert_eq!(q.drain(&mut io), Err(Error::Producer));
        assert_eq!(io.event_reads, 0);
        assert!(io.writes.is_empty());
    }
    for count in 252..256 {
        let mut q = queue();
        let mut io = Fake::new();
        io.push(count);
        assert_eq!(q.drain(&mut io), Err(Error::ExcessEvents));
        assert_eq!(io.event_reads, 0);
        assert!(io.writes.is_empty());
    }
    let mut q = queue();
    let mut io = Fake::new();
    assert_eq!(q.drain(&mut io), Ok(0));
    assert_eq!(io.acquires, 0);
    assert_eq!(q.consume_and_recycle(&mut io, |_| panic!()), Ok(None));
}

#[test]
fn rxevents_full_queue_stalls_without_reusing_held_buffers() {
    let mut q = queue();
    let mut io = Fake::new();
    io.push(251);
    assert_eq!(q.drain(&mut io), Ok(251));
    let held = q.report();
    assert_eq!((held.held, held.in_flight), (251, 0));
    assert_eq!(io.stores, 0);
    assert_eq!(io.writes, [(EVENT_DOORBELL, 0x80008fa0)]);
    assert_eq!(q.drain(&mut io), Ok(0));
    let mut copied = vec![];
    for n in 0..251 {
        let before = io.stores;
        q.consume_and_recycle(&mut io, |c| {
            assert_eq!(c.buffer(), n);
            copied.push(c.buffer());
            Ok(())
        })
        .unwrap();
        assert_eq!(io.stores, before + 4);
    }
    assert_eq!(copied, (0..251).collect::<Vec<_>>());
    assert_eq!((q.report().held, q.report().in_flight), (0, 251));
    assert_eq!(q.report().publish_cursor, 246);
    io.push(251);
    assert_eq!(q.drain(&mut io), Ok(251));
    assert_eq!(q.front().unwrap().unwrap().buffer(), 0);
    io.push(1);
    let reads = io.event_reads;
    assert_eq!(q.drain(&mut io), Err(Error::ExcessEvents));
    assert_eq!(io.event_reads, reads);
}

#[test]
fn rxevents_foreign_stale_skipped_high_or_misaligned_tre_and_length() {
    for pointer in [
        0,
        0x80004000,
        0x80008ff0,
        0x80009001,
        0x80009010,
        0x80009ff0,
        0x8000a000,
        0x180009000,
    ] {
        let mut q = queue();
        let mut io = Fake::new();
        io.push(1);
        let mut raw = event(0);
        raw[..8].copy_from_slice(&u64::to_le_bytes(pointer));
        io.event(0, raw);
        assert_eq!(q.drain(&mut io), Err(Error::Transfer));
        assert!(io.writes.is_empty());
    }
    for ch in 0..=255 {
        if ch == 2 {
            continue;
        }
        let mut q = queue();
        let mut io = Fake::new();
        io.push(1);
        let mut raw = event(0);
        raw[15] = ch;
        io.event(0, raw);
        assert_eq!(q.drain(&mut io), Err(Error::Channel));
    }
    for len in [8129u16, 8192, 16384, 65535] {
        let mut q = queue();
        let mut io = Fake::new();
        io.push(1);
        let mut raw = event(0);
        raw[8..10].copy_from_slice(&len.to_le_bytes());
        io.event(0, raw);
        assert_eq!(q.drain(&mut io), Err(Error::Length));
    }
    // A duplicate TRE after the first accepted event must not return even
    // the preceding valid event's credits or expose that buffer to a reader.
    let mut q = queue();
    let mut io = Fake::new();
    io.push(2);
    io.event(1, event(0));
    assert_eq!(q.drain(&mut io), Err(Error::Transfer));
    assert_eq!(q.report().events, 1);
    assert_eq!(q.front(), Err(Error::Quarantined));
    assert!(io.writes.is_empty());
}

#[test]
fn rxevents_raw_code_type_reserved_bytes_zero_length_are_not_guessed_errors() {
    for byte in 0..=255 {
        let mut q = queue();
        let mut io = Fake::new();
        io.push(1);
        let mut raw = event(0);
        raw[10..15].fill(byte);
        io.event(0, raw);
        assert_eq!(q.drain(&mut io), Ok(1));
        let c = q.front().unwrap().unwrap();
        assert_eq!(
            (c.code(), c.kind(), c.length(), c.raw()),
            (byte, byte, 0, raw)
        );
    }
    let mut q = queue();
    let mut io = Fake::new();
    io.push(1);
    let mut raw = event(0);
    raw[8..10].copy_from_slice(&8128u16.to_le_bytes());
    io.event(0, raw);
    assert_eq!(q.drain(&mut io), Ok(1));
    assert_eq!(q.front().unwrap().unwrap().length(), 8128);
}

#[test]
fn rxevents_every_io_failure_and_uncertain_publication_quarantines_without_retry() {
    let mut q = queue();
    let mut io = Fake::new();
    io.push(1);
    q.drain(&mut io).unwrap();
    q.consume_and_recycle(&mut io, |_| Ok(())).unwrap();
    let calls = io.calls;
    assert!(calls > 25);
    for failed in 1..=calls {
        let mut q = queue();
        let mut io = Fake::new();
        io.push(1);
        io.fail = Some(failed);
        let result = q
            .drain(&mut io)
            .and_then(|_| q.consume_and_recycle(&mut io, |_| Ok(())));
        assert_eq!(result, Err(Error::Io), "call {failed}");
        let before = io.calls;
        assert_eq!(q.front(), Err(Error::Quarantined));
        assert_eq!(q.drain(&mut io), Err(Error::Quarantined));
        assert_eq!(
            q.consume_and_recycle(&mut io, |_| panic!()),
            Err(Error::Quarantined)
        );
        assert_eq!(io.calls, before);
        assert_eq!(q.report().recycled, 0);
        assert_eq!(q.report().error, Some(Error::Io));
    }
}

#[test]
fn rxevents_torn_events_suppressed_stores_and_consumer_failure() {
    for read in 5..=8 {
        let mut q = queue();
        let mut io = Fake::new();
        io.push(1);
        io.torn = Some(read);
        assert_eq!(q.drain(&mut io), Err(Error::TornEvent));
        assert!(io.writes.is_empty());
    }
    for suppressed in 1..=4 {
        let mut q = queue();
        let mut io = Fake::new();
        io.push(1);
        q.drain(&mut io).unwrap();
        // Start with poison even for a zero-valued descriptor word, so
        // omission of every store is independently observable.
        io.tres[251 * 4..252 * 4].fill(0xa5a5a5a5);
        io.suppress = Some(suppressed);
        let releases_before_recycle = io.releases;
        assert_eq!(
            q.consume_and_recycle(&mut io, |_| Ok(())),
            Err(Error::Readback)
        );
        assert_eq!(q.report().channel_doorbells_sent, 0);
        assert_eq!(io.releases, releases_before_recycle);
    }
    let mut q = queue();
    let mut io = Fake::new();
    io.push(1);
    q.drain(&mut io).unwrap();
    assert_eq!(
        q.consume_and_recycle(&mut io, |_| Err(Error::Io)),
        Err(Error::Consumer)
    );
    assert_eq!(io.stores, 0);
    assert_eq!(q.report().held, 1);
    assert_eq!(q.front(), Err(Error::Quarantined));
}

#[test]
fn rxevents_many_cycles_preserve_unique_buffer_membership_and_cursor_wrap() {
    let mut q = queue();
    let mut io = Fake::new();
    let mut total = 0usize;
    let mut byte_total = 0u64;
    for cycle in 0..400 {
        let count = 1 + cycle * 43 % 251;
        io.push(count);
        assert_eq!(q.drain(&mut io), Ok(count));
        let saved_producer = io.producer;
        // No new events at the same snapshot: no duplicate completion.
        assert_eq!(q.drain(&mut io), Ok(0));
        for n in total..total + count {
            let c = q.front().unwrap().unwrap();
            assert_eq!(c.buffer(), n % 251);
            assert_eq!(c.transfer_iova(), 0x80009000 + ((n % 256) * 16) as u64);
            byte_total += c.length() as u64;
            q.consume_and_recycle(&mut io, |_| Ok(())).unwrap();
        }
        assert_eq!(io.producer, saved_producer);
        total += count;
        let r = q.report();
        assert_eq!((r.in_flight, r.held), (251, 0));
        assert_eq!(r.event_cursor, total % 256);
        assert_eq!(r.completion_cursor, total % 256);
        assert_eq!(r.publish_cursor, (251 + total) % 256);
        assert_eq!(r.bytes, byte_total);
    }
    assert!(total > 40000);
    assert_eq!(q.report().events, total as u64);
    assert_eq!(q.report().recycled, total as u64);
}

#[test]
fn rxevents_new_arrivals_during_snapshot_remain_for_next_drain() {
    let mut q = queue();
    let mut io = Fake::new();
    io.push(1);
    io.arrive_on_event_read = Some(1);
    assert_eq!(q.drain(&mut io), Ok(1));
    assert_eq!(io.producer, 0x80008040);
    assert_eq!(io.event_reads, 8);
    assert_eq!(io.writes, [(EVENT_DOORBELL, 0x80008000)]);
    assert_eq!(q.report().held, 1);
    assert_eq!(q.drain(&mut io), Ok(3));
    assert_eq!(q.report().held, 4);
    assert_eq!(io.event_reads, 32);
    assert_eq!(io.writes[1], (EVENT_DOORBELL, 0x80008030));
    for n in 0..4 {
        let c = q.front().unwrap().unwrap();
        assert_eq!(c.buffer(), n);
        q.consume_and_recycle(&mut io, |_| Ok(())).unwrap();
    }
    assert_eq!(q.front(), Ok(None));
    assert_eq!(q.report().in_flight, 251);
    assert_eq!(q.report().events, 4);
}
