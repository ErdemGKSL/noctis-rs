// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Binary entry point. Kept thin: logging, configuration, run. Everything else lives
//! in the library so integration tests can reach it.

use noctis::App;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // `App::from_env` reads NOCTIS_* variables (token, payload key, shard range, proxy).
    // `run` also handles the CLI modes: --publish, --publish-guild <id>, --publish-clear,
    // --dry-run, --features a,b|all, --no-resume. It returns after a graceful shutdown
    // (Ctrl-C / SIGTERM) or when a CLI mode finishes.
    let app = noctis_bot_template::configure(App::from_env()?).build()?;
    app.run().await?;
    Ok(())
}
