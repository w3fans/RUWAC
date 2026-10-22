//! Application-owned selections and an opt-in, write-only PRIMARY receiver.
use gtk::{gdk, glib, prelude::*};
use std::{cell::RefCell, sync::Arc};

pub const MAX_OFFER: usize = 128 * 1024;
thread_local! {
    static LOCAL: RefCell<Option<(gdk::ContentProvider, String, Option<Arc<()>>)>> = const { RefCell::new(None) };
}
pub fn remember(provider: &gdk::ContentProvider, text: &str) {
    LOCAL.with(|value| *value.borrow_mut() = Some((provider.clone(), text.into(), None)));
}
pub fn remember_remote(provider: &gdk::ContentProvider, text: &str, origin: &Arc<()>) {
    LOCAL.with(|value| {
        *value.borrow_mut() = Some((provider.clone(), text.into(), Some(origin.clone())))
    });
}
/// Protect selections from local terminals and other RDP connections, while
/// allowing this connection to refresh its own snapshot after focus returns.
pub fn selected_elsewhere(clipboard: &gdk::Clipboard, origin: &Arc<()>) -> bool {
    selected(clipboard).is_some()
        && LOCAL.with(|value| {
            value.borrow().as_ref().is_some_and(|(_, _, previous)| {
                previous
                    .as_ref()
                    .is_none_or(|previous| !Arc::ptr_eq(previous, origin))
            })
        })
}
pub fn forget() {
    LOCAL.with(|value| {
        value.borrow_mut().take();
    });
}
pub fn selected(clipboard: &gdk::Clipboard) -> Option<String> {
    LOCAL.with(|value| {
        let mut value = value.borrow_mut();
        if value
            .as_ref()
            .is_some_and(|(provider, _, _)| clipboard.content().as_ref() != Some(provider))
        {
            value.take();
        }
        value.as_ref().map(|(_, text, _)| text.clone())
    })
}
pub fn validate(text: &str, enabled: bool, active: bool) -> Result<(), &'static str> {
    if !enabled {
        return Err("Mouse-selection reception is disabled on the remote RUWAC desktop");
    }
    if !active {
        return Err("Focus the remote RUWAC window before pasting a local selection");
    }
    if text.is_empty() || text.len() > MAX_OFFER || text.contains('\0') {
        return Err("Mouse selection is empty, oversized or contains NUL");
    }
    Ok(())
}
pub fn receive(
    window: &adw::ApplicationWindow,
    params: &serde_json::Value,
    desktop_enabled: bool,
) -> ruwac_protocol::Result<serde_json::Value> {
    let text = params
        .as_object()
        .filter(|p| p.len() == 1)
        .and_then(|p| p.get("text"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ruwac_protocol::Error::new("INVALID_REQUEST", "Expected one bounded text selection")
        })?;
    validate(
        text,
        desktop_enabled && crate::sidebar_shell::primary_receive_allowed(),
        window.is_active(),
    )
    .map_err(|message| ruwac_protocol::Error::new("PRIMARY_REFUSED", message))?;
    let display = gtk::prelude::WidgetExt::display(window);
    let provider = gdk::ContentProvider::for_bytes(
        "text/plain;charset=utf-8",
        &glib::Bytes::from_owned(text.as_bytes().to_vec()),
    );
    display
        .primary_clipboard()
        .set_content(Some(&provider))
        .map_err(|_| {
            ruwac_protocol::Error::new(
                "PRIMARY_REFUSED",
                "The compositor refused PRIMARY ownership",
            )
        })?;
    display.flush();
    remember(&provider, text);
    Ok(serde_json::json!({"offered":true}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreground_consent_and_complete_text_are_required() {
        assert!(validate("test", false, true).is_err());
        assert!(validate("test", true, false).is_err());
        assert!(validate("", true, true).is_err());
        assert!(validate("x\0y", true, true).is_err());
        assert!(validate(&"x".repeat(MAX_OFFER + 1), true, true).is_err());
        assert!(validate("Živjo\nsecond line", true, true).is_ok());
    }
}
