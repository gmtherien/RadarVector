//! Generates RadarVector's original icon (radar scope + vector arrow) as PNGs.
//! `cargo run -p radarvector-app --example make_icon`

#![allow(clippy::unwrap_used)]

fn main() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon");
    std::fs::create_dir_all(&out).unwrap();
    for size in [16u32, 32, 64, 128, 256, 512, 1024] {
        let px = render(size);
        let f = std::fs::File::create(out.join(format!("icon-{size}.png"))).unwrap();
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), size, size);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&px).unwrap();
    }
    println!("icons written to {}", out.display());
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// Colour of the icon at normalised coordinates (0..1, y down), straight alpha.
fn shade(x: f32, y: f32) -> [f32; 4] {
    let (cx, cy) = (x - 0.5, y - 0.5);
    // Rounded square background.
    let r = 0.2;
    let qx = (cx.abs() - (0.5 - r)).max(0.0);
    let qy = (cy.abs() - (0.5 - r)).max(0.0);
    if (qx * qx + qy * qy).sqrt() > r - 0.02 || cx.abs() > 0.48 || cy.abs() > 0.48 {
        return [0.0; 4];
    }
    let bg_top = [0.07, 0.12, 0.20, 1.0];
    let bg_bot = [0.03, 0.06, 0.11, 1.0];
    let mut c = mix(bg_top, bg_bot, y);
    let d = (cx * cx + cy * cy).sqrt();
    let ang = cy.atan2(cx); // -pi..pi, 0 = east, y down
    // Sweep wedge (trailing glow behind the beam at -40°).
    let beam = -0.70_f32;
    let mut delta = beam - ang;
    while delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    if d < 0.40 && delta < 1.4 {
        let g = (1.0 - delta / 1.4).powf(2.0) * 0.55;
        c = mix(c, [0.0, 0.62, 0.45, 1.0], g);
    }
    // Rings.
    for rr in [0.13, 0.26, 0.39] {
        if (d - rr).abs() < 0.008 {
            c = mix(c, [0.34, 0.71, 0.91, 1.0], 0.75);
        }
    }
    // Cross hairs.
    if (cx.abs() < 0.005 || cy.abs() < 0.005) && d < 0.40 {
        c = mix(c, [0.34, 0.71, 0.91, 1.0], 0.35);
    }
    // Beam line.
    let along = cx * beam.cos() + cy * beam.sin();
    let across = -cx * beam.sin() + cy * beam.cos();
    if along > 0.0 && along < 0.40 && across.abs() < 0.010 {
        c = mix(c, [0.55, 1.0, 0.82, 1.0], 0.9);
    }
    // Vector arrow (the "vector" of RadarVector): from lower-left blip towards upper-right.
    let (ax, ay, bx, by) = (-0.17_f32, 0.15_f32, 0.12_f32, -0.12_f32);
    let (dx, dy) = (bx - ax, by - ay);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = (dx / len, dy / len);
    let (px, py) = (cx - ax, cy - ay);
    let t = px * ux + py * uy;
    let n = (-px * uy + py * ux).abs();
    let amber = [0.94, 0.65, 0.0, 1.0];
    if t > 0.0 && t < len - 0.06 && n < 0.018 {
        c = amber;
    }
    // Arrow head.
    let ht = t - (len - 0.11);
    if ht > 0.0 && ht < 0.11 && n < (0.11 - ht) * 0.55 {
        c = amber;
    }
    // Blip at the tail.
    if ((cx - ax).powi(2) + (cy - ay).powi(2)).sqrt() < 0.045 {
        c = [1.0, 0.85, 0.35, 1.0];
    }
    c
}

fn render(size: u32) -> Vec<u8> {
    let ss = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..ss {
                for sx in 0..ss {
                    let x = (px as f32 + (sx as f32 + 0.5) / ss as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / ss as f32) / size as f32;
                    let c = shade(x, y);
                    // Premultiply for averaging.
                    acc[0] += c[0] * c[3];
                    acc[1] += c[1] * c[3];
                    acc[2] += c[2] * c[3];
                    acc[3] += c[3];
                }
            }
            let n = (ss * ss) as f32;
            let a = acc[3] / n;
            let un = |v: f32| {
                if a > 0.0 {
                    (v / n / a * 255.0).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            out.extend_from_slice(&[un(acc[0]), un(acc[1]), un(acc[2]), (a * 255.0).round() as u8]);
        }
    }
    out
}
