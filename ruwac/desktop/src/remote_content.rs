//! Keep a received selection's provider alive across copies of the same MIME.
//! Existing compositor offers can then request the current payload. Each read
//! snapshots immutable bytes so an in-flight transfer never mixes generations.
use glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::{collections::BTreeMap, rc::Rc};
pub type LazyReply = tokio::sync::oneshot::Sender<Result<glib::Bytes, String>>;
pub type LazyRead = Rc<dyn Fn(String, LazyReply)>;
mod imp {
    use super::*;
    use gdk::subclass::prelude::*;
    use std::{cell::RefCell, future::Future, pin::Pin};
    #[derive(Default)]
    pub struct RemoteContent {
        pub payload: RefCell<BTreeMap<String, Option<glib::Bytes>>>,
        pub lazy: RefCell<Option<LazyRead>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for RemoteContent {
        const NAME: &'static str = "RuwacRemoteContent";
        type Type = super::RemoteContent;
        type ParentType = gdk::ContentProvider;
    }
    impl ObjectImpl for RemoteContent {}
    impl ContentProviderImpl for RemoteContent {
        fn formats(&self) -> gdk::ContentFormats {
            let p = self.payload.borrow();
            gdk::ContentFormats::new(&p.keys().map(String::as_str).collect::<Vec<_>>())
        }
        fn storable_formats(&self) -> gdk::ContentFormats {
            self.formats()
        }
        fn write_mime_type_future(
            &self,
            mime: &str,
            stream: &gio::OutputStream,
            priority: glib::Priority,
        ) -> Pin<Box<dyn Future<Output = Result<(), glib::Error>> + 'static>> {
            let bytes = self.payload.borrow().get(mime).and_then(Clone::clone);
            let receiver = if bytes.is_none() {
                self.lazy.borrow().as_ref().map(|load| {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    load(mime.to_owned(), tx);
                    rx
                })
            } else {
                None
            };
            let stream = stream.clone();
            Box::pin(async move {
                let bytes = if let Some(bytes) = bytes {
                    Some(bytes)
                } else if let Some(receiver) = receiver {
                    receiver.await.ok().and_then(Result::ok)
                } else {
                    None
                };
                let bytes = bytes.ok_or_else(|| {
                    glib::Error::new(
                        gio::IOErrorEnum::NotSupported,
                        "Clipboard format unavailable",
                    )
                })?;
                match stream.write_all_future(bytes, priority).await {
                    Ok((_, _, None)) => Ok(()),
                    Ok((_, _, Some(e))) | Err((_, e)) => Err(e),
                }
            })
        }
    }
}
glib::wrapper! { pub struct RemoteContent(ObjectSubclass<imp::RemoteContent>) @extends gdk::ContentProvider; }
impl RemoteContent {
    pub fn new(mime: &'static str, bytes: glib::Bytes) -> Self {
        let p: Self = glib::Object::new();
        p.imp()
            .payload
            .borrow_mut()
            .insert(mime.into(), Some(bytes));
        p
    }
    pub fn mime(&self) -> Option<&'static str> {
        let payload = self.imp().payload.borrow();
        if payload.contains_key("image/png") {
            Some("image/png")
        } else if payload.contains_key("text/plain;charset=utf-8") {
            Some("text/plain;charset=utf-8")
        } else {
            None
        }
    }
    pub fn supports_formats(&self, formats: &[&str]) -> bool {
        let payload = self.imp().payload.borrow();
        payload.len() == formats.len() && formats.iter().all(|mime| payload.contains_key(*mime))
    }
    pub fn replace_bytes(&self, bytes: glib::Bytes) {
        if let Some(mime) = self.mime() {
            self.imp()
                .payload
                .borrow_mut()
                .insert(mime.into(), Some(bytes));
        }
    }
    /// Preserve the Wayland source, but discard cached data from older offers.
    pub fn offer(&self, formats: &[&str], ready: &str, bytes: glib::Bytes, read: LazyRead) {
        let mut payload = self.imp().payload.borrow_mut();
        payload.clear();
        for mime in formats {
            payload.insert((*mime).into(), None);
        }
        payload.insert(ready.into(), Some(bytes));
        *self.imp().lazy.borrow_mut() = Some(read);
    }
}
