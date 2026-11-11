pub mod diagnostics;
mod icons;
pub mod media;
pub mod network;
#[cfg(target_os = "linux")]
pub mod portal;
mod preferences;
pub mod profile;
mod rooms_ui;
pub mod rtc;
mod source_picker;
pub mod sources;
mod stage;
pub mod ui;
pub mod video;
#[cfg(windows)]
mod windows_preview;

use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub fps: u32,
    pub height: u32,
    pub bitrate: u32,
    pub audio: bool,
    pub exclude_discord: bool,
    #[serde(skip)]
    pub source: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            fps: 30,
            height: 1080,
            bitrate: 8000,
            audio: true,
            exclude_discord: false,
            source: 0,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if ![15, 30, 60].contains(&self.fps)
            || ![720, 1080, 1440].contains(&self.height)
            || !(1000..=20000).contains(&self.bitrate)
        {
            return Err("Configuração fora dos limites".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct Person {
    pub id: u64,
    pub name: String,
    pub streaming: bool,
}
#[derive(Clone, Debug, Deserialize)]
pub struct RoomInfo {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub members: usize,
}
pub enum Command {
    CreateRoom(String, String),
    EnterRoom(u64, String),
    LeaveRoom,
    ListRooms,
    Start(Settings),
    Captured(u64, u64, Settings, Result<media::Capture, String>),
    Stop,
    Volume(u64, u8),
    Watch(u64),
    Unwatch(u64),
    Quit,
    Connected(u64),
    Disconnected(u64, String),
    Server(u64, serde_json::Value),
    Signal(u64, u64, u64, u64, serde_json::Value),
    RemoteReady(u64, u64, u64, u64, bool, Result<(), String>),
    MediaError(u64, u64, u64, u64, String),
}
#[derive(Clone, Debug)]
pub enum Event {
    Rooms(Vec<RoomInfo>),
    SelectedRoom(Option<RoomInfo>),
    Status(String),
    Room(u64, Vec<Person>),
    Streaming(bool),
    Watching(Vec<u64>),
    Error(String),
    CaptureInfo(String),
    MediaStatus(String),
    Recommendation(Result<(Settings, String), String>),
}
pub const ENDPOINT: &str = "wss://infocoretech.com.br/janja/ws";
pub type Commands = std::sync::mpsc::SyncSender<Command>;
pub type Events = gtk::glib::Sender<Event>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_limits() {
        assert!(Settings::default().validate().is_ok());
        let s = Settings {
            fps: 1000,
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }
}
