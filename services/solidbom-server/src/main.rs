//! Apply the development database's migrations and report readiness.
//!
//! The HTTP API arrives in the next task; this binary exists so a fresh checkout can verify its
//! database configuration before the desktop is involved.

use solidbom_server::Store;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("solidbom-server: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let url = match std::env::var("SOLIDBOM_DATABASE_URL") {
        Ok(url) => url,
        Err(_) => return Err(std::io::Error::other("set SOLIDBOM_DATABASE_URL to the development database").into()),
    };
    Store::connect(&url).await?;
    println!("solidbom-server: migrations applied");
    Ok(())
}
