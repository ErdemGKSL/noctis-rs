# Views and widgets

Covers: `View`, `StaticView`, `ViewCx`, `Nav`, `Layout` (Chrome + body + overflow),
`Navbar`, `Wizard`, `Paginator`, attachments, mentions, state versions, the UI quality
rulebook and proven layout patterns (schwi-v2). Doctrine: UD-1..UD-7. Rules: CV2-*,
CID-02, MSG-05, ATT-*.

## 1. The View contract

```rust
pub trait View: Send + 'static {
    type Route: Route;                                   // declared #[route("id", view)]
    fn state(&self) -> &Self::Route;                     // re-encoded into every control
    fn render(&self, cx: &mut ViewCx<'_, Self::Route>) -> UiResult<V2Message>;

    // provided [planned]
    fn at(self, nav: Nav) -> Positioned<Self> where Self: Sized;      // start on tab/page/step
    fn with_notice(self, key: MessageKey) -> Positioned<Self> where Self: Sized; // one-line notice on top
    fn mentions(&self) -> AllowedMentions { AllowedMentions::none() } // opt in to pings per View
}
```

- **`render` is pure**: no I/O, no `.await`, deterministic (same state → same tree,
  except select nonces). Load data before, in the handler or a `load` fn taking
  `&impl Ctx`. That makes Views snapshot-testable.
- The View struct holds the Route Payload (small, travels in `custom_id`s) **plus** the
  loaded data (rows, rendered `Attachment`s), which never travel.
- Every View Route needs a **loader** `#[handler]` that rebuilds the View from the
  Payload. Core calls it for widget actions (tab, page, jump, Back, Next) after moving
  `ctx.nav()`, and for `cx.reload_button()`. Cancel is handled by core alone.
- `StaticView` (`fn render(&self, cx: &mut StaticCx<'_>) -> UiResult<V2Message>`) has no
  Route and no controls: final states, logs, webhook posts.

`ViewCx` [planned]: `locale()` (User Locale if the response is ephemeral, else Guild
Locale), `nav() -> Nav`, `limits() -> &Limits`, `attach(&Attachment) -> Media`,
`reload_button() -> Button`, `state() -> &R`.

`Nav { tab: u8, page: u16, step: u8, action: NavAction }`, constructors `Nav::tab(n)`,
`Nav::page(n)`, `Nav::step(n)`. `NavAction::{Author, Tab, Page, Jump, Next, Back}`.

## 2. Layout: Chrome, body, overflow

```rust
Layout::new()
    .accent(rgb!(0x5865_F2))       // Container accent
    .header(TextDisplay or Section)           // Chrome: first
    .navbar(navbar)                    // Chrome: top Action Row (UD-5)
    .wizard(wizard)                    // Chrome: "Step n of m: Title" + Back/Next/Cancel footer (UD-4)
    .body(impl Into<Body>)             // Vec<ContainerComponent> or a Paginator
    .footer(ActionRow)                 // Chrome: last
    .overflow(Overflow::Paginate)      // default; Overflow::Error | Overflow::Truncate opt-in
    .build(cx)                         // -> UiResult<V2Message>
```

`build` reserves the Chrome's cost (components and characters) on every page, then
packs the body against the App's `Limits`. With the default policy an overflowing body
is paginated automatically (FR-62): a list that grows from 10 to 300 items needs no
code change. Only one item that can't fit next to the Chrome is an error
(`UiError::ItemTooLarge { index }`): render that item as an image or a detail View.
`Overflow::Error` returns the FR-24 error (path, limit, actual).

Everything renders inside one Container, `allowed_mentions.parse = []`.

## 3. Navbar (UD-5, FR-26)

```rust
let navbar = Navbar::new(cx, [s.dashboard.tab.overview(), s.dashboard.tab.roles(), s.dashboard.tab.modules()])?;
match navbar.active() { 0 => …, 1 => …, _ => … }
```

- A top Action Row of buttons on the View Route (envelope action `t`). Active tab is
  disabled and Primary; others Secondary. Clicking edits the same message.
- Tab state travels in the `custom_id`: switching tabs stores nothing.
- At most 5 tabs (one row); more is `UiError::TooManyTabs` — use a String Select nav or
  split Views. Optional emoji per tab: `Navbar::new(..)?.emoji(1, Emoji::unicode("👥"))`.
- A tab whose body overflows paginates inside the tab, Navbar on every page.
- Start on a tab from a handler: `view.at(Nav::tab(2))`.

## 4. Wizard (UD-4, FR-27)

```rust
let wizard = Wizard::new(cx, [title0, title1, title2])          // ≤ 25 steps
    .require(0, state.log_channel.is_some(), s.setup.need_channel())
    .require(1, state.mod_role.is_some(), s.setup.need_role());
let step = wizard.step();                                        // effective step
let wizard = wizard.finish(FinishSetup { .. }.button().label(s.setup.finish()));
Layout::new().wizard(wizard).body(body_for(step)).build(cx)
```

- Header shows "Step 2 of 3: Roles" (`noctis.wizard.progress`), never page numbers.
- Footer: Cancel (Secondary) · Back (disabled on Step 1) · Next (Primary). On the last
  Step, Next is replaced by your `finish` button (an author Route); without one, Next is
  hidden.
- `require(step, ok, message)`: pressing Next while `ok` is false stays on that Step and
  shows `message` inline.
- Back keeps earlier values (they live in the Payload).
- Cancel edits the message to a cancelled state (`noctis.wizard.cancelled`), removes
  all components and deletes the Wizard's Refs.
- Step data: author actions (selects, buttons) are their own Routes carrying the
  current state; their handlers return your View, e.g.
  `SetupWizard::new(new_state).at(Nav::step(k))`.
  Inline when it fits; otherwise make the View Route `overflow = spill`.
- Free text in a Step: a button opens a modal; the modal submit returns the Wizard at
  the next Step. Noctis never chains modal → modal; a Step that needs a second modal
  returns to a message Step first, automatically.
- Expired Step Ref: route it to an `on_expired` handler that re-renders Step 1 with a
  notice (see `routes-payloads-refs.md` §7).

## 5. Paginator (UD-6, FR-28)

```rust
Paginator::new(&self.roles, |role| TextDisplay::new(format!("{} · `{}`", role.mention, escape_markdown(&role.name))))
    .empty(TextDisplay::new(s.dashboard.no_roles()))       // designed empty state
    .into()                                         // Body
```

- Items: any `IntoIterator`; the closure returns anything `Into<ContainerComponent>`
  (TextDisplay, Section, …) or `Vec<ContainerComponent>` for multi-part items.
- Packs by **budget** (components + characters), not a fixed count; reserves Chrome.
- Pager row: `⏮ ◀ [n/N] ▶ ⏭` with the indicator inert (action `i`, unique id). ◀
  disabled on page 1, ▶ on the last. With > 5 pages, a jump **String Select** (never a
  modal) reaches any page in bounded clicks.
- Page travels in the `custom_id`; paging stores nothing.
- `.as_select(|item| SelectOption::new(..), PickRole { .. })` [planned] renders items
  as String Selects of 25 options each ("Roles 26–50" placeholders) instead of
  Sections, for picking one of many.
- `.page_size_hint(n)` caps items per page for readability (still budget-packed).
- Data from a database: pass the current page's rows; for huge sets use
  `Paginator::remote(total_count, |page| …)` [planned] with `ctx.nav().page` in the loader.

## 6. Attachments in Views (FR-30, AD-22)

- `Attachment` = `{ name, bytes, content_type, alt }`, named
  `<first 16 hex of blake3(bytes)>.<ext>` (`Attachment::new(bytes, "png")`); rendered
  images are named from the render cache key. `ctx.render(..)` returns one.
- Reference only through `cx.attach(&att) -> AttachmentRef` (Thumbnail, Media Gallery, File;
  see `components-v2.md` §6).
- Same image used twice uploads once. On edit, unchanged names are kept without
  re-upload; unused ones are dropped (MSG-08).
- Upload Budget: the interaction's `attachment_size_limit`, else 20 MiB (ATT-01);
  request ≤ 25 MiB (ATT-02); over budget is a typed error before sending.
- Alt text: required for rendered images in strict mode (≤ 1024, ATT-06).

## 7. Mentions and user text

Views ping nobody by default (`parse: []`, MSG-05) even though Text Display mentions
render. Opt in per View with `fn mentions(&self) -> AllowedMentions {
AllowedMentions::new().users(Users::Only(vec![id])) }` (`Users::All | Users::Only`,
≤ 100 ids, MSG-03/04). Escape user-supplied text: `escape_markdown`, `escape_mentions`.

## 8. State versions (FR-25)

For shared, frequently edited Views (two admins clicking one public dashboard):
`#[route("dash", view)]` + `impl View { const STATE_VERSION: bool = true; fn
state_version(&self) -> u8 }` [planned]. A click carrying an older version re-renders
from current state with a short notice instead of applying a stale action.

## 9. Cost without sending

`view.cost(&limits)` [planned] → `Cost { components, text_chars, pages }`; the testkit
reports it per response. Use it in tests to keep dashboards comfortably under 40/4000.

## 10. UI quality rulebook

1. **One primary action per View.** One Primary/Success button; everything else Secondary.
2. **Visible active state.** Disabled active tab, `default` on the current select
   option/value, state text next to toggles ("**Profiles** · enabled").
3. **Confirm destructive actions.** Danger button → small confirm View ("Delete 3
   roles? This can't be undone.") with Cancel (Secondary) and Confirm (Danger) on a
   `by_ref, one_shot, ttl = "15m"` Route. Never delete on first click.
4. **Ephemeral for single-user UI, public for shared content.** Settings, wizards,
   errors, per-user panels → ephemeral (User Locale). Dashboards, cards, leaderboards
   others should see → public (Guild Locale).
5. **A way back or out.** Every interactive View has Back, Cancel, a Navbar, or a
   "Back to …" button re-opening the parent View Route. Drill-down state is just ids
   in Payloads, no navigation stack.
6. **Designed empty and error states.** `Paginator::empty(..)`, "No bio yet." in
   subtext, a disabled placeholder select; errors are one localized line, never a
   stack trace or a missing block.
7. **Hierarchy with Markdown, not components.** `##` title, `-#` subtext, `<t:…:R>`
   timestamps; merge static text into one Text Display.
8. **Short labels.** ≤ 34 chars with emoji; mobile wraps rows past ~3 labelled buttons.
9. **Images are read-only.** Wide visuals in Media Gallery; small icons in Thumbnail;
   never instructions inside an image; always alt text.
10. **Accent colour as state.** Neutral in progress, green complete, red error. Test on
    light and dark themes (clients don't expose theme).

## 11. Proven layouts (from schwi-v2)

**Settings panel** (re-editable config, one Container):
1. Header: `# Title` TextDisplay (or Section + Thumbnail module icon).
2. Entity switcher, only if > 1 entity: caption + String Select with `default` on the
   current entity.
3. Field blocks: caption TextDisplay + Action Row with one select showing the current value
   (`default_channels(current)`).
4. Scalar settings as Sections: "Join delay (30s)" + accessory "Edit" (Secondary when
   set, Primary "Set" when unset — draws attention to missing config).
5. Toggles: style encodes the action (Enable = Success, Disable = Danger).
6. Management at the bottom: "Add new" (Primary), "Delete" (Danger → confirm).
7. Separators: large between blocks, small inside.

**Drill-down**: a select lists children; choosing one returns the child's View; the
child has "Back" built from the parent's View Route.

**Ephemeral sub-panel from a public message**: a button with
`#[handler(response = ephemeral)]` returns a private View whose Routes carry the parent
`message_id`; its handlers may refresh the parent with a bot-token edit
(`ctx.edit_message(channel, message_id, view)` [planned]).

**Same handler, different data**: Prev/Next or per-row buttons share one Route and
differ only in Payload — ids stay unique.

**Wizard vs settings panel**: first-time linear setup → Wizard; later edits → settings
panel. Many bots offer both.

## 12. Example: the template's dashboard

See `src/modules/dashboard.rs`: command → `DashboardView::load` → Navbar (Overview,
Roles, Modules) → Roles tab is a Paginator over all roles → Modules tab has Section
toggles → footer reload button. Its loader `open_dashboard` serves every widget click.
