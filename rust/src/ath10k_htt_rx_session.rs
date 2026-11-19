//! Bounded composition of ath10k FIFO RX completions, buffer ownership, and
//! the WCN3990 HTT descriptor/chain decoder.
//!
//! This moves completed bytes into a caller-owned packet buffer and returns
//! the exact buffer leases that must be reposted or otherwise retained. It
//! performs no DMA, cache maintenance, ring MMIO, interrupt handling, network
//! delivery, or hardware ownership checks. The platform caller must provide
//! bytes for each lease's buffer ID from the same exclusive pool.
#![forbid(unsafe_code)]

use crate::ath10k_htt_rx::{self as htt_rx, RxDescriptor, UnchainOutcome};
use crate::ath10k_htt_rx_owner::{self as rx_owner, RxBufferLease, RxRingOwner};
use crate::ath10k_nwifi_rx::{self as nwifi_rx, CryptoParameters, FourAddressPadding};

pub const MAX_RX_CHAIN_BUFFERS: usize = u8::MAX as usize + 1;
const WCN3990_RX_HEADER_STATUS_OFFSET: usize = 296;
const WCN3990_RX_HEADER_STATUS_END: usize = 360;
const WCN3990_NATIVE_WIFI_DECAP_FORMAT: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DropReason {
    Descriptor(htt_rx::Error),
    BufferUnavailable(u16),
    ChainCountMismatch { expected: usize, completed: usize },
    Unchain(htt_rx::DropReason),
    WrongDecapsulationFormat { expected: u8, observed: u8 },
    NativeWifiMsduBoundary { first: bool, last: bool },
    NativeWifi(nwifi_rx::Error),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Disposition {
    Delivered { payload_bytes: usize },
    Dropped(DropReason),
}

/// Result for one completed MPDU buffer group. `descriptor` is absent only
/// when its first buffer could not be decoded. The caller must use the
/// `leases` written to the provided output slice to complete the source
/// ordered repost or to retain/drop those buffers under its own policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Completion {
    pub descriptor: Option<RxDescriptor>,
    pub buffer_count: usize,
    pub disposition: Disposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidBufferCount,
    LeaseOutputTooSmall,
    Owner(rx_owner::Error),
    MissingLease,
    OutputTooSmall,
}

/// Retire one FIFO-completed MPDU group, decode its first-buffer descriptor,
/// and copy the single MSDU payload into `payload_output` when the source
/// ath10k policy supports it. For a chained MPDU, `completed_buffer_count`
/// must be the number of buffers already removed from the CE/HTT completion
/// path and `buffers` supplies the corresponding fixed-pool views by buffer
/// ID. The routine retires ownership before parsing so even malformed packets
/// return leases for recovery. On any error after that transition, populated
/// entries in `leases_output[..completed_buffer_count]` remain valid.
pub fn complete_fifo_msdu(
    owner: &mut RxRingOwner,
    completed_buffer_count: usize,
    buffers_by_id: &[&[u8]],
    leases_output: &mut [Option<RxBufferLease>],
    payload_output: &mut [u8],
) -> Result<Completion, Error> {
    if completed_buffer_count == 0 || completed_buffer_count > MAX_RX_CHAIN_BUFFERS {
        return Err(Error::InvalidBufferCount);
    }
    if leases_output.len() < completed_buffer_count {
        return Err(Error::LeaseOutputTooSmall);
    }
    leases_output[..completed_buffer_count].fill(None);
    owner
        .complete_fifo(
            completed_buffer_count,
            &mut leases_output[..completed_buffer_count],
        )
        .map_err(Error::Owner)?;

    let first_lease = leases_output[0].ok_or(Error::MissingLease)?;
    let Some(first_buffer) = buffers_by_id
        .get(usize::from(first_lease.buffer_id()))
        .copied()
    else {
        return Ok(Completion {
            descriptor: None,
            buffer_count: completed_buffer_count,
            disposition: Disposition::Dropped(DropReason::BufferUnavailable(
                first_lease.buffer_id(),
            )),
        });
    };
    let descriptor = match RxDescriptor::decode(first_buffer) {
        Ok(descriptor) => descriptor,
        Err(error) => {
            return Ok(Completion {
                descriptor: None,
                buffer_count: completed_buffer_count,
                disposition: Disposition::Dropped(DropReason::Descriptor(error)),
            });
        }
    };

    let expected_buffer_count = 1 + usize::from(descriptor.more_buffers);
    if expected_buffer_count != completed_buffer_count {
        return Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completed_buffer_count,
            disposition: Disposition::Dropped(DropReason::ChainCountMismatch {
                expected: expected_buffer_count,
                completed: completed_buffer_count,
            }),
        });
    }

    if descriptor.more_buffers == 0 {
        let payload = match descriptor.single_buffer_payload(first_buffer) {
            Ok(payload) => payload,
            Err(error) => {
                return Ok(Completion {
                    descriptor: Some(descriptor),
                    buffer_count: completed_buffer_count,
                    disposition: Disposition::Dropped(DropReason::Descriptor(error)),
                });
            }
        };
        if payload_output.len() < payload.len() {
            return Err(Error::OutputTooSmall);
        }
        payload_output[..payload.len()].copy_from_slice(payload);
        return Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completed_buffer_count,
            disposition: Disposition::Delivered {
                payload_bytes: payload.len(),
            },
        });
    }

    let continuation_count = usize::from(descriptor.more_buffers);
    let mut continuation_storage: [&[u8]; u8::MAX as usize] = [&[]; u8::MAX as usize];
    for index in 0..continuation_count {
        let lease = leases_output[index + 1].ok_or(Error::MissingLease)?;
        let Some(buffer) = buffers_by_id.get(usize::from(lease.buffer_id())).copied() else {
            return Ok(Completion {
                descriptor: Some(descriptor),
                buffer_count: completed_buffer_count,
                disposition: Disposition::Dropped(DropReason::BufferUnavailable(lease.buffer_id())),
            });
        };
        continuation_storage[index] = buffer;
    }

    match htt_rx::unchain_raw_msdu(
        descriptor,
        first_buffer,
        &continuation_storage[..continuation_count],
        completed_buffer_count,
        payload_output,
    ) {
        Ok(UnchainOutcome::Assembled { length }) => Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completed_buffer_count,
            disposition: Disposition::Delivered {
                payload_bytes: length,
            },
        }),
        Ok(UnchainOutcome::Drop(reason)) => Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completed_buffer_count,
            disposition: Disposition::Dropped(DropReason::Unchain(reason)),
        }),
        Ok(UnchainOutcome::NotChained) => Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completed_buffer_count,
            disposition: Disposition::Dropped(DropReason::ChainCountMismatch {
                expected: 1,
                completed: completed_buffer_count,
            }),
        }),
        Err(htt_rx::Error::OutputTooSmall) => Err(Error::OutputTooSmall),
        Err(error) => Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completed_buffer_count,
            disposition: Disposition::Dropped(DropReason::Descriptor(error)),
        }),
    }
}

/// Retire one FIFO-completed group and reconstruct an unchained WCN3990
/// Native-WiFi data MSDU into its original 802.11 header form. The raw
/// `rx_hdr_status` bytes come from the first buffer's own descriptor, so the
/// caller cannot accidentally pair a header snapshot with another packet.
///
/// Non-Native-WiFi decapsulation, malformed frames, and unsupported chains are
/// dropped while all completed leases remain available to the caller. This
/// routine only handles data-frame byte reconstruction; it does not classify
/// RX status, verify crypto/decryption, service management frames, or deliver
/// the frame to a network/control service.
pub fn complete_fifo_native_wifi_data_msdu(
    owner: &mut RxRingOwner,
    completed_buffer_count: usize,
    buffers_by_id: &[&[u8]],
    leases_output: &mut [Option<RxBufferLease>],
    four_address_padding: FourAddressPadding,
    iv_stripped: bool,
    crypto_parameters: CryptoParameters,
    frame_output: &mut [u8],
) -> Result<Completion, Error> {
    let mut decapped_output = [0u8; 2048];
    let completion = complete_fifo_msdu(
        owner,
        completed_buffer_count,
        buffers_by_id,
        leases_output,
        &mut decapped_output,
    )?;

    let Disposition::Delivered {
        payload_bytes: decapped_bytes,
    } = completion.disposition
    else {
        return Ok(completion);
    };
    let descriptor = completion.descriptor.ok_or(Error::MissingLease)?;
    if descriptor.decap_format != WCN3990_NATIVE_WIFI_DECAP_FORMAT {
        return Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completion.buffer_count,
            disposition: Disposition::Dropped(DropReason::WrongDecapsulationFormat {
                expected: WCN3990_NATIVE_WIFI_DECAP_FORMAT,
                observed: descriptor.decap_format,
            }),
        });
    }
    if !descriptor.first_msdu || !descriptor.last_msdu {
        return Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completion.buffer_count,
            disposition: Disposition::Dropped(DropReason::NativeWifiMsduBoundary {
                first: descriptor.first_msdu,
                last: descriptor.last_msdu,
            }),
        });
    }

    let lease = leases_output[0].ok_or(Error::MissingLease)?;
    let first_buffer = buffers_by_id
        .get(usize::from(lease.buffer_id()))
        .copied()
        .ok_or(Error::MissingLease)?;
    let Some(header_status) =
        first_buffer.get(WCN3990_RX_HEADER_STATUS_OFFSET..WCN3990_RX_HEADER_STATUS_END)
    else {
        return Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completion.buffer_count,
            disposition: Disposition::Dropped(DropReason::NativeWifi(
                nwifi_rx::Error::HeaderStatusTooShort,
            )),
        });
    };

    match nwifi_rx::rebuild_data_msdu(
        four_address_padding,
        header_status,
        &decapped_output[..decapped_bytes],
        iv_stripped,
        crypto_parameters,
        frame_output,
    ) {
        Ok(frame) => Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completion.buffer_count,
            disposition: Disposition::Delivered {
                payload_bytes: frame.frame_bytes,
            },
        }),
        Err(nwifi_rx::Error::OutputTooSmall) => Err(Error::OutputTooSmall),
        Err(error) => Ok(Completion {
            descriptor: Some(descriptor),
            buffer_count: completion.buffer_count,
            disposition: Disposition::Dropped(DropReason::NativeWifi(error)),
        }),
    }
}
