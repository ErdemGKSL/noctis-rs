#!/usr/bin/env bash
# generate.sh — bash engine for the image-gen skill (macOS / Linux).
#
# EXPERIMENTAL: this mirrors the tested PowerShell engine (generate.ps1) but has NOT been
# verified on macOS/Linux by the author. Please test before relying on it, and open an issue
# or PR if something is off. The PowerShell engine is the reference implementation.
#
# Why this exists (same rationale as the PS engine):
#   1. `codex exec` blocks forever on a non-TTY stdin ("Reading additional input from
#      stdin...") unless it gets EOF. We send the prompt ON stdin, which delivers prompt+EOF
#      in one move. (Also: `-i/--image` is variadic and would eat a trailing positional prompt.)
#   2. The built-in image_gen tool always saves to $CODEX_HOME/generated_images/<session>/,
#      never to a requested path, and Codex's own copy-out is sandbox-blocked. So we tell
#      Codex to ONLY generate, then collect the file ourselves and place it.
#   3. Transparency isn't native: we prompt for a flat chroma-key background and run Codex's
#      bundled remove_chroma_key.py (Pillow) locally to cut a clean alpha.
#
# Built-in tool mode uses your ChatGPT login — no OPENAI_API_KEY needed.
set -euo pipefail

PROMPT=""; OUT=""; SIZE="auto"; QUALITY="auto"; TRANSPARENT=0
EDIT=0; VARIANTS=1; OVERWRITE=0; JSON=0; WORKDIR=""; TIMEOUT=240
declare -a INPUTS=()

die() {
  if [ "$JSON" -eq 1 ]; then printf '{"ok":false,"error":%s}\n' "$(json_str "$1")"; else echo "ERROR: $1" >&2; fi
  exit "${2:-1}"
}
# Minimal JSON string escaper (handles backslash, quote, control chars enough for paths/msgs).
json_str() { local s=${1//\\/\\\\}; s=${s//\"/\\\"}; s=${s//$'\n'/\\n}; s=${s//$'\t'/\\t}; printf '"%s"' "$s"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --prompt)      PROMPT="$2"; shift 2 ;;
    --out)         OUT="$2"; shift 2 ;;
    --size)        SIZE="$2"; shift 2 ;;
    --quality)     QUALITY="$2"; shift 2 ;;
    --transparent) TRANSPARENT=1; shift ;;
    --input)       INPUTS+=("$2"); shift 2 ;;
    --edit)        EDIT=1; shift ;;
    --variants)    VARIANTS="$2"; shift 2 ;;
    --overwrite)   OVERWRITE=1; shift ;;
    --work-dir)    WORKDIR="$2"; shift 2 ;;
    --timeout)     TIMEOUT="$2"; shift 2 ;;
    --json)        JSON=1; shift ;;
    *) die "Unknown argument: $1" ;;
  esac
done
[ -n "$PROMPT" ] || die "--prompt is required"

# --- locate codex ---
CODEX="$(command -v codex || true)"
if [ -z "$CODEX" ]; then
  for c in "$HOME/.local/bin/codex" "/usr/local/bin/codex" "/opt/homebrew/bin/codex"; do
    [ -x "$c" ] && CODEX="$c" && break
  done
fi
[ -n "$CODEX" ] || die "Could not find 'codex' on PATH. Install the OpenAI Codex CLI (e.g. 'npm i -g @openai/codex' or 'brew install --cask codex'), then run 'codex login'."

CODEX_HOME="${CODEX_HOME:-$HOME/.codex}"
GENDIR="$CODEX_HOME/generated_images"
if [ -z "$WORKDIR" ]; then WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/imagegen-XXXXXX")"; fi
mkdir -p "$WORKDIR"

# --- validate input images ---
for img in "${INPUTS[@]:-}"; do
  [ -z "$img" ] && continue
  [ -f "$img" ] || die "Input image not found: $img"
done

read -r -d '' CHROMA <<'EOF' || true
Render the subject on a perfectly flat solid #00ff00 chroma-key background for background removal.
The background must be one uniform color with no shadows, gradients, texture, reflections, floor plane, or lighting variation.
Keep the subject fully separated from the background with crisp edges and generous padding.
Do not use #00ff00 anywhere in the subject. No cast shadow, no contact shadow, no reflection, no watermark, and no text unless explicitly requested.
EOF

build_prompt() {
  local p=""
  if [ "${#INPUTS[@]}" -gt 0 ] && [ -n "${INPUTS[0]:-}" ]; then
    if [ "$EDIT" -eq 1 ]; then
      p+="Edit the attached image (Image 1). Apply only the requested change and keep everything else unchanged."$'\n'
    else
      p+="Use the attached image(s) as visual reference (style / composition / subject guidance), not as an edit target."$'\n'
    fi
  fi
  p+="$PROMPT"$'\n'
  [ "$SIZE" != "auto" ] && p+="Target output size: $SIZE."$'\n'
  [ "$QUALITY" != "auto" ] && p+="Quality: $QUALITY."$'\n'
  [ "$TRANSPARENT" -eq 1 ] && p+="$CHROMA"$'\n'
  p+=$'\n'"Use your built-in image generation tool to create this image. After it is generated, STOP. Do not move, copy, rename, save, or relocate the file, and do not run any shell command to find it. Leave it in your default generated-images directory; it will be collected automatically."
  printf '%s' "$p"
}

# choose a timeout wrapper if available (GNU coreutils 'timeout' or macOS 'gtimeout')
TIMEOUT_BIN=""
command -v timeout  >/dev/null 2>&1 && TIMEOUT_BIN="timeout"
[ -z "$TIMEOUT_BIN" ] && command -v gtimeout >/dev/null 2>&1 && TIMEOUT_BIN="gtimeout"

free_name() {  # echo a non-clobbering path
  local path="$1"
  if [ "$OVERWRITE" -eq 1 ] || [ ! -e "$path" ]; then printf '%s' "$path"; return; fi
  local dir base ext i; dir="$(dirname "$path")"; base="$(basename "$path")"
  ext=""; [[ "$base" == *.* ]] && ext=".${base##*.}" && base="${base%.*}"
  for i in $(seq 2 999); do
    local cand="$dir/${base}-v${i}${ext}"
    [ ! -e "$cand" ] && printf '%s' "$cand" && return
  done
  printf '%s' "$path"
}

FINAL_PROMPT="$(build_prompt)"
RESULTS_JSON=""; COUNT=0

for ((v=1; v<=VARIANTS; v++)); do
  MARKER="$WORKDIR/.marker.$v"; : > "$MARKER"; sleep 1
  declare -a ARGS=(exec -C "$WORKDIR" -s workspace-write --skip-git-repo-check -c model_reasoning_effort=low)
  # -i is variadic -> must come last
  if [ "${#INPUTS[@]}" -gt 0 ] && [ -n "${INPUTS[0]:-}" ]; then ARGS+=(-i "${INPUTS[@]}"); fi

  start=$(date +%s)
  set +e
  if [ -n "$TIMEOUT_BIN" ]; then
    OUTPUT="$(printf '%s' "$FINAL_PROMPT" | "$TIMEOUT_BIN" "${TIMEOUT}s" "$CODEX" "${ARGS[@]}" 2>&1)"
  else
    OUTPUT="$(printf '%s' "$FINAL_PROMPT" | "$CODEX" "${ARGS[@]}" 2>&1)"
  fi
  rc=$?
  set -e
  elapsed=$(( $(date +%s) - start ))
  [ "$rc" -eq 124 ] && die "Codex timed out after ${TIMEOUT}s on variant $v."

  SID="$(printf '%s' "$OUTPUT" | grep -oiE 'session id:[[:space:]]*[0-9a-f-]+' | head -n1 | grep -oiE '[0-9a-f-]{8,}' | head -n1 || true)"

  FOUND=""
  if [ -n "$SID" ] && [ -d "$GENDIR/$SID" ]; then
    FOUND="$(ls -t "$GENDIR/$SID"/*.png "$GENDIR/$SID"/*.webp "$GENDIR/$SID"/*.jpg "$GENDIR/$SID"/*.jpeg 2>/dev/null | head -n1 || true)"
  fi
  if [ -z "$FOUND" ]; then
    FOUND="$(find "$GENDIR" -type f \( -iname '*.png' -o -iname '*.webp' -o -iname '*.jpg' -o -iname '*.jpeg' \) -newer "$MARKER" 2>/dev/null | head -n1 || true)"
  fi
  [ -n "$FOUND" ] || die "Codex ran but no new image was found in $GENDIR. Last output: $(printf '%s' "$OUTPUT" | tail -n 8)"

  SAVED="$FOUND"; PLACED=false
  if [ -n "$OUT" ]; then
    DEST="$OUT"
    [ "$TRANSPARENT" -eq 1 ] && DEST="${DEST%.*}.png"
    if [ "$VARIANTS" -gt 1 ]; then
      d="$(dirname "$DEST")"; b="$(basename "$DEST")"; e=""; [[ "$b" == *.* ]] && e=".${b##*.}" && b="${b%.*}"
      DEST="$d/${b}-${v}${e}"
    fi
    mkdir -p "$(dirname "$DEST")"
    DEST="$(free_name "$DEST")"
    if [ "$TRANSPARENT" -eq 1 ]; then
      CHROMA_PY="$CODEX_HOME/skills/.system/imagegen/scripts/remove_chroma_key.py"
      [ -f "$CHROMA_PY" ] || die "Transparency helper not found at $CHROMA_PY"
      python3 "$CHROMA_PY" --input "$FOUND" --out "$DEST" --auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill >/dev/null 2>&1 || die "Chroma-key removal failed"
      [ -f "$DEST" ] || die "Chroma-key removal produced no output at $DEST"
    else
      cp -f "$FOUND" "$DEST"
    fi
    SAVED="$DEST"; PLACED=true
  fi

  COUNT=$((COUNT+1))
  item="{\"saved\":$(json_str "$SAVED"),\"source\":$(json_str "$FOUND"),\"sessionId\":$(json_str "${SID:-}"),\"elapsedSec\":$elapsed,\"placed\":$PLACED}"
  RESULTS_JSON="${RESULTS_JSON:+$RESULTS_JSON,}$item"
  [ "$JSON" -eq 0 ] && echo "OK  $SAVED  (${elapsed}s$( [ "$PLACED" = false ] && echo ', preview-only in generated_images'))"
done

if [ "$JSON" -eq 1 ]; then
  printf '{"ok":true,"count":%d,"results":[%s]}\n' "$COUNT" "$RESULTS_JSON"
fi
