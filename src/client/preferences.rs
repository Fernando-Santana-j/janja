use super::Settings;
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(serde::Serialize, serde::Deserialize)]
struct Preferences {
    #[serde(default)]
    version: u8,
    #[serde(flatten)]
    settings: Settings,
}
pub fn path() -> Result<PathBuf, String> {
    Ok(super::profile::path()?.with_file_name("quality.json"))
}
pub fn load(path: &Path) -> Result<Settings, String> {
    let bytes = match fs::read(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(e.to_string()),
    };
    if bytes.len() > 4096 {
        return Err("Arquivo de qualidade muito grande".into());
    }
    let mut value: Preferences = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value.version > 1 {
        return Err("Preferências de uma versão mais recente do Janja".into());
    }
    if value.version == 0 {
        // Migrate the old defaults once; later explicit choices keep their saved values.
        value.settings.adaptive = false;
        value.settings.exclude_discord = cfg!(windows);
    }
    value.settings.validate()?;
    Ok(value.settings)
}
pub fn save(path: &Path, value: &Settings) -> Result<(), String> {
    value.validate()?;
    let text = serde_json::to_string(&Preferences {
        version: 1,
        settings: value.clone(),
    })
    .map_err(|e| e.to_string())?;
    super::profile::write_atomic(path, text.as_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_preferences_receive_new_defaults_once() {
        let dir = std::env::temp_dir().join(format!("janja-migration-{}", std::process::id()));
        let path = dir.join("quality.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, br#"{"fps":60,"height":720,"bitrate":4000,"audio":false,"adaptive":true,"exclude_discord":false}"#).unwrap();
        let mut settings = load(&path).unwrap();
        assert!(!settings.adaptive);
        assert_eq!(settings.exclude_discord, cfg!(windows));
        assert_eq!(
            (
                settings.fps,
                settings.height,
                settings.bitrate,
                settings.audio
            ),
            (60, 720, 4000, false)
        );
        settings.adaptive = true;
        settings.exclude_discord = false;
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn first_run_uses_requested_defaults() {
        let path = std::env::temp_dir()
            .join(format!("janja-missing-{}", std::process::id()))
            .join("quality.json");
        let settings = load(&path).unwrap();
        assert!(!settings.adaptive);
        assert_eq!(settings.exclude_discord, cfg!(windows));
    }
    #[test]
    fn quality_roundtrip_omits_source_and_preserves_good_settings() {
        let dir = std::env::temp_dir().join(format!("janja-quality-{}", std::process::id()));
        let path = dir.join("quality.json");
        let mut value = Settings {
            height: 1440,
            fps: 60,
            bitrate: 12000,
            audio: false,
            adaptive: false,
            source: 98765,
            ..Settings::default()
        };
        save(&path, &value).unwrap();
        value.source = 0;
        assert_eq!(load(&path).unwrap(), value);
        assert!(!fs::read_to_string(&path).unwrap().contains("source"));
        let bad = Settings {
            fps: 1000,
            ..value.clone()
        };
        assert!(save(&path, &bad).is_err());
        assert_eq!(load(&path).unwrap(), value);
        fs::write(&path, b"{invalid").unwrap();
        assert!(load(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
