//! SCSI READ(10)/WRITE(10)/SYNCHRONIZE CACHE bridge to the modem storage API.
//!
//! This composes the bounded UFSHCI host with the modem's `BlockIo` contract.
//! It does not initialize the platform, acquire a live UFS lease, or create an
//! `ObservationSource`; callers must wrap it in `UfsModemStorage` so each
//! operation is checked against the configured media and live SMMU authority.
#![forbid(unsafe_code)]

use crate::qcom_modem_handlers::Fault;
use crate::qcom_ufs_host::{DmaIo, Host, RegisterIo, State};
use crate::qcom_ufs_modem_storage::{BlockIo, MediaObservation};
use crate::qcom_ufs_protocol::{self as protocol, ScsiCommand};

const MAX_POLL_ATTEMPTS: u32 = 64;
const COMMAND_DESCRIPTOR_BYTES: u64 = 1040;

pub trait ObservationSource {
    /// Return facts from the current retained platform/media lease. A
    /// production implementation must not synthesize values from configuration.
    fn observe_media(&mut self) -> Result<MediaObservation, Fault>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    HostNotOperational,
    InvalidLun,
    InvalidPollLimit,
    InvalidDmaAddress,
}

pub struct UfsScsiBlockIo<'a, R: RegisterIo, D: DmaIo, O: ObservationSource> {
    host: &'a mut Host,
    registers: &'a mut R,
    dma: &'a mut D,
    observation: &'a mut O,
    lun: u8,
    command_descriptor: u64,
    data_address: u64,
    poll_limit: u32,
    next_task_tag: u8,
}

impl<'a, R: RegisterIo, D: DmaIo, O: ObservationSource> UfsScsiBlockIo<'a, R, D, O> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: &'a mut Host,
        registers: &'a mut R,
        dma: &'a mut D,
        observation: &'a mut O,
        lun: u8,
        command_descriptor: u64,
        data_address: u64,
        poll_limit: u32,
    ) -> Result<Self, Error> {
        if host.state() != State::Operational {
            return Err(Error::HostNotOperational);
        }
        if lun >= 8 {
            return Err(Error::InvalidLun);
        }
        if poll_limit == 0 || poll_limit > MAX_POLL_ATTEMPTS {
            return Err(Error::InvalidPollLimit);
        }
        let descriptor_end = command_descriptor
            .checked_add(COMMAND_DESCRIPTOR_BYTES)
            .ok_or(Error::InvalidDmaAddress)?;
        let data_end = data_address
            .checked_add(protocol::MAX_DATA_BYTES as u64)
            .ok_or(Error::InvalidDmaAddress)?;
        if command_descriptor == 0
            || command_descriptor & 0x7f != 0
            || data_address == 0
            || data_address & 3 != 0
            || (command_descriptor < data_end && data_address < descriptor_end)
        {
            return Err(Error::InvalidDmaAddress);
        }
        Ok(Self {
            host,
            registers,
            dma,
            observation,
            lun,
            command_descriptor,
            data_address,
            poll_limit,
            next_task_tag: 0,
        })
    }

    fn next_tag(&mut self) -> u8 {
        let tag = self.next_task_tag;
        self.next_task_tag = self.next_task_tag.wrapping_add(1) & 0x1f;
        tag
    }

    fn submit(&mut self, command: ScsiCommand, data_address: Option<u64>) -> Result<(), Fault> {
        if self.host.state() != State::Operational {
            return Err(Fault::Failed);
        }
        let task_tag = self.next_tag();
        self.host
            .execute_command(
                self.registers,
                self.dma,
                command,
                self.lun,
                task_tag,
                self.command_descriptor,
                data_address,
                self.poll_limit,
            )
            // Conservatively quarantine all submission failures: errors can
            // occur after doorbell publication and must never trigger replay.
            .map_err(|_| Fault::Uncertain)
    }
}

fn checked_transfer(first_lba: u64, bytes: usize) -> Result<u16, Fault> {
    if bytes == 0 || bytes > protocol::MAX_DATA_BYTES || bytes as u64 % protocol::SECTOR_BYTES != 0
    {
        return Err(Fault::Failed);
    }
    let blocks = u16::try_from(bytes as u64 / protocol::SECTOR_BYTES).map_err(|_| Fault::Failed)?;
    let last_lba = first_lba
        .checked_add(u64::from(blocks) - 1)
        .ok_or(Fault::Failed)?;
    if last_lba > u64::from(u32::MAX) {
        return Err(Fault::Failed);
    }
    Ok(blocks)
}

fn checked_lun_range(first_lba: u64, bytes: usize, lun_sectors: u64) -> Result<u16, Fault> {
    let blocks = checked_transfer(first_lba, bytes)?;
    let end = first_lba
        .checked_add(u64::from(blocks))
        .ok_or(Fault::Failed)?;
    if end > lun_sectors {
        return Err(Fault::Failed);
    }
    Ok(blocks)
}

impl<'a, R: RegisterIo, D: DmaIo, O: ObservationSource> UfsScsiBlockIo<'a, R, D, O> {
    fn media_geometry(&mut self) -> Result<(u32, u64), Fault> {
        let observation = self.observation.observe_media()?;
        if !protocol::supported_logical_block_bytes(observation.logical_block_bytes)
            || observation.lun_sectors == 0
            || observation.lun_sectors
                % (u64::from(observation.logical_block_bytes) / protocol::SECTOR_BYTES)
                != 0
        {
            return Err(Fault::Failed);
        }
        Ok((observation.logical_block_bytes, observation.lun_sectors))
    }

    fn read_native_block(
        &mut self,
        native_lba: u64,
        block_bytes: u32,
        out: &mut [u8],
    ) -> Result<(), Fault> {
        if out.len() != block_bytes as usize {
            return Err(Fault::Failed);
        }
        let command = ScsiCommand::read10_with_block_size(native_lba, 1, block_bytes)
            .map_err(|_| Fault::Failed)?;
        self.submit(command, Some(self.data_address))?;
        self.dma
            .read(self.data_address, out)
            .map_err(|_| Fault::Failed)
    }

    fn write_native_block(
        &mut self,
        native_lba: u64,
        block_bytes: u32,
        bytes: &[u8],
    ) -> Result<(), Fault> {
        if bytes.len() != block_bytes as usize {
            return Err(Fault::Failed);
        }
        let command = ScsiCommand::write10_with_block_size(native_lba, 1, block_bytes)
            .map_err(|_| Fault::Failed)?;
        self.dma
            .write(self.data_address, bytes)
            .map_err(|_| Fault::Failed)?;
        self.submit(command, Some(self.data_address))
    }
}

impl<R: RegisterIo, D: DmaIo, O: ObservationSource> BlockIo for UfsScsiBlockIo<'_, R, D, O> {
    fn observe(&mut self) -> Result<MediaObservation, Fault> {
        if self.host.state() != State::Operational {
            return Err(Fault::Failed);
        }
        let observation = self.observation.observe_media()?;
        if !protocol::supported_logical_block_bytes(observation.logical_block_bytes)
            || observation.lun_sectors == 0
            || observation.lun_sectors
                % (u64::from(observation.logical_block_bytes) / protocol::SECTOR_BYTES)
                != 0
        {
            return Err(Fault::Failed);
        }
        Ok(observation)
    }

    fn read_blocks(&mut self, first_lba: u64, out: &mut [u8]) -> Result<(), Fault> {
        let (block_bytes, lun_sectors) = self.media_geometry()?;
        let blocks = checked_lun_range(first_lba, out.len(), lun_sectors)?;
        if block_bytes == protocol::SECTOR_BYTES as u32 {
            let command = ScsiCommand::read10_with_block_size(first_lba, blocks, block_bytes)
                .map_err(|_| Fault::Failed)?;
            self.submit(command, Some(self.data_address))?;
            return self
                .dma
                .read(self.data_address, out)
                .map_err(|_| Fault::Failed);
        }

        let start_byte = first_lba
            .checked_mul(protocol::SECTOR_BYTES)
            .ok_or(Fault::Failed)?;
        let mut completed = 0usize;
        let mut native = [0u8; protocol::MAX_DATA_BYTES];
        while completed < out.len() {
            let byte_at = start_byte
                .checked_add(completed as u64)
                .ok_or(Fault::Failed)?;
            let native_lba = byte_at / u64::from(block_bytes);
            let within = (byte_at % u64::from(block_bytes)) as usize;
            let take = (out.len() - completed).min(block_bytes as usize - within);
            self.read_native_block(native_lba, block_bytes, &mut native[..block_bytes as usize])?;
            out[completed..completed + take].copy_from_slice(&native[within..within + take]);
            completed += take;
        }
        Ok(())
    }

    fn write_blocks(&mut self, first_lba: u64, bytes: &[u8]) -> Result<(), Fault> {
        let (block_bytes, lun_sectors) = self.media_geometry()?;
        let blocks = checked_lun_range(first_lba, bytes.len(), lun_sectors)?;
        if block_bytes == protocol::SECTOR_BYTES as u32 {
            let command = ScsiCommand::write10_with_block_size(first_lba, blocks, block_bytes)
                .map_err(|_| Fault::Failed)?;
            self.dma
                .write(self.data_address, bytes)
                .map_err(|_| Fault::Failed)?;
            return self.submit(command, Some(self.data_address));
        }

        let start_byte = first_lba
            .checked_mul(protocol::SECTOR_BYTES)
            .ok_or(Fault::Failed)?;
        let mut completed = 0usize;
        let mut written_any = false;
        let mut native = [0u8; protocol::MAX_DATA_BYTES];
        while completed < bytes.len() {
            let byte_at = start_byte
                .checked_add(completed as u64)
                .ok_or(Fault::Failed)?;
            let native_lba = byte_at / u64::from(block_bytes);
            let within = (byte_at % u64::from(block_bytes)) as usize;
            let take = (bytes.len() - completed).min(block_bytes as usize - within);
            if within != 0 || take != block_bytes as usize {
                if let Err(error) = self.read_native_block(
                    native_lba,
                    block_bytes,
                    &mut native[..block_bytes as usize],
                ) {
                    return Err(if written_any { Fault::Uncertain } else { error });
                }
                native[within..within + take].copy_from_slice(&bytes[completed..completed + take]);
                if let Err(error) = self.write_native_block(
                    native_lba,
                    block_bytes,
                    &native[..block_bytes as usize],
                ) {
                    return Err(if written_any { Fault::Uncertain } else { error });
                }
            } else if let Err(error) = self.write_native_block(
                native_lba,
                block_bytes,
                &bytes[completed..completed + take],
            ) {
                return Err(if written_any { Fault::Uncertain } else { error });
            }
            written_any = true;
            completed += take;
        }
        Ok(())
    }

    fn synchronize_cache(&mut self) -> Result<(), Fault> {
        self.submit(ScsiCommand::synchronize_cache10(), None)
    }
}
