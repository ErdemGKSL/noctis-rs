//! Binary entry point. Kept thin: logging, configuration, run. Everything else lives
//! in the library so integration tests can reach it.

use std::path::Path;

use noctis::App;
use noctis_bot_template::bootstrap::EnvFile;
use tracing_subscriber::EnvFilter;

#[tokio::main]
#[expect(
    clippy::print_stdout,
    reason = "the first-run notice prints before logging is set up"
)]
async fn main() -> anyhow::Result<()> {
    // Everything the bot needs is compiled into this binary (locales, theme, font,
    // `.env.example`). On first run this writes `.env` with a fresh payload key and
    // stops so the token can be filled in; later runs load it. It runs before logging
    // is set up so `RUST_LOG` from `.env` applies.
    if noctis_bot_template::bootstrap::load_or_create_env(Path::new(".env"))? == EnvFile::Created {
        println!(
            "Created .env from the built-in template. Set NOCTIS_TOKEN in it, then start the bot again."
        );
        return Ok(());
    }

    // `twilight_http=warn`: twilight-http's `req` span carries the request URL, which
    // holds the token of a webhook request.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,twilight_http=warn".into()),
        )
        .init();

    // `App::from_env` reads NOCTIS_* variables (token, payload key, shard range, proxy).
    // `configure` takes the builder by `&mut`; the temporary lives to the end of the statement.
    // `run` stops on Ctrl-C/SIGTERM: shards close, queued events and running handlers get
    // the shutdown grace period (see `configure`), then whatever is left is cancelled and
    // `run` returns `Ok(())`. It returns `RunError::AllShardsEnded` if every shard closed
    // fatally. It also handles the CLI modes, which publish and exit without connecting:
    // --publish, --publish-guild [<id>] (NOCTIS_DEV_GUILD without an id),
    // --publish-clear [--guild <id>], each with optional --dry-run. --no-resume runs the bot
    // with fresh sessions: shards clear their stored sessions at start and close with 1000.
    // --features a,b|all|none picks the Boot Flags (which flagged Modules and registrations
    // exist); `App::from_env` reads it (and NOCTIS_FEATURES) before `build`, because the
    // selection decides what is folded, and `run` rejects a different one.
    let app = noctis_bot_template::configure(&mut App::from_env()?).build()?;
    app.run().await?;
    Ok(())
}
