pub const BUILD: &str = crate::BUILD;
#[cfg(any(test, feature = "diagnostics"))]
macro_rules! event {
    ($kind:expr,$data:expr $(,)?) => {
        $crate::client::diagnostics::record_event($kind, $data)
    };
}
#[cfg(not(any(test, feature = "diagnostics")))]
macro_rules! event {
    ($kind:expr,$data:expr $(,)?) => {
        ()
    };
}
#[cfg(any(test, feature = "diagnostics"))]
pub use enabled::*;
pub(crate) use event;
#[cfg(not(any(test, feature = "diagnostics")))]
pub fn init() {
    gst::debug_set_active(false);
}
#[cfg(not(any(test, feature = "diagnostics")))]
pub fn flush() {}
#[cfg(not(any(test, feature = "diagnostics")))]
pub fn snapshot(_: &gst::Pipeline) {}
#[cfg(any(test, feature = "diagnostics"))]
mod enabled {
    use super::BUILD;
    use gst::prelude::*;
    use serde_json::json;
    use std::{
        fs::{self, File},
        io::Write,
        path::PathBuf,
        sync::{OnceLock, mpsc},
        time::{SystemTime, UNIX_EPOCH},
    };

    enum LogItem {
        Record(&'static str, serde_json::Value, u128),
        Flush(mpsc::Sender<()>),
    }
    static LOG: OnceLock<mpsc::SyncSender<LogItem>> = OnceLock::new();
    static DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    const LIMIT: u64 = 8 * 1024 * 1024;
    pub fn directory() -> PathBuf {
        std::env::var_os("JANJA_LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    PathBuf::from(
                        std::env::var_os("LOCALAPPDATA")
                            .unwrap_or_else(|| std::env::temp_dir().into_os_string()),
                    )
                    .join("Janja/logs")
                } else {
                    std::env::var_os("XDG_STATE_HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| {
                            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                                .join(".local/state")
                        })
                        .join("janja/logs")
                }
            })
    }
    pub fn init() {
        let result = (|| -> std::io::Result<()> {
            let dir = directory();
            fs::create_dir_all(&dir)?;
            let path = dir.join(format!("janja-{}.jsonl", std::process::id()));
            // Keep the latest four runs, including this one.
            let mut old: Vec<_> = fs::read_dir(&dir)?
                .filter_map(Result::ok)
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    n.starts_with("janja-") && n.ends_with(".jsonl")
                })
                .collect();
            old.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
            for entry in old.iter().take(old.len().saturating_sub(3)) {
                let _ = fs::remove_file(entry.path());
            }
            let mut file = File::create(path)?;
            let (tx, rx) = mpsc::sync_channel::<LogItem>(256);
            std::thread::Builder::new()
                .name("janja-log".into())
                .spawn(move || {
                    for item in rx {
                        match item {
                            LogItem::Record(kind, data, at) => {
                                let dropped = DROPPED.swap(0, std::sync::atomic::Ordering::Relaxed);
                                if dropped > 0 {
                                    let _ = write_record_at(
                                        &mut file,
                                        "diagnostics-overflow",
                                        json!({"dropped":dropped}),
                                        at,
                                    );
                                }
                                let _ = write_record_at(&mut file, kind, data, at);
                            }
                            LogItem::Flush(done) => {
                                let _ = file.flush();
                                let _ = done.send(());
                            }
                        }
                    }
                })?;
            let _ = LOG.set(tx);
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("Não foi possível criar logs: {error}");
        }
        event!(
            "startup",
            json!({"build":BUILD, "os":std::env::consts::OS,"arch":std::env::consts::ARCH,"gstreamer":gst::version_string().to_string()}),
        );
        gst::debug_set_default_threshold(gst::DebugLevel::Warning);
        gst::debug_add_log_function(|category, level, _, _, _, object, message| {
            event!(
                "gstreamer",
                json!({"category":category.get_name(),"level":format!("{level:?}"),"object":object.map(object_identity),"message":message.get().map(|s|s.to_string())}),
            );
        });
    }
    fn object_identity(object: &glib_old::Object) -> String {
        // Warning callbacks may run while the object's non-recursive mutex is held.
        format!("{:p}", object.as_ptr())
    }

    pub fn record_event(kind: &'static str, data: serde_json::Value) {
        if let Some(log) = LOG.get() {
            let at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            if log.try_send(LogItem::Record(kind, data, at)).is_err() {
                DROPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    pub fn flush() {
        if let Some(log) = LOG.get() {
            let (tx, rx) = mpsc::channel();
            if log.try_send(LogItem::Flush(tx)).is_ok() {
                let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
            }
        }
    }
    #[cfg(test)]
    fn write_record(file: &mut File, kind: &str, data: serde_json::Value) -> std::io::Result<()> {
        write_record_at(
            file,
            kind,
            data,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
        )
    }
    fn write_record_at(
        file: &mut File,
        kind: &str,
        data: serde_json::Value,
        at: u128,
    ) -> std::io::Result<()> {
        let record = json!({"at_ms":at,"event":kind,"data":data}).to_string();
        if file.metadata()?.len() + (record.len() as u64) < LIMIT {
            writeln!(file, "{record}")?;
        }
        Ok(())
    }
    pub fn safe_caps(caps: &gst::CapsRef) -> serde_json::Value {
        let fields = [
            "media",
            "payload",
            "clock-rate",
            "encoding-name",
            "ssrc",
            "format",
            "width",
            "height",
            "framerate",
            "pixel-aspect-ratio",
            "rate",
            "channels",
            "alignment",
            "stream-format",
        ];
        json!(
            caps.iter()
                .map(|structure| {
                    let mut cleaned = structure.to_owned();
                    let remove: Vec<_> = cleaned
                        .fields()
                        .filter(|key| !fields.contains(key))
                        .map(str::to_owned)
                        .collect();
                    for key in remove {
                        cleaned.remove_field(&key);
                    }
                    cleaned.to_string()
                })
                .collect::<Vec<_>>()
        )
    }

    pub fn snapshot(p: &gst::Pipeline) {
        for element in p.iterate_recurse().into_iter().flatten() {
            let pads: Vec<_> = element.get_pads().iter().map(|pad|json!({"pad":pad.get_name().to_string(),"caps":pad.get_current_caps().map(|c|safe_caps(c.as_ref()))})).collect();
            event!(
                "pipeline",
                json!({"element":element.get_path_string().to_string(),"factory":element.get_factory().map(|f|f.get_name().to_string()),"pads":pads}),
            );
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn caps_logs_omit_ice_credentials() {
            gst::init().unwrap();
            let caps = gst::Caps::builder("application/x-rtp")
                .field("media", &"video")
                .field("a-ice-pwd", &"secret-credential")
                .build();
            let text = safe_caps(caps.as_ref()).to_string();
            assert!(text.contains("video"));
            assert!(!text.contains("secret-credential"));
            assert!(!text.contains("a-ice-pwd"));
        }
        #[test]
        fn log_identity_does_not_wait_for_gstreamer_object_lock() {
            gst::init().unwrap();
            let object = gst::ElementFactory::make("fakesrc", None)
                .unwrap()
                .upcast::<gst::Object>();
            let copy = object.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            // GStreamer can emit a warning while holding this non-recursive mutex.
            unsafe {
                gst::glib_sys::g_mutex_lock(&mut (*object.as_ptr()).lock);
            }
            let worker = std::thread::spawn(move || {
                let _ = tx.send(object_identity(copy.upcast_ref::<glib_old::Object>()));
            });
            let result = rx.recv_timeout(std::time::Duration::from_millis(300));
            unsafe {
                gst::glib_sys::g_mutex_unlock(&mut (*object.as_ptr()).lock);
            }
            worker.join().unwrap();
            assert!(
                result.is_ok(),
                "Logging tried to acquire a GStreamer object lock"
            );
        }
        #[test]
        fn records_json_and_bounds_log_size() {
            let path = std::env::temp_dir().join(format!("janja-log-test-{}", std::process::id()));
            let mut file = File::create(&path).unwrap();
            write_record(
                &mut file,
                "caps",
                json!({"message":"line\nquoted \"text\""}),
            )
            .unwrap();
            let text = fs::read_to_string(&path).unwrap();
            assert_eq!(text.lines().count(), 1);
            let record: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(record["event"], "caps");
            file.set_len(LIMIT).unwrap();
            write_record(&mut file, "excess", json!({})).unwrap();
            assert_eq!(file.metadata().unwrap().len(), LIMIT);
            drop(file);
            fs::remove_file(path).unwrap();
        }
    }
}
