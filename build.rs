fn main() {
    println!("cargo:rerun-if-changed=assets/janja.ico");
    println!("cargo:rerun-if-changed=assets/janja.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let output =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("janja-icon.o");
        let status = std::process::Command::new("windres")
            .args(["-i", "assets/janja.rc", "-O", "coff", "-o"])
            .arg(&output)
            .status()
            .expect("Instale binutils do MSYS2/UCRT64 (windres)");
        assert!(status.success(), "Falha ao compilar o ícone Windows");
        println!("cargo:rustc-link-arg-bins={}", output.display());
    }

    println!("cargo:rerun-if-env-changed=JANJA_BUNDLE");
    let target =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("janja-bundle.zip");
    if let Some(path) = std::env::var_os("JANJA_BUNDLE") {
        println!(
            "cargo:rerun-if-changed={}",
            std::path::Path::new(&path).display()
        );
        std::fs::copy(path, target).expect("Não foi possível ler JANJA_BUNDLE");
    } else {
        std::fs::write(target, []).unwrap();
    }
}
