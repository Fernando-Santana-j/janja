use super::icons::{self, Icon};
use super::{Command, Commands, Person};
use gtk::{Orientation, gdk, prelude::*};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

fn hide_due(fullscreen: bool, idle: Duration, interacting: bool) -> bool {
    fullscreen && !interacting && idle >= Duration::from_secs(3)
}

struct Tile {
    slot: gtk::Box,
    content: gtk::Box,
    title: gtk::Label,
    expand: gtk::Button,
    fullscreen: RefCell<Option<gtk::Window>>,
    controls: gtk::Revealer,
    last_activity: Cell<Instant>,
    hovered: Cell<bool>,
    keyboard: Cell<bool>,
    timer: RefCell<Option<gtk::glib::SourceId>>,
}
impl Tile {
    fn restore(&self) {
        let fullscreen = self.fullscreen.borrow_mut().take();
        if let Some(window) = fullscreen {
            window.remove(&self.content);
            self.slot.pack_start(&self.content, true, true, 0);
            icons::set(
                &self.expand,
                Icon::Expand,
                "Tela cheia (duplo clique)",
                false,
            );
            if let Some(timer) = self.timer.borrow_mut().take() {
                timer.remove();
            }
            self.controls.set_reveal_child(true);
            self.hovered.set(false);
            self.slot.show_all();
            window.unfullscreen();
            window.hide();
            window.close();
        }
    }
    fn new(
        id: u64,
        title: &str,
        parent: &gtk::ApplicationWindow,
        tx: &Commands,
        error: &gtk::Label,
    ) -> Rc<Self> {
        let content = gtk::Box::new(Orientation::Vertical, 0);
        content.style_context().add_class("video-tile");
        let area = gtk::DrawingArea::new();
        area.set_size_request(240, 135);
        area.set_hexpand(true);
        area.set_vexpand(true);
        let frame = Rc::new(RefCell::new(None::<super::video::Frame>));
        let current = frame.clone();
        area.connect_draw(move |area, cr| {
            cr.set_source_rgb(0.078, 0.09, 0.11);
            let _ = cr.paint();
            if let Some(image) = current.borrow().as_ref() {
                let _ = image.paint(
                    cr,
                    area.allocated_width() as f64,
                    area.allocated_height() as f64,
                );
            }
            gtk::glib::Propagation::Proceed
        });
        // GTK schedules presentation with the display clock; hidden tiles do not poll/copy frames.
        area.add_tick_callback(move |area, _| {
            if let Some(sample) = super::video::sample(id)
                && let Some(image) = super::video::Frame::new(&sample)
            {
                *frame.borrow_mut() = Some(image);
                area.queue_draw();
            }
            gtk::glib::ControlFlow::Continue
        });
        let overlay = gtk::Overlay::new();
        overlay.add(&area);
        content.pack_start(&overlay, true, true, 0);
        let bar = gtk::Box::new(Orientation::Horizontal, 8);
        bar.style_context().add_class("video-toolbar");
        let title = super::ui::label(title, "video-name");
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_hexpand(true);
        bar.pack_start(&title, true, true, 0);
        let volume = gtk::Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        volume.set_value(100.0);
        volume.set_digits(0);
        volume.set_draw_value(false);
        volume.set_size_request(110, -1);
        volume.set_tooltip_text(Some("Volume desta transmissão (0 silencia)"));
        let commands = tx.clone();
        let errors = error.clone();
        volume.connect_value_changed(move |scale| {
            super::ui::submit(
                &commands,
                Command::Volume(id, scale.value().round() as u8),
                &errors,
            );
        });
        let sound = gtk::Box::new(Orientation::Horizontal, 6);
        sound.pack_start(&icons::image(Icon::Volume), false, false, 0);
        sound.pack_start(&volume, false, false, 0);
        bar.pack_start(&sound, false, false, 0);
        let expand = icons::button(Icon::Expand, "Tela cheia (duplo clique)");
        expand.set_tooltip_text(Some("Ampliar este vídeo; Escape para voltar"));
        bar.pack_start(&expand, false, false, 0);
        let close = icons::button(Icon::Close, "Fechar transmissão");
        let commands = tx.clone();
        let errors = error.clone();
        close.connect_clicked(move |_| {
            super::ui::submit(&commands, Command::Unwatch(id), &errors);
        });
        bar.pack_start(&close, false, false, 0);
        let hover = gtk::EventBox::new();
        hover.set_visible_window(false);
        hover.add(&bar);
        let controls = gtk::Revealer::new();
        controls.set_transition_duration(0);
        controls.set_valign(gtk::Align::End);
        controls.set_halign(gtk::Align::Fill);
        controls.set_margin_start(12);
        controls.set_margin_end(12);
        controls.set_margin_bottom(12);
        controls.add(&hover);
        controls.set_reveal_child(true);
        overlay.add_overlay(&controls);
        let slot = gtk::Box::new(Orientation::Vertical, 0);
        slot.pack_start(&content, true, true, 0);
        let tile = Rc::new(Self {
            slot,
            content,
            title,
            expand,
            fullscreen: RefCell::new(None),
            controls,
            last_activity: Cell::new(Instant::now()),
            hovered: Cell::new(false),
            keyboard: Cell::new(false),
            timer: RefCell::new(None),
        });
        let weak = Rc::downgrade(&tile);
        let parent = parent.downgrade();
        area.set_can_focus(true);
        let area_for_focus = area.clone();
        tile.expand.connect_clicked(move |_| {
            let Some(tile) = weak.upgrade() else {
                return;
            };
            let active = tile.fullscreen.borrow().clone();
            if active.is_some() {
                tile.restore();
                return;
            }
            let full = gtk::Window::new(gtk::WindowType::Toplevel);
            full.set_title(&tile.title.text());
            if let Some(parent) = parent.upgrade() {
                full.set_transient_for(Some(&parent));
                full.set_application(parent.application().as_ref());
            }
            full.set_destroy_with_parent(true);
            full.set_decorated(false);
            tile.slot.remove(&tile.content);
            full.add(&tile.content);
            icons::set(
                &tile.expand,
                Icon::Restore,
                "Sair da tela cheia (Esc)",
                false,
            );
            let weak = Rc::downgrade(&tile);
            full.connect_delete_event(move |_, _| {
                if let Some(tile) = weak.upgrade() {
                    tile.restore();
                }
                gtk::glib::Propagation::Proceed
            });
            let weak = Rc::downgrade(&tile);
            full.connect_key_press_event(move |_, event| {
                if let Some(tile) = weak.upgrade() {
                    tile.activity();
                    tile.keyboard.set(true);
                }
                if event.keyval() == gtk::gdk::keys::constants::Escape {
                    if let Some(tile) = weak.upgrade() {
                        tile.restore();
                    }
                    gtk::glib::Propagation::Stop
                } else {
                    gtk::glib::Propagation::Proceed
                }
            });
            *tile.fullscreen.borrow_mut() = Some(full.clone());
            full.fullscreen();
            full.show_all();
            full.present();
            area_for_focus.grab_focus();
            tile.activity();
        });

        // Duplo clique na área de vídeo alterna tela cheia.
        let tile_for_dclick = Rc::downgrade(&tile);
        area.add_events(gdk::EventMask::BUTTON_PRESS_MASK | gdk::EventMask::POINTER_MOTION_MASK);
        area.connect_button_press_event(move |area, event| {
            area.grab_focus();
            if event.event_type() == gdk::EventType::DoubleButtonPress {
                if let Some(tile) = tile_for_dclick.upgrade() {
                    tile.expand.emit_clicked();
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        let weak = Rc::downgrade(&tile);
        area.connect_motion_notify_event(move |_, _| {
            if let Some(tile) = weak.upgrade() {
                tile.activity();
            }
            gtk::glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(&tile);
        hover.connect_enter_notify_event(move |_, _| {
            if let Some(tile) = weak.upgrade() {
                tile.hovered.set(true);
                tile.activity();
            }
            gtk::glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(&tile);
        hover.connect_leave_notify_event(move |_, _| {
            if let Some(tile) = weak.upgrade() {
                tile.hovered.set(false);
                tile.activity();
            }
            gtk::glib::Propagation::Proceed
        });
        tile
    }
    fn activity(self: &Rc<Self>) {
        self.keyboard.set(false);
        self.last_activity.set(Instant::now());
        self.controls.set_reveal_child(true);
        if self.fullscreen.borrow().is_some() && self.timer.borrow().is_none() {
            self.arm(Duration::from_secs(3));
        }
    }
    fn arm(self: &Rc<Self>, delay: Duration) {
        let weak = Rc::downgrade(self);
        *self.timer.borrow_mut() = Some(gtk::glib::timeout_add_local_once(delay, move || {
            if let Some(tile) = weak.upgrade() {
                tile.timer.borrow_mut().take();
                tile.expire();
            }
        }));
    }
    fn expire(self: &Rc<Self>) {
        let full = self.fullscreen.borrow().clone();
        let focused = full
            .as_ref()
            .and_then(|w| w.focused_widget())
            .is_some_and(|w| w.is_ancestor(&self.controls));
        let idle = self.last_activity.get().elapsed();
        if hide_due(
            full.is_some(),
            idle,
            self.hovered.get() || (self.keyboard.get() && focused),
        ) {
            self.controls.set_reveal_child(false);
        } else if full.is_some() {
            self.arm(
                Duration::from_secs(3)
                    .saturating_sub(idle)
                    .max(Duration::from_millis(500)),
            );
        }
    }
}
impl Drop for Tile {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.get_mut().take() {
            timer.remove();
        }
        if let Some(window) = self.fullscreen.get_mut().take() {
            window.close();
        }
    }
}

pub struct Stage {
    pub grid: gtk::Grid,
    tiles: HashMap<u64, Rc<Tile>>,
    empty: gtk::Label,
}
impl Stage {
    pub fn new() -> Self {
        let grid = gtk::Grid::new();
        grid.set_row_spacing(12);
        grid.set_column_spacing(12);
        grid.set_row_homogeneous(true);
        grid.set_column_homogeneous(true);
        grid.set_hexpand(true);
        grid.set_vexpand(true);
        let empty = super::ui::label("Nenhuma transmissão aberta", "empty-title");
        empty.set_halign(gtk::Align::Center);
        empty.set_valign(gtk::Align::Center);
        grid.attach(&empty, 0, 0, 1, 1);
        Self {
            grid,
            tiles: HashMap::new(),
            empty,
        }
    }
    pub fn update(
        &mut self,
        watching: &[u64],
        people: &[Person],
        window: &gtk::ApplicationWindow,
        tx: &Commands,
        error: &gtk::Label,
    ) {
        self.tiles.retain(|id, tile| {
            if watching.contains(id) {
                true
            } else {
                self.grid.remove(&tile.slot);
                false
            }
        });
        let columns = if watching.len() > 1 && self.grid.allocated_width() >= 900 {
            2
        } else {
            1
        };
        for (index, id) in watching.iter().enumerate() {
            let name = people
                .iter()
                .find(|p| p.id == *id)
                .map(|p| p.name.as_str())
                .unwrap_or("Conectando…");
            let tile = self.tiles.entry(*id).or_insert_with(|| {
                let tile = Tile::new(*id, name, window, tx, error);
                self.grid.attach(&tile.slot, 0, 0, 1, 1);
                tile
            });
            tile.title.set_text(name);
            self.grid
                .child_set_property(&tile.slot, "left-attach", &((index % columns) as i32));
            self.grid
                .child_set_property(&tile.slot, "top-attach", &((index / columns) as i32));
        }
        self.grid.show_all();
        self.empty.set_visible(watching.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn controls_hide_only_after_fullscreen_idle_and_not_during_interaction() {
        assert!(!hide_due(false, Duration::from_secs(60), false));
        assert!(!hide_due(true, Duration::from_millis(2999), false));
        assert!(hide_due(true, Duration::from_secs(3), false));
        assert!(!hide_due(true, Duration::from_secs(30), true));
    }
    #[test]
    #[ignore = "requires a graphical desktop; run with --ignored --test-threads=1"]
    fn tiles_keep_controls_and_restore_fullscreen_on_close() {
        gtk::init().unwrap();
        let app = gtk::Application::new(None, gtk::gio::ApplicationFlags::NON_UNIQUE);
        app.register(None::<&gtk::gio::Cancellable>).unwrap();
        let window = gtk::ApplicationWindow::new(&app);
        super::super::ui::check_participant_layout(&window);
        super::super::source_picker::check_picker(&window);
        let mut stage = Stage::new();
        window.add(&stage.grid);
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        let error = gtk::Label::new(None);
        let people = vec![Person {
            id: 2,
            name: "Teste".into(),
            streaming: true,
        }];
        stage.update(&[2], &people, &window, &tx, &error);
        let tile = stage.tiles[&2].clone();
        stage.update(&[2], &people, &window, &tx, &error);
        assert!(Rc::ptr_eq(&tile, &stage.tiles[&2]));
        tile.expand.emit_clicked();
        assert!(tile.fullscreen.borrow().is_some());
        if let Some(timer) = tile.timer.borrow_mut().take() {
            timer.remove();
        }
        tile.last_activity
            .set(Instant::now() - Duration::from_secs(4));
        tile.expire();
        assert!(!tile.controls.reveals_child());
        tile.activity();
        assert!(tile.controls.reveals_child());
        tile.expand.emit_clicked();
        while gtk::events_pending() {
            gtk::main_iteration();
        }
        assert!(tile.fullscreen.borrow().is_none());
        for _ in 0..3 {
            tile.expand.emit_clicked();
            assert!(tile.fullscreen.borrow().is_some());
            tile.restore();
            assert!(
                tile.fullscreen.borrow().is_none(),
                "Exiting must not re-enter fullscreen"
            );
        }
        assert_eq!(
            tile.content.parent().unwrap(),
            tile.slot.clone().upcast::<gtk::Widget>()
        );
        stage.update(&[], &people, &window, &tx, &error);
        assert!(stage.tiles.is_empty());
        assert!(rx.try_recv().is_err());
        window.close();
    }
}
