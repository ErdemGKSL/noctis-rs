# Guides

Guides for people building and running Noctis bots. Coding agents learn Noctis from the
Usage Skill in the project template
([`examples/project-template/.claude/skills/noctis/`](../../examples/project-template/.claude/skills/noctis/SKILL.md));
these guides link to its references for the details.

- [Coming from tescord, dbi, serenity or poise](concept-map.md): the concept map from
  those frameworks to Noctis, and what Noctis deliberately does not do.
- [Registrations defined anywhere](discovery.md): `module = ".."` on every kind
  attribute, `#[noctis::module]` declarations, `AppBuilder::discover()`, the merge and
  order rules, mixing with `Module::add`, tests and where discovery ends.
- [Cluster deployment](cluster.md): running a bot as several processes that each own a
  Shard Range: the shared `IdentifyQueue`, the HTTP proxy, shared Adapters,
  `NOCTIS_PAYLOAD_KEY`, one publisher and a checklist.
- [Application emojis](application-emojis.md): declaring an emoji set in code, drawn
  icons, how Publish uploads it and the sync policies (replace changed, delete
  undeclared), Clusters (only the publisher writes, the others re-list), test and
  production applications with different ids, and testing.
- [Voice and music bots](voice.md): enabling voice, joining the author's channel,
  tracks, the queue and its events, Opus passthrough and pre-encoding, idle leave,
  Cluster notes, limits, testing with the recording driver, and the `examples/music-bot`
  walkthrough.
- [Releasing Noctis](release.md): the maintainer's runbook for releases: how the
  release PR and the `release` job work, the one-time setup, publishing 0.1.0, later
  releases and troubleshooting.
