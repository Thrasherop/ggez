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
