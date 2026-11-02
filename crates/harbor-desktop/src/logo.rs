//! Harbor's mark: a rounded square with an "H" made of two pier posts standing in a
//! wave. One geometry (in a 128 × 128 box) serves the sidebar, where egui paints it in
//! the theme's accent, and the window icon, rasterized here so no image decoder is
//! needed. packaging/harbor.svg draws the same shapes for the desktop menu.
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

/// Colour of the packaged icon (the Original theme's accent).
pub const ICON_BG: Color32 = Color32::from_rgb(0x08, 0x7f, 0x75);
const CORNER: f32 = 28.0;
/// Posts and crossbar of the "H": (x0, y0, x1, y1).
const BARS: [[f32; 4]; 3] = [
    [36.0, 24.0, 50.0, 80.0],
    [78.0, 24.0, 92.0, 80.0],
    [50.0, 46.0, 78.0, 58.0],
];
/// The wave: y = WAVE_Y - WAVE_AMP·sin(2π(x - WAVE_X0) / WAVE_PERIOD) for x in
/// WAVE_X0..WAVE_X1, stroked WAVE_WIDTH wide with round ends.
const WAVE_X0: f32 = 28.0;
const WAVE_X1: f32 = 100.0;
const WAVE_Y: f32 = 97.0;
const WAVE_AMP: f32 = 5.0;
const WAVE_PERIOD: f32 = 36.0;
const WAVE_WIDTH: f32 = 10.0;

fn wave(x: f32) -> f32 {
    WAVE_Y - WAVE_AMP * (std::f32::consts::TAU * (x - WAVE_X0) / WAVE_PERIOD).sin()
}

/// White (or dark, on a light accent) ink that reads on `background`.
pub fn ink_on(background: Color32) -> Color32 {
    let luma = 0.2126 * f32::from(background.r())
        + 0.7152 * f32::from(background.g())
        + 0.0722 * f32::from(background.b());
    if luma > 150.0 {
        Color32::from_rgb(0x10, 0x25, 0x1d)
    } else {
        Color32::WHITE
    }
}

/// Paints the mark into `rect` (a square).
pub fn paint(painter: &egui::Painter, rect: Rect, background: Color32) {
    let scale = rect.width() / 128.0;
    let at = |x: f32, y: f32| Pos2::new(rect.left() + x * scale, rect.top() + y * scale);
    let ink = ink_on(background);
    painter.rect_filled(rect, CORNER * scale, background);
    for [x0, y0, x1, y1] in BARS {
        painter.rect_filled(Rect::from_min_max(at(x0, y0), at(x1, y1)), 0.0, ink);
    }
    let steps = 36;
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let x = WAVE_X0 + (WAVE_X1 - WAVE_X0) * i as f32 / steps as f32;
            at(x, wave(x))
        })
        .collect();
    let radius = WAVE_WIDTH * scale / 2.0;
    painter.circle_filled(points[0], radius, ink);
    painter.circle_filled(points[steps], radius, ink);
    painter.add(egui::Shape::line(
        points,
        Stroke::new(WAVE_WIDTH * scale, ink),
    ));
}

/// Is the point (in the 128 box) inside the rounded square / the white ink?
fn inside_square(x: f32, y: f32) -> bool {
    let (cx, cy) = (
        x.clamp(CORNER, 128.0 - CORNER),
        y.clamp(CORNER, 128.0 - CORNER),
    );
    (0.0..=128.0).contains(&x)
        && (0.0..=128.0).contains(&y)
        && (x - cx).powi(2) + (y - cy).powi(2) <= CORNER * CORNER
}
fn inside_ink(x: f32, y: f32) -> bool {
    if BARS
        .iter()
        .any(|[x0, y0, x1, y1]| (*x0..=*x1).contains(&x) && (*y0..=*y1).contains(&y))
    {
        return true;
    }
    // Distance to the wave: nearest sampled point is precise enough at icon sizes.
    let r2 = (WAVE_WIDTH / 2.0).powi(2);
    let from = (x - WAVE_WIDTH).max(WAVE_X0);
    let to = (x + WAVE_WIDTH).min(WAVE_X1);
    let mut t = from;
    while t <= to {
        if (x - t).powi(2) + (y - wave(t)).powi(2) <= r2 {
            return true;
        }
        t += 0.5;
    }
    false
}

/// The window icon, `size` pixels square, 4 × 4 supersampled.
pub fn icon(size: u32) -> egui::IconData {
    const SAMPLES: u32 = 4;
    let ink = ink_on(ICON_BG);
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (mut square, mut white) = (0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let to_box = |p: u32, s: u32| {
                        (p as f32 + (s as f32 + 0.5) / SAMPLES as f32) * 128.0 / size as f32
                    };
                    let (x, y) = (to_box(px, sx), to_box(py, sy));
                    if inside_square(x, y) {
                        square += 1;
                        white += u32::from(inside_ink(x, y));
                    }
                }
            }
            let n = (SAMPLES * SAMPLES) as f32;
            let alpha = square as f32 / n;
            let mix = if square == 0 {
                0.0
            } else {
                white as f32 / square as f32
            };
            let channel =
                |bg: u8, fg: u8| (f32::from(bg) * (1.0 - mix) + f32::from(fg) * mix).round() as u8;
            rgba.extend_from_slice(&[
                channel(ICON_BG.r(), ink.r()),
                channel(ICON_BG.g(), ink.g()),
                channel(ICON_BG.b(), ink.b()),
                (alpha * 255.0).round() as u8,
            ]);
        }
    }
    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_has_transparent_corners_and_both_colours() {
        let icon = super::icon(64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        assert_eq!(icon.rgba[3], 0, "rounded corner is transparent");
        let pixel = |x: usize, y: usize| &icon.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(pixel(4, 32), [0x08, 0x7f, 0x75, 255], "background");
        assert_eq!(pixel(21, 32), [255, 255, 255, 255], "left post");
    }
}
