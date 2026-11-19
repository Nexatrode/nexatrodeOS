//! Apply QCACLD MPDU status policy and synchronously hand Ethernet MSDUs to a
//! network sink.
//!
//! This composes the HTT RX_IND status/firmware bytes with the separately
//! posted MSDU buffer ring. It copies one MPDU's MSDUs into caller scratch,
//! applies the source rule that unauthenticated peers may deliver EAPOL only,
//! and releases every claimed lease after delivery, filtering, or failure. The
//! sink must consume or copy the borrowed frame before returning. This module
//! does not synchronize DMA, refill/publish the hardware ring, or own a Gate.
#![forbid(unsafe_code)]

use crate::sunfish_qcacld_rx::ParseOptions;
use crate::sunfish_qcacld_rx_ind::{HttRxIndication, MpduStatus};
use crate::sunfish_qcacld_rx_session::{
    Error as SessionError, RxBufferLease, RxGroupSyncError, RxMpduGroup, RxMsduChain,
    SunfishQcacldRxSession,
};

pub trait EthernetRxSink {
    type Error;

    /// Synchronously consume or copy one Ethernet-decapsulated MSDU.
    fn receive(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

impl<F, E> EthernetRxSink for F
where
    F: FnMut(&[u8]) -> Result<(), E>,
{
    type Error = E;

    fn receive(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self(frame)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxDeliveryReport {
    pub group: RxMpduGroup,
    pub delivered_msdus: usize,
    pub filtered_msdus: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxIndicationDeliveryReport {
    pub mpdu_count: usize,
    pub delivered_msdus: usize,
    pub filtered_msdus: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxIndicationRecycleReport {
    pub delivery: RxIndicationDeliveryReport,
    pub recycled_buffer_count: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RxDeliveryError<E> {
    Claim(SessionError),
    Copy(SessionError),
    Sink(E),
    Release(SessionError),
}

#[derive(Debug, Eq, PartialEq)]
pub enum RxDeliverySyncError<S, D> {
    BufferSync(D),
    Delivery(RxDeliveryError<S>),
}

/// Drain each MPDU advertised by one validated HTT RX indication in FIFO
/// order. The per-MSDU firmware-descriptor cursor advances by the actual MSDU
/// count found in each ring group, matching QCACLD even when an MPDU contains
/// an A-MSDU or the indication's descriptor span has a missing tail.
pub fn deliver_indication<const N: usize, S: EthernetRxSink>(
    session: &mut SunfishQcacldRxSession<N>,
    buffers_by_id: &[&[u8]],
    options: ParseOptions,
    indication: HttRxIndication<'_>,
    msdus_output: &mut [Option<RxMsduChain>],
    leases_output: &mut [Option<RxBufferLease>],
    frame_scratch: &mut [u8],
    sink: &mut S,
) -> Result<RxIndicationDeliveryReport, RxDeliveryError<S::Error>> {
    let firmware_descriptors = indication.firmware_descriptor_bytes();
    let mut firmware_descriptor_start = 0usize;
    let mut report = RxIndicationDeliveryReport {
        mpdu_count: 0,
        delivered_msdus: 0,
        filtered_msdus: 0,
    };

    for mpdu in indication.mpdus() {
        let group_report = deliver_mpdu(
            session,
            buffers_by_id,
            options,
            firmware_descriptors,
            firmware_descriptor_start,
            mpdu.status,
            msdus_output,
            leases_output,
            frame_scratch,
            sink,
        )?;
        firmware_descriptor_start = firmware_descriptor_start
            .checked_add(group_report.group.msdu_count)
            .ok_or(RxDeliveryError::Claim(SessionError::InvalidChainLength))?;
        report.mpdu_count += 1;
        report.delivered_msdus += group_report.delivered_msdus;
        report.filtered_msdus += group_report.filtered_msdus;
    }

    Ok(report)
}

/// Drain an indication while synchronizing each completed HTT buffer for CPU
/// access before reading its RX descriptor or payload. The callback receives
/// opaque buffer IDs in FIFO order. A synchronization failure leaves the
/// current MPDU unclaimed; the caller must quarantine the ring generation
/// unless its provider can prove retry is safe. Earlier MPDUs in this
/// indication may already have been delivered and released.
pub fn deliver_indication_with_cpu_sync<const N: usize, S: EthernetRxSink, D>(
    session: &mut SunfishQcacldRxSession<N>,
    buffers_by_id: &[&[u8]],
    options: ParseOptions,
    indication: HttRxIndication<'_>,
    msdus_output: &mut [Option<RxMsduChain>],
    leases_output: &mut [Option<RxBufferLease>],
    frame_scratch: &mut [u8],
    sink: &mut S,
    sync_for_cpu: impl FnMut(u32) -> Result<(), D>,
) -> Result<RxIndicationDeliveryReport, RxDeliverySyncError<S::Error, D>> {
    deliver_indication_with_cpu_sync_inner(
        session,
        buffers_by_id,
        options,
        indication,
        msdus_output,
        leases_output,
        frame_scratch,
        sink,
        sync_for_cpu,
        None,
    )
    .map(|(report, _)| report)
}

/// CPU-sync and deliver an indication while collecting the exact FIFO buffer
/// IDs released by delivery. On success the caller can pass the returned ID
/// prefix to the HTT ring batch refiller after the borrowed CPU buffer view
/// ends. `recycled_buffer_ids` must be large enough for every currently posted
/// buffer; capacity is checked before synchronization or delivery begins.
pub fn deliver_indication_with_cpu_sync_and_collect<const N: usize, S: EthernetRxSink, D>(
    session: &mut SunfishQcacldRxSession<N>,
    buffers_by_id: &[&[u8]],
    options: ParseOptions,
    indication: HttRxIndication<'_>,
    msdus_output: &mut [Option<RxMsduChain>],
    leases_output: &mut [Option<RxBufferLease>],
    frame_scratch: &mut [u8],
    sink: &mut S,
    sync_for_cpu: impl FnMut(u32) -> Result<(), D>,
    recycled_buffer_ids: &mut [u32],
) -> Result<RxIndicationRecycleReport, RxDeliverySyncError<S::Error, D>> {
    let (delivery, recycled_buffer_count) = deliver_indication_with_cpu_sync_inner(
        session,
        buffers_by_id,
        options,
        indication,
        msdus_output,
        leases_output,
        frame_scratch,
        sink,
        sync_for_cpu,
        Some(recycled_buffer_ids),
    )?;
    Ok(RxIndicationRecycleReport {
        delivery,
        recycled_buffer_count,
    })
}

#[allow(clippy::too_many_arguments)]
fn deliver_indication_with_cpu_sync_inner<const N: usize, S: EthernetRxSink, D>(
    session: &mut SunfishQcacldRxSession<N>,
    buffers_by_id: &[&[u8]],
    options: ParseOptions,
    indication: HttRxIndication<'_>,
    msdus_output: &mut [Option<RxMsduChain>],
    leases_output: &mut [Option<RxBufferLease>],
    frame_scratch: &mut [u8],
    sink: &mut S,
    mut sync_for_cpu: impl FnMut(u32) -> Result<(), D>,
    mut recycled_buffer_ids: Option<&mut [u32]>,
) -> Result<(RxIndicationDeliveryReport, usize), RxDeliverySyncError<S::Error, D>> {
    if let Some(output) = recycled_buffer_ids.as_deref()
        && output.len() < session.posted_count()
    {
        return Err(RxDeliverySyncError::Delivery(RxDeliveryError::Claim(
            SessionError::OutputTooSmall {
                needed: session.posted_count(),
                available: output.len(),
            },
        )));
    }

    let firmware_descriptors = indication.firmware_descriptor_bytes();
    let mut firmware_descriptor_start = 0usize;
    let mut recycled_count = 0usize;
    let mut report = RxIndicationDeliveryReport {
        mpdu_count: 0,
        delivered_msdus: 0,
        filtered_msdus: 0,
    };

    for mpdu in indication.mpdus() {
        let group_buffer_count = session
            .sync_next_mpdu_group_for_cpu(
                buffers_by_id,
                options,
                firmware_descriptors,
                firmware_descriptor_start,
                &mut sync_for_cpu,
            )
            .map_err(|error| match error {
                RxGroupSyncError::Session(error) => {
                    RxDeliverySyncError::Delivery(RxDeliveryError::Claim(error))
                }
                RxGroupSyncError::Sync(error) => RxDeliverySyncError::BufferSync(error),
            })?;

        let recycled_end =
            recycled_count
                .checked_add(group_buffer_count)
                .ok_or(RxDeliverySyncError::Delivery(RxDeliveryError::Claim(
                    SessionError::InvalidChainLength,
                )))?;
        if let Some(output) = recycled_buffer_ids.as_deref_mut() {
            session
                .copy_next_posted_buffer_ids(
                    group_buffer_count,
                    &mut output[recycled_count..recycled_end],
                )
                .map_err(|error| RxDeliverySyncError::Delivery(RxDeliveryError::Claim(error)))?;
        }

        let group_report = deliver_mpdu(
            session,
            buffers_by_id,
            options,
            firmware_descriptors,
            firmware_descriptor_start,
            mpdu.status,
            msdus_output,
            leases_output,
            frame_scratch,
            sink,
        )
        .map_err(RxDeliverySyncError::Delivery)?;
        if group_report.group.buffer_count != group_buffer_count {
            return Err(RxDeliverySyncError::Delivery(RxDeliveryError::Claim(
                SessionError::InvalidChainLength,
            )));
        }
        recycled_count = recycled_end;
        firmware_descriptor_start = firmware_descriptor_start
            .checked_add(group_report.group.msdu_count)
            .ok_or(RxDeliverySyncError::Delivery(RxDeliveryError::Claim(
                SessionError::InvalidChainLength,
            )))?;
        report.mpdu_count += 1;
        report.delivered_msdus += group_report.delivered_msdus;
        report.filtered_msdus += group_report.filtered_msdus;
    }

    Ok((report, recycled_count))
}

/// Claim, filter, copy, and synchronously deliver one MPDU's MSDU group.
///
/// `firmware_descriptor_start` identifies the first per-MSDU status byte for
/// this MPDU within the enclosing `HTT_RX_IND`. QCACLD's zero fallback is
/// preserved for missing tail bytes. All leases acquired for the MPDU are
/// released before this function returns, including on sink/copy failure.
pub fn deliver_mpdu<const N: usize, S: EthernetRxSink>(
    session: &mut SunfishQcacldRxSession<N>,
    buffers_by_id: &[&[u8]],
    options: ParseOptions,
    firmware_descriptor_bytes: &[u8],
    firmware_descriptor_start: usize,
    status: MpduStatus,
    msdus_output: &mut [Option<RxMsduChain>],
    leases_output: &mut [Option<RxBufferLease>],
    frame_scratch: &mut [u8],
    sink: &mut S,
) -> Result<RxDeliveryReport, RxDeliveryError<S::Error>> {
    let group = session
        .take_next_mpdu_group_with_firmware_descriptors(
            buffers_by_id,
            options,
            firmware_descriptor_bytes,
            firmware_descriptor_start,
            msdus_output,
            leases_output,
        )
        .map_err(RxDeliveryError::Claim)?;

    let mut delivered_msdus = 0usize;
    let mut filtered_msdus = 0usize;
    let mut failure = None;

    if !status.is_ok() && !status.is_unauthenticated_peer() {
        filtered_msdus = group.msdu_count;
    } else {
        for msdu_index in 0..group.msdu_count {
            let Some(chain) = msdus_output.get(msdu_index).and_then(|chain| *chain) else {
                failure = Some(RxDeliveryError::Copy(SessionError::InvalidChainLength));
                break;
            };
            let Some(firmware_descriptor_index) = firmware_descriptor_start.checked_add(msdu_index)
            else {
                failure = Some(RxDeliveryError::Copy(SessionError::InvalidChainLength));
                break;
            };
            let firmware_descriptor = firmware_descriptor_bytes
                .get(firmware_descriptor_index)
                .copied()
                .unwrap_or(0);

            let copied = match session.copy_ethernet_msdu_chain_with_firmware_descriptor(
                chain,
                leases_output,
                buffers_by_id,
                firmware_descriptor,
                options,
                frame_scratch,
            ) {
                Ok(copied) => copied,
                Err(
                    SessionError::FirmwareDiscard
                    | SessionError::WrongDecapFormat { .. }
                    | SessionError::Descriptor(crate::sunfish_qcacld_rx::Error::FirmwareExtension),
                ) => {
                    filtered_msdus += 1;
                    continue;
                }
                Err(error) => {
                    failure = Some(RxDeliveryError::Copy(error));
                    break;
                }
            };

            let frame = &frame_scratch[..copied.bytes_copied];
            if !status.permits_ethernet_frame(frame) {
                filtered_msdus += 1;
                continue;
            }
            if let Err(error) = sink.receive(frame) {
                failure = Some(RxDeliveryError::Sink(error));
                break;
            }
            delivered_msdus += 1;
        }
    }

    for lease in leases_output.iter_mut().take(group.buffer_count) {
        if let Some(lease) = lease.take() {
            if let Err(error) = session.release(lease) {
                if failure.is_none() {
                    failure = Some(RxDeliveryError::Release(error));
                }
            }
        }
    }

    if let Some(failure) = failure {
        return Err(failure);
    }
    Ok(RxDeliveryReport {
        group,
        delivered_msdus,
        filtered_msdus,
    })
}
