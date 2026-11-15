//! Native snapshot management shares lifecycle logic without granting automation permission.
use crate::workspace_control::Control;
use gtk::prelude::*;
use serde_json::json;
use std::rc::Rc;

fn refresh(control: Rc<Control>, rows: gtk::Box, status: gtk::Label, window: gtk::Window) {
    status.set_text("Loading saved layouts…");
    control.clone().user("workspace.saved",json!({}),move|result|{
        if !window.is_visible(){return;}
        while let Some(child)=rows.first_child(){rows.remove(&child);}
        match result{
            Err(error)=>status.set_text(&error.message),
            Ok(value)=>{
                let Some(saved)=value["saved_workspaces"].as_array()else{status.set_text("Snapshot catalog unavailable");return;};
                status.set_text(if saved.is_empty(){"No saved layouts. Enter a name and save this workspace."}else{"Open adds missing tabs here. Restore rebuilds the saved layout here. New window continues the saved terminals in a separate window; tabs can be dragged between windows."});
                for saved in saved{
                    let Some(id)=saved["id"].as_str().filter(|id|ruwac_protocol::valid_id(id,"workspace"))else{continue};
                    let row=gtk::Box::new(gtk::Orientation::Horizontal,6);
                    let label=gtk::Label::new(Some(&format!("{} · {} tabs",saved["name"].as_str().unwrap_or("Saved layout"),saved["tabs"])));label.set_hexpand(true);label.set_xalign(0.0);label.set_ellipsize(gtk::pango::EllipsizeMode::End);row.append(&label);
                    for (title,method,prefix)in[("Open","workspace.open","workspace-open"),("Restore","workspace.restore","workspace-restore"),("New window","workspace.openWindow","workspace-window")]{
                        let button=gtk::Button::with_label(title);button.set_widget_name(&format!("{prefix}-{id}"));
                        if method=="workspace.openWindow"{button.set_tooltip_text(Some("Open this layout in a new window and continue its terminals there"));}
                        let(control,status,window,id)=(control.clone(),status.clone(),window.downgrade(),id.to_owned());
                        button.connect_clicked(move|button|{
                            button.set_sensitive(false);status.set_text("Applying saved layout…");
                            let(button,status,window)=(button.downgrade(),status.clone(),window.clone());
                            control.clone().user(method,json!({"id":id}),move|result|{
                                if let Some(button)=button.upgrade(){button.set_sensitive(true);}
                                if let Some(window)=window.upgrade(){match result{Ok(_)=>window.close(),Err(error)=>status.set_text(&error.message)}}
                            });
                        });row.append(&button);
                    }
                    rows.append(&row);
                }
            }
        }
    });
}
pub fn button(parent: &impl IsA<gtk::Window>, control: Rc<Control>) -> gtk::Button {
    let button = gtk::Button::with_label("Workspaces…");
    let parent = parent.as_ref().downgrade();
    button.connect_clicked(move |_|{
        let Some(parent)=parent.upgrade()else{return;};
        let dialog=gtk::Window::builder().title("Saved workspaces").transient_for(&parent).modal(true).default_width(680).default_height(460).build();
        let body=gtk::Box::new(gtk::Orientation::Vertical,12);for side in ["start","end","top","bottom"]{match side{"start"=>body.set_margin_start(16),"end"=>body.set_margin_end(16),"top"=>body.set_margin_top(16),_=>body.set_margin_bottom(16)}}
        let description=gtk::Label::new(Some("Save a layout of terminal, RDP and file views. Restoring keeps terminal programs and live connections; it does not connect missing remote views automatically."));description.set_wrap(true);body.append(&description);
        let name=gtk::Entry::builder().placeholder_text("Layout name").hexpand(true).max_length(128).build();name.set_widget_name("workspace-name");
        let save=gtk::Button::with_label("Save current layout");save.set_widget_name("workspace-save");
        let row=gtk::Box::new(gtk::Orientation::Horizontal,6);row.append(&name);row.append(&save);body.append(&row);
        let status=gtk::Label::new(None);status.set_wrap(true);status.set_xalign(0.0);status.set_widget_name("workspace-status");body.append(&status);
        let rows=gtk::Box::new(gtk::Orientation::Vertical,6);let scroll=gtk::ScrolledWindow::builder().vexpand(true).child(&rows).build();body.append(&scroll);
        let buttons=gtk::Box::new(gtk::Orientation::Horizontal,6);let reload=gtk::Button::with_label("Refresh");reload.set_widget_name("workspace-refresh");let close=gtk::Button::with_label("Close");buttons.append(&reload);buttons.append(&close);body.append(&buttons);dialog.set_child(Some(&body));
        let(c,r,s,w)=(control.clone(),rows.clone(),status.clone(),dialog.downgrade());reload.connect_clicked(move |_|{if let Some(w)=w.upgrade(){refresh(c.clone(),r.clone(),s.clone(),w);}});
        let(c,r,s,w)=(control.clone(),rows.clone(),status.clone(),dialog.downgrade());save.connect_clicked(move|button|{
            button.set_sensitive(false);s.set_text("Saving current layout…");
            let(button,c,r,s,w)=(button.downgrade(),c.clone(),r.clone(),s.clone(),w.clone());
            c.clone().user("workspace.save",json!({"name":name.text().as_str()}),move|result|{
                if let Some(button)=button.upgrade(){button.set_sensitive(true);}
                if let Some(w)=w.upgrade(){match result{Ok(_)=>refresh(c,r,s,w),Err(error)=>s.set_text(&error.message)}}
            });
        });
        let w=dialog.downgrade();close.connect_clicked(move |_|{if let Some(w)=w.upgrade(){w.close();}});dialog.present();refresh(control.clone(),rows,status,dialog);
    });
    button
}
pub fn fixture(operation: &str, value: Option<&str>) -> serde_json::Value {
    fn find(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if root.widget_name() == name {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(w) = child {
            if let Some(found) = find(&w, name) {
                return Some(found);
            }
            child = w.next_sibling();
        }
        None
    }
    for root in gtk::Window::list_toplevels() {
        if let Ok(window) = root.downcast::<gtk::Window>() {
            if window.title().as_deref() != Some("Saved workspaces") {
                continue;
            }
            let body: gtk::Widget = window.clone().upcast();
            match operation {
                "name" => {
                    if let Some(entry) =
                        find(&body, "workspace-name").and_then(|w| w.downcast::<gtk::Entry>().ok())
                    {
                        entry.set_text(value.unwrap_or(""));
                        return json!({"set":true});
                    }
                }
                "inspect" => {
                    return json!({"open":true,"status":find(&body,"workspace-status").and_then(|w|w.downcast::<gtk::Label>().ok()).map(|w|w.text().to_string())});
                }
                "close" => {
                    window.close();
                    return json!({"closed":true});
                }
                _ => {
                    let name = match operation {
                        "save" => "workspace-save".to_owned(),
                        "refresh" => "workspace-refresh".to_owned(),
                        "open" | "restore" | "window" => {
                            format!("workspace-{operation}-{}", value.unwrap_or(""))
                        }
                        _ => String::new(),
                    };
                    if let Some(button) =
                        find(&body, &name).and_then(|w| w.downcast::<gtk::Button>().ok())
                    {
                        button.emit_clicked();
                        return json!({"clicked":true});
                    }
                }
            }
        }
    }
    json!({"open":false,"clicked":false})
}
