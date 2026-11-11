use super::icons::{self, Icon};
use super::{Command, Commands, Event, Person, Settings};
use gtk::{Orientation, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const CSS: &str = r#"
* { font-family: "Segoe UI", "Noto Sans", sans-serif; font-size: 14px; color: #f2f4f8; text-shadow: none; }
window, dialog { background: #0e1420; }
.title { font-size: 26px; font-weight: 600; }
.muted { color: #b9c2d0; font-size: 12px; }
.panel { background: #182338; border: 1px solid #363d49; border-radius: 12px; padding: 14px; }
.video-tile { background: #14171c; border: 1px solid #363d49; border-radius: 10px; }
.video-toolbar { background: rgba(20, 26, 38, 0.94); padding: 8px 12px; border-radius: 8px; }
button.icon-button { min-width: 18px; min-height: 18px; padding: 8px; }
.video-name { color: #f2f4f8; font-weight: 600; }
button { background-image: none; background-color: #353c48; color: #f2f4f8; border: 1px solid #4c586b; border-radius: 7px; padding: 8px 12px; box-shadow: none; }
button:hover { background-color: #465267; color: #ffffff; }
button:active, button:checked { background-color: #536485; color: #ffffff; }
button:focus, entry:focus, scale:focus { border-color: #a6b6ff; outline: 2px solid #a6b6ff; outline-offset: 1px; }
button:disabled, button:disabled label { color: #aeb7c4; background-color: #303640; }
button.primary { background-color: #83bcff; color: #0e2138; border-color: #83bcff; font-weight: 700; }
button.primary label { color: #0e2138; }
button.primary drawingarea { color: #0e2138; }
button.primary:disabled, button.primary:disabled label { background-color: #303640; color: #b9c2d0; }
button.primary:hover { background-color: #a5b4ff; }
button.stop, button.stop label { background-color: #5b303b; border-color: #bb7085; color: #ffe8ed; }
list, row { background: transparent; }
row { padding: 0; }
.participants row { margin-bottom: 6px; }
.error { color: #ffb4ae; }
entry, spinbutton, spinbutton entry, combobox button { background: #202630; color: #f2f4f8; border: 1px solid #566277; border-radius: 6px; }
entry { padding: 8px; caret-color: #ffffff; }
entry selection, selection { background-color: #526cb2; color: #ffffff; }
popover, menu, menuitem, tooltip { background: #292f3a; color: #f2f4f8; border-color: #566277; }
menuitem:hover, menuitem:selected { background: #465a82; color: #ffffff; }
checkbutton { padding: 5px 0; }
checkbutton check { background: #202630; color: #ffffff; border: 1px solid #7f8da5; border-radius: 4px; }
checkbutton check:checked { background: #526bc9; color: #ffffff; }
checkbutton:disabled, checkbutton:disabled label { color: #aeb7c4; }
scale trough { background: #161c25; min-height: 4px; border-radius: 3px; }
scale highlight { background: #83bcff; min-height: 4px; border-radius: 3px; }
scale slider { background: #e4eaff; border: none; min-width: 12px; min-height: 12px; border-radius: 8px; box-shadow: none; }
scrollbar, scrollbar trough { background: #0e1420; }
scrollbar slider { background: #586479; border-radius: 5px; min-width: 6px; }
.person-card { background: #171f2e; border: 1px solid #283549; border-radius: 8px; padding: 10px; }
.person-card button { padding: 6px 10px; }
.avatar { background: #304b71; color: #d8e9ff; border-radius: 18px; min-width: 36px; min-height: 36px; font-size: 18px; font-weight: 600; }
.source-choice { padding: 10px; border: 1px solid #344255; border-radius: 8px; background: #171f2e; }
.source-choice:checked { background: #233957; border-color: #83bcff; }
.source-choice:hover { background: #233147; }
.source-choice:focus { outline: 2px solid #a6b6ff; outline-offset: 1px; }
.source-preview { background: #080e18; }
.room-header { padding: 4px 8px 10px; }
.room-title { font-size: 18px; font-weight: 600; color: #f2f4f8; }
.profile-pill { background: #182338; border-radius: 16px; color: #bbd2f1; padding: 7px 14px; }
.stage-shell { background: #080e18; border: 1px solid #263650; border-radius: 10px; padding: 0; }
.dock { background: #171f2e; border: 1px solid #283549; border-radius: 8px; padding: 10px 12px; }
.empty-title { font-size: 20px; color: #c4d5ec; font-weight: 500; }
/* Sidebar de participantes */
.sidebar { background: #0e1420; border-right: 1px solid #263650; }
/* Vídeo em tela cheia: sem decorações, preto absoluto */
.fullscreen-window { background: #000000; }
"#;
pub(super) fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.style_context().add_class(class);
    l
}
pub(super) fn submit(tx: &Commands, command: Command, error: &gtk::Label) -> bool {
    if tx.try_send(command).is_err() {
        error.set_text("Aguarde a operação atual e tente novamente.");
        false
    } else {
        error.set_text("");
        true
    }
}
fn participant_list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_valign(gtk::Align::Start);
    list.style_context().add_class("participants");
    list
}
fn populate(
    list: &gtk::ListBox,
    people: &[Person],
    id: u64,
    watching: &[u64],
    tx: &Commands,
    error: &gtk::Label,
) {
    for child in list.children() {
        list.remove(&child);
    }
    if people.iter().all(|p| p.id == id) {
        let l = label("Nenhuma outra pessoa conectada", "muted");
        l.set_margin_top(20);
        list.add(&l);
    }
    for person in people.iter().filter(|p| p.id != id) {
        let row = gtk::Box::new(Orientation::Horizontal, 12);
        row.style_context().add_class("person-card");
        row.set_valign(gtk::Align::Start);
        let initial = person
            .name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .to_string();
        let avatar = label(&initial, "avatar");
        avatar.set_xalign(0.5);
        avatar.set_valign(gtk::Align::Center);
        avatar.set_size_request(36, 36);
        row.pack_start(&avatar, false, false, 0);
        let text = gtk::Box::new(Orientation::Vertical, 4);
        text.set_valign(gtk::Align::Center);
        let name = label(&person.name, "");
        name.set_max_width_chars(14);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_tooltip_text(Some(&person.name));
        text.pack_start(&name, false, false, 0);
        text.pack_start(
            &label(
                if person.streaming {
                    "Transmitindo"
                } else {
                    "Na sala"
                },
                "muted",
            ),
            false,
            false,
            0,
        );
        row.pack_start(&text, true, true, 0);
        if person.streaming {
            let watched = watching.contains(&person.id);
            let button = icons::button(
                if watched { Icon::Close } else { Icon::Watch },
                if watched {
                    "Fechar transmissão"
                } else {
                    "Assistir"
                },
            );
            button.set_valign(gtk::Align::Center);
            let tx = tx.clone();
            let error = error.clone();
            let target = person.id;
            button.connect_clicked(move |_| {
                submit(&tx, Command::Watch(target), &error);
            });
            row.pack_end(&button, false, false, 0);
        }
        list.add(&row);
    }
    list.show_all();
}
fn choose_name(window: &gtk::ApplicationWindow) -> Option<String> {
    if let Some(name) = super::profile::load() {
        return Some(name);
    }
    let dialog = gtk::Dialog::with_buttons(
        Some("Seu nome"),
        Some(window),
        gtk::DialogFlags::MODAL,
        &[
            ("Sair", gtk::ResponseType::Cancel),
            ("Entrar na sala", gtk::ResponseType::Accept),
        ],
    );
    let entry = gtk::Entry::new();
    entry.set_max_length(32);
    entry.set_placeholder_text(Some("Como você quer aparecer?"));
    entry.set_activates_default(true);
    dialog.set_default_response(gtk::ResponseType::Accept);
    let error = label("", "error");
    dialog.content_area().pack_start(&entry, false, false, 12);
    dialog.content_area().pack_start(&error, false, false, 8);
    dialog.show_all();
    let name = loop {
        if dialog.run() != gtk::ResponseType::Accept {
            break None;
        }
        match super::profile::path().and_then(|p| super::profile::save(&p, entry.text().as_str())) {
            Ok(name) => break Some(name),
            Err(message) => error.set_text(&message),
        }
    };
    dialog.close();
    name
}
pub fn run() {
    let app = gtk::Application::new(Some("br.com.infocoretech.janja"), Default::default());
    app.connect_activate(|app| {
        if let Some(w) = app.active_window() {
            w.present();
            return;
        }
        let icon = gtk::gdk_pixbuf::PixbufLoader::new();
        if icon.write(include_bytes!("../../assets/janja.png")).is_ok() && icon.close().is_ok()
            && let Some(icon) = icon.pixbuf() {
            gtk::Window::set_default_icon_list(&[icon]);
        }
        let window = gtk::ApplicationWindow::new(app);
        window.set_title("Janja");
        window.set_default_size(1280, 760);
        window.set_size_request(960, 540);
        let provider = gtk::CssProvider::new();
        if let Err(_error) = provider.load_from_data(CSS.as_bytes()) {
            super::diagnostics::event!("ui-style-error", serde_json::json!({"error":_error.to_string()}));
        }
        if let Some(screen) = gtk::gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let outer = gtk::Box::new(Orientation::Vertical, 0);
        outer.set_margin_top(8);
        outer.set_margin_bottom(8);
        outer.set_margin_start(8);
        outer.set_margin_end(8);
        // Cabeçalho
        let header = gtk::Box::new(Orientation::Horizontal, 12);
        header.set_margin_bottom(8);
        header.style_context().add_class("room-header");
        let status = label("Conectando…", "muted");
        status.set_max_width_chars(24);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        header.pack_end(&status, false, false, 0);
        outer.pack_start(&header, false, false, 0);
        // Área principal: participantes à esquerda, vídeos no centro.
        let main = gtk::Box::new(Orientation::Horizontal, 12);
        main.set_hexpand(true);
        main.set_vexpand(true);
        // Sidebar esquerda: participantes.
        let sidebar = gtk::Box::new(Orientation::Vertical, 8);
        sidebar.set_width_request(272);
        sidebar.set_hexpand(false);
        sidebar.style_context().add_class("sidebar");
        let people_label = label("Participantes", "muted");
        people_label.set_margin_start(8);
        people_label.set_margin_end(8);
        sidebar.pack_start(&people_label, false, false, 0);
        let list = participant_list();
        let people_scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        people_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        people_scroll.set_min_content_height(120);
        people_scroll.set_vexpand(true);
        people_scroll.add(&list);
        sidebar.pack_start(&people_scroll, true, true, 0);
        main.pack_start(&sidebar, false, false, 0);
        // Centro: palco de vídeos.
        let content = gtk::Box::new(Orientation::Vertical, 0);
        content.style_context().add_class("stage-shell");
        content.set_hexpand(true);
        content.set_vexpand(true);
        let mut stage = super::stage::Stage::new();
        let video_scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        video_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        video_scroll.add(&stage.grid);
        video_scroll.set_hexpand(true);
        video_scroll.set_vexpand(true);
        content.pack_start(&video_scroll, true, true, 0);
        main.pack_start(&content, true, true, 0);
        outer.pack_start(&main, true, true, 0);
        // Dock inferior: controles.
        let dock = gtk::Box::new(Orientation::Horizontal, 12);
        dock.set_margin_top(8);
        dock.style_context().add_class("dock");
        let saved=super::preferences::path().and_then(|p|super::preferences::load(&p)).unwrap_or_else(|_error| {
            super::diagnostics::event!("quality-load-error",serde_json::json!({"error":_error})); Settings::default()
        });
        let controls = gtk::Box::new(Orientation::Vertical, 7);
        let fps = gtk::ComboBoxText::new();
        for n in ["15", "30", "60"] {
            fps.append_text(n);
        }
        fps.set_active(Some(match saved.fps {15=>0,60=>2,_=>1}));
        let resolution = gtk::ComboBoxText::new();
        for n in ["720p", "1080p", "1440p"] {
            resolution.append_text(n);
        }
        resolution.set_active(Some(match saved.height {720=>0,1440=>2,_=>1}));
        for (title, widget) in [("FPS", &fps), ("Resolução", &resolution)] {
            let b = gtk::Box::new(Orientation::Vertical, 4);
            b.pack_start(&label(title, "muted"), false, false, 0);
            b.pack_start(widget, false, false, 0);
            controls.pack_start(&b, false, false, 0);
        }
        let bitrate = gtk::SpinButton::with_range(1.0, 20.0, 0.5);
        bitrate.set_value(saved.bitrate as f64/1000.0);
        bitrate.set_digits(1);
        bitrate.set_tooltip_text(Some(
            "Upload por espectador. Reduza se a conexão não acompanhar.",
        ));
        controls.pack_start(&label("Envio (Mbps)", "muted"), false, false, 0);
        controls.pack_start(&bitrate, false, false, 0);
        let audio = gtk::CheckButton::with_label("Áudio do computador");
        audio.set_active(saved.audio);
        audio.set_tooltip_text(Some("Compartilha o som do computador. Para evitar retorno, zere o volume do vídeo que estiver assistindo."));
        let discord = gtk::CheckButton::with_label("Excluir Discord");
        discord.set_sensitive(cfg!(target_os = "windows"));
        discord.set_active(cfg!(windows) && saved.exclude_discord);
        discord.set_tooltip_text(Some(if cfg!(target_os = "windows") {
            "Requer Windows 11 ou versão compatível com captura de áudio por processo."
        } else {
            "Ainda indisponível no Linux."
        }));
        controls.set_margin_top(16);
        controls.set_margin_bottom(16);
        controls.set_margin_start(16);
        controls.set_margin_end(16);
        controls.pack_start(&audio, false, false, 0);
        controls.pack_start(&discord, false, false, 0);
        let optimize=gtk::Button::with_label("Ajustar para este PC");
        optimize.set_tooltip_text(Some("Mede a codificação local e sugere um perfil. Não mede captura real, conexão ou desempenho dos espectadores."));
        controls.pack_start(&optimize,false,false,4);
        let configure = icons::button(Icon::Settings, "Qualidade da transmissão");
        let popover = gtk::Popover::new(Some(&configure));
        popover.add(&controls);
        configure.connect_clicked(move |_| { popover.show_all(); popover.popup(); });
        dock.pack_start(&configure, false, false, 0);
        let transmit = gtk::Button::with_label("Transmitir");
        icons::set(&transmit,Icon::Cast,"Transmitir",true);
        transmit.style_context().add_class("primary");
        transmit.set_sensitive(false);
        configure.set_valign(gtk::Align::Center);
        transmit.set_valign(gtk::Align::Center);
        dock.pack_end(&transmit, false, false, 0);
        outer.pack_start(&dock, false, false, 0);
        let media_info = label("", "muted"); media_info.set_selectable(true);
        media_info.set_ellipsize(gtk::pango::EllipsizeMode::End);
        media_info.set_hexpand(true);
        dock.pack_start(&media_info, true, true, 4);
        let error = label("", "error");
        error.set_no_show_all(true);
        error.connect_notify_local(Some("label"), |label, _| {
            label.set_visible(!label.text().is_empty());
        });
        error.set_line_wrap(true);
        error.set_max_width_chars(85);
        error.set_lines(4); error.set_ellipsize(gtk::pango::EllipsizeMode::End);
        error.set_selectable(true);
        outer.pack_start(&error, false, false, 0);
        window.add(&outer);
        if let Err(e) = gst::init() {
            error.set_text(&format!("Não foi possível iniciar o sistema de vídeo: {e}"));
            window.show_all();
            return;
        }
        if gst::version().0 < 1 || (gst::version().0 == 1 && gst::version().1 < 22) {
            error.set_text("É necessário GStreamer 1.22 ou mais recente.");
            window.show_all();
            return;
        }
        #[allow(deprecated)]
        let (events, receive) = gtk::glib::MainContext::channel(gtk::glib::Priority::DEFAULT);
        let Some(name) = choose_name(&window) else { app.quit(); return; };
        let profile = label(&name, "profile-pill");
        profile.set_max_width_chars(16); profile.set_ellipsize(gtk::pango::EllipsizeMode::End); profile.set_tooltip_text(Some(&name));
        header.pack_end(&profile, false, false, 0);
        let tx = super::network::start(events.clone(), name);
        let lobby = super::rooms_ui::Lobby::new(&window, &tx, &error);
        outer.pack_start(&lobby.widget, true, true, 0);
        outer.reorder_child(&lobby.widget, 1);
        let room_title = label("", "room-title");
        room_title.set_no_show_all(true);
        room_title.set_max_width_chars(20); room_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        header.pack_start(&room_title, true, true, 8);
        let leave = icons::button(Icon::Leave, "Sair da sala");
        leave.set_no_show_all(true);
        let leave_tx = tx.clone(); let leave_error = error.clone();
        leave.connect_clicked(move |_| { submit(&leave_tx, Command::LeaveRoom, &leave_error); });
        header.pack_end(&leave, false, false, 0);
        main.set_no_show_all(true); dock.set_no_show_all(true);
        main.hide(); dock.hide();
        lobby.update(&[], &window, &tx, &error);
        let in_room = Rc::new(Cell::new(false));

        let streaming = Rc::new(Cell::new(false));
        let people = Rc::new(RefCell::new(Vec::<Person>::new()));
        let my_id = Rc::new(Cell::new(0));
        let watching = Rc::new(RefCell::new(Vec::<u64>::new()));
        let calibrating=Rc::new(Cell::new(false));
        let quality:Rc<dyn Fn()->Settings>={
            let (fps,resolution,bitrate,audio,discord)=(fps.downgrade(),resolution.downgrade(),bitrate.downgrade(),audio.downgrade(),discord.downgrade());
            Rc::new(move || {
                let (Some(fps),Some(resolution),Some(bitrate),Some(audio),Some(discord))=(fps.upgrade(),resolution.upgrade(),bitrate.upgrade(),audio.upgrade(),discord.upgrade()) else{return Settings::default();};
                Settings { fps:fps.active_text().and_then(|t|t.parse().ok()).unwrap_or(30), height:match resolution.active(){Some(0)=>720,Some(2)=>1440,_=>1080}, bitrate:(bitrate.value()*1000.0) as u32, audio:audio.is_active(), exclude_discord:discord.is_active(), source:0 }})
        };
        let pending=Rc::new(RefCell::new(None::<gtk::glib::SourceId>));
        let save:Rc<dyn Fn()>={
            let quality=quality.clone(); let error=error.downgrade(); let pending=pending.clone();
            Rc::new(move || {
                if let Some(timer)=pending.borrow_mut().take(){timer.remove();}
                let (quality,error,pending_callback)=(quality.clone(),error.clone(),pending.clone());
                let timer=gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(350),move || {
                    pending_callback.borrow_mut().take();
                    if let Err(e)=super::preferences::path().and_then(|p|super::preferences::save(&p,&quality()))
                        && let Some(error)=error.upgrade(){error.set_text(&format!("Não foi possível salvar a qualidade: {e}"));}
                });
                *pending.borrow_mut()=Some(timer);
            })
        };
        for combo in [&fps,&resolution] {let save=save.clone();combo.connect_changed(move |_|save());}
        {let save=save.clone();bitrate.connect_value_changed(move |_|save());}
        for check in [&audio,&discord] {let save=save.clone();check.connect_toggled(move |_|save());}
        let optimizer_events=events.clone(); let optimizer_quality=quality.clone(); let busy=calibrating.clone();
        let optimizer_controls=controls.downgrade(); let optimizer_transmit=transmit.downgrade();
        optimize.connect_clicked(move |button| {
            button.set_label("Medindo codificador…");
            if busy.replace(true){return;}
            if let Some(c)=optimizer_controls.upgrade(){c.set_sensitive(false);}
            if let Some(b)=optimizer_transmit.upgrade(){b.set_sensitive(false);}
            let (events,settings)=(optimizer_events.clone(),optimizer_quality());
            std::thread::spawn(move || {let result=super::media::recommend(&settings);let _=events.send(Event::Recommendation(result));});
        });
        let state = streaming.clone();
        let commands = tx.clone();
        let errors = error.clone();
        let config_controls = controls.clone();
        let source_window = window.downgrade();
        let current_quality=quality.clone(); let busy=calibrating.clone();
        transmit.connect_clicked(move |button| {
            if busy.get(){return;}
            let command = if state.get() {
                Command::Stop
            } else {
                let Some(source_window) = source_window.upgrade() else { return; };
                let Some(selected_source) = super::source_picker::choose(&source_window) else { return; };
                let mut settings=current_quality(); settings.source=selected_source;
                if let Err(e)=super::preferences::path().and_then(|p|super::preferences::save(&p,&settings)){errors.set_text(&format!("Não foi possível salvar a qualidade: {e}"));return;}
                Command::Start(settings)
            };
            if submit(&commands, command, &errors) {
                button.set_sensitive(false);
                config_controls.set_sensitive(false);
            }
        });
        let commands = tx.clone();
        let closing_quality=quality.clone(); let closing_pending=pending.clone();
        window.connect_delete_event(move |_, _| {
            if let Some(timer)=closing_pending.borrow_mut().take(){timer.remove();let _=super::preferences::path().and_then(|p|super::preferences::save(&p,&closing_quality()));}
            let _ = commands.try_send(Command::Quit);
            gtk::glib::Propagation::Proceed
        });
        let stage_window = window.clone();
        receive.attach(None, move |event| {
            match event {
                Event::Recommendation(result)=>{
                    optimize.set_label("Ajustar para este PC");
                    calibrating.set(false); controls.set_sensitive(!streaming.get()); transmit.set_sensitive(in_room.get() && my_id.get()!=0);
                    match result {
                        Ok((settings,report))=>{
                            fps.set_active(Some(match settings.fps{15=>0,60=>2,_=>1}));
                            resolution.set_active(Some(match settings.height{720=>0,1440=>2,_=>1}));
                            bitrate.set_value(settings.bitrate as f64/1000.0);
                            media_info.set_text(&report);media_info.set_tooltip_text(Some(&report));save();
                        }, Err(e)=>error.set_text(&e)
                    }
                }
                Event::Rooms(rooms) => { lobby.update(&rooms, &stage_window, &tx, &error); }
                Event::SelectedRoom(room) => {
                    let selected = room.is_some(); in_room.set(selected);
                    room_title.set_text(room.as_ref().map(|r|r.name.as_str()).unwrap_or("Salas"));
                    room_title.set_visible(selected);
                    leave.set_visible(selected); lobby.widget.set_visible(!selected);
                    if selected { main.set_no_show_all(false); dock.set_no_show_all(false); main.show_all(); dock.show_all(); }
                    else { main.hide(); dock.hide(); main.set_no_show_all(true); dock.set_no_show_all(true); }
                    transmit.set_sensitive(selected && my_id.get() != 0 && !calibrating.get());
                }
                Event::CaptureInfo(text) => { media_info.set_tooltip_text(Some(&text)); media_info.set_text(&text); },
                Event::MediaStatus(text) => { media_info.set_text(&text); },
                Event::Status(text) => {
                    status.set_text(&text);
                    status.set_tooltip_text(Some(&text));
                }
                Event::Error(text) => {
                    error.set_text(&text); error.set_tooltip_text(Some(&text));
                    transmit.set_sensitive(in_room.get() && my_id.get() != 0 && !calibrating.get());
                    controls.set_sensitive(!streaming.get() && !calibrating.get());
                },
                Event::Room(id, room) => {
                    my_id.set(id);
                    *people.borrow_mut() = room;
                    transmit.set_sensitive(in_room.get() && id != 0 && !calibrating.get());
                    populate(&list, &people.borrow(), id, &watching.borrow(), &tx, &error);
                    stage.update(&watching.borrow(), &people.borrow(), &stage_window, &tx, &error);
                }
                Event::Watching(id) => {
                    *watching.borrow_mut() = id;
                    populate(&list, &people.borrow(), my_id.get(), &watching.borrow(), &tx, &error);
                    stage.update(&watching.borrow(), &people.borrow(), &stage_window, &tx, &error);
                }
                Event::Streaming(active) => {
                    streaming.set(active);
                    if !active { media_info.set_text(""); }
                    icons::set(&transmit,if active {Icon::Stop}else{Icon::Cast},if active {"Parar transmissão"}else{"Transmitir"},true);
                    transmit.set_sensitive(in_room.get() && my_id.get() != 0 && !calibrating.get());
                    controls.set_sensitive(!active && !calibrating.get());
                    if active {
                        transmit.style_context().add_class("stop");
                    } else {
                        transmit.style_context().remove_class("stop");
                    }
                }
            }
            gtk::glib::ControlFlow::Continue
        });
        window.show_all();
    });
    app.run();
}

#[cfg(test)]
pub(super) fn check_participant_layout(window: &gtk::ApplicationWindow) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS.as_bytes()).unwrap();
    gtk::StyleContext::add_provider_for_screen(
        &gtk::gdk::Screen::default().unwrap(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let list = participant_list();
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.add(&list);
    window.add(&scroll);
    let (tx, _rx) = std::sync::mpsc::sync_channel(8);
    populate(
        &list,
        &[Person {
            id: 2,
            name: "Participante".into(),
            streaming: true,
        }],
        1,
        &[],
        &tx,
        &gtk::Label::new(None),
    );
    for (width, height) in [(960, 540), (1920, 1080)] {
        window.resize(width, height);
        window.show_all();
        while gtk::events_pending() {
            gtk::main_iteration();
        }
        let row = list.children().remove(0);
        assert!(
            (56..100).contains(&row.allocated_height()),
            "Participant stretched to {}px",
            row.allocated_height()
        );
        let card = row
            .downcast::<gtk::ListBoxRow>()
            .unwrap()
            .child()
            .unwrap()
            .downcast::<gtk::Box>()
            .unwrap();
        for widget in card.children() {
            if widget.is::<gtk::Button>() || widget.style_context().has_class("avatar") {
                assert!(
                    widget.allocated_height() < 50,
                    "Control stretched to {}px",
                    widget.allocated_height()
                );
            }
        }
    }
    window.remove(&scroll);
    window.hide();
}
