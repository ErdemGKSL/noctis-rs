# Concept map: other frameworks → Noctis

How to translate what you know from tescord, dbi (`@mostfeatured/dbi`), serenity and
poise into Noctis. Each row names the Noctis item and the reference that documents it.
Behaviour Noctis deliberately does not offer (collectors, inline listeners, framework
widgets) is listed with what to write instead.

## tescord / dbi → Noctis

| tescord / dbi | Noctis |
| --- | --- |
| `Tescord` root, `Pack` | `App`, `Module` (`Module::new("a/b")`, nested with `.module(..)`) |
| `pack.slashCommand({ name: "a (b\|c)", handle })` | `#[derive(SlashCommand)] #[command("a (b\|c)")] struct A;` + `impl A { #[handler] async fn handle(self, ctx, path: APath) }` |
| `options: { user: { type: "User" } }` | fields of the command struct: `struct Ban { #[option(..)] user: ResolvedUser }` |
| `autoComplete` | `#[option(.., autocomplete)]` + an `#[autocomplete(field)] async fn x(ctx, typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<T>>` method on the command struct; Noctis sends the result (`commands.md` §4) |
| `pack.button({ id, options, handle })` + `buildComponent({ id, data, overrides })` | `#[derive(Button)] #[button("id", defaults(..))] struct X {..}` + `impl X { #[handler] async fn handle(self, ctx) }` + `X { .. }.build().label(..)` |
| `pack.stringSelect` / `userSelect` / … / `modal` | `#[derive(StringSelectMenu)]`, `UserSelectMenu`, `RoleSelectMenu`, `MentionableSelectMenu`, `ChannelSelectMenu`, `Modal` |
| `pack.event({ name, handle })` | `#[derive(EventHandler)] #[event] struct X;` + `impl X { #[handler] async fn handle(self, ctx, event: MessageCreate) }` |
| lifecycle listeners (`ready`, shard state, shutdown) | `#[derive(Hook)] struct X;` + `impl X { #[handler] async fn handle(self, ctx: HookCtx, event: AllShardsReady) }` (`noctis::lifecycle`; `references/app-and-sharding.md` §7) |
| `renderSetupMenu(..)` + `deferUpdate()` + `editReply({ components, flags })` | `render_x(..) -> V2Message` + `ctx.update(render_x(..))` (flags and deferral are automatic) |
| `data[i] as number` | typed Payload fields |
| dbi references / store | `by_ref` Routes + `RefStore` |
| `locale.user.x.y(a)` / `locale.guild…` | `Strings::of(ctx.user_locale()).x.y(a)` / `ctx.guild_locale()` |
| `*.interactions.yaml` `names:` | `cmdword.<word>` and `cmd.<path>.*` keys in the same locale files |
| `beforeInteraction` returning `false`/object | Middleware returning `Flow::Stop` / inserting an `Ext<T>` |
| `--publish [--guildId]` | `--publish`, `--publish-guild [<id>]`, `--publish-clear`, `--dry-run`; in code `app.publish(PublishScope::..)`; `--no-resume` (run without resuming, fresh shutdown) |
| `--features a,b` | Boot Flags: `Module::flags([..])` / `flags = [..]` on a kind attribute, selected with `--features a,b\|all\|none` or `NOCTIS_FEATURES` (read by `App::from_env()`) or `.boot_flags(BootFlags)` |
| inline listeners | not supported (UD-7): Route + `one_shot` Ref |
| dbi `dbi.register(({ ChatInput, Button, Event, Locale }) => { .. })` | one Module per feature: `Module::new("x").add::<Cmd>().add::<Btn>()`, mounted with `AppBuilder::module` (`references/modules-middleware-ratelimits.md` §1) |
| dbi `ChatInput({ name, description, onExecute })` | `#[derive(SlashCommand)] #[command("name")]`; the description comes from `cmd.<path>.description` in `locales/en-US.yaml` (`references/commands.md` §1) |
| dbi `Button({ name, onExecute })` + `button.toJSON({ reference: { data: [..] } })` | `#[derive(Button)]` struct whose fields are the Payload; `X { .. }.build()` (`references/routes-payloads-refs.md` §1) |
| dbi references (`ref` objects kept in memory) | `by_ref` Routes, stored in the `RefStore` port with a TTL (`references/routes-payloads-refs.md` §4) |
| dbi `InteractionLocale` / `Locale` | `locales/*.yaml` compiled by `i18n!`, read with `Strings::of(locale)` (`references/i18n.md`) |
| dbi `rateLimits: [{ type: "User", duration }]` | `rate_limit(user, n, per = "10s")` on the kind attribute (`references/modules-middleware-ratelimits.md` §7) |
| dbi `dbi.publish("Guild", id)` | `cargo run --release -- --publish-guild <id>` or `app.publish(PublishScope::..)` (`references/commands.md` §6) |
| dbi `CustomEvent` / `dbi.emit(..)` | plugin events through `Emitter` and `ExtensionEvent` (`references/app-and-sharding.md` §7) |
| tescord or dbi in-memory `Map` stores | `ctx.kv()` (the `Kv` store over `CacheStore`) or a custom Adapter (`references/adapters.md` §3) |

## serenity / poise → Noctis

| serenity / poise | Noctis |
| --- | --- |
| `serenity::Client::builder(token, intents).event_handler(H)` | `App::builder().token(..)` (or `App::from_env()`), Modules with `#[derive(EventHandler)]` structs; intents are derived from the handlers (`references/app-and-sharding.md` §1) |
| `impl EventHandler for H { async fn message(&self, ctx, msg) }` | `#[derive(EventHandler)] #[event] struct OnMessage;` + `impl OnMessage { #[handler] async fn handle(self, ctx: EventCtx, event: MessageCreate) -> HandlerResult<()> }` |
| `#[poise::command(slash_command)] async fn ping(ctx: Context<'_>) -> Result<(), Error>` | `#[derive(SlashCommand)] #[command("ping")] struct Ping;` + `impl Ping { #[handler] async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> }` |
| poise function arguments as options (`#[description = ".."] user: serenity::User`) | fields of the command struct (`user: ResolvedUser`); descriptions in `cmd.<path>.opt.<name>.description` (`references/commands.md` §3) |
| `#[autocomplete = "f"]` | `#[option(.., autocomplete)]` + `#[autocomplete(field)]` method returning `Suggestions<T>` (`references/commands.md` §4) |
| `ctx.say(..)` / `CreateReply::default().embed(..)` | `ctx.respond(render_x(..))` with a Components V2 View, or `ctx.reply_text(key)` for a notice; no embeds (UD-1) |
| `ctx.defer()` | automatic: the auto-defer acknowledges slow handlers (`references/responder.md` §2) |
| `ComponentInteractionCollector` / `await_component_interaction` | not supported (UD-7): one `#[handler]` per component struct, state in the Payload or a Ref |
| `poise::builtins::register_in_guild` / `register_globally` | Publish: `--publish-guild [<id>]` / `--publish` (never on READY) |
| `poise::Framework::builder().setup(..)` user data (`Data`) | `AppBuilder::state(..)`, read in a handler with `ctx.state::<T>()` (an `Option<State<T>>`), or a Middleware `Ext<T>` per run |
| `poise` checks (`check = "f"`, `required_permissions`) | `permissions = ..` on `#[command]`, Route Guards (`owner`, `permissions`, `guard`) and Middleware (`references/modules-middleware-ratelimits.md` §5) |
| `poise` cooldowns | `rate_limit(..)` on the kind attribute |
| `on_error` in `FrameworkOptions` | Error Sinks (`.error_sink(ErrorOrigin::Command, ..)`) and `HandlerError::user(key)` |
| `serenity::cache::Cache` | `ctx.cache()` (`DiscordCache` port, owned values; `references/adapters.md`) |
| `ShardManager` | built-in sharding; Clusters through `NOCTIS_SHARD_RANGE` (`references/app-and-sharding.md` §4) |
| `songbird` setup | the `voice` feature and `ctx.voice()` (`references/voice.md`) |
