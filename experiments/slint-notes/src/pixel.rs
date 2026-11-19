#![no_std]
#![no_main]

extern crate alloc;

use alloc::rc::Rc;
use core::{
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering},
};
use personal_os_slint_notes_spike::{NotesWindow, guest_platform};
use sel4_microkit::{
    Channel, ChannelSet, Handler, Infallible, MessageInfo, debug_println, memory_region_symbol,
    protection_domain, set_mr,
};
use slint::{
    ComponentHandle, LogicalPosition, PhysicalSize,
    platform::{
        PointerEventButton, WindowAdapter, WindowEvent,
        software_renderer::{LineBufferProvider, MinimalSoftwareWindow, RepaintBufferType},
    },
};

const WIDTH: usize = 1080;
const HEIGHT: usize = 2340;
const FRAME_PIXELS: usize = WIDTH * HEIGHT;
const HEAP_BYTES: usize = 0x800000;
const SCALE: f32 = 2.6086957;
const NOTES_CHANNEL: Channel = Channel::new(1);
const NOTES_KEY: u64 = 0x501;
const NOTES_SAVE: u64 = 0x502;
const NOTES_OK: u64 = 0x500;

#[repr(C)]
struct NotesView {
    revision: u32,
    len: u32,
    status: u32,
    text: [u8; 128],
}

fn refresh_notes(ui: &NotesWindow, view: NonNull<NotesView>) {
    let ptr = view.as_ptr();
    for _ in 0..4 {
        let before = unsafe { core::ptr::addr_of!((*ptr).revision).read_volatile() };
        if before & 1 != 0 {
            continue;
        }
        let len = unsafe { core::ptr::addr_of!((*ptr).len).read_volatile() } as usize;
        if len > 128 {
            break;
        }
        let status = unsafe { core::ptr::addr_of!((*ptr).status).read_volatile() };
        let mut text = [0u8; 128];
        for (index, byte) in text[..len].iter_mut().enumerate() {
            *byte = unsafe { core::ptr::addr_of!((*ptr).text[index]).read_volatile() };
        }
        core::sync::atomic::fence(Ordering::Acquire);
        let after = unsafe { core::ptr::addr_of!((*ptr).revision).read_volatile() };
        if before != after {
            continue;
        }
        let Ok(value) = core::str::from_utf8(&text[..len]) else {
            break;
        };
        ui.set_note(value.into());
        ui.set_status(
            match status {
                0 => "VOLATILE · NOT SAVED",
                1 => "SAVED IN VOLATILE VAULT",
                _ => "VAULT WRITE FAILED",
            }
            .into(),
        );
        return;
    }
    ui.set_status("NOTES VIEW ERROR".into());
}

fn key_code(key: &str) -> Option<u64> {
    match key {
        "DELETE" => Some(0x100),
        "SPACE" => Some(0x101),
        "ENTER" => Some(0x102),
        _ if key.len() == 1
            && (key.as_bytes()[0].is_ascii_uppercase() || key.as_bytes()[0].is_ascii_digit()) =>
        {
            Some(key.as_bytes()[0] as u64)
        }
        _ => None,
    }
}

#[cfg(feature = "pixel-notes-smoke")]
fn smoke_notes(view: NonNull<NotesView>) {
    set_mr(0, b'A' as u64);
    let key = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_KEY, 1));
    let ptr = view.as_ptr();
    let typed = key.label() == NOTES_OK
        && unsafe {
            core::ptr::addr_of!((*ptr).len).read_volatile() == 1
                && core::ptr::addr_of!((*ptr).text[0]).read_volatile() == b'A'
        };
    let saved = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_SAVE, 0));
    let stored = saved.label() == NOTES_OK
        && unsafe { core::ptr::addr_of!((*ptr).status).read_volatile() == 1 };
    set_mr(0, 0x100);
    let delete = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_KEY, 1));
    let cleared = delete.label() == NOTES_OK
        && unsafe {
            core::ptr::addr_of!((*ptr).len).read_volatile() == 0
                && core::ptr::addr_of!((*ptr).status).read_volatile() == 0
        };
    if typed && stored && cleared {
        debug_println!("SUNFISH NOTES IPC SMOKE PASS");
    } else {
        debug_println!(
            "SUNFISH NOTES IPC SMOKE FAIL {} {} {}",
            typed,
            stored,
            cleared
        );
    }
}

#[cfg(feature = "pixel-notes-smoke")]
fn smoke_heap_reuse() {
    // 40 MiB in total must fit in the 8 MiB region by reusing each freed block.
    for iteration in 0..160 {
        let mut temporary = alloc::vec::Vec::<u8>::new();
        if temporary.try_reserve_exact(256 * 1024).is_err() {
            debug_println!("SUNFISH SLINT ALLOC REUSE FAIL at iteration {}", iteration);
            return;
        }
        temporary.push(1);
        core::hint::black_box(&temporary);
    }
    debug_println!("SUNFISH SLINT ALLOC REUSE PASS");
}

#[global_allocator]
static HEAP: sel4_dlmalloc::DeferredStaticDlmalloc<sel4_microkit::_private::heap::RawOneShotMutex> =
    sel4_dlmalloc::DeferredStaticDlmalloc::new();

struct FrameLines {
    framebuffer: NonNull<u32>,
}

impl LineBufferProvider for FrameLines {
    type TargetPixel = slint::Rgb8Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        if line >= HEIGHT || range.end > WIDTH {
            return;
        }
        let mut pixels = [slint::Rgb8Pixel::default(); WIDTH];
        render_fn(&mut pixels[..range.len()]);
        for (column, pixel) in range.zip(pixels.iter()) {
            // Qualcomm continuous-splash memory uses 32-bit XRGB pixels.
            let color = ((pixel.r as u32) << 16) | ((pixel.g as u32) << 8) | pixel.b as u32;
            let index = line * WIDTH + column;
            unsafe {
                self.framebuffer.as_ptr().add(index).write(color);
                self.framebuffer
                    .as_ptr()
                    .add(FRAME_PIXELS + index)
                    .write(color);
            }
        }
    }
}

#[repr(C)]
struct TouchEvent {
    revision: u32,
    x: u32,
    y: u32,
}

#[repr(C)]
struct TouchState {
    event_write: u32,
    events: [TouchEvent; 256],
}

struct PixelService {
    _ui: NotesWindow,
    window: Rc<MinimalSoftwareWindow>,
    framebuffer: NonNull<u32>,
    touch: NonNull<TouchState>,
    frame_wake: Option<NonNull<AtomicU64>>,
    touch_read_sequence: u32,
}

/// Wallet controls in the same logical space as the 1080×2340 framebuffer.
/// `top-inset` is 45 and the wallet page starts 88 below that.
fn apply_wallet_tap(ui: &NotesWindow, x: f32, y: f32) {
    if ui.get_page() != 16 {
        return;
    }
    let w = WIDTH as f32 / SCALE;
    let h = HEIGHT as f32 / SCALE;
    let origin_y = 45.0 + 88.0;
    let nav_top = h - 64.0;
    let send_keyboard =
        ui.get_wallet_tab() == 2 && (ui.get_edit_target() == 11 || ui.get_edit_target() == 12);
    if !send_keyboard && y >= nav_top && y < h && w > 0.0 {
        let tab = ((x / w) * 4.0) as i32;
        if (0..4).contains(&tab) {
            ui.set_wallet_tab(tab);
            ui.set_edit_target(0);
            debug_println!("SUNFISH WALLET TAB {}", tab);
        }
        return;
    }
    if ui.get_wallet_tab() != 0 {
        return;
    }
    let button_w = (w - 80.0) / 2.0;
    let top = origin_y + 8.0 + 102.0;
    if button_w <= 0.0 || !(top..top + 36.0).contains(&y) {
        return;
    }
    let send_left = 34.0;
    let receive_left = 46.0 + button_w;
    if (send_left..send_left + button_w).contains(&x) {
        ui.set_wallet_tab(2);
        ui.set_edit_target(0);
        debug_println!("SUNFISH WALLET HIT send");
    } else if (receive_left..receive_left + button_w).contains(&x) {
        ui.set_wallet_tab(1);
        ui.set_edit_target(0);
        debug_println!("SUNFISH WALLET HIT receive");
    }
}

#[protection_domain]
fn init() -> PixelService {
    let heap = memory_region_symbol!(slint_heap: *mut u8);
    HEAP.set_bounds(sel4_dlmalloc::StaticHeapBounds::new(heap.as_ptr(), HEAP_BYTES))
        .expect("Slint heap initialized once");
    #[cfg(feature = "pixel-notes-smoke")]
    smoke_heap_reuse();
    let framebuffer = memory_region_symbol!(slint_framebuffer: *mut u32);
    let touch = memory_region_symbol!(slint_touch_state: *mut TouchState);
    let frame_wake =
        NonNull::new(*sel4_microkit::var!(slint_frame_wake_request: usize = 0) as *mut AtomicU64);
    let notes_view = memory_region_symbol!(slint_notes_view: *mut NotesView);
    #[cfg(feature = "pixel-notes-smoke")]
    smoke_notes(notes_view);
    debug_println!("SUNFISH SLINT START");

    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    guest_platform::install(&window);
    window
        .window()
        .dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: SCALE,
        });
    window.set_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    let ui = NotesWindow::new().expect("create Slint shell");
    // The Pixel 4a punch-hole camera covers the unshifted top-left labels.
    ui.set_top_inset(45.0);
    personal_os_slint_notes_spike::suite::wire_with_notes(
        &ui,
        move |ui, key| {
            if let Some(code) = key_code(key) {
                set_mr(0, code);
                let reply = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_KEY, 1));
                if reply.label() == NOTES_OK {
                    refresh_notes(ui, notes_view);
                } else {
                    ui.set_status("NOTES SERVICE ERROR".into());
                }
            }
        },
        move |ui| {
            let reply = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_SAVE, 0));
            if reply.label() == NOTES_OK {
                refresh_notes(ui, notes_view);
            } else {
                ui.set_status("NOTES SERVICE ERROR".into());
            }
        },
    );
    refresh_notes(&ui, notes_view);
    // Same NexaWal screen the UTM guest opens. Notes stay available from Home.
    ui.set_page(16);
    ui.show().expect("show Slint shell");

    let service = PixelService {
        _ui: ui,
        window,
        framebuffer,
        touch,
        frame_wake,
        touch_read_sequence: 0,
    };
    #[cfg(feature = "pixel-notes-smoke")]
    slint::Timer::single_shot(core::time::Duration::from_millis(50), || {
        debug_println!("SUNFISH SLINT TIMER WAKE PASS");
    });
    service.draw_and_arm();
    debug_println!("SUNFISH SLINT FRAME READY");
    service
}

impl PixelService {
    fn draw_and_arm(&self) {
        if self.window.draw_if_needed(|renderer| {
            renderer.render_by_line(FrameLines {
                framebuffer: self.framebuffer,
            });
        }) {
            core::sync::atomic::fence(Ordering::SeqCst);
        }
        // The board maps the UI-owned, aligned request page for the PD's lifetime.
        unsafe { guest_platform::arm_phone_frame(&self.window, self.frame_wake) };
    }

    fn drain_touch_queue(&mut self) {
        let ptr = self.touch.as_ptr();
        let latest = unsafe { core::ptr::addr_of!((*ptr).event_write).read_volatile() };
        if latest.wrapping_sub(self.touch_read_sequence) > 256 {
            self.touch_read_sequence = latest.wrapping_sub(256);
        }
        while self.touch_read_sequence != latest {
            let sequence = self.touch_read_sequence.wrapping_add(1);
            let entry = unsafe {
                core::ptr::addr_of!((*ptr).events[(sequence.wrapping_sub(1) % 256) as usize])
            };
            let mut sample = None;
            for _ in 0..4 {
                let before = unsafe { core::ptr::addr_of!((*entry).revision).read_volatile() };
                if before != sequence.wrapping_mul(2) {
                    continue;
                }
                let x = unsafe { core::ptr::addr_of!((*entry).x).read_volatile() };
                let y = unsafe { core::ptr::addr_of!((*entry).y).read_volatile() };
                core::sync::atomic::fence(Ordering::Acquire);
                let after = unsafe { core::ptr::addr_of!((*entry).revision).read_volatile() };
                if before == after {
                    sample = Some((x, y));
                    break;
                }
            }
            let Some((x, y)) = sample else {
                break;
            };
            self.touch_read_sequence = sequence;
            let position = LogicalPosition::new(x as f32 / SCALE, y as f32 / SCALE);
            // The current UI consists of tap targets. Complete the click at
            // finger-down so a delayed release cannot hold the interface.
            self.window
                .window()
                .dispatch_event(WindowEvent::PointerPressed {
                    position,
                    button: PointerEventButton::Left,
                });
            self.window
                .window()
                .dispatch_event(WindowEvent::PointerReleased {
                    position,
                    button: PointerEventButton::Left,
                });
            // The wallet buttons are fixed rectangles. A Slint miss leaves
            // the finger with no effect, which is what the Pixel showed.
            apply_wallet_tap(&self._ui, position.x, position.y);
            debug_println!("SUNFISH SLINT TOUCH {} {}", x, y);
        }
    }
}

impl Handler for PixelService {
    type Error = Infallible;
    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        slint::platform::update_timers_and_animations();
        self.drain_touch_queue();
        self.draw_and_arm();
        Ok(())
    }
}
