//! Writes the program icon as a PNG file, for packaging: macOS app bundles
//! and Linux desktop entries want the icon as an image file, while the
//! program itself draws it from code.
//!
//!     cargo run -p ggez-gui --example icon_png -- icon.png 512

#[path = "../src/icon.rs"]
#[allow(dead_code)]
mod icon;

/// The checksum PNG puts after every chunk.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg());
        }
    }
    !crc
}

/// The checksum zlib puts after the data.
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in bytes {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    file.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = file.len();
    file.extend_from_slice(kind);
    file.extend_from_slice(data);
    let checksum = crc32(&file[start..]);
    file.extend_from_slice(&checksum.to_be_bytes());
}

/// A PNG of `size` by `size` pixels. The pixels are stored without
/// compression: a few hundred kilobytes at most, written once per release,
/// are not worth a dependency.
fn png(size: usize, rgba: &[u8]) -> Vec<u8> {
    // Each row starts with a byte saying it is not filtered.
    let mut rows = Vec::with_capacity(size * (size * 4 + 1));
    for row in rgba.chunks_exact(size * 4) {
        rows.push(0);
        rows.extend_from_slice(row);
    }
    // A zlib stream of "stored" blocks, each at most 65535 bytes.
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = rows.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&rows).to_be_bytes());

    let mut file = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::new();
    header.extend_from_slice(&(size as u32).to_be_bytes());
    header.extend_from_slice(&(size as u32).to_be_bytes());
    // 8 bits per channel, red-green-blue-alpha, the only compression and
    // filter methods there are, not interlaced.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut file, b"IHDR", &header);
    chunk(&mut file, b"IDAT", &zlib);
    chunk(&mut file, b"IEND", &[]);
    file
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let (Some(path), Some(size)) = (
        arguments.next(),
        arguments.next().and_then(|s| s.parse::<usize>().ok()),
    ) else {
        eprintln!("usage: icon_png <file.png> <size in pixels>");
        std::process::exit(2);
    };
    if !(16..=1024).contains(&size) {
        eprintln!("the size must be between 16 and 1024");
        std::process::exit(2);
    }
    if let Err(error) = std::fs::write(&path, png(size, &icon::rgba(size))) {
        eprintln!("could not write {path}: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_their_published_test_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn a_png_has_its_three_chunks_and_every_pixel() {
        let size = 200;
        let rgba = icon::rgba(size);
        let file = png(size, &rgba);
        assert_eq!(file[..8], [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&file[12..16], b"IHDR");
        assert_eq!(file[16..24], [0, 0, 0, 200, 0, 0, 0, 200]);
        assert_eq!(&file[file.len() - 8..file.len() - 4], b"IEND");
        // More than one stored block is needed at this size.
        assert!(size * (size * 4 + 1) > 65_535);
        assert!(file.len() > rgba.len());
    }
}
