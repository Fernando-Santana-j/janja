use super::{Command, Commands, RoomInfo};
use gtk::{Orientation, prelude::*};

pub struct Lobby {
    pub widget: gtk::Box,
    list: gtk::ListBox,
    count: gtk::Label,
    empty_title: gtk::Label,
    empty_hint: gtk::Label,
}
impl Lobby {
    pub fn new(window: &gtk::ApplicationWindow, tx: &Commands, error: &gtk::Label) -> Self {
        let widget = gtk::Box::new(Orientation::Vertical, 16);
        widget.style_context().add_class("lobby");
        let intro = gtk::Box::new(Orientation::Vertical, 8);
        intro.style_context().add_class("lobby-intro");
        intro.pack_start(
            &super::ui::label("COMPARTILHAMENTO DE TELA", "section-label"),
            false,
            false,
            0,
        );
        intro.pack_start(
            &super::ui::label("Encontre sua sala", "title"),
            false,
            false,
            0,
        );
        intro.pack_start(
            &super::ui::label(
                "Entre com sua turma ou crie um espaço para compartilhar.",
                "empty-description",
            ),
            false,
            false,
            0,
        );
        widget.pack_start(&intro, false, false, 0);
        let header = gtk::Box::new(Orientation::Horizontal, 12);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Buscar sala pelo nome"));
        search.set_tooltip_text(Some("Filtrar as salas disponíveis pelo nome"));
        header.pack_start(&search, true, true, 0);
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
            if let Some(window) = parent.upgrade()
                && super::ui::submit(&commands, Command::CancelRoomLookup, &errors)
            {
                credentials(&window, None, None, &commands, &errors);
            }
        });
        header.pack_end(&create, false, false, 0);
        widget.pack_start(&header, false, false, 0);
        let count = super::ui::label("BUSCANDO SALAS…", "section-label");
        widget.pack_start(&count, false, false, 0);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.set_valign(gtk::Align::Start);
        list.style_context().add_class("participants");
        let search_filter = search.downgrade();
        list.set_filter_func(Some(Box::new(move |row| {
            let Some(search) = search_filter.upgrade() else {
                return true;
            };
            let query = search.text().trim().to_lowercase();
            row.tooltip_text()
                .is_some_and(|name| name.to_lowercase().contains(&query))
        })));
        let filtered_list = list.downgrade();
        search.connect_search_changed(move |_| {
            if let Some(list) = filtered_list.upgrade() {
                list.invalidate_filter();
            }
        });
        let empty = gtk::Box::new(Orientation::Vertical, 10);
        empty.set_halign(gtk::Align::Center);
        empty.set_valign(gtk::Align::Center);
        empty.style_context().add_class("empty-state");
        let empty_title = super::ui::label("Buscando salas…", "empty-title");
        let empty_hint = super::ui::label(
            "A lista será atualizada quando a conexão estiver pronta.",
            "empty-description",
        );
        empty_hint.set_line_wrap(true);
        empty_hint.set_max_width_chars(48);
        empty.pack_start(&empty_title, false, false, 0);
        empty.pack_start(&empty_hint, false, false, 0);
        empty.show_all();
        list.set_placeholder(Some(&empty));
        let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.add(&list);
        widget.pack_start(&scroll, true, true, 0);
        Self {
            widget,
            list,
            count,
            empty_title,
            empty_hint,
        }
    }
    pub fn update(
        &self,
        rooms: &[RoomInfo],
        _window: &gtk::ApplicationWindow,
        tx: &Commands,
        error: &gtk::Label,
    ) {
        for child in self.list.children() {
            self.list.remove(&child);
        }
        self.count
            .set_text(&format!("SALAS DISPONÍVEIS · {}", rooms.len()));
        self.empty_title.set_text(if rooms.is_empty() {
            "A primeira sala pode ser sua"
        } else {
            "Nenhuma sala encontrada"
        });
        self.empty_hint.set_text(if rooms.is_empty() {
            "Use Criar sala para começar. Convide até mais 7 pessoas."
        } else {
            "Tente outro nome ou limpe a busca para ver todas as salas."
        });
        for room in rooms {
            let row = gtk::Box::new(Orientation::Horizontal, 16);
            row.style_context().add_class("room-card");
            let initial = room
                .name
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            let avatar = super::ui::label(&initial, "avatar");
            avatar.set_xalign(0.5);
            avatar.set_valign(gtk::Align::Center);
            avatar.set_size_request(44, 44);
            row.pack_start(&avatar, false, false, 0);
            let text = gtk::Box::new(Orientation::Vertical, 4);
            let name = super::ui::label(&room.name, "video-name");
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.pack_start(&name, false, false, 0);
            text.pack_start(
                &super::ui::label(
                    &format!(
                        "{} / 8 pessoas{}",
                        room.members,
                        if room.locked {
                            " · Protegida por senha"
                        } else {
                            " · Acesso livre"
                        }
                    ),
                    "muted",
                ),
                false,
                false,
                0,
            );
            row.pack_start(&text, true, true, 0);
            let enter = gtk::Button::with_label(if room.members < 8 {
                "Entrar na sala"
            } else {
                "Sala cheia"
            });
            enter.set_sensitive(room.members < 8);
            enter.set_valign(gtk::Align::Center);
            let selected = room.clone();
            let commands = tx.clone();
            let errors = error.clone();
            enter.connect_clicked(move |_| {
                super::ui::submit(&commands, Command::RequestRoom(selected.clone()), &errors);
            });
            row.pack_end(&enter, false, false, 0);
            let item = gtk::ListBoxRow::new();
            item.set_tooltip_text(Some(&room.name));
            item.add(&row);
            self.list.add(&item);
        }
        self.list.show_all();
    }
}
#[cfg(test)]
pub(super) fn check_lobby(window: &gtk::ApplicationWindow) {
    let (tx, rx) = std::sync::mpsc::sync_channel(4);
    let lobby = Lobby::new(window, &tx, &gtk::Label::new(None));
    window.add(&lobby.widget);
    window.show_all();
    assert_eq!(lobby.empty_title.text(), "Buscando salas…");
    lobby.update(
        &[
            RoomInfo {
                id: 1,
                name: "Sala principal".into(),
                members: 1,
                locked: false,
            },
            RoomInfo {
                id: 2,
                name: "Sala cheia".into(),
                members: 8,
                locked: true,
            },
        ],
        window,
        &tx,
        &gtk::Label::new(None),
    );
    let header = lobby.widget.children()[1]
        .clone()
        .downcast::<gtk::Box>()
        .unwrap();
    let search = header
        .children()
        .into_iter()
        .find_map(|widget| widget.downcast::<gtk::SearchEntry>().ok())
        .unwrap();
    search.set_text("  PRINCIPAL ");
    lobby.list.invalidate_filter();
    let rows = lobby.list.children();
    assert!(rows[0].is_child_visible());
    assert!(!rows[1].is_child_visible());
    search.set_text("inexistente");
    lobby.list.invalidate_filter();
    assert!(rows.iter().all(|row| !row.is_child_visible()));
    search.set_text("");
    lobby.list.invalidate_filter();
    let first = rows[0]
        .clone()
        .downcast::<gtk::ListBoxRow>()
        .unwrap()
        .child()
        .unwrap()
        .downcast::<gtk::Box>()
        .unwrap();
    let button = first
        .children()
        .into_iter()
        .find_map(|widget| widget.downcast::<gtk::Button>().ok())
        .unwrap();
    button.emit_clicked();
    assert!(matches!(rx.try_recv(), Ok(Command::RequestRoom(room)) if room.id == 1));
    let full = rows[1]
        .clone()
        .downcast::<gtk::ListBoxRow>()
        .unwrap()
        .child()
        .unwrap()
        .downcast::<gtk::Box>()
        .unwrap();
    let button = full
        .children()
        .into_iter()
        .find_map(|widget| widget.downcast::<gtk::Button>().ok())
        .unwrap();
    assert!(!button.is_sensitive());
    window.remove(&lobby.widget);
    window.hide();
}

pub fn password_required(
    window: &gtk::ApplicationWindow,
    epoch: u64,
    request: u64,
    room: &RoomInfo,
    tx: &Commands,
    error: &gtk::Label,
) {
    credentials(window, Some(room), Some((epoch, request)), tx, error);
}
fn credentials(
    window: &gtk::ApplicationWindow,
    room: Option<&RoomInfo>,
    request: Option<(u64, u64)>,
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
        let label = super::ui::label("Nome da sala", "");
        label.set_mnemonic_widget(Some(&name));
        fields.pack_start(&label, false, false, 0);
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
    let password_label = super::ui::label(
        if room.is_some() {
            "Senha da sala"
        } else {
            "Senha (opcional)"
        },
        "",
    );
    password_label.set_mnemonic_widget(Some(&password));
    fields.pack_start(&password_label, false, false, 0);
    fields.pack_start(&password, false, false, 0);
    if room.is_some() {
        let remember = super::ui::label(
            "Após entrar, a senha será lembrada pelo cofre do sistema quando disponível.",
            "muted",
        );
        remember.set_line_wrap(true);
        remember.set_max_width_chars(48);
        fields.pack_start(&remember, false, false, 0);
    }
    let validation = super::ui::label("", "error");
    validation.set_line_wrap(true);
    validation.set_max_width_chars(48);
    if room.is_some() {
        validation.set_text(&error.text());
    }
    fields.pack_start(&validation, false, false, 0);
    dialog.content_area().add(&fields);
    dialog.set_default_response(gtk::ResponseType::Accept);
    dialog.show_all();
    loop {
        if dialog.run() != gtk::ResponseType::Accept {
            if let Some((epoch, request)) = request {
                let _ = tx.try_send(Command::CancelRoomRequest(epoch, request));
            }
            break;
        }
        let secret = password.text().to_string();
        if (room.is_none() && name.text().trim().is_empty())
            || ((room.is_some() || !secret.is_empty()) && !(8..=128).contains(&secret.len()))
        {
            validation
                .set_text("Informe o nome. A senha é opcional; se preenchida, use 8–128 bytes.");
            continue;
        }
        let command = if let (Some(room), Some((epoch, request))) = (room, request) {
            Command::RoomPassword(epoch, request, room.clone(), secret)
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
