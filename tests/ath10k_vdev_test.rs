#![allow(dead_code)]
#![forbid(unsafe_code)]
#[path = "../rust/src/ath10k_ce.rs"]
mod ath10k_ce;
#[path = "../rust/src/ath10k_vdev.rs"]
mod ath10k_vdev;
#[path = "../rust/src/ath10k_wmi_tx.rs"]
mod ath10k_wmi_tx;
use ath10k_vdev::*;

fn queue_tracker(
    tracker: &mut StartTracker,
    generation: u64,
    vdev_id: u32,
) -> Result<(), TrackedStartError> {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::WmiCommandTxQueue;
    let mut queue = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 512,
        },
        HtcServicePolicy {
            receive_allocation: 16,
            disable_credit_flow: false,
            credit_size: 16,
            initial_credits: 16,
        },
        false,
        9,
    )
    .unwrap();
    let command = StationCommand::Start {
        vdev_id,
        channel: ChannelWords {
            frequency: 2412,
            center1: 2412,
            center2: 0,
            flags: 0,
            regulatory0: 0,
            regulatory1: 0,
        },
        beacon_interval: 100,
        dtim_period: 2,
        pmf: false,
    };
    let mut output = [0u8; 160];
    let entry = queue_tracked_start(
        &mut queue,
        tracker,
        generation,
        command,
        0x100000,
        1,
        5,
        &mut output,
    )?;
    tracker
        .mark_published(generation, entry.slot, 1)
        .map_err(TrackedStartError::State)
}

#[test]
fn start_tracker_accepts_only_the_matching_success_once_per_generation() {
    let mut tracker = StartTracker::new(9, 3).unwrap();
    assert_eq!(queue_tracker(&mut tracker, 9, 3), Ok(()));
    let response = StartResponse {
        vdev_id: 3,
        requestor_id: 0,
        response_type: 0,
        status: 0,
    };
    assert_eq!(
        tracker.on_response(8, WMI_10_4_VDEV_START_RESPONSE, response),
        StartAction::Ignored
    );
    assert_eq!(
        tracker.on_response(9, 0x9005, response),
        StartAction::Ignored
    );
    assert_eq!(
        tracker.on_response(
            9,
            WMI_10_4_VDEV_START_RESPONSE,
            StartResponse {
                vdev_id: 4,
                ..response
            }
        ),
        StartAction::Ignored
    );
    assert_eq!(
        tracker.on_response(
            9,
            WMI_10_4_VDEV_START_RESPONSE,
            StartResponse {
                requestor_id: 1,
                ..response
            }
        ),
        StartAction::Ignored
    );
    assert_eq!(
        tracker.on_response(
            9,
            WMI_10_4_VDEV_START_RESPONSE,
            StartResponse {
                response_type: 1,
                ..response
            }
        ),
        StartAction::Ignored
    );
    assert_eq!(
        tracker.on_response(9, WMI_10_4_VDEV_START_RESPONSE, response),
        StartAction::Started
    );
    assert_eq!(
        tracker.on_response(9, WMI_10_4_VDEV_START_RESPONSE, response),
        StartAction::Ignored
    );
    assert_eq!(
        queue_tracker(&mut tracker, 9, 3),
        Err(TrackedStartError::State(StartError::AlreadyUsed))
    );
}

#[test]
fn failed_start_or_timeout_consumes_the_attempt() {
    let mut tracker = StartTracker::new(5, 8).unwrap();
    queue_tracker(&mut tracker, 5, 8).unwrap();
    let response = StartResponse {
        vdev_id: 8,
        requestor_id: 0,
        response_type: 0,
        status: 7,
    };
    assert_eq!(
        tracker.on_response(5, WMI_10_4_VDEV_START_RESPONSE, response),
        StartAction::Failed(7)
    );
    assert_eq!(
        queue_tracker(&mut tracker, 5, 8),
        Err(TrackedStartError::State(StartError::AlreadyUsed))
    );
    let mut timeout = StartTracker::new(6, 8).unwrap();
    queue_tracker(&mut timeout, 6, 8).unwrap();
    assert_eq!(timeout.timeout(5), Err(StartError::WrongGeneration));
    assert_eq!(timeout.timeout(6), Ok(()));
    assert_eq!(
        timeout.on_response(
            6,
            WMI_10_4_VDEV_START_RESPONSE,
            StartResponse {
                status: 0,
                ..response
            }
        ),
        StartAction::Ignored
    );
}

#[test]
fn tracked_start_commits_only_after_queue_acceptance() {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::{Error as TxError, WmiCommandTxQueue};
    let mut queue = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 512,
        },
        HtcServicePolicy {
            receive_allocation: 16,
            disable_credit_flow: false,
            credit_size: 16,
            initial_credits: 16,
        },
        false,
        9,
    )
    .unwrap();
    let command = StationCommand::Start {
        vdev_id: 3,
        channel: ChannelWords {
            frequency: 2412,
            center1: 2412,
            center2: 0,
            flags: 0,
            regulatory0: 0,
            regulatory1: 0,
        },
        beacon_interval: 100,
        dtim_period: 2,
        pmf: false,
    };
    let mut tracker = StartTracker::new(9, 3).unwrap();
    let mut short = [0xa5; 8];
    assert_eq!(
        queue_tracked_start(
            &mut queue,
            &mut tracker,
            9,
            command,
            0x100000,
            1,
            5,
            &mut short
        ),
        Err(TrackedStartError::Queue(QueueError::Transport(
            TxError::OutputTooSmall
        )))
    );
    let mut out = [0u8; 160];
    let entry = queue_tracked_start(
        &mut queue,
        &mut tracker,
        9,
        command,
        0x100000,
        1,
        5,
        &mut out,
    )
    .unwrap();
    assert_eq!(
        queue_tracked_start(
            &mut queue,
            &mut tracker,
            9,
            command,
            0x100000,
            1,
            5,
            &mut out
        ),
        Err(TrackedStartError::State(StartError::AlreadyUsed))
    );
    let response = StartResponse {
        vdev_id: 3,
        requestor_id: 0,
        response_type: 0,
        status: 0,
    };
    assert_eq!(
        tracker.on_response(9, WMI_10_4_VDEV_START_RESPONSE, response),
        StartAction::Ignored
    );
    assert_eq!(
        tracker.mark_published(9, entry.slot, 2),
        Err(StartError::WrongTransfer)
    );
    assert_eq!(
        tracker.mark_published(8, entry.slot, 1),
        Err(StartError::WrongGeneration)
    );
    assert_eq!(tracker.mark_published(9, entry.slot, 1), Ok(()));
    assert_eq!(
        tracker.on_response(9, WMI_10_4_VDEV_START_RESPONSE, response),
        StartAction::Started
    );
}

#[test]
fn peer_assoc_10_4_matches_packed_linux_body() {
    let request = PeerAssoc {
        mac: [2, 1, 2, 3, 4, 5],
        vdev_id: 7,
        reassoc: false,
        association_id: 42,
        flags: 0x11223344,
        capabilities: 0x5678,
        listen_interval: 5,
        ht_capabilities: 0x12345678,
        max_mpdu: 65535,
        mpdu_density: 16,
        rate_capabilities: 0x42,
        legacy_rates: &[2, 4, 11],
        ht_rates: &[0, 1, 7],
        spatial_streams: 2,
        vht_capabilities: 0x99887766,
        phy_mode: 9,
        vht_rates: [1, 2, 3, 4],
        bandwidth_rxnss_override: 0x80000001,
    };
    let mut out = [0xa5; 360];
    assert_eq!(encode_peer_assoc(request, &mut out), Ok(356));
    let word = |at| u32::from_le_bytes(out[at..at + 4].try_into().unwrap());
    assert_eq!(&out[..8], &[2, 1, 2, 3, 4, 5, 0, 0]);
    assert_eq!(
        (word(8), word(12), word(16), word(20)),
        (7, 1, 42, 0x11223344)
    );
    assert_eq!(
        (word(24), word(28), word(32), word(36), word(40), word(44)),
        (0x5678, 5, 0x12345678, 65535, 16, 0x42)
    );
    assert_eq!(word(48), 3);
    assert_eq!(&out[52..55], &[2, 4, 11]);
    assert!(out[55..184].iter().all(|&b| b == 0));
    assert_eq!(word(184), 3);
    assert_eq!(&out[188..191], &[0, 1, 7]);
    assert!(out[191..320].iter().all(|&b| b == 0));
    assert_eq!((word(320), word(324), word(328)), (2, 0x99887766, 9));
    assert_eq!((word(332), word(336), word(340), word(344)), (1, 2, 3, 4));
    assert_eq!((word(348), word(352)), (0xff, 0x80000001));
    assert_eq!(&out[356..], &[0xa5; 4]);
    assert_eq!(WMI_10_4_PEER_ASSOC, 0x9026);
}

#[test]
fn peer_assoc_rejects_bad_inputs_before_touching_output() {
    let long = [1u8; 129];
    let mut request = PeerAssoc {
        mac: [2, 0, 0, 0, 0, 1],
        vdev_id: 0,
        reassoc: true,
        association_id: 1,
        flags: 0,
        capabilities: 0,
        listen_interval: 1,
        ht_capabilities: 0,
        max_mpdu: 0,
        mpdu_density: 0,
        rate_capabilities: 0,
        legacy_rates: &long,
        ht_rates: &[],
        spatial_streams: 1,
        vht_capabilities: 0,
        phy_mode: 0,
        vht_rates: [0; 4],
        bandwidth_rxnss_override: 0,
    };
    let mut out = [0xa5; 356];
    assert_eq!(
        encode_peer_assoc(request, &mut out),
        Err(Error::TooManyRates)
    );
    assert!(out.iter().all(|&x| x == 0xa5));
    request.legacy_rates = &[];
    request.mpdu_density = 17;
    assert_eq!(
        encode_peer_assoc(request, &mut out),
        Err(Error::InvalidMpduDensity)
    );
    assert!(out.iter().all(|&x| x == 0xa5));
    request.mpdu_density = 0;
    assert_eq!(
        encode_peer_assoc(request, &mut out[..355]),
        Err(Error::OutputTooSmall)
    );
    assert!(out.iter().all(|&x| x == 0xa5));
}

#[test]
fn peer_assoc_queues_only_on_control_endpoint_and_carries_10_4_id() {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::WmiCommandTxQueue;
    let request = PeerAssoc {
        mac: [2, 0, 0, 0, 0, 1],
        vdev_id: 7,
        reassoc: true,
        association_id: 9,
        flags: 0,
        capabilities: 0,
        listen_interval: 1,
        ht_capabilities: 0,
        max_mpdu: 0,
        mpdu_density: 0,
        rate_capabilities: 0,
        legacy_rates: &[2, 4],
        ht_rates: &[],
        spatial_streams: 1,
        vht_capabilities: 0,
        phy_mode: 0,
        vht_rates: [0; 4],
        bandwidth_rxnss_override: 0,
    };
    let mut queue = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 512,
        },
        HtcServicePolicy {
            receive_allocation: 16,
            disable_credit_flow: false,
            credit_size: 64,
            initial_credits: 16,
        },
        false,
        9,
    )
    .unwrap();
    let mut frame = [0xa5; 400];
    let before = queue;
    let bad = PeerAssoc {
        mpdu_density: 17,
        ..request
    };
    assert_eq!(
        queue_peer_assoc(&mut queue, bad, 0x100000, 1, 5, &mut frame),
        Err(QueueError::Encode(Error::InvalidMpduDensity))
    );
    assert_eq!(queue, before);
    assert!(frame.iter().all(|&b| b == 0xa5));
    let queued = queue_peer_assoc(&mut queue, request, 0x100000, 1, 5, &mut frame).unwrap();
    assert_eq!(queued.transfer.tx.frame_bytes, 368);
    assert_eq!(
        u32::from_le_bytes(frame[8..12].try_into().unwrap()),
        WMI_10_4_PEER_ASSOC
    );
    assert_eq!(u32::from_le_bytes(frame[24..28].try_into().unwrap()), 0); // reassociation
    assert_eq!(u32::from_le_bytes(frame[28..32].try_into().unwrap()), 9);
}

#[test]
fn queue_requires_wmi_control_and_serializes_the_start_command() {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::WmiCommandTxQueue;
    let mut generic = WmiCommandTxQueue::<4>::new(3, 512, 16, 16, true, false, 9).unwrap();
    let command = StationCommand::Start {
        vdev_id: 3,
        channel: ChannelWords {
            frequency: 2412,
            center1: 2412,
            center2: 0,
            flags: 0,
            regulatory0: 0,
            regulatory1: 0,
        },
        beacon_interval: 100,
        dtim_period: 2,
        pmf: false,
    };
    let mut output = [0xa5u8; 160];
    assert_eq!(
        queue_station_command(&mut generic, command, 0x100000, 1, 5, &mut output),
        Err(QueueError::NotWmiControl)
    );
    assert!(output.iter().all(|&b| b == 0xa5));
    let mut queue = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 512,
        },
        HtcServicePolicy {
            receive_allocation: 16,
            disable_credit_flow: false,
            credit_size: 16,
            initial_credits: 16,
        },
        false,
        9,
    )
    .unwrap();
    let queued = queue_station_command(&mut queue, command, 0x100000, 1, 5, &mut output).unwrap();
    assert_eq!(queued.transfer.tx.frame_bytes, 140);
    assert_eq!(
        u32::from_le_bytes(output[8..12].try_into().unwrap()),
        WMI_10_4_VDEV_START
    );
    assert_eq!(u32::from_le_bytes(output[12..16].try_into().unwrap()), 2412);
    assert_eq!(u32::from_le_bytes(output[36..40].try_into().unwrap()), 3);
}

#[test]
fn each_lifecycle_command_queues_with_its_own_wmi_id() {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::WmiCommandTxQueue;
    let mac = [2, 1, 2, 3, 4, 5];
    let cases = [
        (
            StationCommand::Create { vdev_id: 7, mac },
            WMI_10_4_VDEV_CREATE,
            20,
        ),
        (
            StationCommand::PeerCreate {
                vdev_id: 7,
                mac,
                kind: PeerType::Bss,
            },
            WMI_10_4_PEER_CREATE,
            16,
        ),
        (
            StationCommand::PeerDelete { vdev_id: 7, mac },
            WMI_10_4_PEER_DELETE,
            12,
        ),
        (
            StationCommand::Up {
                vdev_id: 7,
                association_id: 1,
                bssid: mac,
            },
            WMI_10_4_VDEV_UP,
            16,
        ),
        (StationCommand::Down { vdev_id: 7 }, WMI_10_4_VDEV_DOWN, 4),
        (StationCommand::Stop { vdev_id: 7 }, WMI_10_4_VDEV_STOP, 4),
        (
            StationCommand::Delete { vdev_id: 7 },
            WMI_10_4_VDEV_DELETE,
            4,
        ),
    ];
    for (command, id, length) in cases {
        let mut queue = WmiCommandTxQueue::<4>::new_wmi_control(
            HtcEndpoint {
                service_id: HTC_WMI_CONTROL_SERVICE_ID,
                endpoint_id: 3,
                max_message_size: 512,
            },
            HtcServicePolicy {
                receive_allocation: 16,
                disable_credit_flow: false,
                credit_size: 16,
                initial_credits: 16,
            },
            false,
            9,
        )
        .unwrap();
        let mut out = [0xa5; 160];
        let entry = queue_station_command(&mut queue, command, 0x100000, 1, 5, &mut out).unwrap();
        assert_eq!(entry.transfer.tx.frame_bytes, length + 12);
        assert_eq!(u32::from_le_bytes(out[8..12].try_into().unwrap()), id);
    }
}

#[test]
fn queue_rejects_bad_peer_and_short_frame_without_consuming_state() {
    use ath10k_ce::{HTC_WMI_CONTROL_SERVICE_ID, HtcEndpoint, HtcServicePolicy};
    use ath10k_wmi_tx::{Error as TxError, WmiCommandTxQueue};
    let mut queue = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 512,
        },
        HtcServicePolicy {
            receive_allocation: 16,
            disable_credit_flow: false,
            credit_size: 16,
            initial_credits: 16,
        },
        false,
        9,
    )
    .unwrap();
    let before = queue;
    let mut out = [0xa5; 8];
    let invalid = StationCommand::PeerCreate {
        vdev_id: 7,
        mac: [1, 0, 0, 0, 0, 1],
        kind: PeerType::Default,
    };
    assert_eq!(
        queue_station_command(&mut queue, invalid, 0x100000, 1, 5, &mut out),
        Err(QueueError::Encode(Error::InvalidMac))
    );
    assert_eq!(queue, before);
    let valid = StationCommand::PeerCreate {
        vdev_id: 7,
        mac: [2, 0, 0, 0, 0, 1],
        kind: PeerType::Default,
    };
    assert_eq!(
        queue_station_command(&mut queue, valid, 0x100000, 1, 5, &mut out),
        Err(QueueError::Transport(TxError::OutputTooSmall))
    );
    assert_eq!(queue, before);
    assert_eq!(out, [0xa5; 8]);
}

#[test]
fn station_start_encodes_linux_packed_fields_and_zeroes_unused_data() {
    let channel = ChannelWords {
        frequency: 2412,
        center1: 2412,
        center2: 0,
        flags: 0x1234,
        regulatory0: 0x11223344,
        regulatory1: 0x55667788,
    };
    let mut body = [0xa5; 132];
    assert_eq!(
        encode_station_start(3, channel, 100, 2, true, &mut body),
        Ok(128)
    );
    let w = |i: usize| u32::from_le_bytes(body[i..i + 4].try_into().unwrap());
    assert_eq!(
        (w(0), w(4), w(8), w(12), w(16), w(20)),
        (2412, 2412, 0, 0x1234, 0x11223344, 0x55667788)
    );
    assert_eq!((w(24), w(28), w(32), w(36), w(40)), (3, 0, 100, 2, 2));
    assert!(body[44..128].iter().all(|&b| b == 0));
    assert_eq!(&body[128..], &[0xa5; 4]);
}

#[test]
fn create_up_delete_encode_padded_mac_and_wmi_ids() {
    let mac = [2, 1, 2, 3, 4, 5];
    let mut buf = [0xa5; 24];
    assert_eq!(encode_station_create(7, mac, &mut buf), Ok(20));
    assert_eq!(
        &buf[..20],
        &[7, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 2, 1, 2, 3, 4, 5, 0, 0]
    );
    assert_eq!(
        encode_peer_create(7, mac, PeerType::Default, &mut buf),
        Ok(16)
    );
    assert_eq!(
        &buf[..16],
        &[7, 0, 0, 0, 2, 1, 2, 3, 4, 5, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(encode_vdev_up(7, 0x123, mac, &mut buf), Ok(16));
    assert_eq!(
        &buf[..16],
        &[7, 0, 0, 0, 0x23, 1, 0, 0, 2, 1, 2, 3, 4, 5, 0, 0]
    );
    assert_eq!(encode_peer_delete(7, mac, &mut buf), Ok(12));
    assert_eq!(WMI_10_4_VDEV_CREATE, 0x9013);
    assert_eq!(WMI_10_4_VDEV_DELETE, 0x9014);
    assert_eq!(WMI_10_4_VDEV_START, 0x9015);
    assert_eq!(WMI_10_4_VDEV_UP, 0x9017);
    assert_eq!(WMI_10_4_VDEV_STOP, 0x9018);
    assert_eq!(WMI_10_4_VDEV_DOWN, 0x9019);
    assert_eq!(WMI_10_4_PEER_CREATE, 0x9022);
    assert_eq!(WMI_10_4_PEER_DELETE, 0x9023);
    assert_eq!(WMI_10_4_VDEV_START_RESPONSE, 0x9004);
    assert_eq!(encode_vdev_id(0x12345678, &mut buf), Ok(4));
    assert_eq!(&buf[..4], &[0x78, 0x56, 0x34, 0x12]);
}

#[test]
fn errors_leave_output_unchanged_and_response_parsing_is_bounded() {
    let mut out = [0xa5; 16];
    assert_eq!(
        encode_station_create(1, [2, 0, 0, 0, 0, 1], &mut out),
        Err(Error::OutputTooSmall)
    );
    assert_eq!(
        encode_peer_create(1, [1, 0, 0, 0, 0, 1], PeerType::Default, &mut out),
        Err(Error::InvalidMac)
    );
    assert_eq!(out, [0xa5; 16]);
    assert_eq!(decode_start_response(&[0; 15]), Err(Error::TruncatedEvent));
    let event = [3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0];
    assert_eq!(
        decode_start_response(&event),
        Ok(StartResponse {
            vdev_id: 3,
            requestor_id: 4,
            response_type: 0,
            status: 9
        })
    );
}
