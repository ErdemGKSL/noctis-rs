//! Runs the music bot: `NOCTIS_TOKEN=… cargo run --release -p music-bot` (`yt-dlp` on
//! `PATH`).

use noctis::App;

#[tokio::main]
async fn main() -> Result<(), noctis::Error> {
    let mut builder = App::from_env()?;
    music_bot::configure(&mut builder);
    builder.build()?.run().await
}
