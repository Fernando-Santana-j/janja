use super::{
    Command, Commands, ENDPOINT, Event, Events, Person, RoomInfo,
    media::{Capture, Peer},
    rtc,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    sync::mpsc,
    time::Duration,
};
use tokio::sync::mpsc as async_mpsc;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};

pub fn initialize_tls() {
    // Another caller may already have installed the process-wide provider.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn start(events: Events, name: String) -> Commands {
    initialize_tls();
    let (tx, rx) = mpsc::sync_channel(128);
    let commands = tx.clone();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                let _ = events.send(Event::Error(e.to_string()));
                return;
            }
        };
        let (outgoing, receive) = async_mpsc::channel(64);
        let mut network = runtime.spawn(connect(commands.clone(), receive, name));
        let mut engine = Engine {
            epoch: 0,
            id: None,
            people: vec![],
            capture: None,
            peers: HashMap::new(),
            watching: BTreeSet::new(),
            media_tick: std::time::Instant::now(),
            last_frames: 0,
            capture_request: 0,
            starting: false,
            selected_room: None,
            ice: None,
            access: super::room_access::Access::start(events.clone()),
            tx: commands,
            events,
            outgoing,
        };
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Command::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(command) => {
                    if let Err(e) = engine.handle(command) {
                        super::diagnostics::event!("engine-error", json!({"error":e}));
                        let _ = engine.events.send(Event::Error(e));
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            engine.poll();
            if network.is_finished() {
                let message = match runtime.block_on(&mut network) {
                    Ok(()) => "A conexão foi encerrada. Reabra o aplicativo.".to_string(),
                    Err(error) => format!("Falha na conexão: {error}. Reabra o aplicativo."),
                };
                let _ = engine.events.send(Event::Status("Desconectado".into()));
                let _ = engine.events.send(Event::Error(message));
                break;
            }
        }
        engine.clear();
        network.abort();
    });
    tx
}
async fn connect(tx: Commands, mut outgoing: async_mpsc::Receiver<(u64, Value)>, name: String) {
    let mut epoch = 0;
    let mut delay = 1;
    loop {
        epoch += 1;
        let result=async {
            let config=WebSocketConfig::default().max_message_size(Some(65_536)).max_frame_size(Some(65_536));
            let (mut ws,_)=tokio::time::timeout(Duration::from_secs(15),connect_async_with_config(ENDPOINT,Some(config),false))
                .await.map_err(|_| "Tempo esgotado na conexão".to_string())?.map_err(|e|e.to_string())?;
            send_command(&tx, Command::Connected(epoch)).await?;
            ws.send(Message::Text(json!({"type":"join","name":name,"protocol":2}).to_string().into())).await.map_err(|e|e.to_string())?;
            let mut heartbeat=tokio::time::interval(Duration::from_secs(15));
            let mut seen=tokio::time::Instant::now();
            loop {
                tokio::select! {
                    message=ws.next()=>{
                        seen=tokio::time::Instant::now();
                        match message {
                            Some(Ok(Message::Text(text)))=>{
                                let value:Value=serde_json::from_str(&text).map_err(|e|e.to_string())?;
                                send_command(&tx, Command::Server(epoch,value)).await?;
                                delay=1;
                            }
                            Some(Ok(Message::Ping(bytes)))=>ws.send(Message::Pong(bytes)).await.map_err(|e|e.to_string())?,
                            Some(Ok(Message::Pong(_)))=>{},
                            Some(Ok(Message::Close(_)))|None=>return Err("Conexão encerrada".to_string()),
                            Some(Err(e))=>return Err(e.to_string()),
                            _=>return Err("Mensagem inesperada".to_string()),
                        }
                    }
                    message=outgoing.recv()=>{
                        let Some((generation,value))=message else { return Ok(()); };
                        if generation==epoch {
                            tokio::time::timeout(Duration::from_secs(5),ws.send(Message::Text(value.to_string().into())))
                                .await.map_err(|_| "Servidor não responde".to_string())?.map_err(|e|e.to_string())?;
                        }
                    }
                    _=heartbeat.tick()=>{
                        if seen.elapsed()>Duration::from_secs(60) {return Err("Servidor não responde".to_string());}
                        tokio::time::timeout(Duration::from_secs(5),ws.send(Message::Ping(vec![].into())))
                            .await.map_err(|_|"Servidor não responde".to_string())?.map_err(|e|e.to_string())?;
                    }
                }
            }
        }.await;
        match result {
            Ok(()) => return,
            Err(e) => {
                if send_command(&tx, Command::Disconnected(epoch, e))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(15);
    }
}
async fn send_command(tx: &Commands, mut command: Command) -> Result<(), String> {
    loop {
        match tx.try_send(command) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(pending)) => {
                command = pending;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err("Aplicativo encerrado".into());
            }
        }
    }
}
struct Engine {
    epoch: u64,
    id: Option<u64>,
    people: Vec<Person>,
    capture: Option<Capture>,
    peers: HashMap<(u64, u64), Peer>,
    watching: BTreeSet<u64>,
    media_tick: std::time::Instant,
    last_frames: u64,
    capture_request: u64,
    starting: bool,
    selected_room: Option<u64>,
    ice: Option<Vec<String>>,
    access: super::room_access::Access,
    tx: Commands,
    events: Events,
    outgoing: async_mpsc::Sender<(u64, Value)>,
}
impl Engine {
    fn send(&self, v: Value) -> Result<(), String> {
        self.outgoing
            .try_send((self.epoch, v))
            .map_err(|_| "Fila de conexão cheia; tente novamente".into())
    }
    fn clear(&mut self) {
        self.access.cancel();
        self.starting = false;
        self.capture_request += 1;
        self.capture = None;
        self.peers.clear();
        self.watching.clear();
        self.id = None;
        self.selected_room = None;
        self.ice = None;
        let _ = self.events.send(Event::SelectedRoom(None));
        let _ = self.events.send(Event::Viewers(vec![]));
        let _ = self.events.send(Event::Rooms(vec![]));
        self.people.clear();
        let _ = self.events.send(Event::Streaming(false));
        let _ = self
            .events
            .send(Event::Watching(self.watching.iter().copied().collect()));
        let _ = self.events.send(Event::Room(0, vec![]));
    }
    fn unwatch(&mut self, id: u64) -> Result<(), String> {
        if self.watching.remove(&id) {
            self.peers.remove(&(id, id));
            let _ = self
                .events
                .send(Event::Watching(self.watching.iter().copied().collect()));
            self.send(json!({"type":"unsubscribe","to":id}))?;
        }
        Ok(())
    }
    fn stop(&mut self) -> Result<(), String> {
        self.starting = false;
        self.capture_request += 1;
        self.capture = None;
        if let Some(id) = self.id {
            self.peers.retain(|(_, stream), _| *stream != id);
        }
        let _ = self.events.send(Event::Streaming(false));
        let _ = self.events.send(Event::Viewers(vec![]));
        self.send(json!({"type":"state","streaming":false}))
    }
    fn handle(&mut self, command: Command) -> Result<(), String> {
        match command {
            Command::CreateRoom(name, password) => {
                if self.id.is_none() {
                    return Err("Aguarde a conexão".into());
                }
                if self.selected_room.is_some() {
                    return Err("Saia da sala atual primeiro".into());
                }
                if name.trim().is_empty()
                    || name.chars().count() > 32
                    || name.chars().any(char::is_control)
                    || (!password.is_empty() && !(8..=128).contains(&password.len()))
                {
                    return Err("Informe um nome válido. A senha é opcional; se preenchida, use 8–128 bytes.".into());
                }
                self.access.idle()?;
                self.send(json!({"type":"createRoom","name":name.trim(),"password":password}))?;
                self.access
                    .submitted(self.epoch, None, Some(name.trim().into()), password);
            }
            Command::EnterRoom(room, password) => {
                if self.id.is_none() {
                    return Err("Aguarde a conexão".into());
                }
                if self.selected_room.is_some() {
                    return Err("Saia da sala atual primeiro".into());
                }
                if password.len() > 128 {
                    return Err("Senha inválida".into());
                }
                self.access.idle()?;
                self.send(json!({"type":"enterRoom","room":room,"password":password}))?;
                let name = self.access.room(room).map(|room| room.name);
                self.access
                    .submitted(self.epoch, Some(room), name, password);
            }
            Command::RequestRoom(room) => {
                if self.id.is_none() {
                    return Err("Aguarde a conexão".into());
                }
                if self.selected_room.is_some() {
                    return Err("Saia da sala atual primeiro".into());
                }
                let current = self
                    .access
                    .room(room.id)
                    .filter(|current| current.name == room.name)
                    .ok_or("Sala indisponível")?;
                if let Some(password) = self.access.begin(self.epoch, current.clone())? {
                    self.enter_room(current, password)?;
                }
            }
            Command::RoomPassword(epoch, request, room, password)
                if epoch == self.epoch && self.access.current(epoch, request, &room) =>
            {
                if !(8..=128).contains(&password.len()) {
                    return Err("Use uma senha de 8–128 bytes.".into());
                }
                self.enter_room(room, password)?;
            }
            Command::CancelRoomLookup => {
                if self.id.is_some() {
                    self.access.cancel_lookup();
                } else {
                    self.access.cancel();
                }
            }
            Command::CancelRoomRequest(epoch, request) => {
                self.access.cancel_request(epoch, request)
            }
            Command::LeaveRoom => {
                self.access.cancel();
                self.stop()?;
                self.peers.clear();
                self.watching.clear();
                self.send(json!({"type":"leaveRoom"}))?;
            }
            Command::ListRooms => {
                self.send(json!({"type":"listRooms"}))?;
            }
            Command::Connected(epoch) => {
                self.clear();
                self.epoch = epoch;
                let _ = self.events.send(Event::Status("Entrando…".into()));
            }
            Command::Disconnected(epoch, e) => {
                if epoch >= self.epoch {
                    self.epoch = epoch;
                    self.clear();
                    let _ = self
                        .events
                        .send(Event::Status(format!("Reconectando… {e}")));
                }
            }
            Command::Start(settings) => {
                if self.id.is_none() || self.selected_room.is_none() {
                    let _ = self.events.send(Event::Streaming(false));
                    return Err("Entre em uma sala antes de transmitir".into());
                }
                if self.capture.is_some() || self.starting {
                    return Ok(());
                }
                if let Err(error) = settings.validate() {
                    let _ = self.events.send(Event::Streaming(false));
                    return Err(error);
                }
                self.starting = true;
                self.capture_request += 1;
                let request = self.capture_request;
                let epoch = self.epoch;
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let result = Capture::new(&settings).map(Box::new);
                    let _ = tx.send(Command::Captured(epoch, request, settings, result));
                });
            }
            Command::Captured(epoch, request, settings, result)
                if epoch == self.epoch && request == self.capture_request && self.starting =>
            {
                self.starting = false;
                self.last_frames = 0;
                self.media_tick = std::time::Instant::now();
                match result {
                    Ok(capture) => {
                        self.last_frames =
                            capture.frames.load(std::sync::atomic::Ordering::Relaxed);
                        capture
                            .raw_frames
                            .store(0, std::sync::atomic::Ordering::Relaxed);
                        capture
                            .encoder_input
                            .store(0, std::sync::atomic::Ordering::Relaxed);
                        capture
                            .encoded_bytes
                            .store(0, std::sync::atomic::Ordering::Relaxed);
                        let _ = self.events.send(Event::CaptureInfo(format!(
                            "{} / {}p / {} FPS / {} Mbps por espectador",
                            capture.encoder,
                            settings.height,
                            settings.fps,
                            settings.bitrate as f64 / 1000.0
                        )));
                        self.capture = Some(*capture);
                    }
                    Err(e) => {
                        let _ = self.events.send(Event::Streaming(false));
                        return Err(e);
                    }
                }
                if let Err(e) = self.send(json!({"type":"state","streaming":true})) {
                    self.capture = None;
                    let _ = self.events.send(Event::Streaming(false));
                    return Err(e);
                }
                let _ = self.events.send(Event::Streaming(true));
            }
            Command::Stop => self.stop()?,
            Command::Volume(id, percent) => {
                let peer = self
                    .peers
                    .get(&(id, id))
                    .ok_or("Transmissão indisponível")?;
                peer.set_volume(percent)?;
            }
            Command::Watch(id) => {
                if self.id.is_none()
                    || self.id == Some(id)
                    || !self.people.iter().any(|p| p.id == id && p.streaming)
                {
                    return Err("Transmissão indisponível".into());
                }
                if self.watching.contains(&id) {
                    return self.unwatch(id);
                }
                let peer = Peer::new(self.epoch, id, id, false, true, self.tx.clone())?;
                if let Some(ice) = &self.ice {
                    rtc::configure_ice(&peer.rtc, ice)?;
                }
                self.peers.insert((id, id), peer);
                self.watching.insert(id);
                if let Err(e) = self.send(json!({"type":"subscribe","to":id})) {
                    self.peers.remove(&(id, id));
                    self.watching.remove(&id);
                    return Err(e);
                }
                let _ = self
                    .events
                    .send(Event::Watching(self.watching.iter().copied().collect()));
            }
            Command::Unwatch(id) => self.unwatch(id)?,
            Command::Server(epoch, value) if epoch == self.epoch => self.server(value)?,
            Command::Signal(epoch, to, stream, generation, mut data) if epoch == self.epoch => {
                if self
                    .peers
                    .get(&(to, stream))
                    .is_some_and(|p| p.generation == generation)
                {
                    let Some(session) = self
                        .peers
                        .get(&(to, stream))
                        .and_then(|p| p.session.clone())
                    else {
                        return Ok(());
                    };
                    data["session"] = json!(session);
                    self.send(json!({"type":"signal","to":to,"stream":stream,"data":data}))?;
                }
            }
            Command::RemoteReady(epoch, to, stream, generation, offer, result)
                if epoch == self.epoch =>
            {
                if let Some(peer) = self
                    .peers
                    .get_mut(&(to, stream))
                    .filter(|p| p.generation == generation)
                {
                    if let Err(error) = result.and_then(|()| peer.ready()) {
                        return self
                            .handle(Command::MediaError(epoch, to, stream, generation, error));
                    }
                    if offer {
                        let tx = self.tx.clone();
                        rtc::create_description(&peer.rtc, false, move |result| {
                            let cmd = match result {
                                Ok(v) => Command::Signal(epoch, to, stream, generation, v),
                                Err(e) => Command::MediaError(epoch, to, stream, generation, e),
                            };
                            let _ = tx.try_send(cmd);
                        })?;
                    }
                }
            }
            Command::MediaError(epoch, to, stream, generation, e)
                if epoch == self.epoch
                    && self
                        .peers
                        .get(&(to, stream))
                        .is_some_and(|p| p.generation == generation) =>
            {
                if stream == to {
                    let _ = self.unwatch(to);
                }
                if self.id == Some(stream)
                    && let Some(c) = &self.capture
                {
                    c.remove(to);
                }
                self.peers.remove(&(to, stream));
                return Err(e);
            }
            _ => {}
        }
        Ok(())
    }
    fn enter_room(&mut self, room: RoomInfo, password: String) -> Result<(), String> {
        if self.id.is_none() || self.selected_room.is_some() {
            return Err("Selecione uma sala disponível após conectar.".into());
        }
        self.access.idle()?;
        self.send(json!({"type":"enterRoom","room":room.id,"password":password}))?;
        self.access
            .submitted(self.epoch, Some(room.id), Some(room.name), password);
        Ok(())
    }
    fn server(&mut self, v: Value) -> Result<(), String> {
        match v["type"].as_str() {
            Some("rooms") => {
                let rooms: Vec<RoomInfo> =
                    serde_json::from_value(v["rooms"].clone()).map_err(|e| e.to_string())?;
                if rooms.len() > 32
                    || rooms.iter().any(|r| {
                        r.id == 0
                            || r.name.trim().is_empty()
                            || r.name.chars().count() > 32
                            || r.name.chars().any(char::is_control)
                            || r.members > 8
                    })
                {
                    return Err("Lista de salas inválida".into());
                }
                self.access.update_rooms(self.epoch, &rooms);
                let _ = self.events.send(Event::Rooms(rooms));
            }
            Some("entered") => {
                let room: Option<RoomInfo> =
                    serde_json::from_value(v["room"].clone()).map_err(|e| e.to_string())?;
                if room.as_ref().is_some_and(|room| {
                    room.id == 0
                        || room.name.trim().is_empty()
                        || room.name.chars().count() > 32
                        || room.name.chars().any(char::is_control)
                }) {
                    return Err("Confirmação de sala inválida".into());
                }
                self.access.accepted(self.epoch, room.as_ref())?;
                self.starting = false;
                self.capture_request += 1;
                self.capture = None;
                self.peers.clear();
                self.watching.clear();
                self.people.clear();
                self.selected_room = room.as_ref().map(|r| r.id);
                let _ = self.events.send(Event::Streaming(false));
                let _ = self.events.send(Event::Watching(vec![]));
                let _ = self.events.send(Event::Room(self.id.unwrap_or(0), vec![]));
                let _ = self.events.send(Event::SelectedRoom(room));
            }
            Some("welcome") => {
                let ice = v.get("iceServers").map(rtc::ice_urls).transpose()?;
                if v["protocol"].as_u64() != Some(2) {
                    let _ = self.events.send(Event::Error(
                        "Atualize o servidor Janja para usar salas com senha".into(),
                    ));
                }
                self.id = Some(
                    v["id"]
                        .as_u64()
                        .filter(|id| *id > 0)
                        .ok_or("Identificação inválida")?,
                );
                self.ice = ice;
                let _ = self.events.send(Event::Status("Conectado".into()));
            }
            Some("room") => {
                let people: Vec<Person> =
                    serde_json::from_value(v["people"].clone()).map_err(|e| e.to_string())?;
                if people.len() > 8
                    || people.iter().any(|p| {
                        p.name.chars().count() > 32 || p.name.chars().any(char::is_control)
                    })
                {
                    return Err("Lista de participantes inválida".into());
                }
                self.people = people;
                let removed: Vec<_> = self
                    .peers
                    .keys()
                    .filter(|(id, stream)| {
                        !self.people.iter().any(|p| p.id == *id)
                            || !self.people.iter().any(|p| p.id == *stream && p.streaming)
                    })
                    .copied()
                    .collect();
                for (id, stream) in removed {
                    self.peers.remove(&(id, stream));
                    if self.id == Some(stream)
                        && let Some(c) = &self.capture
                    {
                        c.remove(id);
                    }
                    if stream == id && self.watching.remove(&id) {
                        let _ = self
                            .events
                            .send(Event::Watching(self.watching.iter().copied().collect()));
                    }
                }
                let _ = self
                    .events
                    .send(Event::Room(self.id.unwrap_or(0), self.people.clone()));
            }
            Some("subscribe") => {
                let to = v["from"].as_u64().ok_or("Participante inválido")?;
                let Some(stream) = self.id else {
                    return Ok(());
                };
                let Some(capture) = &self.capture else {
                    return Ok(());
                };
                if !self.people.iter().any(|p| p.id == to)
                    || to == stream
                    || self.peers.contains_key(&(to, stream))
                {
                    return Ok(());
                }
                let peer = Peer::new(
                    self.epoch,
                    to,
                    stream,
                    true,
                    capture.has_audio,
                    self.tx.clone(),
                )?;
                if let Some(ice) = &self.ice {
                    rtc::configure_ice(&peer.rtc, ice)?;
                }
                capture.add(to, &peer);
                let rtc = peer.rtc.clone();
                let generation = peer.generation;
                self.peers.insert((to, stream), peer);
                let tx = self.tx.clone();
                let epoch = self.epoch;
                rtc::create_description(&rtc, true, move |result| {
                    let cmd = match result {
                        Ok(v) => Command::Signal(epoch, to, stream, generation, v),
                        Err(e) => Command::MediaError(epoch, to, stream, generation, e),
                    };
                    let _ = tx.try_send(cmd);
                })?;
            }
            Some("unsubscribe") => {
                if let (Some(to), Some(id)) = (v["from"].as_u64(), self.id) {
                    if let Some(c) = &self.capture {
                        c.remove(to);
                    }
                    self.peers.remove(&(to, id));
                }
            }
            Some("signal") => {
                let to = v["from"].as_u64().ok_or("Participante inválido")?;
                let stream = v["stream"].as_u64().ok_or("Transmissão inválida")?;
                let Some(peer) = self.peers.get_mut(&(to, stream)) else {
                    return Ok(());
                };
                let data = &v["data"];
                let session = data["session"].as_str().filter(|s| s.starts_with("native-v2-") && s.len() < 128)
                    .ok_or("Use esta versão nativa em todos os PCs; o cliente antigo/navegador não é compatível.")?;
                if peer.session.as_deref().is_some_and(|s| s != session) {
                    return Ok(());
                }
                if let Some(d) = data.get("description") {
                    let kind = d["type"].as_str().ok_or("SDP inválido")?;
                    let offer = kind == "offer";
                    if offer != (self.id != Some(stream)) || peer.remote_ready {
                        return Err("Negociação inesperada".into());
                    }
                    peer.session = Some(session.into());
                    let text = d["sdp"].as_str().ok_or("SDP inválido")?;
                    let tx = self.tx.clone();
                    let epoch = self.epoch;
                    let generation = peer.generation;
                    rtc::set_remote(&peer.rtc, kind, text, move |r| {
                        let _ = tx.try_send(Command::RemoteReady(
                            epoch, to, stream, generation, offer, r,
                        ));
                    })?;
                } else if let Some(c) = data.get("candidate") {
                    let index = c["sdpMLineIndex"]
                        .as_u64()
                        .filter(|v| *v <= 8)
                        .ok_or("ICE inválido")? as u32;
                    let candidate = c["candidate"].as_str().ok_or("ICE inválido")?.to_owned();
                    peer.candidate(index, candidate, Some(session.into()))?;
                }
            }
            Some("error") => {
                let message = v["message"].as_str().unwrap_or("Erro no servidor");
                if !self.access.error(self.epoch, message) {
                    return Err(message.into());
                }
            }
            _ => return Err("Resposta inesperada do servidor".into()),
        }
        Ok(())
    }
    fn poll(&mut self) {
        if let Some((room, password)) = self.access.poll(self.epoch)
            && let Err(error) = self.enter_room(room, password)
        {
            let _ = self.events.send(Event::Error(error));
        }
        if self.media_tick.elapsed() >= Duration::from_secs(2) {
            let mut status = String::new();
            let network_pressure = self
                .peers
                .iter()
                .any(|((_, stream), p)| Some(*stream) == self.id && p.congested());
            if let Some(capture) = &mut self.capture {
                let frames = capture.frames.load(std::sync::atomic::Ordering::Relaxed);
                let fps = frames.saturating_sub(self.last_frames) as f64
                    / self.media_tick.elapsed().as_secs_f64();
                let seconds = self.media_tick.elapsed().as_secs_f64();
                let raw_fps = capture
                    .raw_frames
                    .swap(0, std::sync::atomic::Ordering::Relaxed)
                    as f64
                    / seconds;
                let _input_fps = capture
                    .encoder_input
                    .swap(0, std::sync::atomic::Ordering::Relaxed)
                    as f64
                    / seconds;
                let mbps = capture
                    .encoded_bytes
                    .swap(0, std::sync::atomic::Ordering::Relaxed)
                    as f64
                    * 8.0
                    / seconds
                    / 1_000_000.0;
                super::diagnostics::event!(
                    "capture-metrics",
                    json!({"raw_fps":raw_fps,"encoder_input_fps":_input_fps,"encoded_fps":fps,"encoded_mbps":mbps,"encoder":capture.encoder}),
                );
                let viewers = self
                    .peers
                    .keys()
                    .filter(|(_, stream)| Some(*stream) == self.id)
                    .count();
                let adjustment = match capture.adapt(
                    self.media_tick.elapsed(),
                    raw_fps,
                    fps,
                    network_pressure,
                ) {
                    Ok(reason) => reason,
                    Err(error) => {
                        capture.settings.adaptive = false;
                        let _ = self.events.send(Event::Error(format!(
                            "Ajuste automático desativado: {error}"
                        )));
                        None
                    }
                };
                status = format!(
                    "{}p / até {} FPS / encoder {fps:.0} FPS / {mbps:.1} Mbps por espectador / {viewers} assistindo / {} / áudio: {}{}",
                    capture.settings.height,
                    capture.settings.fps,
                    capture.encoder,
                    capture.audio_status(),
                    adjustment
                        .map(|reason| format!(" / qualidade reduzida: {reason}"))
                        .unwrap_or_default(),
                );
                self.last_frames = frames;
            }
            let viewers = self
                .peers
                .iter()
                .filter_map(|((id, stream), peer)| {
                    (Some(*stream) == self.id && peer.connected()).then_some(*id)
                })
                .collect();
            let _ = self.events.send(Event::Viewers(viewers));
            for peer in self.peers.values() {
                peer.tune_latency(self.media_tick.elapsed());
                peer.log_stats();
            }
            let received: Vec<_> = self
                .peers
                .iter()
                .filter(|((to, stream), _)| to == stream)
                .map(|((to, _), peer)| {
                    let name = self
                        .people
                        .iter()
                        .find(|p| p.id == *to)
                        .map(|p| p.name.as_str())
                        .unwrap_or("Participante");
                    format!("{name}: {}", peer.audio_status())
                })
                .collect();
            if !received.is_empty() {
                if !status.is_empty() {
                    status.push_str(" / ");
                }
                status.push_str(&format!("Áudio recebido — {}", received.join(", ")));
            }
            if !status.is_empty() {
                let _ = self.events.send(Event::MediaStatus(status));
            }
            self.media_tick = std::time::Instant::now();
        }
        if let Some(error) = self.capture.as_ref().and_then(Capture::error) {
            let _ = self.stop();
            let _ = self.events.send(Event::Error(error));
        }
        let errors: Vec<_> = self
            .peers
            .iter()
            .filter_map(|(key, p)| p.error().map(|e| (*key, e)))
            .collect();
        for ((to, stream), error) in errors {
            if self.id == Some(stream)
                && let Some(c) = &self.capture
            {
                c.remove(to);
            }
            self.peers.remove(&(to, stream));
            if stream == to {
                let _ = self.unwatch(to);
            }
            let _ = self.events.send(Event::Error(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gst::prelude::*;
    #[tokio::test]
    async fn full_engine_queue_preserves_connection_events_until_capacity_returns() {
        let (tx, rx) = mpsc::sync_channel(1);
        assert!(tx.try_send(Command::Connected(1)).is_ok());
        let pending = send_command(&tx, Command::Disconnected(1, "reconnect".into()));
        tokio::pin!(pending);
        assert!(futures_util::poll!(&mut pending).is_pending());
        assert!(matches!(rx.try_recv(), Ok(Command::Connected(1))));
        tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            rx.try_recv(),
            Ok(Command::Disconnected(1, error)) if error == "reconnect"
        ));
    }
    #[tokio::test]
    async fn closed_engine_queue_stops_forwarding() {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        assert!(send_command(&tx, Command::Connected(1)).await.is_err());
    }
    fn engine() -> Engine {
        #[allow(deprecated)]
        let (events, _receive) = gtk::glib::MainContext::channel(gtk::glib::Priority::DEFAULT);
        let (tx, _rx) = mpsc::sync_channel(16);
        let (outgoing, _receive) = async_mpsc::channel(16);
        Engine {
            epoch: 2,
            id: None,
            people: vec![],
            capture: None,
            peers: HashMap::new(),
            watching: BTreeSet::new(),
            media_tick: std::time::Instant::now(),
            last_frames: 0,
            capture_request: 0,
            starting: false,
            selected_room: Some(0),
            ice: None,
            access: super::super::room_access::Access::without_store(events.clone()),
            tx,
            events,
            outgoing,
        }
    }
    #[test]
    fn receive_only_audio_status_reports_mute_without_erasing_idle_messages() {
        gst::init().unwrap();
        let context = gtk::glib::MainContext::new();
        #[allow(deprecated)]
        let (events, receive) = gtk::glib::MainContext::channel(gtk::glib::Priority::DEFAULT);
        let received = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let output = received.clone();
        receive.attach(Some(&context), move |event| {
            if let Event::MediaStatus(text) = event {
                output.borrow_mut().push(text);
            }
            gtk::glib::ControlFlow::Continue
        });
        let mut e = engine();
        e.events = events;
        e.people = vec![Person {
            id: 7,
            name: "Teste".into(),
            streaming: true,
        }];
        let peer = Peer::new(2, 7, 7, false, true, e.tx.clone()).unwrap();
        peer.set_volume(0).unwrap();
        e.peers.insert((7, 7), peer);
        e.media_tick = std::time::Instant::now() - Duration::from_secs(3);
        e.poll();
        while context.pending() {
            context.iteration(false);
        }
        assert_eq!(
            received.borrow().as_slice(),
            ["Áudio recebido — Teste: silenciado"]
        );
        e.peers.clear();
        e.media_tick = std::time::Instant::now() - Duration::from_secs(3);
        e.poll();
        while context.pending() {
            context.iteration(false);
        }
        assert_eq!(
            received.borrow().len(),
            1,
            "Idle polling erased a calibration message"
        );
        drop(e);
        while context.pending() {
            context.iteration(false);
        }
    }
    #[test]
    fn cancelled_capture_result_cannot_restart_streaming() {
        let mut e = engine();
        e.id = Some(1);
        e.starting = true;
        e.capture_request = 4;
        let (outgoing, _receive) = async_mpsc::channel(16);
        e.outgoing = outgoing;
        e.stop().unwrap();
        e.handle(Command::Captured(
            2,
            4,
            super::super::Settings::default(),
            Err("stale".into()),
        ))
        .unwrap();
        assert!(!e.starting);
        assert!(e.capture.is_none());
    }
    #[test]
    fn welcome_ice_configuration_reaches_new_receivers() {
        gst::init().unwrap();
        let mut e = engine();
        let (outgoing, _receive) = async_mpsc::channel(16);
        e.outgoing = outgoing;
        e.server(json!({"type":"welcome","id":1,"protocol":2,"iceServers":[{"urls":"stun:127.0.0.1:3479"}]})).unwrap();
        e.people = vec![Person {
            id: 7,
            name: "viewer".into(),
            streaming: true,
        }];
        e.handle(Command::Watch(7)).unwrap();
        assert_eq!(
            e.peers[&(7, 7)]
                .rtc
                .get_property("stun-server")
                .unwrap()
                .get::<String>()
                .unwrap()
                .as_deref(),
            Some("stun://127.0.0.1:3479")
        );
    }
    #[test]
    fn stale_peer_callbacks_cannot_break_a_new_attempt() {
        gst::init().unwrap();
        let mut e = engine();
        let peer = Peer::new(2, 7, 7, false, true, e.tx.clone()).unwrap();
        let generation = peer.generation;
        e.peers.insert((7, 7), peer);
        e.handle(Command::RemoteReady(
            2,
            7,
            7,
            generation + 1,
            false,
            Err("stale".into()),
        ))
        .unwrap();
        e.handle(Command::MediaError(2, 7, 7, generation + 1, "stale".into()))
            .unwrap();
        e.handle(Command::Signal(
            2,
            7,
            7,
            generation + 1,
            json!({"candidate":{}}),
        ))
        .unwrap();
        assert!(e.peers.contains_key(&(7, 7)));
        assert!(
            e.handle(Command::MediaError(2, 7, 7, generation, "current".into()))
                .is_err()
        );
        assert!(!e.peers.contains_key(&(7, 7)));
    }
    #[test]
    fn failed_outgoing_stream_preserves_incoming_stream_from_same_person() {
        gst::init().unwrap();
        let mut e = engine();
        e.id = Some(1);
        e.watching.insert(7);
        let incoming = Peer::new(2, 7, 7, false, true, e.tx.clone()).unwrap();
        let outgoing = Peer::new(2, 7, 1, true, true, e.tx.clone()).unwrap();
        let generation = outgoing.generation;
        e.peers.insert((7, 7), incoming);
        e.peers.insert((7, 1), outgoing);
        assert!(
            e.handle(Command::MediaError(2, 7, 1, generation, "failed".into()))
                .is_err()
        );
        assert!(e.watching.contains(&7));
        assert!(e.peers.contains_key(&(7, 7)));
        assert!(!e.peers.contains_key(&(7, 1)));
    }
    #[test]
    fn leaving_a_room_invalidates_media_callbacks_and_keeps_connection() {
        gst::init().unwrap();
        let mut e = engine();
        e.id = Some(7);
        e.selected_room = Some(3);
        e.starting = true;
        let request = e.capture_request;
        e.handle(Command::Server(
            e.epoch,
            json!({"type":"entered","room":null}),
        ))
        .unwrap();
        assert_eq!(e.id, Some(7));
        assert!(e.selected_room.is_none());
        assert!(e.capture_request > request);
        assert!(!e.starting);
        assert!(
            e.handle(Command::Start(super::super::Settings::default()))
                .is_err()
        );
        e.handle(Command::Server(
            e.epoch,
            json!({"type":"entered","room":{"id":4,"name":"Amigos"}}),
        ))
        .unwrap();
        assert_eq!(e.selected_room, Some(4));
    }
    #[test]
    fn lobby_accepts_optional_password_and_rejects_short_nonempty_passwords() {
        let mut e = engine();
        e.id = Some(1);
        e.selected_room = None;
        let (outgoing, mut receive) = async_mpsc::channel(8);
        e.outgoing = outgoing;
        assert!(
            e.handle(Command::CreateRoom("Aberta".into(), String::new()))
                .is_ok()
        );
        assert_eq!(
            receive.try_recv().unwrap().1,
            json!({"type":"createRoom","name":"Aberta","password":""})
        );
        e.access.cancel();
        assert!(
            e.handle(Command::CreateRoom("Privada".into(), "curta".into()))
                .is_err()
        );
        assert!(
            e.handle(Command::CreateRoom("Privada".into(), "a".repeat(129)))
                .is_err()
        );
        assert!(e.handle(Command::EnterRoom(2, String::new())).is_ok());
        assert_eq!(
            receive.try_recv().unwrap().1,
            json!({"type":"enterRoom","room":2,"password":""})
        );
    }
    #[test]
    fn room_password_flow_ignores_stale_dialogs_and_reuses_only_accepted_passwords() {
        let mut e = engine();
        e.id = Some(1);
        e.selected_room = None;
        let context = gtk::glib::MainContext::new();
        #[allow(deprecated)]
        let (events, receive) = gtk::glib::MainContext::channel(gtk::glib::Priority::DEFAULT);
        let received = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let output = received.clone();
        receive.attach(Some(&context), move |event| {
            output.borrow_mut().push(event);
            gtk::glib::ControlFlow::Continue
        });
        e.events = events.clone();
        e.access = super::super::room_access::Access::without_store(events);
        let (outgoing, mut wire) = async_mpsc::channel(16);
        e.outgoing = outgoing;
        e.server(
            json!({"type":"rooms","rooms":[{"id":7,"name":"Amigos","locked":true,"members":1}]}),
        )
        .unwrap();
        let room = e.access.room(7).unwrap();
        e.handle(Command::RequestRoom(room.clone())).unwrap();
        while context.pending() {
            context.iteration(false);
        }
        let request = received
            .borrow()
            .iter()
            .find_map(|event| match event {
                Event::PasswordRequired(epoch, request, _) => Some((*epoch, *request)),
                _ => None,
            })
            .unwrap();
        for (epoch, token) in [(request.0 - 1, request.1), (request.0, request.1 + 1)] {
            e.handle(Command::RoomPassword(
                epoch,
                token,
                room.clone(),
                "senha-recente".into(),
            ))
            .unwrap();
            assert!(wire.try_recv().is_err());
        }
        e.handle(Command::RoomPassword(
            request.0,
            request.1,
            room.clone(),
            "senha-recente".into(),
        ))
        .unwrap();
        assert_eq!(wire.try_recv().unwrap().1["password"], "senha-recente");
        assert!(
            e.handle(Command::CreateRoom("Outra".into(), String::new()))
                .is_err(),
            "Only one wire attempt at a time"
        );
        e.server(json!({"type":"error","message":"Senha incorreta."}))
            .unwrap();
        assert!(
            wire.try_recv().is_err(),
            "Wrong password must not retry automatically"
        );
        while context.pending() {
            context.iteration(false);
        }
        let retry = received
            .borrow()
            .iter()
            .rev()
            .find_map(|event| match event {
                Event::PasswordRequired(epoch, request, _) => Some((*epoch, *request)),
                _ => None,
            })
            .unwrap();
        e.handle(Command::RoomPassword(
            request.0,
            request.1,
            room.clone(),
            "senha-recente".into(),
        ))
        .unwrap();
        assert!(wire.try_recv().is_err());
        e.handle(Command::RoomPassword(
            retry.0,
            retry.1,
            room.clone(),
            "senha-correta".into(),
        ))
        .unwrap();
        assert_eq!(wire.try_recv().unwrap().1["password"], "senha-correta");
        e.server(json!({"type":"entered","room":{"id":7,"name":"Amigos"}}))
            .unwrap();
        e.server(json!({"type":"entered","room":null})).unwrap();
        e.handle(Command::RequestRoom(room)).unwrap();
        assert_eq!(wire.try_recv().unwrap().1["password"], "senha-correta");
        while context.pending() {
            context.iteration(false);
        }
        let debug = format!("{:?}", received.borrow());
        assert!(!debug.contains("senha-recente") && !debug.contains("senha-correta"));
    }
    #[test]
    fn multiple_subscriptions_close_independently() {
        gst::init().unwrap();
        let mut e = engine();
        let (outgoing, mut receive) = async_mpsc::channel(16);
        e.outgoing = outgoing;
        e.id = Some(1);
        e.people = vec![
            Person {
                id: 2,
                name: "A".into(),
                streaming: true,
            },
            Person {
                id: 3,
                name: "B".into(),
                streaming: true,
            },
        ];
        e.handle(Command::Watch(2)).unwrap();
        e.handle(Command::Watch(3)).unwrap();
        for peer in e.peers.values() {
            peer.pipeline
                .add(&gst::ElementFactory::make("volume", Some("playback_volume")).unwrap())
                .unwrap();
        }
        e.handle(Command::Volume(2, 25)).unwrap();
        e.handle(Command::Volume(3, 0)).unwrap();
        assert!(e.handle(Command::Volume(2, 101)).is_err());
        assert!(e.handle(Command::Volume(999, 50)).is_err());
        let gain = |id| {
            e.peers
                .get(&(id, id))
                .unwrap()
                .pipeline
                .get_by_name("playback_volume")
                .unwrap()
                .get_property("volume")
                .unwrap()
                .get::<f64>()
                .unwrap()
                .unwrap()
        };
        assert_eq!(gain(2), 0.25);
        assert_eq!(gain(3), 0.0);
        assert_eq!(e.watching.len(), 2);
        assert_eq!(e.peers.len(), 2);
        e.handle(Command::Unwatch(2)).unwrap();
        assert!(e.watching.contains(&3));
        assert!(!e.peers.contains_key(&(2, 2)));
        assert_eq!(receive.try_recv().unwrap().1["type"], "subscribe");
        assert_eq!(receive.try_recv().unwrap().1["type"], "subscribe");
        assert_eq!(receive.try_recv().unwrap().1["to"], 2);
    }
    #[test]
    fn stale_wire_session_is_rejected_before_applying_sdp() {
        gst::init().unwrap();
        let mut e = engine();
        e.id = Some(1);
        let mut peer = Peer::new(2, 7, 7, false, true, e.tx.clone()).unwrap();
        peer.session = Some("native-v2-current".into());
        e.peers.insert((7, 7), peer);
        e.server(json!({"type":"signal","from":7,"stream":7,"data":{"session":"native-v2-old","description":{"type":"offer","sdp":"invalid"}}})).unwrap();
        assert!(!e.peers.get(&(7, 7)).unwrap().remote_ready);
    }
    #[test]
    fn tls_client_starts_without_network() {
        initialize_tls();
        initialize_tls();
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        let mut client = rustls::ClientConnection::new(
            std::sync::Arc::new(config),
            "infocoretech.com.br".try_into().unwrap(),
        )
        .unwrap();
        let mut hello = Vec::new();
        assert!(client.write_tls(&mut hello).unwrap() > 0);
        assert!(!hello.is_empty());
    }

    #[test]
    fn ignores_messages_from_previous_connection() {
        let mut e = engine();
        e.handle(Command::Server(1, json!({"type":"welcome","id":77})))
            .unwrap();
        assert_eq!(e.id, None);
        e.handle(Command::Server(2, json!({"type":"welcome","id":4})))
            .unwrap();
        assert_eq!(e.id, Some(4));
    }
    #[test]
    fn rejects_oversized_room_and_control_characters() {
        let mut e = engine();
        assert!(
            e.server(
                json!({"type":"room","people":vec![json!({"id":1,"name":"A","streaming":false});9]})
            )
            .is_err()
        );
        assert!(
            e.server(
                json!({"type":"room","people":[{"id":1,"name":"bad\nname","streaming":false}]})
            )
            .is_err()
        );
        assert!(e.people.is_empty());
    }
}
