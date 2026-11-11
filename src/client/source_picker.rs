use gtk::{Orientation, prelude::*};
use std::{cell::Cell, rc::Rc};

pub fn choose(window: &gtk::ApplicationWindow) -> Option<u64> {
    if cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Some(0); // The compositor authorizes the source through the portal.
    }
    choose_from(window, &super::sources::list())
}
fn choose_from(window: &gtk::ApplicationWindow, sources: &[(u64, String)]) -> Option<u64> {
    // shortcut: Windows uses HWND/DWM previews; the system picker requires capture-item support in the media backend.
    let dialog = gtk::Dialog::with_buttons(
        Some("Compartilhar tela"),
        Some(window),
        gtk::DialogFlags::MODAL,
        &[
            ("Cancelar", gtk::ResponseType::Cancel),
            ("Transmitir", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_resizable(false);
    dialog.set_default_response(gtk::ResponseType::Accept);
    let content = gtk::Box::new(Orientation::Vertical, 12);
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    let stack = gtk::Stack::new();
    let selected = Rc::new(Cell::new(sources.first().map(|s| s.0)));
    let mut group = None::<gtk::RadioButton>;
    for (page, items) in sources.chunks(6).enumerate() {
        let grid = gtk::Grid::new();
        grid.set_row_spacing(12);
        grid.set_column_spacing(12);
        grid.set_column_homogeneous(true);
        for (index, (id, title)) in items.iter().enumerate() {
            let choice = match &group {
                Some(first) => gtk::RadioButton::from_widget(first),
                None => gtk::RadioButton::new(),
            };
            if group.is_none() {
                group = Some(choice.clone());
            }
            choice.set_mode(false);
            choice.set_active(Some(*id) == selected.get());
            choice.style_context().add_class("source-choice");
            let card = gtk::Box::new(Orientation::Vertical, 8);
            let preview = gtk::DrawingArea::new();
            preview.set_size_request(200, 112);
            preview.style_context().add_class("source-preview");
            #[cfg(windows)]
            if *id != 0 {
                super::windows_preview::attach(&preview, &dialog, *id);
            }
            if *id == 0
                && let Some(root) = gtk::gdk::Screen::default().and_then(|s| s.root_window())
                && let Some(image) = root.pixbuf(0, 0, root.width(), root.height())
                && let Some(image) = {
                    let scale = (200.0 / image.width() as f64).min(112.0 / image.height() as f64);
                    image.scale_simple(
                        (image.width() as f64 * scale).max(1.0) as i32,
                        (image.height() as f64 * scale).max(1.0) as i32,
                        gtk::gdk_pixbuf::InterpType::Bilinear,
                    )
                }
            {
                use gtk::gdk::prelude::GdkContextExt;
                preview.connect_draw(move |area, cr| {
                    cr.set_source_pixbuf(
                        &image,
                        (area.allocated_width() - image.width()) as f64 / 2.0,
                        (area.allocated_height() - image.height()) as f64 / 2.0,
                    );
                    let _ = cr.paint();
                    gtk::glib::Propagation::Proceed
                });
            }
            card.pack_start(&preview, false, false, 0);
            let title_label = super::ui::label(title, "video-name");
            title_label.set_max_width_chars(24);
            title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            choice.set_tooltip_text(Some(title));
            card.pack_start(&title_label, false, false, 0);
            choice.add(&card);
            let picked = selected.clone();
            let id = *id;
            choice.connect_toggled(move |button| {
                if button.is_active() {
                    picked.set(Some(id));
                }
            });
            grid.attach(&choice, (index % 3) as i32, (index / 3) as i32, 1, 1);
        }
        stack.add_named(&grid, &page.to_string());
    }
    content.pack_start(&stack, true, true, 0);
    let pages = sources.len().div_ceil(6);
    if pages > 1 {
        let navigation = gtk::Box::new(Orientation::Horizontal, 8);
        let previous = gtk::Button::with_label("Anterior");
        previous.set_sensitive(false);
        let next = gtk::Button::with_label("Próxima");
        let position = super::ui::label(&format!("1 / {pages}"), "muted");
        navigation.pack_start(&previous, false, false, 0);
        navigation.pack_start(&position, true, true, 0);
        navigation.pack_end(&next, false, false, 0);
        let page = Rc::new(Cell::new(0usize));
        for (button, forward) in [(&previous, false), (&next, true)] {
            let page = page.clone();
            let stack = stack.clone();
            let position = position.clone();
            let previous = previous.downgrade();
            let next = next.downgrade();
            button.connect_clicked(move |_| {
                let current = if forward {
                    (page.get() + 1).min(pages - 1)
                } else {
                    page.get().saturating_sub(1)
                };
                page.set(current);
                stack.set_visible_child_name(&current.to_string());
                position.set_text(&format!("{} / {pages}", current + 1));
                if let Some(previous) = previous.upgrade() {
                    previous.set_sensitive(current > 0);
                }
                if let Some(next) = next.upgrade() {
                    next.set_sensitive(current + 1 < pages);
                }
            });
        }
        content.pack_start(&navigation, false, false, 0);
    }
    dialog.content_area().add(&content);
    dialog.set_response_sensitive(gtk::ResponseType::Accept, selected.get().is_some());
    dialog.show_all();
    let result = (dialog.run() == gtk::ResponseType::Accept)
        .then(|| selected.get())
        .flatten();
    dialog.close();
    result
}

#[cfg(test)]
pub(super) fn check_picker(window: &gtk::ApplicationWindow) {
    fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut widgets = vec![widget.clone()];
        if let Some(container) = widget.downcast_ref::<gtk::Container>() {
            for child in container.children() {
                widgets.extend(descendants(&child));
            }
        }
        widgets
    }
    gtk::glib::idle_add_local_once(|| {
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .find_map(|w| w.downcast::<gtk::Dialog>().ok())
            .unwrap();
        let widgets = descendants(dialog.upcast_ref());
        widgets
            .iter()
            .filter_map(|w| w.downcast_ref::<gtk::Button>())
            .find(|b| b.label().as_deref() == Some("Próxima"))
            .unwrap()
            .emit_clicked();
        widgets
            .iter()
            .filter_map(|w| w.downcast_ref::<gtk::RadioButton>())
            .find(|b| b.tooltip_text().as_deref() == Some("Window7"))
            .unwrap()
            .set_active(true);
        dialog.response(gtk::ResponseType::Accept);
    });
    let sources = (1..=7)
        .map(|id| (id, format!("Window{id}")))
        .collect::<Vec<_>>();
    assert_eq!(choose_from(window, &sources), Some(7));
    gtk::glib::idle_add_local_once(|| {
        for widget in gtk::Window::list_toplevels() {
            if let Ok(dialog) = widget.downcast::<gtk::Dialog>() {
                dialog.response(gtk::ResponseType::Cancel);
            }
        }
    });
    assert_eq!(choose_from(window, &sources), None);
}
