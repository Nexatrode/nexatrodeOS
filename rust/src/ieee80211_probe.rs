//! Bounded IEEE 802.11 Probe Request frame construction.
//!
//! This produces a management frame suitable for a Native-WiFi TX queue. It
//! does not schedule a channel, assign TX rates, encrypt, or access hardware.
#![forbid(unsafe_code)]

const IEEE80211_HEADER_BYTES: usize = 24;
const IEEE80211_PROBE_REQUEST_FC: u16 = 0x0040;
const IEEE80211_BROADCAST: [u8; 6] = [0xff; 6];
const SSID_ELEMENT_ID: u8 = 0;
const SUPPORTED_RATES_ELEMENT_ID: u8 = 1;
const EXTENDED_SUPPORTED_RATES_ELEMENT_ID: u8 = 50;
const MAX_SSID_BYTES: usize = 32;
const MAX_SUPPORTED_RATES_BYTES: usize = 8;
const MAX_INFORMATION_ELEMENT_BYTES: usize = u8::MAX as usize;
const MAX_SEQUENCE_NUMBER: u16 = 0x0fff;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeRequestError {
    InvalidTransmitterAddress,
    SsidTooLong,
    InvalidSupportedRatesLength,
    ExtendedSupportedRatesTooLong,
    SequenceNumberOutOfRange,
    LengthOverflow,
    OutputTooSmall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeRequestLayout {
    pub frame_bytes: usize,
    pub information_element_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProbeRequest<'a> {
    pub transmitter: [u8; 6],
    /// Empty creates a wildcard SSID IE; otherwise the bytes are sent verbatim.
    pub ssid: &'a [u8],
    /// Supported Rates IE, required to contain one through eight rate bytes.
    pub supported_rates: &'a [u8],
    /// Optional Extended Supported Rates IE. An empty slice omits the IE.
    pub extended_supported_rates: &'a [u8],
    /// 12-bit 802.11 sequence number; fragment number is always zero.
    pub sequence_number: u16,
}

fn write_ie(output: &mut [u8], offset: usize, id: u8, value: &[u8]) -> usize {
    output[offset] = id;
    output[offset + 1] = value.len() as u8;
    output[offset + 2..offset + 2 + value.len()].copy_from_slice(value);
    offset + 2 + value.len()
}

/// Encode one Probe Request without modifying `output` on failure.
pub fn encode_probe_request(
    request: ProbeRequest<'_>,
    output: &mut [u8],
) -> Result<ProbeRequestLayout, ProbeRequestError> {
    if request.transmitter.iter().all(|byte| *byte == 0) || request.transmitter[0] & 1 != 0 {
        return Err(ProbeRequestError::InvalidTransmitterAddress);
    }
    if request.ssid.len() > MAX_SSID_BYTES {
        return Err(ProbeRequestError::SsidTooLong);
    }
    if request.supported_rates.is_empty()
        || request.supported_rates.len() > MAX_SUPPORTED_RATES_BYTES
    {
        return Err(ProbeRequestError::InvalidSupportedRatesLength);
    }
    if request.extended_supported_rates.len() > MAX_INFORMATION_ELEMENT_BYTES {
        return Err(ProbeRequestError::ExtendedSupportedRatesTooLong);
    }
    if request.sequence_number > MAX_SEQUENCE_NUMBER {
        return Err(ProbeRequestError::SequenceNumberOutOfRange);
    }

    let mut information_element_bytes = 2usize
        .checked_add(request.ssid.len())
        .and_then(|length| length.checked_add(2 + request.supported_rates.len()))
        .ok_or(ProbeRequestError::LengthOverflow)?;
    if !request.extended_supported_rates.is_empty() {
        information_element_bytes = information_element_bytes
            .checked_add(2 + request.extended_supported_rates.len())
            .ok_or(ProbeRequestError::LengthOverflow)?;
    }
    let frame_bytes = IEEE80211_HEADER_BYTES
        .checked_add(information_element_bytes)
        .ok_or(ProbeRequestError::LengthOverflow)?;
    if output.len() < frame_bytes {
        return Err(ProbeRequestError::OutputTooSmall);
    }

    output[..2].copy_from_slice(&IEEE80211_PROBE_REQUEST_FC.to_le_bytes());
    output[2..4].fill(0); // Duration.
    output[4..10].copy_from_slice(&IEEE80211_BROADCAST);
    output[10..16].copy_from_slice(&request.transmitter);
    output[16..22].copy_from_slice(&IEEE80211_BROADCAST);
    let sequence_control = request.sequence_number << 4;
    output[22..24].copy_from_slice(&sequence_control.to_le_bytes());

    let mut offset = IEEE80211_HEADER_BYTES;
    offset = write_ie(output, offset, SSID_ELEMENT_ID, request.ssid);
    offset = write_ie(
        output,
        offset,
        SUPPORTED_RATES_ELEMENT_ID,
        request.supported_rates,
    );
    if !request.extended_supported_rates.is_empty() {
        offset = write_ie(
            output,
            offset,
            EXTENDED_SUPPORTED_RATES_ELEMENT_ID,
            request.extended_supported_rates,
        );
    }

    debug_assert_eq!(offset, frame_bytes);
    Ok(ProbeRequestLayout {
        frame_bytes,
        information_element_bytes,
    })
}
