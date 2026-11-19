//! Bounded WCN3990 Native-WiFi RX decapsulation reconstruction.
//!
//! ath10k's Linux path rebuilds the original 802.11 header from HTT
//! `rx_hdr_status`, restoring the DA/SA addresses from the native-decapped
//! three/four-address header. This module implements that byte transformation
//! for one already-bounded MSDU. It does not parse the HTT ring, provide RX
//! status/crypto policy, perform DMA, or implement mac80211/supplicant policy.
#![forbid(unsafe_code)]

pub const RX_HEADER_STATUS_BYTES: usize = 64;
pub const WCN3990_CRYPTO_PARAM_ALIGNMENT: usize = 1;

const FC_VERSION_MASK: u16 = 0x0003;
const FC_TYPE_MASK: u16 = 0x000c;
const FC_TYPE_DATA: u16 = 0x0008;
const FC_SUBTYPE_QOS_BIT: u16 = 0x0080;
const FC_TO_DS: u16 = 0x0100;
const FC_FROM_DS: u16 = 0x0200;
const FC_ORDER: u16 = 0x8000;

const IEEE80211_ADDR1_OFFSET: usize = 4;
const IEEE80211_ADDR2_OFFSET: usize = 10;
const IEEE80211_ADDR3_OFFSET: usize = 16;
const IEEE80211_ADDR4_OFFSET: usize = 24;
const IEEE80211_BASE_HEADER_BYTES: usize = 24;
const IEEE80211_ADDR_BYTES: usize = 6;

const FC_TYPE_MANAGEMENT: u16 = 0x0000;
const FC_TYPE_CONTROL: u16 = 0x0004;
const FC_TYPE_EXTENSION: u16 = 0x000c;
const FC_CONTROL_SHORT_HEADER_MASK: u16 = 0x00e0;
const FC_CONTROL_SHORT_HEADER_VALUE: u16 = 0x00c0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FourAddressPadding {
    /// Linux rounds the decapped 802.11 header length up to four bytes.
    PaddedToFour,
    /// Firmware advertises NO_NWIFI_DECAP_4ADDR_PADDING.
    Unpadded,
}

/// Crypto-parameter prefix sizes supported by the pinned ath10k WCN3990 path:
/// no IV, WEP IV, or TKIP/CCMP/GCMP IV.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoParameters {
    None,
    WepIv,
    TkipOrAesIv,
}

impl CryptoParameters {
    const fn byte_len(self) -> usize {
        match self {
            Self::None => 0,
            Self::WepIv => 4,
            Self::TkipOrAesIv => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    HeaderStatusTooShort,
    InvalidFrameControl,
    NonDataFrame,
    UnexpectedQosInDecappedHeader,
    HeaderTooShort,
    UnsupportedAddressMapping,
    CryptoParametersOutOfBounds,
    DecappedHeaderOutOfBounds,
    OutputTooSmall,
}

/// IEEE 802.11 frame class, as encoded in Frame Control.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameKind {
    Management,
    Control,
    Data,
    Extension,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameHeaderError {
    FrameControlTooShort,
    UnsupportedVersion,
    HeaderTruncated,
}

/// A borrowed view of a complete 802.11 header and its remaining frame body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameView<'a> {
    pub frame_control: u16,
    pub kind: FrameKind,
    pub subtype: u8,
    pub header: &'a [u8],
    pub body: &'a [u8],
}

/// Inspect a bounded 802.11 frame without interpreting or mutating its body.
/// Header sizes follow the pinned Linux `ieee80211_hdrlen()` implementation.
pub fn inspect_frame(frame: &[u8]) -> Result<FrameView<'_>, FrameHeaderError> {
    let fc_bytes = frame
        .get(..2)
        .ok_or(FrameHeaderError::FrameControlTooShort)?;
    let fc = u16::from_le_bytes([fc_bytes[0], fc_bytes[1]]);
    if fc & FC_VERSION_MASK != 0 {
        return Err(FrameHeaderError::UnsupportedVersion);
    }

    let frame_type = fc & FC_TYPE_MASK;
    let (kind, header_bytes) = match frame_type {
        FC_TYPE_MANAGEMENT => (
            FrameKind::Management,
            IEEE80211_BASE_HEADER_BYTES + if fc & FC_ORDER != 0 { 4 } else { 0 },
        ),
        FC_TYPE_CONTROL => (
            FrameKind::Control,
            if fc & FC_CONTROL_SHORT_HEADER_MASK == FC_CONTROL_SHORT_HEADER_VALUE {
                10
            } else {
                16
            },
        ),
        FC_TYPE_DATA => (
            FrameKind::Data,
            data_header_len(fc).map_err(|_| FrameHeaderError::UnsupportedVersion)?,
        ),
        FC_TYPE_EXTENSION => (FrameKind::Extension, 4),
        _ => return Err(FrameHeaderError::UnsupportedVersion),
    };
    let header = frame
        .get(..header_bytes)
        .ok_or(FrameHeaderError::HeaderTruncated)?;
    Ok(FrameView {
        frame_control: fc,
        kind,
        subtype: ((fc & 0x00f0) >> 4) as u8,
        header,
        body: &frame[header_bytes..],
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InformationElementError {
    TruncatedHeader,
    TruncatedValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InformationElement<'a> {
    pub id: u8,
    pub value: &'a [u8],
    /// Complete ID/length/value bytes for code that must retain an IE verbatim.
    pub encoded: &'a [u8],
}

/// Bounded iterator for the ID/length/value sequence used by 802.11 IEs.
#[derive(Clone, Copy, Debug)]
pub struct InformationElementIter<'a> {
    remaining: &'a [u8],
    failed: bool,
}

impl<'a> InformationElementIter<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            remaining: bytes,
            failed: false,
        }
    }
}

impl<'a> Iterator for InformationElementIter<'a> {
    type Item = Result<InformationElement<'a>, InformationElementError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.remaining.is_empty() {
            return None;
        }
        if self.remaining.len() < 2 {
            self.failed = true;
            return Some(Err(InformationElementError::TruncatedHeader));
        }

        let id = self.remaining[0];
        let value_bytes = usize::from(self.remaining[1]);
        let end = 2 + value_bytes;
        if self.remaining.len() < end {
            self.failed = true;
            return Some(Err(InformationElementError::TruncatedValue));
        }

        let encoded = &self.remaining[..end];
        let value = &self.remaining[2..end];
        self.remaining = &self.remaining[end..];
        Some(Ok(InformationElement { id, value, encoded }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BeaconParseError {
    FrameHeader(FrameHeaderError),
    NotBeaconOrProbeResponse,
    FixedFieldsTruncated,
    MalformedInformationElements(InformationElementError),
}

/// Fixed beacon/probe-response fields plus a borrowed, lazy IE iterator.
#[derive(Clone, Debug)]
pub struct BeaconProbeResponse<'a> {
    pub timestamp: u64,
    pub beacon_interval: u16,
    pub capabilities: u16,
    pub information_elements: InformationElementIter<'a>,
}

const BEACON_PROBE_RESPONSE_FIXED_BYTES: usize = 12;
const IEEE80211_STYPE_PROBE_RESPONSE: u8 = 5;
const IEEE80211_STYPE_BEACON: u8 = 8;
const WLAN_EID_SSID: u8 = 0;
const WLAN_EID_SUPPORTED_RATES: u8 = 1;
const WLAN_EID_DS_PARAMETERS: u8 = 3;
const WLAN_EID_RSN: u8 = 48;
const WLAN_EID_EXTENDED_RATES: u8 = 50;
const WLAN_EID_HT_OPERATION: u8 = 61;
const MAX_SSID_BYTES: usize = 32;
const MAX_SUPPORTED_RATES_BYTES: usize = 8;
const HT_OPERATION_BYTES: usize = 22;

/// Parse the fixed fields and fully validate the IE stream from a beacon or
/// probe response. The frame must exclude any RX FCS. SSID/security policy
/// remains with the caller; IE values are never copied.
pub fn parse_beacon_probe_response(
    frame: &[u8],
) -> Result<BeaconProbeResponse<'_>, BeaconParseError> {
    let view = inspect_frame(frame).map_err(BeaconParseError::FrameHeader)?;
    if view.kind != FrameKind::Management
        || !matches!(
            view.subtype,
            IEEE80211_STYPE_PROBE_RESPONSE | IEEE80211_STYPE_BEACON
        )
    {
        return Err(BeaconParseError::NotBeaconOrProbeResponse);
    }
    if view.body.len() < BEACON_PROBE_RESPONSE_FIXED_BYTES {
        return Err(BeaconParseError::FixedFieldsTruncated);
    }

    let timestamp = u64::from_le_bytes([
        view.body[0],
        view.body[1],
        view.body[2],
        view.body[3],
        view.body[4],
        view.body[5],
        view.body[6],
        view.body[7],
    ]);
    let beacon_interval = u16::from_le_bytes([view.body[8], view.body[9]]);
    let capabilities = u16::from_le_bytes([view.body[10], view.body[11]]);
    let ie_bytes = &view.body[BEACON_PROBE_RESPONSE_FIXED_BYTES..];
    let mut validation_iter = InformationElementIter::new(ie_bytes);
    while let Some(element) = validation_iter.next() {
        element.map_err(BeaconParseError::MalformedInformationElements)?;
    }
    Ok(BeaconProbeResponse {
        timestamp,
        beacon_interval,
        capabilities,
        information_elements: InformationElementIter::new(ie_bytes),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BssAdvertisementError {
    Beacon(BeaconParseError),
    FrameHeader(FrameHeaderError),
    InvalidBssid,
    DuplicateElement(u8),
    InvalidElementLength(u8),
    InvalidRate,
    InvalidChannel,
    ConflictingPrimaryChannels,
}

/// Borrowed scan identity and common link parameters from a beacon or probe
/// response. RSN bytes are retained verbatim; this type makes no security or
/// association decision, and channel remains optional when neither DS nor HT
/// operation information is advertised.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BssAdvertisement<'a> {
    pub bssid: [u8; 6],
    /// Fixed beacon/probe-response capability information.
    pub capabilities: u16,
    /// `Some(&[])` represents a hidden SSID; `None` means no SSID element.
    pub ssid: Option<&'a [u8]>,
    pub supported_rates: Option<&'a [u8]>,
    pub extended_rates: Option<&'a [u8]>,
    pub primary_channel: Option<u8>,
    pub rsn: Option<&'a [u8]>,
    /// Complete RSN IE, including the element ID and length bytes.
    pub rsn_ie: Option<&'a [u8]>,
}

/// Extract the bounded BSS fields needed to hand a scan observation to link
/// control. Unknown IEs are ignored, while malformed or duplicate recognized
/// IEs fail closed. Security data is borrowed and left for a supplicant.
pub fn parse_bss_advertisement(
    frame: &[u8],
) -> Result<BssAdvertisement<'_>, BssAdvertisementError> {
    let beacon = parse_beacon_probe_response(frame).map_err(BssAdvertisementError::Beacon)?;
    let view = inspect_frame(frame).map_err(BssAdvertisementError::FrameHeader)?;
    let bssid_bytes = view
        .header
        .get(IEEE80211_ADDR3_OFFSET..IEEE80211_ADDR3_OFFSET + IEEE80211_ADDR_BYTES)
        .ok_or(BssAdvertisementError::FrameHeader(
            FrameHeaderError::HeaderTruncated,
        ))?;
    let mut bssid = [0; IEEE80211_ADDR_BYTES];
    bssid.copy_from_slice(bssid_bytes);
    if bssid.iter().all(|byte| *byte == 0) || bssid[0] & 1 != 0 {
        return Err(BssAdvertisementError::InvalidBssid);
    }

    let mut ssid = None;
    let mut supported_rates = None;
    let mut extended_rates = None;
    let mut ds_channel = None;
    let mut ht_channel = None;
    let mut rsn = None;
    let mut rsn_ie = None;

    for item in beacon.information_elements {
        let element = item.map_err(|error| {
            BssAdvertisementError::Beacon(BeaconParseError::MalformedInformationElements(error))
        })?;
        match element.id {
            WLAN_EID_SSID => {
                if ssid.is_some() {
                    return Err(BssAdvertisementError::DuplicateElement(element.id));
                }
                if element.value.len() > MAX_SSID_BYTES {
                    return Err(BssAdvertisementError::InvalidElementLength(element.id));
                }
                ssid = Some(element.value);
            }
            WLAN_EID_SUPPORTED_RATES => {
                if supported_rates.is_some() {
                    return Err(BssAdvertisementError::DuplicateElement(element.id));
                }
                if element.value.is_empty() || element.value.len() > MAX_SUPPORTED_RATES_BYTES {
                    return Err(BssAdvertisementError::InvalidElementLength(element.id));
                }
                validate_rates(element.value)?;
                supported_rates = Some(element.value);
            }
            WLAN_EID_EXTENDED_RATES => {
                if extended_rates.is_some() {
                    return Err(BssAdvertisementError::DuplicateElement(element.id));
                }
                if element.value.is_empty() {
                    return Err(BssAdvertisementError::InvalidElementLength(element.id));
                }
                validate_rates(element.value)?;
                extended_rates = Some(element.value);
            }
            WLAN_EID_DS_PARAMETERS => {
                if ds_channel.is_some() {
                    return Err(BssAdvertisementError::DuplicateElement(element.id));
                }
                if element.value.len() != 1 {
                    return Err(BssAdvertisementError::InvalidElementLength(element.id));
                }
                if element.value[0] == 0 {
                    return Err(BssAdvertisementError::InvalidChannel);
                }
                ds_channel = Some(element.value[0]);
            }
            WLAN_EID_HT_OPERATION => {
                if ht_channel.is_some() {
                    return Err(BssAdvertisementError::DuplicateElement(element.id));
                }
                if element.value.len() != HT_OPERATION_BYTES {
                    return Err(BssAdvertisementError::InvalidElementLength(element.id));
                }
                if element.value[0] == 0 {
                    return Err(BssAdvertisementError::InvalidChannel);
                }
                ht_channel = Some(element.value[0]);
            }
            WLAN_EID_RSN => {
                if rsn.is_some() {
                    return Err(BssAdvertisementError::DuplicateElement(element.id));
                }
                if element.value.len() < 2 {
                    return Err(BssAdvertisementError::InvalidElementLength(element.id));
                }
                rsn = Some(element.value);
                rsn_ie = Some(element.encoded);
            }
            _ => {}
        }
    }

    let primary_channel = match (ds_channel, ht_channel) {
        (Some(ds), Some(ht)) if ds != ht => {
            return Err(BssAdvertisementError::ConflictingPrimaryChannels);
        }
        (Some(ds), _) => Some(ds),
        (_, Some(ht)) => Some(ht),
        (None, None) => None,
    };

    Ok(BssAdvertisement {
        bssid,
        capabilities: beacon.capabilities,
        ssid,
        supported_rates,
        extended_rates,
        primary_channel,
        rsn,
        rsn_ie,
    })
}

fn validate_rates(rates: &[u8]) -> Result<(), BssAdvertisementError> {
    if rates.iter().any(|rate| rate & 0x7f == 0) {
        return Err(BssAdvertisementError::InvalidRate);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RebuiltFrame {
    pub frame_bytes: usize,
    pub header_bytes: usize,
    pub crypto_parameter_bytes: usize,
    pub body_bytes: usize,
}

fn frame_control(header: &[u8]) -> Result<u16, Error> {
    let bytes = header.get(..2).ok_or(Error::HeaderTooShort)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Return the header length ath10k/mac80211 uses for an ordinary data frame.
/// Control and management frames are handled on separate Linux paths.
fn data_header_len(fc: u16) -> Result<usize, Error> {
    if fc & FC_VERSION_MASK != 0 {
        return Err(Error::InvalidFrameControl);
    }
    if fc & FC_TYPE_MASK != FC_TYPE_DATA {
        return Err(Error::NonDataFrame);
    }

    let four_address = fc & (FC_TO_DS | FC_FROM_DS) == (FC_TO_DS | FC_FROM_DS);
    let qos = fc & FC_SUBTYPE_QOS_BIT != 0;
    let ht_control = qos && fc & FC_ORDER != 0;
    Ok(IEEE80211_BASE_HEADER_BYTES
        + if four_address {
            IEEE80211_ADDR_BYTES
        } else {
            0
        }
        + if qos { 2 } else { 0 }
        + if ht_control { 4 } else { 0 })
}

fn round_up_four(value: usize) -> Option<usize> {
    value.checked_add(3).map(|aligned| aligned & !3)
}

fn addresses(header: &[u8], fc: u16) -> Result<([u8; 6], [u8; 6]), Error> {
    let addr1 = header
        .get(IEEE80211_ADDR1_OFFSET..IEEE80211_ADDR1_OFFSET + IEEE80211_ADDR_BYTES)
        .ok_or(Error::HeaderTooShort)?;
    let addr2 = header
        .get(IEEE80211_ADDR2_OFFSET..IEEE80211_ADDR2_OFFSET + IEEE80211_ADDR_BYTES)
        .ok_or(Error::HeaderTooShort)?;
    let addr3 = header
        .get(IEEE80211_ADDR3_OFFSET..IEEE80211_ADDR3_OFFSET + IEEE80211_ADDR_BYTES)
        .ok_or(Error::HeaderTooShort)?;
    let to_ds = fc & FC_TO_DS != 0;
    let from_ds = fc & FC_FROM_DS != 0;

    let (da, sa) = match (to_ds, from_ds) {
        (true, true) => {
            let addr4 = header
                .get(IEEE80211_ADDR4_OFFSET..IEEE80211_ADDR4_OFFSET + IEEE80211_ADDR_BYTES)
                .ok_or(Error::HeaderTooShort)?;
            (addr3, addr4)
        }
        (true, false) => (addr3, addr2),
        (false, true) => (addr1, addr3),
        (false, false) => (addr1, addr2),
    };

    let mut da_value = [0; IEEE80211_ADDR_BYTES];
    let mut sa_value = [0; IEEE80211_ADDR_BYTES];
    da_value.copy_from_slice(da);
    sa_value.copy_from_slice(sa);
    Ok((da_value, sa_value))
}

fn write_addresses(header: &mut [u8], fc: u16, da: &[u8; 6], sa: &[u8; 6]) {
    let to_ds = fc & FC_TO_DS != 0;
    let from_ds = fc & FC_FROM_DS != 0;
    let (da_offset, sa_offset) = match (to_ds, from_ds) {
        (true, true) => (IEEE80211_ADDR3_OFFSET, IEEE80211_ADDR4_OFFSET),
        (true, false) => (IEEE80211_ADDR3_OFFSET, IEEE80211_ADDR2_OFFSET),
        (false, true) => (IEEE80211_ADDR1_OFFSET, IEEE80211_ADDR3_OFFSET),
        (false, false) => (IEEE80211_ADDR1_OFFSET, IEEE80211_ADDR2_OFFSET),
    };
    header[da_offset..da_offset + IEEE80211_ADDR_BYTES].copy_from_slice(da);
    header[sa_offset..sa_offset + IEEE80211_ADDR_BYTES].copy_from_slice(sa);
}

/// Rebuild one Native-WiFi MSDU into the original 802.11 data-frame shape.
///
/// `first_header_status` is the WCN3990 HTT 64-byte `rx_hdr_status` snapshot.
/// `decapped_msdu` is the exact, length-bounded payload from the RX
/// descriptor: padded native 802.11 header followed by RFC1042/LLC and data.
/// WCN3990 reports no L3 padding and uses one-byte crypto-parameter alignment
/// in the pinned ath10k profile. The caller supplies whether firmware enabled
/// the no-four-address-padding feature and whether hardware stripped the IV.
///
/// On error, `output` is unchanged. On success, the returned length bounds
/// the valid prefix; bytes after it are left untouched.
pub fn rebuild_data_msdu(
    four_address_padding: FourAddressPadding,
    first_header_status: &[u8],
    decapped_msdu: &[u8],
    iv_stripped: bool,
    crypto_parameters: CryptoParameters,
    output: &mut [u8],
) -> Result<RebuiltFrame, Error> {
    if first_header_status.len() < RX_HEADER_STATUS_BYTES {
        return Err(Error::HeaderStatusTooShort);
    }

    let original_fc = frame_control(first_header_status)?;
    let original_header_bytes = data_header_len(original_fc)?;
    if original_header_bytes > RX_HEADER_STATUS_BYTES {
        return Err(Error::HeaderTooShort);
    }

    let decapped_fc = frame_control(decapped_msdu)?;
    let decapped_header_bytes = data_header_len(decapped_fc)?;
    if decapped_fc & FC_SUBTYPE_QOS_BIT != 0 {
        // ath10k's Native-WiFi path documents that firmware reports QoS frames
        // as non-QoS and omits the QoS Control field from this header.
        return Err(Error::UnexpectedQosInDecappedHeader);
    }
    let storage_header_bytes = match four_address_padding {
        FourAddressPadding::PaddedToFour => {
            round_up_four(decapped_header_bytes).ok_or(Error::DecappedHeaderOutOfBounds)?
        }
        FourAddressPadding::Unpadded => decapped_header_bytes,
    };
    if storage_header_bytes > decapped_msdu.len() {
        return Err(Error::DecappedHeaderOutOfBounds);
    }

    let (da, sa) = addresses(decapped_msdu, decapped_fc).map_err(|error| match error {
        Error::HeaderTooShort => Error::DecappedHeaderOutOfBounds,
        other => other,
    })?;

    let inserted_crypto_bytes = if iv_stripped {
        0
    } else {
        crypto_parameters.byte_len()
    };
    let crypto_offset = original_header_bytes
        .checked_add(WCN3990_CRYPTO_PARAM_ALIGNMENT - 1)
        .map(|offset| offset & !(WCN3990_CRYPTO_PARAM_ALIGNMENT - 1))
        .ok_or(Error::CryptoParametersOutOfBounds)?;
    let crypto_end = crypto_offset
        .checked_add(inserted_crypto_bytes)
        .ok_or(Error::CryptoParametersOutOfBounds)?;
    if crypto_end > RX_HEADER_STATUS_BYTES {
        return Err(Error::CryptoParametersOutOfBounds);
    }

    let body = decapped_msdu
        .get(storage_header_bytes..)
        .ok_or(Error::DecappedHeaderOutOfBounds)?;
    let output_len = original_header_bytes
        .checked_add(inserted_crypto_bytes)
        .and_then(|len| len.checked_add(body.len()))
        .ok_or(Error::OutputTooSmall)?;
    if output.len() < output_len {
        return Err(Error::OutputTooSmall);
    }

    output[..original_header_bytes].copy_from_slice(&first_header_status[..original_header_bytes]);
    if inserted_crypto_bytes != 0 {
        output[original_header_bytes..crypto_end]
            .copy_from_slice(&first_header_status[crypto_offset..crypto_end]);
    }
    let body_start = original_header_bytes + inserted_crypto_bytes;
    output[body_start..output_len].copy_from_slice(body);
    write_addresses(&mut output[..original_header_bytes], original_fc, &da, &sa);

    Ok(RebuiltFrame {
        frame_bytes: output_len,
        header_bytes: original_header_bytes,
        crypto_parameter_bytes: inserted_crypto_bytes,
        body_bytes: body.len(),
    })
}
