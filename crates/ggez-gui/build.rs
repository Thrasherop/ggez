//! Gives the Windows program its icon and file description, so it looks
//! right in Explorer, the taskbar and the Start menu. Does nothing on other
//! systems, where icons belong to the app bundle or desktop entry instead.

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

/// An `.ico` file holding the icon at several sizes.
#[cfg_attr(not(windows), allow(dead_code))]
fn ico(sizes: &[usize]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().map(|&size| bitmap(size)).collect();
    let mut file = Vec::new();
    // Header: reserved, type 1 (icon), image count.
    file.extend_from_slice(&[0, 0, 1, 0]);
    file.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (&size, image) in sizes.iter().zip(&images) {
        // Width and height (0 would mean 256), no palette, reserved,
        // one plane, 32 bits per pixel, then where the image is.
        file.extend_from_slice(&[size as u8, size as u8, 0, 0, 1, 0, 32, 0]);
        file.extend_from_slice(&(image.len() as u32).to_le_bytes());
        file.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in images {
        file.extend(image);
    }
    file
}

/// One icon image in the format `.ico` files use: a bitmap header, the
/// pixels bottom row first as blue-green-red-alpha, then a transparency
/// mask (unused with 32-bit pixels, but required).
#[cfg_attr(not(windows), allow(dead_code))]
fn bitmap(size: usize) -> Vec<u8> {
    let rgba = icon::rgba(size);
    let mut image = Vec::new();
    image.extend_from_slice(&40u32.to_le_bytes());
    image.extend_from_slice(&(size as i32).to_le_bytes());
    // The height counts the pixels and the mask together.
    image.extend_from_slice(&(2 * size as i32).to_le_bytes());
    image.extend_from_slice(&1u16.to_le_bytes());
    image.extend_from_slice(&32u16.to_le_bytes());
    image.extend_from_slice(&[0; 24]);
    for row in (0..size).rev() {
        for pixel in rgba[row * size * 4..(row + 1) * size * 4].chunks_exact(4) {
            image.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    let mask_row = size.div_ceil(32) * 4;
    image.extend(std::iter::repeat_n(0u8, mask_row * size));
    image
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("set by cargo"));
        let path = out.join("ggez.ico");
        std::fs::write(&path, ico(&[16, 24, 32, 48, 64, 128])).expect("can write the icon");
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(path.to_str().expect("a readable path"));
        resource.set("ProductName", "ggez");
        resource.set("FileDescription", "ggez");
        // The icon is a nicety: a machine without a resource compiler
        // should still be able to build the program.
        if let Err(error) = resource.compile() {
            println!("cargo:warning=the program will have no icon: {error}");
        }
    }
}
