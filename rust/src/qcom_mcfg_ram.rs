//! Bounded in-memory staging for `/readwrite/mcfg.tmp` only.
//! No transport, filesystem, persistent storage, or concurrent writer support.
#![forbid(unsafe_code)]

pub const CAPACITY: usize = 8192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Busy,
    NoWriter,
    NoFile,
    OutOfOrder,
    ChangedRetry,
    Capacity,
    BadOffset,
    BadRequest,
    OutputTooSmall,
}

pub struct McfgRam {
    committed: [u8; CAPACITY],
    committed_len: usize,
    present: bool,
    staging: [u8; CAPACITY],
    staging_len: usize,
    writer: Option<u32>,
    last_block: u16,
    last_len: usize,
}

impl McfgRam {
    pub const fn new() -> Self {
        Self {
            committed: [0; CAPACITY],
            committed_len: 0,
            present: false,
            staging: [0; CAPACITY],
            staging_len: 0,
            writer: None,
            last_block: 0,
            last_len: 0,
        }
    }
    pub fn begin(&mut self, peer: u32) -> Result<(), Error> {
        match self.writer {
            Some(current) if current == peer && self.last_block == 0 => return Ok(()),
            Some(_) => return Err(Error::Busy),
            None => {}
        }
        self.writer = Some(peer);
        self.staging_len = 0;
        self.last_block = 0;
        self.last_len = 0;
        Ok(())
    }
    pub fn write_block(&mut self, peer: u32, block: u16, bytes: &[u8]) -> Result<(), Error> {
        if self.writer != Some(peer) {
            return Err(Error::NoWriter);
        }
        if block != 0 && block == self.last_block {
            return if self.last_len == bytes.len()
                && self.staging[self.staging_len - self.last_len..self.staging_len] == *bytes
            {
                Ok(())
            } else {
                Err(Error::ChangedRetry)
            };
        }
        if self.last_block.checked_add(1) != Some(block) {
            return Err(Error::OutOfOrder);
        }
        let end = self
            .staging_len
            .checked_add(bytes.len())
            .filter(|end| *end <= CAPACITY)
            .ok_or(Error::Capacity)?;
        self.staging[self.staging_len..end].copy_from_slice(bytes);
        self.staging_len = end;
        self.last_block = block;
        self.last_len = bytes.len();
        Ok(())
    }
    pub fn finish(&mut self, peer: u32) -> Result<(), Error> {
        if self.writer != Some(peer) {
            return Err(Error::NoWriter);
        }
        self.committed[..self.staging_len].copy_from_slice(&self.staging[..self.staging_len]);
        self.committed_len = self.staging_len;
        self.present = true;
        self.writer = None;
        Ok(())
    }
    pub fn abort(&mut self, peer: u32) -> Result<(), Error> {
        if self.writer != Some(peer) {
            return Err(Error::NoWriter);
        }
        self.writer = None;
        self.staging_len = 0;
        Ok(())
    }
    pub fn len(&self) -> Option<usize> {
        self.present.then_some(self.committed_len)
    }

    pub fn read(&self, offset: usize, out: &mut [u8]) -> Result<usize, Error> {
        if !self.present {
            return Err(Error::NoFile);
        }
        if offset > self.committed_len {
            return Err(Error::BadOffset);
        }
        let n = out.len().min(self.committed_len - offset);
        out[..n].copy_from_slice(&self.committed[offset..offset + n]);
        Ok(n)
    }
}

/// Restricted first WRQ parser. No values from the unobserved suffix of a
/// captured packet are assumed. Return a 512-byte block and window of one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteOptions {
    pub timeout_ms: u32,
}

pub fn decode_write_request(packet: &[u8]) -> Result<WriteOptions, Error> {
    if packet.len() > 512 || !packet.starts_with(&[0, 2]) {
        return Err(Error::BadRequest);
    }
    let mut fields = packet[2..].split(|b| *b == 0);
    if fields.next() != Some(b"/readwrite/mcfg.tmp".as_slice())
        || !fields
            .next()
            .is_some_and(|mode| mode.eq_ignore_ascii_case(b"octet"))
        || packet.last() != Some(&0)
    {
        return Err(Error::BadRequest);
    }
    let mut seen = 0u8;
    let mut timeout_ms = 1000;
    loop {
        let name = fields.next().ok_or(Error::BadRequest)?;
        if name.is_empty() {
            if fields.next().is_none() {
                break;
            }
            return Err(Error::BadRequest);
        }
        let value = fields.next().ok_or(Error::BadRequest)?;
        if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
            return Err(Error::BadRequest);
        }
        let mut n = 0u32;
        for c in value {
            n = n
                .checked_mul(10)
                .and_then(|v| v.checked_add(u32::from(c - b'0')))
                .ok_or(Error::BadRequest)?;
        }
        let bit = match name {
            b"blksize" => {
                if !(512..=65464).contains(&n) {
                    return Err(Error::BadRequest);
                }
                1
            }
            b"timeoutms" => {
                if !(1..=255000).contains(&n) {
                    return Err(Error::BadRequest);
                }
                timeout_ms = n;
                2
            }
            b"wsize" => {
                if !(1..=65535).contains(&n) {
                    return Err(Error::BadRequest);
                }
                4
            }
            _ => return Err(Error::BadRequest),
        };
        if seen & bit != 0 {
            return Err(Error::BadRequest);
        }
        seen |= bit;
    }
    // A fixed OACK must not introduce options the requester did not send.
    if seen != 7 || timeout_ms != 1000 {
        return Err(Error::BadRequest);
    }
    Ok(WriteOptions { timeout_ms })
}

pub const WRITE_OACK: &[u8] = b"\0\x06blksize\0512\0timeoutms\01000\0wsize\01\0";

/// One peer's bounded write transfer. Callers must route only that peer's
/// packets here and retain each returned reply until transport publication.
pub struct WriteTransfer {
    peer: u32,
    done: bool,
    final_block: u16,
    final_data: [u8; 512],
    final_len: usize,
}

impl WriteTransfer {
    pub fn start(
        file: &mut McfgRam,
        peer: u32,
        request: &[u8],
        out: &mut [u8],
    ) -> Result<(Self, usize), Error> {
        decode_write_request(request)?;
        if out.len() < WRITE_OACK.len() {
            return Err(Error::OutputTooSmall);
        }
        file.begin(peer)?;
        out[..WRITE_OACK.len()].copy_from_slice(WRITE_OACK);
        Ok((
            Self {
                peer,
                done: false,
                final_block: 0,
                final_data: [0; 512],
                final_len: 0,
            },
            WRITE_OACK.len(),
        ))
    }
    pub fn complete(&self) -> bool {
        self.done
    }

    pub fn cancel(&mut self, file: &mut McfgRam) -> Result<(), Error> {
        if !self.done {
            file.abort(self.peer)?;
            self.done = true;
        }
        Ok(())
    }

    pub fn data(
        &mut self,
        file: &mut McfgRam,
        peer: u32,
        packet: &[u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        if peer != self.peer {
            return Err(Error::NoWriter);
        }
        if packet.len() < 4 || packet[..2] != [0, 3] || packet.len() > 516 {
            return Err(Error::BadRequest);
        }
        if out.len() < 4 {
            return Err(Error::OutputTooSmall);
        }
        let block = u16::from_be_bytes([packet[2], packet[3]]);
        let data = &packet[4..];
        if self.done {
            if self.final_block == 0
                || block != self.final_block
                || data.len() != self.final_len
                || self.final_data[..self.final_len] != *data
            {
                return Err(Error::ChangedRetry);
            }
        } else {
            file.write_block(peer, block, data)?;
            if data.len() < 512 {
                file.finish(peer)?;
                self.done = true;
                self.final_block = block;
                self.final_len = data.len();
                self.final_data[..data.len()].copy_from_slice(data);
            }
        }
        out[..4].copy_from_slice(&[0, 4, packet[2], packet[3]]);
        Ok(4)
    }
}
