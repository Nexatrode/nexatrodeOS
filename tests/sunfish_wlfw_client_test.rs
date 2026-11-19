#![allow(dead_code)]

#[path = "../rust/src/ath10k_board.rs"]
mod ath10k_board;
#[path = "../rust/src/qcom_wlan_power.rs"]
mod qcom_wlan_power;
#[path = "../rust/src/qcom_glink_qmi.rs"]
mod qcom_glink_qmi;
#[path = "../rust/src/qcom_ipa.rs"]
mod qcom_ipa;
#[path = "../rust/src/qcom_ipa_service.rs"]
#[allow(dead_code)]
mod qcom_ipa_service;
#[path = "../rust/src/qcom_ipa_client.rs"]
mod qcom_ipa_client;
#[path = "../rust/src/qcom_modem_handlers.rs"]
mod qcom_modem_handlers;
#[path = "../rust/src/qcom_modem_pools.rs"]
mod qcom_modem_pools;
#[path = "../rust/src/qcom_modem_services.rs"]
mod qcom_modem_services;
#[path = "../rust/src/qcom_msa.rs"]
mod qcom_msa;
#[path = "../rust/src/qcom_msa_handoff.rs"]
mod qcom_msa_handoff;
#[path = "../rust/src/qcom_msa_native.rs"]
mod qcom_msa_native;
#[path = "../rust/src/qcom_qmi.rs"]
mod qcom_qmi;
#[path = "../rust/src/qcom_qmi_router.rs"]
mod qcom_qmi_router;
#[path = "../rust/src/qcom_qrtr_control.rs"]
mod qcom_qrtr_control;
#[path = "../rust/src/qcom_scm_assign.rs"]
mod qcom_scm_assign;
#[path = "../rust/src/qcom_servreg.rs"]
pub mod qcom_servreg;
#[path = "../rust/src/qcom_servreg_notif.rs"]
pub mod qcom_servreg_notif;
#[path = "../rust/src/qcom_servreg_notif_client.rs"]
pub mod qcom_servreg_notif_client;
#[path = "../rust/src/qcom_tftp.rs"]
pub mod qcom_tftp;
#[path = "../rust/src/qcom_wlfw_bdf.rs"]
mod qcom_wlfw_bdf;
#[path = "../rust/src/qcom_wlfw_client.rs"]
mod qcom_wlfw_client;
#[path = "../rust/src/sunfish_ipa_profile.rs"]
mod sunfish_ipa_profile;
#[path = "../rust/src/sunfish_qcacld_profile.rs"]
mod sunfish_qcacld_profile;
#[path = "../rust/src/wlfw_protocol.rs"]
mod wlfw_protocol;

use qcom_modem_handlers::Fault;
use qcom_modem_pools::Call;
use qcom_msa::{Reservation, SUNFISH_MSA_BASE, SUNFISH_MSA_BYTES};
use qcom_msa_handoff::{Handoff, Io as HandoffIo, LiveObservation};
use qcom_qmi_router::Address;
use qcom_qrtr_control::{Server, WLFW_SERVICE_ID, WLFW_VERSION_INSTANCE};
use qcom_wlfw_client::{Client, Error, RegistrationOutcome, State};
use wlfw_protocol::WLFW_IND_REGISTER_MESSAGE_ID;

const LOCAL: Address = Address {
    node: 1,
    port: 0x4100,
};
const SERVER: Address = Address { node: 5, port: 700 };

fn server(address: Address) -> Server {
    Server {
        service: WLFW_SERVICE_ID,
        instance: WLFW_VERSION_INSTANCE,
        address,
    }
}

fn response(transaction_id: u16, result: u16, error: u16, fw_status: Option<u64>) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend_from_slice(&transaction_id.to_le_bytes());
    qmi.extend_from_slice(&WLFW_IND_REGISTER_MESSAGE_ID.to_le_bytes());
    let mut payload = vec![2, 4, 0];
    payload.extend_from_slice(&result.to_le_bytes());
    payload.extend_from_slice(&error.to_le_bytes());
    if let Some(status) = fw_status {
        payload.extend_from_slice(&[0x10, 8, 0]);
        payload.extend_from_slice(&status.to_le_bytes());
    }
    qmi.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    qmi.extend_from_slice(&payload);

    let mut datagram = vec![0; 32];
    for (index, value) in [
        1u32,
        1,
        SERVER.node,
        SERVER.port,
        0,
        qmi.len() as u32,
        LOCAL.node,
        LOCAL.port,
    ]
    .iter()
    .enumerate()
    {
        datagram[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    datagram.extend_from_slice(&qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn msa_info_response(transaction_id: u16, confirm_rx: bool, regions: &[(u64, u32, u8)]) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend_from_slice(&transaction_id.to_le_bytes());
    qmi.extend_from_slice(&wlfw_protocol::WLFW_MSA_INFO_MESSAGE_ID.to_le_bytes());
    let mut payload = vec![2, 4, 0, 0, 0, 0, 0];
    let mut region_value = vec![regions.len() as u8];
    for (address, size, secure) in regions {
        region_value.extend_from_slice(&address.to_le_bytes());
        region_value.extend_from_slice(&size.to_le_bytes());
        region_value.push(*secure);
    }
    payload.extend_from_slice(&[3, region_value.len() as u8, 0]);
    payload.extend_from_slice(&region_value);
    qmi.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    qmi.extend_from_slice(&payload);

    let mut datagram = vec![0; 32];
    for (index, value) in [
        1u32,
        1,
        SERVER.node,
        SERVER.port,
        u32::from(confirm_rx),
        qmi.len() as u32,
        LOCAL.node,
        LOCAL.port,
    ]
    .iter()
    .enumerate()
    {
        datagram[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    datagram.extend_from_slice(&qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn status_response(
    transaction_id: u16,
    message_id: u16,
    result: u16,
    error: u16,
    confirm_rx: bool,
    optional_tlvs: &[(u8, &[u8])],
) -> Vec<u8> {
    let mut qmi = vec![2];
    qmi.extend_from_slice(&transaction_id.to_le_bytes());
    qmi.extend_from_slice(&message_id.to_le_bytes());
    let mut payload = vec![2, 4, 0];
    payload.extend_from_slice(&result.to_le_bytes());
    payload.extend_from_slice(&error.to_le_bytes());
    for (kind, value) in optional_tlvs {
        payload.extend_from_slice(&[*kind, value.len() as u8, 0]);
        payload.extend_from_slice(value);
    }
    qmi.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    qmi.extend_from_slice(&payload);

    let mut datagram = vec![0; 32];
    for (index, value) in [
        1u32,
        1,
        SERVER.node,
        SERVER.port,
        u32::from(confirm_rx),
        qmi.len() as u32,
        LOCAL.node,
        LOCAL.port,
    ]
    .iter()
    .enumerate()
    {
        datagram[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    datagram.extend_from_slice(&qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

fn startup_indication(message_id: u16, confirm_rx: bool, transaction_id: u16) -> Vec<u8> {
    let mut qmi = vec![4];
    qmi.extend_from_slice(&transaction_id.to_le_bytes());
    qmi.extend_from_slice(&message_id.to_le_bytes());
    qmi.extend_from_slice(&[0, 0]);
    let mut datagram = vec![0; 32];
    for (index, value) in [
        1u32,
        1,
        SERVER.node,
        SERVER.port,
        u32::from(confirm_rx),
        qmi.len() as u32,
        LOCAL.node,
        LOCAL.port,
    ]
    .iter()
    .enumerate()
    {
        datagram[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    datagram.extend_from_slice(&qmi);
    datagram.resize((datagram.len() + 3) & !3, 0);
    datagram
}

struct MsaHandoffIo {
    observation: LiveObservation,
    invoked: usize,
}

impl HandoffIo for MsaHandoffIo {
    fn observe(&mut self) -> Result<LiveObservation, Fault> {
        Ok(self.observation)
    }

    fn stage(&mut self, _call: &Call) -> Result<(), Fault> {
        Ok(())
    }

    fn invoke(&mut self, _call: &Call) -> Result<[u64; 8], Fault> {
        self.invoked += 1;
        Ok([0; 8])
    }

    fn complete(&mut self) -> Result<(), Fault> {
        Ok(())
    }
}

fn msa_ready_permit(generation: u64, identity: u64) -> qcom_msa::MsaReadyPermit {
    let regions = [
        qcom_msa::MsaRegion {
            base: SUNFISH_MSA_BASE,
            bytes: 0x4000,
            secure: true,
        },
        qcom_msa::MsaRegion {
            base: SUNFISH_MSA_BASE + 0x4000,
            bytes: 0x1fc000,
            secure: false,
        },
    ];
    let plan = qcom_msa::plan_handoff(
        Reservation::validate(
            generation,
            identity,
            SUNFISH_MSA_BASE,
            SUNFISH_MSA_BYTES,
            true,
            true,
            &[],
        )
        .unwrap(),
        &regions,
    )
    .unwrap();
    let io = MsaHandoffIo {
        observation: LiveObservation {
            generation,
            identity,
            ready: true,
            contract: true,
            initial_hlos: true,
        },
        invoked: 0,
    };
    let mut handoff = Handoff::new(plan, io).unwrap();
    assert!(!handoff.step().unwrap());
    assert!(handoff.step().unwrap());
    handoff.into_msa_ready_permit().unwrap()
}

fn registered_client() -> Client {
    let mut client = ready_client();
    let request = client.begin_indication_registration(7, 64).unwrap();
    client
        .accept_indication_registration_response(7, &response(request.transaction_id, 0, 0, None))
        .unwrap();
    client
}

fn capability_pending_client() -> (Client, qcom_wlfw_client::OutboundRequest) {
    let mut client = registered_client();
    let msa = client.begin_msa_info(7, msa_reservation(7), 60).unwrap();
    let info = client
        .accept_msa_info_response(
            7,
            &msa_info_response(
                msa.transaction_id,
                false,
                &[
                    (SUNFISH_MSA_BASE, 0x4000, 1),
                    (SUNFISH_MSA_BASE + 0x4000, 0x1fc000, 0),
                ],
            ),
        )
        .unwrap();
    let plan = info.handoff_plan.unwrap();
    let mut permit = msa_ready_permit(plan.generation(), plan.identity());
    let ready = client.begin_msa_ready(7, &mut permit, 40).unwrap();
    client
        .accept_msa_ready_response(
            7,
            &status_response(
                ready.transaction_id,
                wlfw_protocol::WLFW_MSA_READY_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    let capability = client.begin_capability_request(7, 40).unwrap();
    (client, capability)
}

fn msa_reservation(generation: u64) -> Reservation {
    Reservation::validate(
        generation,
        0xabc,
        SUNFISH_MSA_BASE,
        SUNFISH_MSA_BYTES,
        true,
        true,
        &[],
    )
    .unwrap()
}

fn ready_client() -> Client {
    let mut client = Client::new(LOCAL, 7).unwrap();
    assert_eq!(client.observe_server(7, server(SERVER)), Ok(true));
    client
}

#[test]
fn builds_exact_bounded_registration_datagram_and_stops_at_msa_gate() {
    let mut client = Client::new(LOCAL, 7).unwrap();
    assert_eq!(client.state(), State::Searching);
    assert_eq!(client.observe_server(7, server(SERVER)), Ok(true));
    assert_eq!(client.observe_server(7, server(SERVER)), Ok(false));
    assert_eq!(client.state(), State::ReadyToRegister);
    assert_eq!(
        client.begin_indication_registration(7, 59),
        Err(Error::OutputTooSmall)
    );
    assert_eq!(client.state(), State::ReadyToRegister);

    let request = client.begin_indication_registration(7, 64).unwrap();
    assert_eq!(request.len, 60);
    assert_eq!(request.generation, 7);
    assert_eq!(request.source, LOCAL);
    assert_eq!(request.destination, SERVER);
    assert_eq!(request.message_id, WLFW_IND_REGISTER_MESSAGE_ID);
    let words = (0..8)
        .map(|index| {
            u32::from_le_bytes(request.bytes[index * 4..index * 4 + 4].try_into().unwrap())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        words,
        [
            1,
            1,
            LOCAL.node,
            LOCAL.port,
            0,
            26,
            SERVER.node,
            SERVER.port
        ]
    );
    assert_eq!(request.bytes[32], 0);
    assert_eq!(
        u16::from_le_bytes([request.bytes[33], request.bytes[34]]),
        request.transaction_id
    );
    assert_eq!(
        u16::from_le_bytes([request.bytes[35], request.bytes[36]]),
        WLFW_IND_REGISTER_MESSAGE_ID
    );
    assert_eq!(&request.bytes[request.len - 2..request.len], &[0, 0]);
    assert_eq!(client.state(), State::RegistrationPending);

    assert_eq!(
        client.accept_indication_registration_response(
            7,
            &response(request.transaction_id, 0, 0, Some(0x1234)),
        ),
        Ok(RegistrationOutcome::Accepted {
            firmware_status: Some(0x1234),
            confirm_rx: false,
        })
    );
    assert_eq!(client.state(), State::MsaInfoReady);
    assert_eq!(
        client.begin_indication_registration(7, 80),
        Err(Error::AlreadyRegistered)
    );
}

#[test]
fn icnss_default_registration_subscribes_to_fw_rejuvenation() {
    let mut client = ready_client();
    let request = client.begin_icnss_indication_registration(7, 72).unwrap();
    assert_eq!(request.len, 68);
    assert_eq!(request.message_id, WLFW_IND_REGISTER_MESSAGE_ID);
    assert_eq!(
        u32::from_le_bytes(request.bytes[20..24].try_into().unwrap()),
        33
    );
    let qmi = &request.bytes[32..65];
    assert_eq!(qmi[7 + 19..7 + 26], [0x19, 4, 0, 1, 0, 0, 0]);
    client
        .accept_indication_registration_response(7, &response(request.transaction_id, 0, 0, None))
        .unwrap();
    assert_eq!(client.state(), State::MsaInfoReady);
}

#[test]
fn registration_fw_ready_status_skips_msa_handoff_and_is_reported() {
    let mut client = ready_client();
    let request = client.begin_indication_registration(7, 80).unwrap();
    let outcome = client
        .accept_indication_registration_response(
            7,
            &response(
                request.transaction_id,
                0,
                0,
                Some(wlfw_protocol::WLFW_FW_READY_STATUS_BIT),
            ),
        )
        .unwrap();
    assert_eq!(
        outcome,
        RegistrationOutcome::Accepted {
            firmware_status: Some(wlfw_protocol::WLFW_FW_READY_STATUS_BIT),
            confirm_rx: false,
        }
    );
    assert!(client.firmware_ready_seen());
    assert_eq!(client.state(), State::MsaInfoReady);
    assert_eq!(
        client.begin_msa_info(7, msa_reservation(7), 76),
        Err(Error::FirmwareAlreadyReady)
    );
    assert_eq!(client.pending_message_id(), None);
}

#[test]
fn icnss_fw_ready_status_does_not_replace_indication_or_block_msa_info() {
    let mut client = ready_client();
    let request = client.begin_icnss_indication_registration(7, 80).unwrap();
    client
        .accept_indication_registration_response(
            7,
            &response(
                request.transaction_id,
                0,
                0,
                Some(wlfw_protocol::WLFW_FW_READY_STATUS_BIT),
            ),
        )
        .unwrap();

    assert!(client.firmware_ready_seen());
    assert!(!client.firmware_ready_indication_seen());
    let msa = client
        .begin_icnss_msa_info(7, msa_reservation(7), 76)
        .unwrap();
    assert_eq!(msa.message_id, wlfw_protocol::WLFW_MSA_INFO_MESSAGE_ID);
    assert_eq!(
        client.pending_message_id(),
        Some(wlfw_protocol::WLFW_MSA_INFO_MESSAGE_ID)
    );
}

#[test]
fn icnss_already_registered_status_blocks_a_second_msa_info_transfer() {
    let mut client = ready_client();
    let request = client.begin_icnss_indication_registration(7, 80).unwrap();
    client
        .accept_indication_registration_response(
            7,
            &response(
                request.transaction_id,
                0,
                0,
                Some(wlfw_protocol::WLFW_ALREADY_REGISTERED_STATUS_BIT),
            ),
        )
        .unwrap();

    assert_eq!(client.state(), State::IcnssAlreadyRegistered);
    assert_eq!(
        client.begin_icnss_msa_info(7, msa_reservation(7), 76),
        Err(Error::ServerAlreadyRegistered)
    );
    assert_eq!(client.pending_message_id(), None);

    let config = sunfish_qcacld_profile::wlan_config();
    assert_eq!(
        client.begin_icnss_wlan_config(7, config, qcom_wlfw_client::MAX_DATAGRAM_BYTES,),
        Err(Error::FirmwareNotReady)
    );
    client
        .accept_startup_indication(
            7,
            &startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 0),
        )
        .unwrap();
    let wlan_cfg = client
        .begin_icnss_wlan_config(7, config, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    assert_eq!(wlan_cfg.message_id, wlfw_protocol::WLFW_WLAN_CFG_MESSAGE_ID);
    client
        .accept_wlan_config_response(
            7,
            &status_response(
                wlan_cfg.transaction_id,
                wlfw_protocol::WLFW_WLAN_CFG_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    let wlan_mode = client.begin_wlan_mode(7, 56).unwrap();
    assert_eq!(
        wlan_mode.message_id,
        wlfw_protocol::WLFW_WLAN_MODE_MESSAGE_ID
    );
}

#[test]
fn fw_ready_indication_before_registration_response_is_not_lost() {
    let mut client = ready_client();
    let request = client.begin_icnss_indication_registration(7, 80).unwrap();
    client
        .accept_startup_indication(
            7,
            &startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 0),
        )
        .unwrap();
    assert!(client.firmware_ready_indication_seen());

    client
        .accept_indication_registration_response(7, &response(request.transaction_id, 0, 0, None))
        .unwrap();
    assert!(client.firmware_ready_seen());
    assert!(client.firmware_ready_indication_seen());
}

#[test]
fn sends_fixed_msa_info_only_after_registration_and_validates_returned_ranges() {
    let mut client = registered_client();
    assert_eq!(client.state(), State::MsaInfoReady);
    assert_eq!(client.pending_message_id(), None);
    assert_eq!(
        client.begin_msa_info(7, msa_reservation(8), 76),
        Err(Error::StaleGeneration)
    );
    assert_eq!(client.state(), State::MsaInfoReady);
    assert_eq!(
        client.begin_msa_info(7, msa_reservation(7), 59),
        Err(Error::OutputTooSmall)
    );
    assert_eq!(client.state(), State::MsaInfoReady);

    let request = client.begin_msa_info(7, msa_reservation(7), 60).unwrap();
    assert_eq!(request.message_id, wlfw_protocol::WLFW_MSA_INFO_MESSAGE_ID);
    assert_eq!(request.transaction_id, 2);
    assert_eq!(request.len, 60);
    assert_eq!(
        u32::from_le_bytes(request.bytes[20..24].try_into().unwrap()),
        25
    );
    let qmi = &request.bytes[32..57];
    assert_eq!(qmi[0], 0);
    assert_eq!(u16::from_le_bytes(qmi[1..3].try_into().unwrap()), 2);
    assert_eq!(
        u16::from_le_bytes(qmi[3..5].try_into().unwrap()),
        wlfw_protocol::WLFW_MSA_INFO_MESSAGE_ID
    );
    assert_eq!(u16::from_le_bytes(qmi[5..7].try_into().unwrap()), 18);
    assert_eq!(&qmi[7..10], &[1, 8, 0]);
    assert_eq!(
        u64::from_le_bytes(qmi[10..18].try_into().unwrap()),
        SUNFISH_MSA_BASE
    );
    assert_eq!(&qmi[18..21], &[2, 4, 0]);
    assert_eq!(
        u32::from_le_bytes(qmi[21..25].try_into().unwrap()),
        SUNFISH_MSA_BYTES
    );
    assert_eq!(&request.bytes[57..60], &[0, 0, 0]);
    assert_eq!(
        client.pending_message_id(),
        Some(wlfw_protocol::WLFW_MSA_INFO_MESSAGE_ID)
    );
    assert_eq!(client.state(), State::MsaInfoPending);

    let result = client
        .accept_msa_info_response(
            7,
            &msa_info_response(
                request.transaction_id,
                true,
                &[
                    (SUNFISH_MSA_BASE, 0x4000, 1),
                    (SUNFISH_MSA_BASE + 0x4000, 0x1fc000, 0),
                ],
            ),
        )
        .unwrap();
    assert_eq!(result.result, 0);
    assert_eq!(result.error, 0);
    assert_eq!(result.region_count, 2);
    assert_eq!(result.confirm_rx, true);
    assert_eq!(result.regions[0].address, SUNFISH_MSA_BASE);
    assert_eq!(result.regions[0].size, 0x4000);
    assert!(result.regions[0].secure);
    assert_eq!(result.regions[1].address, SUNFISH_MSA_BASE + 0x4000);
    assert_eq!(result.regions[1].size, 0x1fc000);
    assert!(!result.regions[1].secure);
    let plan = result.handoff_plan.unwrap();
    assert_eq!((plan.generation(), plan.identity()), (7, 0xabc));
    assert_eq!(plan.transfers()[0].destination_count(), 2);
    assert_eq!(plan.transfers()[1].destination_count(), 3);
    assert_eq!(client.state(), State::MsaOwnershipRequired);
}

#[test]
fn msa_ready_requires_verified_handoff_then_gates_capability_negotiation() {
    let mut client = registered_client();
    let msa = client.begin_msa_info(7, msa_reservation(7), 60).unwrap();
    let outcome = client
        .accept_msa_info_response(
            7,
            &msa_info_response(
                msa.transaction_id,
                true,
                &[
                    (SUNFISH_MSA_BASE, 0x4000, 1),
                    (SUNFISH_MSA_BASE + 0x4000, 0x1fc000, 0),
                ],
            ),
        )
        .unwrap();
    assert_eq!(client.state(), State::MsaOwnershipRequired);
    let plan = outcome.handoff_plan.unwrap();
    let mut stale_permit = msa_ready_permit(plan.generation(), plan.identity() + 1);
    assert_eq!(
        client.begin_msa_ready(7, &mut stale_permit, 40),
        Err(Error::MsaReadyPermitMismatch)
    );
    assert_eq!(client.state(), State::MsaOwnershipRequired);

    let mut permit = msa_ready_permit(plan.generation(), plan.identity());
    assert_eq!(
        client.begin_msa_ready(7, &mut permit, 39),
        Err(Error::OutputTooSmall)
    );
    let request = client.begin_msa_ready(7, &mut permit, 40).unwrap();
    assert_eq!(client.begin_msa_ready(7, &mut permit, 40), Err(Error::Busy));
    assert_eq!(request.transaction_id, 3);
    assert_eq!(request.message_id, wlfw_protocol::WLFW_MSA_READY_MESSAGE_ID);
    assert_eq!(request.len, 40);
    assert_eq!(
        u32::from_le_bytes(request.bytes[20..24].try_into().unwrap()),
        7
    );
    assert_eq!(
        u16::from_le_bytes(request.bytes[35..37].try_into().unwrap()),
        wlfw_protocol::WLFW_MSA_READY_MESSAGE_ID
    );
    assert_eq!(client.state(), State::MsaReadyPending);
    assert_eq!(
        client.accept_msa_ready_response(
            7,
            &status_response(
                request.transaction_id,
                wlfw_protocol::WLFW_MSA_READY_MESSAGE_ID,
                0,
                0,
                true,
                &[],
            ),
        ),
        Ok(qcom_wlfw_client::MsaReadyOutcome {
            result: 0,
            error: 0,
            confirm_rx: true,
        })
    );
    assert_eq!(client.state(), State::CapabilityReady);

    let capability = client.begin_capability_request(7, 40).unwrap();
    assert_eq!(capability.transaction_id, 4);
    assert_eq!(capability.message_id, wlfw_protocol::WLFW_CAP_MESSAGE_ID);
    assert_eq!(capability.len, 40);
    assert_eq!(
        u16::from_le_bytes(capability.bytes[35..37].try_into().unwrap()),
        wlfw_protocol::WLFW_CAP_MESSAGE_ID
    );
    assert_eq!(client.state(), State::CapabilityPending);

    let chip = [0x34, 0x12, 0, 0, 0x78, 0x56, 0, 0];
    let reply = status_response(
        capability.transaction_id,
        wlfw_protocol::WLFW_CAP_MESSAGE_ID,
        0,
        0,
        false,
        &[(0x10, &chip)],
    );
    let decoded = client.accept_capability_response(7, &reply).unwrap();
    assert_eq!(decoded.details.result, 0);
    assert_eq!(decoded.details.chip_info, Some((0x1234, 0x5678)));
    assert!(!decoded.confirm_rx);
    assert_eq!(client.state(), State::CapabilitiesAccepted);
    assert_eq!(client.pending_message_id(), None);
}

#[test]
fn msa_ready_rejection_is_terminal_and_malformed_capability_keeps_pending() {
    let mut client = registered_client();
    let msa = client.begin_msa_info(7, msa_reservation(7), 60).unwrap();
    let info = client
        .accept_msa_info_response(
            7,
            &msa_info_response(
                msa.transaction_id,
                false,
                &[
                    (SUNFISH_MSA_BASE, 0x4000, 1),
                    (SUNFISH_MSA_BASE + 0x4000, 0x1fc000, 0),
                ],
            ),
        )
        .unwrap();
    let plan = info.handoff_plan.unwrap();
    let mut permit = msa_ready_permit(plan.generation(), plan.identity());
    let request = client.begin_msa_ready(7, &mut permit, 40).unwrap();
    assert_eq!(
        client.accept_msa_ready_response(
            7,
            &status_response(
                request.transaction_id,
                wlfw_protocol::WLFW_MSA_READY_MESSAGE_ID,
                1,
                0x21,
                false,
                &[],
            ),
        ),
        Ok(qcom_wlfw_client::MsaReadyOutcome {
            result: 1,
            error: 0x21,
            confirm_rx: false,
        })
    );
    assert_eq!(client.state(), State::Failed);
    assert_eq!(client.begin_capability_request(7, 40), Err(Error::Failed));

    let mut ready = registered_client();
    let msa = ready.begin_msa_info(7, msa_reservation(7), 60).unwrap();
    let info = ready
        .accept_msa_info_response(
            7,
            &msa_info_response(
                msa.transaction_id,
                false,
                &[
                    (SUNFISH_MSA_BASE, 0x4000, 1),
                    (SUNFISH_MSA_BASE + 0x4000, 0x1fc000, 0),
                ],
            ),
        )
        .unwrap();
    let plan = info.handoff_plan.unwrap();
    let mut permit = msa_ready_permit(plan.generation(), plan.identity());
    let msa_ready = ready.begin_msa_ready(7, &mut permit, 40).unwrap();
    ready
        .accept_msa_ready_response(
            7,
            &status_response(
                msa_ready.transaction_id,
                wlfw_protocol::WLFW_MSA_READY_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    let capability = ready.begin_capability_request(7, 40).unwrap();
    let wrong_endpoint = {
        let mut frame = status_response(
            capability.transaction_id,
            wlfw_protocol::WLFW_CAP_MESSAGE_ID,
            0,
            0,
            false,
            &[],
        );
        frame[12..16].copy_from_slice(&(SERVER.port + 1).to_le_bytes());
        frame
    };
    assert_eq!(
        ready.accept_capability_response(7, &wrong_endpoint),
        Err(Error::WrongEndpoint)
    );
    assert_eq!(ready.state(), State::CapabilityPending);
    assert_eq!(
        ready.pending_message_id(),
        Some(wlfw_protocol::WLFW_CAP_MESSAGE_ID)
    );
}

#[test]
fn maximum_bounded_capability_response_fits_qrtr_datagram_and_is_summarized() {
    let (mut client, request) = capability_pending_client();
    let large_a = [0xa5; 96];
    let large_b = [0x5a; 98];
    let reply = status_response(
        request.transaction_id,
        wlfw_protocol::WLFW_CAP_MESSAGE_ID,
        0,
        0,
        false,
        &[(0x16, &large_a), (0x17, &large_b)],
    );
    assert_eq!(reply.len(), 248);
    assert_eq!(u32::from_le_bytes(reply[20..24].try_into().unwrap()), 214);
    let decoded = client.accept_capability_response(7, &reply).unwrap();
    assert_eq!(decoded.details.unknown_optional_count, 2);
    assert_eq!(client.state(), State::CapabilitiesAccepted);
    assert_eq!(
        client.capability_summary().unwrap().unknown_optional_count,
        2
    );
}

#[test]
fn unsolicited_startup_indications_are_bounded_and_preserve_pending_qmi() {
    let (mut client, capability) = capability_pending_client();
    let msa_ready = startup_indication(wlfw_protocol::WLFW_MSA_READY_IND_MESSAGE_ID, true, 0);
    assert_eq!(
        client.accept_startup_indication(7, &msa_ready),
        Ok(qcom_wlfw_client::IndicationOutcome {
            indication: wlfw_protocol::WlfwIndication::MsaReady,
            confirm_rx: true,
        })
    );
    assert_eq!(client.state(), State::CapabilityPending);
    assert_eq!(
        client.pending_message_id(),
        Some(wlfw_protocol::WLFW_CAP_MESSAGE_ID)
    );

    let mut wrong_peer = startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 0);
    wrong_peer[8..12].copy_from_slice(&(SERVER.node + 1).to_le_bytes());
    assert_eq!(
        client.accept_startup_indication(7, &wrong_peer),
        Err(Error::WrongEndpoint)
    );
    assert!(!client.firmware_ready_seen());
    assert_eq!(
        client.accept_startup_indication(
            7,
            &startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 2),
        ),
        Err(Error::Protocol(
            wlfw_protocol::ProtocolError::WrongTransactionId
        ))
    );
    assert_eq!(client.state(), State::CapabilityPending);
    assert_eq!(client.pending_message_id(), Some(capability.message_id));
    client
        .accept_startup_indication(
            7,
            &startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 0),
        )
        .unwrap();
    assert!(client.firmware_ready_seen());
    assert_eq!(client.state(), State::CapabilityPending);
    assert_eq!(client.pending_message_id(), Some(capability.message_id));
    client.begin_generation(8).unwrap();
    assert!(!client.firmware_ready_seen());
    assert_eq!(client.state(), State::Searching);
    assert_eq!(client.pending_message_id(), None);
}

fn capability_accepted_client(include_msa_ready_indication: bool) -> Client {
    let (mut client, capability) = capability_pending_client();
    if include_msa_ready_indication {
        client
            .accept_startup_indication(
                7,
                &startup_indication(wlfw_protocol::WLFW_MSA_READY_IND_MESSAGE_ID, false, 0),
            )
            .unwrap();
    }
    let board_id = 0x1202u32.to_le_bytes();
    client
        .accept_capability_response(
            7,
            &status_response(
                capability.transaction_id,
                wlfw_protocol::WLFW_CAP_MESSAGE_ID,
                0,
                0,
                false,
                &[(0x11, &board_id)],
            ),
        )
        .unwrap();
    client
}

fn append_test_ie(output: &mut Vec<u8>, id: u32, payload: &[u8]) {
    output.extend_from_slice(&id.to_le_bytes());
    output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    output.extend_from_slice(payload);
    while output.len() % 4 != 0 {
        output.push(0);
    }
}

fn synthetic_board2(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut file = b"QCA-ATH10K-BOARD\0".to_vec();
    while file.len() % 4 != 0 {
        file.push(0);
    }
    for (name, data) in entries {
        let mut section = Vec::new();
        append_test_ie(&mut section, 0, name);
        append_test_ie(&mut section, 1, data);
        append_test_ie(&mut file, 0, &section);
    }
    file
}

fn cal_report_accepted_client() -> Client {
    let bytes = [0x5a; 32];
    let image = qcom_wlfw_bdf::VerifiedBdfImage::test_fixture(&bytes, 0x1202, 7, 0xabc, 1);
    let mut client = capability_accepted_client(true);
    let bdf = client
        .begin_bdf_download(7, &image, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    client
        .accept_bdf_download_response(
            7,
            &status_response(
                bdf.transaction_id,
                wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    let cal = client
        .begin_empty_cal_report(7, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    client
        .accept_cal_report_response(
            7,
            &status_response(
                cal.transaction_id,
                wlfw_protocol::WLFW_CAL_REPORT_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    client
}

#[test]
fn bdf_segments_route_through_client_and_advance_only_on_exact_acknowledgements() {
    let bytes = vec![0xa5; 26_328];
    let image = qcom_wlfw_bdf::VerifiedBdfImage::test_fixture(&bytes, 0x1202, 7, 0xabc, 1);
    let mut client = capability_accepted_client(true);
    let expected = [4023, 4023, 4023, 4023, 4023, 4023, 2190];
    let expected_cumulative = [4023, 8046, 12069, 16092, 20115, 24138, 26328];
    let mut request = client
        .begin_bdf_download(7, &image, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    for (segment, length) in expected.into_iter().enumerate() {
        assert_eq!(
            request.message_id,
            wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID
        );
        assert_eq!(request.len, if segment == 6 { 2264 } else { 4096 });
        assert_eq!(
            u32::from_le_bytes(request.bytes[20..24].try_into().unwrap()),
            41 + length as u32
        );
        assert_eq!(
            u16::from_le_bytes(request.bytes[35..37].try_into().unwrap()),
            wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID
        );
        assert_eq!(
            client.bdf_progress(),
            Some((segment * 4023, segment as u32))
        );
        let outcome = client
            .accept_bdf_download_response(
                7,
                &status_response(
                    request.transaction_id,
                    wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
                    0,
                    0,
                    segment == 2,
                    &[],
                ),
            )
            .unwrap();
        assert_eq!(outcome.completed, segment == 6);
        assert_eq!(outcome.bytes_accepted, expected_cumulative[segment]);
        assert_eq!(outcome.segments_accepted, (segment + 1) as u32);
        if segment < 6 {
            assert_eq!(client.state(), State::BdfReady);
            request = client
                .continue_bdf_download(7, &image, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
                .unwrap();
        }
    }
    assert_eq!(client.state(), State::BdfComplete);
    assert_eq!(
        client.continue_bdf_download(7, &image, 4096),
        Err(Error::NoPendingRequest)
    );
}

#[test]
fn board2_selection_feeds_only_the_exact_sunfish_payload_to_wlfw() {
    let names = ath10k_board::SnocQmiBoardNames::new(
        0x1202,
        0x7150,
        Some(b"google_sunfish"),
    )
    .unwrap();
    let decoy = b"not-the-selected-sunfish-board";
    let selected_payload = b"selected-sunfish-board-bytes";
    let file = synthetic_board2(&[
        (b"bus=snoc,qmi-board-id=1202", decoy),
        (names.as_bytes()[0], selected_payload),
        (names.as_bytes()[1], b"lower-priority-entry"),
    ]);
    let selected = ath10k_board::find_snoc_qmi_board_data(
        &file,
        0x1202,
        0x7150,
        Some(b"google_sunfish"),
    )
    .unwrap();
    assert_eq!(selected.candidate_index, 0);
    assert_eq!(selected.bytes, selected_payload);

    let image = qcom_wlfw_bdf::VerifiedBdfImage::test_fixture(
        selected.bytes,
        0x1202,
        7,
        0xabc,
        1,
    );
    let mut client = capability_accepted_client(true);
    let request = client
        .begin_bdf_download(7, &image, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    let qmi_start = 32;
    let bdf_data_offset = qmi_start + 7 + 4 + 7 + 7 + 7 + 3 + 2;
    assert_eq!(
        &request.bytes[bdf_data_offset..bdf_data_offset + selected_payload.len()],
        selected_payload
    );
    let expected_datagram_len = 32 + 7 + 34 + selected_payload.len();
    assert_eq!(request.len, (expected_datagram_len + 3) & !3);
}

#[test]
fn bdf_transfer_requires_live_board_identity_and_stops_on_mismatch_or_rejection() {
    let bytes = vec![0x5a; 5000];
    let image = qcom_wlfw_bdf::VerifiedBdfImage::test_fixture(&bytes, 0x1202, 7, 0xabc, 1);
    let mut client = capability_accepted_client(false);
    assert_eq!(
        client.begin_bdf_download(7, &image, 4096),
        Err(Error::BdfPrerequisite)
    );

    let mut client = capability_accepted_client(true);
    assert_eq!(
        client.begin_bdf_download(7, &image, 100),
        Err(Error::OutputTooSmall)
    );
    assert_eq!(client.state(), State::CapabilitiesAccepted);
    let wrong_board = qcom_wlfw_bdf::VerifiedBdfImage::test_fixture(&bytes, 0x1203, 7, 0xabc, 1);
    assert_eq!(
        client.begin_bdf_download(7, &wrong_board, 4096),
        Err(Error::BdfImageMismatch)
    );
    let first = client
        .begin_bdf_download(7, &image, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    client
        .accept_bdf_download_response(
            7,
            &status_response(
                first.transaction_id,
                wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    let wrong_asset = qcom_wlfw_bdf::VerifiedBdfImage::test_fixture(&bytes, 0x1202, 7, 0xabc, 2);
    assert_eq!(
        client.continue_bdf_download(7, &wrong_asset, 4096),
        Err(Error::BdfImageMismatch)
    );
    let second = client
        .continue_bdf_download(7, &image, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    let rejected = client
        .accept_bdf_download_response(
            7,
            &status_response(
                second.transaction_id,
                wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
                1,
                7,
                false,
                &[],
            ),
        )
        .unwrap();
    assert_eq!(rejected.result, 1);
    assert!(!rejected.completed);
    assert_eq!(client.bdf_progress(), Some((4023, 1)));
    assert_eq!(client.state(), State::Failed);
}

#[test]
fn empty_cal_report_waits_for_bdf_completion_and_matches_the_exact_response() {
    let bytes = vec![0x5a; 5000];
    let image = qcom_wlfw_bdf::VerifiedBdfImage::test_fixture(&bytes, 0x1202, 7, 0xabc, 1);
    let mut client = capability_accepted_client(true);
    assert_eq!(
        client.begin_empty_cal_report(7, qcom_wlfw_client::MAX_DATAGRAM_BYTES),
        Err(Error::CalReportPrerequisite)
    );

    let first = client
        .begin_bdf_download(7, &image, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    client
        .accept_bdf_download_response(
            7,
            &status_response(
                first.transaction_id,
                wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    assert_eq!(
        client.begin_empty_cal_report(7, qcom_wlfw_client::MAX_DATAGRAM_BYTES),
        Err(Error::CalReportPrerequisite)
    );
    let second = client
        .continue_bdf_download(7, &image, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    client
        .accept_bdf_download_response(
            7,
            &status_response(
                second.transaction_id,
                wlfw_protocol::WLFW_BDF_DOWNLOAD_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    assert_eq!(client.state(), State::BdfComplete);

    let request = client
        .begin_empty_cal_report(7, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    assert_eq!(
        request.message_id,
        wlfw_protocol::WLFW_CAL_REPORT_MESSAGE_ID
    );
    assert_eq!(request.len, 44);
    assert_eq!(
        &request.bytes[32..43],
        &[
            0,
            request.transaction_id as u8,
            (request.transaction_id >> 8) as u8,
            0x26,
            0,
            4,
            0,
            1,
            1,
            0,
            0,
        ]
    );
    assert_eq!(client.state(), State::CalReportPending);
    assert_eq!(
        client.accept_cal_report_response(
            7,
            &status_response(
                request.transaction_id + 1,
                wlfw_protocol::WLFW_CAL_REPORT_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        ),
        Err(Error::Protocol(
            wlfw_protocol::ProtocolError::WrongTransactionId
        ))
    );
    assert_eq!(client.state(), State::CalReportPending);
    let accepted = client
        .accept_cal_report_response(
            7,
            &status_response(
                request.transaction_id,
                wlfw_protocol::WLFW_CAL_REPORT_MESSAGE_ID,
                0,
                0,
                true,
                &[],
            ),
        )
        .unwrap();
    assert_eq!(
        accepted,
        qcom_wlfw_client::CalReportOutcome {
            result: 0,
            error: 0,
            confirm_rx: true,
        }
    );
    assert_eq!(client.state(), State::CalReportAccepted);
}

#[test]
fn wlan_cfg_encoder_matches_the_source_schema_and_enforces_array_limits() {
    use wlfw_protocol::{
        CeServicePipeConfig, CeTargetPipeConfig, ShadowRegisterConfig, ShadowRegisterV2Config,
        WlanConfig,
    };

    let target = [CeTargetPipeConfig {
        pipe_num: 1,
        pipe_dir: 2,
        entries: 16,
        max_bytes: 2048,
        flags: 8,
    }];
    let service = [CeServicePipeConfig {
        service_id: 0x123,
        pipe_dir: 2,
        pipe_num: 3,
    }];
    let shadow = [ShadowRegisterConfig {
        id: 3,
        offset: 0x3c,
    }];
    let shadow_v2 = [ShadowRegisterV2Config {
        address: 0x1234_5000,
    }];
    let config = WlanConfig {
        host_version: None,
        target_pipes: &target,
        service_pipes: &service,
        shadow_registers: &shadow,
        shadow_registers_v2: Some(&shadow_v2),
    };
    let mut encoded = [0u8; 128];
    let len = wlfw_protocol::encode_wlan_cfg_request(0x1234, config, &mut encoded).unwrap();
    let mut expected = vec![0, 0x34, 0x12, 0x23, 0, 68, 0];
    expected.extend_from_slice(&[0x11, 24, 0, 1, 0, 0, 0]);
    for value in [1u32, 2, 16, 2048, 8] {
        expected.extend_from_slice(&value.to_le_bytes());
    }
    expected.extend_from_slice(&[0x12, 16, 0, 1, 0, 0, 0]);
    for value in [0x123u32, 2, 3] {
        expected.extend_from_slice(&value.to_le_bytes());
    }
    expected.extend_from_slice(&[0x13, 8, 0, 1, 0, 0, 0]);
    expected.extend_from_slice(&3u16.to_le_bytes());
    expected.extend_from_slice(&0x3cu16.to_le_bytes());
    expected.extend_from_slice(&[0x14, 8, 0, 1, 0, 0, 0]);
    expected.extend_from_slice(&0x1234_5000u32.to_le_bytes());
    assert_eq!(len, expected.len());
    assert_eq!(&encoded[..len], expected);

    let empty = WlanConfig {
        host_version: None,
        target_pipes: &[],
        service_pipes: &service,
        shadow_registers: &[],
        shadow_registers_v2: Some(&[]),
    };
    assert_eq!(
        wlfw_protocol::encode_wlan_cfg_request(1, empty, &mut encoded),
        Err(wlfw_protocol::ProtocolError::InvalidWlfwField)
    );
    let too_many_targets = [target[0]; wlfw_protocol::WLFW_MAX_CE_TARGET_CFG + 1];
    let oversized = WlanConfig {
        host_version: None,
        target_pipes: &too_many_targets,
        service_pipes: &service,
        shadow_registers: &[],
        shadow_registers_v2: Some(&[]),
    };
    assert_eq!(
        wlfw_protocol::encode_wlan_cfg_request(1, oversized, &mut encoded),
        Err(wlfw_protocol::ProtocolError::InvalidWlfwField)
    );

    let max_targets = [target[0]; wlfw_protocol::WLFW_MAX_CE_TARGET_CFG];
    let max_services = [service[0]; wlfw_protocol::WLFW_MAX_CE_SERVICE_CFG];
    let max_shadow = [shadow[0]; wlfw_protocol::WLFW_MAX_SHADOW_REG_CFG];
    let max_shadow_v2 = [shadow_v2[0]; wlfw_protocol::WLFW_MAX_SHADOW_REG_V2_CFG];
    let maximum = WlanConfig {
        host_version: None,
        target_pipes: &max_targets,
        service_pipes: &max_services,
        shadow_registers: &max_shadow,
        shadow_registers_v2: Some(&max_shadow_v2),
    };
    let mut maximum_encoded = [0u8; 803];
    assert_eq!(
        wlfw_protocol::encode_wlan_cfg_request(1, maximum, &mut maximum_encoded),
        Ok(maximum_encoded.len())
    );
}

#[test]
fn sunfish_icnss_wlan_cfg_matches_downstream_adrastea_profile() {
    let config = sunfish_qcacld_profile::wlan_config();
    assert_eq!(config.host_version, Some("5.2.03.9H"));
    assert_eq!(config.target_pipes.len(), 12);
    assert_eq!(config.service_pipes.len(), 20);
    assert_eq!(config.shadow_registers.len(), 12);
    assert_eq!(config.shadow_registers_v2, None);
    assert_eq!(config.target_pipes[4].flags, 0x08);
    assert_eq!(config.target_pipes[6].pipe_dir, 3);
    assert_eq!(config.target_pipes[7].pipe_dir, 4);
    assert_eq!(config.service_pipes[0].service_id, 0x0104);
    assert_eq!(config.service_pipes[16].service_id, 0x0500);
    assert_eq!(config.service_pipes[19].service_id, 0x0600);
    assert_eq!(config.shadow_registers[11].id, 11);
    assert_eq!(config.shadow_registers[11].offset, 0x40);
    assert_eq!(
        config
            .target_pipes
            .iter()
            .map(|pipe| (
                pipe.pipe_num,
                pipe.pipe_dir,
                pipe.entries,
                pipe.max_bytes,
                pipe.flags,
            ))
            .collect::<Vec<_>>(),
        [
            (0, 2, 32, 2048, 0),
            (1, 1, 32, 2048, 0),
            (2, 1, 64, 2048, 0),
            (3, 2, 32, 2048, 0),
            (4, 2, 256, 256, 8),
            (5, 2, 1024, 64, 8),
            (6, 3, 32, 16384, 0),
            (7, 4, 0, 0, 8),
            (8, 1, 32, 2048, 0),
            (9, 1, 32, 2048, 0),
            (10, 1, 32, 2048, 0),
            (11, 1, 32, 2048, 0),
        ]
    );
    assert_eq!(
        config
            .service_pipes
            .iter()
            .map(|pipe| (pipe.service_id, pipe.pipe_dir, pipe.pipe_num))
            .collect::<Vec<_>>(),
        [
            (0x0104, 2, 3),
            (0x0104, 1, 2),
            (0x0102, 2, 3),
            (0x0102, 1, 2),
            (0x0101, 2, 3),
            (0x0101, 1, 2),
            (0x0103, 2, 3),
            (0x0103, 1, 2),
            (0x0100, 2, 3),
            (0x0100, 1, 2),
            (0x0001, 2, 0),
            (0x0001, 1, 2),
            (0xfe00, 2, 0),
            (0xfe00, 1, 2),
            (0x0300, 2, 4),
            (0x0300, 1, 1),
            (0x0500, 2, 5),
            (0x0301, 1, 9),
            (0x0302, 1, 10),
            (0x0600, 1, 11),
        ]
    );
    assert_eq!(
        config
            .shadow_registers
            .iter()
            .map(|register| (register.id, register.offset))
            .collect::<Vec<_>>(),
        [
            (0, 0x3c),
            (3, 0x3c),
            (4, 0x3c),
            (5, 0x3c),
            (7, 0x3c),
            (1, 0x40),
            (2, 0x40),
            (7, 0x40),
            (8, 0x40),
            (9, 0x40),
            (10, 0x40),
            (11, 0x40),
        ]
    );

    let mut encoded = [0u8; 803];
    let len = wlfw_protocol::encode_wlan_cfg_request(6, config, &mut encoded).unwrap();
    assert_eq!(len, 568);
    assert_eq!(&encoded[..7], &[0, 6, 0, 0x23, 0, 0x31, 0x02]);

    let mut at = 7;
    let mut kinds = Vec::new();
    while at < len {
        let kind = encoded[at];
        let tlv_len = u16::from_le_bytes([encoded[at + 1], encoded[at + 2]]) as usize;
        let value = &encoded[at + 3..at + 3 + tlv_len];
        kinds.push(kind);
        match kind {
            0x10 => assert_eq!(value, b"5.2.03.9H"),
            0x11 => {
                assert_eq!(u32::from_le_bytes(value[..4].try_into().unwrap()), 12);
                assert_eq!(u32::from_le_bytes(value[4..8].try_into().unwrap()), 0);
                assert_eq!(
                    u32::from_le_bytes(value[4 + 7 * 20 + 4..4 + 7 * 20 + 8].try_into().unwrap()),
                    4
                );
            }
            0x12 => assert_eq!(u32::from_le_bytes(value[..4].try_into().unwrap()), 20),
            0x13 => assert_eq!(u32::from_le_bytes(value[..4].try_into().unwrap()), 12),
            _ => panic!("unexpected WLAN_CFG TLV {kind:#x}"),
        }
        at += 3 + tlv_len;
    }
    assert_eq!(kinds, [0x10, 0x11, 0x12, 0x13]);

    let (mut client, capability) = capability_pending_client();
    let board_id = 0x1202u32.to_le_bytes();
    client
        .accept_capability_response(
            7,
            &status_response(
                capability.transaction_id,
                wlfw_protocol::WLFW_CAP_MESSAGE_ID,
                0,
                0,
                false,
                &[(0x11, &board_id)],
            ),
        )
        .unwrap();
    assert_eq!(
        client.begin_icnss_wlan_config(7, config, qcom_wlfw_client::MAX_DATAGRAM_BYTES),
        Err(Error::DynamicFeatureMaskPrerequisite)
    );
    let mask_request = client
        .begin_dynamic_feature_mask(
            7,
            sunfish_qcacld_profile::DEFAULT_DYNAMIC_FEATURE_MASK,
            qcom_wlfw_client::MAX_DATAGRAM_BYTES,
        )
        .unwrap();
    assert_eq!(
        mask_request.message_id,
        wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID
    );
    let mut encoded_mask = [0u8; 32];
    let encoded_mask_len = wlfw_protocol::encode_dynamic_feature_mask_request(
        mask_request.transaction_id,
        sunfish_qcacld_profile::DEFAULT_DYNAMIC_FEATURE_MASK,
        &mut encoded_mask,
    )
    .unwrap();
    assert_eq!(encoded_mask_len, 18);
    assert_eq!(
        &mask_request.bytes[32..32 + encoded_mask_len],
        &encoded_mask[..encoded_mask_len]
    );
    let previous_mask = 0u64.to_le_bytes();
    let current_mask = 1u64.to_le_bytes();
    let mask_outcome = client
        .accept_dynamic_feature_mask_response(
            7,
            &status_response(
                mask_request.transaction_id,
                wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
                0,
                0,
                true,
                &[(0x10, &previous_mask), (0x11, &current_mask)],
            ),
        )
        .unwrap();
    assert_eq!(mask_outcome.current_mask, Some(1));
    assert!(client.dynamic_feature_mask_done());
    assert_eq!(
        client.begin_icnss_wlan_config(7, config, qcom_wlfw_client::MAX_DATAGRAM_BYTES),
        Err(Error::FirmwareNotReady)
    );
    client
        .accept_startup_indication(
            7,
            &startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 0),
        )
        .unwrap();
    let request = client
        .begin_icnss_wlan_config(7, config, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    assert_eq!(&request.bytes[32..request.len], &encoded[..len]);
    let accepted = client
        .accept_wlan_config_response(
            7,
            &status_response(
                request.transaction_id,
                wlfw_protocol::WLFW_WLAN_CFG_MESSAGE_ID,
                0,
                0,
                true,
                &[],
            ),
        )
        .unwrap();
    assert_eq!(accepted.result, 0);
}

#[test]
fn icnss_dynamic_mask_malformed_reply_stays_pending_but_rejection_is_best_effort() {
    let mut client = capability_accepted_client(false);
    let request = client
        .begin_dynamic_feature_mask(
            7,
            sunfish_qcacld_profile::DEFAULT_DYNAMIC_FEATURE_MASK,
            qcom_wlfw_client::MAX_DATAGRAM_BYTES,
        )
        .unwrap();
    let mask = 1u64.to_le_bytes();
    let duplicate = status_response(
        request.transaction_id,
        wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
        0,
        0,
        false,
        &[(0x10, &mask), (0x10, &mask)],
    );
    assert_eq!(
        client.accept_dynamic_feature_mask_response(7, &duplicate),
        Err(Error::Protocol(wlfw_protocol::ProtocolError::DuplicateTlv))
    );
    assert_eq!(client.state(), State::DynamicFeatureMaskPending);
    assert_eq!(
        client.pending_message_id(),
        Some(wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID)
    );

    let rejected = status_response(
        request.transaction_id,
        wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
        1,
        42,
        false,
        &[(0x11, &mask)],
    );
    let outcome = client
        .accept_dynamic_feature_mask_response(7, &rejected)
        .unwrap();
    assert_eq!((outcome.result, outcome.error), (1, 42));
    assert!(client.dynamic_feature_mask_done());
    assert_eq!(client.state(), State::CapabilitiesAccepted);
    client
        .accept_startup_indication(
            7,
            &startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 0),
        )
        .unwrap();
    assert!(client
        .begin_icnss_wlan_config(
            7,
            sunfish_qcacld_profile::wlan_config(),
            qcom_wlfw_client::MAX_DATAGRAM_BYTES,
        )
        .is_ok());
}

#[test]
fn icnss_dynamic_mask_timeout_completes_only_the_pending_mask_step() {
    let mut client = capability_accepted_client(false);
    let request = client
        .begin_dynamic_feature_mask(
            7,
            sunfish_qcacld_profile::DEFAULT_DYNAMIC_FEATURE_MASK,
            qcom_wlfw_client::MAX_DATAGRAM_BYTES,
        )
        .unwrap();
    assert_eq!(client.complete_dynamic_feature_mask_timeout(7), Ok(()));
    assert_eq!(client.state(), State::CapabilitiesAccepted);
    assert!(client.dynamic_feature_mask_done());
    assert_eq!(client.pending_message_id(), None);
    client
        .accept_startup_indication(
            7,
            &startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 0),
        )
        .unwrap();
    assert!(client
        .begin_icnss_wlan_config(
            7,
            sunfish_qcacld_profile::wlan_config(),
            qcom_wlfw_client::MAX_DATAGRAM_BYTES,
        )
        .is_ok());
    assert_eq!(
        client.accept_dynamic_feature_mask_response(
            7,
            &status_response(
                request.transaction_id,
                wlfw_protocol::WLFW_DYNAMIC_FEATURE_MASK_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        ),
        Err(Error::NoPendingRequest)
    );
}

#[test]
fn wlan_cfg_host_version_is_bounded_ascii_and_nul_free() {
    use wlfw_protocol::{CeServicePipeConfig, CeTargetPipeConfig, WlanConfig};
    let target = [CeTargetPipeConfig {
        pipe_num: 0,
        pipe_dir: 2,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    }];
    let service = [CeServicePipeConfig {
        service_id: 1,
        pipe_dir: 2,
        pipe_num: 3,
    }];
    let mut output = [0u8; 803];
    for host_version in [Some("12345678901234567"), Some("bad\0version"), Some("☃")] {
        let config = WlanConfig {
            host_version,
            target_pipes: &target,
            service_pipes: &service,
            shadow_registers: &[],
            shadow_registers_v2: None,
        };
        assert_eq!(
            wlfw_protocol::encode_wlan_cfg_request(1, config, &mut output),
            Err(wlfw_protocol::ProtocolError::InvalidWlfwString)
        );
    }
}

#[test]
fn wlan_config_and_mission_mode_require_fw_ready_and_exact_ack_order() {
    use wlfw_protocol::{CeServicePipeConfig, CeTargetPipeConfig, WlanConfig};

    let target = [CeTargetPipeConfig {
        pipe_num: 0,
        pipe_dir: 2,
        entries: 32,
        max_bytes: 2048,
        flags: 0,
    }];
    let service = [CeServicePipeConfig {
        service_id: 1,
        pipe_dir: 2,
        pipe_num: 3,
    }];
    let config = WlanConfig {
        host_version: None,
        target_pipes: &target,
        service_pipes: &service,
        shadow_registers: &[],
        shadow_registers_v2: Some(&[]),
    };
    let mut client = cal_report_accepted_client();
    assert_eq!(
        client.begin_wlan_config(7, config, qcom_wlfw_client::MAX_DATAGRAM_BYTES),
        Err(Error::FirmwareNotReady)
    );
    client
        .accept_startup_indication(
            7,
            &startup_indication(wlfw_protocol::WLFW_FW_READY_IND_MESSAGE_ID, false, 0),
        )
        .unwrap();
    assert_eq!(
        client.begin_wlan_config(7, config, 64),
        Err(Error::OutputTooSmall)
    );
    assert_eq!(client.state(), State::CalReportAccepted);

    let request = client
        .begin_wlan_config(7, config, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    assert_eq!(request.message_id, wlfw_protocol::WLFW_WLAN_CFG_MESSAGE_ID);
    assert_eq!(client.state(), State::WlanConfigPending);
    assert_eq!(
        client.begin_wlan_mode(7, qcom_wlfw_client::MAX_DATAGRAM_BYTES),
        Err(Error::Busy)
    );
    assert_eq!(
        client.accept_wlan_config_response(
            7,
            &status_response(
                request.transaction_id + 1,
                wlfw_protocol::WLFW_WLAN_CFG_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        ),
        Err(Error::Protocol(
            wlfw_protocol::ProtocolError::WrongTransactionId
        ))
    );
    let config_result = client
        .accept_wlan_config_response(
            7,
            &status_response(
                request.transaction_id,
                wlfw_protocol::WLFW_WLAN_CFG_MESSAGE_ID,
                0,
                0,
                true,
                &[],
            ),
        )
        .unwrap();
    assert_eq!(config_result.result, 0);
    assert_eq!(client.state(), State::WlanModeReady);

    let mode = client
        .begin_wlan_mode(7, qcom_wlfw_client::MAX_DATAGRAM_BYTES)
        .unwrap();
    assert_eq!(mode.message_id, wlfw_protocol::WLFW_WLAN_MODE_MESSAGE_ID);
    assert_eq!(mode.len, 52);
    assert_eq!(
        &mode.bytes[32..50],
        &[
            0,
            mode.transaction_id as u8,
            (mode.transaction_id >> 8) as u8,
            0x22,
            0,
            11,
            0,
            0x01,
            4,
            0,
            0,
            0,
            0,
            0,
            0x10,
            1,
            0,
            0,
        ]
    );
    let mode_result = client
        .accept_wlan_mode_response(
            7,
            &status_response(
                mode.transaction_id,
                wlfw_protocol::WLFW_WLAN_MODE_MESSAGE_ID,
                0,
                0,
                false,
                &[],
            ),
        )
        .unwrap();
    assert_eq!(mode_result.result, 0);
    assert_eq!(client.state(), State::WlanModeAccepted);
}

#[test]
fn unexpected_but_in_pool_msa_layout_fails_without_creating_handoff_plan() {
    let mut client = registered_client();
    let request = client.begin_msa_info(7, msa_reservation(7), 60).unwrap();
    assert_eq!(
        client.accept_msa_info_response(
            7,
            &msa_info_response(
                request.transaction_id,
                false,
                &[(SUNFISH_MSA_BASE, 0x200000, 0)],
            ),
        ),
        Err(Error::MsaPolicy(crate::qcom_msa::Error::WrongRegionProfile))
    );
    assert_eq!(client.state(), State::Failed);
    assert_eq!(client.pending_message_id(), None);
    assert!(client.msa_info_outcome().unwrap().handoff_plan.is_none());
}

#[test]
fn rejects_out_of_pool_or_overlapping_msa_regions_without_consuming_pending() {
    for regions in [
        vec![(SUNFISH_MSA_BASE - 0x1000, 0x2000, 1)],
        vec![
            (SUNFISH_MSA_BASE, 0x4000, 1),
            (SUNFISH_MSA_BASE + 0x3000, 0x1000, 0),
        ],
        vec![(SUNFISH_MSA_BASE, u32::MAX, 0)],
        vec![(SUNFISH_MSA_BASE, 0, 0)],
    ] {
        let mut client = registered_client();
        let request = client.begin_msa_info(7, msa_reservation(7), 60).unwrap();
        assert_eq!(
            client.accept_msa_info_response(
                7,
                &msa_info_response(request.transaction_id, false, &regions),
            ),
            Err(Error::Protocol(
                wlfw_protocol::ProtocolError::InvalidMsaRegions
            ))
        );
        assert_eq!(client.state(), State::MsaInfoPending);
        assert_eq!(
            client.pending_message_id(),
            Some(wlfw_protocol::WLFW_MSA_INFO_MESSAGE_ID)
        );
    }
}

#[test]
fn rejects_wrong_endpoint_and_transaction_without_consuming_pending_request() {
    let mut client = ready_client();
    let request = client.begin_indication_registration(7, 64).unwrap();
    let mut wrong_endpoint = response(request.transaction_id, 0, 0, None);
    wrong_endpoint[12..16].copy_from_slice(&701u32.to_le_bytes());
    assert_eq!(
        client.accept_indication_registration_response(7, &wrong_endpoint),
        Err(Error::WrongEndpoint)
    );
    assert_eq!(client.state(), State::RegistrationPending);

    assert_eq!(
        client.accept_indication_registration_response(
            7,
            &response(request.transaction_id + 1, 0, 0, None),
        ),
        Err(Error::Protocol(
            wlfw_protocol::ProtocolError::WrongTransactionId
        ))
    );
    assert_eq!(client.state(), State::RegistrationPending);
    assert!(matches!(
        client.accept_indication_registration_response(
            7,
            &response(request.transaction_id, 0, 0, None)
        ),
        Ok(RegistrationOutcome::Accepted { .. })
    ));
}

#[test]
fn remote_rejection_is_terminal_until_server_departure() {
    let mut client = ready_client();
    let request = client.begin_indication_registration(7, 64).unwrap();
    assert_eq!(
        client.accept_indication_registration_response(
            7,
            &response(request.transaction_id, 1, 0x21, None),
        ),
        Ok(RegistrationOutcome::Rejected {
            result: 1,
            error: 0x21,
            confirm_rx: false,
        })
    );
    assert_eq!(client.state(), State::Failed);
    assert_eq!(
        client.begin_indication_registration(7, 64),
        Err(Error::Failed)
    );
    assert_eq!(client.remove_server(7, server(SERVER)), Ok(true));
    assert_eq!(client.state(), State::Searching);
}

#[test]
fn validates_service_identity_generation_and_departure() {
    let mut client = Client::new(LOCAL, 7).unwrap();
    let mut wrong = server(SERVER);
    wrong.service += 1;
    assert_eq!(client.observe_server(7, wrong), Err(Error::InvalidServer));
    wrong = server(SERVER);
    wrong.instance += 1;
    assert_eq!(client.observe_server(7, wrong), Err(Error::InvalidServer));
    assert_eq!(
        client.observe_server(6, server(SERVER)),
        Err(Error::StaleGeneration)
    );
    assert_eq!(client.observe_server(7, server(SERVER)), Ok(true));
    assert_eq!(
        client.observe_server(7, server(Address { node: 5, port: 701 })),
        Err(Error::EndpointConflict)
    );
    assert_eq!(
        client.remove_server(7, server(Address { node: 5, port: 701 })),
        Ok(false)
    );
    assert_eq!(client.remove_server(7, server(SERVER)), Ok(true));
    assert_eq!(client.endpoint(), None);

    client.begin_generation(8).unwrap();
    assert_eq!(
        client.observe_server(7, server(SERVER)),
        Err(Error::StaleGeneration)
    );
    assert_eq!(client.observe_server(8, server(SERVER)), Ok(true));
}

#[test]
fn malformed_qrtr_padding_does_not_consume_request() {
    let mut client = ready_client();
    let request = client.begin_indication_registration(7, 64).unwrap();
    let mut malformed = response(request.transaction_id, 0, 0, None);
    *malformed.last_mut().unwrap() = 1;
    assert_eq!(
        client.accept_indication_registration_response(7, &malformed),
        Err(Error::InvalidQrtr)
    );
    assert_eq!(client.state(), State::RegistrationPending);
}

#[test]
fn qrtr_confirm_rx_is_returned_to_the_transport_for_control_ack() {
    let mut client = ready_client();
    let request = client.begin_indication_registration(7, 64).unwrap();
    let mut confirm_required = response(request.transaction_id, 0, 0, None);
    confirm_required[16..20].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        client.accept_indication_registration_response(7, &confirm_required),
        Ok(RegistrationOutcome::Accepted {
            firmware_status: None,
            confirm_rx: true,
        })
    );
    assert_eq!(client.state(), State::MsaInfoReady);

    let mut client = ready_client();
    let request = client.begin_indication_registration(7, 64).unwrap();
    let mut invalid_confirm = response(request.transaction_id, 0, 0, None);
    invalid_confirm[16..20].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        client.accept_indication_registration_response(7, &invalid_confirm),
        Err(Error::InvalidQrtr)
    );
    assert_eq!(client.state(), State::RegistrationPending);
    assert!(matches!(
        client.accept_indication_registration_response(
            7,
            &response(request.transaction_id, 0, 0, None)
        ),
        Ok(RegistrationOutcome::Accepted { .. })
    ));
}
