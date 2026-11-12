use super::{Event, Events, RoomInfo};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc,
};

type Key = (u64, String);
const BUSY: &str =
    "Cofre ocupado ou indisponível; a senha continua disponível apenas nesta sessão.";

enum Job {
    Load(u64, u64, RoomInfo),
    Save(u64, RoomInfo, String),
    Remove(u64, RoomInfo),
}

enum Reply {
    Loaded(u64, u64, RoomInfo, Result<Option<String>, String>),
    Saved(u64, Result<(), String>),
    Removed(u64, Result<(), String>),
}
struct Attempt {
    epoch: u64,
    id: Option<u64>,
    name: Option<String>,
    password: String,
}

pub(super) struct Access {
    jobs: mpsc::SyncSender<Job>,
    replies: mpsc::Receiver<Reply>,
    events: Events,
    rooms: HashMap<u64, RoomInfo>,
    cache: HashMap<Key, String>,
    rejected: HashSet<Key>,
    request: u64,
    lookup: Option<(u64, u64, RoomInfo)>,
    attempt: Option<Attempt>,
}

impl Access {
    fn channels(events: Events) -> (Self, mpsc::Receiver<Job>, mpsc::SyncSender<Reply>) {
        let (jobs, receive) = mpsc::sync_channel(8);
        let (reply, replies) = mpsc::sync_channel(8);
        (
            Self {
                jobs,
                replies,
                events,
                rooms: HashMap::new(),
                cache: HashMap::new(),
                rejected: HashSet::new(),
                request: 0,
                lookup: None,
                attempt: None,
            },
            receive,
            reply,
        )
    }

    pub fn start(events: Events) -> Self {
        let (access, receive, replies) = Self::channels(events);
        // One bounded worker owns all blocking OS calls; dropping Access never waits on the vault.
        let _ = std::thread::Builder::new()
            .name("janja-room-vault".into())
            .spawn(move || {
                while let Ok(job) = receive.recv() {
                    let reply = match job {
                        Job::Load(epoch, request, room) => {
                            let result = super::credentials::load(room.id, &room.name);
                            Reply::Loaded(epoch, request, room, result)
                        }
                        Job::Save(epoch, room, password) => Reply::Saved(
                            epoch,
                            super::credentials::save(room.id, &room.name, &password),
                        ),
                        Job::Remove(epoch, room) => {
                            Reply::Removed(epoch, super::credentials::remove(room.id, &room.name))
                        }
                    };
                    if replies.send(reply).is_err() {
                        break;
                    }
                }
            });
        access
    }

    #[cfg(test)]
    pub fn without_store(events: Events) -> Self {
        Self::channels(events).0
    }

    fn status(&self, text: &str) {
        let _ = self.events.send(Event::Status(text.into()));
    }
    fn warning(&self, text: &str) {
        let _ = self.events.send(Event::Error(text.into()));
    }

    pub fn room(&self, id: u64) -> Option<RoomInfo> {
        self.rooms.get(&id).cloned()
    }

    pub fn update_rooms(&mut self, epoch: u64, rooms: &[RoomInfo]) {
        self.rooms = rooms.iter().map(|room| (room.id, room.clone())).collect();
        let removed = self
            .cache
            .keys()
            .filter(|(id, name)| !self.rooms.get(id).is_some_and(|room| room.name == *name))
            .cloned()
            .collect::<Vec<_>>();
        for (id, name) in removed {
            self.cache.remove(&(id, name.clone()));
            if self
                .jobs
                .try_send(Job::Remove(
                    epoch,
                    RoomInfo {
                        id,
                        name,
                        locked: true,
                        members: 0,
                    },
                ))
                .is_err()
            {
                self.warning("Não foi possível apagar do cofre a senha de uma sala encerrada.");
            }
        }
        self.rejected
            .retain(|(id, name)| self.rooms.get(id).is_some_and(|room| room.name == *name));
        if self.lookup.as_ref().is_some_and(|(_, _, room)| {
            !self
                .rooms
                .get(&room.id)
                .is_some_and(|current| current.name == room.name)
        }) {
            self.cancel();
        }
    }

    pub fn idle(&self) -> Result<(), String> {
        if self.attempt.is_some() {
            Err("Aguarde a confirmação da entrada na sala.".into())
        } else {
            Ok(())
        }
    }

    pub fn cancel(&mut self) {
        self.request = self.request.wrapping_add(1);
        self.lookup = None;
        self.attempt = None;
    }

    pub fn cancel_lookup(&mut self) {
        self.request = self.request.wrapping_add(1);
        self.lookup = None;
        self.status("Conectado");
    }

    pub fn current(&self, epoch: u64, request: u64, room: &RoomInfo) -> bool {
        self.lookup.as_ref().is_some_and(|(e, r, selected)| {
            *e == epoch && *r == request && selected.id == room.id && selected.name == room.name
        })
    }

    pub fn cancel_request(&mut self, epoch: u64, request: u64) {
        if self
            .lookup
            .as_ref()
            .is_some_and(|(e, r, _)| *e == epoch && *r == request)
        {
            self.cancel_lookup();
        }
    }

    fn prompt(&self) {
        if let Some((epoch, request, room)) = &self.lookup {
            let _ = self
                .events
                .send(Event::PasswordRequired(*epoch, *request, room.clone()));
        }
    }

    pub fn begin(&mut self, epoch: u64, room: RoomInfo) -> Result<Option<String>, String> {
        self.idle()?;
        self.cancel_lookup();
        self.lookup = Some((epoch, self.request, room.clone()));
        if !room.locked {
            return Ok(Some(String::new()));
        }
        let key = (room.id, room.name.clone());
        if let Some(password) = self.cache.get(&key) {
            return Ok(Some(password.clone()));
        }
        if self.rejected.contains(&key) {
            self.prompt();
            return Ok(None);
        }
        if self
            .jobs
            .try_send(Job::Load(epoch, self.request, room))
            .is_err()
        {
            self.warning("Não foi possível consultar o cofre. Digite a senha para esta sessão.");
            self.prompt();
        } else {
            self.status("Consultando senha…");
        }
        Ok(None)
    }

    pub fn submitted(
        &mut self,
        epoch: u64,
        id: Option<u64>,
        name: Option<String>,
        password: String,
    ) {
        self.lookup = None;
        self.attempt = Some(Attempt {
            epoch,
            id,
            name,
            password,
        });
        self.status("Entrando na sala…");
    }

    pub fn accepted(&mut self, epoch: u64, room: Option<&RoomInfo>) -> Result<(), String> {
        self.lookup = None;
        let Some(attempt) = self.attempt.take() else {
            return Ok(());
        };
        self.status("Conectado");
        let Some(room) = room else {
            return Ok(());
        };
        if attempt.epoch != epoch
            || room.id == 0
            || attempt.id.is_some_and(|id| id != room.id)
            || attempt.name.as_ref().is_some_and(|name| *name != room.name)
            || (attempt.id.is_none() && self.rooms.contains_key(&room.id))
        {
            return Err(
                "A confirmação não corresponde à sala solicitada; senha não guardada.".into(),
            );
        }
        if attempt.password.is_empty() || attempt.name.is_none() {
            return Ok(());
        }
        let key = (room.id, room.name.clone());
        self.rejected.remove(&key);
        self.cache.insert(key, attempt.password.clone());
        if self
            .jobs
            .try_send(Job::Save(epoch, room.clone(), attempt.password))
            .is_err()
        {
            self.warning(BUSY);
        } else {
            self.status("Salvando senha no cofre…");
        }
        Ok(())
    }

    pub fn error(&mut self, epoch: u64, message: &str) -> bool {
        let Some(attempt) = self.attempt.take() else {
            return false;
        };
        if attempt.epoch == epoch {
            self.status("Conectado");
        }
        if message != "Senha incorreta." || attempt.epoch != epoch {
            return false;
        }
        let Some(room) = attempt.id.and_then(|id| self.rooms.get(&id)).cloned() else {
            return false;
        };
        self.cache.remove(&(room.id, room.name.clone()));
        self.rejected.insert((room.id, room.name.clone()));
        if self
            .jobs
            .try_send(Job::Remove(epoch, room.clone()))
            .is_err()
        {
            self.warning("Senha recusada nesta sessão; não foi possível apagá-la do cofre.");
        } else {
            self.warning("Senha incorreta. Digite a senha novamente.");
        }
        self.cancel_lookup();
        self.lookup = Some((epoch, self.request, room));
        self.prompt();
        true
    }

    pub fn poll(&mut self, epoch: u64) -> Option<(RoomInfo, String)> {
        while let Ok(reply) = self.replies.try_recv() {
            match reply {
                Reply::Loaded(e, request, room, result)
                    if e == epoch && self.current(e, request, &room) =>
                {
                    self.status("Conectado");
                    match result {
                        Ok(Some(password)) if (8..=128).contains(&password.len()) => {
                            return Some((room, password));
                        }
                        Ok(None) => {}
                        Ok(Some(_)) => {
                            self.warning("Senha inválida no cofre. Digite a senha novamente.")
                        }
                        Err(error) => {
                            self.warning(&format!("Não foi possível ler a senha guardada. {error}"))
                        }
                    }
                    self.prompt();
                }
                Reply::Saved(e, result) if e == epoch => {
                    self.status("Conectado");
                    if let Err(error) = result {
                        self.warning(&format!(
                            "Senha lembrada nesta sessão, mas não foi salva no cofre. {error}"
                        ));
                    }
                }
                Reply::Removed(e, Err(error)) if e == epoch => self.warning(&format!(
                    "Senha descartada nesta sessão, mas não foi apagada do cofre. {error}"
                )),
                _ => {}
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    struct Harness {
        access: Access,
        jobs: mpsc::Receiver<Job>,
        replies: mpsc::SyncSender<Reply>,
        context: gtk::glib::MainContext,
        events: Rc<RefCell<Vec<Event>>>,
    }
    impl Harness {
        fn new() -> Self {
            let context = gtk::glib::MainContext::new();
            #[allow(deprecated)]
            let (events, receive) = gtk::glib::MainContext::channel(gtk::glib::Priority::DEFAULT);
            let output = Rc::new(RefCell::new(Vec::new()));
            let delivered = output.clone();
            receive.attach(Some(&context), move |event| {
                delivered.borrow_mut().push(event);
                gtk::glib::ControlFlow::Continue
            });
            let (access, jobs, replies) = Access::channels(events);
            Self {
                access,
                jobs,
                replies,
                context,
                events: output,
            }
        }
        fn drain(&self) {
            while self.context.pending() {
                self.context.iteration(false);
            }
        }
        fn complete(&self, store: &mut HashMap<Key, String>) {
            let reply = match self.jobs.try_recv().unwrap() {
                Job::Load(epoch, request, room) => {
                    let password = store.get(&(room.id, room.name.clone())).cloned();
                    Reply::Loaded(epoch, request, room, Ok(password))
                }
                Job::Save(epoch, room, password) => {
                    store.insert((room.id, room.name), password);
                    Reply::Saved(epoch, Ok(()))
                }
                Job::Remove(epoch, room) => {
                    store.remove(&(room.id, room.name));
                    Reply::Removed(epoch, Ok(()))
                }
            };
            assert!(self.replies.try_send(reply).is_ok());
        }
    }
    fn room(id: u64, name: &str) -> RoomInfo {
        RoomInfo {
            id,
            name: name.into(),
            locked: true,
            members: 1,
        }
    }

    #[test]
    fn saves_only_the_exact_room_confirmed_by_entered_and_reloads_after_restart() {
        let mut h = Harness::new();
        let target = room(7, "Amigos");
        h.access.update_rooms(2, std::slice::from_ref(&target));
        h.access.submitted(
            2,
            Some(target.id),
            Some(target.name.clone()),
            "senha-teste".into(),
        );
        assert!(h.jobs.try_recv().is_err(), "No write before entered");
        assert!(h.access.accepted(2, Some(&room(8, "Amigos"))).is_err());
        assert!(h.jobs.try_recv().is_err());
        h.access.submitted(
            2,
            Some(target.id),
            Some(target.name.clone()),
            "senha-teste".into(),
        );
        assert!(h.access.accepted(2, Some(&room(7, "Outra"))).is_err());
        assert!(h.jobs.try_recv().is_err());
        h.access.submitted(
            2,
            Some(target.id),
            Some(target.name.clone()),
            "senha-teste".into(),
        );
        h.access.accepted(2, Some(&target)).unwrap();
        let mut store = HashMap::new();
        h.complete(&mut store);
        h.access.poll(2);
        h.drain();
        assert!(!format!("{:?}", h.events.borrow()).contains("senha-teste"));
        drop(h);

        let mut reopened = Harness::new();
        reopened
            .access
            .update_rooms(3, std::slice::from_ref(&target));
        assert!(reopened.access.begin(3, target.clone()).unwrap().is_none());
        reopened.complete(&mut store);
        let (selected, password) = reopened.access.poll(3).unwrap();
        assert_eq!(selected.id, target.id);
        assert_eq!(password, "senha-teste");
        assert!(reopened.access.cache.is_empty(), "Loaded is not accepted");
        assert!(reopened.jobs.try_recv().is_err());
    }

    #[test]
    fn create_uses_the_new_id_and_keeps_accepted_password_in_memory_on_store_failure() {
        let mut h = Harness::new();
        let target = room(9, "Nova sala");
        h.access
            .submitted(2, None, Some(target.name.clone()), "senha-nova".into());
        assert!(h.jobs.try_recv().is_err());
        h.access.accepted(2, Some(&target)).unwrap();
        assert!(matches!(h.jobs.try_recv(), Ok(Job::Save(2, room, password))
            if room.id == 9 && room.name == target.name && password == "senha-nova"));
        assert!(
            h.replies
                .try_send(Reply::Saved(2, Err("Cofre bloqueado".into())))
                .is_ok()
        );
        h.access.poll(2);
        h.access.update_rooms(2, std::slice::from_ref(&target));
        assert_eq!(
            h.access.begin(2, target).unwrap().as_deref(),
            Some("senha-nova")
        );
        h.drain();
        assert!(h.events.borrow().iter().any(|event| matches!(event,
            Event::Error(text) if text.contains("não foi salva no cofre"))));
        h.access
            .submitted(2, None, Some("Nova sala".into()), "senha-outra".into());
        assert!(
            h.access.accepted(2, Some(&room(9, "Nova sala"))).is_err(),
            "Create must return a new ID"
        );
    }

    #[test]
    fn wrong_password_discards_cache_and_store_then_prompts_without_automatic_retry() {
        let mut h = Harness::new();
        let target = room(7, "Amigos");
        h.access.update_rooms(2, std::slice::from_ref(&target));
        let mut store = HashMap::from([((target.id, target.name.clone()), "senha-antiga".into())]);
        assert!(h.access.begin(2, target.clone()).unwrap().is_none());
        h.complete(&mut store);
        let (_, password) = h.access.poll(2).unwrap();
        h.access
            .submitted(2, Some(target.id), Some(target.name.clone()), password);
        assert!(h.access.error(2, "Senha incorreta."));
        assert!(h.access.cache.is_empty());
        h.complete(&mut store);
        h.access.poll(2);
        assert!(store.is_empty());
        assert!(h.jobs.try_recv().is_err());
        h.drain();
        assert_eq!(
            h.events
                .borrow()
                .iter()
                .filter(|event| matches!(event, Event::PasswordRequired(..)))
                .count(),
            1
        );
        let request = h.access.request;
        h.access.cancel_request(2, request);
        assert!(!h.access.current(2, request, &target));
        h.access.begin(2, target.clone()).unwrap();
        assert!(
            h.jobs.try_recv().is_err(),
            "Do not reload a refused password"
        );
        h.access.submitted(
            2,
            Some(target.id),
            Some(target.name.clone()),
            "senha-corrigida".into(),
        );
        h.access.accepted(2, Some(&target)).unwrap();
        h.complete(&mut store);
        assert_eq!(
            store.get(&(target.id, target.name)).map(String::as_str),
            Some("senha-corrigida")
        );
    }

    #[test]
    fn stale_lookups_and_cancelled_dialogs_cannot_enter_another_connection_or_room() {
        let mut h = Harness::new();
        let first = room(7, "Primeira");
        let second = room(8, "Segunda");
        h.access.update_rooms(2, &[first.clone(), second.clone()]);
        h.access.begin(2, first.clone()).unwrap();
        let old = h.access.request;
        h.access.begin(2, second.clone()).unwrap();
        let current = h.access.request;
        assert!(!h.access.current(2, old, &first));
        h.access.cancel_request(2, old);
        assert!(h.access.current(2, current, &second));
        assert!(
            h.replies
                .try_send(Reply::Loaded(
                    2,
                    old,
                    first.clone(),
                    Ok(Some("senha-primeira".into()))
                ))
                .is_ok()
        );
        assert!(h.access.poll(2).is_none());
        h.access.cancel();
        h.access.begin(3, second.clone()).unwrap();
        assert!(
            h.replies
                .try_send(Reply::Loaded(
                    2,
                    current,
                    second.clone(),
                    Ok(Some("senha-segunda".into()))
                ))
                .is_ok()
        );
        assert!(h.access.poll(3).is_none());
        let current = h.access.request;
        h.access.cancel_request(3, current);
        assert!(
            h.replies
                .try_send(Reply::Loaded(
                    3,
                    current,
                    second,
                    Ok(Some("senha-cancelada".into()))
                ))
                .is_ok()
        );
        assert!(h.access.poll(3).is_none());
    }

    #[test]
    fn missing_invalid_unavailable_and_full_vault_always_allow_manual_entry() {
        for result in [
            Ok(None),
            Ok(Some("curta".into())),
            Ok(Some("x".repeat(129))),
            Err("Cofre indisponível".into()),
        ] {
            let mut h = Harness::new();
            let target = room(7, "Amigos");
            h.access.begin(2, target.clone()).unwrap();
            assert!(
                h.replies
                    .try_send(Reply::Loaded(2, h.access.request, target, result))
                    .is_ok()
            );
            assert!(h.access.poll(2).is_none());
            h.drain();
            assert!(
                h.events
                    .borrow()
                    .iter()
                    .any(|event| matches!(event, Event::PasswordRequired(..)))
            );
        }
        let mut h = Harness::new();
        for id in 1..=9 {
            h.access.begin(2, room(id, "Amigos")).unwrap();
        }
        h.access
            .submitted(2, Some(9), Some("Amigos".into()), "senha-cheia".into());
        h.access.accepted(2, Some(&room(9, "Amigos"))).unwrap();
        assert_eq!(
            h.access
                .cache
                .get(&(9, "Amigos".into()))
                .map(String::as_str),
            Some("senha-cheia")
        );
        h.drain();
        assert!(
            h.events
                .borrow()
                .iter()
                .any(|event| matches!(event, Event::Error(text) if text == BUSY))
        );
    }
}
