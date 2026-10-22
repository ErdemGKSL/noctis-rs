// NOTE: the bot code in `src/` is still written against the planned Noctis API
// (architecture spine); this file uses the real `noctis::testkit` API (Story 8.4).

//! NFR-2 / UD-7: framework memory stays flat however many dashboards are opened.
//!
//! `assert_memory_flat` reads the process-wide heap through the dhat allocator, so this
//! binary installs it and holds exactly one `#[test]` with its own multi-thread runtime.

use noctis::prelude::*;
use noctis::testkit::{TestApp, TestGuild, TestUser, assert_memory_flat};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{GuildMarker, UserMarker};
use noctis_bot_template::configure;

#[global_allocator]
static ALLOC: noctis::testkit::dhat::Alloc = noctis::testkit::dhat::Alloc;

const GUILD: Id<GuildMarker> = Id::new(100);
const ADMIN: Id<UserMarker> = Id::new(200);

#[test]
fn handlers_hold_no_per_message_state() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(async {
        let app = TestApp::build(configure(&mut App::builder()))
            .await?
            .guild(TestGuild::new(GUILD, "Test Guild"))
            .user(TestUser::new(ADMIN).permissions(noctis::Permissions::MANAGE_GUILD));
        let report = assert_memory_flat(&app, 1_000, |app| async move {
            app.command("dashboard").by(ADMIN).in_guild(GUILD).run().await.map(drop)
        })
        .await?;
        assert_eq!(report.runs, 1_000);
        app.stop().await?;
        Ok(())
    })
}
