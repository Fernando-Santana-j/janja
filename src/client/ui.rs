use super::icons::{self, Icon};
use super::{Command, Commands, Event, Person, Settings};
use gtk::{Orientation, prelude::*};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

const CSS: &str = include_str!("ui.css");
fn quality_description(settings: &Settings) -> String {
    format!(
        "{}p · {} FPS · {:.1} Mb/s · áudio {}",
        settings.height,
        settings.fps,
        settings.bitrate as f64 / 1000.0,
        if settings.audio {
            "ligado"
        } else {
            "desligado"
        }
    )
}

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
fn start_transmission(
    tx: &Commands,
    settings: Settings,
    error: &gtk::Label,
    path: Result<PathBuf, String>,
) -> bool {
    if let Err(message) = settings.validate() {
        error.set_text(&message);
        return false;
    }
    let warning = path
        .and_then(|p| super::preferences::save(&p, &settings))
        .err();
    if !submit(tx, Command::Start(settings), error) {
        return false;
    }
    if let Some(message) = warning {
        error.set_text(&format!(
            "Qualidade aplicada nesta sessão, mas não foi salva: {message}"
        ));
    }
    true
}

fn session_name(
    value: &str,
    path: Result<PathBuf, String>,
) -> Result<(String, Option<String>), String> {
    let name = super::profile::name(value)?;
    let warning = path.and_then(|p| super::profile::save(&p, &name)).err();
    Ok((name, warning))
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
        if watching.contains(&person.id) {
            row.style_context().add_class("watching");
        }
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
        let activity = label(
            if person.streaming {
                "Ao vivo"
            } else {
                "Na sala"
            },
            if person.streaming {
                "live-label"
            } else {
                "muted"
            },
        );
        activity.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.pack_start(&activity, false, false, 0);
        row.pack_start(&text, true, true, 0);
        if person.streaming {
            let watched = watching.contains(&person.id);
            let button = gtk::Button::new();
            icons::set(
                &button,
                if watched { Icon::Close } else { Icon::Watch },
                if watched {
                    "Fechar transmissão"
                } else {
                    "Assistir"
                },
                !watched,
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
fn choose_name(window: &gtk::ApplicationWindow, warning: &gtk::Label) -> Option<String> {
    if let Some(name) = super::profile::load() {
        return Some(name);
    }
    let dialog = gtk::Dialog::with_buttons(
        Some("Seu nome"),
        Some(window),
        gtk::DialogFlags::MODAL,
        &[
            ("Sair", gtk::ResponseType::Cancel),
            ("Continuar", gtk::ResponseType::Accept),
        ],
    );
    let entry = gtk::Entry::new();
    entry.set_max_length(32);
    entry.set_placeholder_text(Some("Como você quer aparecer?"));
    entry.set_activates_default(true);
    let name_label = label("Como você quer aparecer?", "");
    name_label.set_mnemonic_widget(Some(&entry));
    dialog.set_default_response(gtk::ResponseType::Accept);
    let error = label("", "error");
    dialog
        .content_area()
        .pack_start(&name_label, false, false, 8);
    dialog.content_area().pack_start(&entry, false, false, 12);
    dialog.content_area().pack_start(&error, false, false, 8);
    dialog.show_all();
    let name = loop {
        if dialog.run() != gtk::ResponseType::Accept {
            break None;
        }
        match session_name(entry.text().as_str(), super::profile::path()) {
            Ok((name, persistence_error)) => {
                if let Some(message) = persistence_error {
                    warning.set_text(&format!(
                        "Nome usado nesta sessão, mas não foi salvo: {message}"
                    ));
                }
                break Some(name);
            }
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
        let header = gtk::Box::new(Orientation::Horizontal, 12);
        header.style_context().add_class("app-header");
        let brand_text = gtk::Box::new(Orientation::Vertical, 2);
        brand_text.pack_start(&label("Janja", "brand-name"), false, false, 0);
        brand_text.pack_start(&label("Sua tela. Sua sala.", "muted"), false, false, 0);
        header.pack_start(&brand_text, true, true, 0);
        let status = label("Conectando…", "badge");
        status.set_max_width_chars(24);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        header.pack_end(&status, false, false, 0);
        outer.pack_start(&header, false, false, 0);
        // Área principal: participantes à esquerda, vídeos no centro.
        let main = gtk::Box::new(Orientation::Horizontal, 0);
        main.set_hexpand(true);
        main.set_vexpand(true);
        // Sidebar esquerda: participantes.
        let sidebar = gtk::Box::new(Orientation::Vertical, 8);
        sidebar.set_width_request(252);
        sidebar.set_hexpand(false);
        sidebar.style_context().add_class("sidebar");
        let people_label = label("PESSOAS NA SALA", "section-label");
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
        let sidebar_note = gtk::Box::new(Orientation::Vertical, 6);
        sidebar_note.style_context().add_class("sidebar-note");
        let own_status = label("Sua tela está oculta", "video-name");
        let sidebar_hint = label("Escolha alguém ao vivo para assistir ou compartilhe sua tela.", "muted");
        sidebar_hint.set_line_wrap(true);
        sidebar_hint.set_max_width_chars(24);
        sidebar_note.pack_start(&own_status, false, false, 0);
        sidebar_note.pack_start(&sidebar_hint, false, false, 0);
        sidebar.pack_start(&sidebar_note, false, false, 0);
        main.pack_start(&sidebar, false, false, 0);
        // Centro: palco de vídeos.
        let content = gtk::Box::new(Orientation::Vertical, 0);
        content.style_context().add_class("stage-shell");
        content.set_hexpand(true);
        content.set_vexpand(true);
        let stage_header = gtk::Box::new(Orientation::Horizontal, 12);
        stage_header.style_context().add_class("room-header");
        let room_text = gtk::Box::new(Orientation::Vertical, 3);
        let room_title = label("", "room-title");
        room_title.set_max_width_chars(26);
        room_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        room_text.pack_start(&room_title, false, false, 0);
        room_text.pack_start(&label("Compartilhe uma tela. Acompanhe as outras.", "muted"), false, false, 0);
        stage_header.pack_start(&room_text, true, true, 0);
        let live_count = label("Nenhuma tela ao vivo", "badge");
        stage_header.pack_start(&live_count, false, false, 0);
        content.pack_start(&stage_header, false, false, 0);
        let mut stage = super::stage::Stage::new();
        let video_scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        video_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        video_scroll.style_context().add_class("stage-canvas");
        video_scroll.add(&stage.grid);
        video_scroll.set_hexpand(true);
        video_scroll.set_vexpand(true);
        content.pack_start(&video_scroll, true, true, 0);
        main.pack_start(&content, true, true, 0);
        outer.pack_start(&main, true, true, 0);
        // Dock inferior: controles.
        let dock = gtk::Box::new(Orientation::Horizontal, 12);
        dock.style_context().add_class("dock");
        let preferences = super::preferences::path().and_then(|p| super::preferences::load(&p));
        let persist_on_open = preferences.is_ok();
        let saved = preferences.unwrap_or_else(|_error| {
            super::diagnostics::event!("quality-load-error",serde_json::json!({"error":_error})); Settings::default()
        });
        let controls = gtk::Box::new(Orientation::Vertical, 10);
        controls.set_width_request(340);
        controls.pack_start(&label("Qualidade da transmissão", "room-title"), false, false, 0);
        let next_share = label("As mudanças valem para a próxima transmissão.", "muted");
        next_share.set_line_wrap(true);
        next_share.set_max_width_chars(40);
        controls.pack_start(&next_share, false, false, 0);
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
        let advanced = gtk::Box::new(Orientation::Vertical, 10);
        let dimensions = gtk::Box::new(Orientation::Horizontal, 12);
        for (title, widget) in [("Quadros por segundo", &fps), ("Resolução", &resolution)] {
            let b = gtk::Box::new(Orientation::Vertical, 4);
            b.pack_start(&label(title, "muted"), false, false, 0);
            b.pack_start(widget, false, false, 0);
            dimensions.pack_start(&b, true, true, 0);
        }
        advanced.pack_start(&dimensions, false, false, 0);
        let bitrate = gtk::SpinButton::with_range(1.0, 20.0, 0.5);
        bitrate.set_value(saved.bitrate as f64/1000.0);
        bitrate.set_digits(1);
        bitrate.set_tooltip_text(Some(
            "Upload por espectador. Reduza se a conexão não acompanhar.",
        ));
        advanced.pack_start(&label("Envio por espectador (Mbps)", "muted"), false, false, 0);
        advanced.pack_start(&bitrate, false, false, 0);
        let presets = gtk::Box::new(Orientation::Horizontal, 6);
        for (name, height, rate) in [("Leve", 720, 3000), ("Equilibrado", 1080, 8000), ("Mais detalhes", 1440, 12000)] {
            let button = gtk::Button::with_label(name);
            button.set_tooltip_text(Some(&format!("{height}p · 30 FPS · {} Mb/s por espectador", rate / 1000)));
            let (fps, resolution, bitrate) = (fps.downgrade(), resolution.downgrade(), bitrate.downgrade());
            button.connect_clicked(move |_| {
                if let (Some(fps), Some(resolution), Some(bitrate)) = (fps.upgrade(), resolution.upgrade(), bitrate.upgrade()) {
                    fps.set_active(Some(1));
                    resolution.set_active(Some(match height {720 => 0, 1440 => 2, _ => 1}));
                    bitrate.set_value(rate as f64 / 1000.0);
                }
            });
            presets.pack_start(&button, true, true, 0);
        }
        controls.pack_start(&label("PERFIS RÁPIDOS", "section-label"), false, false, 0);
        controls.pack_start(&presets, false, false, 0);
        controls.pack_start(&gtk::Separator::new(Orientation::Horizontal), false, false, 4);
        let audio = gtk::CheckButton::with_label("Compartilhar áudio do computador");
        audio.set_active(saved.audio);
        audio.set_tooltip_text(Some("Compartilha o som do computador. Para evitar retorno, zere o volume do vídeo que estiver assistindo."));
        let discord = gtk::CheckButton::with_label(if cfg!(windows) { "Remover o áudio do Discord" } else { "Remover o Discord — apenas no Windows" });
        discord.set_sensitive(cfg!(target_os = "windows"));
        discord.set_active(cfg!(windows) && saved.exclude_discord);
        discord.set_tooltip_text(Some(if cfg!(target_os = "windows") {
            "Abra o Discord antes de iniciar. Exige captura de áudio por processo em versão compatível do Windows. Se estiver fechado, o áudio normal é capturado."
        } else {
            "Ainda indisponível no Linux."
        }));
        controls.set_margin_top(16);
        controls.set_margin_bottom(16);
        controls.set_margin_start(16);
        controls.set_margin_end(16);
        controls.pack_start(&audio, false, false, 0);
        controls.pack_start(&discord, false, false, 0);
        let adaptive = gtk::CheckButton::with_label("Ajustar automaticamente");
        adaptive.set_active(saved.adaptive);
        adaptive.set_tooltip_text(Some("Reduz a qualidade se houver sobrecarga persistente ou perdas na rede. O perfil escolhido é o limite máximo. O áudio é preservado."));
        controls.pack_start(&adaptive, false, false, 0);
        let optimize=gtk::Button::with_label("Ajustar para este PC");
        optimize.set_tooltip_text(Some("Mede a codificação local e sugere um perfil. Não mede captura real, conexão ou desempenho dos espectadores."));
        advanced.pack_start(&optimize,false,false,4);
        let expander = gtk::Expander::new(Some("Ajustes avançados"));
        expander.add(&advanced);
        controls.pack_start(&expander, false, false, 0);
        let configure = gtk::Button::new();
        icons::set(&configure, Icon::Settings, "Qualidade", true);
        let popover = gtk::Popover::new(Some(&configure));
        let settings_scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        settings_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        settings_scroll.set_propagate_natural_height(true);
        settings_scroll.set_propagate_natural_width(true);
        settings_scroll.set_max_content_height(420);
        settings_scroll.add(&controls);
        popover.add(&settings_scroll);
        configure.connect_clicked(move |_| { popover.show_all(); popover.popup(); });

        let transmit = gtk::Button::new();
        icons::set(&transmit,Icon::Cast,"Compartilhar tela",true);
        let empty_transmit = transmit.downgrade();
        stage.share.connect_clicked(move |_| {
            if let Some(transmit) = empty_transmit.upgrade().filter(|button| button.is_sensitive()) {
                transmit.emit_clicked();
            }
        });
        transmit.style_context().add_class("primary");
        transmit.set_sensitive(false);
        transmit.bind_property("sensitive", &stage.share, "sensitive")
            .flags(gtk::glib::BindingFlags::SYNC_CREATE).build();
        transmit.set_tooltip_text(Some("Compartilhar tela (Ctrl+Shift+S)"));
        configure.set_valign(gtk::Align::Center);
        transmit.set_valign(gtk::Align::Center);
        dock.pack_end(&transmit, false, false, 0);
        dock.pack_end(&configure, false, false, 0);
        let audience = super::audience::Audience::new();
        dock.pack_end(&audience.button, false, false, 0);
        let share_text = gtk::Box::new(Orientation::Vertical, 5);
        share_text.set_hexpand(true);
        let share_title = label("Pronto para compartilhar", "dock-title");
        let quality_summary = label(&quality_description(&saved), "muted");
        quality_summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        quality_summary.set_max_width_chars(48);
        share_text.pack_start(&share_title, false, false, 0);
        share_text.pack_start(&quality_summary, false, false, 0);
        dock.pack_start(&share_text, true, true, 0);
        outer.pack_start(&dock, false, false, 0);
        let media_info = label("Escolha alguém ao vivo na lateral para assistir.", "muted"); media_info.set_selectable(true);
        media_info.set_ellipsize(gtk::pango::EllipsizeMode::End);
        media_info.set_hexpand(true);
        media_info.style_context().add_class("stage-footer");
        content.pack_start(&media_info, false, false, 0);
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
        let awake = Rc::new(super::playback::Awake::new(events.clone()));
        let Some(name) = choose_name(&window, &error) else { app.quit(); return; };
        let profile = label(&name, "profile-pill");
        profile.set_max_width_chars(16); profile.set_ellipsize(gtk::pango::EllipsizeMode::End); profile.set_tooltip_text(Some(&name));
        header.pack_end(&profile, false, false, 0);
        let tx = super::network::start(events.clone(), name);
        let lobby = super::rooms_ui::Lobby::new(&window, &tx, &error);
        outer.pack_start(&lobby.widget, true, true, 0);
        outer.reorder_child(&lobby.widget, 1);
        let leave = gtk::Button::new();
        icons::set(&leave, Icon::Leave, "Sair", true);
        leave.set_tooltip_text(Some("Sair da sala e voltar às salas disponíveis"));
        leave.style_context().add_class("quiet");
        leave.set_no_show_all(true);
        let leave_tx = tx.clone(); let leave_error = error.clone();
        leave.connect_clicked(move |_| { submit(&leave_tx, Command::LeaveRoom, &leave_error); });
        stage_header.pack_end(&leave, false, false, 0);
        main.set_no_show_all(true); dock.set_no_show_all(true);
        main.hide(); dock.hide();
        let in_room = Rc::new(Cell::new(false));

        let streaming = Rc::new(Cell::new(false));
        let people = Rc::new(RefCell::new(Vec::<Person>::new()));
        let my_id = Rc::new(Cell::new(0));
        let watching = Rc::new(RefCell::new(Vec::<u64>::new()));
        let viewers = RefCell::new(Vec::<u64>::new());
        let calibrating=Rc::new(Cell::new(false));
        let quality:Rc<dyn Fn()->Settings>={
            let (fps,resolution,bitrate,audio,discord,adaptive)=(fps.downgrade(),resolution.downgrade(),bitrate.downgrade(),audio.downgrade(),discord.downgrade(),adaptive.downgrade());
            Rc::new(move || {
                let (Some(fps),Some(resolution),Some(bitrate),Some(audio),Some(discord),Some(adaptive))=(fps.upgrade(),resolution.upgrade(),bitrate.upgrade(),audio.upgrade(),discord.upgrade(),adaptive.upgrade()) else{return Settings::default();};
                Settings { fps:fps.active_text().and_then(|t|t.parse().ok()).unwrap_or(30), height:match resolution.active(){Some(0)=>720,Some(2)=>1440,_=>1080}, bitrate:(bitrate.value()*1000.0) as u32, audio:audio.is_active(), exclude_discord:discord.is_active(), adaptive:adaptive.is_active(), source:0 }})
        };
        let pending=Rc::new(RefCell::new(None::<gtk::glib::SourceId>));
        let save:Rc<dyn Fn()>={
            let quality=quality.clone(); let error=error.downgrade(); let pending=pending.clone();
            let summary = quality_summary.downgrade();
            Rc::new(move || {
                if let Some(summary) = summary.upgrade() {
                    let settings = quality();
                    let text = quality_description(&settings);
                    summary.set_text(&text);
                    summary.set_tooltip_text(Some(&text));
                }
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
        for check in [&audio,&discord,&adaptive] {let save=save.clone();check.connect_toggled(move |_|save());}
        // Also persists the one-time defaults migration without resetting later choices.
        if persist_on_open { save(); }
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
            let submitted = if state.get() {
                submit(&commands, Command::Stop, &errors)
            } else {
                let Some(source_window) = source_window.upgrade() else { return; };
                let Some(selected_source) = super::source_picker::choose(&source_window) else { return; };
                let mut settings=current_quality(); settings.source=selected_source;
                start_transmission(&commands, settings, &errors, super::preferences::path())
            };
            if submitted {
                button.set_sensitive(false);
                config_controls.set_sensitive(false);
            }
        });
        let shortcut_transmit = transmit.downgrade();
        window.connect_key_press_event(move |_, event| {
            let modifiers = event.state() & (gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK | gtk::gdk::ModifierType::MOD1_MASK | gtk::gdk::ModifierType::SUPER_MASK);
            let shortcut = gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK;
            if modifiers == shortcut && matches!(event.keyval(), gtk::gdk::keys::constants::s | gtk::gdk::keys::constants::S)
                && let Some(button) = shortcut_transmit.upgrade().filter(|button| button.is_sensitive() && button.is_visible()) {
                    button.emit_clicked();
                    return gtk::glib::Propagation::Stop;
                }
            gtk::glib::Propagation::Proceed
        });
        let commands = tx.clone();
        let closing_quality=quality.clone(); let closing_pending=pending.clone();
        window.connect_delete_event(move |_, _| {
            if let Some(timer)=closing_pending.borrow_mut().take(){timer.remove();let _=super::preferences::path().and_then(|p|super::preferences::save(&p,&closing_quality()));}
            let _ = commands.try_send(Command::Quit);
            gtk::glib::Propagation::Proceed
        });
        let closing_awake = awake.clone();
        window.connect_destroy(move |_| closing_awake.shutdown());
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
                Event::PasswordRequired(epoch, request, room) => {
                    super::rooms_ui::password_required(&stage_window, epoch, request, &room, &tx, &error);
                }
                Event::SelectedRoom(room) => {
                    let selected = room.is_some(); in_room.set(selected);
                    if !selected { awake.watching(false); }
                    room_title.set_text(room.as_ref().map(|r|r.name.as_str()).unwrap_or("Salas"));
                    room_title.set_tooltip_text(room.as_ref().map(|room| room.name.as_str()));
                    leave.set_visible(selected); lobby.widget.set_visible(!selected);
                    if selected { main.set_no_show_all(false); dock.set_no_show_all(false); main.show_all(); dock.show_all(); }
                    else { main.hide(); dock.hide(); main.set_no_show_all(true); dock.set_no_show_all(true); }
                    transmit.set_sensitive(selected && my_id.get() != 0 && !calibrating.get());
                }
                Event::CaptureInfo(text) => { media_info.set_tooltip_text(Some(&text)); media_info.set_text(&text); },
                Event::MediaStatus(text) => { media_info.set_tooltip_text(Some(&text)); media_info.set_text(&text); },
                Event::Status(text) => {
                    status.set_text(&text);
                    status.set_tooltip_text(Some(&text));
                }
                Event::PlaybackError(text) => {
                    error.set_text(&text); error.set_tooltip_text(Some(&text));
                }
                Event::Error(text) => {
                    error.set_text(&text); error.set_tooltip_text(Some(&text));
                    transmit.set_sensitive(in_room.get() && my_id.get() != 0 && !calibrating.get());
                    controls.set_sensitive(!streaming.get() && !calibrating.get());
                },
                Event::Room(id, room) => {
                    my_id.set(id);
                    people_label.set_text(&format!("PESSOAS · {} / 8", room.len()));
                    let live = room.iter().filter(|person| person.streaming).count();
                    live_count.set_text(&match live { 0 => "Nenhuma tela ao vivo".into(), 1 => "1 tela ao vivo".into(), n => format!("{n} telas ao vivo") });
                    audience.update(&viewers.borrow(), &room);
                    *people.borrow_mut() = room;
                    transmit.set_sensitive(in_room.get() && id != 0 && !calibrating.get());
                    populate(&list, &people.borrow(), id, &watching.borrow(), &tx, &error);
                    stage.update(&watching.borrow(), &people.borrow(), &stage_window, &tx, &error);
                }
                Event::Viewers(ids) => {
                    audience.update(&ids, &people.borrow());
                    *viewers.borrow_mut() = ids;
                }
                Event::Watching(id) => {
                    awake.watching(!id.is_empty());
                    *watching.borrow_mut() = id;
                    if watching.borrow().is_empty() && !streaming.get() { media_info.set_text(""); media_info.set_tooltip_text(None); }
                    populate(&list, &people.borrow(), my_id.get(), &watching.borrow(), &tx, &error);
                    stage.update(&watching.borrow(), &people.borrow(), &stage_window, &tx, &error);
                }
                Event::Streaming(active) => {
                    streaming.set(active);
                    audience.set_streaming(active);
                    if !active { viewers.borrow_mut().clear(); }
                    stage.set_streaming(active);
                    own_status.set_text(if active { "Você está transmitindo" } else { "Sua tela está oculta" });
                    share_title.set_text(if active { "Sua tela está ao vivo" } else { "Pronto para compartilhar" });
                    if !active { media_info.set_text(""); }
                    icons::set(&transmit,if active {Icon::Stop}else{Icon::Cast},if active {"Parar transmissão"}else{"Compartilhar tela"},true);
                    transmit.set_tooltip_text(Some("Compartilhar ou parar a transmissão (Ctrl+Shift+S)"));
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
pub(super) fn check_persistence_fallback() {
    let error = gtk::Label::new(None);
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    assert!(start_transmission(
        &tx,
        Settings::default(),
        &error,
        Err("Pasta indisponível".into()),
    ));
    assert!(matches!(rx.try_recv(), Ok(Command::Start(_))));
    assert!(error.text().contains("não foi salva"));
    assert!(!start_transmission(
        &tx,
        Settings {
            fps: 1000,
            ..Settings::default()
        },
        &error,
        Err("Pasta indisponível".into()),
    ));
    assert!(rx.try_recv().is_err());
    assert!(tx.try_send(Command::Stop).is_ok());
    assert!(!start_transmission(
        &tx,
        Settings::default(),
        &error,
        Err("Pasta indisponível".into()),
    ));
    assert!(error.text().starts_with("Aguarde"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_keeps_validation_when_persistence_is_unavailable() {
        let (name, warning) = session_name(" Ana ", Err("Pasta indisponível".into())).unwrap();
        assert_eq!(name, "Ana");
        assert_eq!(warning.as_deref(), Some("Pasta indisponível"));
        for invalid in ["", "Ana\n"] {
            assert!(session_name(invalid, Err("Pasta indisponível".into())).is_err());
        }
    }
}
