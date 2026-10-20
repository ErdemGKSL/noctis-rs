// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Binary entry point. Kept thin: logging, configuration, run. Everything else lives
//! in the library so integration tests can reach it.

use std::path::Path;

use noctis::App;
use noctis_bot_template::bootstrap::EnvFile;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Everything the bot needs is compiled into this binary (locales, theme, font,
    // `.env.example`). On first run this writes `.env` with a fresh payload key and
    // stops so the token can be filled in; later runs load it. It runs before logging
    // is set up so `RUST_LOG` from `.env` applies.
    if noctis_bot_template::bootstrap::load_or_create_env(Path::new(".env"))? == EnvFile::Created {
        println!("Created .env from the built-in template. Set NOCTIS_TOKEN in it, then start the bot again.");
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // `App::from_env` reads NOCTIS_* variables (token, payload key, shard range, proxy).
    // `configure` takes the builder by `&mut`; the temporary lives to the end of the statement.
    // `run` also handles the CLI modes: --publish, --publish-guild <id>, --publish-clear,
    // --dry-run, --features a,b|all, --no-resume. It returns after a graceful shutdown
    // (Ctrl-C / SIGTERM) or when a CLI mode finishes.
    let app = noctis_bot_template::configure(&mut App::from_env()?).build()?;
    app.run().await?;
    Ok(())
}
