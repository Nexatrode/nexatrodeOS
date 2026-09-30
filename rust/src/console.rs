#![no_std]
#![no_main]

use core::{ptr, ptr::NonNull};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageLabel, MessageRegisterValue,
    debug_print, debug_println, get_mr, memory_region_symbol, protection_domain, set_mr,
};

const UART_IRQ: Channel = Channel::new(2);
const TIMER_TO_CONSOLE: Channel = Channel::new(1);
const CLIENT_TO_CONSOLE: Channel = Channel::new(3);
const CLIENT_CALL: Channel = Channel::new(0);
const CLOCK_CALL: Channel = Channel::new(4);
const START_CRASHER: Channel = Channel::new(5);

const REQUEST_CONSOLE_CALC: MessageLabel = 0x201;
const REQUEST_CONSOLE_STATUS: MessageLabel = 0x202;
const REQUEST_CONSOLE_CRASH: MessageLabel = 0x203;
const REQUEST_CONSOLE_NETPEER: MessageLabel = 0x205;
const REQUEST_CONSOLE_NETSEAL: MessageLabel = 0x206;
const REQUEST_CONSOLE_MODEMRING: MessageLabel = 0x207;
const REQUEST_CONSOLE_MODEMSMS: MessageLabel = 0x208;
const REQUEST_CLOCK_STATUS: MessageLabel = 0x204;
const REPLY_OK: MessageLabel = 0x100;
const REPLY_OVERFLOW: MessageLabel = 0x102;
const REPLY_NO_NET: MessageLabel = 0x412;
const MODEM_SMS_MAX: usize = 16;

const UART_DR: usize = 0x00;
const UART_FR: usize = 0x18;
const UART_IBRD: usize = 0x24;
const UART_FBRD: usize = 0x28;
const UART_LCRH: usize = 0x2c;
const UART_CR: usize = 0x30;
const UART_IMSC: usize = 0x38;
const UART_ICR: usize = 0x44;

const UART_FR_RXFE: u32 = 1 << 4;
const UART_INT_RX: u32 = 1 << 4;
const UART_INT_RT: u32 = 1 << 6;
const UART_INT_RX_ALL: u32 = UART_INT_RX | UART_INT_RT;
const LINE_CAPACITY: usize = 64;

#[protection_domain]
fn init() -> Console {
    let mut console = Console {
        uart: memory_region_symbol!(uart_regs: *mut u32),
        line: [0; LINE_CAPACITY],
        line_length: 0,
        heartbeat_bucket: 0,
        line_too_long: false,
        ready: false,
    };

    // QEMU virt's PL011 UART0 uses a 24 MHz input clock.
    console.write_uart(UART_CR, 0);
    console.write_uart(UART_IBRD, 13);
    console.write_uart(UART_FBRD, 1);
    console.write_uart(UART_LCRH, 0x70); // FIFO, 8 data bits, no parity.
    console.write_uart(UART_ICR, 0x7ff);
    console.write_uart(UART_IMSC, UART_INT_RX_ALL);
    console.write_uart(UART_CR, 0x301); // UART, transmitter, receiver.
    console
}

struct Console {
    uart: NonNull<u32>,
    line: [u8; LINE_CAPACITY],
    line_length: usize,
    heartbeat_bucket: MessageRegisterValue,
    line_too_long: bool,
    ready: bool,
}

impl Console {
    fn read_uart(&self, offset: usize) -> u32 {
        // This address comes from the PL011 memory region mapped by Microkit.
        unsafe { ptr::read_volatile(self.uart.as_ptr().add(offset / 4)) }
    }

    fn write_uart(&mut self, offset: usize, value: u32) {
        // Device registers require volatile writes to preserve MMIO ordering.
        unsafe { ptr::write_volatile(self.uart.as_ptr().add(offset / 4), value) }
    }

    fn prompt(&self) {
        debug_print!("> ");
        if self.line_length != 0 {
            let input = core::str::from_utf8(&self.line[..self.line_length]).unwrap_or("");
            debug_print!("{}", input);
        }
    }

    fn print_reply_error(reply: MessageInfo) {
        if reply.label() == REPLY_OVERFLOW {
            debug_print!("error: unsigned addition overflow\r\n");
        } else if reply.label() == REPLY_NO_NET {
            debug_print!("error: network not ready (offline image or no DHCP)\r\n");
        } else {
            debug_print!("error: request rejected by service\r\n");
        }
    }

    fn request_netpeer(argument: &[u8]) {
        let mut cursor = 0;
        Self::skip_spaces(argument, &mut cursor);
        let Some(a) = Self::parse_octet(argument, &mut cursor) else {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        };
        if argument.get(cursor) != Some(&b'.') {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        }
        cursor += 1;
        let Some(b) = Self::parse_octet(argument, &mut cursor) else {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        };
        if argument.get(cursor) != Some(&b'.') {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        }
        cursor += 1;
        let Some(c) = Self::parse_octet(argument, &mut cursor) else {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        };
        if argument.get(cursor) != Some(&b'.') {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        }
        cursor += 1;
        let Some(d) = Self::parse_octet(argument, &mut cursor) else {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        };
        if argument.get(cursor) != Some(&b':') {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        }
        cursor += 1;
        let Some(port) = Self::parse_word(argument, &mut cursor) else {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        };
        Self::skip_spaces(argument, &mut cursor);
        if cursor != argument.len() || port == 0 || port > 65535 {
            debug_print!("usage: netpeer A.B.C.D:PORT\r\n");
            return;
        }
        let packed = (a as MessageRegisterValue)
            | ((b as MessageRegisterValue) << 8)
            | ((c as MessageRegisterValue) << 16)
            | ((d as MessageRegisterValue) << 24);
        set_mr(0, packed);
        set_mr(1, port);
        let reply = CLIENT_CALL.pp_call(MessageInfo::new(REQUEST_CONSOLE_NETPEER, 2));
        if reply.label() != REPLY_OK || reply.count() != 0 {
            Self::print_reply_error(reply);
            return;
        }
        debug_print!("net peer locked to {}.{}.{}.{}:{}\r\n", a, b, c, d, port);
    }

    fn request_netseal() {
        let reply = CLIENT_CALL.pp_call(MessageInfo::new(REQUEST_CONSOLE_NETSEAL, 0));
        if reply.label() != REPLY_OK || reply.count() != 0 {
            Self::print_reply_error(reply);
            return;
        }
        debug_print!("sealed demo handed to dhcp_client\r\n");
    }

    fn request_modemring() {
        let reply = CLIENT_CALL.pp_call(MessageInfo::new(REQUEST_CONSOLE_MODEMRING, 0));
        if reply.label() != REPLY_OK || reply.count() != 0 {
            Self::print_reply_error(reply);
            return;
        }
        debug_print!("modem ring injected\r\n");
    }

    fn request_modemsms(argument: &[u8]) {
        let mut cursor = 0;
        Self::skip_spaces(argument, &mut cursor);
        let start = cursor;
        while let Some(byte) = argument.get(cursor).copied() {
            if !(byte.is_ascii_alphanumeric()) {
                break;
            }
            cursor += 1;
        }
        let body = &argument[start..cursor];
        Self::skip_spaces(argument, &mut cursor);
        if body.is_empty() || body.len() > MODEM_SMS_MAX || cursor != argument.len() {
            debug_print!("usage: modemsms BODY  (1-16 alphanumeric)\r\n");
            return;
        }
        let mut words = [0u8; MODEM_SMS_MAX];
        words[..body.len()].copy_from_slice(body);
        set_mr(0, body.len() as MessageRegisterValue);
        set_mr(
            1,
            u64::from_le_bytes(words[..8].try_into().unwrap()) as MessageRegisterValue,
        );
        set_mr(
            2,
            u64::from_le_bytes(words[8..].try_into().unwrap()) as MessageRegisterValue,
        );
        let reply = CLIENT_CALL.pp_call(MessageInfo::new(REQUEST_CONSOLE_MODEMSMS, 3));
        if reply.label() != REPLY_OK || reply.count() != 0 {
            Self::print_reply_error(reply);
            return;
        }
        debug_print!("modem SMS injected\r\n");
    }

    fn parse_octet(input: &[u8], cursor: &mut usize) -> Option<u8> {
        let mut parsed: u32 = 0;
        let mut found_digit = false;
        while let Some(byte @ b'0'..=b'9') = input.get(*cursor).copied() {
            let digit = (byte - b'0') as u32;
            parsed = parsed.checked_mul(10)?.checked_add(digit)?;
            if parsed > 255 {
                return None;
            }
            found_digit = true;
            *cursor += 1;
        }
        found_digit.then_some(parsed as u8)
    }

    fn read_uptime() -> Option<MessageRegisterValue> {
        let reply = CLOCK_CALL.pp_call(MessageInfo::new(REQUEST_CLOCK_STATUS, 0));
        if reply.label() != REPLY_OK || reply.count() != 1 {
            return None;
        }
        Some(get_mr(0))
    }

    fn print_status() {
        let reply = CLIENT_CALL.pp_call(MessageInfo::new(REQUEST_CONSOLE_STATUS, 0));
        if reply.label() != REPLY_OK || reply.count() != 1 {
            Self::print_reply_error(reply);
            return;
        }
        let handled_faults = get_mr(0);

        let Some(seconds) = Self::read_uptime() else {
            debug_print!("error: clock service did not respond\r\n");
            return;
        };

        debug_print!("uptime: {} seconds\r\n", seconds);
        debug_print!("components: console, clock, supervisor, calculator, crasher\r\n");
        debug_print!("isolated faults handled: {}\r\n", handled_faults);
    }

    fn request_crash() {
        let reply = CLIENT_CALL.pp_call(MessageInfo::new(REQUEST_CONSOLE_CRASH, 0));
        if reply.label() != REPLY_OK || reply.count() != 0 {
            Self::print_reply_error(reply);
            return;
        }
        // Finish the console acknowledgement before the child is allowed to
        // fault and emit its own diagnostics on the shared debug channel.
        debug_print!("crasher restarted; watching the supervisor handle its fault\r\n");
        // The supervisor receives this notification only after the protected
        // call has returned, so the child cannot fault over a pending reply.
        START_CRASHER.notify();
    }

    fn run_calculation(expression: &[u8]) {
        let mut cursor = 0;
        Self::skip_spaces(expression, &mut cursor);
        let Some(left) = Self::parse_word(expression, &mut cursor) else {
            debug_print!("usage: calc NUMBER + NUMBER\r\n");
            return;
        };
        Self::skip_spaces(expression, &mut cursor);
        if expression.get(cursor) != Some(&b'+') {
            debug_print!("usage: calc NUMBER + NUMBER\r\n");
            return;
        }
        cursor += 1;
        Self::skip_spaces(expression, &mut cursor);
        let Some(right) = Self::parse_word(expression, &mut cursor) else {
            debug_print!("usage: calc NUMBER + NUMBER\r\n");
            return;
        };
        Self::skip_spaces(expression, &mut cursor);
        if cursor != expression.len() {
            debug_print!("usage: calc NUMBER + NUMBER\r\n");
            return;
        }

        set_mr(0, left);
        set_mr(1, right);
        let reply = CLIENT_CALL.pp_call(MessageInfo::new(REQUEST_CONSOLE_CALC, 2));
        if reply.label() != REPLY_OK || reply.count() != 1 {
            Self::print_reply_error(reply);
            return;
        }
        debug_print!("= {}\r\n", get_mr(0));
    }

    fn skip_spaces(input: &[u8], cursor: &mut usize) {
        while matches!(input.get(*cursor), Some(b' ' | b'\t')) {
            *cursor += 1;
        }
    }

    fn parse_word(input: &[u8], cursor: &mut usize) -> Option<MessageRegisterValue> {
        let mut parsed: MessageRegisterValue = 0;
        let mut found_digit = false;
        while let Some(byte @ b'0'..=b'9') = input.get(*cursor).copied() {
            let digit = (byte - b'0') as MessageRegisterValue;
            parsed = parsed.checked_mul(10)?.checked_add(digit)?;
            found_digit = true;
            *cursor += 1;
        }
        found_digit.then_some(parsed)
    }

    fn execute_line(&self) {
        let input = &self.line[..self.line_length];
        if self.line_too_long {
            debug_print!("error: command is too long\r\n");
        } else if input.is_empty() {
            // An empty command only redraws the prompt.
        } else if input == b"help" {
            debug_print!(
                "help          show commands\r\n\
                 status        show uptime and component state\r\n\
                 calc A + B    add two unsigned integers in the isolated service\r\n\
                 crash         restart the isolated fault demonstration\r\n\
                 netpeer A.B.C.D:PORT  lock Messages sealed-TX peer (merged net image)\r\n\
                 netseal       send the fixed PSL1 demo to the locked/default peer\r\n\
                 modemring     inject a fake incoming call ring\r\n\
                 modemsms BODY inject a fake SMS (1-16 alphanumeric)\r\n"
            );
        } else if input == b"status" {
            Self::print_status();
        } else if input.len() >= 5 && &input[..4] == b"calc" && matches!(input[4], b' ' | b'\t') {
            Self::run_calculation(&input[5..]);
        } else if input == b"crash" {
            Self::request_crash();
        } else if input.len() >= 8
            && &input[..7] == b"netpeer"
            && matches!(input[7], b' ' | b'\t')
        {
            Self::request_netpeer(&input[8..]);
        } else if input == b"netseal" {
            Self::request_netseal();
        } else if input == b"modemring" {
            Self::request_modemring();
        } else if input.len() >= 9
            && &input[..8] == b"modemsms"
            && matches!(input[8], b' ' | b'\t')
        {
            Self::request_modemsms(&input[9..]);
        } else {
            debug_print!("unknown command; enter help\r\n");
        }
    }

    fn receive_character(&mut self, input: u8) {
        if !self.ready {
            return;
        }
        // UTM/serial terminals often send CR alone; QEMU may send LF or CRLF.
        if input == b'\r' || input == b'\n' {
            debug_print!("\r\n");
            self.execute_line();
            self.line_length = 0;
            self.line_too_long = false;
            self.prompt();
            return;
        }
        if input == b'\x08' || input == b'\x7f' {
            if self.line_length != 0 {
                self.line_length -= 1;
                debug_print!("\x08 \x08");
            }
            return;
        }
        if !(0x20..=0x7e).contains(&input) {
            return;
        }
        if self.line_length == LINE_CAPACITY - 1 {
            self.line_too_long = true;
            return;
        }

        self.line[self.line_length] = input;
        self.line_length += 1;
        debug_print!("{}", input as char);
    }

    fn handle_timer(&mut self) {
        if !self.ready {
            return;
        }
        let Some(seconds) = Self::read_uptime() else {
            debug_print!("error: clock service did not respond\r\n");
            self.prompt();
            return;
        };
        if seconds / 5 > self.heartbeat_bucket {
            self.heartbeat_bucket = seconds / 5;
            debug_print!(
                "\r\n[heartbeat: {} seconds; console and clock responded]\r\n",
                seconds
            );
            self.prompt();
        }
    }

    fn handle_uart_irq(&mut self) {
        while self.read_uart(UART_FR) & UART_FR_RXFE == 0 {
            self.receive_character((self.read_uart(UART_DR) & 0xff) as u8);
        }
        // Clear PL011 receive/timeout causes before unmasking the GIC line.
        self.write_uart(UART_ICR, UART_INT_RX_ALL);
        UART_IRQ.irq_ack().unwrap();
    }
}

impl Handler for Console {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        let mut recognized = false;
        if channels.contains(TIMER_TO_CONSOLE) {
            recognized = true;
            self.handle_timer();
        }
        if channels.contains(CLIENT_TO_CONSOLE) {
            recognized = true;
            self.ready = true;
            debug_println!("Personal OS console; enter help for commands.");
            debug_print!("\r\n[boot isolation checks complete; console ready]\r\n");
            self.prompt();
        }
        if channels.contains(UART_IRQ) {
            recognized = true;
            self.handle_uart_irq();
        }
        if channels.iter().any(|channel| {
            channel != TIMER_TO_CONSOLE && channel != CLIENT_TO_CONSOLE && channel != UART_IRQ
        }) || !recognized
        {
            debug_print!("CONSOLE|ERROR: unexpected notification\r\n");
        }
        Ok(())
    }
}
