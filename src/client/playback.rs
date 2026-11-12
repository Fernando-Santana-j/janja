use super::{Event, Events};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    thread::{self, Thread},
};

const IDLE: u8 = 0;
pub(super) const WATCHING: u8 = 1;
const CLOSED: u8 = 2;

pub struct Awake {
    state: Arc<AtomicU8>,
    worker: Option<Thread>,
    #[cfg(windows)]
    _screensaver: Option<super::screensaver::Filter>,
}

impl Awake {
    pub fn new(events: Events) -> Self {
        let state = Arc::new(AtomicU8::new(IDLE));
        let desired = state.clone();
        let errors = events.clone();
        let worker = thread::Builder::new()
            .name("janja-playback".into())
            .spawn(move || {
                run(&desired, platform::Guard::acquire, |message| {
                    let _ = errors.send(Event::PlaybackError(format!(
                        "Não foi possível impedir o descanso automático: {message}"
                    )));
                });
            });
        let worker = match worker {
            Ok(worker) => Some(worker.thread().clone()),
            Err(error) => {
                let _ = events.send(Event::PlaybackError(format!(
                    "Não foi possível manter a tela ativa: {error}"
                )));
                None
            }
        };
        Self {
            #[cfg(windows)]
            _screensaver: Some(super::screensaver::Filter::new(state.clone())),
            state,
            worker,
        }
    }

    pub fn watching(&self, active: bool) {
        let next = if active { WATCHING } else { IDLE };
        let mut current = self.state.load(Ordering::Acquire);
        while current != CLOSED && current != next {
            match self.state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.wake();
                    break;
                }
                Err(latest) => current = latest,
            }
        }
    }

    pub fn shutdown(&self) {
        self.state.store(CLOSED, Ordering::Release);
        self.wake();
    }

    fn wake(&self) {
        if let Some(worker) = &self.worker {
            worker.unpark();
        }
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run<G>(
    state: &AtomicU8,
    mut acquire: impl FnMut() -> Result<G, String>,
    mut failure: impl FnMut(String),
) {
    let mut guard = None;
    let mut attempted = false;
    loop {
        match state.load(Ordering::Acquire) {
            CLOSED => break,
            IDLE => {
                guard.take();
                attempted = false;
            }
            WATCHING if !attempted => {
                attempted = true;
                match acquire() {
                    Ok(value) => guard = Some(value),
                    Err(message) if state.load(Ordering::Acquire) == WATCHING => failure(message),
                    Err(_) => {}
                }
            }
            _ => {}
        }
        // Latest state wins, including a stop received while an OS call was pending.
        thread::park();
    }
}

#[cfg(windows)]
mod platform {
    const CONTINUOUS: u32 = 0x8000_0000;
    const SYSTEM_REQUIRED: u32 = 1;
    const DISPLAY_REQUIRED: u32 = 2;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetThreadExecutionState(flags: u32) -> u32;
    }
    pub struct Guard;
    impl Guard {
        pub fn acquire() -> Result<Self, String> {
            // SAFETY: integer-only Win32 ABI; this worker also owns the reset on the same thread.
            if unsafe { SetThreadExecutionState(CONTINUOUS | SYSTEM_REQUIRED | DISPLAY_REQUIRED) }
                == 0
            {
                Err(std::io::Error::last_os_error().to_string())
            } else {
                Ok(Self)
            }
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: clears only this worker's execution requirements on its owning thread.
            unsafe {
                SetThreadExecutionState(CONTINUOUS);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use dbus::{
        Path,
        arg::{PropMap, Variant},
        blocking::Connection,
    };
    use std::time::Duration;
    const TIMEOUT: Duration = Duration::from_secs(2);
    const REASON: &str = "Assistindo a uma transmissão no Janja";
    const FLAGS: u32 = 4 | 8; // Suspend and idle; never inhibit logout or switching users.
    const MANAGERS: [(&str, &str, &str); 2] = [
        (
            "org.gnome.SessionManager",
            "/org/gnome/SessionManager",
            "org.gnome.SessionManager",
        ),
        (
            "org.xfce.SessionManager",
            "/org/xfce/SessionManager",
            "org.xfce.Session.Manager",
        ),
    ];
    enum Handle {
        Session(usize, u32),
        Portal(Path<'static>),
    }
    pub struct Guard {
        connection: Connection,
        handle: Handle,
    }
    impl Guard {
        pub fn acquire() -> Result<Self, String> {
            Self::connect(Connection::new_session().map_err(|e| e.to_string())?)
        }
        fn connect(connection: Connection) -> Result<Self, String> {
            for (index, (name, path, interface)) in MANAGERS.iter().enumerate() {
                let reply: Result<(u32,), _> =
                    connection.with_proxy(*name, *path, TIMEOUT).method_call(
                        *interface,
                        "Inhibit",
                        ("br.com.infocoretech.janja", 0u32, REASON, FLAGS),
                    );
                if let Ok((cookie,)) = reply {
                    return Ok(Self {
                        connection,
                        handle: Handle::Session(index, cookie),
                    });
                }
            }
            let mut options = PropMap::new();
            options.insert("reason".into(), Variant(Box::new(REASON.to_owned())));
            let (handle,): (Path<'static>,) = connection.with_proxy(
                "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", TIMEOUT
            ).method_call("org.freedesktop.portal.Inhibit", "Inhibit", ("", FLAGS, options))
                .map_err(|e| format!("O desktop precisa oferecer o portal Inhibit ou um gerenciador de sessão compatível ({e})."))?;
            Ok(Self {
                connection,
                handle: Handle::Portal(handle),
            })
        }
    }
    #[cfg(test)]
    #[test]
    fn private_bus_checks_session_and_portal_acquire_and_release() {
        use dbus::{
            channel::{MatchingReceiver, Sender},
            message::MatchRule,
        };
        use std::{
            fs,
            io::{BufRead, BufReader},
            process::{Command, Stdio},
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
                mpsc,
            },
            thread,
        };
        struct Bus(std::process::Child);
        impl Drop for Bus {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        fn connection(address: &str) -> Connection {
            let mut channel = dbus::channel::Channel::open_private(address).unwrap();
            channel.register().unwrap();
            channel.into()
        }
        let dir = std::env::temp_dir().join(format!("janja-playback-bus-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("bus.conf");
        // No service directories: this test can never activate the user's desktop services.
        fs::write(&config, format!(r#"<busconfig><type>session</type><listen>unix:tmpdir={}</listen><auth>EXTERNAL</auth><policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>"#, dir.display())).unwrap();
        let mut bus = match Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(bus) => Bus(bus),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("SKIP: dbus-daemon não instalado");
                fs::remove_dir_all(dir).unwrap();
                return;
            }
            Err(e) => panic!("{e}"),
        };
        let mut address = String::new();
        BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let address = address.trim();
        for portal in [false, true] {
            let server = connection(address);
            server
                .request_name(
                    if portal {
                        "org.freedesktop.portal.Desktop"
                    } else {
                        MANAGERS[0].0
                    },
                    false,
                    true,
                    false,
                )
                .unwrap();
            let (tx, rx) = mpsc::channel();
            server.start_receive(
                MatchRule::new_method_call(),
                Box::new(move |msg, conn| {
                    let reply = match msg.member().as_deref() {
                        Some("Inhibit") if portal => {
                            let (_, flags, options): (String, u32, PropMap) = msg.read3().unwrap();
                            assert_eq!(flags, 12);
                            assert!(options.contains_key("reason"));
                            tx.send("acquired").unwrap();
                            msg.method_return().append1(
                                Path::new("/org/freedesktop/portal/desktop/request/test/awake")
                                    .unwrap(),
                            )
                        }
                        Some("Inhibit") => {
                            let (app, _, _, flags): (String, u32, String, u32) =
                                msg.read4().unwrap();
                            assert_eq!(app, "br.com.infocoretech.janja");
                            assert_eq!(flags, 12);
                            tx.send("acquired").unwrap();
                            msg.method_return().append1(71u32)
                        }
                        Some("Uninhibit") => {
                            assert_eq!(msg.read1::<u32>().unwrap(), 71);
                            tx.send("released").unwrap();
                            msg.method_return()
                        }
                        Some("Close") => {
                            tx.send("released").unwrap();
                            msg.method_return()
                        }
                        other => panic!("Unexpected method {other:?}"),
                    };
                    conn.send(reply).unwrap();
                    true
                }),
            );
            let stop = Arc::new(AtomicBool::new(false));
            let done = stop.clone();
            let server = thread::spawn(move || {
                while !done.load(Ordering::Acquire) {
                    server.process(Duration::from_millis(10)).unwrap();
                }
            });
            let hold = Guard::connect(connection(address)).unwrap();
            assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), "acquired");
            assert_eq!(matches!(hold.handle, Handle::Portal(_)), portal);
            drop(hold);
            assert_eq!(rx.recv_timeout(TIMEOUT).unwrap(), "released");
            stop.store(true, Ordering::Release);
            server.join().unwrap();
        }
        drop(bus);
        fs::remove_dir_all(dir).unwrap();
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            match &self.handle {
                Handle::Session(index, cookie) => {
                    let (name, path, interface) = MANAGERS[*index];
                    let _: Result<(), _> = self
                        .connection
                        .with_proxy(name, path, TIMEOUT)
                        .method_call(interface, "Uninhibit", (*cookie,));
                }
                Handle::Portal(path) => {
                    let _: Result<(), _> = self
                        .connection
                        .with_proxy("org.freedesktop.portal.Desktop", path, TIMEOUT)
                        .method_call("org.freedesktop.portal.Request", "Close", ());
                }
            }
            // The private bus connection also releases holds if explicit cleanup fails.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Barrier, mpsc},
        time::Duration,
    };
    struct Hold(mpsc::Sender<&'static str>);
    impl Drop for Hold {
        fn drop(&mut self) {
            self.0.send("released").unwrap();
        }
    }
    fn take(rx: &mpsc::Receiver<&'static str>) -> &'static str {
        rx.recv_timeout(Duration::from_secs(2)).unwrap()
    }

    #[test]
    fn watching_holds_once_until_last_video_closes_and_releases_on_shutdown() {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(IDLE));
        let worker_state = state.clone();
        let worker = thread::spawn(move || {
            run(
                &worker_state,
                || {
                    tx.send("acquired").unwrap();
                    Ok(Hold(tx.clone()))
                },
                |message| panic!("{message}"),
            )
        });
        let awake = Awake {
            state,
            worker: Some(worker.thread().clone()),
            #[cfg(windows)]
            _screensaver: None,
        };
        awake.watching(true);
        assert_eq!(take(&rx), "acquired");
        awake.watching(true);
        awake.watching(false);
        assert_eq!(take(&rx), "released");
        awake.watching(true);
        assert_eq!(take(&rx), "acquired");
        awake.shutdown();
        awake.watching(true);
        worker.join().unwrap();
        assert_eq!(take(&rx), "released");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn close_during_os_request_releases_late_guard_without_blocking_ui() {
        let (tx, rx) = mpsc::channel();
        let release = Arc::new(Barrier::new(2));
        let pending = release.clone();
        let state = Arc::new(AtomicU8::new(WATCHING));
        let worker_state = state.clone();
        let worker = thread::spawn(move || {
            run(
                &worker_state,
                || {
                    tx.send("pending").unwrap();
                    pending.wait();
                    Ok(Hold(tx.clone()))
                },
                |message| panic!("{message}"),
            )
        });
        let awake = Awake {
            state,
            worker: Some(worker.thread().clone()),
            #[cfg(windows)]
            _screensaver: None,
        };
        assert_eq!(take(&rx), "pending");
        drop(awake);
        release.wait();
        worker.join().unwrap();
        assert_eq!(take(&rx), "released");
    }

    #[test]
    fn failure_does_not_retry_in_a_busy_loop() {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(WATCHING));
        let worker_state = state.clone();
        let worker = thread::spawn(move || {
            run::<()>(
                &worker_state,
                || Err("unavailable".into()),
                |_| {
                    tx.send("error").unwrap();
                },
            )
        });
        let awake = Awake {
            state,
            worker: Some(worker.thread().clone()),
            #[cfg(windows)]
            _screensaver: None,
        };
        assert_eq!(take(&rx), "error");
        drop(awake);
        worker.join().unwrap();
        assert!(rx.try_recv().is_err());
    }
}
