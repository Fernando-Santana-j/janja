use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn name(value: &str) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err("O nome não pode conter caracteres de controle".into());
    }
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 32 {
        return Err("Use um nome de 1 a 32 caracteres".into());
    }
    Ok(value.into())
}
pub fn path() -> Result<PathBuf, String> {
    let base = if cfg!(windows) {
        env::var_os("APPDATA").map(PathBuf::from)
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
    };
    Ok(base
        .ok_or("Pasta de configuração indisponível")?
        .join("janja/name.txt"))
}
pub fn load() -> Option<String> {
    name(&fs::read_to_string(path().ok()?).ok()?).ok()
}
pub fn save(path: &Path, value: &str) -> Result<String, String> {
    let value = name(value)?;
    write_atomic(path, value.as_bytes())?;
    Ok(value)
}
pub(super) fn write_atomic(path: &Path, value: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Pasta inválida")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, value).map_err(|e| e.to_string())?;
    if let Err(e) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(temporary);
        return Err(e.to_string());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_valid_name_and_preserves_it_on_invalid_input() {
        let dir = env::temp_dir().join(format!("janja-profile-{}", std::process::id()));
        let path = dir.join("name.txt");
        assert_eq!(save(&path, " Fernando ").unwrap(), "Fernando");
        for bad in ["".to_string(), "x".repeat(33), "Ana\n".into()] {
            assert!(save(&path, &bad).is_err());
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), "Fernando");
        save(&path, "Ana").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "Ana");
        fs::remove_dir_all(dir).unwrap();
    }
}
