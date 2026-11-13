//! Linux desktop association for the portable executable (Wayland and X11).
use std::{io, path::Path};

const ID: &str = "br.com.infocoretech.janja";
const ICON: &[u8] = include_bytes!("../assets/janja.png");

fn entry(executable: &Path) -> io::Result<String> {
    let path = executable
        .to_str()
        .filter(|p| !p.contains(['\n', '\r']))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Caminho não suportado pelo desktop",
            )
        })?;
    let escaped = path
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=Janja\nExec=\"{escaped}\"\nIcon={ID}\nTerminal=false\nStartupWMClass=janja\nCategories=Network;AudioVideo;\n"
    ))
}
fn write_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if std::fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, path)
}
fn install_at(data: &Path, executable: &Path) -> io::Result<()> {
    let desktop = entry(executable)?;
    write_changed(
        &data.join(format!("icons/hicolor/256x256/apps/{ID}.png")),
        ICON,
    )?;
    write_changed(
        &data.join(format!("applications/{ID}.desktop")),
        desktop.as_bytes(),
    )
}
pub fn install() -> io::Result<()> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".local/share"))
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Pasta de usuário ausente"))?;
    let executable = std::env::current_exe()?;
    let launcher = std::env::var_os("JANJA_INSTALLED_LAUNCHER")
        .map(std::path::PathBuf::from)
        .filter(|path| path.parent() == executable.parent() && path.is_file());
    install_at(&data, launcher.as_deref().unwrap_or(&executable))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installs_matching_desktop_and_icon_and_quotes_paths() {
        let data = std::env::temp_dir().join(format!("janja-icon-test-{}", std::process::id()));
        let exe = Path::new("/tmp/My App/$test%/janja");
        install_at(&data, exe).unwrap();
        install_at(&data, exe).unwrap();
        let desktop =
            std::fs::read_to_string(data.join(format!("applications/{ID}.desktop"))).unwrap();
        assert!(desktop.contains("Exec=\"/tmp/My App/\\$test%%/janja\""));
        assert!(desktop.contains(&format!("Icon={ID}")));
        assert_eq!(
            std::fs::read(data.join(format!("icons/hicolor/256x256/apps/{ID}.png"))).unwrap(),
            ICON
        );
        assert!(entry(Path::new("/tmp/invalid\npath")).is_err());
        std::fs::remove_dir_all(data).unwrap();
    }
}
