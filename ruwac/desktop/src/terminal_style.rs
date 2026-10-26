//! Optional canonical attributes after the six legacy render-cell fields.
use gtk::cairo;
use serde_json::Value;

/// Solid cell tiles must share hard edges, including fractional font metrics.
/// Antialiased neighbouring fills otherwise expose the canvas between cells.
pub fn background(
    cr: &cairo::Context,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    rgb: (f64, f64, f64),
) {
    let _ = cr.save();
    cr.set_antialias(cairo::Antialias::None);
    cr.set_source_rgb(rgb.0, rgb.1, rgb.2);
    cr.rectangle(x, y, width, height);
    let _ = cr.fill();
    let _ = cr.restore();
}

pub fn decoration(
    cr: &cairo::Context,
    style: &Value,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    foreground: (f64, f64, f64),
    underline_color: (f64, f64, f64),
) {
    if style["hidden"] == true
        || (style["strike"] != true && matches!(style["underline"].as_str(), None | Some("none")))
    {
        return;
    }
    let _ = cr.save();
    cr.set_line_width(1.0);
    cr.set_dash(&[], 0.0);
    if style["strike"] == true {
        cr.set_source_rgb(foreground.0, foreground.1, foreground.2);
        cr.move_to(x, y + height * 0.5);
        cr.line_to(x + width, y + height * 0.5);
        let _ = cr.stroke();
    }
    cr.set_source_rgb(underline_color.0, underline_color.1, underline_color.2);
    let baseline = y + height - 3.0;
    match style["underline"].as_str() {
        Some("single" | "double" | "dotted" | "dashed") => {
            match style["underline"].as_str() {
                Some("dotted") => cr.set_dash(&[1.0, 2.0], x.rem_euclid(3.0)),
                Some("dashed") => cr.set_dash(&[4.0, 2.0], x.rem_euclid(6.0)),
                _ => (),
            }
            cr.move_to(x, baseline);
            cr.line_to(x + width, baseline);
            let _ = cr.stroke();
            if style["underline"] == "double" {
                cr.move_to(x, baseline - 3.0);
                cr.line_to(x + width, baseline - 3.0);
                let _ = cr.stroke();
            }
        }
        Some("curly") => {
            cr.move_to(x, baseline);
            let mut offset = 0.0;
            while offset < width {
                let end = (offset + 4.0).min(width);
                cr.curve_to(
                    x + offset + 1.0,
                    baseline - 2.0,
                    x + end - 1.0,
                    baseline + 2.0,
                    x + end,
                    baseline,
                );
                offset = end;
            }
            let _ = cr.stroke();
        }
        _ => (),
    }
    let _ = cr.restore();
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn pixels(style: Value) -> Vec<u8> {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 40, 24).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_source_rgb(1.0, 0.0, 0.0);
            decoration(
                &cr,
                &style,
                2.0,
                0.0,
                32.0,
                20.0,
                (1.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
            );
        }
        surface.flush();
        surface.data().unwrap().to_vec()
    }
    #[test]
    fn fractional_cell_backgrounds_are_continuous_at_multiple_scales() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let draw = |tiled: bool| {
                let mut surface =
                    cairo::ImageSurface::create(cairo::Format::ARgb32, 192, 40).unwrap();
                {
                    let cr = cairo::Context::new(&surface).unwrap();
                    cr.set_source_rgb(0.1, 0.1, 0.1);
                    cr.paint().unwrap();
                    cr.scale(scale, scale);
                    if tiled {
                        for n in 0..12 {
                            background(&cr, n as f64 * 7.7, 0.0, 7.7, 16.0, (0.6, 0.8, 0.9));
                        }
                    } else {
                        background(&cr, 0.0, 0.0, 12.0 * 7.7, 16.0, (0.6, 0.8, 0.9));
                    }
                    assert_eq!(
                        cr.antialias(),
                        cairo::Antialias::Default,
                        "text antialiasing must be restored"
                    );
                }
                surface.flush();
                let pixels = surface.data().unwrap().to_vec();
                pixels
            };
            assert_eq!(draw(true), draw(false), "cell seams at scale {scale}");
        }
    }
    #[test]
    fn underline_variants_and_strike_draw_distinct_real_pixels() {
        let blank = pixels(json!({}));
        let mut images = Vec::new();
        for name in ["single", "double", "curly", "dotted", "dashed"] {
            let image = pixels(json!({"underline":name}));
            assert_ne!(image, blank);
            assert!(!images.contains(&image), "{name}");
            images.push(image);
        }
        let strike = pixels(json!({"strike":true}));
        assert_ne!(strike, blank);
        assert!(!images.contains(&strike));
        assert_eq!(
            pixels(json!({"hidden":true,"strike":true,"underline":"single"})),
            blank
        );
    }
    #[test]
    fn strike_uses_foreground_and_underline_uses_its_own_color() {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 40, 24).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            decoration(
                &cr,
                &json!({"strike":true,"underline":"single"}),
                2.0,
                0.0,
                32.0,
                20.0,
                (1.0, 0.0, 0.0),
                (0.0, 0.0, 1.0),
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let at = |x: usize, y: usize| {
            u32::from_ne_bytes(
                data[y * stride + x * 4..y * stride + x * 4 + 4]
                    .try_into()
                    .unwrap(),
            )
        };
        let strike = at(20, 9);
        let underline = at(20, 16);
        assert!(strike & 0x00ff0000 > 0 && strike & 0x000000ff == 0);
        assert!(underline & 0x000000ff > 0 && underline & 0x00ff0000 == 0);
    }
}
