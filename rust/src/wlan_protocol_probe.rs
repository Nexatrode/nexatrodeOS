#![no_std]
#![no_main]

use personal_os_rust_server::{
    ath10k_ce::{
        HTC_WMI_CONTROL_SERVICE_ID, HtcControlMessage, HtcEndpoint, HtcRxHeader, HtcServicePolicy,
        HtcSession, HtcTxHeader, Wcn3990Descriptor,
    },
    ath10k_firmware::Ath10kFirmwareMetadata,
    ath10k_htt::{
        HTT_10_4_T2H_SECURITY_INDICATION, HTT_SECURITY_INDICATION_BYTES,
        HTT_SECURITY_TYPE_AES_CCMP, HttSecurityIndication,
    },
    ath10k_key::{
        CCMP128_KEY_BYTES, CcmpKeyInstall, CcmpKeyInstallTracker, KeyInstallAction, KeyKind,
        VDEV_INSTALL_KEY_FIXED_BYTES, WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID, queue_ccmp_install_key,
    },
    ath10k_scan::{
        ScanEvent, ScanEventAction, ScanPhase, ScanSession, StartScanRequest,
        WMI_10_4_START_SCAN_COMMAND_ID, WMI_10_4_STOP_SCAN_COMMAND_ID, WMI_SCAN_EVENT_STARTED,
        queue_start_scan, queue_stop_scan,
    },
    ath10k_wmi::{
        WMI_10_4_INIT_CMD_ID, Wmi10_4ReadyEvent, Wmi10_4ServiceReadyView, WmiHostMemoryPolicy,
        WmiWcn3990ResourceConfig10_4,
    },
    ath10k_wmi_tx::WmiCommandTxQueue,
    qcacld_wmi::{
        QcacldPeerLimit, SUNFISH_WMI_INIT_COMMAND_ID, ServiceAvailableView, ServiceReadyView,
        WmiResourceConfig,
    },
    qcom_htc,
    sunfish_qcacld_resource::{
        SunfishQcacldResourceCandidate, SunfishQcacldResourceInputs,
        encode_sunfish_qcacld_wmi_init_frame,
    },
    wlan_protocol_probe_ipa,
    wlfw_protocol::{QRTR_CTRL_PACKET_SIZE, encode_wlfw_qrtr_lookup},
};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain,
};

const FB_WIDTH: usize = 1080;
const FB_HEIGHT: usize = 2340;
const FB_PIXELS: usize = FB_WIDTH * FB_HEIGHT;
const FB_SCALE: usize = 9;
const FONT: [[u8; 7]; 36] = [
    [14, 17, 17, 31, 17, 17, 17],
    [30, 17, 17, 30, 17, 17, 30],
    [14, 17, 16, 16, 16, 17, 14],
    [30, 17, 17, 17, 17, 17, 30],
    [31, 16, 16, 30, 16, 16, 31],
    [31, 16, 16, 30, 16, 16, 16],
    [14, 17, 16, 23, 17, 17, 15],
    [17, 17, 17, 31, 17, 17, 17],
    [14, 4, 4, 4, 4, 4, 14],
    [7, 2, 2, 2, 18, 18, 12],
    [17, 18, 20, 24, 20, 18, 17],
    [16, 16, 16, 16, 16, 16, 31],
    [17, 27, 21, 21, 17, 17, 17],
    [17, 25, 21, 19, 17, 17, 17],
    [14, 17, 17, 17, 17, 17, 14],
    [30, 17, 17, 30, 16, 16, 16],
    [14, 17, 17, 17, 21, 18, 13],
    [30, 17, 17, 30, 20, 18, 17],
    [15, 16, 16, 14, 1, 1, 30],
    [31, 4, 4, 4, 4, 4, 4],
    [17, 17, 17, 17, 17, 17, 14],
    [17, 17, 17, 17, 17, 10, 4],
    [17, 17, 17, 21, 21, 21, 10],
    [17, 17, 10, 4, 10, 17, 17],
    [17, 17, 10, 4, 4, 4, 4],
    [31, 1, 2, 4, 8, 16, 31],
    [14, 17, 19, 21, 25, 17, 14],
    [4, 12, 4, 4, 4, 4, 14],
    [14, 17, 1, 2, 4, 8, 31],
    [30, 1, 1, 14, 1, 1, 30],
    [2, 6, 10, 18, 31, 2, 2],
    [31, 16, 16, 30, 1, 1, 30],
    [14, 16, 16, 30, 17, 17, 14],
    [31, 1, 2, 4, 8, 8, 8],
    [14, 17, 17, 14, 17, 17, 14],
    [14, 17, 17, 15, 1, 1, 14],
];

#[protection_domain]
fn init() -> ProtocolProbe {
    let descriptor = Wcn3990Descriptor::encode(0x1_2345_6789, 1500, 0x321, true, true);
    let descriptor_ok = descriptor.is_ok_and(|descriptor| {
        let bytes = descriptor.as_bytes();
        u32::from_le_bytes(bytes[0..4].try_into().unwrap()) == 0x2345_6789
            && u32::from_le_bytes(bytes[4..8].try_into().unwrap()) == 0x8000_0001
            && u16::from_le_bytes(bytes[8..10].try_into().unwrap()) == 1500
            && u16::from_le_bytes(bytes[10..12].try_into().unwrap()) == ((0x321 << 4) | 0b11)
    });

    let mut lookup = [0u8; QRTR_CTRL_PACKET_SIZE];
    let lookup_len = encode_wlfw_qrtr_lookup(&mut lookup);
    let mut lookup_ok = lookup_len == Ok(QRTR_CTRL_PACKET_SIZE);
    for (index, word) in [10u32, 0x45, 1, 0, 0].into_iter().enumerate() {
        lookup_ok &= lookup[index * 4..index * 4 + 4] == word.to_le_bytes();
    }

    let ipa_ok = ipa_codec_ok();
    let mut wmi_command = [0u8; 188];
    let wmi_ok = wmi_10_4_codec_ok(&mut wmi_command);
    let htc_tx_ok = wmi_ok && ath10k_wmi_control_tx_ok(&wmi_command);
    let qcacld_init_ok = qcacld_wmi_init_codec_ok();
    let scan_tx_ok = scan_transfer_codec_ok();
    let key_frame_ok = key_frame_codec_ok();
    let passed = descriptor_ok
        && lookup_ok
        && ipa_ok
        && wmi_ok
        && htc_tx_ok
        && qcacld_init_ok
        && scan_tx_ok
        && key_frame_ok;
    debug_println!(
        "WLAN-PROTOCOL|{}: CE, QRTR, IPA, WMI 10.4, QCACLD INIT/READY, HTC, scan, key codecs",
        if passed { "PASS" } else { "FAIL" }
    );
    draw_report(
        passed,
        descriptor_ok,
        lookup_ok,
        ipa_ok,
        wmi_ok,
        htc_tx_ok,
        qcacld_init_ok,
        scan_tx_ok,
        key_frame_ok,
    );
    ProtocolProbe
}

fn qcacld_wmi_init_codec_ok() -> bool {
    // Rebuild the captured firmware capability inputs and the observed
    // 68-word resource profile in bounded ordinary memory. This is the
    // downstream Sunfish QCACLD path; it is distinct from ath10k WMI 10.4 and
    // never publishes the resulting HTC frame to a CE ring.
    const BASE_SERVICE_WORDS: [u32; 32] = [
        0x0000_000d,
        0x0000_0007,
        0x0000_000f,
        0x0000_0003,
        0x0000_000f,
        0x0000_0009,
        0x0000_0009,
        0x0000_000f,
        0x0000_000a,
        0x0000_0009,
        0x0000_0002,
        0x0000_000a,
        0x0000_0001,
        0x0000_000d,
        0x0000_0007,
        0x0000_000f,
        0x0000_000b,
        0x0000_0004,
        0x0000_0004,
        0x0000_000b,
        0x0000_0007,
        0x0000_0000,
        0x0000_0008,
        0x0000_0001,
        0x0000_0009,
        0x0000_0003,
        0x0000_000d,
        0x0000_0001,
        0x0000_0005,
        0x0000_000e,
        0x0000_0008,
        0x0000_0005,
    ];
    const EXTENDED_SERVICE_WORDS: [u32; 4] = [0x4531_e0c8, 0x0051_2000, 0x0000_2000, 0];

    let mut fixed = [0u8; 128];
    fixed[..4].copy_from_slice(&0x3226_822eu32.to_le_bytes());
    let abi: [u32; 6] = [0x0100_0000, 762, 0x5f41_4351, 0x0000_4c4d, 0, 0];
    for (index, word) in abi.into_iter().enumerate() {
        fixed[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes());
    }

    let mut service_bitmap = [0u8; 128];
    for (index, word) in BASE_SERVICE_WORDS.into_iter().enumerate() {
        service_bitmap[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }

    let mut service_ready_bytes = [0u8; 304];
    let Some(cursor) = append_qcacld_tlv(&mut service_ready_bytes, 0, 32, &fixed) else {
        return false;
    };
    let Some(cursor) = append_qcacld_tlv(&mut service_ready_bytes, cursor, 33, &[0; 36]) else {
        return false;
    };
    if append_qcacld_tlv(&mut service_ready_bytes, cursor, 16, &service_bitmap)
        != Some(service_ready_bytes.len())
    {
        return false;
    }

    let mut available_fixed = [0u8; 20];
    available_fixed[..4].copy_from_slice(&128u32.to_le_bytes());
    for (index, word) in EXTENDED_SERVICE_WORDS.into_iter().enumerate() {
        available_fixed[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes());
    }
    let mut available_bytes = [0u8; 24];
    if append_qcacld_tlv(&mut available_bytes, 0, 559, &available_fixed)
        != Some(available_bytes.len())
    {
        return false;
    }

    let Ok(service_ready) = ServiceReadyView::parse(&service_ready_bytes) else {
        return false;
    };
    let Ok(service_available) = ServiceAvailableView::parse(&available_bytes) else {
        return false;
    };
    if service_ready.service_enabled(99) != Some(true)
        || service_available.service_enabled(176) != Some(true)
        || service_available.service_enabled(182) != Some(true)
    {
        return false;
    }

    let inputs = SunfishQcacldResourceInputs::observed_lineage_wmi_init_2026_10_24(
        QcacldPeerLimit::OtherTarget32,
    );
    let Ok(resource) =
        SunfishQcacldResourceCandidate::build(&service_ready, &service_available, inputs)
    else {
        return false;
    };
    let words = resource.words();
    if words.len() != 68
        || words[0] != 4
        || words[1] != 38
        || words[7] != 3
        || words[8] != 3
        || words[28] != 1056
        || words[29] != 2
        || words[34] != 2
        || words[43] != 0x0018_0600
        || words[53] != 1
        || words[61] != u32::MAX
    {
        return false;
    }

    let mut wmi_body = [0u8; 512];
    let mut htc_frame = [0u8; 512];
    let Ok(plan) = encode_sunfish_qcacld_wmi_init_frame(
        &service_ready,
        &service_available,
        inputs,
        None,
        2,
        512,
        128,
        5,
        &mut wmi_body,
        &mut htc_frame,
    ) else {
        return false;
    };
    if plan.body_bytes != 312 || plan.htc.frame_len != 324 || plan.htc.credits_required != 3 {
        return false;
    }
    let Ok(parsed) = qcom_htc::parse_wmi_tx_frame(&htc_frame[..plan.htc.frame_len], 2, 512) else {
        return false;
    };
    if parsed.command_id != SUNFISH_WMI_INIT_COMMAND_ID
        || parsed.tlv_body != &wmi_body[..plan.body_bytes]
    {
        return false;
    }
    let Ok(parsed_resource) = WmiResourceConfig::parse(&parsed.tlv_body[32..308]) else {
        return false;
    };
    if parsed_resource.as_tlv()[..4] != [0x10, 0x01, 0x4b, 0x00]
        || parsed_resource.feature_flags() != 0x0018_0600
        || parsed_resource.num_packet_filters() != u32::MAX
    {
        return false;
    }

    // Exercise the next QCACLD startup codec with a bounded synthetic READY
    // event. This checks the target-side parser after INIT composition; it is
    // not evidence that the real firmware sent READY.
    let mut ready_fixed = [0u8; 60];
    for (index, word) in abi.into_iter().enumerate() {
        ready_fixed[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    ready_fixed[24..28].copy_from_slice(&0x4433_2211u32.to_le_bytes());
    ready_fixed[28..32].copy_from_slice(&0x0000_6655u32.to_le_bytes());
    ready_fixed[40..44].copy_from_slice(&1u32.to_le_bytes()); // one extra MAC
    ready_fixed[44..48].copy_from_slice(&38u32.to_le_bytes());
    ready_fixed[48..52].copy_from_slice(&2u32.to_le_bytes());
    let extra_mac = [0x5a; 8];
    let mut ready_event = [0u8; 76];
    let Some(ready_cursor) = append_qcacld_tlv(&mut ready_event, 0, 35, &ready_fixed) else {
        return false;
    };
    if append_qcacld_tlv(&mut ready_event, ready_cursor, 19, &extra_mac) != Some(ready_event.len())
    {
        return false;
    }
    let Ok(startup) = service_ready.require_sunfish_wmi_startup() else {
        return false;
    };
    let Ok(ready) = startup.decode_ready_event(&ready_event) else {
        return false;
    };
    ready.mac_address_words == [0x4433_2211, 0x0000_6655]
        && ready.extra_mac_addresses == extra_mac
        && ready.total_peers == 38
        && ready.extra_peers == 2
}

fn append_qcacld_tlv(output: &mut [u8], offset: usize, tag: u16, value: &[u8]) -> Option<usize> {
    let end = offset.checked_add(4)?.checked_add(value.len())?;
    if end > output.len() || value.len() > usize::from(u16::MAX) {
        return None;
    }
    output[offset..offset + 2].copy_from_slice(&(value.len() as u16).to_le_bytes());
    output[offset + 2..offset + 4].copy_from_slice(&tag.to_le_bytes());
    output[offset + 4..end].copy_from_slice(value);
    Some(end)
}

fn wmi_10_4_codec_ok(command: &mut [u8; 188]) -> bool {
    // Synthetic API-5 metadata and fixed WMI 10.4 events exercise the exact
    // profile-to-INIT command path without firmware bytes or hardware access.
    let mut metadata_bytes = [0u8; 36];
    metadata_bytes[..11].copy_from_slice(b"QCA-ATH10K\0");
    metadata_bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
    metadata_bytes[16..20].copy_from_slice(&3u32.to_le_bytes());
    metadata_bytes[20..23].copy_from_slice(&[0, 0, 0x08]); // NON_BMI
    metadata_bytes[24..28].copy_from_slice(&5u32.to_le_bytes());
    metadata_bytes[28..32].copy_from_slice(&4u32.to_le_bytes());
    metadata_bytes[32..36].copy_from_slice(&6u32.to_le_bytes()); // WMI 10.4
    let Ok(metadata) = Ath10kFirmwareMetadata::parse(&metadata_bytes) else {
        return false;
    };
    if metadata.wmi_op_version() != Some(6) || !metadata.requires_external_firmware() {
        return false;
    }

    let mut service_ready_bytes = [0u8; 160];
    service_ready_bytes[24..28].copy_from_slice(&(1u32 << 5).to_le_bytes()); // raw bit 37
    let Ok(service_ready) = Wmi10_4ServiceReadyView::parse(&service_ready_bytes) else {
        return false;
    };
    if !service_ready.service_enabled(37) || service_ready.service_enabled(78) {
        return false;
    }
    let Ok(resource_config) =
        WmiWcn3990ResourceConfig10_4::from_firmware_metadata(&metadata, &service_ready)
    else {
        return false;
    };
    if u32::from_le_bytes(resource_config.as_bytes()[4..8].try_into().unwrap()) != 528 {
        return false;
    }

    let policies: [WmiHostMemoryPolicy; 0] = [];
    let Ok(batch) = service_ready.plan_host_memory(&policies, 0) else {
        return false;
    };
    let Ok(assignment) = batch.assign_iova_pool(0x8000_0000, 0) else {
        return false;
    };
    if assignment.encode_init_command_10_4(resource_config.as_bytes(), command) != Ok(command.len())
        || command[..4] != WMI_10_4_INIT_CMD_ID.to_le_bytes()
        || command[4..172] != *resource_config.as_bytes()
        || command[172..176] != [0; 4]
    {
        return false;
    }

    Wmi10_4ReadyEvent::parse(&[0u8; 20]).is_ok()
}

fn ath10k_wmi_control_tx_ok(command: &[u8]) -> bool {
    // Model the source-defined HTC READY / WMI_CONTROL service exchange, then
    // wrap the WMI command for the dynamically assigned endpoint and CE TX
    // descriptor. The synthetic peer never touches the radio or DMA engine.
    const WMI_CONTROL_SERVICE_ID: u16 = 0x0100;

    let ready_payload = [1, 0, 24, 0, 0, 1, 8, 0];
    let mut ready_frame = [0u8; 16];
    ready_frame[2..4].copy_from_slice(&(ready_payload.len() as u16).to_le_bytes());
    ready_frame[8..].copy_from_slice(&ready_payload);
    let Ok(ready_header) = HtcRxHeader::parse(&ready_frame, ready_frame.len()) else {
        return false;
    };
    if ready_header.endpoint_id != 0 || ready_header.data_len() != ready_payload.len() {
        return false;
    }
    let Ok(HtcControlMessage::Ready(ready)) = HtcControlMessage::parse(&ready_frame[8..]) else {
        return false;
    };
    let Ok(mut session) = HtcSession::new(ready) else {
        return false;
    };
    let Ok(policy) = HtcServicePolicy::derive(WMI_CONTROL_SERVICE_ID, ready) else {
        return false;
    };

    let connect_request = policy.connect_request(WMI_CONTROL_SERVICE_ID).encode();
    let mut connect_frame = [0u8; 16];
    if HtcTxHeader::write_frame(0, &connect_request, 0, false, &mut connect_frame) != Ok(16)
        || connect_frame[..8] != [0, 0, 8, 0, 0, 0, 0, 0]
        || connect_frame[8..] != connect_request
    {
        return false;
    }

    let response_payload = [3, 0, 0, 1, 0, 3, 0, 8]; // WMI_CONTROL -> endpoint 3, 2048-byte max
    let mut response_frame = [0u8; 16];
    response_frame[2..4].copy_from_slice(&(response_payload.len() as u16).to_le_bytes());
    response_frame[8..].copy_from_slice(&response_payload);
    let Ok(response_header) = HtcRxHeader::parse(&response_frame, response_frame.len()) else {
        return false;
    };
    if response_header.endpoint_id != 0 || response_header.data_len() != response_payload.len() {
        return false;
    }
    let Ok(HtcControlMessage::ConnectServiceResponse(response)) =
        HtcControlMessage::parse(&response_frame[8..])
    else {
        return false;
    };
    let Ok((endpoint, service_policy)) = session.connect(WMI_CONTROL_SERVICE_ID, response) else {
        return false;
    };
    if endpoint.endpoint_id != 3
        || endpoint.service_id != WMI_CONTROL_SERVICE_ID
        || command.len() > usize::from(endpoint.max_message_size)
        || !service_policy.flow_control_enabled()
    {
        return false;
    }

    let mut tx_frame = [0u8; 512];
    let Ok(frame_len) = HtcTxHeader::write_frame(
        endpoint.endpoint_id,
        command,
        0,
        service_policy.flow_control_enabled(),
        &mut tx_frame,
    ) else {
        return false;
    };
    let Ok(tx_header) = HtcRxHeader::parse(&tx_frame[..frame_len], frame_len) else {
        return false;
    };
    if tx_header.endpoint_id != endpoint.endpoint_id
        || tx_header.flags != 1
        || usize::from(tx_header.payload_len) != command.len()
        || tx_header.data_len() != command.len()
        || tx_frame[8..frame_len] != *command
    {
        return false;
    }

    let mut credits = session.credits();
    if credits.consume(endpoint.endpoint_id, frame_len) != Ok(1)
        || credits.available(endpoint.endpoint_id) != Ok(23)
    {
        return false;
    }
    let Ok(descriptor) = Wcn3990Descriptor::encode(0x8000_0000, frame_len, 1, false, false) else {
        return false;
    };
    let descriptor = descriptor.as_bytes();
    u32::from_le_bytes(descriptor[0..4].try_into().unwrap()) == 0x8000_0000
        && u32::from_le_bytes(descriptor[4..8].try_into().unwrap()) == 0
        && u16::from_le_bytes(descriptor[8..10].try_into().unwrap()) as usize == frame_len
        && u16::from_le_bytes(descriptor[10..12].try_into().unwrap()) == 0x0010
}

fn scan_transfer_codec_ok() -> bool {
    // Exercise a synthetic passive START_SCAN and its matching STOP_SCAN
    // through WMI, HTC, and WCN3990 CE serialization, then only the software
    // credit/sequence/ring model. No descriptor is published to hardware.
    let channels = [2412u16];
    let ssids: [&[u8]; 0] = [];
    let ies = [];
    let request = StartScanRequest::linux_reference_defaults(1, &channels, &ssids, &ies);
    let mut frame = [0xa5; 512];
    let Ok(mut queue) = WmiCommandTxQueue::<4>::new_wmi_control(
        HtcEndpoint {
            service_id: HTC_WMI_CONTROL_SERVICE_ID,
            endpoint_id: 3,
            max_message_size: 2048,
        },
        HtcServicePolicy {
            receive_allocation: 24,
            disable_credit_flow: false,
            credit_size: 8,
            initial_credits: 24,
        },
        false,
        7,
    ) else {
        return false;
    };
    let mut session = ScanSession::new();
    let Ok(start) = queue_start_scan(
        &mut session,
        &mut queue,
        &request,
        0x8000_1000,
        2,
        1,
        &mut frame,
    ) else {
        return false;
    };

    let start_transfer = start.transfer.transfer;
    let start_descriptor = start_transfer.descriptor.as_bytes();
    if !(start.transfer.slot == 0
        && start_transfer.tx.sequence_number == 7
        && start_transfer.tx.credit_update_requested
        && start_transfer.tx.credits_required == 13
        && start_transfer.tx.credits_remaining + start_transfer.tx.credits_required == 24
        && start_transfer.tx.frame_bytes == 8 + start_transfer.tx.wmi_payload_bytes
        && start_transfer.tx.wmi_payload_bytes == 4 + start.command.payload_bytes
        && frame[0] == 3
        && frame[1] == 1
        && u16::from_le_bytes([frame[2], frame[3]]) as usize == start_transfer.tx.wmi_payload_bytes
        && frame[5] == 7
        && frame[8..12] == WMI_10_4_START_SCAN_COMMAND_ID.to_le_bytes()
        && u32::from_le_bytes(start_descriptor[0..4].try_into().unwrap()) == 0x8000_1000
        && u16::from_le_bytes(start_descriptor[8..10].try_into().unwrap()) as usize
            == start_transfer.tx.frame_bytes
        && u16::from_le_bytes(start_descriptor[10..12].try_into().unwrap()) == 0x0020
        && queue.credits_available() == Ok(11)
        && queue.next_sequence_number() == 8
        && queue.ring_indices() == (1, 0))
    {
        return false;
    }

    if session.observe_event(ScanEvent {
        event_type: WMI_SCAN_EVENT_STARTED,
        reason: 0,
        channel_frequency_mhz: 0,
        requestor_id: 0xa001,
        scan_id: 0xa000,
        vdev_id: 1,
    }) != ScanEventAction::Started
    {
        return false;
    }
    let Ok(stop) = queue_stop_scan(&mut session, &mut queue, 0x8000_2000, 3, 2, &mut frame) else {
        return false;
    };
    let stop_transfer = stop.transfer.transfer;
    let stop_descriptor = stop_transfer.descriptor.as_bytes();
    if !(stop.command.command_id == WMI_10_4_STOP_SCAN_COMMAND_ID
        && stop.command.payload_bytes == 16
        && stop.transfer.slot == 1
        && stop_transfer.tx.sequence_number == 8
        && stop_transfer.tx.credits_required == 4
        && stop_transfer.tx.frame_bytes == 28
        && frame[0] == 3
        && frame[1] == 1
        && u16::from_le_bytes([frame[2], frame[3]]) as usize == 20
        && frame[5] == 8
        && frame[8..12] == WMI_10_4_STOP_SCAN_COMMAND_ID.to_le_bytes()
        && u32::from_le_bytes(frame[12..16].try_into().unwrap()) == 0xa001
        && u32::from_le_bytes(frame[16..20].try_into().unwrap()) == 0xa000
        && u32::from_le_bytes(frame[20..24].try_into().unwrap()) == 0
        && u32::from_le_bytes(frame[24..28].try_into().unwrap()) == 1
        && u32::from_le_bytes(stop_descriptor[0..4].try_into().unwrap()) == 0x8000_2000
        && u16::from_le_bytes(stop_descriptor[8..10].try_into().unwrap()) as usize == 28
        && u16::from_le_bytes(stop_descriptor[10..12].try_into().unwrap()) == 0x0030
        && session.phase() == ScanPhase::Aborting
        && queue.credits_available() == Ok(7)
        && queue.next_sequence_number() == 9
        && queue.ring_indices() == (2, 0))
    {
        return false;
    }

    let mut completed = [0u32; 2];
    queue.reclaim_to(2, &mut completed) == Ok(2)
        && completed == [1, 2]
        && queue.credits_available() == Ok(7)
        && queue.ring_indices() == (2, 2)
}

fn key_frame_codec_ok() -> bool {
    // Synthetic test bytes exercise the source-audited CCMP pairwise/group
    // command and SEC_IND parser. Queue entries and descriptors stay in
    // ordinary memory; nothing is published to a CE ring or radio.
    let key = [0x5a; CCMP128_KEY_BYTES];
    let endpoint = HtcEndpoint {
        service_id: HTC_WMI_CONTROL_SERVICE_ID,
        endpoint_id: 3,
        max_message_size: 2048,
    };
    let policy = HtcServicePolicy {
        receive_allocation: 64,
        disable_credit_flow: false,
        credit_size: 8,
        initial_credits: 64,
    };
    let Ok(mut queue) = WmiCommandTxQueue::<4>::new_wmi_control(endpoint, policy, false, 11) else {
        return false;
    };
    let mut body = [0u8; VDEV_INSTALL_KEY_FIXED_BYTES + CCMP128_KEY_BYTES];
    let mut frame = [0u8; 256];
    let pairwise = CcmpKeyInstall {
        vdev_id: 1,
        peer_address: [0x02, 0x10, 0x20, 0x30, 0x40, 0x50],
        key_index: 0,
        kind: KeyKind::Pairwise,
        temporal_key: &key,
    };
    let Ok(pairwise_queued) = queue_ccmp_install_key(
        &mut queue,
        pairwise,
        0x8000_3000,
        10,
        0x100,
        &mut body,
        &mut frame,
    ) else {
        return false;
    };
    let Ok(mut tracker) = CcmpKeyInstallTracker::new(
        1,
        1,
        0x0042,
        pairwise.peer_address,
        pairwise.key_index,
        Some(1),
    ) else {
        return false;
    };
    if tracker.record_queued(1, pairwise_queued).is_err() {
        return false;
    }
    if pairwise_queued.command.command_id != WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID
        || pairwise_queued.command.kind != KeyKind::Pairwise
        || pairwise_queued.transfer.slot != 0
        || pairwise_queued.transfer.transfer.tx.sequence_number != 11
        || frame[8..12] != WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID.to_le_bytes()
        || u32::from_le_bytes(frame[12 + 16..12 + 20].try_into().unwrap()) != 0
        || frame[12 + VDEV_INSTALL_KEY_FIXED_BYTES..12 + VDEV_INSTALL_KEY_FIXED_BYTES + 16] != key
    {
        return false;
    }
    let mut pairwise_indication = [0u8; HTT_SECURITY_INDICATION_BYTES];
    pairwise_indication[0] = HTT_10_4_T2H_SECURITY_INDICATION;
    pairwise_indication[1] = HTT_SECURITY_TYPE_AES_CCMP | 0x80;
    pairwise_indication[2..4].copy_from_slice(&0x0042u16.to_le_bytes());
    let Ok(pairwise_indication) = HttSecurityIndication::parse(&pairwise_indication) else {
        return false;
    };
    if tracker.observe_security_indication(1, pairwise_indication)
        != KeyInstallAction::IgnoredBeforePublication
        || tracker.all_required_installed()
    {
        return false;
    }
    // This self-test models publication; it does not submit a CE descriptor.
    if tracker.mark_published(1, KeyKind::Pairwise, pairwise_queued.transfer).is_err() {
        return false;
    }
    if tracker.observe_security_indication(0, pairwise_indication)
        != KeyInstallAction::IgnoredStaleGeneration
        || tracker.observe_security_indication(
            1,
            HttSecurityIndication {
                peer_id: 0x0043,
                ..pairwise_indication
            },
        ) != KeyInstallAction::IgnoredMismatch
        || tracker.observe_security_indication(1, pairwise_indication)
            != (KeyInstallAction::Installed {
                kind: KeyKind::Pairwise,
                all_required_installed: false,
            })
        || tracker.observe_security_indication(1, pairwise_indication)
            != (KeyInstallAction::Duplicate {
                kind: KeyKind::Pairwise,
            })
    {
        return false;
    }

    let group = CcmpKeyInstall {
        vdev_id: 1,
        // In station mode ath10k passes the selected BSSID for both group
        // and pairwise key installation.
        peer_address: pairwise.peer_address,
        key_index: 1,
        kind: KeyKind::Group,
        temporal_key: &key,
    };
    let Ok(group_queued) = queue_ccmp_install_key(
        &mut queue,
        group,
        0x8000_4000,
        11,
        0x101,
        &mut body,
        &mut frame,
    ) else {
        return false;
    };
    if tracker.record_queued(1, group_queued).is_err() {
        return false;
    }
    if group_queued.command.command_id != WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID
        || group_queued.command.kind != KeyKind::Group
        || group_queued.transfer.slot != 1
        || group_queued.transfer.transfer.tx.sequence_number != 12
        || frame[8..12] != WMI_10_4_VDEV_INSTALL_KEY_COMMAND_ID.to_le_bytes()
        || u32::from_le_bytes(frame[12 + 16..12 + 20].try_into().unwrap()) != 1
        || frame[12 + VDEV_INSTALL_KEY_FIXED_BYTES..12 + VDEV_INSTALL_KEY_FIXED_BYTES + 16] != key
    {
        return false;
    }
    let mut group_indication = [0u8; HTT_SECURITY_INDICATION_BYTES];
    group_indication[0] = HTT_10_4_T2H_SECURITY_INDICATION;
    group_indication[1] = HTT_SECURITY_TYPE_AES_CCMP;
    group_indication[2..4].copy_from_slice(&0x0042u16.to_le_bytes());
    let Ok(group_indication) = HttSecurityIndication::parse(&group_indication) else {
        return false;
    };
    if tracker.observe_security_indication(1, group_indication)
        != KeyInstallAction::IgnoredBeforePublication
        || tracker.all_required_installed()
    {
        return false;
    }
    if tracker.mark_published(1, KeyKind::Group, group_queued.transfer).is_err() {
        return false;
    }
    if tracker.observe_security_indication(1, group_indication)
        != (KeyInstallAction::Installed {
            kind: KeyKind::Group,
            all_required_installed: true,
        })
        || !tracker.all_required_installed()
        || queue.credits_available() != Ok(34)
        || queue.next_sequence_number() != 13
        || queue.ring_indices() != (2, 0)
    {
        return false;
    }

    let mut completed = [0u32; 2];
    let Ok(reclaimed) = queue.reclaim_to(2, &mut completed) else {
        return false;
    };
    body.fill(0);
    frame.fill(0);
    reclaimed == 2 && completed == [0x100, 0x101] && queue.ring_indices() == (2, 2)
}

fn ipa_codec_ok() -> bool {
    wlan_protocol_probe_ipa::check().is_ok()
}

fn draw_report(
    passed: bool,
    descriptor_ok: bool,
    lookup_ok: bool,
    ipa_ok: bool,
    wmi_ok: bool,
    htc_tx_ok: bool,
    qcacld_init_ok: bool,
    scan_tx_ok: bool,
    key_frame_ok: bool,
) {
    let framebuffer = memory_region_symbol!(splash_fb: *mut u32).as_ptr();
    let background = 0x00122b52;
    let white = 0x00f4f7ff;
    let cyan = 0x0000e5e5;
    let green = 0x0000d080;
    let red = 0x00ff4050;
    unsafe {
        for buffer in 0..2 {
            let frame = framebuffer.add(buffer * FB_PIXELS);
            for pixel in 0..FB_PIXELS {
                frame.add(pixel).write_volatile(background);
            }
            draw_text(frame, "WLAN PROTOCOL", 72, 420, cyan);
            draw_text(frame, "TARGET LINK CHECK", 72, 560, white);
            draw_text(
                frame,
                if descriptor_ok {
                    "CE DESC PASS"
                } else {
                    "CE DESC FAIL"
                },
                72,
                860,
                if descriptor_ok { green } else { red },
            );
            draw_text(
                frame,
                if lookup_ok { "QRTR PASS" } else { "QRTR FAIL" },
                72,
                1000,
                if lookup_ok { green } else { red },
            );
            draw_text(
                frame,
                if ipa_ok {
                    "IPA CODEC PASS"
                } else {
                    "IPA CODEC FAIL"
                },
                72,
                1140,
                if ipa_ok { green } else { red },
            );
            draw_text(
                frame,
                if wmi_ok {
                    "WMI 10 4 PASS"
                } else {
                    "WMI 10 4 FAIL"
                },
                72,
                1280,
                if wmi_ok { green } else { red },
            );
            draw_text(
                frame,
                if htc_tx_ok {
                    "HTC WMI TX PASS"
                } else {
                    "HTC WMI TX FAIL"
                },
                72,
                1420,
                if htc_tx_ok { green } else { red },
            );
            draw_text(
                frame,
                if scan_tx_ok {
                    "SCAN FRAME PASS"
                } else {
                    "SCAN FRAME FAIL"
                },
                72,
                1560,
                if scan_tx_ok { green } else { red },
            );
            draw_text(
                frame,
                if qcacld_init_ok {
                    "QCACLD INIT PASS"
                } else {
                    "QCACLD INIT FAIL"
                },
                72,
                1700,
                if qcacld_init_ok { green } else { red },
            );
            draw_text(
                frame,
                if key_frame_ok {
                    "KEY FRAME PASS"
                } else {
                    "KEY FRAME FAIL"
                },
                72,
                1840,
                if key_frame_ok { green } else { red },
            );
            draw_text(
                frame,
                if passed {
                    "NO RADIO ACCESS"
                } else {
                    "CHECK FAILED"
                },
                72,
                1980,
                white,
            );
        }
    }
}

unsafe fn draw_text(frame: *mut u32, text: &str, x: usize, y: usize, colour: u32) {
    for (character_index, byte) in text.bytes().enumerate() {
        let glyph = match byte {
            b'A'..=b'Z' => usize::from(byte - b'A'),
            b'0'..=b'9' => 26 + usize::from(byte - b'0'),
            b' ' => continue,
            _ => continue,
        };
        let origin_x = x + character_index * 6 * FB_SCALE;
        for row in 0..7 {
            for column in 0..5 {
                if FONT[glyph][row] & (1 << (4 - column)) == 0 {
                    continue;
                }
                for dy in 0..FB_SCALE {
                    for dx in 0..FB_SCALE {
                        let pixel = (y + row * FB_SCALE + dy) * FB_WIDTH
                            + origin_x
                            + column * FB_SCALE
                            + dx;
                        unsafe { frame.add(pixel).write_volatile(colour) };
                    }
                }
            }
        }
    }
}

struct ProtocolProbe;

impl Handler for ProtocolProbe {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        debug_println!("WLAN-PROTOCOL|FAIL: unexpected notification");
        Ok(())
    }

    fn protected(
        &mut self,
        _channel: Channel,
        _request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        Ok(MessageInfo::new(0, 0))
    }
}
