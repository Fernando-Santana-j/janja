#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    sala::server::run_janja().await
}
