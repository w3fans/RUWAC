//! Desktop and ANSI palettes; explicit RGB and extended indexed colors stay unchanged.
use std::cell::Cell;
#[derive(Clone, Copy)]
pub struct Scheme {
    pub id: &'static str,
    pub name: &'static str,
    pub colors: [u32; 17],
}
pub const SCHEMES: [Scheme; 6] = [
    Scheme {
        id: "graphite",
        name: "Graphite",
        colors: [
            0x17181b, 0x1e1f23, 0x2a2c31, 0x26282c, 0x2f3136, 0xe7e7ea, 0x9b9ca3, 0x7aa7f5,
            0x131416, 0xd9dade, 0x6d6f76, 0x6fa8ff, 0x8fcf86, 0xe2c27a, 0xf0737a, 0xc39ae8,
            0x76cfd0,
        ],
    },
    Scheme {
        id: "nocturne",
        name: "Nocturne",
        colors: [
            0x161826, 0x1c1e2c, 0x2a2c3d, 0x242637, 0x2c2f41, 0xe9e9ed, 0x9c9eb3, 0x9184d9,
            0x121320, 0xdcdce6, 0x6b6d85, 0x8fa2f0, 0x9fd3a6, 0xe6c98a, 0xee8a9a, 0xb9a6f2,
            0x86cfe0,
        ],
    },
    Scheme {
        id: "fjord",
        name: "Fjord",
        colors: [
            0x2b303b, 0x2e3440, 0x3e4555, 0x383e4c, 0x414959, 0xeceff4, 0xa3abba, 0x88c0d0,
            0x262a33, 0xd8dee9, 0x6c7689, 0x81a1c1, 0xa3be8c, 0xebcb8b, 0xd36c76, 0xb48ead,
            0x88c0d0,
        ],
    },
    Scheme {
        id: "ember",
        name: "Ember",
        colors: [
            0x1c1a17, 0x23201c, 0x36302a, 0x2d2924, 0x3a342d, 0xebdbb2, 0xa89984, 0xe0a04a,
            0x181613, 0xe6d6ae, 0x7c6f64, 0x83a598, 0xb8bb26, 0xfabd2f, 0xfb4934, 0xd3869b,
            0x8ec07c,
        ],
    },
    Scheme {
        id: "paper",
        name: "Paper",
        colors: [
            0xf3f2ee, 0xe9e7e1, 0xfbfaf7, 0xdfddd6, 0xd4d1c9, 0x1e1e1c, 0x66635d, 0x3563c9,
            0xfbfaf7, 0x262624, 0x8a877f, 0x2a5bbf, 0x2f7a2b, 0x8a6400, 0xbd3328, 0x8a3fa8,
            0x17797a,
        ],
    },
    Scheme {
        id: "contrast",
        name: "High contrast",
        colors: [
            0x050505, 0x0c0c0c, 0x2b2b2b, 0x1e1e1e, 0x6b6b6b, 0xffffff, 0xd0d0d0, 0xffd60a,
            0x000000, 0xffffff, 0xbdbdbd, 0x74b9ff, 0x5dfc8a, 0xffe066, 0xff6b6b, 0xff9cf5,
            0x5ef1ff,
        ],
    },
];
thread_local! { static CURRENT: Cell<&'static Scheme> = const { Cell::new(&SCHEMES[0]) }; }
thread_local! {
    static VIVID: Cell<bool> = const { Cell::new(true) };
}
/// GNOME Terminal's default palette (normal 0–7, bright 8–15), with normal
/// yellow and blue lifted for legibility on dark backgrounds.
const VIVID_ANSI: [u32; 16] = [
    0x171421, 0xc01c28, 0x26a269, 0xc88800, 0x3584e4, 0xa347ba, 0x2aa1b3, 0xd0cfcc, 0x5e5c64,
    0xf66151, 0x33da7a, 0xe9ad0c, 0x2a7bde, 0xc061cb, 0x33c7de, 0xffffff,
];
pub fn set_vivid(vivid: bool) {
    VIVID.with(|v| v.set(vivid));
}
pub fn current() -> &'static Scheme {
    CURRENT.with(Cell::get)
}
pub fn select(id: &str) -> Option<&'static Scheme> {
    let s = SCHEMES.iter().find(|s| s.id == id)?;
    CURRENT.with(|c| c.set(s));
    Some(s)
}
pub fn rgb(n: u32) -> (f64, f64, f64) {
    (
        ((n >> 16) & 255) as f64 / 255.,
        ((n >> 8) & 255) as f64 / 255.,
        (n & 255) as f64 / 255.,
    )
}
pub fn css(s: &Scheme) -> String {
    let tokens = [
        "bg",
        "bar",
        "raised",
        "hover",
        "border",
        "text",
        "muted",
        "accent",
        "t_bg",
        "t_fg",
        "t_dim",
        "t_blue",
        "t_green",
        "t_yellow",
        "t_red",
        "t_magenta",
        "t_cyan",
    ];
    tokens
        .iter()
        .zip(s.colors)
        .map(|(n, c)| format!("@define-color ruwac_{n} #{c:06x};\n"))
        .collect()
}
// Adjust OKLCH lightness while retaining chroma and hue (equivalently OKLab a/b).
fn bright(rgb: (f64, f64, f64), paper: bool) -> (f64, f64, f64) {
    fn linear(x: f64) -> f64 {
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    }
    fn gamma(x: f64) -> f64 {
        let x = x.clamp(0., 1.);
        if x <= 0.0031308 {
            12.92 * x
        } else {
            1.055 * x.powf(1. / 2.4) - 0.055
        }
    }
    let (r, g, b) = (linear(rgb.0), linear(rgb.1), linear(rgb.2));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let light = (0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s
        + if paper { -0.12 } else { 0.12 })
    .clamp(0., 1.);
    let a = 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s;
    let l = (light + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (light - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (light - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    (
        gamma(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        gamma(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        gamma(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    )
}
impl Scheme {
    pub fn bg(&self) -> (f64, f64, f64) {
        rgb(self.colors[8])
    }
    pub fn fg(&self) -> (f64, f64, f64) {
        rgb(self.colors[9])
    }
    pub fn ansi(&self, n: usize) -> (f64, f64, f64) {
        // Black and white keep the scheme's contrast with its background.
        if VIVID.with(Cell::get) && self.id != "paper" && !matches!(n, 0 | 7 | 8 | 15) {
            return rgb(VIVID_ANSI[n % 16]);
        }
        let base = match n % 8 {
            0 => {
                let b = self.bg();
                let f = self.fg();
                (
                    b.0 * 0.15 + (1. - f.0) * 0.85,
                    b.1 * 0.15 + (1. - f.1) * 0.85,
                    b.2 * 0.15 + (1. - f.2) * 0.85,
                )
            }
            1 => rgb(self.colors[14]),
            2 => rgb(self.colors[12]),
            3 => rgb(self.colors[13]),
            4 => rgb(self.colors[11]),
            5 => rgb(self.colors[15]),
            6 => rgb(self.colors[16]),
            _ => self.fg(),
        };
        if n >= 8 {
            bright(base, self.id == "paper")
        } else {
            base
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palettes_are_bounded_unique_and_css_complete() {
        let mut ids = std::collections::HashSet::new();
        for s in SCHEMES {
            assert!(ids.insert(s.id));
            assert!(css(&s).contains("@define-color ruwac_t_yellow"));
            for i in 0..16 {
                let c = s.ansi(i);
                assert!(
                    [c.0, c.1, c.2]
                        .iter()
                        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                );
            }
        }
    }
}
