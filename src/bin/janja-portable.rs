#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#[cfg(target_os = "linux")]
#[path = "../desktop.rs"]
mod desktop;
use std::{
    error::Error,
    fs,
    hash::{Hash, Hasher},
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};
const BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/janja-bundle.zip"));

fn extract(data: &[u8], directory: &Path) -> Result<(), Box<dyn Error>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(data))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.enclosed_name().ok_or("Caminho inválido no pacote")?;
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("Link inválido no pacote".into());
        }
        let path = directory.join(name);
        if entry.is_dir() {
            fs::create_dir_all(path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        std::io::copy(&mut entry, &mut fs::File::create(&path)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                path,
                fs::Permissions::from_mode(entry.unix_mode().unwrap_or(0o644) & 0o777),
            )?;
        }
    }
    Ok(())
}
fn cache_root() -> Result<PathBuf, Box<dyn Error>> {
    #[cfg(windows)]
    return Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA ausente")?)
            .join("Janja/runtime"),
    );
    #[cfg(unix)]
    {
        let root = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
            .ok_or("Pasta de usuário ausente")?;
        Ok(root.join("janja/runtime"))
    }
}
fn run() -> Result<(), Box<dyn Error>> {
    #[cfg(target_os = "linux")]
    if std::env::args().any(|arg| arg == "--install-desktop") {
        desktop::install()?;
        return Ok(());
    }
    if BUNDLE.is_empty() {
        return Err("Pacote vazio; gere o executável com scripts/package-native.py".into());
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    BUNDLE.hash(&mut hash);
    let root = cache_root()?;
    fs::create_dir_all(&root)?;
    let directory = root.join(format!("{:016x}", hash.finish()));
    if !directory.join(".ready").is_file() {
        let temporary = root.join(format!(
            ".extract-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir(&temporary)?;
        let result = (|| -> Result<(), Box<dyn Error>> {
            extract(BUNDLE, &temporary)?;
            fs::write(temporary.join(".ready"), b"1")?;
            if let Err(e) = fs::rename(&temporary, &directory)
                && !directory.join(".ready").is_file()
            {
                return Err(e.into());
            }
            Ok(())
        })();
        if temporary.exists() {
            let _ = fs::remove_dir_all(&temporary);
        }
        result?;
    }
    #[cfg(target_os = "linux")]
    if std::env::args_os().len() == 1
        && let Err(error) = desktop::install()
    {
        eprintln!("Não foi possível registrar o ícone no desktop: {error}");
    }
    let bin = directory.join("bin");
    let executable = bin.join(if cfg!(windows) { "janja.exe" } else { "janja" });
    let mut command = Command::new(executable);
    command.current_dir(&directory);
    command.args(std::env::args_os().skip(1));
    let mut paths = vec![bin.clone()];
    if let Some(value) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&value));
    }
    command.env("PATH", std::env::join_paths(paths)?);
    if directory.join("etc/fonts/fonts.conf").is_file() {
        command.env("FONTCONFIG_PATH", directory.join("etc/fonts"));
        command.env("FONTCONFIG_FILE", directory.join("etc/fonts/fonts.conf"));
    }
    command.env("GST_PLUGIN_PATH_1_0", directory.join("plugins"));
    command.env("GST_PLUGIN_SYSTEM_PATH_1_0", "");
    command.env(
        "GST_PLUGIN_SCANNER",
        bin.join(if cfg!(windows) {
            "gst-plugin-scanner.exe"
        } else {
            "gst-plugin-scanner"
        }),
    );
    command.env("GST_REGISTRY", directory.join("registry.bin"));
    command.env(
        "GSETTINGS_SCHEMA_DIR",
        directory.join("share/glib-2.0/schemas"),
    );
    #[cfg(unix)]
    {
        command.env("SPA_PLUGIN_DIR", directory.join("lib/spa-0.2"));
        command.env("PIPEWIRE_MODULE_DIR", directory.join("lib/pipewire-0.3"));
        let mut libs = vec![directory.join("lib")];
        if let Some(v) = std::env::var_os("LD_LIBRARY_PATH") {
            libs.extend(std::env::split_paths(&v));
        }
        command.env("LD_LIBRARY_PATH", std::env::join_paths(libs)?);
        let mut dirs = vec![directory.join("share")];
        dirs.extend(std::env::split_paths(
            &std::env::var_os("XDG_DATA_DIRS").unwrap_or("/usr/local/share:/usr/share".into()),
        ));
        command.env("XDG_DATA_DIRS", std::env::join_paths(dirs)?);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Replace the extractor so its embedded archive does not stay resident during streaming.
        Err(command.exec().into())
    }
    #[cfg(windows)]
    {
        if std::env::args().any(|a| console_mode(&a)) {
            if !command.status()?.success() {
                return Err("O diagnóstico encerrou com erro.".into());
            }
        } else {
            command.spawn()?;
        }
        Ok(())
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Janja: {e}");
        #[cfg(windows)]
        if !std::env::args().any(|a| console_mode(&a)) {
            unsafe {
                #[link(name = "user32")]
                unsafe extern "system" {
                    fn MessageBoxW(
                        window: *mut std::ffi::c_void,
                        text: *const u16,
                        title: *const u16,
                        flags: u32,
                    ) -> i32;
                }
                let text: Vec<_> = format!("{e}").encode_utf16().chain(Some(0)).collect();
                let title: Vec<_> = "Janja".encode_utf16().chain(Some(0)).collect();
                MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
            }
        }
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn extraction_rejects_parent_paths() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("../escape", zip::write::FileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(extract(&bytes, Path::new("/tmp/janja-unused")).is_err());
    }
    #[test]
    fn extracts_an_executable_and_content() {
        let directory =
            std::env::temp_dir().join(format!("janja-extract-test-{}", std::process::id()));
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "bin/janja",
            zip::write::FileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
        zip.write_all(b"test").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        extract(&bytes, &directory).unwrap();
        assert_eq!(fs::read(directory.join("bin/janja")).unwrap(), b"test");
        fs::remove_dir_all(directory).unwrap();
    }
}

#[cfg(windows)]
fn console_mode(arg: &str) -> bool {
    if arg == "--version" {
        return true;
    }
    #[cfg(feature = "diagnostics")]
    if matches!(arg, "--check" | "--self-test" | "--network-test") {
        return true;
    }
    false
}
