# Guides

Guides for people building and running Noctis bots. Coding agents learn Noctis from the
Usage Skill in the project template
([`examples/project-template/.claude/skills/noctis/`](../../examples/project-template/.claude/skills/noctis/SKILL.md));
these guides link to its references for the details.

- [Coming from tescord, dbi, serenity or poise](concept-map.md): the concept map from
  those frameworks to Noctis, and what Noctis deliberately does not do.
- [Cluster deployment](cluster.md): running a bot as several processes that each own a
  Shard Range: the shared `IdentifyQueue`, the HTTP proxy, shared Adapters,
  `NOCTIS_PAYLOAD_KEY`, one publisher and a checklist.
- [Voice and music bots](voice.md): enabling voice, joining the author's channel,
  tracks, the queue and its events, Opus passthrough and pre-encoding, idle leave,
  Cluster notes, limits, testing with the recording driver, and the `examples/music-bot`
  walkthrough.
