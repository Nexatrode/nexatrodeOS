//! Bounded WMI 10.4 management-frame command encoding for ath10k.
//!
//! This is only the WMI management-TX path used when firmware advertises the
//! matching feature or service. HTT management submission, TX completion
//! reporting, DMA, channel scheduling, and radio authority stay with the
//! caller.
#![forbid(unsafe_code)]

use crate::ath10k_nwifi_rx::{FrameKind, inspect_frame};
use crate::ath10k_wmi_tx::{Error as WmiTxError, QueuedTransfer, WmiCommandTxQueue};

pub const WMI_10_4_MGMT_TX_COMMAND_ID: u32 = 0x9031;
pub const WMI_MGMT_TX_HEADER_BYTES: usize = 24;
pub const MAX_MANAGEMENT_FRAME_BYTES: usize = 2352;
pub const MAX_MANAGEMENT_COMMAND_BODY_BYTES: usize =
    WMI_MGMT_TX_HEADER_BYTES + MAX_MANAGEMENT_FRAME_BYTES;

const FC_PROTECTED: u16 = 0x4000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    NotManagementFrame,
    ProtectedManagementUnsupported,
    InvalidDestinationAddress,
    FrameTooLarge,
    LengthOverflow,
    OutputTooSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandLayout {
    pub command_id: u32,
    pub body_bytes: usize,
    pub frame_bytes: usize,
    pub padded_bytes: usize,
    pub vdev_id: u32,
    pub peer_address: [u8; 6],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    NotWmiControlEndpoint,
    WmiManagementUnsupported,
    Management(Error),
    Transport(WmiTxError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedManagementTx {
    pub command: CommandLayout,
    pub transfer: QueuedTransfer,
}

/// Encode the WMI 10.4 management-TX payload used by ath10k's WMI path.
///
/// Only unprotected management frames are supported here, which covers probe,
/// open-authentication, and association traffic. For these frame types Linux
/// leaves tx rate/power zero and the command's `buf_len` is the exact frame
/// size. The complete command body is rounded up to four bytes.
pub fn encode_management_tx(
    vdev_id: u32,
    frame: &[u8],
    output: &mut [u8],
) -> Result<CommandLayout, Error> {
    if frame.len() > MAX_MANAGEMENT_FRAME_BYTES {
        return Err(Error::FrameTooLarge);
    }
    let view = inspect_frame(frame).map_err(|_| Error::NotManagementFrame)?;
    if view.kind != FrameKind::Management {
        return Err(Error::NotManagementFrame);
    }
    if view.frame_control & FC_PROTECTED != 0 {
        return Err(Error::ProtectedManagementUnsupported);
    }
    let address = view
        .header
        .get(4..10)
        .ok_or(Error::InvalidDestinationAddress)?;
    let mut peer_address = [0; 6];
    peer_address.copy_from_slice(address);
    if peer_address.iter().all(|byte| *byte == 0) {
        return Err(Error::InvalidDestinationAddress);
    }

    let unpadded_bytes = WMI_MGMT_TX_HEADER_BYTES
        .checked_add(frame.len())
        .ok_or(Error::LengthOverflow)?;
    let padded_bytes = unpadded_bytes
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or(Error::LengthOverflow)?;
    if output.len() < padded_bytes {
        return Err(Error::OutputTooSmall);
    }

    let command = &mut output[..padded_bytes];
    command.fill(0);
    command[..4].copy_from_slice(&vdev_id.to_le_bytes());
    command[4..10].copy_from_slice(&peer_address);
    // MAC-address alignment bytes and tx_rate/tx_power default to zero.
    command[20..24].copy_from_slice(&(frame.len() as u32).to_le_bytes());
    command[WMI_MGMT_TX_HEADER_BYTES..WMI_MGMT_TX_HEADER_BYTES + frame.len()]
        .copy_from_slice(frame);

    Ok(CommandLayout {
        command_id: WMI_10_4_MGMT_TX_COMMAND_ID,
        body_bytes: unpadded_bytes,
        frame_bytes: frame.len(),
        padded_bytes,
        vdev_id,
        peer_address,
    })
}

/// Upstream selects WMI management TX only when either source of capability is
/// present. A false result means this WMI encoder must not be submitted; the
/// alternative HTT path is intentionally not synthesized here.
pub const fn wmi_management_tx_supported(firmware_feature: bool, wmi_service: bool) -> bool {
    firmware_feature || wmi_service
}

/// Serialize an unprotected management frame and queue its WMI command through
/// the caller's already-connected WMI_CONTROL HTC queue. The returned CE
/// descriptor remains a software plan: this function does not publish DMA or
/// submit anything to hardware.
#[allow(clippy::too_many_arguments)]
pub fn queue_management_tx<const RING_SIZE: usize>(
    queue: &mut WmiCommandTxQueue<RING_SIZE>,
    firmware_feature: bool,
    wmi_service: bool,
    vdev_id: u32,
    frame: &[u8],
    ce_iova: u64,
    transfer_id: u16,
    context: u32,
    gather: bool,
    byte_swap: bool,
    htc_frame_output: &mut [u8],
) -> Result<QueuedManagementTx, QueueError> {
    if !queue.is_wmi_control_bound() {
        return Err(QueueError::NotWmiControlEndpoint);
    }
    if !wmi_management_tx_supported(firmware_feature, wmi_service) {
        return Err(QueueError::WmiManagementUnsupported);
    }

    // Keep the nested WMI body in bounded stack storage so an outer HTC/CE
    // preflight failure cannot leave a caller-visible half-encoded command.
    let mut command_body = [0u8; MAX_MANAGEMENT_COMMAND_BODY_BYTES];
    let command =
        encode_management_tx(vdev_id, frame, &mut command_body).map_err(QueueError::Management)?;
    let transfer = queue
        .queue_transfer(
            command.command_id,
            &command_body[..command.padded_bytes],
            ce_iova,
            transfer_id,
            context,
            gather,
            byte_swap,
            htc_frame_output,
        )
        .map_err(QueueError::Transport)?;

    Ok(QueuedManagementTx { command, transfer })
}

#[cfg(test)]
mod tests {
    use crate::ath10k_ce::{
        HTC_WMI_CONTROL_SERVICE_ID, HtcConnectServiceResponse, HtcEndpoint, HtcEndpointMap,
        HtcReady, HtcServicePolicy,
    };
    use crate::ath10k_wmi_tx::Error as WmiTxError;
    use crate::ieee80211_join::{JoinConfig, JoinPhase, Wpa2Join};

    use super::{
        Error, MAX_MANAGEMENT_FRAME_BYTES, QueueError, WMI_10_4_MGMT_TX_COMMAND_ID,
        WMI_MGMT_TX_HEADER_BYTES, WmiCommandTxQueue, encode_management_tx, queue_management_tx,
        wmi_management_tx_supported,
    };

    fn management_frame(frame_control: u16, destination: [u8; 6]) -> Vec<u8> {
        let mut frame = vec![0; 24 + 2];
        frame[..2].copy_from_slice(&frame_control.to_le_bytes());
        frame[4..10].copy_from_slice(&destination);
        frame[10..16].copy_from_slice(&[0x02, 0, 0, 0, 0, 2]);
        frame[16..22].copy_from_slice(&[0x02, 0, 0, 0, 0, 3]);
        frame[24..].copy_from_slice(b"ok");
        frame
    }

    fn wmi_control_queue(initial_credits: u16) -> WmiCommandTxQueue<4> {
        let mut endpoints = HtcEndpointMap::new();
        let endpoint = endpoints
            .connect(
                HTC_WMI_CONTROL_SERVICE_ID,
                HtcConnectServiceResponse {
                    service_id: HTC_WMI_CONTROL_SERVICE_ID,
                    status: 0,
                    endpoint_id: 3,
                    max_message_size: 1024,
                },
            )
            .unwrap();
        let policy = HtcServicePolicy::derive(
            HTC_WMI_CONTROL_SERVICE_ID,
            HtcReady {
                credit_count: initial_credits,
                credit_size: 8,
                max_endpoints: 9,
                version: Some(1),
                max_rx_bundle: Some(8),
                alternate_data_credit_size: None,
            },
        )
        .unwrap();
        WmiCommandTxQueue::new_wmi_control(endpoint, policy, false, 9).unwrap()
    }

    fn join_authentication_response(station: [u8; 6], bssid: [u8; 6]) -> [u8; 30] {
        let mut frame = [0; 30];
        frame[..2].copy_from_slice(&0x00b0u16.to_le_bytes());
        frame[4..10].copy_from_slice(&station);
        frame[10..16].copy_from_slice(&bssid);
        frame[16..22].copy_from_slice(&bssid);
        frame[24..30].copy_from_slice(&[0, 0, 2, 0, 0, 0]);
        frame
    }

    fn join_association_response(station: [u8; 6], bssid: [u8; 6]) -> [u8; 30] {
        let mut frame = [0; 30];
        frame[..2].copy_from_slice(&0x0010u16.to_le_bytes());
        frame[4..10].copy_from_slice(&station);
        frame[10..16].copy_from_slice(&bssid);
        frame[16..22].copy_from_slice(&bssid);
        frame[24..30].copy_from_slice(&[0x11, 0, 0, 0, 1, 0xc0]);
        frame
    }

    fn assert_queued_management_frame(output: &[u8], frame: &[u8], sequence_number: u8) {
        let padded_command_body_bytes = (WMI_MGMT_TX_HEADER_BYTES + frame.len() + 3) & !3;
        let expected_htc_bytes = 8 + 4 + padded_command_body_bytes;
        assert_eq!(output[0], 3); // negotiated WMI_CONTROL endpoint
        assert_eq!(output[1], 1); // credit-update request flag
        assert_eq!(
            u16::from_le_bytes([output[2], output[3]]) as usize,
            expected_htc_bytes - 8
        );
        assert_eq!(output[5], sequence_number);
        assert_eq!(&output[8..12], &WMI_10_4_MGMT_TX_COMMAND_ID.to_le_bytes());
        assert_eq!(
            &output
                [8 + 4 + WMI_MGMT_TX_HEADER_BYTES..8 + 4 + WMI_MGMT_TX_HEADER_BYTES + frame.len()],
            frame
        );
        assert!(
            output[expected_htc_bytes..]
                .iter()
                .all(|byte| *byte == 0xa5)
        );
    }

    #[test]
    fn generic_or_non_wmi_endpoint_cannot_queue_management_commands() {
        let frame = management_frame(0x00b0, [0x02, 0, 0, 0, 0, 9]);
        let mut generic = WmiCommandTxQueue::<4>::new(3, 1024, 8, 24, true, false, 9).unwrap();
        let mut output = [0xa5; 128];
        assert_eq!(
            queue_management_tx(
                &mut generic,
                true,
                false,
                1,
                &frame,
                0x8000_1000,
                7,
                0x55aa,
                false,
                false,
                &mut output,
            ),
            Err(QueueError::NotWmiControlEndpoint)
        );
        assert_eq!(generic.next_sequence_number(), 9);
        assert_eq!(generic.ring_indices(), (0, 0));
        assert!(output.iter().all(|byte| *byte == 0xa5));

        let wrong_service = HtcEndpoint {
            service_id: 0x0300,
            endpoint_id: 3,
            max_message_size: 1024,
        };
        let valid_control_policy = HtcServicePolicy::derive(
            HTC_WMI_CONTROL_SERVICE_ID,
            HtcReady {
                credit_count: 24,
                credit_size: 8,
                max_endpoints: 9,
                version: Some(1),
                max_rx_bundle: Some(8),
                alternate_data_credit_size: None,
            },
        )
        .unwrap();
        assert_eq!(
            WmiCommandTxQueue::<4>::new_wmi_control(wrong_service, valid_control_policy, false, 9,),
            Err(crate::ath10k_wmi_tx::Error::WrongService)
        );
        assert_eq!(
            WmiCommandTxQueue::<4>::new_wmi_control(
                HtcEndpoint {
                    service_id: HTC_WMI_CONTROL_SERVICE_ID,
                    endpoint_id: 3,
                    max_message_size: 1024,
                },
                HtcServicePolicy {
                    disable_credit_flow: true,
                    ..valid_control_policy
                },
                false,
                9,
            ),
            Err(crate::ath10k_wmi_tx::Error::InvalidWmiControlPolicy)
        );
    }

    #[test]
    fn command_body_matches_wmi_10_4_mgmt_tx_layout_and_padding() {
        let frame = management_frame(0x00b0, [0x02, 0, 0, 0, 0, 9]);
        let mut output = [0xa5; 64];
        let layout = encode_management_tx(3, &frame, &mut output).unwrap();
        assert_eq!(layout.command_id, WMI_10_4_MGMT_TX_COMMAND_ID);
        assert_eq!(layout.vdev_id, 3);
        assert_eq!(layout.peer_address, [0x02, 0, 0, 0, 0, 9]);
        assert_eq!(layout.body_bytes, WMI_MGMT_TX_HEADER_BYTES + frame.len());
        assert_eq!(layout.padded_bytes, layout.body_bytes + 2);
        assert_eq!(&output[..4], &3u32.to_le_bytes());
        assert_eq!(&output[4..10], &layout.peer_address);
        assert_eq!(&output[10..16], &[0; 6]);
        assert_eq!(&output[12..20], &[0; 8]);
        assert_eq!(&output[20..24], &(frame.len() as u32).to_le_bytes());
        assert_eq!(&output[24..24 + frame.len()], &frame);
        assert_eq!(&output[layout.body_bytes..layout.padded_bytes], &[0, 0]);
        assert!(
            output[layout.padded_bytes..]
                .iter()
                .all(|byte| *byte == 0xa5)
        );
    }

    #[test]
    fn probe_destination_and_non_word_frame_length_use_broadcast_and_zero_padding() {
        let frame = management_frame(0x0040, [0xff; 6]);
        let mut output = [0xa5; 64];
        let layout = encode_management_tx(0, &frame, &mut output).unwrap();
        assert_eq!(layout.peer_address, [0xff; 6]);
        assert_eq!(layout.body_bytes % 4, 2);
        assert_eq!(layout.padded_bytes, layout.body_bytes + 2);
        assert_eq!(&output[layout.body_bytes..layout.padded_bytes], &[0, 0]);
    }

    #[test]
    fn rejects_data_protected_malformed_and_oversized_frames_without_output_changes() {
        let mut output = [0xa5; 64];
        assert_eq!(
            encode_management_tx(0, &management_frame(0x0208, [0x02; 6]), &mut output),
            Err(Error::NotManagementFrame)
        );
        assert_eq!(
            encode_management_tx(0, &management_frame(0x40b0, [0x02; 6]), &mut output),
            Err(Error::ProtectedManagementUnsupported)
        );
        let mut malformed = management_frame(0x00b0, [0x02; 6]);
        malformed.truncate(10);
        assert_eq!(
            encode_management_tx(0, &malformed, &mut output),
            Err(Error::NotManagementFrame)
        );
        let oversized = vec![0; MAX_MANAGEMENT_FRAME_BYTES + 1];
        assert_eq!(
            encode_management_tx(0, &oversized, &mut output),
            Err(Error::FrameTooLarge)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }

    #[test]
    fn short_output_is_atomic_and_wmi_path_requires_advertised_capability() {
        let frame = management_frame(0x00b0, [0x02, 0, 0, 0, 0, 9]);
        let mut output = [0xa5; 32];
        assert_eq!(
            encode_management_tx(0, &frame, &mut output),
            Err(Error::OutputTooSmall)
        );
        assert!(output.iter().all(|byte| *byte == 0xa5));
        assert!(!wmi_management_tx_supported(false, false));
        assert!(wmi_management_tx_supported(true, false));
        assert!(wmi_management_tx_supported(false, true));
    }

    #[test]
    fn management_command_composes_through_wmi_htc_and_ce_software_queue() {
        let frame = management_frame(0x00b0, [0x02, 0, 0, 0, 0, 9]);
        let mut queue = wmi_control_queue(24);
        assert!(queue.is_wmi_control_bound());
        let mut output = [0xa5; 128];
        let queued = queue_management_tx(
            &mut queue,
            true,
            false,
            1,
            &frame,
            0x8000_1000,
            7,
            0x55aa,
            false,
            false,
            &mut output,
        )
        .unwrap();

        let transfer = queued.transfer.transfer;
        let frame_bytes = transfer.tx.frame_bytes;
        assert_eq!(queued.command.command_id, WMI_10_4_MGMT_TX_COMMAND_ID);
        assert_eq!(queued.command.peer_address, [0x02, 0, 0, 0, 0, 9]);
        assert_eq!(transfer.tx.sequence_number, 9);
        assert_eq!(transfer.tx.credits_required, 8);
        assert_eq!(queued.transfer.slot, 0);
        assert_eq!(output[0], 3);
        assert_eq!(output[1], 1);
        assert_eq!(u16::from_le_bytes([output[2], output[3]]), 56);
        assert_eq!(output[5], 9);
        assert_eq!(&output[8..12], &WMI_10_4_MGMT_TX_COMMAND_ID.to_le_bytes());
        assert_eq!(&output[12..16], &1u32.to_le_bytes());
        assert_eq!(&output[16..22], &[0x02, 0, 0, 0, 0, 9]);
        assert_eq!(&output[32..36], &(frame.len() as u32).to_le_bytes());
        assert_eq!(&output[36..36 + frame.len()], &frame);
        assert_eq!(&output[36 + frame.len()..frame_bytes], &[0, 0]);
        assert_eq!(
            u32::from_le_bytes(transfer.descriptor.as_bytes()[..4].try_into().unwrap()),
            0x8000_1000
        );
        assert_eq!(queue.credits_available(), Ok(16));
        assert_eq!(queue.next_sequence_number(), 10);
        assert_eq!(queue.ring_indices(), (1, 0));
    }

    #[test]
    fn selected_bss_auth_and_association_frames_reach_wmi_control_queue() {
        // This is a host-only protocol/transport composition test. The CE
        // descriptors and IOVAs are plans; nothing is mapped or submitted.
        const STATION: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
        const BSSID: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
        const RSN_IE: [u8; 22] = [
            48, 20, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 4, 1, 0, 0, 15, 172, 2, 0, 0,
        ];

        let mut join = Wpa2Join::new(JoinConfig {
            association_generation: 1,
            station: STATION,
            bssid: BSSID,
            initial_sequence_number: 0x0fff,
            ssid: b"test-network",
            passphrase: b"correct horse battery",
            rsn_ie: &RSN_IE,
            supported_rates: &[0x82, 0x84, 0x8b, 0x96],
            extended_supported_rates: &[],
            additional_ies: &[],
            capability_info: 0x0011,
            listen_interval: 10,
            snonce: [0x33; 32],
        })
        .unwrap();
        let mut queue = wmi_control_queue(48);
        let mut output = [0xa5; 512];
        let mut frame = [0xa5; 256];

        let auth_len = join.begin(&mut frame).unwrap();
        let auth = &frame[..auth_len];
        assert_eq!(auth_len, 30);
        assert_eq!(join.phase(), JoinPhase::AwaitingAuthenticationResponse);
        let queued_auth = queue_management_tx(
            &mut queue,
            true,
            false,
            1,
            auth,
            0x8000_1000,
            1,
            0x101,
            false,
            false,
            &mut output,
        )
        .unwrap();
        assert_eq!(queued_auth.command.command_id, WMI_10_4_MGMT_TX_COMMAND_ID);
        assert_eq!(queued_auth.command.peer_address, BSSID);
        assert_eq!(queued_auth.transfer.slot, 0);
        assert_queued_management_frame(&output, auth, 9);
        assert_eq!(queue.next_sequence_number(), 10);

        let assoc_len = join
            .process_authentication_response(
                &join_authentication_response(STATION, BSSID),
                &mut frame,
            )
            .unwrap();
        let association = &frame[..assoc_len];
        assert_eq!(join.phase(), JoinPhase::AwaitingAssociationResponse);
        output.fill(0xa5);
        let queued_assoc = queue_management_tx(
            &mut queue,
            true,
            false,
            1,
            association,
            0x8000_2000,
            2,
            0x202,
            false,
            false,
            &mut output,
        )
        .unwrap();
        assert_eq!(queued_assoc.command.command_id, WMI_10_4_MGMT_TX_COMMAND_ID);
        assert_eq!(queued_assoc.command.peer_address, BSSID);
        assert_eq!(queued_assoc.transfer.slot, 1);
        assert_queued_management_frame(&output, association, 10);
        assert_eq!(queue.next_sequence_number(), 11);

        let association_id = join
            .process_association_response(&join_association_response(STATION, BSSID))
            .unwrap();
        assert_eq!(association_id, 1);
        assert_eq!(join.phase(), JoinPhase::AwaitingMessage1);
        assert!(!join.is_authorized());
        assert_eq!(queue.ring_indices(), (2, 0));
    }

    #[test]
    fn unsupported_or_credit_starved_management_tx_does_not_commit_output_or_queue_state() {
        let frame = management_frame(0x00b0, [0x02, 0, 0, 0, 0, 9]);
        let mut queue = wmi_control_queue(1);
        let mut output = [0xa5; 128];

        assert_eq!(
            queue_management_tx(
                &mut queue,
                false,
                false,
                1,
                &frame,
                0x8000_1000,
                7,
                0x55aa,
                false,
                false,
                &mut output,
            ),
            Err(QueueError::WmiManagementUnsupported)
        );
        assert_eq!(queue.next_sequence_number(), 9);
        assert_eq!(queue.ring_indices(), (0, 0));
        assert!(output.iter().all(|byte| *byte == 0xa5));

        assert_eq!(
            queue_management_tx(
                &mut queue,
                false,
                true,
                1,
                &frame,
                0x8000_1000,
                7,
                0x55aa,
                false,
                false,
                &mut output,
            ),
            Err(QueueError::Transport(WmiTxError::InsufficientCredits))
        );
        assert_eq!(queue.next_sequence_number(), 9);
        assert_eq!(queue.ring_indices(), (0, 0));
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }
}
