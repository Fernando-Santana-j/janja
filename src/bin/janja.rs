#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
fn main() {
    if std::env::args().any(|arg| arg == "--version") {
        println!(
            "Janja {} (GTK + GStreamer; sem navegador)",
            sala::client::diagnostics::BUILD
        );
        return;
    }
    #[cfg(target_os = "windows")]
    unsafe {
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
        }
        let id: Vec<u16> = "br.com.infocoretech.janja"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
    }
    // Xlib must be initialized before GTK and any multimedia worker.
    #[cfg(target_os = "linux")]
    unsafe {
        #[link(name = "X11")]
        unsafe extern "C" {
            fn XInitThreads() -> i32;
        }
        XInitThreads();
    }
    sala::client::diagnostics::init();
    #[cfg(feature = "diagnostics")]
    {
        let args: Vec<_> = std::env::args().collect();
        if args
            .iter()
            .any(|a| a == "--self-test" || a == "--network-test")
        {
            let result = sala::client::media::self_test(args.iter().any(|a| a == "--network-test"));
            let report = match &result {
                Ok(value) => value.clone(),
                Err(error) => serde_json::json!({"error":error}),
            };
            let text = serde_json::to_string_pretty(&report).unwrap();
            println!("{text}");
            sala::client::diagnostics::flush();
            if let Some(path) = args
                .iter()
                .position(|a| a == "--report")
                .and_then(|i| args.get(i + 1))
                && let Err(error) = std::fs::write(path, text)
            {
                eprintln!("{error}");
                std::process::exit(1);
            }
            if result.is_err() {
                std::process::exit(1);
            }
            return;
        }
        if std::env::args().any(|arg| arg == "--check") {
            sala::client::network::initialize_tls();
            match sala::client::media::check_runtime() {
                Ok(()) => println!("Janja: provedor TLS, bibliotecas e plugins disponíveis."),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            return;
        }
    }
    sala::client::ui::run();
    sala::client::diagnostics::flush();
}
