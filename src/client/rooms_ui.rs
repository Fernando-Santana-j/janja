use super::{Command, Commands, RoomInfo};
use gtk::{Orientation, prelude::*};

pub struct Lobby {
    pub widget: gtk::Box,
    list: gtk::ListBox,
}
impl Lobby {
    pub fn new(window: &gtk::ApplicationWindow, tx: &Commands, error: &gtk::Label) -> Self {
        let widget = gtk::Box::new(Orientation::Vertical, 16);
        widget.style_context().add_class("panel");
        widget.set_margin_top(24);
        widget.set_margin_bottom(24);
        widget.set_margin_start(48);
        widget.set_margin_end(48);
        let header = gtk::Box::new(Orientation::Horizontal, 12);
        header.pack_start(&super::ui::label("Salas", "title"), true, true, 0);
        let refresh = gtk::Button::with_label("Atualizar");
        let commands = tx.clone();
        let errors = error.clone();
        refresh.connect_clicked(move |_| {
            super::ui::submit(&commands, Command::ListRooms, &errors);
        });
        header.pack_end(&refresh, false, false, 0);
        let create = gtk::Button::with_label("Criar sala");
        create.style_context().add_class("primary");
        let commands = tx.clone();
        let errors = error.clone();
        let parent = window.downgrade();
        create.connect_clicked(move |_| {
            if let Some(window) = parent.upgrade() {
                credentials(&window, None, &commands, &errors);
            }
        });
        header.pack_end(&create, false, false, 0);
        widget.pack_start(&header, false, false, 0);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_valign(gtk::Align::Start);
        list.style_context().add_class("participants");
        let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.add(&list);
        widget.pack_start(&scroll, true, true, 0);
        Self { widget, list }
    }
    pub fn update(
        &self,
        rooms: &[RoomInfo],
        window: &gtk::ApplicationWindow,
        tx: &Commands,
        error: &gtk::Label,
    ) {
        for child in self.list.children() {
            self.list.remove(&child);
        }
        if rooms.is_empty() {
            self.list.add(&super::ui::label(
                "Nenhuma sala disponível. Crie uma para começar.",
                "muted",
            ));
        }
        for room in rooms {
            let row = gtk::Box::new(Orientation::Horizontal, 16);
            row.style_context().add_class("person-card");
            let text = gtk::Box::new(Orientation::Vertical, 4);
            let name = super::ui::label(&room.name, "video-name");
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.pack_start(&name, false, false, 0);
            text.pack_start(
                &super::ui::label(
                    &format!(
                        "{} / 8 pessoas{}",
                        room.members,
                        if room.locked { " · Com senha" } else { "" }
                    ),
                    "muted",
                ),
                false,
                false,
                0,
            );
            row.pack_start(&text, true, true, 0);
            let enter = gtk::Button::with_label("Entrar");
            enter.set_sensitive(room.members < 8);
            enter.set_valign(gtk::Align::Center);
            let selected = room.clone();
            let parent = window.downgrade();
            let commands = tx.clone();
            let errors = error.clone();
            enter.connect_clicked(move |_| {
                if !selected.locked {
                    super::ui::submit(
                        &commands,
                        Command::EnterRoom(selected.id, String::new()),
                        &errors,
                    );
                } else if let Some(window) = parent.upgrade() {
                    credentials(&window, Some(&selected), &commands, &errors);
                }
            });
            row.pack_end(&enter, false, false, 0);
            self.list.add(&row);
        }
        self.list.show_all();
    }
}
fn credentials(
    window: &gtk::ApplicationWindow,
    room: Option<&RoomInfo>,
    tx: &Commands,
    error: &gtk::Label,
) {
    let dialog = gtk::Dialog::with_buttons(
        Some(if room.is_some() {
            "Entrar na sala"
        } else {
            "Criar sala"
        }),
        Some(window),
        gtk::DialogFlags::MODAL,
        &[
            ("Cancelar", gtk::ResponseType::Cancel),
            (
                if room.is_some() { "Entrar" } else { "Criar" },
                gtk::ResponseType::Accept,
            ),
        ],
    );
    let fields = gtk::Box::new(Orientation::Vertical, 10);
    fields.set_margin_top(20);
    fields.set_margin_bottom(20);
    fields.set_margin_start(20);
    fields.set_margin_end(20);
    let name = gtk::Entry::new();
    name.set_max_length(32);
    name.set_placeholder_text(Some("Nome da sala"));
    if let Some(room) = room {
        fields.pack_start(&super::ui::label(&room.name, "title"), false, false, 0);
    } else {
        fields.pack_start(&name, false, false, 0);
    }
    let password = gtk::Entry::new();
    password.set_visibility(false);
    password.set_max_length(128);
    password.set_placeholder_text(Some(if room.is_some() {
        "Senha"
    } else {
        "Senha opcional (8–128 bytes)"
    }));
    password.set_input_purpose(gtk::InputPurpose::Password);
    password.set_activates_default(true);
    fields.pack_start(&password, false, false, 0);
    let validation = super::ui::label("", "error");
    fields.pack_start(&validation, false, false, 0);
    dialog.content_area().add(&fields);
    dialog.set_default_response(gtk::ResponseType::Accept);
    dialog.show_all();
    loop {
        if dialog.run() != gtk::ResponseType::Accept {
            break;
        }
        let secret = password.text().to_string();
        if room.is_none()
            && (name.text().trim().is_empty()
                || (!secret.is_empty() && !(8..=128).contains(&secret.len())))
        {
            validation
                .set_text("Informe o nome. A senha é opcional; se preenchida, use 8–128 bytes.");
            continue;
        }
        let command = if let Some(room) = room {
            Command::EnterRoom(room.id, secret)
        } else {
            Command::CreateRoom(name.text().to_string(), secret)
        };
        if super::ui::submit(tx, command, error) {
            break;
        }
    }
    password.set_text("");
    dialog.close();
}
