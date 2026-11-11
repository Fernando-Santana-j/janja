use dbus::{
    Path,
    arg::{PropMap, Variant},
    blocking::{Connection, stdintf::org_freedesktop_dbus::Properties},
    message::MatchRule,
};
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
pub struct Session {
    connection: Connection,
    path: Path<'static>,
    remote: OwnedFd,
    pub node: u32,
}
impl Session {
    pub fn fd(&self) -> i32 {
        self.remote.as_raw_fd()
    }
    pub fn open() -> Result<Self, String> {
        let connection = Connection::new_session().map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::sync_channel::<(String, u32, PropMap)>(64);
        connection
            .add_match(
                MatchRule::new_signal("org.freedesktop.portal.Request", "Response"),
                move |(code, results): (u32, PropMap), _, msg| {
                    let path = msg.path().map(|p| p.to_string()).unwrap_or_default();
                    let _ = tx.try_send((path, code, results));
                    true
                },
            )
            .map_err(|e| e.to_string())?;
        let proxy = connection.with_proxy(
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            Duration::from_secs(10),
        );
        let wait = |request: Path<'static>| -> Result<PropMap, String> {
            let deadline = Instant::now() + Duration::from_secs(90);
            while Instant::now() < deadline {
                connection
                    .process(Duration::from_millis(100))
                    .map_err(|e| e.to_string())?;
                while let Ok((path, code, results)) = rx.try_recv() {
                    if path == request.to_string() {
                        return if code == 0 {
                            Ok(results)
                        } else {
                            Err("Compartilhamento de tela cancelado".into())
                        };
                    }
                }
            }
            let p = connection.with_proxy(
                "org.freedesktop.portal.Desktop",
                request,
                Duration::from_secs(2),
            );
            let _: Result<(), _> = p.method_call("org.freedesktop.portal.Request", "Close", ());
            Err("Tempo esgotado na escolha da tela".into())
        };
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = format!(
            "janja{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let mut options = PropMap::new();
        options.insert("session_handle_token".into(), Variant(Box::new(token)));
        let (request,): (Path<'static>,) = proxy
            .method_call(
                "org.freedesktop.portal.ScreenCast",
                "CreateSession",
                (options,),
            )
            .map_err(|e| e.to_string())?;
        let result = wait(request)?;
        let path = Path::new(
            result
                .get("session_handle")
                .and_then(|v| v.0.as_str())
                .ok_or("Sessão de captura ausente")?
                .to_owned(),
        )
        .map_err(|e| e.to_string())?;
        let result = (|| {
            let mut options = PropMap::new();
            let types = proxy
                .get::<u32>("org.freedesktop.portal.ScreenCast", "AvailableSourceTypes")
                .unwrap_or(1)
                & 3;
            if types == 0 {
                return Err("O portal não oferece captura de tela/janela".into());
            }
            let cursors = proxy
                .get::<u32>("org.freedesktop.portal.ScreenCast", "AvailableCursorModes")
                .unwrap_or(1);
            options.insert("types".into(), Variant(Box::new(types)));
            options.insert("multiple".into(), Variant(Box::new(false)));
            options.insert(
                "cursor_mode".into(),
                Variant(Box::new(if cursors & 2 != 0 { 2u32 } else { 1u32 })),
            );
            let (request,): (Path<'static>,) = proxy
                .method_call(
                    "org.freedesktop.portal.ScreenCast",
                    "SelectSources",
                    (&path, options),
                )
                .map_err(|e| e.to_string())?;
            wait(request)?;
            let (request,): (Path<'static>,) = proxy
                .method_call(
                    "org.freedesktop.portal.ScreenCast",
                    "Start",
                    (&path, "", PropMap::new()),
                )
                .map_err(|e| e.to_string())?;
            let result = wait(request)?;
            let node = result
                .get("streams")
                .and_then(|v| v.0.as_iter())
                .and_then(|mut v| v.next())
                .and_then(|v| v.as_iter())
                .and_then(|mut v| v.next())
                .and_then(|v| v.as_u64())
                .ok_or("Tela não selecionada")? as u32;
            let (remote,): (dbus::arg::OwnedFd,) = proxy
                .method_call(
                    "org.freedesktop.portal.ScreenCast",
                    "OpenPipeWireRemote",
                    (&path, PropMap::new()),
                )
                .map_err(|e| e.to_string())?;
            Ok((node, unsafe { OwnedFd::from_raw_fd(remote.into_fd()) }))
        })();
        match result {
            Ok((node, remote)) => Ok(Self {
                connection,
                path,
                remote,
                node,
            }),
            Err(e) => {
                let _: Result<(), _> = connection
                    .with_proxy(
                        "org.freedesktop.portal.Desktop",
                        path,
                        Duration::from_secs(2),
                    )
                    .method_call("org.freedesktop.portal.Session", "Close", ());
                Err(e)
            }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _: Result<(), _> = self
            .connection
            .with_proxy(
                "org.freedesktop.portal.Desktop",
                self.path.clone(),
                Duration::from_secs(2),
            )
            .method_call("org.freedesktop.portal.Session", "Close", ());
    }
}
