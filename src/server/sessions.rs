//! Bounded signaling-session recovery; no media or plaintext passwords are stored.
use super::*;
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const GRACE: Duration = Duration::from_secs(120);
pub(super) struct Session {
    digest: [u8; 32],
    deadline: Option<Instant>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema: u8,
    next: u64,
    next_channel: u64,
    channels: HashMap<u64, PrivateRoom>,
    sessions: Vec<Saved>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    person: Person,
    room: Option<u64>,
    digest: [u8; 32],
    expires: Option<u64>,
}
pub(super) fn cache_path() -> PathBuf {
    env::var_os("JANJA_SESSION_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| "state/session-cache.json".into())
}
fn seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn token_digest(token: &str) -> Option<[u8; 32]> {
    (token.len() == 48 && token.bytes().all(|b| b.is_ascii_hexdigit())).then(|| {
        digest::digest(&digest::SHA256, token.as_bytes())
            .as_ref()
            .try_into()
            .expect("SHA-256 has 32 bytes")
    })
}
impl Room {
    pub(super) fn new_session(&mut self, id: u64) -> Option<String> {
        let mut bytes = [0; 24];
        SystemRandom::new().fill(&mut bytes).ok()?;
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.sessions.insert(
            id,
            Session {
                digest: token_digest(&token)?,
                deadline: None,
            },
        );
        Some(token)
    }
    pub(super) fn resume(
        &mut self,
        token: Option<&str>,
        tx: &mpsc::Sender<Value>,
    ) -> Option<(u64, String)> {
        let token = token?;
        let hash = token_digest(token)?;
        let id = self.sessions.iter().find_map(|(id, s)| {
            // A resume secret authenticates an existing identity, independently of its display name.
            (s.digest
                .iter()
                .zip(hash)
                .fold(0u8, |different, (a, b)| different | (a ^ b))
                == 0
                && s.deadline.is_none_or(|end| end > Instant::now()))
            .then_some(*id)
        })?;
        self.peers.get_mut(&id)?.tx = tx.clone();
        self.sessions.get_mut(&id)?.deadline = None;
        Some((id, token.to_owned()))
    }
    pub(super) fn disconnect(&mut self, id: u64, tx: &mpsc::Sender<Value>, now: Instant) {
        if !self.peers.get(&id).is_some_and(|p| p.tx.same_channel(tx)) {
            return;
        }
        if let Some(session) = self
            .sessions
            .get_mut(&id)
            .filter(|_| self.memberships.contains_key(&id))
        {
            session.deadline = Some(now + GRACE);
        } else {
            self.sessions.remove(&id);
            self.peers.remove(&id);
            self.memberships.remove(&id);
            self.prune();
            self.broadcast();
        }
        self.persist_sessions();
    }
    pub(super) fn expire_sessions(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(id, s)| s.deadline.filter(|end| *end <= now).map(|_| *id))
            .collect();
        if expired.is_empty() {
            return;
        }
        for id in expired {
            self.sessions.remove(&id);
            self.peers.remove(&id);
            self.memberships.remove(&id);
        }
        self.prune();
        self.persist_sessions();
        self.broadcast();
    }
    pub(super) fn persist_sessions(&mut self) {
        let Some(path) = &self.cache else {
            return;
        };
        let now = Instant::now();
        let snapshot = Snapshot {
            schema: 1,
            next: self.next,
            next_channel: self.next_channel,
            channels: self.channels.clone(),
            sessions: self
                .sessions
                .iter()
                .filter_map(|(id, session)| {
                    let peer = self.peers.get(id)?;
                    Some(Saved {
                        person: peer.info.clone(),
                        room: self.memberships.get(id).copied(),
                        digest: session.digest,
                        expires: session.deadline.map(|end| {
                            seconds().saturating_add(end.saturating_duration_since(now).as_secs())
                        }),
                    })
                })
                .collect(),
        };
        let result = (|| -> Result<(), ServerError> {
            let data = serde_json::to_vec(&snapshot)?;
            if data.len() > MAX_MESSAGE {
                return Err("Cache de sessões excede o limite".into());
            }
            let pending = path.with_extension("pending");
            let mut options = fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&pending)?;
            file.write_all(&data)?;
            file.sync_all()?;
            drop(file);
            fs::rename(pending, path)?;
            Ok(())
        })();
        if let Err(error) = result {
            if !self.cache_failed {
                eprintln!("Não foi possível salvar as sessões do Janja: {error}");
            }
            self.cache_failed = true;
        } else {
            self.cache_failed = false;
        }
    }
    pub(super) fn load_sessions(path: PathBuf) -> Result<Self, Box<dyn Error>> {
        let mut room = Self {
            cache: Some(path.clone()),
            ..Self::default()
        };
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let mut directory = fs::DirBuilder::new();
            directory.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                directory.mode(0o700);
            }
            directory.create(parent)?;
        }
        match fs::metadata(&path) {
            Ok(metadata) if metadata.len() > MAX_MESSAGE as u64 => {
                return Err("Cache de sessões excede o limite".into());
            }
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                room.persist_sessions();
                if room.cache_failed {
                    return Err("Não foi possível criar o cache de sessões do servidor".into());
                }
                return Ok(room);
            }
            Err(e) => return Err(e.into()),
        };
        if data.len() > MAX_MESSAGE {
            return Err("Cache de sessões excede o limite".into());
        }
        let snapshot: Snapshot = serde_json::from_slice(&data)?;
        if snapshot.schema != 1
            || snapshot.next == u64::MAX
            || snapshot.next_channel == u64::MAX
            || snapshot.sessions.len() > 32
            || snapshot.channels.len() > 32
            || snapshot
                .channels
                .iter()
                .any(|(id, r)| *id == 0 || *id > snapshot.next_channel || !valid_name(&r.name))
        {
            return Err("Cache de sessões inválido".into());
        }
        room.next = snapshot.next;
        room.next_channel = snapshot.next_channel;
        room.channels = snapshot.channels;
        let now = Instant::now();
        for saved in snapshot.sessions {
            let id = saved.person.id;
            if id == 0
                || id > room.next
                || !valid_name(&saved.person.name)
                || room.peers.contains_key(&id)
                || saved
                    .room
                    .is_some_and(|id| !room.channels.contains_key(&id))
            {
                return Err("Participante inválido no cache de sessões".into());
            }
            let remaining = saved
                .expires
                .map(|end| end.saturating_sub(seconds()))
                .unwrap_or(GRACE.as_secs())
                .min(GRACE.as_secs());
            if remaining == 0 {
                continue;
            }
            if let Some(channel) = saved.room {
                if room.memberships.values().filter(|r| **r == channel).count() >= MAX_PEOPLE {
                    return Err("Sala excede a capacidade no cache de sessões".into());
                }
                room.memberships.insert(id, channel);
            }
            let (tx, _rx) = mpsc::channel(1);
            room.peers.insert(
                id,
                Peer {
                    modern: true,
                    info: saved.person,
                    tx,
                },
            );
            room.sessions.insert(
                id,
                Session {
                    digest: saved.digest,
                    deadline: Some(now + Duration::from_secs(remaining)),
                },
            );
        }
        room.prune();
        Ok(room)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type Client = WebSocketStream<tokio::io::DuplexStream>;
    async fn client(app: Shared, resume: Option<&str>) -> (Client, tokio::task::JoinHandle<()>) {
        let (server_io, client_io) = tokio::io::duplex(65_536);
        let task = tokio::spawn(async move {
            socket(
                WebSocketStream::from_raw_socket(server_io, Role::Server, None).await,
                app,
            )
            .await;
        });
        let mut ws = WebSocketStream::from_raw_socket(client_io, Role::Client, None).await;
        let mut join = json!({"type":"join","name":"Amigo","protocol":2});
        if let Some(token) = resume {
            join["resume"] = json!(token);
        }
        ws.send(Message::Text(join.to_string().into()))
            .await
            .unwrap();
        (ws, task)
    }
    async fn receive(ws: &mut Client, kind: &str) -> Value {
        for _ in 0..32 {
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
    fn app(path: PathBuf) -> Shared {
        Arc::new(App {
            public: true,
            key: String::new(),
            origin: None,
            ice: json!([]),
            room: Mutex::new(Room::load_sessions(path).unwrap()),
            connections: Arc::new(Semaphore::new(32)),
        })
    }
    #[tokio::test]
    async fn a_replaced_websocket_exits_without_waiting_for_heartbeat() {
        let path = env::temp_dir().join(format!(
            "janja-takeover-{}-{}.json",
            std::process::id(),
            seconds()
        ));
        let app = app(path.clone());
        let (mut old, old_task) = client(app.clone(), None).await;
        let welcome = receive(&mut old, "welcome").await;
        let (mut new, new_task) = client(app.clone(), welcome["resume"].as_str()).await;
        let resumed = receive(&mut new, "welcome").await;
        assert_eq!(resumed["id"], welcome["id"]);
        assert_eq!(resumed["resumed"], true);
        tokio::time::timeout(Duration::from_secs(1), old_task)
            .await
            .expect("A stale socket held its connection slot until heartbeat")
            .unwrap();
        assert!(
            app.room
                .lock()
                .unwrap()
                .peers
                .contains_key(&welcome["id"].as_u64().unwrap())
        );
        new_task.abort();
        let _ = new_task.await;
        fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn websocket_resumes_the_same_private_room_after_a_real_server_task_restart() {
        let path = env::temp_dir().join(format!(
            "janja-socket-cache-{}-{}.json",
            std::process::id(),
            seconds()
        ));
        let original = app(path.clone());
        let (mut owner, server) = client(original.clone(), None).await;
        let welcome = receive(&mut owner, "welcome").await;
        let token = welcome["resume"].as_str().unwrap().to_owned();
        let id = welcome["id"].clone();
        owner
            .send(Message::Text(
                json!({"type":"createRoom","name":"Amigos","password":"senha1234"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let channel = receive(&mut owner, "entered").await["room"]["id"].clone();
        owner
            .send(Message::Text(
                json!({"type":"state","streaming":true}).to_string().into(),
            ))
            .await
            .unwrap();
        while receive(&mut owner, "room").await["people"][0]["streaming"] != true {}
        // Abort the task without a clean disconnect, as in a signaling server crash.
        server.abort();
        let _ = server.await;
        drop(original);
        drop(owner);
        let restored = app(path.clone());
        let (mut resumed, server) = client(restored.clone(), Some(&token)).await;
        let welcome = receive(&mut resumed, "welcome").await;
        assert_eq!(welcome["id"], id);
        assert_eq!(welcome["resumed"], true);
        let entered = receive(&mut resumed, "entered").await;
        assert_eq!(entered["room"]["id"], channel);
        assert_eq!(entered["resumed"], true);
        assert_eq!(
            receive(&mut resumed, "room").await["people"][0]["streaming"],
            true
        );
        let (mut stranger, stranger_task) = client(restored.clone(), Some(&"0".repeat(48))).await;
        let stranger_welcome = receive(&mut stranger, "welcome").await;
        assert_ne!(stranger_welcome["id"], id);
        assert_eq!(stranger_welcome["resumed"], false);
        stranger
            .send(Message::Text(
                json!({"type":"enterRoom","room":channel,"password":"wrong"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        assert_eq!(
            receive(&mut stranger, "error").await["message"],
            "Senha incorreta."
        );
        resumed
            .send(Message::Text(
                json!({"type":"leaveRoom"}).to_string().into(),
            ))
            .await
            .unwrap();
        assert!(receive(&mut resumed, "entered").await["room"].is_null());
        assert!(restored.room.lock().unwrap().channels.is_empty());
        server.abort();
        stranger_task.abort();
        let _ = server.await;
        let _ = stranger_task.await;
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn resume_preserves_membership_and_stale_socket_cleanup_cannot_remove_it() {
        let mut room = Room::default();
        let (old_tx, _old_rx) = mpsc::channel(4);
        room.peers.insert(
            1,
            Peer {
                modern: true,
                info: Person {
                    id: 1,
                    name: "Amigo".into(),
                    streaming: true,
                },
                tx: old_tx.clone(),
            },
        );
        room.memberships.insert(1, 5);
        let token = room.new_session(1).unwrap();
        let now = Instant::now();
        room.disconnect(1, &old_tx, now);
        assert_eq!(room.memberships.get(&1), Some(&5));
        let (new_tx, _new_rx) = mpsc::channel(4);
        assert!(room.resume(Some(&"0".repeat(48)), &new_tx).is_none());
        assert!(room.resume(Some(&token), &new_tx).is_some());
        room.disconnect(1, &old_tx, now);
        room.expire_sessions(now + GRACE + GRACE);
        assert!(room.peers[&1].tx.same_channel(&new_tx));
        assert!(room.peers[&1].info.streaming);
        room.disconnect(1, &new_tx, now);
        room.expire_sessions(now + GRACE);
        assert!(room.peers.is_empty());
        assert!(room.memberships.is_empty());
    }
    #[test]
    fn restart_recovers_rooms_and_hashed_sessions_without_plaintext_secrets() {
        let path = env::temp_dir().join(format!(
            "janja-session-test-{}-{}.json",
            std::process::id(),
            seconds()
        ));
        let mut room = Room::load_sessions(path.clone()).unwrap();
        room.next = 1;
        room.next_channel = 5;
        room.channels.insert(
            5,
            PrivateRoom {
                name: "Amigos".into(),
                password: None,
            },
        );
        let (tx, _rx) = mpsc::channel(4);
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
        room.memberships.insert(1, 5);
        let token = room.new_session(1).unwrap();
        room.persist_sessions();
        assert!(!room.cache_failed);
        let data = fs::read_to_string(&path).unwrap();
        assert!(!data.contains(&token));
        let mut restored = Room::load_sessions(path.clone()).unwrap();
        let (tx, _rx) = mpsc::channel(4);
        assert!(restored.resume(Some(&token), &tx).is_some());
        assert_eq!(restored.memberships.get(&1), Some(&5));
        assert_eq!(restored.channels[&5].name, "Amigos");
        assert!(restored.peers[&1].info.streaming);
        // Repeated writes replace the previous complete snapshot, including on Windows.
        restored.persist_sessions();
        assert!(!restored.cache_failed);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn malformed_or_oversized_cache_is_rejected_and_failed_write_keeps_previous_snapshot() {
        let path = env::temp_dir().join(format!(
            "janja-invalid-cache-{}-{}.json",
            std::process::id(),
            seconds()
        ));
        fs::write(&path, "not json").unwrap();
        assert!(Room::load_sessions(path.clone()).is_err());
        fs::write(&path, vec![b' '; MAX_MESSAGE + 1]).unwrap();
        assert!(Room::load_sessions(path.clone()).is_err());
        fs::remove_file(&path).unwrap();
        let mut room = Room::load_sessions(path.clone()).unwrap();
        room.persist_sessions();
        let good = fs::read(&path).unwrap();
        fs::create_dir(path.with_extension("pending")).unwrap();
        room.next = 2;
        room.persist_sessions();
        assert!(room.cache_failed);
        assert_eq!(fs::read(&path).unwrap(), good);
        fs::remove_dir(path.with_extension("pending")).unwrap();
        fs::remove_file(path).unwrap();
    }
}
