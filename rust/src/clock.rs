#![no_std]
#![no_main]

mod app_view;

use core::{ptr, ptr::NonNull};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, MessageLabel, MessageRegisterValue,
    debug_println, memory_region_symbol, protection_domain, set_mr,
};
use app_view::AppViewState;
use personal_os_rust_server::clock_display::{UTC_TIME_TEXT_LENGTH, format_utc_hms};

const TIMER_IRQ: Channel = Channel::new(0);
const TIMER_TO_CONSOLE: Channel = Channel::new(1);
const CLOCK_TO_CONSOLE: Channel = Channel::new(2);

const REQUEST_CLOCK_STATUS: MessageLabel = 0x204;
const REPLY_OK: MessageLabel = 0x100;
const REPLY_CONSOLE_ERROR: MessageLabel = 0x2ff;

const RTC_DR: usize = 0x00;
const RTC_MR: usize = 0x04;
const RTC_CR: usize = 0x0c;
const RTC_IMSC: usize = 0x10;
const RTC_MIS: usize = 0x18;
const RTC_ICR: usize = 0x1c;
const RTC_ALARM_BIT: u32 = 1;

#[protection_domain]
fn init() -> Clock {
    let registers = memory_region_symbol!(rtc_regs: *mut u32);
    let clock_view = memory_region_symbol!(clock_view: *mut AppViewState);
    let mut rtc = Rtc {
        registers,
        starting_seconds: 0,
    };

    // The RTC page is mapped only into this domain. Volatile accesses are
    // required for device registers and stay behind this small MMIO wrapper.
    rtc.write(RTC_CR, 1);
    rtc.write(RTC_ICR, RTC_ALARM_BIT);
    rtc.write(RTC_IMSC, RTC_ALARM_BIT);
    rtc.starting_seconds = rtc.read(RTC_DR);
    rtc.arm_next_second();
    unsafe { clock_view.as_ptr().write(AppViewState::new()) };
    let mut clock = Clock { rtc, clock_view };
    clock.publish_time();
    clock
}

struct Rtc {
    registers: NonNull<u32>,
    starting_seconds: u32,
}

impl Rtc {
    fn read(&self, offset: usize) -> u32 {
        // `registers` comes from Microkit's mapped PL031 MMIO region.
        unsafe { ptr::read_volatile(self.registers.as_ptr().add(offset / 4)) }
    }

    fn write(&mut self, offset: usize, value: u32) {
        // `registers` comes from Microkit's mapped PL031 MMIO region.
        unsafe { ptr::write_volatile(self.registers.as_ptr().add(offset / 4), value) }
    }

    fn arm_next_second(&mut self) {
        self.write(RTC_MR, self.read(RTC_DR).wrapping_add(1));
    }
}

struct Clock {
    rtc: Rtc,
    clock_view: NonNull<AppViewState>,
}

impl Clock {
    fn publish_time(&mut self) {
        let mut text = [0; UTC_TIME_TEXT_LENGTH];
        format_utc_hms(self.rtc.read(RTC_DR), &mut text);
        if !unsafe { self.clock_view.as_mut() }.set_display_text(&text) {
            debug_println!("CLOCK|ERROR: invalid time display snapshot");
        }
    }
}

impl Handler for Clock {
    type Error = Infallible;

    fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error> {
        if !channels.contains(TIMER_IRQ) {
            debug_println!("CLOCK|ERROR: unexpected notification");
            return Ok(());
        }

        if self.rtc.read(RTC_MIS) & RTC_ALARM_BIT != 0 {
            // Clear and re-arm the level interrupt before notifying the console.
            self.rtc.write(RTC_ICR, RTC_ALARM_BIT);
            self.rtc.arm_next_second();
            self.publish_time();
            TIMER_TO_CONSOLE.notify();
        }
        TIMER_IRQ.irq_ack().unwrap();
        Ok(())
    }

    fn protected(
        &mut self,
        channel: Channel,
        request: MessageInfo,
    ) -> Result<MessageInfo, Self::Error> {
        if channel != CLOCK_TO_CONSOLE
            || request.label() != REQUEST_CLOCK_STATUS
            || request.count() != 0
        {
            return Ok(MessageInfo::new(REPLY_CONSOLE_ERROR, 0));
        }

        let elapsed = self
            .rtc
            .read(RTC_DR)
            .wrapping_sub(self.rtc.starting_seconds);
        set_mr(0, elapsed as MessageRegisterValue);
        Ok(MessageInfo::new(REPLY_OK, 1))
    }
}
