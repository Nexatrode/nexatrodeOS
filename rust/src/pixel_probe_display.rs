//! Small two-buffer text report for native Pixel bring-up probes.

const WIDTH: usize = 1080;
const HEIGHT: usize = 2340;
const PIXELS: usize = WIDTH * HEIGHT;
const SCALE: usize = 7;
const FONT: [[u8; 7]; 36] = [
    [14, 17, 17, 31, 17, 17, 17],
    [30, 17, 17, 30, 17, 17, 30],
    [14, 17, 16, 16, 16, 17, 14],
    [30, 17, 17, 17, 17, 17, 30],
    [31, 16, 16, 30, 16, 16, 31],
    [31, 16, 16, 30, 16, 16, 16],
    [14, 17, 16, 23, 17, 17, 15],
    [17, 17, 17, 31, 17, 17, 17],
    [14, 4, 4, 4, 4, 4, 14],
    [7, 2, 2, 2, 18, 18, 12],
    [17, 18, 20, 24, 20, 18, 17],
    [16, 16, 16, 16, 16, 16, 31],
    [17, 27, 21, 21, 17, 17, 17],
    [17, 25, 21, 19, 17, 17, 17],
    [14, 17, 17, 17, 17, 17, 14],
    [30, 17, 17, 30, 16, 16, 16],
    [14, 17, 17, 17, 21, 18, 13],
    [30, 17, 17, 30, 20, 18, 17],
    [15, 16, 16, 14, 1, 1, 30],
    [31, 4, 4, 4, 4, 4, 4],
    [17, 17, 17, 17, 17, 17, 14],
    [17, 17, 17, 17, 17, 10, 4],
    [17, 17, 17, 21, 21, 21, 10],
    [17, 17, 10, 4, 10, 17, 17],
    [17, 17, 10, 4, 4, 4, 4],
    [31, 1, 2, 4, 8, 16, 31],
    [14, 17, 19, 21, 25, 17, 14],
    [4, 12, 4, 4, 4, 4, 14],
    [14, 17, 1, 2, 4, 8, 31],
    [30, 1, 1, 14, 1, 1, 30],
    [2, 6, 10, 18, 31, 2, 2],
    [31, 16, 16, 30, 1, 1, 30],
    [14, 16, 16, 30, 17, 17, 14],
    [31, 1, 2, 4, 8, 8, 8],
    [14, 17, 17, 14, 17, 17, 14],
    [14, 17, 17, 15, 1, 1, 14],
];

pub struct Display {
    pixels: *mut u32,
}

impl Display {
    /// # Safety
    /// `pixels` must map at least two contiguous 1080x2340 XRGB8888 buffers
    /// with device-write permission for the lifetime of the returned display.
    pub unsafe fn new(pixels: *mut u32) -> Self {
        Self { pixels }
    }

    pub(crate) fn pixels_ptr(&self) -> *mut u32 {
        self.pixels
    }

    pub fn clear(&self) {
        for index in 0..2 * PIXELS {
            unsafe { self.pixels.add(index).write_volatile(0x00122b52) };
        }
    }

    pub fn line(&self, row: usize, text: &str) {
        self.line_scaled(row, text, SCALE, 0);
    }

    fn line_scaled(&self, row: usize, text: &str, scale: usize, offset: usize) {
        let y = 180 + row * 72 + offset;
        let color = if row % 2 == 0 { 0x0000e5e5 } else { 0x00f4f7ff };
        for buffer in 0..2 {
            for (column, byte) in text.bytes().enumerate() {
                let glyph = match byte {
                    b'A'..=b'Z' => usize::from(byte - b'A'),
                    b'0'..=b'9' => 26 + usize::from(byte - b'0'),
                    _ => continue,
                };
                let x = 48 + column * 6 * scale;
                if x + 5 * scale > WIDTH || y + 7 * scale > HEIGHT {
                    break;
                }
                for gy in 0..7 {
                    for gx in 0..5 {
                        if FONT[glyph][gy] & (1 << (4 - gx)) == 0 {
                            continue;
                        }
                        for dy in 0..scale {
                            for dx in 0..scale {
                                let index = buffer * PIXELS
                                    + (y + gy * scale + dy) * WIDTH
                                    + x
                                    + gx * scale
                                    + dx;
                                unsafe { self.pixels.add(index).write_volatile(color) };
                            }
                        }
                    }
                }
            }
        }
    }

    /// Replace one text row, including any longer text previously drawn there.
    /// This is useful for live progress markers whose labels change while a
    /// diagnostic is running; `line` intentionally only paints glyph pixels.
    pub fn replace_line(&self, row: usize, text: &str) {
        let y = 180 + row * 72;
        if y >= HEIGHT {
            return;
        }
        let end_y = (y + 72).min(HEIGHT);
        for buffer in 0..2 {
            for row_y in y..end_y {
                for x in 0..WIDTH {
                    let index = buffer * PIXELS + row_y * WIDTH + x;
                    unsafe { self.pixels.add(index).write_volatile(0x00122b52) };
                }
            }
        }
        self.line(row, text);
    }

    #[cfg(feature="sunfish-wrq-post-return-callers")]
    pub fn header_label(&self,row:usize,text:&str) {
        self.replace_line(row,"");
        self.line_scaled(row,text,3,0);
    }

    #[cfg(feature = "sunfish-ipa-registration-reply-headers")]
    pub fn header_row(&self, row: usize, second: bool, fields: &[(u32, usize)]) {
        let mut line = [b' '; 64];
        let mut end = 0;
        for &(value, width) in fields {
            for index in 0..width {
                let digit = (value >> (4 * (width - 1 - index))) as usize & 0xf;
                line[end + index] = b"0123456789ABCDEF"[digit];
            }
            end += width + 1;
        }
        if !second {
            self.replace_line(row, "");
        }
        if let Ok(text) = core::str::from_utf8(&line[..end]) {
            self.line_scaled(row, text, 3, if second { 27 } else { 0 });
        }
    }

    /// Four bounded one-line wire records per standard row, smaller font.
    #[cfg(feature="sunfish-registry-tx3-wire-headers")]
    pub fn wire_row(&self,row:usize,lane:usize,fields:&[(u32,usize)]) {
        if lane>=4{return;}
        let mut line=[b' ';96];let mut end=0;
        for &(value,width) in fields {
            if width==0 || width>8 || end+width+1>line.len(){return;}
            for index in 0..width {let digit=(value>>(4*(width-1-index)))as usize&15;line[end+index]=b"0123456789ABCDEF"[digit];}
            end+=width+1;
        }
        if lane==0{self.replace_line(row,"");}
        if let Ok(text)=core::str::from_utf8(&line[..end]){self.line_scaled(row,text,2,lane*18);}
    }

    pub fn hex(&self, row: usize, label: &str, value: u32) {
        let mut line = [b' '; 24];
        let label = label.as_bytes();
        if label.len() > line.len() - 9 {
            return;
        }
        line[..label.len()].copy_from_slice(label);
        for index in 0..8 {
            let digit = (value >> (28 - 4 * index)) as usize & 0xf;
            line[label.len() + 1 + index] = b"0123456789ABCDEF"[digit];
        }
        if let Ok(text) = core::str::from_utf8(&line[..label.len() + 9]) {
            self.replace_line(row, text);
        }
    }

    /// Dedicated event panels own rows 2..28 even between full redraws.
    pub fn runtime_progress(&self, preserve_panel: bool, phase: u32, sent: u32, received: u32, chunks: u32) {
        if preserve_panel {
            self.hex(29, "SERVICE PHASE", phase);
        } else {
            self.hex(15, "CHECKPOINT", phase);
            self.hex(16, "WAKE TRY", sent);
            self.hex(17, "WAKE RECV", received);
            self.hex(18, "RMTFS CHUNKS", chunks);
        }
    }

    pub fn flush(&self) {
        unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
    }
}
