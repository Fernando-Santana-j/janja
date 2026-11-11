use super::Settings;
use std::{
    fs,
    path::{Path, PathBuf},
};
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
    let value: Settings = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    value.validate()?;
    Ok(value)
}
pub fn save(path: &Path, value: &Settings) -> Result<(), String> {
    value.validate()?;
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    super::profile::write_atomic(path, text.as_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quality_roundtrip_omits_source_and_preserves_good_settings() {
        let dir = std::env::temp_dir().join(format!("janja-quality-{}", std::process::id()));
        let path = dir.join("quality.json");
        let mut value = Settings {
            height: 1440,
            fps: 60,
            bitrate: 12000,
            audio: false,
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
