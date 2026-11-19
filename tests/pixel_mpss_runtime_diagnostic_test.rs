#[path = "../rust/src/pixel_mpss_runtime_diagnostic.rs"]
mod diagnostic;

#[test]
fn oack_publish_io_latches_only_the_publication_transition_and_handles_bad_counters() {
    let mut observation = diagnostic::OackPublishIo::default();
    observation.observe(None, Some(5), [1, 2, 3, 4], [11, 22, 33, 44]);
    assert_eq!(observation.ms(0, 1000), u32::MAX);
    observation.observe(Some(1), Some(1), [1, 2, 3, 4], [11, 22, 33, 44]);
    assert_eq!(observation.ms(0, 1000), u32::MAX);
    observation.observe(Some(1), Some(5), [10, 20, 30, 40], [25, 23, 29, u64::MAX]);
    assert_eq!([observation.ms(0, 1000), observation.ms(1, 1000), observation.ms(2, 1000), observation.ms(3, 1000)], [15, 3, u32::MAX, u32::MAX]);
    observation.observe(Some(1), Some(5), [0; 4], [9; 4]);
    assert_eq!(observation.ms(0, 1000), 15, "first captured turn remains latched");
    assert_eq!(observation.ms(0, 0), u32::MAX);
}

#[test]
fn oack_publish_io_supports_six_independent_operation_categories() {
    let mut observation = diagnostic::OackPublishIo::<6>::default();
    observation.observe(Some(1), Some(5), [1, 2, 3, 4, 5, 6], [11, 22, 33, 44, 55, 66]);
    assert_eq!((0..6).map(|i| observation.ms(i, 1000)).collect::<Vec<_>>(), vec![10, 20, 30, 40, 50, 60]);
    assert_eq!(observation.ms(6, 1000), u32::MAX);
}

#[test]
fn oack_domain_times_preserve_missing_samples() {
    let mut observation = diagnostic::OackPublishIo::<2>::default();
    observation.observe(Some(1), Some(5), [20, 30], [45, u64::MAX]);
    assert_eq!(observation.ms(0, 1000), 25);
    assert_eq!(observation.ms(1, 1000), u32::MAX);
}

#[test]
fn first_rrq_classification_distinguishes_known_paths_and_unavailable() {
    assert_eq!(diagnostic::first_rrq_kind(None), 0);
    assert_eq!(diagnostic::first_rrq_kind(Some("/readwrite/ota_firewall/ruleset")), 1);
    assert_eq!(diagnostic::first_rrq_kind(Some("/readwrite/modem_efs")), 2);
    assert_eq!(diagnostic::first_rrq_kind(Some("/readonly/firmware/image/wlanmdsp.mbn")), 3);
    assert_eq!(diagnostic::first_rrq_kind(Some("/readwrite/modem_efs.backup")), 4);
    assert_eq!(diagnostic::first_rrq_kind(Some("")), 4);
}

#[test]
fn observer_path_hex_preserves_case_and_punctuation_in_screen_sized_chunks() {
    let path = b"/readwrite/server_check.txt";
    let mut encoded = String::new();
    for part in 0..3 {
        let (row, len) = diagnostic::path_hex_chunk(path, part);
        assert!(len <= 20);
        encoded.push_str(core::str::from_utf8(&row[..len]).unwrap());
    }
    assert_eq!(encoded, "2F7265616477726974652F7365727665725F636865636B2E747874");
    assert_eq!(diagnostic::path_hex_chunk(path, usize::MAX).1, 0);
}

#[test]
fn receive_failure_preserves_badge_scope_and_nested_error() {
    #[derive(Debug)]
    enum Dispatch { Receive(&'static str) }
    let error = Dispatch::Receive("Guard");
    let Dispatch::Receive(value) = &error;
    assert_eq!(*value, "Guard");
    let failure = diagnostic::ReceiveFailure::capture(8, 0x11, 0x4a, &error);
    assert_eq!((failure.badge, failure.scope, failure.native_error), (8, 0x11, 0x4a));
    assert!(failure.text.line(0).contains("RECEIVE"));
    assert!(failure.text.line(0).contains("GUARD"));
}

#[test]
fn formatter_makes_nested_debug_errors_readable_on_the_pixel_font() {
    let text = diagnostic::ErrorText::from_debug(&"Transport(Session(InvalidReply))");
    assert_eq!(text.line(0), " TRANSPORT SESSION INVAL");
    assert_eq!(text.line(1), "IDREPLY   ");
    assert!(!text.truncated());
}

#[test]
fn formatter_preserves_three_rows_and_reports_truncation() {
    let text = diagnostic::ErrorText::from_debug(&"abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz");
    assert_eq!(text.line(0).len(), 24);
    assert_eq!(text.line(1).len(), 24);
    assert_eq!(text.line(2).len(), 24);
    assert!(text.truncated());
    assert_eq!(text.line(3), "");
}

#[test]
fn nested_control_error_fits_without_losing_its_variant() {
    #[allow(dead_code)]
    #[derive(Debug)] enum Control { ClientGone }
    #[derive(Debug)] enum Session { Control(Control) }
    #[derive(Debug)] enum Transport { Session(Session) }
    let text = diagnostic::ErrorText::from_debug(&Transport::Session(Session::Control(Control::ClientGone)));
    let joined = [text.line(0), text.line(1), text.line(2)].concat();
    assert!(joined.contains("CLIENTGONE"));
    assert!(!text.truncated());
}

#[test]
fn progress_trace_counts_software_wakeups_and_preserves_last_checkpoint() {
    let mut trace = diagnostic::ProgressTrace::default();
    trace.checkpoint(1);
    trace.sent();
    trace.received();
    trace.received();
    trace.checkpoint(2);
    assert_eq!((trace.phase, trace.wake_sent, trace.wake_received), (2, 1, 2));
}

#[test]
fn timing_trace_tracks_maximum_and_total_without_wrapping() {
    let mut timing = diagnostic::TimingTrace::default();
    timing.record(0, 20);
    timing.record(0, 5);
    timing.record(1, 7);
    timing.record(4, 11);
    assert_eq!(timing.total[4], 11);
    assert_eq!(timing.total[0], 25);
    assert_eq!(timing.max[0], 20);
    assert_eq!(timing.total[1], 7);
    timing.record(0, u64::MAX);
    assert_eq!(timing.total[0], u64::MAX);
    assert_eq!(diagnostic::ticks_to_ms(19_200_000, 19_200_000), 1000);
    assert_eq!(diagnostic::ticks_to_ms(1, 0), u32::MAX);
}

#[test]
fn intent_deadline_wakes_until_ten_seconds_without_extending_on_grant() {
    let mut deadline = diagnostic::IntentDeadline::default();
    assert_eq!(deadline.update(0, 5, 10), (false, false));
    assert_eq!(deadline.update(1, 100, 10), (true, false));
    assert_eq!(deadline.update(2, 199, 10), (true, false));
    assert_eq!(deadline.update(2, 200, 10), (false, true));
    assert_eq!(deadline.update(4, 201, 10), (false, false));
    assert_eq!(deadline.update(1, u64::MAX - 40, 10), (true, false));
    assert_eq!(deadline.update(1, 59, 10), (false, true));
    assert_eq!(deadline.update(1, 60, 0), (false, true));
}

#[test]
fn quiet_deadline_wake_never_hides_pending_work_or_physical_interrupts() {
    assert!(diagnostic::intent_deadline_only(1, 256, 256, false, 0));
    assert!(!diagnostic::intent_deadline_only(1, 256, 256, true, 0));
    assert!(!diagnostic::intent_deadline_only(1, 256, 256, false, 16));
    assert!(!diagnostic::intent_deadline_only(1, 272, 256, false, 0));
    assert!(!diagnostic::intent_deadline_only(4, 256, 256, false, 0));
}

#[test]
fn ipa_flags_preserve_core_presence_and_each_handshake_milestone() {
    assert_eq!(diagnostic::ipa_flags(false, false, false, false, None), 0);
    assert_eq!(diagnostic::ipa_flags(true, true, false, false,
        Some([true, false, true, false, true, false])), 0x553);
    assert_eq!(diagnostic::ipa_flags(false, false, true, true,
        Some([true, true, true, true, false, false])), 0x4fc);
}

#[test]
fn repaint_tracks_ipa_changes_without_repainting_idle_continuations() {
    const CONTINUATION: u64 = 1 << 8;
    assert!(!diagnostic::should_repaint(0, CONTINUATION, CONTINUATION, false));
    assert!(diagnostic::should_repaint(0, CONTINUATION, CONTINUATION, true));
    assert!(diagnostic::should_repaint(1, CONTINUATION, CONTINUATION, false));
    assert!(diagnostic::should_repaint(0, 1 << 4, CONTINUATION, false));
    assert!(!diagnostic::should_repaint(0, 0, CONTINUATION, false));
}


#[test]
fn oack_paint_gate_throttles_routine_updates_but_not_errors_or_completion() {
    let mut gate = diagnostic::OackPaintGate::default();
    assert!(gate.allow(false, false, 0, 100));
    assert!(!gate.allow(true, false, 100, 100));
    assert!(!gate.allow(true, false, 599, 100));
    assert!(gate.allow(true, false, 600, 100));
    assert!(!gate.allow(true, false, 601, 100));
    assert!(gate.allow(true, true, 602, 100));
    assert!(!gate.allow(true, false, 603, 100));
    assert!(gate.allow(false, false, 604, 100));
    assert!(!gate.allow(true, false, 605, 100));
    assert!(gate.allow(true, false, 606, 0), "invalid clock fails open for display");
    assert!(gate.allow(true, false, 607, u64::MAX), "overflow fails open for display");
    assert!(!gate.allow(true, false, 900, 100));
    assert!(gate.allow(true, false, 800, 100), "backwards counter permits a refresh");
    let mut wrap = diagnostic::OackPaintGate::default();
    assert!(!wrap.allow(true, false, u64::MAX - 100, 100));
    assert!(!wrap.allow(true, false, 398, 100));
    assert!(wrap.allow(true, false, 399, 100));
}

#[test]
fn oack_phase_snapshot_uses_first_request_and_never_overwrites_boundaries() {
    let mut phases = diagnostic::OackPhases::default();
    phases.mark(1, 1010);
    phases.begin(None, 1015);
    assert_eq!(phases.age_ms(0, 1000), u32::MAX);
    phases.begin(Some(1000), 1020);
    phases.mark(1, 1100);
    phases.mark(2, 1200);
    phases.mark(3, 1300);
    phases.begin(Some(2000), 2020);
    phases.mark(1, 2100);
    assert_eq!((0..4).map(|i| phases.age_ms(i, 1000)).collect::<Vec<_>>(), [20, 100, 200, 300]);
    assert_eq!(phases.age_ms(4, 1000), u32::MAX);
    assert_eq!(phases.age_ms(0, 0), u32::MAX);
    let mut reversed = diagnostic::OackPhases::default();
    reversed.begin(Some(500), 499);
    assert_eq!(reversed.age_ms(0, 1000), u32::MAX);
}

#[cfg(feature = "sunfish-wlan-elf-registry-gap")]
#[test]
fn registry_gap_attributes_longest_interval_and_next_notification() {
    let mut trace = diagnostic::RegistryGap::default();
    trace.begin_callback(90, 0x10);
    trace.enter_registry(100, [5, 0, 0, 0, 0]);
    assert!(trace.longest.is_none());
    trace.end_callback(120);

    trace.begin_callback(170, 0x100);
    trace.enter_registry(200, [20, 5, 7, 3, 4]);
    let sample = trace.longest.as_ref().unwrap();
    assert_eq!(sample.elapsed, 100);
    assert_eq!(sample.work, [15, 5, 7, 3, 4]);
    assert_eq!(sample.outside, 50);
    assert_eq!(sample.unaccounted, 16);
    assert_eq!(sample.notifications, 1);
    assert_eq!(sample.next_badge, 0x100);

    trace.enter_registry(220, [20, 5, 7, 3, 10]);
    assert_eq!(trace.longest.as_ref().unwrap().elapsed, 100);
}

#[cfg(feature = "sunfish-wlan-elf-gap-io")]
#[test]
fn registry_gap_io_follows_the_same_interval_and_preserves_invalid_samples() {
    let mut trace = diagnostic::RegistryGap::default();
    trace.record_router_io([0; 12], [10; 12]);
    trace.enter_registry(100, [0; 5]);
    trace.record_router_io([10; 12], [25; 12]);
    trace.enter_registry(200, [0; 5]);
    assert_eq!(trace.longest.as_ref().unwrap().router_io, [15; 12]);

    trace.record_router_io([25; 12], [30; 12]);
    trace.enter_registry(220, [0; 5]);
    assert_eq!(trace.longest.as_ref().unwrap().router_io, [15; 12]);

    let mut after = [35; 12];
    after[2] = 20;
    after[10] = u64::MAX;
    trace.record_router_io([30; 12], after);
    trace.enter_registry(400, [0; 5]);
    let sample = trace.longest.as_ref().unwrap();
    assert_eq!(sample.elapsed, 180);
    assert_eq!(sample.router_io[0], 5);
    assert_eq!(sample.router_io[2], u64::MAX);
    assert_eq!(sample.router_io[10], u64::MAX);
}

#[cfg(feature = "sunfish-wlan-elf-phase-io")]
#[test]
fn elf_phase_io_preserves_absent_saturated_and_reversed_counters() {
    assert_eq!(diagnostic::elf_io_delta(None, Some([5; 12])), [u64::MAX; 12]);
    assert_eq!(diagnostic::elf_io_delta(Some([5; 12]), None), [u64::MAX; 12]);
    assert_eq!(diagnostic::elf_io_delta(Some([5; 12]), Some([5; 12])), [0; 12]);
    let mut before = [5; 12]; let mut after = [15; 12];
    before[1] = u64::MAX; after[2] = u64::MAX; after[3] = 4;
    let mut expected = [10; 12]; expected[1..4].fill(u64::MAX);
    assert_eq!(diagnostic::elf_io_delta(Some(before), Some(after)), expected);
}

#[test]
fn ipa_schedule_splits_real_milestones_without_double_counting_and_freezes() {
    let mut t = diagnostic::IpaScheduleTiming::default();
    t.observe(0, 7, [None; 5]);
    t.observe(20, 2, [Some(10), None, None, None, None]);
    t.observe(35, 0, [Some(10), Some(30), None, None, None]);
    t.observe(50, 3, [Some(10), Some(30), None, None, None]);
    t.observe(70, 6, [Some(10), Some(30), None, None, None]);
    t.observe(90, 0, [Some(10), Some(30), Some(80), None, None]);
    t.observe(120, 1, [Some(10), Some(30), Some(80), None, None]);
    t.observe(150, 2, [Some(10), Some(30), Some(80), Some(130), Some(140)]);
    assert!(!t.invalid);
    assert_eq!(t.work[0], [10, 0, 10, 0, 0, 0, 0, 0]);
    assert_eq!(t.work[1], [15, 0, 0, 15, 0, 0, 20, 0]);
    assert_eq!(t.work[2], [10, 30, 10, 0, 0, 0, 0, 0]);
    assert_eq!(t.work[3], [0, 0, 10, 0, 0, 0, 0, 0]);
    assert_eq!(t.phase_ms(0, 1000), 20);
    assert_eq!(t.phase_ms(1, 1000), 50);
    assert_eq!(t.phase_ms(2, 1000), 50);
    assert_eq!(t.phase_ms(3, 1000), 10);
    assert_eq!(t.packed_ms(0, 1000), (10 << 16) | 25);
    let frozen = t.work;
    t.observe(1000, 7, [Some(10), Some(30), Some(80), Some(130), Some(140)]);
    assert_eq!(t.work, frozen);
}

#[test]
fn ipa_schedule_marks_missing_late_or_reversed_evidence_and_saturates_display() {
    let mut t = diagnostic::IpaScheduleTiming::default();
    assert_eq!(t.packed_ms(0, 1000), u32::MAX);
    t.observe(0, 7, [None; 5]);
    t.observe(100_000, 0, [Some(1), Some(90_000), None, None, None]);
    assert_eq!(t.packed_ms(0, 1000), (0xfffe << 16) | 10_000);
    assert_eq!(t.phase_ms(1, 1000), u32::MAX);
    assert_eq!(t.packed_ms(0, 0), u32::MAX);
    assert_eq!(diagnostic::packed_counts(0x10000, 22), 0xffff0016);
    for milestones in [
        [Some(10), Some(9), None, None, None],
        [None, Some(20), None, None, None],
        [Some(10), Some(200), None, None, None],
    ] {
        let mut t = diagnostic::IpaScheduleTiming::default();
        t.observe(0, 7, [None; 5]);
        t.observe(100, 0, milestones);
        assert!(t.invalid);
        assert_eq!(t.packed_ms(0, 1000), u32::MAX);
    }
    let mut t = diagnostic::IpaScheduleTiming::default();
    t.observe(20, 7, [None; 5]);
    t.observe(30, 0, [Some(10), None, None, None, None]);
    assert!(t.invalid, "late-discovered boundary must not silently misattribute time");
    let mut t = diagnostic::IpaScheduleTiming::default();
    t.observe(u64::MAX, 7, [None; 5]);
    t.observe(0, 0, [None; 5]);
    assert!(t.invalid, "counter discontinuity is not a huge elapsed interval");
}
