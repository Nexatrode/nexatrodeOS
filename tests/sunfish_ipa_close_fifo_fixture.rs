thread_local! {
    static CLOSE_ARRIVAL: std::cell::RefCell<Option<(&'static str, Vec<u8>)>> = const { std::cell::RefCell::new(None) };
    static CLOSE_AFTER_NEXT_WRITE: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
    static GRANT_AFTER_DONE: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
    static MATCHED_REG_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn inject_at_registration_write(peer: &Fake, name: &str, op: &Op) {
    #[cfg(feature = "sunfish-ipa-init-close-visibility")]
    inject_at_init_write(peer, name, op);
    if let Op::Write(_, bytes) = op {
        if let Some(close) = CLOSE_AFTER_NEXT_WRITE.take() {
            peer.push(&close);
        }
        if bytes.first() == Some(&6) && bytes.len() == 8 && GRANT_AFTER_DONE.get() == Some(lo_word(bytes, 4)) {
            GRANT_AFTER_DONE.set(None);
            peer.push(&control(7, 2, 52));
        }
        MATCHED_REG_WRITE.set(bytes.first() == Some(&9) && bytes.get(48..53) == Some(&[2, 0x34, 0x12, 0x20, 0]));
    }
    if MATCHED_REG_WRITE.get() {
        if name == "notify" && CLOSE_ARRIVAL.with_borrow(|p| p.as_ref().is_some_and(|(stage, _)| *stage == "fault")) {
            CLOSE_ARRIVAL.take();
            peer.0.borrow_mut().fail = Some("snapshot");
            return;
        }
        CLOSE_ARRIVAL.with_borrow_mut(|pending| {
            #[cfg(feature = "sunfish-ipa-init-empty-rx")]
            if let Some((stage, _)) = pending.as_mut() {
                // The first cursor snapshot after the doorbell is the fresh
                // registration sample. Expose the close on the following one.
                match (*stage, name) {
                    ("after_fresh", "notify") => { *stage = "fresh_snapshot"; return; }
                    ("fresh_snapshot", "snapshot") => { *stage = "snapshot"; return; }
                    _ => {}
                }
            }
            if pending.as_ref().is_some_and(|(stage, _)| *stage == name) {
                peer.push(&pending.take().unwrap().1);
            }
        });
    }
}


#[cfg(feature = "sunfish-ipa-init-close-visibility")]
thread_local! {
    // The first vector arrives at the INIT doorbell. The second arrives after
    // the first fresh snapshot, at the following validated cursor read.
    static INIT_ARRIVAL: std::cell::RefCell<Option<(Vec<u8>, Vec<u8>, bool)>> = const { std::cell::RefCell::new(None) };
    static INIT_SAMPLE_STAGE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(feature = "sunfish-ipa-init-close-visibility")]
fn inject_at_init_write(peer: &Fake, name: &str, op: &Op) {
    INIT_ARRIVAL.with_borrow_mut(|pending| {
        let Some((first, later, fault)) = pending.as_mut() else { return; };
        if let Op::Write(_, bytes) = op && bytes.first() == Some(&9)
            && bytes.get(48..53) == Some(&[0, 0, 0, 0x21, 0]) && lo_word(bytes, 44) == LO_MODEM
        {
            if INIT_SAMPLE_STAGE.get() == 4 {
                peer.push(first);
                first.clear();
            }
            INIT_SAMPLE_STAGE.set(1);
        }
        match (INIT_SAMPLE_STAGE.get(), name) {
            (1, "notify") => {
                peer.push(first);
                if *fault { peer.0.borrow_mut().fail = Some("snapshot"); }
                INIT_SAMPLE_STAGE.set(2);
            }
            (2, "snapshot") => INIT_SAMPLE_STAGE.set(3),
            (3, "snapshot") => {
                peer.push(later);
                *pending = None;
                INIT_SAMPLE_STAGE.set(0);
            }
            _ => {}
        }
    });
}
