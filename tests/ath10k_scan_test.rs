#![allow(dead_code)]
#![forbid(unsafe_code)]

#[path = "../rust/src/ath10k_ce.rs"]
mod ath10k_ce;
#[path = "../rust/src/ath10k_nwifi_rx.rs"]
mod ath10k_nwifi_rx;
#[path = "../rust/src/ath10k_scan.rs"]
mod ath10k_scan;
#[path = "../rust/src/ath10k_wmi_tx.rs"]
mod ath10k_wmi_tx;

use ath10k_scan::{
    Error, LINUX_SCAN_ID, LINUX_SCAN_NOTIFY_EVENTS, LINUX_SCAN_PRIORITY_LOW,
    LINUX_SCAN_REQUESTOR_ID, MAX_SCAN_BSSIDS, MAX_SCAN_CHANNELS, MAX_SCAN_IE_BYTES, MAX_SCAN_SSIDS,
    MAX_SSID_BYTES, QueueError, STOP_SCAN_COMMAND_BYTES, ScanEvent, ScanEventAction, ScanEventKind,
    ScanPhase, ScanSession, StartScanRequest, WMI_10_4_START_SCAN_COMMAND_ID,
    WMI_10_4_STOP_SCAN_COMMAND_ID, WMI_SCAN_CHAN_STAT_EVENT, WMI_SCAN_EVENT_BYTES,
    WMI_SCAN_EVENT_COMPLETED, WMI_SCAN_EVENT_FOREIGN_CHANNEL, WMI_SCAN_EVENT_START_FAILED,
    WMI_SCAN_EVENT_STARTED, WMI_SCAN_FLAG_PASSIVE, encode_start_scan, encode_stop_scan,
    parse_scan_event, queue_start_scan, queue_stop_scan, start_scan_payload_len,
};

const NO_CHANNELS: [u16; 0] = [];
const NO_SSIDS: [&[u8]; 0] = [];
const NO_BSSIDS: [[u8; 6]; 0] = [];
const NO_IES: [u8; 0] = [];

fn request() -> StartScanRequest<'static> {
    StartScanRequest {
        scan_id: 2,
        requestor_id: 3,
        vdev_id: 4,
        priority: 2,
        notify_scan_events: 5,
        active_dwell_ms: 40,
        passive_dwell_ms: 60,
        min_rest_ms: 10,
        max_rest_ms: 20,
        repeat_probe_ms: 30,
        probe_spacing_ms: 5,
        idle_ms: 7,
        max_scan_ms: 500,
        probe_delay_ms: 2,
        scan_control_flags: 0x1234,
        channels_mhz: &NO_CHANNELS,
        ssids: &NO_SSIDS,
        bssids: &NO_BSSIDS,
        extra_information_elements: &NO_IES,
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn event(event_type: u32, reason: u32, channel_frequency_mhz: u32) -> ScanEvent {
    ScanEvent {
        event_type,
        reason,
        channel_frequency_mhz,
        requestor_id: 0xa003,
        scan_id: 0xa002,
        vdev_id: 4,
    }
}

fn wmi_control_queue(initial_credits: u16) -> ath10k_wmi_tx::WmiCommandTxQueue<4> {
    ath10k_wmi_tx::WmiCommandTxQueue::new_wmi_control(
        ath10k_ce::HtcEndpoint {
            service_id: ath10k_ce::HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 128,
        },
        ath10k_ce::HtcServicePolicy {
            receive_allocation: u8::try_from(initial_credits).unwrap(),
            disable_credit_flow: false,
            credit_size: 16,
            initial_credits,
        },
        false,
        9,
    )
    .unwrap()
}

#[test]
fn linux_reference_profile_uses_pinned_defaults_and_passive_empty_ssids() {
    let channels = [2412, 5180];
    let ssid = b"lab";
    let ssids: [&[u8]; 1] = [ssid];
    let active = StartScanRequest::linux_reference_defaults(7, &channels, &ssids, &NO_IES);

    assert_eq!(active.scan_id, LINUX_SCAN_ID);
    assert_eq!(active.requestor_id, LINUX_SCAN_REQUESTOR_ID);
    assert_eq!(active.vdev_id, 7);
    assert_eq!(active.priority, LINUX_SCAN_PRIORITY_LOW);
    assert_eq!(active.notify_scan_events, LINUX_SCAN_NOTIFY_EVENTS);
    assert_eq!(active.active_dwell_ms, 50);
    assert_eq!(active.passive_dwell_ms, 150);
    assert_eq!((active.min_rest_ms, active.max_rest_ms), (50, 500));
    assert_eq!(
        (
            active.repeat_probe_ms,
            active.probe_spacing_ms,
            active.idle_ms
        ),
        (0, 0, 0)
    );
    assert_eq!((active.max_scan_ms, active.probe_delay_ms), (20_000, 5));
    assert_eq!(active.scan_control_flags, WMI_SCAN_CHAN_STAT_EVENT);
    assert_eq!(active.bssids, &[[0xff; 6]]);

    let passive = StartScanRequest::linux_reference_defaults(7, &channels, &NO_SSIDS, &NO_IES);
    assert_eq!(
        passive.scan_control_flags,
        WMI_SCAN_CHAN_STAT_EVENT | WMI_SCAN_FLAG_PASSIVE
    );
}

#[test]
fn scan_session_correlates_events_and_completes_once() {
    let mut session = ScanSession::new();
    assert_eq!(session.phase(), ScanPhase::Idle);
    session.reserve_start(&request()).unwrap();
    assert_eq!(session.phase(), ScanPhase::Starting);

    let mut stale = event(WMI_SCAN_EVENT_STARTED, 0, 0);
    stale.vdev_id += 1;
    assert_eq!(session.observe_event(stale), ScanEventAction::Unmatched);
    assert_eq!(session.phase(), ScanPhase::Starting);

    assert_eq!(
        session.observe_event(event(WMI_SCAN_EVENT_STARTED, 0, 0)),
        ScanEventAction::Started
    );
    assert_eq!(session.phase(), ScanPhase::Running);
    assert_eq!(
        session.observe_event(event(WMI_SCAN_EVENT_FOREIGN_CHANNEL, 0, 5180)),
        ScanEventAction::ForeignChannel {
            frequency_mhz: 5180
        }
    );
    assert_eq!(
        session.observe_event(event(WMI_SCAN_EVENT_COMPLETED, 7, 0)),
        ScanEventAction::Completed {
            reason: 7,
            aborted: false
        }
    );
    assert_eq!(session.phase(), ScanPhase::Idle);
    assert_eq!(
        session.observe_event(event(WMI_SCAN_EVENT_COMPLETED, 7, 0)),
        ScanEventAction::Unmatched
    );
}

#[test]
fn scan_session_rejects_overlap_and_preserves_start_failure_reason() {
    let mut session = ScanSession::new();
    session.reserve_start(&request()).unwrap();
    assert_eq!(
        session.reserve_start(&request()),
        Err(Error::ScanAlreadyActive)
    );
    assert_eq!(
        session.observe_event(event(WMI_SCAN_EVENT_COMPLETED, 0, 0)),
        ScanEventAction::UnexpectedPhase
    );
    assert_eq!(session.phase(), ScanPhase::Starting);
    assert_eq!(
        session.observe_event(event(WMI_SCAN_EVENT_START_FAILED, 9, 0)),
        ScanEventAction::StartFailed { reason: 9 }
    );
    assert_eq!(session.phase(), ScanPhase::Idle);
}

#[test]
fn scan_session_releases_start_reservation_when_command_submission_fails() {
    let mut session = ScanSession::new();
    assert_eq!(session.submission_failed(), Err(Error::ScanNotStarting));
    session.reserve_start(&request()).unwrap();
    assert_eq!(session.phase(), ScanPhase::Starting);
    session.submission_failed().unwrap();
    assert_eq!(session.phase(), ScanPhase::Idle);

    session.reserve_start(&request()).unwrap();
    session.observe_event(event(WMI_SCAN_EVENT_STARTED, 0, 0));
    assert_eq!(session.submission_failed(), Err(Error::ScanNotStarting));
    assert_eq!(session.phase(), ScanPhase::Running);
}

#[test]
fn scan_session_marks_abort_only_after_started_and_finishes_as_aborted() {
    let mut session = ScanSession::new();
    assert_eq!(session.mark_abort_requested(), Err(Error::ScanNotRunning));
    session.reserve_start(&request()).unwrap();
    assert_eq!(session.mark_abort_requested(), Err(Error::ScanNotRunning));
    session.observe_event(event(WMI_SCAN_EVENT_STARTED, 0, 0));
    session.mark_abort_requested().unwrap();
    assert_eq!(session.phase(), ScanPhase::Aborting);
    assert_eq!(
        session.mark_abort_requested(),
        Err(Error::ScanAlreadyAborting)
    );
    assert_eq!(
        session.observe_event(event(WMI_SCAN_EVENT_COMPLETED, 1, 0)),
        ScanEventAction::Completed {
            reason: 1,
            aborted: true
        }
    );
    assert_eq!(session.phase(), ScanPhase::Idle);
}

#[test]
fn stop_scan_encoder_uses_active_ids_and_preserves_output_on_error() {
    let mut session = ScanSession::new();
    session.reserve_start(&request()).unwrap();
    session.observe_event(event(WMI_SCAN_EVENT_STARTED, 0, 0));
    assert_eq!(
        encode_stop_scan(&session, &mut [0xa5; STOP_SCAN_COMMAND_BYTES]),
        Err(Error::ScanNotRunning)
    );
    session.mark_abort_requested().unwrap();

    let mut short = [0xa5; STOP_SCAN_COMMAND_BYTES - 1];
    assert_eq!(
        encode_stop_scan(&session, &mut short),
        Err(Error::OutputTooSmall)
    );
    assert!(short.iter().all(|byte| *byte == 0xa5));

    let mut output = [0xa5; STOP_SCAN_COMMAND_BYTES + 4];
    let encoded = encode_stop_scan(&session, &mut output).unwrap();
    assert_eq!(encoded.command_id, WMI_10_4_STOP_SCAN_COMMAND_ID);
    assert_eq!(encoded.payload_bytes, STOP_SCAN_COMMAND_BYTES);
    assert_eq!(
        [
            word(&output, 0),
            word(&output, 4),
            word(&output, 8),
            word(&output, 12)
        ],
        [0xa003, 0xa002, 0, 4]
    );
    assert_eq!(&output[STOP_SCAN_COMMAND_BYTES..], &[0xa5; 4]);
}

#[test]
fn scan_session_ignores_nonterminal_and_unknown_events_without_state_changes() {
    let mut session = ScanSession::new();
    session.reserve_start(&request()).unwrap();
    let mut ignored = event(0xfeed, 0, 0);
    ignored.vdev_id = 4;
    assert_eq!(session.observe_event(ignored), ScanEventAction::Ignored);
    assert_eq!(session.phase(), ScanPhase::Starting);
    session.observe_event(event(WMI_SCAN_EVENT_STARTED, 0, 0));
    assert_eq!(
        session.observe_event(event(0xfeed, 0, 0)),
        ScanEventAction::Ignored
    );
    assert_eq!(session.phase(), ScanPhase::Running);
}

#[test]
fn emits_fixed_wmi_10_4_scan_body_and_host_id_prefixes() {
    let mut output = [0xa5; 80];
    let layout = encode_start_scan(&request(), &mut output).unwrap();

    assert_eq!(layout.command_id, WMI_10_4_START_SCAN_COMMAND_ID);
    assert_eq!(layout.payload_bytes, 64);
    let expected = [
        0xa002, 0xa003, 4, 2, 5, 40, 60, 10, 20, 30, 5, 7, 500, 2, 0x1234, 0,
    ];
    for (index, value) in expected.into_iter().enumerate() {
        assert_eq!(word(&output, index * 4), value);
    }
    assert!(output[64..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn encodes_optional_lists_in_source_order_with_zero_padding() {
    let channels = [2412, 5180];
    let ssid = b"net";
    let ssids: [&[u8]; 1] = [ssid];
    let bssid = [[0x02, 1, 2, 3, 4, 5]];
    let ies = [0xdd, 2, 0xaa, 0xbb];
    let mut scan = request();
    scan.channels_mhz = &channels;
    scan.ssids = &ssids;
    scan.bssids = &bssid;
    scan.extra_information_elements = &ies;
    let mut output = [0xa5; 160];

    let layout = encode_start_scan(&scan, &mut output).unwrap();
    assert_eq!(layout.payload_bytes, 152);
    assert_eq!(word(&output, 64), 1); // Channel-list tag.
    assert_eq!(word(&output, 68), 2);
    assert_eq!(&output[72..80], &[0x6c, 0x09, 0, 0, 0x3c, 0x14, 0, 0]);
    assert_eq!(word(&output, 80), 2); // SSID-list tag.
    assert_eq!(word(&output, 84), 1);
    assert_eq!(word(&output, 88), 3);
    assert_eq!(&output[92..95], b"net");
    assert!(output[95..124].iter().all(|byte| *byte == 0));
    assert_eq!(word(&output, 124), 3); // BSSID-list tag.
    assert_eq!(word(&output, 128), 1);
    assert_eq!(&output[132..140], &[0x02, 1, 2, 3, 4, 5, 0, 0]);
    assert_eq!(word(&output, 140), 4); // Extra-IE tag.
    assert_eq!(word(&output, 144), 4);
    assert_eq!(&output[148..152], &ies);
    assert!(output[152..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn empty_lists_are_omitted_and_zero_length_ssid_is_valid() {
    let wildcard_ssid: [&[u8]; 1] = [&[]];
    let mut scan = request();
    scan.ssids = &wildcard_ssid;
    let mut output = [0; 112];

    let layout = encode_start_scan(&scan, &mut output).unwrap();
    assert_eq!(layout.payload_bytes, 108);
    assert_eq!(word(&output, 64), 2);
    assert_eq!(word(&output, 68), 1);
    assert_eq!(word(&output, 72), 0);
    assert!(output[76..108].iter().all(|byte| *byte == 0));
}

#[test]
fn validation_rejects_out_of_range_ids_priority_and_resource_counts() {
    let mut scan = request();
    scan.scan_id = 0x1000;
    assert_eq!(start_scan_payload_len(&scan), Err(Error::ScanIdOutOfRange));

    scan = request();
    scan.requestor_id = 0x1000;
    assert_eq!(
        start_scan_payload_len(&scan),
        Err(Error::RequestorIdOutOfRange)
    );

    scan = request();
    scan.priority = 5;
    assert_eq!(
        start_scan_payload_len(&scan),
        Err(Error::PriorityOutOfRange)
    );

    let channels = [2412; MAX_SCAN_CHANNELS + 1];
    scan = request();
    scan.channels_mhz = &channels;
    assert_eq!(start_scan_payload_len(&scan), Err(Error::TooManyChannels));

    let ssids = [b"x".as_slice(); MAX_SCAN_SSIDS + 1];
    scan = request();
    scan.ssids = &ssids;
    assert_eq!(start_scan_payload_len(&scan), Err(Error::TooManySsids));

    let bssids = [[2, 0, 0, 0, 0, 1]; MAX_SCAN_BSSIDS + 1];
    scan = request();
    scan.bssids = &bssids;
    assert_eq!(start_scan_payload_len(&scan), Err(Error::TooManyBssids));

    let ies = [0; MAX_SCAN_IE_BYTES + 1];
    scan = request();
    scan.extra_information_elements = &ies;
    assert_eq!(
        start_scan_payload_len(&scan),
        Err(Error::TooManyInformationElementBytes)
    );
}

#[test]
fn validation_rejects_invalid_channel_ssid_and_information_elements() {
    let channels = [0];
    let mut scan = request();
    scan.channels_mhz = &channels;
    assert_eq!(
        start_scan_payload_len(&scan),
        Err(Error::InvalidChannelFrequency)
    );

    let long_ssid = [b'x'; MAX_SSID_BYTES + 1];
    let ssids: [&[u8]; 1] = [&long_ssid];
    scan = request();
    scan.ssids = &ssids;
    assert_eq!(start_scan_payload_len(&scan), Err(Error::SsidTooLong));

    let malformed_ies = [0xdd, 3, 1];
    scan = request();
    scan.extra_information_elements = &malformed_ies;
    assert_eq!(
        start_scan_payload_len(&scan),
        Err(Error::MalformedInformationElements)
    );
}

#[test]
fn errors_leave_output_unchanged() {
    let mut output = [0xa5; 63];
    assert_eq!(
        encode_start_scan(&request(), &mut output),
        Err(Error::OutputTooSmall)
    );
    assert!(output.iter().all(|byte| *byte == 0xa5));

    let malformed_ies = [0xdd, 2, 1];
    let mut scan = request();
    scan.extra_information_elements = &malformed_ies;
    let mut large_output = [0xa5; 128];
    assert_eq!(
        encode_start_scan(&scan, &mut large_output),
        Err(Error::MalformedInformationElements)
    );
    assert!(large_output.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn scan_event_parser_decodes_fixed_fields_and_preserves_unknown_types() {
    let mut bytes = [0; WMI_SCAN_EVENT_BYTES + 4];
    for (index, value) in [WMI_SCAN_EVENT_FOREIGN_CHANNEL, 3, 5180, 0xa002, 0xa003, 4]
        .into_iter()
        .enumerate()
    {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[WMI_SCAN_EVENT_BYTES..].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);

    let event = parse_scan_event(&bytes).unwrap();
    assert_eq!(event.kind(), Some(ScanEventKind::ForeignChannel));
    assert_eq!(event.reason, 3);
    assert_eq!(event.channel_frequency_mhz, 5180);
    assert_eq!(event.requestor_id, 0xa002);
    assert_eq!(event.scan_id, 0xa003);
    assert_eq!(event.vdev_id, 4);

    bytes[..4].copy_from_slice(&0x8000_0001u32.to_le_bytes());
    let unknown = parse_scan_event(&bytes).unwrap();
    assert_eq!(unknown.event_type, 0x8000_0001);
    assert_eq!(unknown.kind(), None);
}

#[test]
fn scan_event_parser_rejects_every_truncated_fixed_prefix() {
    let bytes = [0; WMI_SCAN_EVENT_BYTES];
    for length in 0..WMI_SCAN_EVENT_BYTES {
        assert_eq!(
            parse_scan_event(&bytes[..length]),
            Err(Error::ScanEventTooShort)
        );
    }

    let event = parse_scan_event(&bytes).unwrap();
    assert_eq!(event.event_type, 0);
    assert_eq!(event.kind(), None);
    assert_ne!(WMI_SCAN_EVENT_STARTED, WMI_SCAN_EVENT_FOREIGN_CHANNEL);
}

#[test]
fn start_scan_body_composes_into_the_upstream_ath10k_wmi_htc_frame() {
    let mut body = [0; 64];
    let encoded = encode_start_scan(&request(), &mut body).unwrap();
    let mut frame = [0xa5; 96];
    let mut queue =
        ath10k_wmi_tx::WmiCommandTxQueue::<4>::new(2, 128, 16, 8, true, false, 9).unwrap();
    let queued = queue
        .queue_transfer(
            encoded.command_id,
            &body[..encoded.payload_bytes],
            0x1_2345_6000,
            3,
            0x1234,
            false,
            false,
            &mut frame,
        )
        .unwrap();
    let transfer = queued.transfer;

    assert_eq!(queued.slot, 0);
    assert_eq!(transfer.tx.frame_bytes, 76);
    assert_eq!(transfer.tx.wmi_payload_bytes, 68);
    assert_eq!(transfer.tx.credits_required, 5);
    assert_eq!(transfer.tx.sequence_number, 9);
    assert_eq!(&frame[0..2], &[2, 1]);
    assert_eq!(&frame[2..4], &[68, 0]);
    assert_eq!(frame[5], 9);
    assert_eq!(&frame[8..12], &WMI_10_4_START_SCAN_COMMAND_ID.to_le_bytes());
    assert_eq!(&frame[12..76], &body);
    assert!(frame[76..].iter().all(|byte| *byte == 0xa5));
    assert_eq!(
        &transfer.descriptor.as_bytes()[..12],
        &[
            0x00, 0x60, 0x45, 0x23, 0x01, 0x00, 0x00, 0x00, 0x4c, 0x00, 0x30, 0x00
        ]
    );
    assert_eq!(queue.credits_available(), Ok(3));
    assert_eq!(queue.next_sequence_number(), 10);
    assert_eq!(queue.ring_indices(), (1, 0));
    let mut contexts = [0; 1];
    assert_eq!(queue.reclaim_to(1, &mut contexts), Ok(1));
    assert_eq!(contexts, [0x1234]);
    assert_eq!(queue.credits_available(), Ok(3));
}

#[test]
fn queues_start_scan_on_wmi_control_and_reserves_scan_state() {
    let mut session = ScanSession::new();
    let mut queue = wmi_control_queue(8);
    let mut frame = [0xa5; 128];

    let queued = queue_start_scan(
        &mut session,
        &mut queue,
        &request(),
        0x1_2345_6000,
        3,
        0x1234,
        &mut frame,
    )
    .unwrap();

    assert_eq!(queued.command.command_id, WMI_10_4_START_SCAN_COMMAND_ID);
    assert_eq!(queued.command.payload_bytes, 64);
    assert_eq!(queued.transfer.slot, 0);
    assert_eq!(queued.transfer.transfer.tx.frame_bytes, 76);
    assert_eq!(queued.transfer.transfer.tx.wmi_payload_bytes, 68);
    assert_eq!(queued.transfer.transfer.tx.sequence_number, 9);
    assert_eq!(&frame[8..12], &WMI_10_4_START_SCAN_COMMAND_ID.to_le_bytes());
    assert_eq!(word(&frame[12..], 0), 0xa002);
    assert_eq!(word(&frame[12..], 4), 0xa003);
    assert_eq!(word(&frame[12..], 8), 4);
    assert_eq!(session.phase(), ScanPhase::Starting);
    assert_eq!(queue.next_sequence_number(), 10);
    assert_eq!(queue.credits_available(), Ok(3));
    assert_eq!(queue.ring_indices(), (1, 0));
    assert!(frame[76..].iter().all(|byte| *byte == 0xa5));
}

#[test]
fn queues_matching_stop_scan_after_start_and_keeps_abort_pending() {
    let mut session = ScanSession::new();
    let mut queue = wmi_control_queue(8);
    let mut frame = [0xa5; 128];
    queue_start_scan(
        &mut session,
        &mut queue,
        &request(),
        0x1_2345_6000,
        3,
        0x1234,
        &mut frame,
    )
    .unwrap();
    session.observe_event(event(WMI_SCAN_EVENT_STARTED, 0, 0));
    assert_eq!(session.phase(), ScanPhase::Running);

    let queued = queue_stop_scan(
        &mut session,
        &mut queue,
        0x1_2345_7000,
        4,
        0x5678,
        &mut frame,
    )
    .unwrap();

    assert_eq!(queued.command.command_id, WMI_10_4_STOP_SCAN_COMMAND_ID);
    assert_eq!(queued.command.payload_bytes, STOP_SCAN_COMMAND_BYTES);
    assert_eq!(queued.transfer.slot, 1);
    assert_eq!(queued.transfer.transfer.tx.frame_bytes, 28);
    assert_eq!(queued.transfer.transfer.tx.wmi_payload_bytes, 20);
    assert_eq!(queued.transfer.transfer.tx.credits_required, 2);
    assert_eq!(queued.transfer.transfer.tx.sequence_number, 10);
    assert_eq!(&frame[0..2], &[3, 1]);
    assert_eq!(&frame[2..4], &[20, 0]);
    assert_eq!(frame[5], 10);
    assert_eq!(&frame[8..12], &WMI_10_4_STOP_SCAN_COMMAND_ID.to_le_bytes());
    assert_eq!(
        [
            word(&frame, 12),
            word(&frame, 16),
            word(&frame, 20),
            word(&frame, 24)
        ],
        [0xa003, 0xa002, 0, 4]
    );
    assert_eq!(session.phase(), ScanPhase::Aborting);
    assert_eq!(queue.credits_available(), Ok(1));
    assert_eq!(queue.next_sequence_number(), 11);
    assert_eq!(queue.ring_indices(), (2, 0));

    assert_eq!(
        session.observe_event(event(WMI_SCAN_EVENT_COMPLETED, 1, 0)),
        ScanEventAction::Completed {
            reason: 1,
            aborted: true,
        }
    );
    assert_eq!(session.phase(), ScanPhase::Idle);
}

#[test]
fn rejected_stop_scan_restores_running_state_and_unbound_queue_is_inert() {
    let mut session = ScanSession::new();
    session.reserve_start(&request()).unwrap();
    session.observe_event(event(WMI_SCAN_EVENT_STARTED, 0, 0));
    let mut unbound =
        ath10k_wmi_tx::WmiCommandTxQueue::<4>::new(3, 128, 16, 8, true, false, 9).unwrap();
    let mut frame = [0xa5; 128];
    assert_eq!(
        queue_stop_scan(
            &mut session,
            &mut unbound,
            0x1_2345_6000,
            3,
            0x1234,
            &mut frame,
        ),
        Err(QueueError::NotWmiControlEndpoint)
    );
    assert_eq!(session.phase(), ScanPhase::Running);
    assert_eq!(unbound.next_sequence_number(), 9);
    assert_eq!(unbound.ring_indices(), (0, 0));
    assert!(frame.iter().all(|byte| *byte == 0xa5));

    let mut low_credit = wmi_control_queue(1);
    assert_eq!(
        queue_stop_scan(
            &mut session,
            &mut low_credit,
            0x1_2345_6000,
            3,
            0x1234,
            &mut frame,
        ),
        Err(QueueError::Transport(
            ath10k_wmi_tx::Error::InsufficientCredits
        ))
    );
    assert_eq!(session.phase(), ScanPhase::Running);
    assert_eq!(low_credit.next_sequence_number(), 9);
    assert_eq!(low_credit.credits_available(), Ok(1));
    assert_eq!(low_credit.ring_indices(), (0, 0));
    assert!(frame.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn scan_queue_rejects_unbound_endpoint_without_reserving_or_mutating() {
    let mut session = ScanSession::new();
    let mut queue =
        ath10k_wmi_tx::WmiCommandTxQueue::<4>::new(3, 128, 16, 8, true, false, 9).unwrap();
    let mut frame = [0xa5; 128];

    assert_eq!(
        queue_start_scan(
            &mut session,
            &mut queue,
            &request(),
            0x1_2345_6000,
            3,
            0x1234,
            &mut frame,
        ),
        Err(QueueError::NotWmiControlEndpoint)
    );
    assert_eq!(session.phase(), ScanPhase::Idle);
    assert_eq!(queue.next_sequence_number(), 9);
    assert_eq!(queue.credits_available(), Ok(8));
    assert_eq!(queue.ring_indices(), (0, 0));
    assert!(frame.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn scan_queue_rolls_back_reservation_when_wmi_credits_are_insufficient() {
    let mut session = ScanSession::new();
    let mut queue = wmi_control_queue(1);
    let mut frame = [0xa5; 128];

    assert_eq!(
        queue_start_scan(
            &mut session,
            &mut queue,
            &request(),
            0x1_2345_6000,
            3,
            0x1234,
            &mut frame,
        ),
        Err(QueueError::Transport(
            ath10k_wmi_tx::Error::InsufficientCredits
        ))
    );
    assert_eq!(session.phase(), ScanPhase::Idle);
    assert_eq!(queue.next_sequence_number(), 9);
    assert_eq!(queue.credits_available(), Ok(1));
    assert_eq!(queue.ring_indices(), (0, 0));
    assert!(frame.iter().all(|byte| *byte == 0xa5));
}

#[test]
fn scan_queue_rolls_back_reservation_if_ce_descriptor_is_invalid() {
    let mut session = ScanSession::new();
    let mut queue = wmi_control_queue(8);
    let mut frame = [0xa5; 128];

    assert_eq!(
        queue_start_scan(
            &mut session,
            &mut queue,
            &request(),
            1 << 35,
            3,
            0x1234,
            &mut frame,
        ),
        Err(QueueError::Transport(ath10k_wmi_tx::Error::CeDescriptor(
            ath10k_ce::CeError::AddressOutOfRange
        )))
    );
    assert_eq!(session.phase(), ScanPhase::Idle);
    assert_eq!(queue.next_sequence_number(), 10);
    assert_eq!(queue.credits_available(), Ok(8));
    assert_eq!(queue.ring_indices(), (0, 0));
    assert!(frame.iter().all(|byte| *byte == 0xa5));
}
