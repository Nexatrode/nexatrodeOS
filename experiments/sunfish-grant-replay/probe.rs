// Host replay instrumentation. One count is one cursor sample.
thread_local! {
    static COUNT: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
    static ANCHOR: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
    static DEADLINE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static FRAME: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
    static PHASES: std::cell::RefCell<Vec<(&'static str, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
    static WRITE_LABEL: std::cell::Cell<&'static str> = const { std::cell::Cell::new("head") };
}
fn mark(stage: &'static str) {
    if let Some(n) = COUNT.get() {
        PHASES.with_borrow_mut(|events| events.push((stage, n)));
    }
}
fn start_window() {
    mark("init-fresh");
}
fn replay_io(peer: &Fake, op: &Op) {
    let Some(n) = COUNT.get() else { return };
    match op {
        Op::Write(_, bytes) => WRITE_LABEL.set(match bytes[0] {
            9 if bytes.get(48..53) == Some(&[2, 0x34, 0x12, 0x20, 0]) => "reg-head",
            9 if bytes.get(48..53) == Some(&[0, 0, 0, 0x21, 0])
                && lo_word(bytes, 44) == LO_MODEM =>
            {
                "init-head"
            }
            5 if bytes.len() == 16 && lo_word(bytes, 4) == 1 && lo_word(bytes, 8) >= 52 => {
                "ad-head"
            }
            8 if bytes.len() == 8 && lo_word(bytes, 4) == 1 => "ack-head",
            9 if bytes.get(48..50) == Some(&[0, 6]) && lo_word(bytes, 44) == 0x1f => "oack-head",
            _ => "head",
        }),
        Op::Head(_) => {
            let label = WRITE_LABEL.get();
            if label == "reg-head" {
                ANCHOR.set(Some(n));
            }
            mark(label);
        }
        Op::Snapshot => {
            COUNT.set(Some(n + 1));
            if ANCHOR.get().is_some_and(|at| n + 1 == at + DEADLINE.get())
                && let Some(frame) = FRAME.take()
            {
                peer.push(&frame);
                mark("injected");
            }
        }
        _ => {}
    }
}
