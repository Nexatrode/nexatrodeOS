//! Bounded read-only snapshot of Sunfish Apps DRV2 TCS programming.
//!
//! Layout pinned to the observed config and final DT: active0-1, sleep2-4,
//! wake5-7, 16 commands per bank. Never reads the hidden/control TCS, an IRQ
//! clear register, or a disabled command's stale payload. A stable snapshot
//! is an observation, not an atomic capture, ownership grant, complete vote
//! inventory, or safe rollback value for a resource.

pub const OBSERVED_CONFIG: u32 = 0x8000_8104;
pub const TCS_COUNT: usize = 8;
pub const COMMAND_COUNT: usize = 16;
pub const MAPPING_BYTES: usize = 0x3000;
const DRV_BASE: usize = 0xd00;
const TCS_STRIDE: usize = 0x2a0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Register(usize);

impl Register {
    pub const CONFIG: Self = Self(0x0c);
    pub const IRQ_ENABLE: Self = Self(DRV_BASE);
    pub const IRQ_STATUS: Self = Self(DRV_BASE + 4);

    pub const fn bank(bank: usize, field: BankField) -> Option<Self> {
        if bank < TCS_COUNT {
            Some(Self(DRV_BASE + bank * TCS_STRIDE + field as usize))
        } else {
            None
        }
    }

    pub const fn command(bank: usize, command: usize, field: CommandField) -> Option<Self> {
        if bank < TCS_COUNT && command < COMMAND_COUNT {
            Some(Self(
                DRV_BASE + bank * TCS_STRIDE + command * 20 + field as usize,
            ))
        } else {
            None
        }
    }

    pub const fn offset(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BankField {
    Wait = 0x10,
    Control = 0x14,
    Idle = 0x18,
    Enable = 0x1c,
}

#[derive(Clone, Copy, Debug)]
pub enum CommandField {
    Message = 0x30,
    Address = 0x34,
    Data = 0x38,
}

/// Deliberately has no write operation.
pub trait ReadRegisters {
    fn read(&mut self, register: Register) -> u32;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Command {
    pub message: u32,
    pub address: u32,
    pub data: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Bank {
    pub wait: u32,
    pub control: u32,
    pub idle: u32,
    pub enable: u32,
    pub commands: [Option<Command>; COMMAND_COUNT],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    pub config: u32,
    pub irq_enable: u32,
    pub irq_status: u32,
    pub banks: [Bank; TCS_COUNT],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Geometry,
    EnableMask,
    Unstable,
    WaitMask,
    Attempts,
}

impl Error {
    pub const fn code(self) -> u32 {
        match self {
            Self::Geometry => 1,
            Self::EnableMask => 2,
            Self::Unstable => 3,
            Self::WaitMask => 4,
            Self::Attempts => 5,
        }
    }
}

fn capture(io: &mut impl ReadRegisters) -> Result<Snapshot, Error> {
    let config = io.read(Register::CONFIG);
    if config != OBSERVED_CONFIG {
        return Err(Error::Geometry);
    }
    let mut snapshot = Snapshot {
        config,
        irq_enable: io.read(Register::IRQ_ENABLE),
        irq_status: io.read(Register::IRQ_STATUS),
        ..Snapshot::default()
    };
    for (index, bank) in snapshot.banks.iter_mut().enumerate() {
        let mut read = |field| io.read(Register::bank(index, field).unwrap());
        bank.enable = read(BankField::Enable);
        if bank.enable & !0xffff != 0 {
            return Err(Error::EnableMask);
        }
        bank.wait = read(BankField::Wait);
        if bank.wait & !0xffff != 0 {
            return Err(Error::WaitMask);
        }
        bank.control = read(BankField::Control);
        bank.idle = read(BankField::Idle);
        for (command, value) in bank.commands.iter_mut().enumerate() {
            if bank.enable & (1 << command) == 0 {
                continue;
            }
            let mut read = |field| io.read(Register::command(index, command, field).unwrap());
            *value = Some(Command {
                message: read(CommandField::Message),
                address: read(CommandField::Address),
                data: read(CommandField::Data),
            });
        }
    }
    Ok(snapshot)
}

/// Require two consecutive equal captures, with a small explicit work bound.
/// Changed geometry or masks fail immediately; retries only repeat reads.
pub fn capture_stable(io: &mut impl ReadRegisters, attempts: u8) -> Result<Snapshot, Error> {
    if attempts == 0 || attempts > 3 {
        return Err(Error::Attempts);
    }
    for _ in 0..attempts {
        let first = capture(io)?;
        let second = capture(io)?;
        if first == second {
            return Ok(second);
        }
    }
    Err(Error::Unstable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    struct Memory {
        words: [u32; MAPPING_BYTES / 4],
        reads: Vec<usize>,
        captures: u32,
        change_each_capture: bool,
    }
    impl Memory {
        fn ready() -> Self {
            let mut words = [0; MAPPING_BYTES / 4];
            words[Register::CONFIG.offset() / 4] = OBSERVED_CONFIG;
            Self {
                words,
                reads: Vec::new(),
                captures: 0,
                change_each_capture: false,
            }
        }
        fn set(&mut self, reg: Register, value: u32) {
            self.words[reg.offset() / 4] = value;
        }
    }
    impl ReadRegisters for Memory {
        fn read(&mut self, reg: Register) -> u32 {
            self.reads.push(reg.offset());
            if reg == Register::CONFIG {
                self.captures += 1;
                if self.change_each_capture {
                    self.set(
                        Register::command(7, 15, CommandField::Data).unwrap(),
                        self.captures,
                    );
                }
            }
            self.words[reg.offset() / 4]
        }
    }

    #[test]
    fn exact_geometry_is_required_before_any_bank_read() {
        let mut io = Memory::ready();
        io.set(Register::CONFIG, OBSERVED_CONFIG ^ 0x1000);
        assert_eq!(capture_stable(&mut io, 3), Err(Error::Geometry));
        assert_eq!(io.reads, [0x0c]);
    }

    #[test]
    fn bank_and_command_indices_cannot_escape_fixed_window() {
        assert_eq!(Register::bank(8, BankField::Enable), None);
        assert_eq!(Register::bank(usize::MAX, BankField::Enable), None);
        assert_eq!(Register::command(7, 16, CommandField::Data), None);
        assert_eq!(Register::command(8, 0, CommandField::Data), None);
        assert_eq!(
            Register::command(usize::MAX, usize::MAX, CommandField::Data),
            None
        );
        assert_eq!(
            Register::command(7, 15, CommandField::Data)
                .unwrap()
                .offset(),
            0x20c4
        );
    }

    #[test]
    fn empty_banks_never_read_stale_commands_or_clearing_registers() {
        let mut io = Memory::ready();
        assert!(capture_stable(&mut io, 1).is_ok());
        assert_eq!(io.reads.len(), 70); // Two times (config, IRQ pair, 8*4).
        for offset in io.reads {
            assert_ne!(offset, 0xd08);
            assert!(
                offset == 0x0c
                    || offset == 0xd00
                    || offset == 0xd04
                    || (0..8).any(|bank| [0x10, 0x14, 0x18, 0x1c]
                        .contains(&(offset.wrapping_sub(0xd00 + bank * 0x2a0))))
            );
        }
    }

    #[test]
    fn last_enabled_command_and_unknown_payloads_are_preserved_verbatim() {
        let mut io = Memory::ready();
        io.set(Register::bank(7, BankField::Enable).unwrap(), 1 << 15);
        io.set(Register::bank(7, BankField::Wait).unwrap(), 1 << 15);
        for (field, value) in [
            (CommandField::Message, 0x10108),
            (CommandField::Address, 0xdeadbeef),
            (CommandField::Data, 0xffffffff),
        ] {
            io.set(Register::command(7, 15, field).unwrap(), value);
        }
        let result = capture_stable(&mut io, 1).unwrap();
        assert_eq!(
            result.banks[7].commands[15],
            Some(Command {
                message: 0x10108,
                address: 0xdeadbeef,
                data: 0xffffffff
            })
        );
        assert_eq!(result.banks[7].commands[..15], [None; 15]);
        assert_eq!(io.reads.len(), 76);
        assert_eq!(io.reads.iter().copied().max(), Some(0x20c4));
    }

    #[test]
    fn out_of_geometry_masks_are_errors_before_commands_are_read() {
        for (field, expected) in [
            (BankField::Enable, Error::EnableMask),
            (BankField::Wait, Error::WaitMask),
        ] {
            let mut io = Memory::ready();
            io.set(Register::bank(0, field).unwrap(), 1 << 16);
            assert_eq!(capture_stable(&mut io, 1), Err(expected));
            assert!(!io.reads.contains(&0xd30));
        }
    }

    #[test]
    fn changing_enabled_payload_is_unstable_and_read_work_is_bounded() {
        let mut io = Memory::ready();
        io.set(Register::bank(7, BankField::Enable).unwrap(), 1 << 15);
        io.change_each_capture = true;
        assert_eq!(capture_stable(&mut io, 3), Err(Error::Unstable));
        assert_eq!(io.captures, 6);
        assert_eq!(io.reads.len(), 228);
    }

    #[test]
    fn work_limit_is_validated_before_any_read() {
        let mut io = Memory::ready();
        for attempts in [0, 4, 255] {
            assert_eq!(capture_stable(&mut io, attempts), Err(Error::Attempts));
        }
        assert!(io.reads.is_empty());
    }
}
