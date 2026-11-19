//! Borrowed decoder for the pinned ath10k WMI 10.4 management RX event.
#![forbid(unsafe_code)]

pub const WMI_10_4_MGMT_RX_HEADER_BYTES: usize = 28;
pub const WMI_RX_STATUS_ERR_CRC: u32 = 0x01;
pub const WMI_RX_STATUS_ERR_DECRYPT: u32 = 0x08;
pub const WMI_RX_STATUS_ERR_MIC: u32 = 0x10;
pub const WMI_RX_STATUS_ERR_KEY_CACHE_MISS: u32 = 0x20;
pub const WMI_RX_STATUS_EXT_INFO: u32 = 0x40;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    HeaderTooShort,
    FrameTooShort,
    FrameLengthOutOfBounds,
    ExtensionInfoTruncated,
    LengthOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagementRxEvent<'a> {
    /// WMI channel number (not frequency in MHz).
    pub channel_number: u32,
    pub snr: u32,
    /// Four raw per-chain control RSSI bytes from the 10.4 header.
    pub rssi_control: [u8; 4],
    /// Raw WMI rate value; ath10k divides it by 100 to index its rate table.
    pub rate: u32,
    pub phy_mode: u32,
    pub status: u32,
    pub frame: &'a [u8],
    pub rx_mac_timestamp: Option<u64>,
}

/// Parse the 10.4 management RX header and borrow its advertised frame bytes.
/// Trailing transport padding is ignored, matching ath10k's skb trim behavior.
pub fn parse_management_rx_event(input: &[u8]) -> Result<ManagementRxEvent<'_>, Error> {
    if input.len() < WMI_10_4_MGMT_RX_HEADER_BYTES {
        return Err(Error::HeaderTooShort);
    }

    let frame_length =
        usize::try_from(read_u32(input, 20)).map_err(|_| Error::FrameLengthOutOfBounds)?;
    if frame_length < 2 {
        return Err(Error::FrameTooShort);
    }
    let frame_start = WMI_10_4_MGMT_RX_HEADER_BYTES;
    let frame_end = frame_start
        .checked_add(frame_length)
        .ok_or(Error::LengthOverflow)?;
    if frame_end > input.len() {
        return Err(Error::FrameLengthOutOfBounds);
    }

    let status = read_u32(input, 24);
    let rx_mac_timestamp = if status & WMI_RX_STATUS_EXT_INFO != 0 {
        let aligned_length = round_up_four(frame_length)?;
        let extension_start = frame_start
            .checked_add(aligned_length)
            .ok_or(Error::LengthOverflow)?;
        let extension_end = extension_start
            .checked_add(8)
            .ok_or(Error::LengthOverflow)?;
        if extension_end > input.len() {
            return Err(Error::ExtensionInfoTruncated);
        }
        Some(u64::from_le_bytes(
            input[extension_start..extension_end]
                .try_into()
                .map_err(|_| Error::ExtensionInfoTruncated)?,
        ))
    } else {
        None
    };

    Ok(ManagementRxEvent {
        channel_number: read_u32(input, 0),
        snr: read_u32(input, 4),
        rssi_control: [input[8], input[9], input[10], input[11]],
        rate: read_u32(input, 12),
        phy_mode: read_u32(input, 16),
        status,
        frame: &input[frame_start..frame_end],
        rx_mac_timestamp,
    })
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ])
}

fn round_up_four(value: usize) -> Result<usize, Error> {
    value
        .checked_add(3)
        .map(|aligned| aligned & !3)
        .ok_or(Error::LengthOverflow)
}
