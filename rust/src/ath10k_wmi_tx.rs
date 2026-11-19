//! Bounded single-command WMI-over-HTC TX framing for upstream ath10k.
#![forbid(unsafe_code)]

use crate::ath10k_ce::{
    CeError, CeTxRing, HTC_WMI_CONTROL_SERVICE_ID, HtcCreditLedger, HtcEndpoint, HtcServicePolicy,
    Wcn3990Descriptor,
};

pub const HTC_HEADER_BYTES: usize = 8;
pub const WMI_COMMAND_HEADER_BYTES: usize = 4;
pub const HTC_ENDPOINT_COUNT: u8 = 9;
pub const HTC_FLAG_NEED_CREDIT_UPDATE: u8 = 0x01;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidEndpoint,
    InvalidMaximumMessageSize,
    InvalidCommandId,
    EmptyCommandBody,
    MessageTooLarge,
    LengthOverflow,
    OutputTooSmall,
    ZeroCreditSize,
    InsufficientCredits,
    CeDescriptor(CeError),
    CreditState(CeError),
    TxRing(CeError),
    CreditAccountingMismatch,
    WrongService,
    InvalidWmiControlPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TxPlan {
    pub frame_bytes: usize,
    pub wmi_payload_bytes: usize,
    pub credits_required: u16,
    pub credits_remaining: u16,
    pub credit_update_requested: bool,
    pub sequence_number: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferPlan {
    pub tx: TxPlan,
    pub descriptor: Wcn3990Descriptor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuedTransfer {
    pub slot: usize,
    pub transfer: TransferPlan,
}

/// Host-only command submission state for one connected HTC endpoint. The
/// queue models the upstream credit, sequence, and CE-slot transitions; it
/// does not publish a hardware descriptor, ring a doorbell, or own DMA memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmiCommandTxQueue<const RING_SIZE: usize> {
    endpoint_id: u8,
    maximum_message_bytes: usize,
    credit_size: u16,
    credit_flow_enabled: bool,
    bundle_tx_enabled: bool,
    next_sequence_number: u8,
    credits: HtcCreditLedger,
    ring: CeTxRing<RING_SIZE>,
    wmi_control_bound: bool,
}

impl<const RING_SIZE: usize> WmiCommandTxQueue<RING_SIZE> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint_id: u8,
        maximum_message_bytes: usize,
        credit_size: u16,
        initial_credits: u16,
        credit_flow_enabled: bool,
        bundle_tx_enabled: bool,
        first_sequence_number: u8,
    ) -> Result<Self, Error> {
        if endpoint_id == 0 || endpoint_id >= HTC_ENDPOINT_COUNT {
            return Err(Error::InvalidEndpoint);
        }
        if maximum_message_bytes == 0 || maximum_message_bytes > usize::from(u16::MAX) {
            return Err(Error::InvalidMaximumMessageSize);
        }
        if credit_size == 0 {
            return Err(Error::ZeroCreditSize);
        }

        let mut credits = HtcCreditLedger::new();
        credits
            .configure(
                endpoint_id,
                credit_size,
                initial_credits,
                credit_flow_enabled,
            )
            .map_err(Error::CreditState)?;
        let ring = CeTxRing::new().map_err(Error::TxRing)?;
        Ok(Self {
            endpoint_id,
            maximum_message_bytes,
            credit_size,
            credit_flow_enabled,
            bundle_tx_enabled,
            next_sequence_number: first_sequence_number,
            credits,
            ring,
            wmi_control_bound: false,
        })
    }

    /// Construct a WMI command queue from a negotiated WMI_CONTROL endpoint
    /// and the matching HTC_READY-derived credit policy.
    pub fn new_wmi_control(
        endpoint: HtcEndpoint,
        policy: HtcServicePolicy,
        bundle_tx_enabled: bool,
        first_sequence_number: u8,
    ) -> Result<Self, Error> {
        if endpoint.service_id != HTC_WMI_CONTROL_SERVICE_ID {
            return Err(Error::WrongService);
        }
        if policy.disable_credit_flow
            || policy.credit_size == 0
            || policy.initial_credits == 0
            || u16::from(policy.receive_allocation) != policy.initial_credits
        {
            return Err(Error::InvalidWmiControlPolicy);
        }

        let mut queue = Self::new(
            endpoint.endpoint_id,
            usize::from(endpoint.max_message_size),
            policy.credit_size,
            policy.initial_credits,
            true,
            bundle_tx_enabled,
            first_sequence_number,
        )?;
        queue.wmi_control_bound = true;
        Ok(queue)
    }

    /// Prepare a command and reserve its credit and software CE-ring slot.
    /// As in ath10k, sequence numbers advance once credit is obtained and the
    /// HTC header is prepared. A later descriptor or ring failure restores
    /// credits but does not reuse the sequence number.
    pub fn queue_transfer(
        &mut self,
        command_id: u32,
        command_body: &[u8],
        ce_iova: u64,
        transfer_id: u16,
        context: u32,
        gather: bool,
        byte_swap: bool,
        frame_output: &mut [u8],
    ) -> Result<QueuedTransfer, Error> {
        let available_credits = self
            .credits
            .available(self.endpoint_id)
            .map_err(Error::CreditState)?;
        let sequence_number = self.next_sequence_number;
        let tx = preflight_wmi_command_frame(
            self.endpoint_id,
            self.maximum_message_bytes,
            self.credit_size,
            available_credits,
            self.credit_flow_enabled,
            self.bundle_tx_enabled,
            sequence_number,
            command_id,
            command_body,
            frame_output.len(),
        )?;

        let mut next_credits = self.credits;
        let consumed = next_credits
            .consume(self.endpoint_id, tx.frame_bytes)
            .map_err(Error::CreditState)?;
        if consumed != tx.credits_required {
            return Err(Error::CreditAccountingMismatch);
        }

        // Linux increments the endpoint sequence in HTC header preparation,
        // before DMA mapping and the HIF/CE queue operation can fail.
        self.next_sequence_number = self.next_sequence_number.wrapping_add(1);

        let descriptor =
            Wcn3990Descriptor::encode(ce_iova, tx.frame_bytes, transfer_id, gather, byte_swap)
                .map_err(Error::CeDescriptor)?;
        write_wmi_command_frame(self.endpoint_id, command_id, command_body, tx, frame_output);

        let mut next_ring = self.ring;
        let slot = next_ring.enqueue(context).map_err(Error::TxRing)?;
        self.credits = next_credits;
        self.ring = next_ring;
        Ok(QueuedTransfer {
            slot,
            transfer: TransferPlan { tx, descriptor },
        })
    }

    /// Apply credits returned by a validated HTC trailer report. CE completion
    /// alone does not return HTC credits.
    pub fn apply_credit_report(&mut self, credits: u16) -> Result<(), Error> {
        self.credits
            .apply_report(self.endpoint_id, credits)
            .map_err(Error::CreditState)
    }

    /// Reclaim software transfer contexts through the observed CE read index.
    pub fn reclaim_to(
        &mut self,
        hardware_read_index: usize,
        output: &mut [u32],
    ) -> Result<usize, Error> {
        self.ring
            .reclaim_to(hardware_read_index, output)
            .map_err(Error::TxRing)
    }

    pub fn credits_available(&self) -> Result<u16, Error> {
        self.credits
            .available(self.endpoint_id)
            .map_err(Error::CreditState)
    }

    pub const fn next_sequence_number(&self) -> u8 {
        self.next_sequence_number
    }

    pub const fn ring_indices(&self) -> (usize, usize) {
        self.ring.indices()
    }

    pub const fn is_wmi_control_bound(&self) -> bool {
        self.wmi_control_bound
    }
}

/// Encode a single upstream ath10k command into one HTC frame.
///
/// `maximum_message_bytes` is the connected endpoint's `max_msg_size` and
/// applies to the WMI command header plus body. When credit flow is enabled,
/// the credit reservation includes the 8-byte HTC header, matching
/// `ath10k_htc_send()`. This does not send the frame or mutate a live ledger.
pub fn encode_wmi_command_frame(
    endpoint_id: u8,
    maximum_message_bytes: usize,
    credit_size: u16,
    available_credits: u16,
    credit_flow_enabled: bool,
    bundle_tx_enabled: bool,
    sequence_number: u8,
    command_id: u32,
    command_body: &[u8],
    output: &mut [u8],
) -> Result<TxPlan, Error> {
    let plan = preflight_wmi_command_frame(
        endpoint_id,
        maximum_message_bytes,
        credit_size,
        available_credits,
        credit_flow_enabled,
        bundle_tx_enabled,
        sequence_number,
        command_id,
        command_body,
        output.len(),
    )?;
    write_wmi_command_frame(endpoint_id, command_id, command_body, plan, output);
    Ok(plan)
}

/// Encode one WMI command frame and its caller-supplied WCN3990 CE descriptor.
///
/// Descriptor validation runs before the frame output is changed. The returned
/// descriptor only describes the IOVA supplied by the caller; this function
/// does not allocate/map DMA memory, reserve a ring slot or HTC credit, or
/// submit anything to hardware.
#[allow(clippy::too_many_arguments)]
pub fn encode_wmi_command_transfer(
    endpoint_id: u8,
    maximum_message_bytes: usize,
    credit_size: u16,
    available_credits: u16,
    credit_flow_enabled: bool,
    bundle_tx_enabled: bool,
    sequence_number: u8,
    command_id: u32,
    command_body: &[u8],
    ce_iova: u64,
    transfer_id: u16,
    gather: bool,
    byte_swap: bool,
    frame_output: &mut [u8],
) -> Result<TransferPlan, Error> {
    let tx = preflight_wmi_command_frame(
        endpoint_id,
        maximum_message_bytes,
        credit_size,
        available_credits,
        credit_flow_enabled,
        bundle_tx_enabled,
        sequence_number,
        command_id,
        command_body,
        frame_output.len(),
    )?;
    let descriptor =
        Wcn3990Descriptor::encode(ce_iova, tx.frame_bytes, transfer_id, gather, byte_swap)
            .map_err(Error::CeDescriptor)?;
    write_wmi_command_frame(endpoint_id, command_id, command_body, tx, frame_output);
    Ok(TransferPlan { tx, descriptor })
}

#[allow(clippy::too_many_arguments)]
fn preflight_wmi_command_frame(
    endpoint_id: u8,
    maximum_message_bytes: usize,
    credit_size: u16,
    available_credits: u16,
    credit_flow_enabled: bool,
    bundle_tx_enabled: bool,
    sequence_number: u8,
    command_id: u32,
    command_body: &[u8],
    output_bytes: usize,
) -> Result<TxPlan, Error> {
    if endpoint_id == 0 || endpoint_id >= HTC_ENDPOINT_COUNT {
        return Err(Error::InvalidEndpoint);
    }
    if maximum_message_bytes == 0 || maximum_message_bytes > usize::from(u16::MAX) {
        return Err(Error::InvalidMaximumMessageSize);
    }
    if command_id > 0x00ff_ffff {
        return Err(Error::InvalidCommandId);
    }
    if command_body.is_empty() {
        return Err(Error::EmptyCommandBody);
    }

    let wmi_payload_bytes = WMI_COMMAND_HEADER_BYTES
        .checked_add(command_body.len())
        .ok_or(Error::LengthOverflow)?;
    if wmi_payload_bytes > maximum_message_bytes || wmi_payload_bytes > usize::from(u16::MAX) {
        return Err(Error::MessageTooLarge);
    }
    let frame_bytes = HTC_HEADER_BYTES
        .checked_add(wmi_payload_bytes)
        .ok_or(Error::LengthOverflow)?;
    if output_bytes < frame_bytes {
        return Err(Error::OutputTooSmall);
    }

    let credits_required = if credit_flow_enabled {
        if credit_size == 0 {
            return Err(Error::ZeroCreditSize);
        }
        let credits = div_ceil(frame_bytes, usize::from(credit_size));
        u16::try_from(credits).map_err(|_| Error::MessageTooLarge)?
    } else {
        0
    };
    if credits_required > available_credits {
        return Err(Error::InsufficientCredits);
    }
    let credits_remaining = available_credits - credits_required;
    let credit_update_requested = credit_flow_enabled && !bundle_tx_enabled;

    Ok(TxPlan {
        frame_bytes,
        wmi_payload_bytes,
        credits_required,
        credits_remaining,
        credit_update_requested,
        sequence_number,
    })
}

fn write_wmi_command_frame(
    endpoint_id: u8,
    command_id: u32,
    command_body: &[u8],
    plan: TxPlan,
    output: &mut [u8],
) {
    let frame_bytes = plan.frame_bytes;
    let wmi_payload_bytes = plan.wmi_payload_bytes;
    output[..frame_bytes].fill(0);
    output[0] = endpoint_id;
    output[1] = if plan.credit_update_requested {
        HTC_FLAG_NEED_CREDIT_UPDATE
    } else {
        0
    };
    output[2..4].copy_from_slice(&(wmi_payload_bytes as u16).to_le_bytes());
    output[5] = plan.sequence_number;
    output[HTC_HEADER_BYTES..HTC_HEADER_BYTES + WMI_COMMAND_HEADER_BYTES]
        .copy_from_slice(&command_id.to_le_bytes());
    output[HTC_HEADER_BYTES + WMI_COMMAND_HEADER_BYTES..frame_bytes].copy_from_slice(command_body);
}

fn div_ceil(value: usize, divisor: usize) -> usize {
    value / divisor + usize::from(value % divisor != 0)
}
