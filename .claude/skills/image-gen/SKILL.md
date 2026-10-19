---
name: image-gen
description: >-
  Generate or edit raster images (PNG/JPG/WebP) from a text description using the
  local OpenAI Codex CLI's built-in image generator — photos, illustrations, icons,
  logos, mascots/characters, hero/OG/social images, favicons, textures, sprites, UI
  mockups, product shots, and transparent-background cutouts, plus edits to an
  existing image (recolor, restyle, add/remove/replace an element, change the
  lighting, remove the background, make variations). Use this WHENEVER the user wants
  to create, generate, make, draw, render, design, paint, or produce an image,
  picture, icon, logo, illustration, graphic, visual, asset, hero image, OG/social
  card, favicon, sprite, texture, mascot, or mockup — even when they don't say
  "Codex", don't name a file format, or just describe the picture they want ("I need
  a header image of …"). Do NOT use for: vector/SVG icon systems or logos meant to be
  authored as code; charts/graphs built from data; diagrams or flowcharts better
  expressed as Mermaid/SVG/HTML; or purely mechanical operations on an existing image
  file — resizing, super-resolution "upscaling", compression/optimization, or format
  conversion such as PNG→ICO — because this skill creates and edits image content, it
  does not transform files.
---

# image-gen

Generates and edits **bitmap** images by driving the locally-installed OpenAI Codex
CLI's built-in `image_gen` tool. It uses your ChatGPT login through Codex — **no
`OPENAI_API_KEY` and no API setup required.**

A bundled engine handles every sharp edge that makes the naive
`codex exec "...save as ./x.png"` approach fail in an automated context. **Always go
through the engine — do not call `codex exec` yourself.**

## The one command

Pick the engine for the current OS and resolve it relative to this skill's directory:

- **Windows** → `scripts/generate.ps1` (PowerShell 7+) — the tested reference implementation.
- **macOS / Linux** → `scripts/generate.sh` (bash) — same logic, **experimental** (see README).

Windows (PowerShell):
```powershell
& "<skill-dir>/scripts/generate.ps1" -Prompt "<structured prompt>" -OutFile "<abs dest>" -Size <size> -Quality <q> -Json
```

macOS / Linux (bash):
```bash
bash "<skill-dir>/scripts/generate.sh" --prompt "<structured prompt>" --out "<abs dest>" --size <size> --quality <q> --json
```

Both print a JSON result; parse `.results[].saved` for the final path(s), then **view the
image inline** with the Read tool so the user sees what was produced.

### Parameters

| PowerShell | bash | Purpose |
| --- | --- | --- |
| `-Prompt` (req) | `--prompt` (req) | The image description. Shape it well — see *Writing the prompt*. |
| `-OutFile` | `--out` | Absolute destination path. Omit for *preview-only* (leaves the file in Codex's `generated_images` and reports the path). On Windows, WSL paths like `/home/you/...` are auto-mapped to `\\wsl.localhost\<distro>\...`. |
| `-Size` | `--size` | `auto` (default) · `1024x1024` square · `1536x1024` landscape · `1024x1536` portrait · `2048x2048` 2K · `2048x1152` widescreen · `3840x2160` 4K landscape · `2160x3840` 4K portrait. |
| `-Quality` | `--quality` | `auto` (default) · `low` (fast drafts/icons) · `medium` · `high` (final assets, dense text, photoreal). |
| `-Transparent` | `--transparent` | Generate on a chroma-key background, then extract a clean alpha PNG locally. For icons/logos/cutouts. Forces `.png`. |
| `-InputImage a,b` | `--input a --input b` | Reference/edit image(s). Attaches them to the request. |
| `-Edit` | `--edit` | With an input image, treats the first as an **edit target** rather than a style reference. |
| `-Variants N` | `--variants N` | Produce N independent takes of the same prompt (saved `name-1.png`, `name-2.png`, …). |
| `-Overwrite` | `--overwrite` | Overwrite the destination instead of writing a `-v2` sibling. |
| `-Json` | `--json` | Machine-readable result. Always pass this so you can parse the saved path. |

A run typically takes **25–70 s**. For slow ones (4K/high quality) give the call a generous
timeout (e.g. 300 s).

## Where to save (save-location policy)

1. **If the user names a path, folder, or filename, use it.** Honor it exactly.
2. **Otherwise infer from the project, then tell the user where it went:**
   - Web project (has `package.json`, or a `public/`, `static/`, or `src/assets/` dir) →
     site-facing images in `public/` (or `public/images/`); component art in `src/assets/`.
     OG/social/hero images → `public/`.
   - No obvious convention → `./images/` at the repo root (the engine creates it).
   - Pure preview / "just show me one" → omit the destination (preview-only) and view inline.
3. Derive a short **kebab-case filename** from the prompt if the user didn't give one
   (e.g. `hero-control-plane.png`). The engine writes a `-v2` sibling rather than clobbering
   an existing file unless you pass overwrite.

## Writing the prompt

Codex applies its own image-gen prompting on top of what you send, so you don't need to
over-engineer it — but a **structured, specific** prompt beats a bare phrase. Use this
lightweight scaffold (include only the lines that matter):

```
Use case: <photorealistic | product-mockup | ui-mockup | infographic | logo-brand | illustration | stylized-concept | …>
Asset type: <where it'll be used — e.g. landing-page hero, blog header, app icon>
Primary request: <the user's core ask>
Style/medium: <photo | flat illustration | 3D | watercolor | line art | …>
Composition/framing: <wide | centered | top-down; note negative space if copy goes over it>
Lighting/mood: <…>
Color palette: <brand colors or neutral>
Text (verbatim): "<exact words, if any text must appear>"
Constraints: <must keep / must avoid — e.g. no logos, no watermark>
```

Principles:
- **Match the user's specificity.** If their prompt is already detailed, just normalize it
  — don't invent extra characters, props, brands, or slogans they didn't ask for.
- For **photorealism**, say so and name real texture (skin pores, fabric wear, grain).
- For **text in the image**, quote it verbatim and bump quality to `high` (text is hard).
- For **edits**, state invariants explicitly: "change only X; keep Y unchanged," and repeat
  them on each iteration. Iterate with one targeted change at a time.

More recipes (per use-case copy/paste specs, transparency details, edit patterns) are in
`references/prompt-recipes.md` — read it for non-trivial requests or a tuned starting point.

## After generating

- Parse the JSON, **Read the saved image to view it**, and tell the user the path.
- If you placed it in a project, wire it into the consuming code/reference if that was the
  intent (e.g. point the component at the new `public/hero.png`).
- If it's not quite right, iterate: re-run with a single targeted change, or pass the result
  back as an input image with `--edit` for a surgical fix. Don't rewrite the whole prompt.

## How it works (so you can debug it)

- The built-in `image_gen` tool always saves to `$CODEX_HOME/generated_images/<session>/`,
  **not** to a path you request. The engine therefore tells Codex to *only generate*, then
  collects the new file (by session id) and places it at the destination itself — sidestepping
  the sandbox, which blocks Codex's own copy attempts.
- The prompt is sent on **stdin**, not as an argument. `codex exec` blocks forever on a
  non-TTY stdin unless it gets EOF, and the `-i` flag is variadic (it would eat a trailing
  positional prompt). Stdin solves both.
- **Transparency** isn't native to the built-in tool: the engine prompts for a flat `#00ff00`
  background and runs Codex's bundled `remove_chroma_key.py` (Pillow) locally to cut a clean
  alpha. For genuinely hard subjects (hair, fur, glass, smoke, soft shadows), warn the user
  that true native transparency needs the API-key CLI path with `gpt-image-1.5` — ask before
  going there; it's out of scope for the default flow.

## Requirements

- OpenAI Codex CLI/app installed and `codex login` completed (built-in image tool uses the
  ChatGPT session — no API key).
- Windows: PowerShell 7+. macOS/Linux: bash + a `codex` on `PATH`.
- Transparency: a Codex version that ships the built-in imagegen skill (provides
  `remove_chroma_key.py`) plus Python with Pillow.
- If `codex` can't be found, the engine prints an install/login hint.
