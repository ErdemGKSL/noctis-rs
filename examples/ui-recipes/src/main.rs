//! Runs the recipes bot: `NOCTIS_TOKEN=… cargo run --release -p ui-recipes`.

use noctis::App;
use ui_recipes::store::Data;

#[tokio::main]
async fn main() -> Result<(), noctis::Error> {
    let mut builder = App::from_env()?;
    ui_recipes::configure(&mut builder, Data::sample());
    builder.build()?.run().await
}
