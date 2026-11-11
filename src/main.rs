#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("serve") {
        sala::server::run(None).await
    } else {
        eprintln!(
            "Use janja para abrir o aplicativo ou sala serve para iniciar o servidor configurável."
        );
        Ok(())
    }
}
