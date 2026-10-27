//! Product information and optional support; no network access until a link is opened.
use adw::prelude::*;
pub const DONATION_URL: &str = "https://idz.si/bs/ruwac-donate.html";
pub const WEBSITE: &str = "https://github.com/w3fans/RUWAC";
pub fn present(parent: &adw::ApplicationWindow) -> gtk::Window {
    let window = gtk::Window::builder()
        .title("About RUWAC")
        .transient_for(parent)
        .modal(true)
        .default_width(640)
        .default_height(740)
        .build();
    window.set_widget_name("ruwac-about");
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = adw::HeaderBar::new();
    header.add_css_class("flat");
    header.set_title_widget(Some(&gtk::Label::new(Some("About RUWAC"))));
    window.set_titlebar(Some(&header));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(12);
    content.set_margin_bottom(32);
    content.set_margin_start(24);
    content.set_margin_end(24);
    let identity = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let icon = gtk::Image::from_icon_name("utilities-terminal-symbolic");
    icon.set_pixel_size(48);
    identity.append(&icon);
    let title = gtk::Label::new(Some("RUWAC"));
    title.add_css_class("title-1");
    identity.append(&title);
    let tagline = gtk::Label::new(Some("Your remote work, together"));
    tagline.add_css_class("dim-label");
    identity.append(&tagline);
    content.append(&identity);

    let info = adw::PreferencesGroup::builder().title("About").build();
    for (name, value) in [
        (
            "Description",
            "Persistent terminals, SSH agents and RDP desktops in one workspace. Detach and return to your terminal work, organize saved machines, and move between local and remote tools.",
        ),
        ("Author", "Denis Zvegelj, 2026"),
        ("License", "Apache License 2.0"),
        ("Installed version", env!("CARGO_PKG_VERSION")),
    ] {
        let row = adw::ActionRow::builder()
            .title(name)
            .subtitle(value)
            .build();
        row.set_subtitle_lines(0);
        info.add(&row);
    }
    content.append(&info);
    let links = adw::PreferencesGroup::builder().title("Project").build();
    for (title, subtitle, url, label) in [
        (
            "Source and contributions",
            "Explore the code, build RUWAC or help improve it.",
            WEBSITE,
            "GitHub",
        ),
        (
            "Feedback and bug reports",
            "Tell us what happened and which version you used.",
            "https://github.com/w3fans/RUWAC/issues",
            "Report an issue",
        ),
    ] {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .build();
        let button = gtk::LinkButton::with_label(url, label);
        button.set_valign(gtk::Align::Center);
        row.add_suffix(&button);
        row.set_activatable_widget(Some(&button));
        links.add(&row);
    }
    content.append(&links);
    let support = adw::PreferencesGroup::builder().title("Support").build();
    let support_row = adw::ActionRow::builder()
        .title("Support further development")
        .subtitle("If RUWAC is useful to you, you can support its development on idz.si. Donations are optional.")
        .build();
    support_row.set_subtitle_lines(0);
    let donation = gtk::LinkButton::with_label(DONATION_URL, "Open donation page");
    donation.set_valign(gtk::Align::Center);
    support_row.add_suffix(&donation);
    support_row.set_activatable_widget(Some(&donation));
    support.add(&support_row);
    let scan = adw::ExpanderRow::builder()
        .title("Open on another device")
        .subtitle("Scan the QR code for RUWAC’s support page")
        .build();
    let qr = gtk::DrawingArea::builder()
        .content_width(196)
        .content_height(196)
        .halign(gtk::Align::Center)
        .build();
    qr.set_widget_name("ruwac-donation-qr");
    qr.set_tooltip_text(Some(DONATION_URL));
    qr.update_property(&[gtk::accessible::Property::Label(
        "QR code for the RUWAC donation page",
    )]);
    qr.set_draw_func(|_, c, w, h| {
        c.set_antialias(gtk::cairo::Antialias::None);
        c.set_source_rgb(1.0, 1.0, 1.0);
        let _ = c.paint();
        let step = (w.min(h) as f64 / QR.len() as f64).floor().max(1.0);
        let ox = (w as f64 - step * QR.len() as f64) / 2.0;
        let oy = (h as f64 - step * QR.len() as f64) / 2.0;
        c.set_source_rgb(0.0, 0.0, 0.0);
        for (y, row) in QR.iter().enumerate() {
            for (x, bit) in row.bytes().enumerate() {
                if bit == b'1' {
                    c.rectangle(ox + x as f64 * step, oy + y as f64 * step, step, step);
                }
            }
        }
        let _ = c.fill();
    });
    qr.set_margin_top(16);
    qr.set_margin_bottom(16);
    scan.add_row(&qr);
    support.add(&scan);
    content.append(&support);
    let notice = gtk::Label::new(Some(
        "Engineering preview · See the project’s Security policy for known limitations.",
    ));
    notice.set_wrap(true);
    notice.set_justify(gtk::Justification::Center);
    notice.add_css_class("dim-label");
    notice.add_css_class("caption");
    content.append(&notice);
    let clamp = adw::Clamp::builder()
        .maximum_size(620)
        .child(&content)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    page.append(&scroll);
    window.set_child(Some(&page));
    window.present();
    window
}
// Generated with qrcode 8.2, error correction M, four-module quiet zone.
// Fixed public URL; no runtime dependency or remotely loaded image.
pub const QR: &[&str] = &[
    "0000000000000000000000000000000000000",
    "0000000000000000000000000000000000000",
    "0000000000000000000000000000000000000",
    "0000000000000000000000000000000000000",
    "0000111111101000011101100011111110000",
    "0000100000101010111101110010000010000",
    "0000101110101101011000010010111010000",
    "0000101110100111101111101010111010000",
    "0000101110101100110100110010111010000",
    "0000100000100001111011001010000010000",
    "0000111111101010101010101011111110000",
    "0000000000000100001010000000000000000",
    "0000100111111111111011011100101110000",
    "0000101011011000001100110101101100000",
    "0000101010101001001111000101001000000",
    "0000111101010111101001101100010010000",
    "0000110101101111111010010011000010000",
    "0000001000001010100111100010111110000",
    "0000001110101010001111010100001010000",
    "0000010101010110111000011110001010000",
    "0000111010101000000110000100010000000",
    "0000110111011100100100111000101100000",
    "0000111001101101011110010001110010000",
    "0000110010011001110000100111111000000",
    "0000111111101110110011001111111100000",
    "0000000000001011110100101000110000000",
    "0000111111101110110101111010110000000",
    "0000100000101101110101001000100100000",
    "0000101110101000110010011111110110000",
    "0000101110101100001100000100000010000",
    "0000101110100011011100010101101110000",
    "0000100000100101000000110011111010000",
    "0000111111101101101100011111000000000",
    "0000000000000000000000000000000000000",
    "0000000000000000000000000000000000000",
    "0000000000000000000000000000000000000",
    "0000000000000000000000000000000000000",
];
