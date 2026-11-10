// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Just enough PNG for `frus mcp`'s screenshots: read one, make it smaller, write it again.
//!
//! A phone's screenshot is 1080 by 2340 pixels, a few megabytes in base64: more than an agent
//! needs to see what is on screen, and more than it should pay for. The tool has no
//! dependencies, so the decoder (8-bit, not interlaced, every colour type) and the encoder
//! (LZ77 with the fixed Huffman codes) are here, small and slow enough for a few screenshots.

/// An image, 8-bit RGBA, row after row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Reads a PNG.
pub(crate) fn decode(png: &[u8]) -> Result<Image, String> {
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("not a PNG".into());
    }
    let mut at = 8;
    let (mut width, mut height, mut depth, mut colour, mut interlace) = (0, 0, 0, 0, 0);
    let mut palette: &[u8] = &[];
    let mut alpha: &[u8] = &[];
    let mut data = Vec::new();
    while at + 8 <= png.len() {
        let length = u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
        let kind = &png[at + 4..at + 8];
        let body = png
            .get(at + 8..at + 8 + length)
            .ok_or("a PNG chunk runs past the end")?;
        match kind {
            b"IHDR" if length >= 13 => {
                width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
                depth = body[8];
                colour = body[9];
                interlace = body[12];
            }
            b"PLTE" => palette = body,
            b"tRNS" => alpha = body,
            b"IDAT" => data.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + length;
    }
    if depth != 8 || interlace != 0 {
        return Err(format!(
            "a PNG of {depth} bits{} is not read here",
            if interlace != 0 { ", interlaced," } else { "" }
        ));
    }
    let channels = match colour {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err(format!("PNG colour type {colour}")),
    };
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 64 << 20 {
        return Err("a PNG of no size, or too big".into());
    }
    let raw = inflate(data.get(2..).ok_or("no image data")?)?;
    let stride = width as usize * channels;
    if raw.len() < (stride + 1) * height as usize {
        return Err("the PNG's data is short".into());
    }
    let mut pixels = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        let filter = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (done, rest) = pixels.split_at_mut(y * stride);
        let above = if y == 0 {
            None
        } else {
            Some(&done[(y - 1) * stride..])
        };
        unfilter(filter, line, above, &mut rest[..stride], channels)?;
    }
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for px in pixels.chunks(channels) {
        match colour {
            0 => rgba.extend_from_slice(&[px[0], px[0], px[0], 255]),
            2 => rgba.extend_from_slice(&[px[0], px[1], px[2], 255]),
            3 => {
                let i = px[0] as usize;
                let rgb = palette.get(i * 3..i * 3 + 3).unwrap_or(&[0, 0, 0]);
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], *alpha.get(i).unwrap_or(&255)]);
            }
            4 => rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]),
            _ => rgba.extend_from_slice(px),
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

fn unfilter(
    filter: u8,
    line: &[u8],
    above: Option<&[u8]>,
    out: &mut [u8],
    bpp: usize,
) -> Result<(), String> {
    for i in 0..line.len() {
        let a = if i >= bpp { out[i - bpp] } else { 0 };
        let b = above.map_or(0, |row| row[i]);
        let c = if i >= bpp {
            above.map_or(0, |row| row[i - bpp])
        } else {
            0
        };
        out[i] = line[i].wrapping_add(match filter {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
            4 => paeth(a, b, c),
            _ => return Err(format!("PNG filter {filter}")),
        });
    }
    Ok(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// The image made smaller, if needed, so that neither side is over `max` pixels: each pixel of
/// the result is the average of the ones it covers.
pub(crate) fn shrink(image: &Image, max: u32) -> Image {
    let longest = image.width.max(image.height);
    if max == 0 || longest <= max {
        return image.clone();
    }
    let scale = f64::from(max) / f64::from(longest);
    let width = ((f64::from(image.width) * scale).round() as u32).max(1);
    let height = ((f64::from(image.height) * scale).round() as u32).max(1);
    let (sw, sh) = (image.width as usize, image.height as usize);
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        let y0 = y * sh / height as usize;
        let y1 = ((y + 1) * sh / height as usize).max(y0 + 1);
        for x in 0..width as usize {
            let x0 = x * sw / width as usize;
            let x1 = ((x + 1) * sw / width as usize).max(x0 + 1);
            let mut sum = [0u32; 4];
            for row in y0..y1 {
                for col in x0..x1 {
                    let at = (row * sw + col) * 4;
                    for (s, v) in sum.iter_mut().zip(&image.rgba[at..at + 4]) {
                        *s += u32::from(*v);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            rgba.extend(sum.iter().map(|s| ((s + n / 2) / n) as u8));
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

/// Writes the image as a PNG: RGB when every pixel is opaque, RGBA otherwise.
pub(crate) fn encode(image: &Image) -> Vec<u8> {
    let opaque = image.rgba.chunks(4).all(|px| px[3] == 255);
    let channels = if opaque { 3 } else { 4 };
    let stride = image.width as usize * channels;
    let pixels: Vec<u8> = if opaque {
        image
            .rgba
            .chunks(4)
            .flat_map(|px| [px[0], px[1], px[2]])
            .collect()
    } else {
        image.rgba.clone()
    };
    // Each row with the filter that leaves the smallest numbers, as most encoders choose.
    let mut raw = Vec::with_capacity((stride + 1) * image.height as usize);
    let mut candidate = vec![0u8; stride];
    let mut best = vec![0u8; stride];
    for y in 0..image.height as usize {
        let line = &pixels[y * stride..(y + 1) * stride];
        let above = (y > 0).then(|| &pixels[(y - 1) * stride..y * stride]);
        let mut best_filter = 0;
        let mut best_cost = u64::MAX;
        for filter in 0..5u8 {
            for i in 0..stride {
                let a = if i >= channels { line[i - channels] } else { 0 };
                let b = above.map_or(0, |row| row[i]);
                let c = if i >= channels {
                    above.map_or(0, |row| row[i - channels])
                } else {
                    0
                };
                candidate[i] = line[i].wrapping_sub(match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    _ => paeth(a, b, c),
                });
            }
            let cost: u64 = candidate
                .iter()
                .map(|&v| u64::from((v as i8).unsigned_abs()))
                .sum();
            if cost < best_cost {
                best_cost = cost;
                best_filter = filter;
                best.copy_from_slice(&candidate);
            }
        }
        raw.push(best_filter);
        raw.extend_from_slice(&best);
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&image.width.to_be_bytes());
    header.extend_from_slice(&image.height.to_be_bytes());
    header.extend_from_slice(&[8, if opaque { 2 } else { 6 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// A zlib stream of `data`: one deflate block with the fixed codes, its matches found through a
/// hash of three bytes.
fn zlib(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32 * 1024;
    const NONE: usize = usize::MAX;
    let mut bits = BitWriter::default();
    bits.out.extend_from_slice(&[0x78, 0x01]);
    bits.put(1, 1);
    bits.put(1, 2);
    let hash = |i: usize| {
        ((usize::from(data[i]) << 10) ^ (usize::from(data[i + 1]) << 5) ^ usize::from(data[i + 2]))
            & 0x7fff
    };
    let mut head = vec![NONE; 1 << 15];
    let mut prev = vec![NONE; data.len()];
    let insert = |i: usize, head: &mut [usize], prev: &mut [usize]| {
        if i + 2 < data.len() {
            let h = hash(i);
            prev[i] = head[h];
            head[h] = i;
        }
    };
    let mut i = 0;
    while i < data.len() {
        let (mut best, mut distance) = (0, 0);
        if i + 2 < data.len() {
            let longest = (data.len() - i).min(258);
            let mut candidate = head[hash(i)];
            let mut tries = 0;
            while candidate != NONE && i - candidate <= WINDOW && tries < 48 {
                let length = data[candidate..]
                    .iter()
                    .zip(&data[i..i + longest])
                    .take_while(|(a, b)| a == b)
                    .count();
                if length > best {
                    best = length;
                    distance = i - candidate;
                    if length == longest {
                        break;
                    }
                }
                candidate = prev[candidate];
                tries += 1;
            }
        }
        if best >= 3 {
            let code = LENGTH_BASE
                .iter()
                .rposition(|&b| usize::from(b) <= best)
                .unwrap_or(0);
            bits.symbol(257 + code as u16);
            bits.put(
                (best - usize::from(LENGTH_BASE[code])) as u32,
                LENGTH_EXTRA[code],
            );
            let code = DISTANCE_BASE
                .iter()
                .rposition(|&b| usize::from(b) <= distance)
                .unwrap_or(0);
            bits.reversed(code as u32, 5);
            bits.put(
                (distance - usize::from(DISTANCE_BASE[code])) as u32,
                DISTANCE_EXTRA[code],
            );
            for at in i..i + best {
                insert(at, &mut head, &mut prev);
            }
            i += best;
        } else {
            bits.symbol(u16::from(data[i]));
            insert(i, &mut head, &mut prev);
            i += 1;
        }
    }
    bits.symbol(256);
    let mut out = bits.finish();
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    bits: u64,
    count: u8,
}

impl BitWriter {
    /// `n` bits of `value`, least significant first.
    fn put(&mut self, value: u32, n: u8) {
        self.bits |= u64::from(value) << self.count;
        self.count += n;
        while self.count >= 8 {
            self.out.push(self.bits as u8);
            self.bits >>= 8;
            self.count -= 8;
        }
    }

    /// A Huffman code: its most significant bit first.
    fn reversed(&mut self, code: u32, n: u8) {
        let mut flipped = 0;
        for bit in 0..n {
            flipped |= ((code >> bit) & 1) << (n - 1 - bit);
        }
        self.put(flipped, n);
    }

    /// A literal or length symbol, in the fixed code.
    fn symbol(&mut self, symbol: u16) {
        let s = u32::from(symbol);
        match symbol {
            0..=143 => self.reversed(0x30 + s, 8),
            144..=255 => self.reversed(0x190 + s - 144, 9),
            256..=279 => self.reversed(s - 256, 7),
            _ => self.reversed(0xc0 + s - 280, 8),
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.bits as u8);
        }
        self.out
    }
}

/// A canonical Huffman code: how many codes of each length, and the symbols in code order.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Self {
        let mut counts = [0u16; 16];
        for &length in lengths {
            counts[length as usize] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for length in 1..15 {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = vec![0; lengths.len()];
        for (symbol, &length) in lengths.iter().enumerate() {
            if length != 0 {
                symbols[offsets[length as usize] as usize] = symbol as u16;
                offsets[length as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    at: usize,
    bits: u32,
    count: u32,
}

impl BitReader<'_> {
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        while self.count < n {
            let byte = *self.data.get(self.at).ok_or("the PNG's data ends early")?;
            self.at += 1;
            self.bits |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let value = self.bits & ((1u32 << n) - 1);
        self.bits >>= n;
        self.count -= n;
        Ok(value)
    }

    fn decode(&mut self, code: &Huffman) -> Result<u16, String> {
        let (mut value, mut first, mut index) = (0i32, 0i32, 0i32);
        for length in 1..16 {
            value |= self.bits(1)? as i32;
            let count = i32::from(code.counts[length]);
            if value - count < first {
                return Ok(code.symbols[(index + value - first) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            value <<= 1;
        }
        Err("a bad code in the PNG's data".into())
    }
}

/// Inflates a deflate stream (RFC 1951).
fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut input = BitReader {
        data,
        at: 0,
        bits: 0,
        count: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = input.bits(1)? == 1;
        match input.bits(2)? {
            0 => {
                // The bits left in the byte are dropped; fewer than eight are ever held.
                input.bits = 0;
                input.count = 0;
                let at = input.at;
                let header = data.get(at..at + 4).ok_or("the PNG's data ends early")?;
                let length = usize::from(u16::from_le_bytes([header[0], header[1]]));
                let stored = data
                    .get(at + 4..at + 4 + length)
                    .ok_or("the PNG's data ends early")?;
                out.extend_from_slice(stored);
                input.at = at + 4 + length;
            }
            1 => {
                let mut lengths = [0u8; 288];
                lengths[..144].fill(8);
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                lengths[280..].fill(8);
                let literals = Huffman::new(&lengths);
                let distances = Huffman::new(&[5; 30]);
                block(&mut input, &mut out, &literals, &distances)?;
            }
            2 => {
                let literal_count = input.bits(5)? as usize + 257;
                let distance_count = input.bits(5)? as usize + 1;
                let code_count = input.bits(4)? as usize + 4;
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut code_lengths = [0u8; 19];
                for &at in &ORDER[..code_count] {
                    code_lengths[at] = input.bits(3)? as u8;
                }
                let codes = Huffman::new(&code_lengths);
                let mut lengths = Vec::with_capacity(literal_count + distance_count);
                while lengths.len() < literal_count + distance_count {
                    let (value, repeat) = match input.decode(&codes)? {
                        symbol @ 0..=15 => (symbol as u8, 1),
                        16 => (
                            *lengths.last().ok_or("a bad code in the PNG's data")?,
                            3 + input.bits(2)?,
                        ),
                        17 => (0, 3 + input.bits(3)?),
                        _ => (0, 11 + input.bits(7)?),
                    };
                    lengths.extend(std::iter::repeat_n(value, repeat as usize));
                }
                if lengths.len() > literal_count + distance_count {
                    return Err("a bad code in the PNG's data".into());
                }
                let literals = Huffman::new(&lengths[..literal_count]);
                let distances = Huffman::new(&lengths[literal_count..]);
                block(&mut input, &mut out, &literals, &distances)?;
            }
            _ => return Err("a bad block in the PNG's data".into()),
        }
        if last {
            return Ok(out);
        }
    }
}

fn block(
    input: &mut BitReader,
    out: &mut Vec<u8>,
    literals: &Huffman,
    distances: &Huffman,
) -> Result<(), String> {
    loop {
        let symbol = input.decode(literals)?;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()),
            _ => {
                let code = usize::from(symbol - 257);
                let base = *LENGTH_BASE
                    .get(code)
                    .ok_or("a bad length in the PNG's data")?;
                let length =
                    usize::from(base) + input.bits(u32::from(LENGTH_EXTRA[code]))? as usize;
                let code = usize::from(input.decode(distances)?);
                let base = *DISTANCE_BASE
                    .get(code)
                    .ok_or("a bad distance in the PNG's data")?;
                let distance =
                    usize::from(base) + input.bits(u32::from(DISTANCE_EXTRA[code]))? as usize;
                if distance > out.len() {
                    return Err("a bad distance in the PNG's data".into());
                }
                let from = out.len() - distance;
                for i in 0..length {
                    out.push(out[from + i]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32, alpha: bool) -> Image {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let a = if alpha { (x * 7 % 256) as u8 } else { 255 };
                rgba.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, ((x + y) / 4) as u8, a]);
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn an_image_is_written_and_read_back() {
        for alpha in [false, true] {
            let image = gradient(97, 61, alpha);
            let png = encode(&image);
            assert_eq!(decode(&png).unwrap(), image);
        }
        // A screen is mostly flat: it compresses far below its size.
        let flat = Image {
            width: 400,
            height: 800,
            rgba: [250, 250, 250, 255].repeat(400 * 800),
        };
        let png = encode(&flat);
        assert!(png.len() < 20_000, "{}", png.len());
        assert_eq!(decode(&png).unwrap(), flat);
    }

    /// PNGs written by other encoders, with dynamic codes: the template's icons.
    #[test]
    fn other_encoders_pngs_are_read() {
        let icon = include_bytes!("../template/res/mipmap-xxxhdpi/ic_launcher.png");
        let image = decode(icon).unwrap();
        assert!(image.width > 100 && image.height > 100);
        assert_eq!(decode(&encode(&image)).unwrap(), image);
    }

    #[test]
    fn a_big_image_is_made_smaller_by_averaging() {
        // Columns of white and black, which become grey.
        let mut image = gradient(1080, 2340, false);
        for (i, px) in image.rgba.chunks_mut(4).enumerate() {
            let v = if i % 2 == 0 { 255 } else { 0 };
            px.copy_from_slice(&[v, v, v, 255]);
        }
        let small = shrink(&image, 1024);
        assert_eq!((small.width, small.height), (473, 1024));
        assert_eq!(small.rgba.len(), 473 * 1024 * 4);
        for px in small.rgba.chunks(4).take(500) {
            assert!((80..180).contains(&px[0]) && px[3] == 255, "{px:?}");
        }
        assert_eq!(shrink(&image, 0), image);
        assert_eq!(shrink(&small, 2000), small);
    }

    #[test]
    fn broken_pngs_are_refused() {
        assert!(decode(b"nope").is_err());
        let png = encode(&gradient(10, 10, false));
        assert!(decode(&png[..png.len() / 2]).is_err());
    }
}
