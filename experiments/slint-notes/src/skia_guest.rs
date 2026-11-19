use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::Ordering;

use sel4_microkit::debug_println;

const PREFIX: usize = 16;
const SCALE: f32 = 2.6086957;
const LABEL_WIDTH: i32 = 860;
const LABEL_HEIGHT: i32 = 64;
const LABEL_BYTES: usize = (LABEL_WIDTH as usize) * (LABEL_HEIGHT as usize) * 4;
const TITLE_WIDTH: i32 = 900;
const TITLE_HEIGHT: i32 = 110;
const TITLE_BYTES: usize = (TITLE_WIDTH as usize) * (TITLE_HEIGHT as usize) * 4;
const TITLE_SIZE: f32 = 32.0 * SCALE;
const HOME_LABEL_WIDTH: i32 = 360;
const HOME_LABEL_HEIGHT: i32 = 64;
const HOME_LABEL_BYTES: usize = (HOME_LABEL_WIDTH as usize) * (HOME_LABEL_HEIGHT as usize) * 4;
const HOME_DETAIL_WIDTH: i32 = 560;
const HOME_DETAIL_HEIGHT: i32 = 48;
const HOME_DETAIL_BYTES: usize = (HOME_DETAIL_WIDTH as usize) * (HOME_DETAIL_HEIGHT as usize) * 4;
const HOME_LABEL_SIZE: f32 = 18.0 * SCALE;
const HOME_DETAIL_SIZE: f32 = 13.0 * SCALE;
const HINT_WIDTH: i32 = 280;
const HINT_HEIGHT: i32 = 40;
const HINT_BYTES: usize = (HINT_WIDTH as usize) * (HINT_HEIGHT as usize) * 4;
const HINT_SIZE: f32 = 12.0 * SCALE;
const CHAT_SLOTS: usize = 16;
const CHAT_TEXT_CAP: usize = 80;
const CHAT_NAME_WIDTH: i32 = 480;
const CHAT_NAME_HEIGHT: i32 = 56;
const CHAT_NAME_BYTES: usize = (CHAT_NAME_WIDTH as usize) * (CHAT_NAME_HEIGHT as usize) * 4;
const CHAT_PREVIEW_WIDTH: i32 = 620;
const CHAT_PREVIEW_HEIGHT: i32 = 44;
const CHAT_PREVIEW_BYTES: usize = (CHAT_PREVIEW_WIDTH as usize) * (CHAT_PREVIEW_HEIGHT as usize) * 4;
const CHAT_INITIAL_WIDTH: i32 = 64;
const CHAT_INITIAL_HEIGHT: i32 = 64;
const CHAT_INITIAL_BYTES: usize = (CHAT_INITIAL_WIDTH as usize) * (CHAT_INITIAL_HEIGHT as usize) * 4;
const CHAT_NAME_SIZE: f32 = 16.0 * SCALE;
const CHAT_PREVIEW_SIZE: f32 = 13.0 * SCALE;
const CHAT_TIME_WIDTH: i32 = 120;
const CHAT_TIME_HEIGHT: i32 = 40;
const CHAT_TIME_BYTES: usize = (CHAT_TIME_WIDTH as usize) * (CHAT_TIME_HEIGHT as usize) * 4;
const CHAT_TIME_SIZE: f32 = 11.0 * SCALE;
const CHAT_INITIAL_SIZE: f32 = 18.0 * SCALE;
const CHAT_ACCENTS: [u32; 4] = [0xFF28_D8E8, 0xFFB4_8CFF, 0xFFFF_CC55, 0xFF68_DDC2];

struct HomeCard {
    label: &'static str,
    detail: &'static str,
    color: u32,
    column: u8,
    row: u8,
}

const HOME_CARDS: [HomeCard; 5] = [
    HomeCard { label: "SIMPLEX", detail: "PRIVATE CHATS", color: 0xFF28_D8E8, column: 0, row: 0 },
    HomeCard { label: "CONTACTS", detail: "PEOPLE", color: 0xFFB4_A0FF, column: 1, row: 0 },
    HomeCard { label: "NEXAWAL", detail: "MONERO WALLET", color: 0xFFFF_9B68, column: 0, row: 1 },
    HomeCard { label: "TOOLS", detail: "NOTES \u{00B7} FILES \u{00B7} MORE", color: 0xFF8F_B5CC, column: 1, row: 1 },
    HomeCard { label: "SETTINGS", detail: "PRIVACY \u{00B7} APPEARANCE", color: 0xFF68_DDC2, column: 0, row: 2 },
];

fn px(logical: f32) -> usize {
    (logical * SCALE + 0.5) as usize
}

fn home_text_origin(card: &HomeCard) -> (usize, usize, usize) {
    let parent_w = 1080.0 / SCALE;
    let parent_h = 2340.0 / SCALE;
    let card_x = if card.column == 0 {
        17.0
    } else {
        27.0 + (parent_w - 44.0) / 2.0
    };
    let card_y = match card.row {
        0 => parent_h * 0.14,
        1 => parent_h * 0.34,
        _ => parent_h * 0.55,
    };
    (px(card_x + 58.0), px(card_y + 12.0), px(card_y + 43.0))
}

fn card_box(column: u8, row: u8) -> (f32, f32, f32, f32) {
    let parent_w = 1080.0 / SCALE;
    let parent_h = 2340.0 / SCALE;
    let card_x = if column == 0 {
        17.0
    } else {
        27.0 + (parent_w - 44.0) / 2.0
    };
    let card_y = match row {
        0 => parent_h * 0.14,
        1 => parent_h * 0.34,
        _ => parent_h * 0.55,
    };
    let card_w = if row >= 2 {
        parent_w - 34.0
    } else {
        (parent_w - 44.0) / 2.0
    };
    let card_h = if row >= 2 {
        parent_h * 0.11
    } else {
        parent_h * 0.19
    };
    (card_x, card_y, card_w, card_h)
}

fn px_i(logical: f32) -> i32 {
    (logical * SCALE + 0.5) as i32
}

const TITLES: [&str; 15] = [
    "PRIVATE NOTES",
    "CONTACTS",
    "CALCULATOR",
    "CLOCK",
    "CALENDAR",
    "SIMPLEX",
    "PHONE DEMO",
    "SETTINGS",
    "FILES",
    "TOOLS",
    "SYSTEM",
    "APPEARANCE",
    "PRIVACY",
    "",
    "DATABASE",
];

static mut LABEL_READY: bool = false;
static mut LABEL_PIXELS: [u8; LABEL_BYTES] = [0; LABEL_BYTES];
static mut TITLE_READY: [bool; TITLES.len()] = [false; TITLES.len()];
static mut TITLE_PIXELS: [[u8; TITLE_BYTES]; TITLES.len()] = [[0; TITLE_BYTES]; TITLES.len()];
static mut HOME_LABEL_READY: [bool; HOME_CARDS.len()] = [false; HOME_CARDS.len()];
static mut HOME_LABEL_PIXELS: [[u8; HOME_LABEL_BYTES]; HOME_CARDS.len()] = [[0; HOME_LABEL_BYTES]; HOME_CARDS.len()];
static mut HOME_DETAIL_READY: [bool; HOME_CARDS.len()] = [false; HOME_CARDS.len()];
static mut HOME_DETAIL_PIXELS: [[u8; HOME_DETAIL_BYTES]; HOME_CARDS.len()] = [[0; HOME_DETAIL_BYTES]; HOME_CARDS.len()];
static mut HINT_READY: [bool; 4] = [false; 4];
static mut HINT_PIXELS: [[u8; HINT_BYTES]; 4] = [[0; HINT_BYTES]; 4];
static mut CHAT_COUNT: usize = 0;
static mut CHAT_NAME_LEN: [usize; CHAT_SLOTS] = [0; CHAT_SLOTS];
static mut CHAT_NAME_KEY: [[u8; CHAT_TEXT_CAP]; CHAT_SLOTS] = [[0; CHAT_TEXT_CAP]; CHAT_SLOTS];
static mut CHAT_NAME_DIRTY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_NAME_READY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_NAME_PIXELS: [[u8; CHAT_NAME_BYTES]; CHAT_SLOTS] = [[0; CHAT_NAME_BYTES]; CHAT_SLOTS];
static mut CHAT_PREVIEW_LEN: [usize; CHAT_SLOTS] = [0; CHAT_SLOTS];
static mut CHAT_PREVIEW_KEY: [[u8; CHAT_TEXT_CAP]; CHAT_SLOTS] = [[0; CHAT_TEXT_CAP]; CHAT_SLOTS];
static mut CHAT_PREVIEW_DIRTY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_PREVIEW_READY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_PREVIEW_PIXELS: [[u8; CHAT_PREVIEW_BYTES]; CHAT_SLOTS] = [[0; CHAT_PREVIEW_BYTES]; CHAT_SLOTS];
static mut CHAT_TIME_LEN: [usize; CHAT_SLOTS] = [0; CHAT_SLOTS];
static mut CHAT_TIME_KEY: [[u8; 8]; CHAT_SLOTS] = [[0; 8]; CHAT_SLOTS];
static mut CHAT_TIME_DIRTY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_TIME_READY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_TIME_PIXELS: [[u8; CHAT_TIME_BYTES]; CHAT_SLOTS] = [[0; CHAT_TIME_BYTES]; CHAT_SLOTS];
static mut CHAT_INITIAL_LEN: [usize; CHAT_SLOTS] = [0; CHAT_SLOTS];
static mut CHAT_INITIAL_KEY: [[u8; 4]; CHAT_SLOTS] = [[0; 4]; CHAT_SLOTS];
static mut CHAT_INITIAL_DIRTY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_INITIAL_READY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_INITIAL_PIXELS: [[u8; CHAT_INITIAL_BYTES]; CHAT_SLOTS] = [[0; CHAT_INITIAL_BYTES]; CHAT_SLOTS];
static mut CHAT_HAS_PHOTO: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
const CHAT_BADGE_SIZE: i32 = 52;
const CHAT_BADGE_BYTES: usize = (CHAT_BADGE_SIZE as usize) * (CHAT_BADGE_SIZE as usize) * 4;
static mut CHAT_UNREAD: [u8; CHAT_SLOTS] = [0; CHAT_SLOTS];
static mut CHAT_UNREAD_DIRTY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_BADGE_READY: [bool; CHAT_SLOTS] = [false; CHAT_SLOTS];
static mut CHAT_BADGE_PIXELS: [[u8; CHAT_BADGE_BYTES]; CHAT_SLOTS] = [[0; CHAT_BADGE_BYTES]; CHAT_SLOTS];

unsafe extern "C" {
    fn skia_run_ctors();
    fn skia_prepare_font(font_bytes: *const u8, font_len: i32) -> i32;
    fn skia_draw_text(
        rgba: *mut u8,
        width: i32,
        height: i32,
        stride: i32,
        text: *const u8,
        text_len: i32,
        size: f32,
        color: u32,
        baseline: f32,
        embolden: i32,
    ) -> i32;
}

unsafe fn guest_alloc(size: usize, align: usize) -> *mut u8 {
    let align = align.max(PREFIX);
    let Some(total) = size.checked_add(align) else {
        return core::ptr::null_mut();
    };
    let Ok(layout) = Layout::from_size_align(total.max(1), align) else {
        return core::ptr::null_mut();
    };
    let raw = unsafe { super::ALLOCATOR.alloc(layout) };
    if raw.is_null() {
        return raw;
    }
    let user = unsafe { raw.add(align) };
    unsafe {
        let header = user.cast::<usize>().sub(2);
        header.write(size);
        header.add(1).write(align);
    }
    user
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
    unsafe { guest_alloc(size, PREFIX) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    if ptr.is_null() {
        return unsafe { guest_alloc(size, PREFIX) };
    }
    if size == 0 {
        return core::ptr::null_mut();
    }
    unsafe {
        let old = ptr.cast::<usize>().sub(2).read();
        let align = ptr.cast::<usize>().sub(1).read();
        let next = guest_alloc(size, align);
        if next.is_null() {
            return core::ptr::null_mut();
        }
        core::ptr::copy_nonoverlapping(ptr, next, old.min(size));
        next
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(_ptr: *mut u8) {}

macro_rules! libm1 {
    ($name:ident, $ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(value: $ty) -> $ty {
            libm::$name(value)
        }
    };
}

macro_rules! libm2 {
    ($name:ident, $ty:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(left: $ty, right: $ty) -> $ty {
            libm::$name(left, right)
        }
    };
}

libm1!(acos, f64);
libm1!(acosf, f32);
libm1!(acosh, f64);
libm1!(asin, f64);
libm1!(asinh, f64);
libm1!(atan, f64);
libm2!(atan2, f64);
libm2!(atan2f, f32);
libm1!(atanh, f64);
libm1!(cbrt, f64);
libm1!(cbrtf, f32);
libm1!(ceil, f64);
libm1!(ceilf, f32);
libm1!(cos, f64);
libm1!(cosf, f32);
libm1!(cosh, f64);
libm1!(exp, f64);
libm1!(exp2, f64);
libm1!(expf, f32);
libm1!(fabs, f64);
libm1!(fabsf, f32);
libm1!(floor, f64);
libm1!(floorf, f32);
libm2!(fmaxf, f32);
libm2!(fminf, f32);
libm2!(fmod, f64);
libm2!(fmodf, f32);
libm1!(log, f64);
libm1!(log2, f64);
libm1!(log2f, f32);
libm2!(nextafterf, f32);
libm2!(pow, f64);
libm2!(powf, f32);
libm2!(remainder, f64);
libm1!(round, f64);
libm1!(sin, f64);
libm1!(sinf, f32);
libm1!(sinh, f64);
libm1!(sqrt, f64);
libm1!(sqrtf, f32);
libm1!(tan, f64);
libm1!(tanf, f32);
libm1!(tanh, f64);
libm1!(trunc, f64);

#[unsafe(no_mangle)]
pub extern "C" fn fma(x: f64, y: f64, z: f64) -> f64 {
    libm::fma(x, y, z)
}

pub fn prepare_skia_label() -> bool {
    unsafe {
        if core::ptr::addr_of!(LABEL_READY).read() {
            return false;
        }
        skia_run_ctors();
        let font = include_bytes!("../assets/fonts/Roboto-Regular.ttf");
        let prepared = skia_prepare_font(font.as_ptr(), font.len() as i32);
        let status = "SECURE \u{00B7} PRIVATE \u{00B7} YOURS";
        let drawn = if prepared == 0 {
            0
        } else {
            skia_draw_text(
                core::ptr::addr_of_mut!(LABEL_PIXELS).cast::<u8>(),
                LABEL_WIDTH,
                LABEL_HEIGHT,
                LABEL_WIDTH * 4,
                status.as_ptr(),
                status.len() as i32,
                17.0 * SCALE,
                0xFFFF_FFFF,
                -1.0,
                0,
            )
        };
        core::ptr::addr_of_mut!(LABEL_READY).write(drawn != 0);
        let mut titles_drawn = 0;
        if prepared != 0 {
            for (index, title) in TITLES.iter().enumerate() {
                let pixels = core::ptr::addr_of_mut!(TITLE_PIXELS)
                    .cast::<u8>()
                    .add(index * TITLE_BYTES);
                let drawn = skia_draw_text(
                    pixels,
                    TITLE_WIDTH,
                    TITLE_HEIGHT,
                    TITLE_WIDTH * 4,
                    title.as_ptr(),
                    title.len() as i32,
                    TITLE_SIZE,
                    0xFFEE_F8FD,
                    -1.0,
                    1,
                );
                core::ptr::addr_of_mut!(TITLE_READY)
                    .cast::<bool>()
                    .add(index)
                    .write(drawn != 0);
                if drawn != 0 {
                    titles_drawn += 1;
                }
            }
        }
        let mut home_drawn = 0;
        if prepared != 0 {
            for (index, card) in HOME_CARDS.iter().enumerate() {
                let label_pixels = core::ptr::addr_of_mut!(HOME_LABEL_PIXELS)
                    .cast::<u8>()
                    .add(index * HOME_LABEL_BYTES);
                let label_drawn = skia_draw_text(
                    label_pixels,
                    HOME_LABEL_WIDTH,
                    HOME_LABEL_HEIGHT,
                    HOME_LABEL_WIDTH * 4,
                    card.label.as_ptr(),
                    card.label.len() as i32,
                    HOME_LABEL_SIZE,
                    card.color,
                    -1.0,
                    1,
                );
                core::ptr::addr_of_mut!(HOME_LABEL_READY)
                    .cast::<bool>()
                    .add(index)
                    .write(label_drawn != 0);
                let detail_pixels = core::ptr::addr_of_mut!(HOME_DETAIL_PIXELS)
                    .cast::<u8>()
                    .add(index * HOME_DETAIL_BYTES);
                let detail_drawn = skia_draw_text(
                    detail_pixels,
                    HOME_DETAIL_WIDTH,
                    HOME_DETAIL_HEIGHT,
                    HOME_DETAIL_WIDTH * 4,
                    card.detail.as_ptr(),
                    card.detail.len() as i32,
                    HOME_DETAIL_SIZE,
                    0xFFA9_C1CF,
                    -1.0,
                    0,
                );
                core::ptr::addr_of_mut!(HOME_DETAIL_READY)
                    .cast::<bool>()
                    .add(index)
                    .write(detail_drawn != 0);
                if label_drawn != 0 && detail_drawn != 0 {
                    home_drawn += 1;
                }
            }
        }
        let mut hints_drawn = 0;
        if prepared != 0 {
            let hint = "TAP TO OPEN";
            for index in 0..4 {
                let pixels = core::ptr::addr_of_mut!(HINT_PIXELS)
                    .cast::<u8>()
                    .add(index * HINT_BYTES);
                let hint_drawn = skia_draw_text(
                    pixels,
                    HINT_WIDTH,
                    HINT_HEIGHT,
                    HINT_WIDTH * 4,
                    hint.as_ptr(),
                    hint.len() as i32,
                    HINT_SIZE,
                    HOME_CARDS[index].color,
                    -1.0,
                    0,
                );
                core::ptr::addr_of_mut!(HINT_READY)
                    .cast::<bool>()
                    .add(index)
                    .write(hint_drawn != 0);
                if hint_drawn != 0 {
                    hints_drawn += 1;
                }
            }
        }
        let used = super::HEAP_USED.load(Ordering::Relaxed);
        debug_println!(
            "SLINT SKIA LABEL {} TITLES {} HOME {} HINT {} HEAP {}",
            drawn,
            titles_drawn,
            home_drawn,
            hints_drawn,
            used
        );
        drawn != 0
    }
}

/// Returns true when the title band was written (caller should damage it).
pub fn blit_skia_chrome(frame: &mut [u8], page: i32) -> bool {
    // Slint has already painted this frame. Only clear the band on pages
    // whose title Skia owns. NexaWal draws Home and its tab title here.
    if !(1..=TITLES.len() as i32).contains(&page) {
        return false;
    }
    let index = (page - 1) as usize;
    let origin_x = px(17.0);
    let origin_y = px(51.0);
    let ready = unsafe { core::ptr::addr_of!(TITLE_READY).cast::<bool>().add(index).read() };
    let pixels = unsafe {
        core::ptr::addr_of!(TITLE_PIXELS)
            .cast::<u8>()
            .add(index * TITLE_BYTES)
    };
    fill_rect(frame, origin_x, origin_y, TITLE_WIDTH as usize, TITLE_HEIGHT as usize,
              0x05, 0x0a, 0x12);
    blit_sprite(
        frame,
        pixels,
        TITLE_WIDTH as usize,
        TITLE_HEIGHT as usize,
        origin_x,
        origin_y,
        ready,
    );
    true
}

pub fn chat_count() -> usize {
    unsafe { core::ptr::addr_of!(CHAT_COUNT).read() }
}

pub fn set_chat_names(names: &[&str]) {
    unsafe {
        let count = names.len().min(CHAT_SLOTS);
        core::ptr::addr_of_mut!(CHAT_COUNT).write(count);
        for index in 0..CHAT_SLOTS {
            let bytes = names.get(index).map(|name| name.as_bytes()).unwrap_or(b"");
            store_chat_text(
                index,
                bytes,
                core::ptr::addr_of_mut!(CHAT_NAME_KEY).cast::<u8>(),
                core::ptr::addr_of_mut!(CHAT_NAME_LEN).cast::<usize>(),
                core::ptr::addr_of_mut!(CHAT_NAME_DIRTY).cast::<bool>(),
                CHAT_TEXT_CAP,
            );
            let mut initial = [0u8; 4];
            let initial_len = initial_bytes(bytes, &mut initial);
            store_chat_text(
                index,
                &initial[..initial_len],
                core::ptr::addr_of_mut!(CHAT_INITIAL_KEY).cast::<u8>(),
                core::ptr::addr_of_mut!(CHAT_INITIAL_LEN).cast::<usize>(),
                core::ptr::addr_of_mut!(CHAT_INITIAL_DIRTY).cast::<bool>(),
                4,
            );
        }
    }
}

pub fn note_chat_name(index: usize, name: &[u8]) {
    if index >= CHAT_SLOTS {
        return;
    }
    unsafe {
        store_chat_text(
            index,
            name,
            core::ptr::addr_of_mut!(CHAT_NAME_KEY).cast::<u8>(),
            core::ptr::addr_of_mut!(CHAT_NAME_LEN).cast::<usize>(),
            core::ptr::addr_of_mut!(CHAT_NAME_DIRTY).cast::<bool>(),
            CHAT_TEXT_CAP,
        );
        let mut initial = [0u8; 4];
        let initial_len = initial_bytes(name, &mut initial);
        store_chat_text(
            index,
            &initial[..initial_len],
            core::ptr::addr_of_mut!(CHAT_INITIAL_KEY).cast::<u8>(),
            core::ptr::addr_of_mut!(CHAT_INITIAL_LEN).cast::<usize>(),
            core::ptr::addr_of_mut!(CHAT_INITIAL_DIRTY).cast::<bool>(),
            4,
        );
    }
}

pub fn note_chat_time(index: usize, time: &[u8]) {
    if index >= CHAT_SLOTS {
        return;
    }
    unsafe {
        store_chat_text(
            index,
            time,
            core::ptr::addr_of_mut!(CHAT_TIME_KEY).cast::<u8>(),
            core::ptr::addr_of_mut!(CHAT_TIME_LEN).cast::<usize>(),
            core::ptr::addr_of_mut!(CHAT_TIME_DIRTY).cast::<bool>(),
            8,
        );
    }
}

/// Paint a contact portrait into the round avatar slot. Official apps show the
/// profile image when the peer sent one; otherwise the initial letter stays.
pub fn note_chat_photo(index: usize, rgba: &[u8], width: usize, height: usize) {
    if index >= CHAT_SLOTS || width == 0 || height == 0 {
        return;
    }
    if rgba.len() < width.saturating_mul(height).saturating_mul(4) {
        return;
    }
    let out_w = CHAT_INITIAL_WIDTH as usize;
    let out_h = CHAT_INITIAL_HEIGHT as usize;
    let cx = (out_w as i32) / 2;
    let cy = (out_h as i32) / 2;
    let radius = cx.min(cy) - 1;
    unsafe {
        let pixels = core::ptr::addr_of_mut!(CHAT_INITIAL_PIXELS)
            .cast::<u8>()
            .add(index * CHAT_INITIAL_BYTES);
        for y in 0..out_h {
            for x in 0..out_w {
                let dx = x as i32 - cx;
                let dy = y as i32 - cy;
                let dest = (y * out_w + x) * 4;
                if dx * dx + dy * dy > radius * radius {
                    pixels.add(dest).write(0);
                    pixels.add(dest + 1).write(0);
                    pixels.add(dest + 2).write(0);
                    pixels.add(dest + 3).write(0);
                    continue;
                }
                let src_x = x * width / out_w;
                let src_y = y * height / out_h;
                let src = (src_y * width + src_x) * 4;
                pixels.add(dest).write(rgba[src]);
                pixels.add(dest + 1).write(rgba[src + 1]);
                pixels.add(dest + 2).write(rgba[src + 2]);
                pixels.add(dest + 3).write(255);
            }
        }
        core::ptr::addr_of_mut!(CHAT_HAS_PHOTO)
            .cast::<bool>()
            .add(index)
            .write(true);
        core::ptr::addr_of_mut!(CHAT_INITIAL_READY)
            .cast::<bool>()
            .add(index)
            .write(true);
        core::ptr::addr_of_mut!(CHAT_INITIAL_DIRTY)
            .cast::<bool>()
            .add(index)
            .write(false);
        core::ptr::addr_of_mut!(CHAT_NAME_DIRTY)
            .cast::<bool>()
            .add(index)
            .write(true);
    }
}

pub fn note_chat_unread(index: usize, count: usize) {
    if index >= CHAT_SLOTS {
        return;
    }
    let shown = count.min(10) as u8;
    unsafe {
        let slot = core::ptr::addr_of_mut!(CHAT_UNREAD).cast::<u8>().add(index);
        if slot.read() == shown {
            return;
        }
        slot.write(shown);
        core::ptr::addr_of_mut!(CHAT_UNREAD_DIRTY)
            .cast::<bool>()
            .add(index)
            .write(true);
    }
}

pub fn note_chat_preview(index: usize, preview: &[u8]) {
    if index >= CHAT_SLOTS {
        return;
    }
    unsafe {
        store_chat_text(
            index,
            preview,
            core::ptr::addr_of_mut!(CHAT_PREVIEW_KEY).cast::<u8>(),
            core::ptr::addr_of_mut!(CHAT_PREVIEW_LEN).cast::<usize>(),
            core::ptr::addr_of_mut!(CHAT_PREVIEW_DIRTY).cast::<bool>(),
            CHAT_TEXT_CAP,
        );
    }
}

pub fn flush_chat_sprites() {
    unsafe {
        if !core::ptr::addr_of!(LABEL_READY).read() {
            return;
        }
        let mut drawn = 0;
        for index in 0..CHAT_SLOTS {
            if core::ptr::addr_of!(CHAT_NAME_DIRTY).cast::<bool>().add(index).read() {
                let ready = draw_stored(
                    core::ptr::addr_of_mut!(CHAT_NAME_PIXELS).cast::<u8>().add(index * CHAT_NAME_BYTES),
                    CHAT_NAME_WIDTH,
                    CHAT_NAME_HEIGHT,
                    core::ptr::addr_of!(CHAT_NAME_KEY).cast::<u8>().add(index * CHAT_TEXT_CAP),
                    core::ptr::addr_of!(CHAT_NAME_LEN).cast::<usize>().add(index).read(),
                    CHAT_NAME_SIZE,
                    0xFFE8_F5FB,
                    1,
                );
                core::ptr::addr_of_mut!(CHAT_NAME_READY).cast::<bool>().add(index).write(ready);
                core::ptr::addr_of_mut!(CHAT_NAME_DIRTY).cast::<bool>().add(index).write(false);
                if ready {
                    drawn += 1;
                }
            }
            if core::ptr::addr_of!(CHAT_PREVIEW_DIRTY).cast::<bool>().add(index).read() {
                let ready = draw_stored(
                    core::ptr::addr_of_mut!(CHAT_PREVIEW_PIXELS).cast::<u8>().add(index * CHAT_PREVIEW_BYTES),
                    CHAT_PREVIEW_WIDTH,
                    CHAT_PREVIEW_HEIGHT,
                    core::ptr::addr_of!(CHAT_PREVIEW_KEY).cast::<u8>().add(index * CHAT_TEXT_CAP),
                    core::ptr::addr_of!(CHAT_PREVIEW_LEN).cast::<usize>().add(index).read(),
                    CHAT_PREVIEW_SIZE,
                    0xFF8E_ACBB,
                    0,
                );
                core::ptr::addr_of_mut!(CHAT_PREVIEW_READY).cast::<bool>().add(index).write(ready);
                core::ptr::addr_of_mut!(CHAT_PREVIEW_DIRTY).cast::<bool>().add(index).write(false);
            }
            if core::ptr::addr_of!(CHAT_TIME_DIRTY).cast::<bool>().add(index).read() {
                let ready = draw_stored(
                    core::ptr::addr_of_mut!(CHAT_TIME_PIXELS).cast::<u8>().add(index * CHAT_TIME_BYTES),
                    CHAT_TIME_WIDTH,
                    CHAT_TIME_HEIGHT,
                    core::ptr::addr_of!(CHAT_TIME_KEY).cast::<u8>().add(index * 8),
                    core::ptr::addr_of!(CHAT_TIME_LEN).cast::<usize>().add(index).read(),
                    CHAT_TIME_SIZE,
                    0xFF82_99A7,
                    0,
                );
                core::ptr::addr_of_mut!(CHAT_TIME_READY).cast::<bool>().add(index).write(ready);
                core::ptr::addr_of_mut!(CHAT_TIME_DIRTY).cast::<bool>().add(index).write(false);
            }
            if core::ptr::addr_of!(CHAT_INITIAL_DIRTY).cast::<bool>().add(index).read() {
                if core::ptr::addr_of!(CHAT_HAS_PHOTO).cast::<bool>().add(index).read() {
                    core::ptr::addr_of_mut!(CHAT_INITIAL_DIRTY).cast::<bool>().add(index).write(false);
                } else {
                    let ready = draw_stored(
                        core::ptr::addr_of_mut!(CHAT_INITIAL_PIXELS).cast::<u8>().add(index * CHAT_INITIAL_BYTES),
                        CHAT_INITIAL_WIDTH,
                        CHAT_INITIAL_HEIGHT,
                        core::ptr::addr_of!(CHAT_INITIAL_KEY).cast::<u8>().add(index * 4),
                        core::ptr::addr_of!(CHAT_INITIAL_LEN).cast::<usize>().add(index).read(),
                        CHAT_INITIAL_SIZE,
                        0xFFFF_FFFF,
                        1,
                    );
                    core::ptr::addr_of_mut!(CHAT_INITIAL_READY).cast::<bool>().add(index).write(ready);
                    core::ptr::addr_of_mut!(CHAT_INITIAL_DIRTY).cast::<bool>().add(index).write(false);
                }
            }
            if core::ptr::addr_of!(CHAT_UNREAD_DIRTY).cast::<bool>().add(index).read() {
                let count = core::ptr::addr_of!(CHAT_UNREAD).cast::<u8>().add(index).read();
                let ready = if count == 0 {
                    false
                } else {
                    paint_unread_badge(
                        core::ptr::addr_of_mut!(CHAT_BADGE_PIXELS)
                            .cast::<u8>()
                            .add(index * CHAT_BADGE_BYTES),
                        count,
                    );
                    true
                };
                core::ptr::addr_of_mut!(CHAT_BADGE_READY).cast::<bool>().add(index).write(ready);
                core::ptr::addr_of_mut!(CHAT_UNREAD_DIRTY).cast::<bool>().add(index).write(false);
            }
        }
        if drawn != 0 {
            debug_println!("SLINT SKIA CHATS {}", drawn);
        }
    }
}

#[cfg(feature = "utm-chat-sync-selftest")]
pub fn test_chat_name_ready(index: usize) -> bool {
    index < CHAT_SLOTS && unsafe { core::ptr::addr_of!(CHAT_NAME_READY).cast::<bool>().add(index).read() }
}

pub fn blit_open_chat_initial(frame: &mut [u8], index: usize) {
    if index >= CHAT_SLOTS {
        return;
    }
    let badge = px_i(40.0);
    let badge_x = px_i(25.0);
    let badge_y = px_i(184.0);
    blit_sprite_clipped(
        frame,
        unsafe {
            core::ptr::addr_of!(CHAT_INITIAL_PIXELS)
                .cast::<u8>()
                .add(index * CHAT_INITIAL_BYTES)
        },
        CHAT_INITIAL_WIDTH as usize,
        CHAT_INITIAL_HEIGHT as usize,
        badge_x + (badge - CHAT_INITIAL_WIDTH) / 2,
        badge_y + (badge - CHAT_INITIAL_HEIGHT) / 2,
        unsafe {
            core::ptr::addr_of!(CHAT_INITIAL_READY)
                .cast::<bool>()
                .add(index)
                .read()
        },
        0,
        super::HEIGHT as i32,
    );
}

pub fn chat_dirty_mask() -> u16 {
    let mut mask = 0u16;
    unsafe {
        for index in 0..CHAT_SLOTS {
            let dirty = core::ptr::addr_of!(CHAT_NAME_DIRTY).cast::<bool>().add(index).read()
                || core::ptr::addr_of!(CHAT_PREVIEW_DIRTY).cast::<bool>().add(index).read()
                || core::ptr::addr_of!(CHAT_TIME_DIRTY).cast::<bool>().add(index).read()
                || core::ptr::addr_of!(CHAT_INITIAL_DIRTY).cast::<bool>().add(index).read()
                || core::ptr::addr_of!(CHAT_UNREAD_DIRTY).cast::<bool>().add(index).read();
            if dirty {
                mask |= 1 << index;
            }
        }
    }
    mask
}

static mut LAST_CHROME_PAGE: i32 = -1;
static mut LAST_OPEN_CHAT: i32 = -2;

pub fn take_chrome_page(page: i32) -> bool {
    unsafe {
        let changed = LAST_CHROME_PAGE != page;
        LAST_CHROME_PAGE = page;
        changed
    }
}

pub fn invalidate_chrome() {
    unsafe {
        LAST_CHROME_PAGE = -1;
    }
}

pub fn take_open_chat(index: i32) -> bool {
    unsafe {
        let changed = LAST_OPEN_CHAT != index;
        LAST_OPEN_CHAT = index;
        changed
    }
}

pub fn open_chat_badge_band() -> (u32, u32) {
    (px(176.0) as u32, px(70.0) as u32)
}

pub fn page_chrome_band(page: i32) -> (u32, u32) {
    if page == 0 {
        let top = px(110.0) as u32;
        let bottom = px(760.0) as u32;
        (top, bottom.saturating_sub(top))
    } else {
        (px(51.0) as u32, TITLE_HEIGHT as u32)
    }
}

pub fn blit_chat_rows(
    frame: &mut [u8],
    scroll_logical: f32,
    viewport_height: f32,
    count: usize,
    dirty_mask: u16,
    damage_y: i32,
    damage_bottom: i32,
) -> (i32, i32) {
    // The chat list is nested at y=210 with its viewport at y=48.
    // Clip every sprite and its background to the visible Slint rows.
    let viewport_top = px_i(258.0);
    let viewport_bottom = px_i(258.0 + viewport_height);
    let mut painted_top = 0i32;
    let mut painted_bottom = 0i32;
    for index in 0..count.min(CHAT_SLOTS) {
        let row_top = px_i(258.0 + index as f32 * 72.0 - scroll_logical);
        let row_bottom = row_top + px_i(72.0);
        if row_bottom <= viewport_top || row_top >= viewport_bottom {
            continue;
        }
        let overlaps = damage_bottom > damage_y && row_bottom > damage_y && row_top < damage_bottom;
        let dirty = dirty_mask & (1u16 << index) != 0;
        if !overlaps && !dirty {
            continue;
        }
        let badge_x = px_i(29.0);
        let badge_y = row_top + px_i(14.0);
        let badge = px_i(42.0);
        let name_x = px_i(81.0);
        let name_y = row_top + px_i(8.0);
        let preview_y = row_top + px_i(36.0);
        let time_x = px_i(340.0);
        let time_y = row_top + px_i(10.0);
        fill_clipped(frame, name_x, name_y, CHAT_NAME_WIDTH, CHAT_NAME_HEIGHT, viewport_top, viewport_bottom, 0x05, 0x0a, 0x12);
        fill_clipped(frame, name_x, preview_y, CHAT_PREVIEW_WIDTH, CHAT_PREVIEW_HEIGHT, viewport_top, viewport_bottom, 0x05, 0x0a, 0x12);
        fill_clipped(frame, time_x, time_y, CHAT_TIME_WIDTH, CHAT_TIME_HEIGHT, viewport_top, viewport_bottom, 0x05, 0x0a, 0x12);
        blit_sprite_clipped(
            frame,
            unsafe { core::ptr::addr_of!(CHAT_INITIAL_PIXELS).cast::<u8>().add(index * CHAT_INITIAL_BYTES) },
            CHAT_INITIAL_WIDTH as usize,
            CHAT_INITIAL_HEIGHT as usize,
            badge_x + (badge - CHAT_INITIAL_WIDTH) / 2,
            badge_y + (badge - CHAT_INITIAL_HEIGHT) / 2,
            unsafe { core::ptr::addr_of!(CHAT_INITIAL_READY).cast::<bool>().add(index).read() },
            viewport_top,
            viewport_bottom,
        );
        blit_sprite_clipped(
            frame,
            unsafe { core::ptr::addr_of!(CHAT_NAME_PIXELS).cast::<u8>().add(index * CHAT_NAME_BYTES) },
            CHAT_NAME_WIDTH as usize,
            CHAT_NAME_HEIGHT as usize,
            px_i(81.0),
            row_top + px_i(8.0),
            unsafe { core::ptr::addr_of!(CHAT_NAME_READY).cast::<bool>().add(index).read() },
            viewport_top,
            viewport_bottom,
        );
        blit_sprite_clipped(
            frame,
            unsafe { core::ptr::addr_of!(CHAT_PREVIEW_PIXELS).cast::<u8>().add(index * CHAT_PREVIEW_BYTES) },
            CHAT_PREVIEW_WIDTH as usize,
            CHAT_PREVIEW_HEIGHT as usize,
            px_i(81.0),
            row_top + px_i(36.0),
            unsafe { core::ptr::addr_of!(CHAT_PREVIEW_READY).cast::<bool>().add(index).read() },
            viewport_top,
            viewport_bottom,
        );
        blit_sprite_clipped(
            frame,
            unsafe { core::ptr::addr_of!(CHAT_TIME_PIXELS).cast::<u8>().add(index * CHAT_TIME_BYTES) },
            CHAT_TIME_WIDTH as usize,
            CHAT_TIME_HEIGHT as usize,
            time_x,
            time_y,
            unsafe { core::ptr::addr_of!(CHAT_TIME_READY).cast::<bool>().add(index).read() },
            viewport_top,
            viewport_bottom,
        );
        let unread_x = px_i(348.0);
        let unread_y = row_top + px_i(34.0);
        fill_clipped(
            frame,
            px_i(344.0),
            row_top + px_i(32.0),
            px_i(32.0),
            px_i(24.0),
            viewport_top,
            viewport_bottom,
            0x05,
            0x0a,
            0x12,
        );
        blit_sprite_clipped(
            frame,
            unsafe {
                core::ptr::addr_of!(CHAT_BADGE_PIXELS)
                    .cast::<u8>()
                    .add(index * CHAT_BADGE_BYTES)
            },
            CHAT_BADGE_SIZE as usize,
            CHAT_BADGE_SIZE as usize,
            unread_x,
            unread_y,
            unsafe { core::ptr::addr_of!(CHAT_BADGE_READY).cast::<bool>().add(index).read() },
            viewport_top,
            viewport_bottom,
        );
        let top = row_top.max(viewport_top).max(0);
        let bottom = row_bottom.min(viewport_bottom);
        if painted_bottom <= painted_top {
            painted_top = top;
            painted_bottom = bottom;
        } else {
            painted_top = painted_top.min(top);
            painted_bottom = painted_bottom.max(bottom);
        }
    }
    (painted_top, painted_bottom)
}

fn glyph_rows(byte: u8) -> [u8; 7] {
    match byte {
        b'0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        b'1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        b'3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        b'4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        b'5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        b'6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        b'7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        b'8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        b'9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        b'+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        _ => [0; 7],
    }
}

fn paint_unread_badge(pixels: *mut u8, count: u8) {
    let size = CHAT_BADGE_SIZE;
    let digit = [b'0' + count.min(9)];
    let text: &[u8] = if count >= 10 { b"9+" } else { &digit };
    unsafe {
        for offset in 0..CHAT_BADGE_BYTES {
            pixels.add(offset).write(0);
        }
    }
    let scale: i32 = if text.len() == 1 { 3 } else { 2 };
    let glyph_w = 5 * scale;
    let glyph_h = 7 * scale;
    let gap = scale;
    let text_w = glyph_w * text.len() as i32 + gap * (text.len() as i32 - 1);
    let origin_x = (size - text_w) / 2;
    let origin_y = (size - glyph_h) / 2;
    let center = (size - 1) as f32 / 2.0;
    let radius = center - 0.5;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = libm::sqrtf(dx * dx + dy * dy);
            if dist > radius + 0.8 {
                continue;
            }
            let edge = if dist > radius {
                ((radius + 0.8 - dist) / 0.8 * 255.0) as u8
            } else {
                255
            };
            let mut ink = false;
            let mut cursor = origin_x;
            for &byte in text {
                let rows = glyph_rows(byte);
                let local_x = x - cursor;
                let local_y = y - origin_y;
                if local_x >= 0 && local_x < glyph_w && local_y >= 0 && local_y < glyph_h {
                    let column = (local_x / scale) as usize;
                    let row = (local_y / scale) as usize;
                    if column < 5 && rows[row] & (1 << (4 - column)) != 0 {
                        ink = true;
                    }
                }
                cursor += glyph_w + gap;
            }
            let (red, green, blue) = if ink {
                (0xff, 0xff, 0xff)
            } else {
                (0x28, 0xd8, 0xe8)
            };
            let offset = ((y * size + x) * 4) as usize;
            unsafe {
                pixels.add(offset).write(red);
                pixels.add(offset + 1).write(green);
                pixels.add(offset + 2).write(blue);
                pixels.add(offset + 3).write(edge);
            }
        }
    }
}

fn fill_clipped(
    frame: &mut [u8],
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    clip_top: i32,
    clip_bottom: i32,
    red: u8,
    green: u8,
    blue: u8,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    let stride = super::FRAME_STRIDE_BYTES as usize;
    for row in 0..height {
        let frame_y = y + row;
        if frame_y < clip_top || frame_y >= clip_bottom || frame_y < 0 {
            continue;
        }
        let frame_y = frame_y as usize;
        if frame_y >= super::HEIGHT {
            break;
        }
        for column in 0..width {
            let frame_x = x + column;
            if frame_x < 0 {
                continue;
            }
            let frame_x = frame_x as usize;
            if frame_x >= super::WIDTH {
                break;
            }
            let dst = frame_y * stride + frame_x * 3;
            if dst + 2 >= frame.len() {
                continue;
            }
            frame[dst] = red;
            frame[dst + 1] = green;
            frame[dst + 2] = blue;
        }
    }
}

fn store_chat_text(
    index: usize,
    bytes: &[u8],
    keys: *mut u8,
    lengths: *mut usize,
    dirty: *mut bool,
    cap: usize,
) {
    let capped = bytes.len().min(cap);
    let key = unsafe { keys.add(index * cap) };
    let same_len = unsafe { lengths.add(index).read() } == capped;
    let mut same = same_len;
    if same {
        for offset in 0..capped {
            if unsafe { key.add(offset).read() } != bytes[offset] {
                same = false;
                break;
            }
        }
    }
    if same {
        return;
    }
    for offset in 0..capped {
        unsafe { key.add(offset).write(bytes[offset]) };
    }
    unsafe {
        lengths.add(index).write(capped);
        dirty.add(index).write(true);
    }
}

fn initial_bytes(name: &[u8], out: &mut [u8; 4]) -> usize {
    if name.is_empty() {
        return 0;
    }
    let width = match name[0] {
        0..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    };
    let width = width.min(name.len()).min(4);
    if width == 1 && name[0].is_ascii_lowercase() {
        out[0] = name[0].to_ascii_uppercase();
    } else {
        out[..width].copy_from_slice(&name[..width]);
    }
    width
}

fn draw_stored(
    pixels: *mut u8,
    width: i32,
    height: i32,
    text: *const u8,
    text_len: usize,
    size: f32,
    color: u32,
    embolden: i32,
) -> bool {
    if text_len == 0 {
        return false;
    }
    let drawn = unsafe {
        skia_draw_text(
            pixels,
            width,
            height,
            width * 4,
            text,
            text_len as i32,
            size,
            color,
            -1.0,
            embolden,
        )
    };
    drawn != 0
}

fn blit_sprite_clipped(
    frame: &mut [u8],
    pixels: *const u8,
    width: usize,
    height: usize,
    origin_x: i32,
    origin_y: i32,
    ready: bool,
    clip_top: i32,
    clip_bottom: i32,
) {
    if !ready {
        return;
    }
    let stride = super::FRAME_STRIDE_BYTES as usize;
    for row in 0..height {
        for column in 0..width {
            let frame_x = origin_x + column as i32;
            let frame_y = origin_y + row as i32;
            if frame_x < 0 || frame_y < clip_top || frame_y >= clip_bottom {
                continue;
            }
            let frame_x = frame_x as usize;
            let frame_y = frame_y as usize;
            if frame_x >= super::WIDTH || frame_y >= super::HEIGHT {
                continue;
            }
            let src = (row * width + column) * 4;
            let alpha = unsafe { pixels.add(src + 3).read() } as u16;
            if alpha == 0 {
                continue;
            }
            let dst = frame_y * stride + frame_x * 3;
            if dst + 2 >= frame.len() {
                continue;
            }
            let inverse = 255 - alpha;
            for channel in 0..3 {
                let blended = unsafe { pixels.add(src + channel).read() } as u16
                    + (frame[dst + channel] as u16 * inverse) / 255;
                frame[dst + channel] = blended.min(255) as u8;
            }
        }
    }
}

fn fill_rect(frame: &mut [u8], x: usize, y: usize, width: usize, height: usize, r: u8, g: u8, b: u8) {
    let stride = super::FRAME_STRIDE_BYTES as usize;
    for row in 0..height {
        let frame_y = y + row;
        if frame_y >= super::HEIGHT {
            break;
        }
        for column in 0..width {
            let frame_x = x + column;
            if frame_x >= super::WIDTH {
                break;
            }
            let dst = frame_y * stride + frame_x * 3;
            if dst + 2 >= frame.len() {
                continue;
            }
            frame[dst] = r;
            frame[dst + 1] = g;
            frame[dst + 2] = b;
        }
    }
}

fn blit_sprite(
    frame: &mut [u8],
    pixels: *const u8,
    width: usize,
    height: usize,
    origin_x: usize,
    origin_y: usize,
    ready: bool,
) {
    if !ready {
        return;
    }
    let stride = super::FRAME_STRIDE_BYTES as usize;
    for row in 0..height {
        for column in 0..width {
            let src = (row * width + column) * 4;
            let alpha = unsafe { pixels.add(src + 3).read() } as u16;
            if alpha == 0 {
                continue;
            }
            let frame_x = origin_x + column;
            let frame_y = origin_y + row;
            if frame_x >= super::WIDTH || frame_y >= super::HEIGHT {
                continue;
            }
            let dst = frame_y * stride + frame_x * 3;
            if dst + 2 >= frame.len() {
                continue;
            }
            let inverse = 255 - alpha;
            for channel in 0..3 {
                let blended = unsafe { pixels.add(src + channel).read() } as u16
                    + (frame[dst + channel] as u16 * inverse) / 255;
                frame[dst + channel] = blended.min(255) as u8;
            }
        }
    }
}
