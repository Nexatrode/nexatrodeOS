#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, rc::Rc};
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use personal_os_slint_notes_spike::NotesWindow;
use sel4_microkit::{Channel, ChannelSet, Handler, Infallible, MessageInfo,
    debug_println, memory_region_symbol, protection_domain, set_mr};
use slint::{
    ComponentHandle, LogicalPosition, PhysicalSize,
    platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent,
        software_renderer::{LineBufferProvider, MinimalSoftwareWindow, RepaintBufferType}},
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
        if before & 1 != 0 { continue; }
        let len = unsafe { core::ptr::addr_of!((*ptr).len).read_volatile() } as usize;
        if len > 128 { break; }
        let status = unsafe { core::ptr::addr_of!((*ptr).status).read_volatile() };
        let mut text = [0u8; 128];
        for (index, byte) in text[..len].iter_mut().enumerate() {
            *byte = unsafe { core::ptr::addr_of!((*ptr).text[index]).read_volatile() };
        }
        core::sync::atomic::fence(Ordering::Acquire);
        let after = unsafe { core::ptr::addr_of!((*ptr).revision).read_volatile() };
        if before != after { continue; }
        let Ok(value) = core::str::from_utf8(&text[..len]) else { break; };
        ui.set_note(value.into());
        ui.set_status(match status {
            0 => "VOLATILE · NOT SAVED",
            1 => "SAVED IN VOLATILE VAULT",
            _ => "VAULT WRITE FAILED",
        }.into());
        return;
    }
    ui.set_status("NOTES VIEW ERROR".into());
}

fn key_code(key: &str) -> Option<u64> {
    match key {
        "DELETE" => Some(0x100),
        "SPACE" => Some(0x101),
        "ENTER" => Some(0x102),
        _ if key.len() == 1 && (key.as_bytes()[0].is_ascii_uppercase()
            || key.as_bytes()[0].is_ascii_digit()) => Some(key.as_bytes()[0] as u64),
        _ => None,
    }
}

#[cfg(feature = "pixel-notes-smoke")]
fn smoke_notes(view: NonNull<NotesView>) {
    set_mr(0, b'A' as u64);
    let key = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_KEY, 1));
    let ptr = view.as_ptr();
    let typed = key.label() == NOTES_OK && unsafe {
        core::ptr::addr_of!((*ptr).len).read_volatile() == 1 &&
        core::ptr::addr_of!((*ptr).text[0]).read_volatile() == b'A'
    };
    let saved = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_SAVE, 0));
    let stored = saved.label() == NOTES_OK && unsafe {
        core::ptr::addr_of!((*ptr).status).read_volatile() == 1
    };
    set_mr(0, 0x100);
    let delete = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_KEY, 1));
    let cleared = delete.label() == NOTES_OK && unsafe {
        core::ptr::addr_of!((*ptr).len).read_volatile() == 0 &&
        core::ptr::addr_of!((*ptr).status).read_volatile() == 0
    };
    if typed && stored && cleared { debug_println!("SUNFISH NOTES IPC SMOKE PASS"); }
    else { debug_println!("SUNFISH NOTES IPC SMOKE FAIL {} {} {}", typed, stored, cleared); }
}

static HEAP_BASE: AtomicUsize = AtomicUsize::new(0);
static HEAP_USED: AtomicUsize = AtomicUsize::new(0);

struct BootHeap;

unsafe impl GlobalAlloc for BootHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = HEAP_BASE.load(Ordering::Acquire);
        if base == 0 { return core::ptr::null_mut(); }
        let align = layout.align();
        let size = layout.size().max(1);
        let mut old = HEAP_USED.load(Ordering::Relaxed);
        loop {
            let Some(start) = base.checked_add(old)
                .and_then(|addr| addr.checked_add(align - 1))
                .map(|addr| addr & !(align - 1))
                .and_then(|addr| addr.checked_sub(base)) else {
                return core::ptr::null_mut();
            };
            let Some(end) = start.checked_add(size) else {
                return core::ptr::null_mut();
            };
            if end > HEAP_BYTES { return core::ptr::null_mut(); }
            match HEAP_USED.compare_exchange_weak(old, end, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return (base + start) as *mut u8,
                Err(current) => old = current,
            }
        }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BootHeap = BootHeap;

struct PixelPlatform { window: Rc<MinimalSoftwareWindow> }

impl Platform for PixelPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration { Duration::ZERO }
}

struct FrameLines { framebuffer: NonNull<u32> }

impl LineBufferProvider for FrameLines {
    type TargetPixel = slint::Rgb8Pixel;

    fn process_line(&mut self, line: usize, range: core::ops::Range<usize>,
                    render_fn: impl FnOnce(&mut [Self::TargetPixel])) {
        if line >= HEIGHT || range.end > WIDTH { return; }
        let mut pixels = [slint::Rgb8Pixel::default(); WIDTH];
        render_fn(&mut pixels[..range.len()]);
        for (column, pixel) in range.zip(pixels.iter()) {
            // Qualcomm continuous-splash memory uses 32-bit XRGB pixels.
            let color = ((pixel.r as u32) << 16) | ((pixel.g as u32) << 8) | pixel.b as u32;
            let index = line * WIDTH + column;
            unsafe {
                self.framebuffer.as_ptr().add(index).write(color);
                self.framebuffer.as_ptr().add(FRAME_PIXELS + index).write(color);
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
    touch_read_sequence: u32,
}

#[protection_domain]
fn init() -> PixelService {
    let heap = memory_region_symbol!(slint_heap: *mut u8);
    HEAP_BASE.store(heap.as_ptr() as usize, Ordering::Release);
    let framebuffer = memory_region_symbol!(slint_framebuffer: *mut u32);
    let touch = memory_region_symbol!(slint_touch_state: *mut TouchState);
    let notes_view = memory_region_symbol!(slint_notes_view: *mut NotesView);
    #[cfg(feature = "pixel-notes-smoke")]
    smoke_notes(notes_view);
    debug_println!("SUNFISH SLINT START");

    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    slint::platform::set_platform(Box::new(PixelPlatform { window: window.clone() }))
        .expect("set Pixel platform");
    window.window().dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: SCALE });
    window.set_size(PhysicalSize::new(WIDTH as u32, HEIGHT as u32));
    let ui = NotesWindow::new().expect("create Slint shell");
    // The Pixel 4a punch-hole camera covers the unshifted top-left labels.
    ui.set_top_inset(45.0);
    personal_os_slint_notes_spike::suite::wire_with_notes(&ui,
        move |ui, key| {
            if let Some(code) = key_code(key) {
                set_mr(0, code);
                let reply = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_KEY, 1));
                if reply.label() == NOTES_OK { refresh_notes(ui, notes_view); }
                else { ui.set_status("NOTES SERVICE ERROR".into()); }
            }
        },
        move |ui| {
            let reply = NOTES_CHANNEL.pp_call(MessageInfo::new(NOTES_SAVE, 0));
            if reply.label() == NOTES_OK { refresh_notes(ui, notes_view); }
            else { ui.set_status("NOTES SERVICE ERROR".into()); }
        });
    refresh_notes(&ui, notes_view);
    ui.show().expect("show Slint shell");

    window.draw_if_needed(|renderer| { renderer.render_by_line(FrameLines { framebuffer }); });
    core::sync::atomic::fence(Ordering::SeqCst);
    debug_println!("SUNFISH SLINT FRAME READY");
    PixelService { _ui: ui, window, framebuffer, touch, touch_read_sequence: 0 }
}

impl PixelService {
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
                if before != sequence.wrapping_mul(2) { continue; }
                let x = unsafe { core::ptr::addr_of!((*entry).x).read_volatile() };
                let y = unsafe { core::ptr::addr_of!((*entry).y).read_volatile() };
                core::sync::atomic::fence(Ordering::Acquire);
                let after = unsafe { core::ptr::addr_of!((*entry).revision).read_volatile() };
                if before == after { sample = Some((x, y)); break; }
            }
            let Some((x, y)) = sample else { break; };
            self.touch_read_sequence = sequence;
            let position = LogicalPosition::new(x as f32 / SCALE, y as f32 / SCALE);
            // The current UI consists of tap targets. Complete the click at
            // finger-down so a delayed release cannot hold the interface.
            self.window.window().dispatch_event(WindowEvent::PointerPressed {
                position, button: PointerEventButton::Left,
            });
            self.window.window().dispatch_event(WindowEvent::PointerReleased {
                position, button: PointerEventButton::Left,
            });
            debug_println!("SUNFISH SLINT TOUCH {} {}", x, y);
            self.window.draw_if_needed(|renderer| {
                renderer.render_by_line(FrameLines { framebuffer: self.framebuffer });
            });
        }
    }
}

impl Handler for PixelService {
    type Error = Infallible;
    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        self.drain_touch_queue();
        Ok(())
    }
}
