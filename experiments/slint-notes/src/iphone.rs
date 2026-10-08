#![no_std]
#![no_main]

#[cfg(all(feature = "workspace-ui", feature = "snapshot-boot"))]
compile_error!("the Notes snapshot asset does not match this UI");
#[cfg(all(feature = "full-suite-ui", feature = "snapshot-boot", not(feature = "full-suite-snapshot-boot")))]
compile_error!("use full-suite-snapshot-boot for this UI");
#[cfg(all(feature = "full-suite-half-res", feature = "snapshot-boot"))]
compile_error!("the full-resolution snapshot asset does not match half-resolution rendering");
#[cfg(all(feature = "full-suite-two-thirds", feature = "snapshot-boot"))]
compile_error!("the full-resolution snapshot asset does not match two-thirds rendering");
#[cfg(all(feature = "full-suite-half-res", feature = "full-suite-two-thirds"))]
compile_error!("select only one reduced render resolution");

extern crate alloc;

use alloc::{boxed::Box, rc::Rc};
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use personal_os_slint_notes_spike::NotesWindow;
use sel4_microkit::{
    ChannelSet, Handler, Infallible, debug_println, memory_region_symbol,
    protection_domain,
};
use slint::{
    ComponentHandle, LogicalPosition, PhysicalSize,
    platform::{
        Platform, PointerEventButton, WindowAdapter, WindowEvent,
        software_renderer::{LineBufferProvider, MinimalSoftwareWindow, RepaintBufferType},
    },
};

const WIDTH: usize = 1080;
const HEIGHT: usize = 1920;
#[cfg(feature = "full-suite-half-res")]
const RENDER_WIDTH: usize = WIDTH / 2;
#[cfg(feature = "full-suite-two-thirds")]
const RENDER_WIDTH: usize = WIDTH * 2 / 3;
#[cfg(not(any(feature = "full-suite-half-res", feature = "full-suite-two-thirds")))]
const RENDER_WIDTH: usize = WIDTH;
#[cfg(feature = "full-suite-half-res")]
const RENDER_HEIGHT: usize = HEIGHT / 2;
#[cfg(feature = "full-suite-two-thirds")]
const RENDER_HEIGHT: usize = HEIGHT * 2 / 3;
#[cfg(not(any(feature = "full-suite-half-res", feature = "full-suite-two-thirds")))]
const RENDER_HEIGHT: usize = HEIGHT;
const STRIDE: usize = 0x1100 / 4;
const HEAP_BYTES: usize = 0x400000;
#[cfg(feature = "full-suite-half-res")]
const SCALE: f32 = 1.3043479;
#[cfg(feature = "full-suite-two-thirds")]
const SCALE: f32 = 1.7391305;
#[cfg(not(any(feature = "full-suite-half-res", feature = "full-suite-two-thirds")))]
const SCALE: f32 = 2.6086957;
const TOUCH_SCALE: f32 = 2.6086957;

static HEAP_BASE: AtomicUsize = AtomicUsize::new(0);
static HEAP_USED: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "frame-timing")]
fn counter_ticks() -> u64 {
    let ticks: u64;
    // This diagnostic SDK enables CNTKCTL_EL1.EL0VCTEN for this read.
    unsafe {
        core::arch::asm!(
            "isb", "mrs {ticks}, cntvct_el0",
            ticks = out(reg) ticks,
            options(nostack, preserves_flags),
        )
    };
    ticks
}

/* A bounded, non-reclaiming heap for this first boot-only UI experiment. */
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
            match HEAP_USED.compare_exchange_weak(old, end,
                    Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return (base + start) as *mut u8,
                Err(current) => old = current,
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BootHeap = BootHeap;

struct IphonePlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for IphonePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration { Duration::ZERO }
}

#[cfg(feature = "line-timing")]
#[derive(Default)]
struct LineTimings {
    render_ticks: u64,
    write_ticks: u64,
}

struct FrameLines<'a> {
    framebuffer: NonNull<u32>,
    #[cfg(feature = "line-timing")]
    timings: Option<&'a mut LineTimings>,
    #[cfg(not(feature = "line-timing"))]
    _lifetime: core::marker::PhantomData<&'a ()>,
}

impl LineBufferProvider for FrameLines<'_> {
    type TargetPixel = slint::Rgb8Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        if line >= RENDER_HEIGHT || range.end > RENDER_WIDTH { return; }
        let mut pixels = [slint::Rgb8Pixel::default(); RENDER_WIDTH];
        #[cfg(feature = "line-timing")]
        let render_start = counter_ticks();
        render_fn(&mut pixels[..range.len()]);
        #[cfg(feature = "line-timing")]
        let write_start = counter_ticks();
        #[cfg(feature = "row-copy")]
        {
            let mut packed = [0u32; WIDTH];
            for (dst, pixel) in packed[..range.len()].iter_mut().zip(pixels.iter()) {
                *dst = ((pixel.r as u32) << 24) | ((pixel.g as u32) << 16)
                    | ((pixel.b as u32) << 8) | 0xff;
            }
            // A single row copy lets the compiler use its optimized memory-copy
            // implementation for the Normal-NC destination.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    packed.as_ptr(),
                    self.framebuffer.as_ptr().add(line * STRIDE + range.start),
                    range.len(),
                );
            }
        }
        #[cfg(all(not(feature = "row-copy"), not(feature = "full-suite-half-res"), not(feature = "full-suite-two-thirds")))]
        for (column, pixel) in range.zip(pixels.iter()) {
            /* D11 scanout uses RRGGBBAA words; alpha occupies the low byte. */
            let color = ((pixel.r as u32) << 24) | ((pixel.g as u32) << 16)
                | ((pixel.b as u32) << 8) | 0xff;
            /* This is mapped framebuffer RAM, as in the working C shell, not
             * a register. Plain stores let LLVM combine sequential writes. */
            unsafe { self.framebuffer.as_ptr().add(line * STRIDE + column).write(color) };
        }
        #[cfg(feature = "full-suite-half-res")]
        for (column, pixel) in range.zip(pixels.iter()) {
            let color = ((pixel.r as u32) << 24) | ((pixel.g as u32) << 16)
                | ((pixel.b as u32) << 8) | 0xff;
            let base = line * 2 * STRIDE + column * 2;
            unsafe {
                self.framebuffer.as_ptr().add(base).write(color);
                self.framebuffer.as_ptr().add(base + 1).write(color);
                self.framebuffer.as_ptr().add(base + STRIDE).write(color);
                self.framebuffer.as_ptr().add(base + STRIDE + 1).write(color);
            }
        }
        #[cfg(all(feature = "full-suite-two-thirds", not(feature = "full-suite-two-thirds-row-copy")))]
        for (column, pixel) in range.zip(pixels.iter()) {
            let color = ((pixel.r as u32) << 24) | ((pixel.g as u32) << 16)
                | ((pixel.b as u32) << 8) | 0xff;
            let x0 = column * 3 / 2;
            let x1 = (column + 1) * 3 / 2;
            let y0 = line * 3 / 2;
            let y1 = (line + 1) * 3 / 2;
            for y in y0..y1 {
                for x in x0..x1 {
                    unsafe { self.framebuffer.as_ptr().add(y * STRIDE + x).write(color) };
                }
            }
        }
        #[cfg(feature = "full-suite-two-thirds-row-copy")]
        {
            let x_start = range.start * 3 / 2;
            let x_end = range.end * 3 / 2;
            let mut packed = [0u32; WIDTH];
            for (column, pixel) in range.zip(pixels.iter()) {
                let color = ((pixel.r as u32) << 24) | ((pixel.g as u32) << 16)
                    | ((pixel.b as u32) << 8) | 0xff;
                let start = column * 3 / 2;
                let end = (column + 1) * 3 / 2;
                packed[start..end].fill(color);
            }
            let y_start = line * 3 / 2;
            let y_end = (line + 1) * 3 / 2;
            for y in y_start..y_end {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        packed.as_ptr().add(x_start),
                        self.framebuffer.as_ptr().add(y * STRIDE + x_start),
                        x_end - x_start,
                    );
                }
            }
        }
        #[cfg(feature = "line-timing")]
        {
            let write_done = counter_ticks();
            if let Some(timings) = &mut self.timings {
                timings.render_ticks += write_start.wrapping_sub(render_start);
                timings.write_ticks += write_done.wrapping_sub(write_start);
            }
        }
    }
}

#[cfg(any(feature = "raster-profile", feature = "snapshot-boot"))]
struct DiscardLines;

#[cfg(any(feature = "raster-profile", feature = "snapshot-boot"))]
impl LineBufferProvider for DiscardLines {
    type TargetPixel = slint::Rgb8Pixel;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        if line >= HEIGHT || range.end > WIDTH { return; }
        let mut pixels = [slint::Rgb8Pixel::default(); WIDTH];
        render_fn(&mut pixels[..range.len()]);
    }
}

#[cfg(feature = "snapshot-boot")]
fn present_snapshot(framebuffer: NonNull<u32>) -> bool {
    // Generated from the exact 1080x1920 Slint default frame by
    // src/bin/encode_snapshot.rs. Each row is a u16 run count followed by
    // (u16 length, u32 RRGGBBAA) records, all little-endian.
    #[cfg(feature = "full-suite-snapshot-boot")]
    let encoded = include_bytes!("../assets/full-suite-home-1080x1920.rle");
    #[cfg(not(feature = "full-suite-snapshot-boot"))]
    let encoded = include_bytes!("../assets/notes-1080x1920.rle");
    let mut offset = 0usize;
    for row in 0..HEIGHT {
        if offset + 2 > encoded.len() { return false; }
        let run_count = u16::from_le_bytes([encoded[offset], encoded[offset + 1]]) as usize;
        offset += 2;
        let mut packed = [0u32; WIDTH];
        let mut column = 0usize;
        for _ in 0..run_count {
            if offset + 6 > encoded.len() { return false; }
            let length = u16::from_le_bytes([encoded[offset], encoded[offset + 1]]) as usize;
            let color = u32::from_le_bytes([
                encoded[offset + 2], encoded[offset + 3],
                encoded[offset + 4], encoded[offset + 5],
            ]);
            offset += 6;
            if length == 0 || column + length > WIDTH { return false; }
            packed[column..column + length].fill(color);
            column += length;
        }
        if column != WIDTH { return false; }
        unsafe {
            core::ptr::copy_nonoverlapping(
                packed.as_ptr(), framebuffer.as_ptr().add(row * STRIDE), WIDTH,
            );
        }
    }
    if offset != encoded.len() { return false; }
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    true
}

#[repr(C)]
struct TouchState {
    revision: u32,
    status: u32,
    stage: u32,
    packets: u32,
    active: u32,
    finger: u32,
    x: u32,
    y: u32,
    #[cfg(feature = "full-suite-queued-touch")]
    event_write: u32,
    #[cfg(feature = "full-suite-queued-touch")]
    events: [TouchEvent; 256],
}

#[cfg(feature = "full-suite-queued-touch")]
#[repr(C)]
struct TouchEvent {
    revision: u32,
    active: u32,
    finger: u32,
    x: u32,
    y: u32,
}

struct NotesService {
    window: Rc<MinimalSoftwareWindow>,
    _ui: NotesWindow,
    framebuffer: NonNull<u32>,
    touch: NonNull<TouchState>,
    pressed: bool,
    pressed_position: LogicalPosition,
    #[cfg(feature = "full-suite-queued-touch")]
    touch_read_sequence: u32,
    #[cfg(feature = "full-suite-input-profile")]
    pressed_ticks: u64,
}

impl NotesService {
    fn render(&self) {
        let framebuffer = self.framebuffer;
        self.window.draw_if_needed(|renderer| {
            renderer.render_by_line(FrameLines {
                framebuffer,
                #[cfg(feature = "line-timing")]
                timings: None,
                #[cfg(not(feature = "line-timing"))]
                _lifetime: core::marker::PhantomData,
            });
        });
        core::sync::atomic::fence(Ordering::SeqCst);
    }

    #[cfg(feature = "raster-profile")]
    fn render_initial_raster_profile(&self) -> (u64, u64) {
        let cpu_start = counter_ticks();
        self.window.draw_if_needed(|renderer| {
            renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
            renderer.render_by_line(DiscardLines);
        });
        let cpu_ms = counter_ticks().wrapping_sub(cpu_start) / 24_000;

        self.window.window().request_redraw();
        let live_start = counter_ticks();
        self.window.draw_if_needed(|renderer| {
            renderer.render_by_line(FrameLines {
                framebuffer: self.framebuffer,
                #[cfg(feature = "line-timing")]
                timings: None,
                #[cfg(not(feature = "line-timing"))]
                _lifetime: core::marker::PhantomData,
            });
            renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
        });
        let live_ms = counter_ticks().wrapping_sub(live_start) / 24_000;
        (cpu_ms, live_ms)
    }

    #[cfg(feature = "line-timing")]
    fn render_initial_line_timed(&self) -> (u64, u64, u64) {
        let mut timings = LineTimings::default();
        let start = counter_ticks();
        self.window.draw_if_needed(|renderer| {
            renderer.render_by_line(FrameLines {
                framebuffer: self.framebuffer,
                timings: Some(&mut timings),
            });
        });
        let total = counter_ticks().wrapping_sub(start);
        (timings.render_ticks / 24_000, timings.write_ticks / 24_000,
         total / 24_000)
    }

    #[cfg(feature = "stage-timing")]
    fn render_initial_staged(&self, stage: NonNull<u32>) -> (u64, u64) {
        let start = counter_ticks();
        self.window.draw_if_needed(|renderer| {
            renderer.render_by_line(FrameLines {
                framebuffer: stage,
                #[cfg(feature = "line-timing")]
                timings: None,
                #[cfg(not(feature = "line-timing"))]
                _lifetime: core::marker::PhantomData,
            });
        });
        let raster_done = counter_ticks();
        // The stage buffer has the same stride as the live Pongo framebuffer.
        // Copy the complete first frame so untouched padding cannot retain old pixels.
        unsafe {
            core::ptr::copy_nonoverlapping(
                stage.as_ptr(), self.framebuffer.as_ptr(), STRIDE * HEIGHT,
            );
        }
        let copy_done = counter_ticks();
        core::sync::atomic::fence(Ordering::SeqCst);
        ((raster_done.wrapping_sub(start)) / 24_000,
         (copy_done.wrapping_sub(raster_done)) / 24_000)
    }

    #[cfg(not(feature = "full-suite-queued-touch"))]
    fn touch_snapshot(&self) -> Option<(bool, u32, u32)> {
        let ptr = self.touch.as_ptr();
        for _ in 0..4 {
            let before = unsafe { core::ptr::addr_of!((*ptr).revision).read_volatile() };
            if before & 1 != 0 { continue; }
            let status = unsafe { core::ptr::addr_of!((*ptr).status).read_volatile() };
            let active = unsafe { core::ptr::addr_of!((*ptr).active).read_volatile() };
            let x = unsafe { core::ptr::addr_of!((*ptr).x).read_volatile() };
            let y = unsafe { core::ptr::addr_of!((*ptr).y).read_volatile() };
            core::sync::atomic::fence(Ordering::Acquire);
            let after = unsafe { core::ptr::addr_of!((*ptr).revision).read_volatile() };
            if before == after && status == 2 {
                return Some((active != 0, x.min(4096), y.min(4096)));
            }
        }
        None
    }
}

#[protection_domain]
fn init() -> NotesService {
    let heap = memory_region_symbol!(slint_heap: *mut u8);
    HEAP_BASE.store(heap.as_ptr() as usize, Ordering::Release);
    let framebuffer = memory_region_symbol!(slint_framebuffer: *mut u32);
    let touch = memory_region_symbol!(slint_touch_state: *mut TouchState);

    #[cfg(feature = "snapshot-boot")]
    let (snapshot_ready, snapshot_ms) = {
        let start = counter_ticks();
        let ready = present_snapshot(framebuffer);
        (ready, counter_ticks().wrapping_sub(start) / 24_000)
    };

    let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    slint::platform::set_platform(Box::new(IphonePlatform { window: window.clone() }))
        .expect("set Slint platform");
    window.window().dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: SCALE });
    window.set_size(PhysicalSize::new(RENDER_WIDTH as u32, RENDER_HEIGHT as u32));

    let ui = NotesWindow::new().expect("create Slint Notes");
    #[cfg(feature = "full-suite-ui")]
    personal_os_slint_notes_spike::suite::wire(&ui);
    #[cfg(not(feature = "full-suite-ui"))]
    {
    let weak = ui.as_weak();
    ui.on_save_note(move |_| {
        if let Some(ui) = weak.upgrade() {
            #[cfg(feature = "workspace-ui")]
            ui.set_status("SAVE TAP RECEIVED".into());
            #[cfg(not(feature = "workspace-ui"))]
            ui.set_status("SAVED THROUGH TOUCH".into());
        }
    });
    }
    ui.show().expect("show Slint Notes");
    let service = NotesService {
        window, _ui: ui, framebuffer, touch, pressed: false,
        pressed_position: LogicalPosition::default(),
        #[cfg(feature = "full-suite-queued-touch")]
        touch_read_sequence: 0,
        #[cfg(feature = "full-suite-input-profile")]
        pressed_ticks: 0,
    };
    #[cfg(feature = "fill-timing")]
    let fill_ms = {
        let start = counter_ticks();
        for index in 0..STRIDE * HEIGHT {
            // A uniform dark blue fill measures sequential screen-memory writes
            // without Slint rasterization or per-pixel color conversion.
            unsafe { framebuffer.as_ptr().add(index).write(0x061c38ff) };
        }
        // Wait for the Normal-NC stores before reading the finish counter.
        unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
        counter_ticks().wrapping_sub(start) / 24_000
    };
    #[cfg(all(not(feature = "stage-timing"), not(feature = "raster-profile"), not(feature = "snapshot-boot")))]
    {
        #[cfg(all(feature = "frame-timing", not(feature = "line-timing")))]
        let frame_start = counter_ticks();
        #[cfg(feature = "line-timing")]
        let (render_ms, write_ms, elapsed_ms) = service.render_initial_line_timed();
        #[cfg(not(feature = "line-timing"))]
        service.render();
        #[cfg(feature = "frame-timing")]
        {
            // 24 MHz was measured on this D11. This includes Slint rasterization
            // and writes to the mapped framebuffer, but not panel scanout.
            #[cfg(not(feature = "line-timing"))]
            let elapsed_ms = counter_ticks().wrapping_sub(frame_start) / 24_000;
            #[cfg(all(not(feature = "line-timing"), not(feature = "fill-timing"), not(feature = "full-suite-profile")))]
            service._ui.set_status(alloc::format!("FRAME {elapsed_ms} MS").into());
            #[cfg(feature = "full-suite-profile")]
            service._ui.set_perf(alloc::format!("FIRST {elapsed_ms} MS").into());
            #[cfg(feature = "line-timing")]
            service._ui.set_status(
                alloc::format!("R{render_ms} W{write_ms} T{elapsed_ms}").into()
            );
            #[cfg(feature = "fill-timing")]
            service._ui.set_status(
                alloc::format!("F{fill_ms} T{elapsed_ms} MS").into()
            );
            service.render();
        }
    }
    #[cfg(feature = "snapshot-boot")]
    {
        if snapshot_ready {
            let start = counter_ticks();
            service.window.draw_if_needed(|renderer| {
                renderer.render_by_line(DiscardLines);
            });
            let cpu_ms = counter_ticks().wrapping_sub(start) / 24_000;
            #[cfg(feature = "full-suite-snapshot-boot")]
            service._ui.set_perf(alloc::format!("S{snapshot_ms} C{cpu_ms} MS").into());
            #[cfg(not(feature = "full-suite-snapshot-boot"))]
            service._ui.set_status(alloc::format!("S{snapshot_ms} C{cpu_ms} MS").into());
        }
        // If decoding failed, Slint performs its ordinary first live draw.
        // Otherwise only the changed status area is written to the screen.
        service.render();
    }
    #[cfg(feature = "raster-profile")]
    {
        let (cpu_ms, live_ms) = service.render_initial_raster_profile();
        service._ui.set_status(alloc::format!("C{cpu_ms} L{live_ms} MS").into());
        service.render();
    }
    #[cfg(feature = "stage-timing")]
    {
        let stage = memory_region_symbol!(slint_stage_framebuffer: *mut u32);
        let (raster_ms, copy_ms) = service.render_initial_staged(stage);
        service._ui.set_status(alloc::format!("R {raster_ms} C {copy_ms} MS").into());
        // Follow-up interaction uses the proven direct render path.
        service.render();
    }
    debug_println!("SLINT|IPHONE|NOTES READY");
    service
}

impl NotesService {
    fn on_touch_sample(&mut self, active: bool, x: u32, y: u32) {
        if active != self.pressed {
                #[cfg(feature = "full-suite-input-profile")]
                let gap_ms = if active {
                    self.pressed_ticks = counter_ticks();
                    0
                } else {
                    counter_ticks().wrapping_sub(self.pressed_ticks) / 24_000
                };
                let position = if active {
                    let position = LogicalPosition::new(
                        x as f32 * WIDTH as f32 / 4096.0 / TOUCH_SCALE,
                        y as f32 * HEIGHT as f32 / 4096.0 / TOUCH_SCALE,
                    );
                    self.pressed_position = position;
                    position
                } else {
                    /* The D11 service publishes (0, 0) when contact ends;
                     * release at the last contact so Slint can form a click. */
                    self.pressed_position
                };
                #[cfg(feature = "full-suite-press-click")]
                {
                    if active {
                        // This UI contains only tap targets. D11 release reports can
                        // arrive seconds after a brief contact, so complete the click
                        // immediately at the first contact and ignore that late release.
                        self.window.window().dispatch_event(WindowEvent::PointerPressed {
                            position, button: PointerEventButton::Left,
                        });
                        self.window.window().dispatch_event(WindowEvent::PointerReleased {
                            position, button: PointerEventButton::Left,
                        });
                        self.pressed = true;
                        let start = counter_ticks();
                        self.render();
                        let draw_ms = counter_ticks().wrapping_sub(start) / 24_000;
                        self._ui.set_perf(alloc::format!("PRESS DRAW {draw_ms} MS").into());
                        self.render();
                    } else {
                        self.pressed = false;
                    }
                    return;
                }
                #[cfg(not(feature = "full-suite-press-click"))]
                {
                self.window.window().dispatch_event(if active {
                    WindowEvent::PointerPressed {
                        position, button: PointerEventButton::Left,
                    }
                } else {
                    WindowEvent::PointerReleased {
                        position, button: PointerEventButton::Left,
                    }
                });
                self.pressed = active;
                #[cfg(feature = "full-suite-profile")]
                let draw_start = counter_ticks();
                self.render();
                #[cfg(feature = "full-suite-profile")]
                if !active {
                    let draw_ms = counter_ticks().wrapping_sub(draw_start) / 24_000;
                    #[cfg(feature = "full-suite-input-profile")]
                    self._ui.set_perf(alloc::format!("GAP {gap_ms} DRAW {draw_ms} MS").into());
                    #[cfg(not(feature = "full-suite-input-profile"))]
                    self._ui.set_perf(alloc::format!("DRAW {draw_ms} MS").into());
                    self.render();
                }
                }
        }
    }

    #[cfg(feature = "full-suite-queued-touch")]
    fn drain_touch_queue(&mut self) {
        let ptr = self.touch.as_ptr();
        let latest = unsafe { core::ptr::addr_of!((*ptr).event_write).read_volatile() };
        if latest.wrapping_sub(self.touch_read_sequence) > 256 {
            self.touch_read_sequence = latest.wrapping_sub(256);
        }
        while self.touch_read_sequence != latest {
            let sequence = self.touch_read_sequence.wrapping_add(1);
            let entry = unsafe { core::ptr::addr_of!((*ptr).events[(sequence.wrapping_sub(1) % 256) as usize]) };
            let mut sample = None;
            for _ in 0..4 {
                let before = unsafe { core::ptr::addr_of!((*entry).revision).read_volatile() };
                if before != sequence.wrapping_mul(2) { continue; }
                let active = unsafe { core::ptr::addr_of!((*entry).active).read_volatile() };
                let x = unsafe { core::ptr::addr_of!((*entry).x).read_volatile() };
                let y = unsafe { core::ptr::addr_of!((*entry).y).read_volatile() };
                core::sync::atomic::fence(Ordering::Acquire);
                let after = unsafe { core::ptr::addr_of!((*entry).revision).read_volatile() };
                if before == after { sample = Some((active != 0, x, y)); break; }
            }
            let Some((active, x, y)) = sample else { break; };
            self.touch_read_sequence = sequence;
            self.on_touch_sample(active, x, y);
        }
    }
}

impl Handler for NotesService {
    type Error = Infallible;

    fn notified(&mut self, _channels: ChannelSet) -> Result<(), Self::Error> {
        #[cfg(feature = "full-suite-queued-touch")]
        self.drain_touch_queue();
        #[cfg(not(feature = "full-suite-queued-touch"))]
        if let Some((active, x, y)) = self.touch_snapshot() {
            self.on_touch_sample(active, x, y);
        }
        Ok(())
    }
}
