# claude-codex-imagegen

A [Claude Code](https://claude.com/claude-code) **skill** that generates and edits raster
images (PNG/JPG/WebP) from a text description by driving the locally-installed
[OpenAI Codex](https://developers.openai.com/codex/cli) CLI's built-in `image_gen` tool.

It uses your **ChatGPT login** through Codex — **no `OPENAI_API_KEY` and no API billing.**
Claude Code can't generate bitmaps itself; this skill lets it hand the job to Codex, then
collects the result and drops it exactly where you want it.

<p align="center">
  <img src="docs/example-fox.png" width="32%" alt="watercolor fox (generation)">
  <img src="docs/example-rocket-transparent.png" width="32%" alt="rocket icon (transparent background)">
  <img src="docs/example-logo.png" width="32%" alt="mountain logo (generation)">
</p>

> Real outputs from the skill: a watercolor illustration, a transparent-background icon, and
> a flat logo mark — all generated end-to-end via `codex exec`.

## Why a skill instead of `codex exec "...save as ./x.png"`?

The one-liner from blog posts works when you type it into an interactive terminal, but it
**fails when an agent runs it.** This skill's engine fixes three real traps (discovered the
hard way — the naive command hung for 21 minutes in testing):

1. **stdin deadlock.** `codex exec` blocks forever on a non-TTY stdin
   (`Reading additional input from stdin...`). The engine sends the prompt *on stdin*, which
   delivers prompt + EOF in one move. (Bonus: the `-i/--image` flag is variadic and would
   otherwise swallow a trailing positional prompt.)
2. **It ignores your save path.** The built-in `image_gen` tool always writes to
   `$CODEX_HOME/generated_images/<session>/`, and Codex's own attempt to copy the file out
   is blocked by the sandbox. The engine tells Codex to *only generate*, then collects the
   new file by session id and places it at the destination itself.
3. **No native transparency.** The engine generates on a flat `#00ff00` chroma-key
   background and runs Codex's bundled `remove_chroma_key.py` (Pillow) locally to cut a
   clean alpha channel.

Inspired by Paul Kuo's write-up,
[Claude Code + Codex image generation](https://paulkuo.tw/en/articles/claude-code-codex-imagegen/),
and hardened for unattended/agent use.

## Requirements

- The [OpenAI Codex CLI/app](https://developers.openai.com/codex/cli) installed, with
  `codex login` completed (the built-in image tool uses your ChatGPT session).
- **Windows:** PowerShell 7+ (`pwsh`). **macOS/Linux:** `bash` and a `codex` on `PATH`.
- **Transparency** additionally needs a Codex version that ships the built-in imagegen skill
  (it provides `remove_chroma_key.py`) plus **Python with Pillow** (`pip install pillow`).

## Install

Clone into your Claude Code skills directory (the skill's `name` is `image-gen`):

```bash
git clone https://github.com/RamazanKara/claude-codex-imagegen.git ~/.claude/skills/image-gen
```

Restart Claude Code (or start a new session) so it picks up the skill. Then just ask
naturally — *"make a hero image for the homepage"*, *"generate a transparent favicon of a
shield"*, *"recolor this logo to navy"* — and the skill triggers. It infers a sensible save
location (a web project → `public/`) or honors a path you name.

## Direct usage

You can also call the engine directly.

**Windows (PowerShell):**
```powershell
& "~/.claude/skills/image-gen/scripts/generate.ps1" `
  -Prompt "A minimal flat-design mountain logo, teal and navy, on white, no text" `
  -OutFile "$PWD/public/logo.png" -Size 1024x1024 -Quality medium -Json
```

**macOS / Linux (bash, experimental):**
```bash
bash ~/.claude/skills/image-gen/scripts/generate.sh \
  --prompt "A minimal flat-design mountain logo, teal and navy, on white, no text" \
  --out "$PWD/public/logo.png" --size 1024x1024 --quality medium --json
```

Both print a JSON result; read `.results[].saved` for the final path.

### Parameters

| PowerShell | bash | Purpose |
| --- | --- | --- |
| `-Prompt` (req) | `--prompt` (req) | The image description. |
| `-OutFile` | `--out` | Destination path (absolute recommended). Omit for *preview-only* (file stays in `generated_images`, path is reported). On Windows, WSL paths like `/home/you/...` are auto-mapped to `\\wsl.localhost\<distro>\...`. |
| `-Size` | `--size` | `auto` · `1024x1024` · `1536x1024` · `1024x1536` · `2048x2048` · `2048x1152` · `3840x2160` · `2160x3840`. |
| `-Quality` | `--quality` | `auto` · `low` · `medium` · `high`. |
| `-Transparent` | `--transparent` | Chroma-key generate → local alpha extraction. Forces `.png`. |
| `-InputImage a,b` | `--input a --input b` | Reference/edit image(s). |
| `-Edit` | `--edit` | Treat the first input image as an edit target (not a style reference). |
| `-Variants N` | `--variants N` | N independent takes of the same prompt. |
| `-Overwrite` | `--overwrite` | Overwrite the destination instead of writing a `-v2` sibling. |
| `-Json` | `--json` | Machine-readable result. |

See [`references/prompt-recipes.md`](references/prompt-recipes.md) for copy/paste prompt
specs per use-case, a size/quality cheat sheet, and edit/transparency patterns.

## Platform support

| Engine | Platform | Status |
| --- | --- | --- |
| `scripts/generate.ps1` | Windows (+ WSL paths) | ✅ Tested reference implementation |
| `scripts/generate.sh` | macOS / Linux | ⚠️ Experimental — mirrors the PS logic but not yet verified by the author. Test before relying on it; issues/PRs welcome. |

## License

[MIT](LICENSE) © Ramazan Kara
