use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    env,
    error::Error,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Semaphore, mpsc};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        Message,
        handshake::server::{create_response, write_response},
        http::{Request, Response, StatusCode, header},
        protocol::{Role, WebSocketConfig},
    },
};
mod sessions;

const MAX_PEOPLE: usize = 8;
const MAX_MESSAGE: usize = 65_536;
type Shared = Arc<App>;
type ServerError = Box<dyn Error + Send + Sync>;
struct App {
    public: bool,
    key: String,
    origin: Option<String>,
    ice: Value,
    room: Mutex<Room>,
    connections: Arc<Semaphore>,
}
#[derive(Default)]
struct Room {
    next: u64,
    peers: HashMap<u64, Peer>,
    memberships: HashMap<u64, u64>,
    channels: HashMap<u64, PrivateRoom>,
    next_channel: u64,
    master_password: Option<RoomPassword>,
    sessions: HashMap<u64, sessions::Session>,
    cache: Option<std::path::PathBuf>,
    cache_failed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct PrivateRoom {
    name: String,
    password: Option<RoomPassword>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomPassword {
    salt: [u8; 16],
    hash: [u8; 32],
}
impl RoomPassword {
    fn accepts(&self, password: &str) -> bool {
        ring::pbkdf2::verify(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            std::num::NonZeroU32::new(100_000).unwrap(),
            &self.salt,
            password.as_bytes(),
            &self.hash,
        )
        .is_ok()
    }
}
struct Peer {
    modern: bool,
    info: Person,
    tx: mpsc::Sender<Value>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Person {
    id: u64,
    name: String,
    streaming: bool,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Input {
    Join {
        #[serde(default)]
        key: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        protocol: Option<u8>,
        #[serde(default)]
        resume: Option<String>,
    },
    CreateRoom {
        name: String,
        #[serde(default)]
        password: String,
    },
    EnterRoom {
        room: u64,
        #[serde(default)]
        password: String,
    },
    LeaveRoom,
    ListRooms,
    State {
        streaming: bool,
    },
    Subscribe {
        to: u64,
        #[serde(default)]
        session: Option<String>,
    },
    Unsubscribe {
        to: u64,
        #[serde(default)]
        request: Option<u64>,
    },
    Signal {
        to: u64,
        stream: u64,
        data: Value,
    },
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 32 && !name.chars().any(char::is_control)
}
fn same_key(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |diff, (x, y)| diff | (x ^ y))
        == 0
}
pub(crate) fn valid_ice(ice: &Value) -> bool {
    let Some(servers) = ice.as_array() else {
        return false;
    };
    servers.len() <= 8
        && servers.iter().all(|server| {
            let urls = match &server["urls"] {
                Value::String(url) => vec![url.as_str()],
                Value::Array(values) if !values.is_empty() => {
                    let Some(urls) = values.iter().map(Value::as_str).collect::<Option<Vec<_>>>()
                    else {
                        return false;
                    };
                    urls
                }
                _ => return false,
            };
            urls.len() <= 8
                && urls.iter().all(|url| {
                    let Some((scheme, host)) = url.split_once(':') else {
                        return false;
                    };
                    matches!(scheme, "stun" | "stuns" | "turn" | "turns")
                        && !host.is_empty()
                        && url.len() <= 2048
                        && !url.chars().any(char::is_whitespace)
                        && (!scheme.starts_with("turn")
                            || ["username", "credential"].iter().all(|key| {
                                server[key]
                                    .as_str()
                                    .is_some_and(|s| !s.is_empty() && s.len() <= 2048)
                            }))
                })
        })
}
fn send(tx: &mpsc::Sender<Value>, value: Value) -> bool {
    tx.try_send(value).is_ok()
}
impl Room {
    #[cfg(test)]
    fn snapshot(&self) -> Value {
        let mut people: Vec<_> = self.peers.values().map(|p| p.info.clone()).collect();
        people.sort_by_key(|p| p.id);
        json!({"type":"room", "people":people})
    }
    fn listing(&self) -> Value {
        let mut rooms: Vec<_> = self.channels.iter().map(|(id, room)| {
            json!({"id":id,"name":room.name,"locked":room.password.is_some(),"members":self.memberships.values().filter(|r| *r == id).count()})
        }).collect();
        rooms.sort_by_key(|r| r["id"].as_u64());
        json!({"type":"rooms","rooms":rooms})
    }
    fn prune(&mut self) {
        self.channels
            .retain(|id, _| self.memberships.values().any(|r| r == id));
    }
    fn broadcast(&self) {
        self.broadcast_departure(None);
    }
    fn broadcast_departure(&self, departed: Option<(u64, u64)>) {
        for (id, peer) in &self.peers {
            if let Some(room) = self.memberships.get(id) {
                let mut people: Vec<_> = self
                    .peers
                    .iter()
                    .filter(|(other, _)| self.memberships.get(other) == Some(room))
                    .map(|(_, p)| p.info.clone())
                    .collect();
                people.sort_by_key(|p| p.id);
                let mut message = json!({"type":"room","people":people});
                if let Some((person, previous_room)) = departed
                    && previous_room == *room
                {
                    message["departed"] = json!(person);
                }
                send(&peer.tx, message);
            }
            if peer.modern {
                send(&peer.tx, self.listing());
            }
        }
    }
    fn room_error(&self, id: u64, message: &str) -> bool {
        if let Some(peer) = self.peers.get(&id) {
            send(&peer.tx, json!({"type":"error","message":message}));
        }
        true
    }
    fn select(&mut self, id: u64, room: Option<u64>) -> bool {
        let previous = self.memberships.get(&id).copied();
        if let Some(peer) = self.peers.get_mut(&id) {
            let selected =
                room.and_then(|r| self.channels.get(&r).map(|v| json!({"id":r,"name":v.name})));
            if !send(&peer.tx, json!({"type":"entered","room":selected})) {
                return false;
            }
            peer.info.streaming = false;
            if let Some(room) = room {
                self.memberships.insert(id, room);
            } else {
                self.memberships.remove(&id);
            }
            self.prune();
            self.broadcast_departure(
                previous
                    .filter(|previous| Some(*previous) != room)
                    .map(|previous| (id, previous)),
            );
        }
        true
    }
    fn route(&mut self, id: u64, input: Input) -> bool {
        match input {
            Input::CreateRoom { name, password } => {
                if !valid_name(&name)
                    || (!password.is_empty() && !(8..=128).contains(&password.len()))
                {
                    return self.room_error(id, "Informe um nome válido. A senha é opcional; se preenchida, use 8–128 bytes.");
                }
                if self.channels.len() >= 32 {
                    return self.room_error(id, "Limite de salas atingido.");
                }
                let password = if password.is_empty() {
                    None
                } else {
                    use ring::rand::SecureRandom;
                    let mut salt = [0; 16];
                    if ring::rand::SystemRandom::new().fill(&mut salt).is_err() {
                        return self.room_error(id, "Falha ao criar sala.");
                    }
                    let mut hash = [0; 32];
                    ring::pbkdf2::derive(
                        ring::pbkdf2::PBKDF2_HMAC_SHA256,
                        std::num::NonZeroU32::new(100_000).unwrap(),
                        &salt,
                        password.as_bytes(),
                        &mut hash,
                    );
                    Some(RoomPassword { salt, hash })
                };
                self.next_channel += 1;
                let room = self.next_channel;
                self.channels.insert(
                    room,
                    PrivateRoom {
                        name: name.trim().into(),
                        password,
                    },
                );
                self.select(id, Some(room))
            }
            Input::EnterRoom { room, password } => {
                if password.len() > 128 {
                    return self.room_error(id, "Senha inválida.");
                }
                let Some(target) = self.channels.get(&room) else {
                    return self.room_error(id, "Sala indisponível.");
                };
                if let Some(secret) = &target.password
                    && !secret.accepts(&password)
                    && !self
                        .master_password
                        .as_ref()
                        .is_some_and(|master| master.accepts(&password))
                {
                    return self.room_error(id, "Senha incorreta.");
                }
                if self.memberships.get(&id) != Some(&room)
                    && self.memberships.values().filter(|r| **r == room).count() >= MAX_PEOPLE
                {
                    return self.room_error(id, "Sala cheia (8 pessoas).");
                }
                self.select(id, Some(room))
            }
            Input::LeaveRoom => self.select(id, None),
            Input::ListRooms => {
                if let Some(peer) = self.peers.get(&id) {
                    send(&peer.tx, self.listing());
                }
                true
            }
            Input::State { streaming } => {
                if !self.memberships.contains_key(&id) {
                    return true;
                }
                if let Some(peer) = self.peers.get_mut(&id) {
                    peer.info.streaming = streaming;
                }
                self.broadcast();
                true
            }
            Input::Subscribe { to, .. } | Input::Unsubscribe { to, .. } => {
                let subscribe = matches!(input, Input::Subscribe { .. });
                if to == id || !self.memberships.contains_key(&id) {
                    return false;
                }
                // The target can leave while an already valid message is in flight.
                if !self.memberships.contains_key(&to) {
                    return true;
                }
                if self.memberships.get(&id) != self.memberships.get(&to) {
                    return false;
                }
                let (session, request) = match input {
                    Input::Subscribe { session, .. } => (session, None),
                    Input::Unsubscribe { request, .. } => (None, request),
                    _ => unreachable!(),
                };
                if request == Some(0) {
                    return false;
                }
                if session
                    .as_ref()
                    .is_some_and(|s| !s.starts_with("native-v2-") || s.len() > 127)
                {
                    return false;
                }
                if let Some(peer) = self.peers.get(&to)
                    && (!subscribe || peer.info.streaming)
                {
                    let mut message =
                        json!({"type":if subscribe {"subscribe"} else {"unsubscribe"}, "from":id});
                    if let Some(session) = session {
                        message["session"] = json!(session);
                    }
                    if let Some(request) = request {
                        message["request"] = json!(request);
                    }
                    send(&peer.tx, message);
                }
                true // A publisher can leave while a subscription is in flight.
            }
            Input::Signal { to, stream, data } => {
                if to == id
                    || (stream != id && stream != to)
                    || !data.is_object()
                    || !self.memberships.contains_key(&id)
                {
                    return false;
                }
                if !self.memberships.contains_key(&to) {
                    return true;
                }
                if self.memberships.get(&id) != self.memberships.get(&to) {
                    return false;
                }
                if !self.peers.get(&stream).is_some_and(|p| p.info.streaming) {
                    return true;
                }
                if let Some(p) = self.peers.get(&to) {
                    send(
                        &p.tx,
                        json!({"type":"signal", "from":id, "stream":stream, "data":data}),
                    );
                }
                true
            }
            Input::Join { .. } => false,
        }
    }
}

async fn socket<S: AsyncRead + AsyncWrite + Unpin>(mut ws: WebSocketStream<S>, app: Shared) {
    let first = tokio::time::timeout(Duration::from_secs(10), ws.next()).await;
    let Ok(Some(Ok(Message::Text(raw)))) = first else {
        return;
    };
    let Ok(Input::Join {
        key,
        name,
        protocol,
        resume,
    }) = serde_json::from_str(&raw)
    else {
        return;
    };
    if (!app.public && !same_key(&key, &app.key))
        || !(valid_name(&name) || (app.public && name.is_empty()))
    {
        let _ = ws
            .send(Message::Text(
                json!({"type":"error","message":"Nome ou chave inválidos."})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    }
    let (tx, mut rx) = mpsc::channel(64);
    let connection_tx = tx.clone();
    let id = {
        let mut room = app.room.lock().unwrap();
        room.expire_sessions(Instant::now());
        if let Some((id, token)) = room.resume(resume.as_deref(), &tx) {
            send(
                &tx,
                json!({"type":"welcome", "id":id,"iceServers":app.ice,"protocol":2,"resume":token,"resumed":true}),
            );
            if let Some(channel) = room.memberships.get(&id).and_then(|id| {
                room.channels
                    .get(id)
                    .map(|r| json!({"id":id,"name":r.name}))
            }) {
                send(&tx, json!({"type":"entered","room":channel,"resumed":true}));
            }
            room.persist_sessions();
            room.broadcast();
            Some(id)
        } else if room.peers.len() >= 32
            || (protocol != Some(2)
                && room.memberships.values().filter(|r| **r == 0).count() >= MAX_PEOPLE)
        {
            None
        } else {
            room.next += 1;
            let id = room.next;
            let token = if protocol == Some(2) {
                room.new_session(id)
            } else {
                None
            };
            send(
                &tx,
                json!({"type":"welcome", "id":id, "iceServers":app.ice,"protocol":2,"resume":token,"resumed":false}),
            );
            room.peers.insert(
                id,
                Peer {
                    modern: protocol == Some(2),
                    info: Person {
                        id,
                        name: if name.is_empty() {
                            format!("Amigo {id}")
                        } else {
                            name.trim().into()
                        },
                        streaming: false,
                    },
                    tx,
                },
            );
            if protocol != Some(2) {
                room.memberships.insert(id, 0);
            }
            room.persist_sessions();
            room.broadcast();
            Some(id)
        }
    };
    let Some(id) = id else {
        let _ = ws
            .send(Message::Text(
                json!({"type":"error","message":"Sala cheia (8 pessoas)."})
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    };
    let connection_owner = connection_tx.downgrade();
    drop(connection_tx);
    let (mut writer, mut reader) = ws.split();
    let period = Duration::from_secs(20);
    let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    let mut last_seen = Instant::now();
    let mut window = Instant::now();
    let mut count = 0u32;
    let mut room_attempt = Instant::now() - Duration::from_secs(1);
    loop {
        let outgoing = tokio::select! {
            _ = heartbeat.tick() => {
                if last_seen.elapsed() > Duration::from_secs(60) { break; }
                Some(Message::Ping(Vec::new().into()))
            }
            value = rx.recv() => value.map(|v| Message::Text(v.to_string().into())),
            incoming = reader.next() => {
                match incoming {
                    Some(Ok(Message::Text(raw))) => {
                        last_seen = Instant::now();
                        if window.elapsed() >= Duration::from_secs(1) { window = Instant::now(); count = 0; }
                        count += 1;
                        if count > 120 { break; }
                        let Ok(input) = serde_json::from_str::<Input>(&raw) else { break; };
                        let mut room = app.room.lock().unwrap();
                        let Some(owner) = connection_owner.upgrade() else { break; };
                        if !room.peers.get(&id).is_some_and(|p| p.tx.same_channel(&owner)) { break; }
                        if matches!(input, Input::CreateRoom { .. } | Input::EnterRoom { .. }) {
                            if room_attempt.elapsed() < Duration::from_millis(500) {
                                room.room_error(id, "Aguarde um instante antes de tentar novamente.");
                                continue;
                            }
                            room_attempt = Instant::now();
                        }
                        let save = matches!(input, Input::CreateRoom { .. } | Input::EnterRoom { .. } | Input::LeaveRoom | Input::State { .. });
                        if !room.route(id, input) { break; }
                        if save { room.persist_sessions(); }
                        continue;
                    }
                    Some(Ok(Message::Pong(_))) => { last_seen = Instant::now(); continue; }
                    Some(Ok(Message::Ping(data))) => { last_seen = Instant::now(); Some(Message::Pong(data)) },
                    _ => break,
                }
            }
        };
        let Some(message) = outgoing else {
            break;
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(5), writer.send(message)).await,
            Ok(Ok(()))
        ) {
            break;
        }
    }
    let mut room = app.room.lock().unwrap();
    if let Some(owner) = connection_owner.upgrade() {
        room.disconnect(id, &owner, Instant::now());
    }
}

fn response<'a>(status: StatusCode, mime: &'static str, body: &'a str) -> Response<&'a str> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    let headers = response.headers_mut();
    for (key, value) in [
        ("content-type", mime),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; media-src 'self' blob:; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cache-control", "no-store"),
        ("connection", "close"),
    ] {
        headers.insert(header::HeaderName::from_static(key), value.parse().unwrap());
    }
    headers.insert(
        header::CONTENT_LENGTH,
        body.len().to_string().parse().unwrap(),
    );
    response
}

fn parse_request(buffer: &[u8]) -> Result<Option<(Request<()>, usize)>, ServerError> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut parsed = httparse::Request::new(&mut headers);
    let httparse::Status::Complete(size) = parsed.parse(buffer)? else {
        return Ok(None);
    };
    if parsed.version != Some(1) {
        return Err("HTTP/1.1 required".into());
    }
    let mut request = Request::builder()
        .method(parsed.method.ok_or("Missing method")?)
        .uri(parsed.path.ok_or("Missing path")?);
    for h in parsed.headers.iter() {
        request = request.header(h.name, h.value);
    }
    let request = request.body(())?;
    if request.headers().contains_key(header::TRANSFER_ENCODING)
        || request
            .headers()
            .get_all(header::CONTENT_LENGTH)
            .iter()
            .any(|v| v != "0")
    {
        return Err("Request bodies are not supported".into());
    }
    Ok(Some((request, size)))
}

async fn write_http(stream: &mut TcpStream, reply: Response<&str>) -> Result<(), ServerError> {
    let mut bytes = Vec::new();
    write_response(&mut bytes, &reply)?;
    bytes.extend_from_slice(reply.body().as_bytes());
    tokio::time::timeout(Duration::from_secs(5), stream.write_all(&bytes)).await??;
    Ok(())
}

async fn connection(mut stream: TcpStream, app: Shared) -> Result<(), ServerError> {
    let (req, tail) = tokio::time::timeout(Duration::from_secs(10), async {
        let mut buffer = vec![];
        loop {
            let mut chunk = [0u8; 2048];
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                return Err::<_, ServerError>("Connection closed".into());
            }
            buffer.extend_from_slice(&chunk[..n]);
            if buffer.len() > 8192 {
                return Err("Request headers too large".into());
            }
            if let Some((request, size)) = parse_request(&buffer)? {
                return Ok((request, buffer.split_off(size)));
            }
        }
    })
    .await??;
    let text = |status, body| response(status, "text/plain; charset=utf-8", body);
    if req.method() != "GET" {
        return write_http(
            &mut stream,
            text(StatusCode::METHOD_NOT_ALLOWED, "GET required"),
        )
        .await;
    }
    let ws_path = if app.public { "/janja/ws" } else { "/ws" };
    if app.public && req.uri().path() == "/janja/health" {
        let body = {
            let room = app.room.lock().unwrap();
            json!({"status":"ok","service":"janja-signaling","protocol":2,"passwordOptional":true,"masterPasswordConfigured":room.master_password.is_some(),"sessionRecovery":true,"sessionCacheHealthy":!room.cache_failed,"build":crate::BUILD}).to_string()
        };
        return write_http(
            &mut stream,
            response(StatusCode::OK, "application/json", &body),
        )
        .await;
    }
    if req.uri().path() != ws_path {
        let Some(asset) = http_info(app.public, req.uri().path()) else {
            return write_http(&mut stream, text(StatusCode::NOT_FOUND, "Not found")).await;
        };
        return write_http(&mut stream, response(StatusCode::OK, asset.0, asset.1)).await;
    }
    let headers = req.headers();
    let get = |key| headers.get(key).and_then(|v| v.to_str().ok()).unwrap_or("");
    let origin = get(header::ORIGIN);
    let host = get(header::HOST);
    let allowed = app.origin.as_deref().map_or_else(
        || origin == format!("http://{host}"),
        |expected| origin == expected,
    );
    let local_browser = app.public && host == "127.0.0.1:8787" && origin == "http://127.0.0.1:8787";
    if !(allowed || local_browser || (app.public && !headers.contains_key(header::ORIGIN))) {
        return write_http(&mut stream, text(StatusCode::FORBIDDEN, "Invalid origin")).await;
    }
    let Ok(permit) = app.connections.clone().try_acquire_owned() else {
        return write_http(&mut stream, text(StatusCode::SERVICE_UNAVAILABLE, "Busy")).await;
    };
    let mut reply = create_response(&req)?.map(|_| "");
    reply
        .headers_mut()
        .insert(header::CONTENT_LENGTH, "0".parse().unwrap());
    write_http(&mut stream, reply).await?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let ws = WebSocketStream::from_partially_read(stream, tail, Role::Server, Some(config)).await;
    let _permit = permit;
    socket(ws, app).await;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServerConfig {
    key: String,
    origin: Option<String>,
    #[serde(default = "default_ice")]
    ice_servers: Value,
    #[serde(default = "default_port")]
    port: u16,
}
fn default_port() -> u16 {
    8787
}
fn default_ice() -> Value {
    json!([{"urls":"stun:stun.cloudflare.com:3478"}])
}

pub async fn run(config: Option<&str>) -> Result<(), Box<dyn Error>> {
    let config: ServerConfig = if let Some(path) = config {
        serde_json::from_str(std::fs::read_to_string(path)?.trim_start_matches('\u{feff}'))?
    } else {
        ServerConfig {
            key: env::var("SALA_KEY")
                .map_err(|_| "Defina SALA_KEY com uma chave de pelo menos 16 caracteres.")?,
            origin: env::var("SALA_ORIGIN").ok().filter(|s| !s.is_empty()),
            ice_servers: serde_json::from_str(
                &env::var("SALA_ICE_SERVERS").unwrap_or_else(|_| "[]".into()),
            )?,
            port: env::var("SALA_PORT")
                .unwrap_or_else(|_| "8787".into())
                .parse()?,
        }
    };
    let ServerConfig {
        key,
        origin,
        ice_servers: ice,
        port,
    } = config;
    if key.len() < 16 || key.len() > 512 {
        return Err("SALA_KEY deve ter entre 16 e 512 bytes.".into());
    }
    if !valid_ice(&ice) {
        return Err(
            "SALA_ICE_SERVERS: use até 8 servidores STUN/TURN; TURN exige username e credential."
                .into(),
        );
    }
    if ice.as_array().unwrap().is_empty() {
        eprintln!(
            "Sem STUN/TURN configurado: conexões entre redes diferentes podem falhar. Leia README.md."
        );
    }
    let app = Arc::new(App {
        public: false,
        key,
        origin,
        ice,
        room: Mutex::new(Room::load_sessions(sessions::cache_path())?),
        connections: Arc::new(Semaphore::new(32)),
    });
    serve(app, port).await
}

fn http_info(public: bool, path: &str) -> Option<(&'static str, &'static str)> {
    let root = if public {
        matches!(path, "/janja" | "/janja/")
    } else {
        path == "/"
    };
    root.then_some((
        "text/plain; charset=utf-8",
        "Janja: servidor de sinalização. Use o aplicativo nativo.",
    ))
}

pub async fn run_janja() -> Result<(), Box<dyn Error>> {
    let path = env::var_os("JANJA_MASTER_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "master-password.json".into());
    let master_password = match std::fs::read(path) {
        Ok(data) => Some(serde_json::from_slice::<RoomPassword>(&data)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    serve(
        Arc::new(App {
            public: true,
            key: String::new(),
            origin: Some("https://infocoretech.com.br".into()),
            ice: default_ice(),
            room: Mutex::new({
                let mut room = Room::load_sessions(sessions::cache_path())?;
                room.master_password = master_password;
                room
            }),
            connections: Arc::new(Semaphore::new(32)),
        }),
        8787,
    )
    .await
}

async fn serve(app: Shared, port: u16) -> Result<(), Box<dyn Error>> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!("Sala em http://{}", listener.local_addr()?);
    let http_slots = Arc::new(Semaphore::new(64));
    let mut cleanup = tokio::time::interval(Duration::from_secs(20));
    loop {
        let (stream, _) = tokio::select! {
            result = listener.accept() => result?,
            _ = cleanup.tick() => { app.room.lock().unwrap().expire_sessions(Instant::now()); continue; },
            _ = tokio::signal::ctrl_c() => return Ok(()),
        };
        let Ok(permit) = http_slots.clone().try_acquire_owned() else {
            continue;
        };
        stream.set_nodelay(true)?;
        let app = app.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = connection(stream, app).await;
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_departed_or_slow_recipient_does_not_disconnect_the_sender() {
        let mut room = Room::default();
        room.memberships.insert(1, 3);
        assert!(
            room.route(
                1,
                Input::Subscribe {
                    to: 2,
                    session: None
                }
            ),
            "A departure race disconnected an innocent viewer"
        );
        let (tx, _rx) = mpsc::channel(1);
        assert!(send(&tx, json!({"type":"pending"})));
        room.peers.insert(
            2,
            Peer {
                modern: true,
                info: Person {
                    id: 2,
                    name: "Amigo".into(),
                    streaming: true,
                },
                tx,
            },
        );
        room.memberships.insert(2, 3);
        assert!(
            room.route(
                1,
                Input::Subscribe {
                    to: 2,
                    session: None
                }
            ),
            "A full recipient queue disconnected the sender"
        );
        assert!(room.route(
            1,
            Input::Signal {
                to: 2,
                stream: 2,
                data: json!({"candidate":null})
            }
        ));
    }
    #[test]
    fn room_selection_requires_delivery_before_changing_membership() {
        let mut room = Room::default();
        let (tx, mut rx) = mpsc::channel(1);
        assert!(send(&tx, json!({"type":"pending"})));
        room.peers.insert(
            1,
            Peer {
                modern: true,
                info: Person {
                    id: 1,
                    name: "Amigo".into(),
                    streaming: true,
                },
                tx,
            },
        );
        room.memberships.insert(1, 0);
        room.channels.insert(
            2,
            PrivateRoom {
                name: "Destino".into(),
                password: None,
            },
        );

        assert!(!room.select(1, Some(2)));
        assert_eq!(room.memberships.get(&1), Some(&0));
        assert!(room.peers[&1].info.streaming);
        assert_eq!(rx.try_recv().unwrap()["type"], "pending");

        assert!(room.select(1, Some(2)));
        assert_eq!(room.memberships.get(&1), Some(&2));
        assert!(!room.peers[&1].info.streaming);
        assert_eq!(
            rx.try_recv().unwrap(),
            json!({"type":"entered","room":{"id":2,"name":"Destino"}})
        );
    }
    #[test]
    fn master_password_opens_private_rooms_without_bypassing_capacity_or_isolation() {
        let mut rooms = Room::default();
        let password = "test-owner-password";
        let salt = [7; 16];
        let mut hash = [0; 32];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA256,
            std::num::NonZeroU32::new(100_000).unwrap(),
            &salt,
            password.as_bytes(),
            &mut hash,
        );
        rooms.master_password = Some(RoomPassword { salt, hash });
        let mut receivers = Vec::new();
        for id in 1..=9 {
            let (tx, rx) = mpsc::channel(64);
            receivers.push(rx);
            rooms.peers.insert(
                id,
                Peer {
                    modern: true,
                    info: Person {
                        id,
                        name: format!("U{id}"),
                        streaming: false,
                    },
                    tx,
                },
            );
        }
        rooms.route(
            1,
            Input::CreateRoom {
                name: "Privada".into(),
                password: "private-room-password".into(),
            },
        );
        rooms.route(
            2,
            Input::EnterRoom {
                room: 1,
                password: "wrong-password".into(),
            },
        );
        assert!(!rooms.memberships.contains_key(&2));
        assert!(!rooms.route(
            2,
            Input::Subscribe {
                to: 1,
                session: None
            }
        ));
        rooms.route(
            2,
            Input::EnterRoom {
                room: 1,
                password: password.into(),
            },
        );
        assert_eq!(rooms.memberships.get(&2), Some(&1));
        for id in 3..=8 {
            rooms.memberships.insert(id, 1);
        }
        rooms.route(
            9,
            Input::EnterRoom {
                room: 1,
                password: password.into(),
            },
        );
        assert!(!rooms.memberships.contains_key(&9));
        let list = rooms.listing().to_string();
        assert!(!list.contains(password));
        assert!(!list.contains("hash"));
        rooms.master_password = None;
        rooms.route(3, Input::LeaveRoom);
        rooms.route(
            9,
            Input::EnterRoom {
                room: 1,
                password: password.into(),
            },
        );
        assert!(!rooms.memberships.contains_key(&9));
    }
    #[test]
    fn open_rooms_accept_empty_password_without_weakening_private_rooms() {
        let mut rooms = Room::default();
        let mut receivers = Vec::new();
        for id in 1..=2 {
            let (tx, rx) = mpsc::channel(64);
            receivers.push(rx);
            rooms.peers.insert(
                id,
                Peer {
                    modern: true,
                    info: Person {
                        id,
                        name: format!("User{id}"),
                        streaming: false,
                    },
                    tx,
                },
            );
        }
        rooms.route(
            1,
            serde_json::from_value(json!({"type":"createRoom","name":"Aberta"})).unwrap(),
        );
        assert_eq!(rooms.memberships.get(&1), Some(&1));
        assert_eq!(rooms.listing()["rooms"][0]["locked"], false);
        rooms.route(
            2,
            serde_json::from_value(json!({"type":"enterRoom","room":1})).unwrap(),
        );
        assert_eq!(rooms.memberships.get(&2), Some(&1));
        assert_eq!(rooms.listing()["rooms"][0]["members"], 2);
        rooms.route(
            2,
            Input::CreateRoom {
                name: "Privada".into(),
                password: "senha1234".into(),
            },
        );
        assert_eq!(rooms.listing()["rooms"][1]["locked"], true);
        rooms.route(
            1,
            Input::EnterRoom {
                room: 2,
                password: String::new(),
            },
        );
        assert_eq!(rooms.memberships.get(&1), Some(&1));
    }
    #[test]
    fn private_rooms_require_password_and_isolate_signals() {
        let mut rooms = Room::default();
        let mut receivers = Vec::new();
        for id in 1..=3 {
            let (tx, rx) = mpsc::channel(64);
            receivers.push(rx);
            rooms.peers.insert(
                id,
                Peer {
                    modern: true,
                    info: Person {
                        id,
                        name: format!("User{id}"),
                        streaming: false,
                    },
                    tx,
                },
            );
        }
        assert!(rooms.route(
            1,
            Input::CreateRoom {
                name: "Amigos".into(),
                password: "senha1234".into()
            }
        ));
        assert_eq!(rooms.memberships.get(&1), Some(&1));
        assert!(rooms.route(
            2,
            Input::EnterRoom {
                room: 1,
                password: "incorreta".into()
            }
        ));
        assert!(!rooms.memberships.contains_key(&2));
        assert!(rooms.route(
            2,
            Input::EnterRoom {
                room: 1,
                password: "senha1234".into()
            }
        ));
        assert_eq!(rooms.memberships.get(&2), Some(&1));
        assert!(rooms.route(
            3,
            Input::CreateRoom {
                name: "Outra".into(),
                password: "senha5678".into()
            }
        ));
        assert!(rooms.route(1, Input::State { streaming: true }));
        assert!(!rooms.route(
            3,
            Input::Subscribe {
                to: 1,
                session: None
            }
        ));
        assert!(!rooms.route(
            3,
            Input::Signal {
                to: 1,
                stream: 1,
                data: json!({"candidate":"invalid"})
            }
        ));
        let list = rooms.listing().to_string();
        assert!(!list.contains("senha1234"));
        assert!(!list.contains("salt"));
        assert_eq!(rooms.listing()["rooms"][0]["members"], 2);
        rooms.route(1, Input::LeaveRoom);
        assert!(!rooms.peers[&1].info.streaming);
        assert!(rooms.channels.contains_key(&1));
        rooms.route(2, Input::LeaveRoom);
        assert!(!rooms.channels.contains_key(&1));
        assert!(rooms.channels.contains_key(&2));
    }
    #[test]
    fn private_room_rejects_invalid_fields_and_over_capacity() {
        let mut rooms = Room::default();
        let (tx, _rx) = mpsc::channel(64);
        rooms.peers.insert(
            1,
            Peer {
                modern: true,
                info: Person {
                    id: 1,
                    name: "A".into(),
                    streaming: false,
                },
                tx,
            },
        );
        rooms.route(
            1,
            Input::CreateRoom {
                name: " ".into(),
                password: "senha1234".into(),
            },
        );
        rooms.route(
            1,
            Input::CreateRoom {
                name: "Sala".into(),
                password: "x".into(),
            },
        );
        assert!(rooms.channels.is_empty());
        rooms.route(
            1,
            Input::CreateRoom {
                name: "Sala".into(),
                password: "senha1234".into(),
            },
        );
        for id in 2..=9 {
            let (tx, _rx) = mpsc::channel(64);
            rooms.peers.insert(
                id,
                Peer {
                    modern: true,
                    info: Person {
                        id,
                        name: format!("User{id}"),
                        streaming: false,
                    },
                    tx,
                },
            );
            if id <= 8 {
                rooms.memberships.insert(id, 1);
            }
        }
        rooms.route(
            9,
            Input::EnterRoom {
                room: 1,
                password: "senha1234".into(),
            },
        );
        assert!(!rooms.memberships.contains_key(&9));
    }
    #[test]
    fn signaling_server_does_not_serve_browser_client() {
        assert!(super::http_info(true, "/janja/").is_some());
        for path in [
            "/",
            "/janja/app.js",
            "/janja/media.js",
            "/janja/janja.css",
            "/janjabad/",
        ] {
            assert!(super::http_info(true, path).is_none());
        }
    }

    use super::*;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    #[test]
    fn server_file_has_free_defaults_and_rejects_unknown_options() {
        let config: ServerConfig = serde_json::from_str(
            r#"{"key":"1234567890123456","origin":"https://sala.example.org"}"#,
        )
        .unwrap();
        assert_eq!(config.port, 8787);
        assert!(valid_ice(&config.ice_servers));
        assert_eq!(config.ice_servers, default_ice());
        assert!(
            serde_json::from_str::<ServerConfig>(r#"{"key":"1234567890123456","typo":true}"#)
                .is_err()
        );
    }

    #[tokio::test]
    async fn lobby_create_password_join_and_leave_over_websocket() {
        type Client = WebSocketStream<tokio::io::DuplexStream>;
        async fn client(app: Shared) -> Client {
            let (server_io, client_io) = tokio::io::duplex(65_536);
            tokio::spawn(async move {
                socket(
                    WebSocketStream::from_raw_socket(server_io, Role::Server, None).await,
                    app,
                )
                .await;
            });
            let mut ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
            ws.send(Message::Text(
                json!({"type":"join","name":"Amigo","protocol":2})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            ws
        }
        async fn receive(ws: &mut Client, kind: &str) -> Value {
            for _ in 0..16 {
                let frame = tokio::time::timeout(Duration::from_secs(2), ws.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                if !frame.is_text() {
                    continue;
                }
                let value: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                if value["type"] == kind {
                    return value;
                }
            }
            panic!("Missing {kind}");
        }
        let app = Arc::new(App {
            public: true,
            key: String::new(),
            origin: None,
            ice: json!([]),
            room: Mutex::new(Room::default()),
            connections: Arc::new(Semaphore::new(32)),
        });
        let mut owner = client(app.clone()).await;
        assert_eq!(receive(&mut owner, "welcome").await["protocol"], 2);
        assert_eq!(receive(&mut owner, "rooms").await["rooms"], json!([]));
        let mut friend = client(app.clone()).await;
        receive(&mut friend, "welcome").await;
        receive(&mut friend, "rooms").await;
        owner
            .send(Message::Text(
                json!({"type":"createRoom","name":"Amigos","password":"senha1234"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let room = receive(&mut owner, "entered").await["room"]["id"].clone();
        assert_eq!(
            receive(&mut owner, "room").await["people"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            receive(&mut friend, "rooms").await["rooms"][0]["locked"],
            true
        );
        friend
            .send(Message::Text(
                json!({"type":"enterRoom","room":room,"password":"wrong"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        assert_eq!(
            receive(&mut friend, "error").await["message"],
            "Senha incorreta."
        );
        tokio::time::sleep(Duration::from_millis(550)).await;
        friend
            .send(Message::Text(
                json!({"type":"enterRoom","room":room,"password":"senha1234"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        assert_eq!(receive(&mut friend, "entered").await["room"]["id"], room);
        assert_eq!(
            receive(&mut friend, "room").await["people"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            receive(&mut owner, "room").await["people"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        friend
            .send(Message::Text(
                json!({"type":"leaveRoom"}).to_string().into(),
            ))
            .await
            .unwrap();
        assert!(receive(&mut friend, "entered").await["room"].is_null());
        friend.close(None).await.unwrap();
        assert_eq!(
            receive(&mut owner, "room").await["people"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        owner
            .send(Message::Text(
                json!({"type":"leaveRoom"}).to_string().into(),
            ))
            .await
            .unwrap();
        assert!(receive(&mut owner, "entered").await["room"].is_null());
        assert!(app.room.lock().unwrap().channels.is_empty());
        owner.close(None).await.unwrap();
    }
    #[tokio::test]
    async fn websocket_frames_authenticate_and_relay_without_network() {
        type Client = WebSocketStream<tokio::io::DuplexStream>;
        async fn client(app: Shared, key: &str) -> Client {
            let (server_io, client_io) = tokio::io::duplex(65_536);
            tokio::spawn(async move {
                socket(
                    WebSocketStream::from_raw_socket(server_io, Role::Server, None).await,
                    app,
                )
                .await;
            });
            let mut ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
            ws.send(Message::Text(
                json!({"type":"join","key":key,"name":"Friend"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            ws
        }
        async fn receive(ws: &mut Client) -> Value {
            let frame = tokio::time::timeout(Duration::from_secs(1), ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            serde_json::from_str(frame.to_text().unwrap()).unwrap()
        }
        let app = Arc::new(App {
            public: false,
            key: "secret-room-key-123".into(),
            origin: None,
            ice: json!([]),
            room: Mutex::new(Room::default()),
            connections: Arc::new(Semaphore::new(32)),
        });
        let mut bad = client(app.clone(), "wrong").await;
        assert_eq!(receive(&mut bad).await["type"], "error");
        assert!(app.room.lock().unwrap().peers.is_empty());
        let mut a = client(app.clone(), "secret-room-key-123").await;
        assert_eq!(receive(&mut a).await["id"], 1);
        assert_eq!(receive(&mut a).await["people"].as_array().unwrap().len(), 1);
        let mut b = client(app.clone(), "secret-room-key-123").await;
        assert_eq!(receive(&mut b).await["id"], 2);
        assert_eq!(receive(&mut b).await["people"].as_array().unwrap().len(), 2);
        receive(&mut a).await;
        a.send(Message::Text(r#"{"type":"state","streaming":true}"#.into()))
            .await
            .unwrap();
        assert_eq!(receive(&mut a).await["people"][0]["streaming"], true);
        assert_eq!(receive(&mut b).await["people"][0]["streaming"], true);
        b.send(Message::Text(r#"{"type":"subscribe","to":1}"#.into()))
            .await
            .unwrap();
        assert_eq!(receive(&mut a).await, json!({"type":"subscribe","from":2}));
        b.send(Message::Text(
            r#"{"type":"signal","to":1,"stream":1,"data":{"candidate":"test"}}"#.into(),
        ))
        .await
        .unwrap();
        assert_eq!(receive(&mut a).await["from"], 2);
        drop(b);
        assert_eq!(receive(&mut a).await["people"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn public_room_accepts_join_without_key_or_name() {
        for (join, expected_name) in [
            (r#"{"type":"join"}"#, "Amigo 1"),
            (r#"{"type":"join","name":" Fernando "}"#, "Fernando"),
        ] {
            let app = Arc::new(App {
                public: true,
                key: String::new(),
                origin: None,
                ice: default_ice(),
                room: Mutex::new(Room::default()),
                connections: Arc::new(Semaphore::new(32)),
            });
            let (server_io, client_io) = tokio::io::duplex(65_536);
            let task = tokio::spawn(async move {
                socket(
                    WebSocketStream::from_raw_socket(server_io, Role::Server, None).await,
                    app,
                )
                .await;
            });
            let mut ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
            ws.send(Message::Text(join.into())).await.unwrap();
            for expected in ["welcome", "room"] {
                let frame = tokio::time::timeout(Duration::from_secs(1), ws.next())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                let value: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                assert_eq!(value["type"], expected);
                if expected == "room" {
                    assert_eq!(value["people"][0]["name"], expected_name);
                }
            }
            drop(ws);
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
        }
    }

    #[test]
    fn rejects_broken_turn_configuration() {
        assert!(valid_ice(&json!([])));
        assert!(valid_ice(
            &json!([{"urls":["stun:example.org", "turns:example.org:443?transport=tcp"], "username":"user", "credential":"pass"}])
        ));
        for value in [
            json!({}),
            json!([{"urls":"https://example.org"}]),
            json!([{"urls":"turn:example.org"}]),
            json!([{"urls":[]}]),
        ] {
            assert!(!valid_ice(&value));
        }
    }

    #[test]
    fn http_parser_handles_partial_headers_and_rejects_bodies() {
        assert!(
            parse_request(b"GET / HTTP/1.1\r\nHost: localhost\r\n")
                .unwrap()
                .is_none()
        );
        let (request, size) = parse_request(b"GET /app.js HTTP/1.1\r\nHost: localhost\r\n\r\ntail")
            .unwrap()
            .unwrap();
        assert_eq!(request.uri().path(), "/app.js");
        assert_eq!(size, 41);
        assert!(parse_request(b"GET / HTTP/1.1\r\nContent-Length: 12\r\n\r\n").is_err());
        assert!(parse_request(b"GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").is_err());
    }

    #[tokio::test]
    async fn real_websocket_auth_presence_capacity_and_cleanup() {
        let app = Arc::new(App {
            public: false,
            key: "correct-room-key-123".into(),
            origin: None,
            ice: json!([{"urls":"turn:example.org", "username":"private", "credential":"private"}]),
            room: Mutex::new(Room::default()),
            connections: Arc::new(Semaphore::new(32)),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shared = app.clone();
        let server = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let app = shared.clone();
                tokio::spawn(async move {
                    let _ = connection(stream, app).await;
                });
            }
        });
        async fn join(
            addr: std::net::SocketAddr,
            key: &str,
        ) -> (
            tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
            Value,
        ) {
            let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
            req.headers_mut()
                .insert(header::ORIGIN, format!("http://{addr}").parse().unwrap());
            let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
            ws.send(Message::Text(
                json!({"type":"join","key":key,"name":"Friend"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            let frame = tokio::time::timeout(Duration::from_secs(2), ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            (ws, value)
        }
        let mut req = format!("ws://{addr}/ws").into_client_request().unwrap();
        req.headers_mut()
            .insert(header::ORIGIN, "https://evil.invalid".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(req).await.is_err());
        let (_, denied) = join(addr, "wrong").await;
        assert_eq!(denied["type"], "error");
        assert!(denied.get("iceServers").is_none());
        assert!(app.room.lock().unwrap().peers.is_empty());
        let mut members = Vec::new();
        for id in 1..=8 {
            let (ws, welcome) = join(addr, "correct-room-key-123").await;
            assert_eq!(welcome["id"], id);
            assert_eq!(welcome["iceServers"][0]["username"], "private");
            members.push(ws);
        }
        let (_, full) = join(addr, "correct-room-key-123").await;
        assert_eq!(full["type"], "error");
        assert_eq!(
            app.room.lock().unwrap().snapshot()["people"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
        drop(members);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if app.room.lock().unwrap().peers.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        server.abort();
    }
    #[test]
    fn rejects_bad_names_keys_and_protocol() {
        assert!(valid_name("Ana"));
        assert!(!valid_name(" \n"));
        assert!(!valid_name(&"x".repeat(33)));
        assert!(same_key("abc", "abc"));
        assert!(!same_key("abc", "abd"));
        assert!(!same_key("abc", "abcd"));
        assert!(
            serde_json::from_str::<Input>(r#"{"type":"state","streaming":true,"id":2}"#).is_err()
        );
    }
    #[tokio::test]
    async fn routing_preserves_sender_and_limits_targets() {
        let (tx, mut rx) = mpsc::channel(4);
        let mut room = Room::default();
        room.peers.insert(
            2,
            Peer {
                modern: false,
                info: Person {
                    id: 2,
                    name: "B".into(),
                    streaming: true,
                },
                tx,
            },
        );
        room.memberships.insert(1, 0);
        room.memberships.insert(2, 0);
        assert!(room.route(
            1,
            Input::Subscribe {
                to: 2,
                session: None
            }
        ));
        assert_eq!(
            rx.recv().await.unwrap(),
            json!({"type":"subscribe","from":1})
        );
        assert!(!room.route(
            1,
            Input::Signal {
                to: 2,
                stream: 3,
                data: json!({})
            }
        ));
        assert!(room.route(
            1,
            Input::Signal {
                to: 2,
                stream: 2,
                data: json!({"candidate":null})
            }
        ));
        assert_eq!(rx.recv().await.unwrap()["from"], 1);
        assert!(room.route(
            1,
            Input::Unsubscribe {
                to: 2,
                request: Some(23)
            }
        ));
        assert_eq!(
            rx.recv().await.unwrap(),
            json!({"type":"unsubscribe","from":1,"request":23})
        );
        assert!(!room.route(
            1,
            Input::Unsubscribe {
                to: 2,
                request: Some(0)
            }
        ));
        assert!(room.route(2, Input::State { streaming: false }));
        assert_eq!(room.snapshot()["people"][0]["streaming"], false);
    }
}
