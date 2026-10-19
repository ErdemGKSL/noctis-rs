#requires -Version 7
<#
.SYNOPSIS
  Generate or edit a raster image with the local OpenAI Codex CLI's built-in image_gen
  tool, then collect the result and place it where you want it.

.DESCRIPTION
  This is the deterministic engine behind the `image-gen` skill. It exists because the
  naive `codex exec "...save it as ./x.png"` from blog posts is fragile when automated:

    1. `codex exec` BLOCKS on stdin when stdin is not a TTY (any scripted/agent context).
       It prints "Reading additional input from stdin..." and hangs forever. We feed it an
       empty stdin ($null | ...) so it gets immediate EOF and proceeds.
    2. The built-in image_gen tool always saves to $CODEX_HOME\generated_images\<session>\,
       NOT to a path you ask for. Codex is *supposed* to copy it out afterward, but that
       copy is a sandboxed shell command that gets blocked under -s workspace-write. So we
       do NOT ask Codex to save anywhere -- we collect the file ourselves (this script has
       full filesystem access) and place it at -OutFile.
    3. Built-in mode can't do true alpha. For -Transparent we ask Codex for a flat
       chroma-key background and then run Codex's own remove_chroma_key.py locally.

  Built-in tool mode uses the user's ChatGPT login -- no OPENAI_API_KEY needed.

.NOTES
  Emit -Json for a machine-readable result the caller can parse for the saved path(s).
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)] [string]   $Prompt,
  [string]   $OutFile,                                   # destination: Windows path, \\wsl.localhost\... UNC, or /home/... WSL path
  [string]   $Size = "auto",                             # auto | 1024x1024 | 1536x1024 | 1024x1536 | 2048x2048 | 2048x1152 | 3840x2160 | 2160x3840
  [ValidateSet("low", "medium", "high", "auto")] [string] $Quality = "auto",
  [switch]   $Transparent,                               # generate on chroma key, then extract alpha locally
  [string[]] $InputImage = @(),                          # reference image(s) or edit target -> codex -i
  [switch]   $Edit,                                      # treat InputImage[0] as an edit target (vs style reference)
  [int]      $Variants = 1,                              # number of independent generations of this prompt
  [switch]   $Overwrite,                                 # overwrite OutFile instead of writing a -v2 sibling
  [string]   $WorkDir,                                   # codex -C working root (default: a fresh temp dir)
  [string]   $WslDistro = "ubuntu",                      # used to map /home/... OutFile -> \\wsl.localhost\<distro>\...
  [int]      $TimeoutSec = 240,                          # per-generation timeout
  [switch]   $Json
)

$ErrorActionPreference = 'Stop'

function Fail([string]$msg, [int]$code = 1) {
  if ($Json) { [pscustomobject]@{ ok = $false; error = $msg } | ConvertTo-Json -Compress | Write-Output }
  else { Write-Error $msg }
  exit $code
}

# --- locate codex.exe (content-addressed bin dir; the hash rotates on update) ---
function Resolve-Codex {
  $cmd = Get-Command codex -ErrorAction SilentlyContinue
  if ($cmd) { return $cmd.Source }
  $candidates = Get-ChildItem "$env:LOCALAPPDATA\OpenAI\Codex\bin\*\codex.exe" -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending
  if ($candidates) { return $candidates[0].FullName }
  Fail "Could not find codex.exe. Install the OpenAI Codex app, or `npm i -g @openai/codex`, then `codex login`."
}

function Get-CodexHome {
  if ($env:CODEX_HOME) { return $env:CODEX_HOME }
  return (Join-Path $env:USERPROFILE ".codex")
}

# Map an OutFile that may be a WSL path (/home/...) to a Windows-usable UNC path.
function ConvertTo-WinPath([string]$p) {
  if ([string]::IsNullOrWhiteSpace($p)) { return $p }
  if ($p -match '^/') { return "\\wsl.localhost\$WslDistro" + ($p -replace '/', '\') }
  if (-not [System.IO.Path]::IsPathRooted($p)) { return (Join-Path (Get-Location).Path $p) }
  return $p
}

# Pick a non-versioned sibling name if the target exists and we're not overwriting.
function Get-FreeName([string]$path) {
  if ($Overwrite -or -not (Test-Path -LiteralPath $path)) { return $path }
  $dir = Split-Path $path -Parent
  $base = [System.IO.Path]::GetFileNameWithoutExtension($path)
  $ext = [System.IO.Path]::GetExtension($path)
  for ($i = 2; $i -lt 1000; $i++) {
    $cand = Join-Path $dir ("{0}-v{1}{2}" -f $base, $i, $ext)
    if (-not (Test-Path -LiteralPath $cand)) { return $cand }
  }
  return $path
}

$CHROMA_PROMPT = @"
Render the subject on a perfectly flat solid #00ff00 chroma-key background for background removal.
The background must be one uniform color with no shadows, gradients, texture, reflections, floor plane, or lighting variation.
Keep the subject fully separated from the background with crisp edges and generous padding.
Do not use #00ff00 anywhere in the subject. No cast shadow, no contact shadow, no reflection, no watermark, and no text unless explicitly requested.
"@

function Build-Prompt {
  $sb = [System.Text.StringBuilder]::new()
  if ($InputImage.Count -gt 0) {
    if ($Edit) {
      [void]$sb.AppendLine("Edit the attached image (Image 1). Apply only the requested change and keep everything else unchanged.")
    } else {
      [void]$sb.AppendLine("Use the attached image(s) as visual reference (style / composition / subject guidance), not as an edit target.")
    }
  }
  [void]$sb.AppendLine($Prompt)
  if ($Size -and $Size -ne "auto") { [void]$sb.AppendLine("Target output size: $Size.") }
  if ($Quality -and $Quality -ne "auto") { [void]$sb.AppendLine("Quality: $Quality.") }
  if ($Transparent) { [void]$sb.AppendLine($CHROMA_PROMPT) }
  # The critical instruction: generate only. Do NOT let Codex try to copy/move the file --
  # that path is sandbox-blocked and just wastes time and tokens. We collect it ourselves.
  [void]$sb.AppendLine("")
  [void]$sb.AppendLine("Use your built-in image generation tool to create this image. After it is generated, STOP. Do not move, copy, rename, save, or relocate the file, and do not run any shell command to find it. Leave it in your default generated-images directory; it will be collected automatically.")
  return $sb.ToString()
}

$codex = Resolve-Codex
$codexHome = Get-CodexHome
$genDir = Join-Path $codexHome "generated_images"
if (-not $WorkDir) { $WorkDir = Join-Path $env:TEMP ("imagegen-" + [guid]::NewGuid().ToString('N').Substring(0, 8)) }
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null

# Validate / map input images for -i
$inImgs = @()
foreach ($img in $InputImage) {
  $w = ConvertTo-WinPath $img
  if (-not (Test-Path -LiteralPath $w)) { Fail "Input image not found: $img (resolved: $w)" }
  $inImgs += $w
}

$finalPrompt = Build-Prompt
$results = @()

for ($v = 1; $v -le $Variants; $v++) {
  $t0 = Get-Date
  $codexArgs = @('exec', '-C', $WorkDir, '-s', 'workspace-write', '--skip-git-repo-check', '-c', 'model_reasoning_effort=low')
  # `-i, --image <FILE>...` is variadic, so it must come LAST or it eats following args.
  if ($inImgs.Count -gt 0) { $codexArgs += '-i'; $codexArgs += $inImgs }

  # The prompt goes via STDIN, not as a positional arg. This is deliberate:
  #  - `-i` is variadic and would swallow a trailing positional prompt as a filename.
  #  - piping the prompt delivers it then closes the pipe (EOF), which is exactly what
  #    stops the non-interactive "Reading additional input from stdin..." hang.
  #  - no shell arg-quoting headaches for long, multi-line, quote-containing prompts.
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $job = Start-Job -ScriptBlock {
    param($exe, $a, $p)
    $p | & $exe @a 2>&1 | Out-String
  } -ArgumentList $codex, $codexArgs, $finalPrompt
  $done = Wait-Job $job -Timeout $TimeoutSec
  if (-not $done) {
    Stop-Job $job -ErrorAction SilentlyContinue; Remove-Job $job -Force -ErrorAction SilentlyContinue
    Fail "Codex timed out after ${TimeoutSec}s on variant $v."
  }
  $out = Receive-Job $job; Remove-Job $job -Force -ErrorAction SilentlyContinue
  $sw.Stop()

  $sid = $null
  if ($out -match 'session id:\s*([0-9a-fA-F-]+)') { $sid = $matches[1] }

  # Collect the freshly generated file(s): prefer the session subdir, else newest-since-t0.
  $found = $null
  $sessDir = if ($sid) { Join-Path $genDir $sid } else { $null }
  if ($sessDir -and (Test-Path $sessDir)) {
    $found = Get-ChildItem $sessDir -File -ErrorAction SilentlyContinue |
      Where-Object { $_.Extension -in '.png', '.webp', '.jpg', '.jpeg' } |
      Sort-Object LastWriteTime -Descending | Select-Object -First 1
  }
  if (-not $found) {
    $found = Get-ChildItem $genDir -Recurse -File -ErrorAction SilentlyContinue |
      Where-Object { ($_.Extension -in '.png', '.webp', '.jpg', '.jpeg') -and $_.LastWriteTime -gt $t0 } |
      Sort-Object LastWriteTime -Descending | Select-Object -First 1
  }
  if (-not $found) {
    $tail = ($out -split "`n" | Select-Object -Last 12) -join "`n"
    Fail "Codex ran but no new image was found in $genDir. Last output:`n$tail"
  }

  $source = $found.FullName
  $saved = $null

  if ($OutFile) {
    $dest = ConvertTo-WinPath $OutFile
    if ($Transparent) { $dest = [System.IO.Path]::ChangeExtension($dest, ".png") }
    # For multiple variants, distinguish filenames.
    if ($Variants -gt 1) {
      $d = Split-Path $dest -Parent
      $b = [System.IO.Path]::GetFileNameWithoutExtension($dest)
      $e = [System.IO.Path]::GetExtension($dest)
      $dest = Join-Path $d ("{0}-{1}{2}" -f $b, $v, $e)
    }
    $destDir = Split-Path $dest -Parent
    if ($destDir -and -not (Test-Path $destDir)) { New-Item -ItemType Directory -Force -Path $destDir | Out-Null }
    $dest = Get-FreeName $dest

    if ($Transparent) {
      $chroma = Join-Path $codexHome "skills\.system\imagegen\scripts\remove_chroma_key.py"
      if (-not (Test-Path $chroma)) { Fail "Transparency helper not found at $chroma" }
      & python $chroma --input $source --out $dest --auto-key border --soft-matte --transparent-threshold 12 --opaque-threshold 220 --despill 2>&1 | Out-Null
      if (-not (Test-Path $dest)) { Fail "Chroma-key removal failed; no output at $dest" }
    } else {
      Copy-Item -LiteralPath $source -Destination $dest -Force
    }
    $saved = $dest
  } else {
    $saved = $source   # preview mode: leave in generated_images, just report it
  }

  $results += [pscustomobject]@{
    saved      = $saved
    source     = $source
    sessionId  = $sid
    elapsedSec = [math]::Round($sw.Elapsed.TotalSeconds, 1)
    placed     = [bool]$OutFile
  }
}

if ($Json) {
  [pscustomobject]@{ ok = $true; count = $results.Count; results = $results } | ConvertTo-Json -Depth 5 | Write-Output
} else {
  foreach ($r in $results) {
    $note = if (-not $r.placed) { ', preview-only in generated_images' } else { '' }
    Write-Output ("OK  {0}  ({1}s{2})" -f $r.saved, $r.elapsedSec, $note)
  }
}
