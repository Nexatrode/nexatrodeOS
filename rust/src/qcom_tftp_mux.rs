//! Offline shared-port dispatch for the bounded server-check and WLAN servers.
//! Transport ownership, close fencing and retained replies remain the caller's responsibility.
#![forbid(unsafe_code)]

use crate::{qcom_qrtr_tftp as firmware, qcom_tftp, qcom_tftp_server_check as check};

pub const FIRMWARE_FIRST_PORT: u32 = 0x5100;
pub const FIRMWARE_LAST_PORT: u32 = 0x51ff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MuxDispatch {
    ServerCheck(Result<check::Reply, check::Error>),
    Firmware(firmware::Dispatch),
}

pub fn dispatch<S: qcom_tftp::ReadSource + ?Sized>(
    server_check: &mut check::ServerCheck,
    firmware_server: &mut firmware::Server<'_, S>,
    route: check::Route,
    packet: &[u8],
    output: &mut [u8],
) -> MuxDispatch {
    let is_firmware = (route.destination.port == check::SERVICE_PORT
        && qcom_tftp::decode_read_request(packet).is_ok())
        || (FIRMWARE_FIRST_PORT..=FIRMWARE_LAST_PORT).contains(&route.destination.port);
    if is_firmware {
        MuxDispatch::Firmware(firmware_server.receive(firmware::IncomingRoute {
            destination_node: route.destination.node,
            destination_port: route.destination.port,
            source: firmware::Peer { node: route.source.node, port: route.source.port },
        }, packet, output))
    } else {
        MuxDispatch::ServerCheck(server_check.receive(route, packet, output))
    }
}
