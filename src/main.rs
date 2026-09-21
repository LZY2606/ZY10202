use std::sync::Arc;

use clap::Parser;
use edge_mtf_bench::{all_fixtures, build_router, db::Db};
use tokio::net::TcpListener;

/// 刃缘解像台 - slanted-edge ESF/LSF/MTF analysis bench
#[derive(Parser, Debug)]
#[command(name = "edge_mtf_bench", version)]
struct Args {
    /// listen address, e.g. 127.0.0.1:5542
    #[arg(long, default_value = "127.0.0.1:5542")]
    listen: String,
    /// SQLite database file
    #[arg(long, default_value = "edge_bench.db")]
    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let db = Arc::new(Db::open(&args.db)?);
    db.seed_fixtures(&all_fixtures())?;
    db.log("start", &format!("listen {}", args.listen));
    let app = build_router(db);
    let listener = TcpListener::bind(&args.listen).await?;
    println!("刃缘解像台 listening on http://{}", args.listen);
    axum::serve(listener, app).await?;
    Ok(())
}
