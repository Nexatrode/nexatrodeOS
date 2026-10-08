use std::{env, fs, io::Write};

const WIDTH: usize = 1080;
const HEIGHT: usize = 1920;
const PPM_HEADER: &[u8] = b"P6\n1080 1920\n255\n";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let source = args.next().ok_or("usage: encode_snapshot SOURCE.ppm DEST.rle")?;
    let destination = args.next().ok_or("usage: encode_snapshot SOURCE.ppm DEST.rle")?;
    if args.next().is_some() {
        return Err("usage: encode_snapshot SOURCE.ppm DEST.rle".into());
    }
    let ppm = fs::read(source)?;
    let rgb = ppm.strip_prefix(PPM_HEADER).ok_or("unexpected PPM header")?;
    if rgb.len() != WIDTH * HEIGHT * 3 {
        return Err("unexpected PPM pixel count".into());
    }

    let mut encoded = Vec::new();
    let mut total_runs = 0usize;
    for row in 0..HEIGHT {
        let run_count_offset = encoded.len();
        encoded.extend_from_slice(&0u16.to_le_bytes());
        let mut run_count = 0u16;
        let mut column = 0usize;
        while column < WIDTH {
            let index = (row * WIDTH + column) * 3;
            let color = ((rgb[index] as u32) << 24)
                | ((rgb[index + 1] as u32) << 16)
                | ((rgb[index + 2] as u32) << 8)
                | 0xff;
            let mut length = 1usize;
            while column + length < WIDTH {
                let next = (row * WIDTH + column + length) * 3;
                let next_color = ((rgb[next] as u32) << 24)
                    | ((rgb[next + 1] as u32) << 16)
                    | ((rgb[next + 2] as u32) << 8)
                    | 0xff;
                if next_color != color { break; }
                length += 1;
            }
            encoded.extend_from_slice(&(length as u16).to_le_bytes());
            encoded.extend_from_slice(&color.to_le_bytes());
            run_count += 1;
            column += length;
        }
        encoded[run_count_offset..run_count_offset + 2]
            .copy_from_slice(&run_count.to_le_bytes());
        total_runs += run_count as usize;
    }

    // Validate the serialized format, not just the runs built in memory.
    let mut decoded = Vec::with_capacity(WIDTH * HEIGHT);
    let mut offset = 0usize;
    for _row in 0..HEIGHT {
        let count = u16::from_le_bytes([encoded[offset], encoded[offset + 1]]) as usize;
        offset += 2;
        let mut column = 0usize;
        for _ in 0..count {
            let length = u16::from_le_bytes([encoded[offset], encoded[offset + 1]]) as usize;
            let color = u32::from_le_bytes([
                encoded[offset + 2], encoded[offset + 3],
                encoded[offset + 4], encoded[offset + 5],
            ]);
            offset += 6;
            decoded.extend(std::iter::repeat_n(color, length));
            column += length;
        }
        if column != WIDTH { return Err("RLE decoded row width mismatch".into()); }
    }
    if offset != encoded.len() { return Err("RLE trailing bytes".into()); }
    for (pixel, source) in decoded.iter().zip(rgb.chunks_exact(3)) {
        if pixel.to_be_bytes() != [source[0], source[1], source[2], 0xff] {
            return Err("RLE round trip failed".into());
        }
    }
    if decoded.len() != WIDTH * HEIGHT {
        return Err("RLE decoded pixel count mismatch".into());
    }
    let mut output = fs::File::create(destination)?;
    output.write_all(&encoded)?;
    println!("encoded {total_runs} runs into {} bytes", encoded.len());
    Ok(())
}
