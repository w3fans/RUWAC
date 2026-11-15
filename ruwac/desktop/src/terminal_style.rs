//! Optional canonical attributes after the six legacy render-cell fields.
use gtk::cairo;
use gtk::prelude::*;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

/// Shape each distinct terminal glyph once, rather than once per cell/frame.
/// Colours and decorations remain Cairo operations outside the cached layout.
#[derive(Default)]
pub struct GlyphCache {
    typography: Option<(String, u8, u32, i32)>,
    entries: [HashMap<String, (gtk::pango::Layout, i32)>; 4],
    order: VecDeque<(usize, String)>,
}
impl GlyphCache {
    pub fn prepare(&mut self, area: &gtk::DrawingArea, font: &str, size: u8) {
        let typography = (
            font.to_owned(),
            size,
            area.pango_context().serial(),
            area.scale_factor(),
        );
        if self.typography.as_ref() != Some(&typography) {
            for entries in &mut self.entries {
                entries.clear();
            }
            self.order.clear();
            self.typography = Some(typography);
        }
    }
    pub fn get(
        &mut self,
        area: &gtk::DrawingArea,
        bold: bool,
        italic: bool,
        glyph: &str,
    ) -> (gtk::pango::Layout, i32) {
        let slot = usize::from(bold) + 2 * usize::from(italic);
        if let Some(shaped) = self.entries[slot].get(glyph) {
            return shaped.clone();
        }
        let (font, size, _, _) = self.typography.as_ref().expect("prepared glyph cache");
        let layout = area.create_pango_layout(None);
        let style = match (bold, italic) {
            (true, true) => " Bold Italic",
            (true, false) => " Bold",
            (false, true) => " Italic",
            _ => "",
        };
        layout.set_font_description(Some(&gtk::pango::FontDescription::from_string(&format!(
            "{font}{style} {size}"
        ))));
        let attributes = gtk::pango::AttrList::new();
        attributes.insert(gtk::pango::AttrFontFeatures::new("liga=0, calt=0"));
        layout.set_attributes(Some(&attributes));
        layout.set_text(glyph);
        let shaped = (layout.clone(), layout.pixel_size().1);
        // Bound both native layout count and retained input bytes. Unusually
        // long combining sequences still render, but are never kept in cache.
        if glyph.len() <= 256 {
            if self.order.len() == 512 {
                if let Some((slot, old)) = self.order.pop_front() {
                    self.entries[slot].remove(&old);
                }
            }
            self.order.push_back((slot, glyph.to_owned()));
            self.entries[slot].insert(glyph.to_owned(), shaped.clone());
        }
        shaped
    }
}

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
