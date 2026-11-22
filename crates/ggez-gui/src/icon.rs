//! The ggez icon: a teal ring and dot on a dark rounded square. Drawn by a
//! function so the program needs no image files; the build script uses the
//! same function to give the Windows program its icon.

/// The icon as `size` x `size` RGBA pixels, top row first.
pub fn rgba(size: usize) -> Vec<u8> {
    // The design is laid out on a 64-pixel grid and scaled from there.
    let scale = size as f32 / 64.0;
    // Sample each pixel a few times so edges are smooth at small sizes.
    const SAMPLES: usize = 4;
    let mut pixels = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / scale;
                    let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / scale;
                    if let Some([cr, cg, cb]) = color_at(px, py) {
                        r += cr;
                        g += cg;
                        b += cb;
                        a += 1.0;
                    }
                }
            }
            let total = (SAMPLES * SAMPLES) as f32;
            let channel = |sum: f32| if a > 0.0 { (sum / a).round() as u8 } else { 0 };
            pixels.extend_from_slice(&[
                channel(r),
                channel(g),
                channel(b),
                (a / total * 255.0).round() as u8,
            ]);
        }
    }
    pixels
}

/// The color at a point on the 64-pixel design grid, or `None` outside
/// the icon's rounded square.
fn color_at(x: f32, y: f32) -> Option<[f32; 3]> {
    let (dx, dy) = (x - 32.0, y - 32.0);
    // A rounded square: inside unless in a corner beyond the radius.
    let (cx, cy) = ((dx.abs() - 19.0).max(0.0), (dy.abs() - 19.0).max(0.0));
    if (cx * cx + cy * cy).sqrt() > 12.0 {
        return None;
    }
    let distance = (dx * dx + dy * dy).sqrt();
    Some(if (13.0..=20.0).contains(&distance) || distance <= 4.5 {
        [45.0, 212.0, 191.0]
    } else {
        [20.0, 23.0, 29.0]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixels: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * size + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    #[test]
    fn has_clear_corners_a_dark_face_and_a_teal_ring() {
        for size in [16, 32, 64, 128] {
            let pixels = rgba(size);
            assert_eq!(pixels.len(), size * size * 4);
            assert_eq!(pixel(&pixels, size, 0, 0)[3], 0, "corner at {size}");
            let centre = pixel(&pixels, size, size / 2, size / 2);
            // At small sizes the dot blends a little with the face around it.
            assert!(
                centre[3] == 255 && centre[1] > 180 && centre[0] < 80,
                "dot at {size}: {centre:?}"
            );
            // Three quarters of the way out along the middle row: the ring.
            let ring = pixel(&pixels, size, size / 2 + size / 4, size / 2);
            assert_eq!(ring[3], 255);
            assert!(ring[1] > 150, "ring at {size}: {ring:?}");
        }
    }
}
