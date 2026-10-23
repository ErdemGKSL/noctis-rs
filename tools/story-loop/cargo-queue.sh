#!/bin/bash
# cargo-queue: one queue for every building cargo command of the story-loop checkouts.
# Installed as $CARGO_HOME/bin/cargo (`tools/story-loop/cargo-queue.sh --install`; again after a container restart),
# it runs cargo directly outside the story-loop checkouts. A command started in the main checkout or a lane copies
# that working tree into one shared work dir and runs there, one command at a time, against one cargo target dir:
# parallel lanes code side by side while their builds wait in line, instead of splitting the CPUs and each filling a
# target dir. Output streams back with work-dir paths mapped to the caller's tree, and files the command changed
# (cargo fmt, --bless, Cargo.lock) are copied back. A caller cut off by a timeout can rerun the same command: it
# attaches to the queued or running one, and one that already succeeded on the identical tree replays its output.
# Commands that do not build (metadata, tree, --version, ...) and anything run by the queue itself go straight to cargo.
CH=${CARGO_HOME:-$HOME/.cargo}
Q=${NOCTIS_CARGO_QUEUE:-/home/claude/cargo-queue}
ROOTS=${NOCTIS_QUEUE_ROOTS:-/home/claude/noctis-rs /home/claude/lanes/*/noctis-rs}
# One work dir for every caller tree. Cargo hashes a workspace crate by its path relative to the workspace root, so
# separate work dirs would share artifacts in the shared target dir while each kept old mtimes: a lane could then
# link another lane's build of a crate as "fresh". In one work dir a file is rewritten, and so rebuilt, exactly when
# its content differs from the tree built last.
W=$Q/work/noctis-rs
LAYOUT=1  # bump to rebuild the work dir from scratch (fresh mtimes everywhere) after a layout change
real(){ exec -a cargo "$CH/bin/rustup" "$@"; }

install(){
  local here; here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  mkdir -p "$Q/bin" "$Q/queue" "$Q/jobs" "$Q/work"
  cp "$here/cargo-queue.sh" "$Q/bin/cargo-queue.sh.tmp" && mv "$Q/bin/cargo-queue.sh.tmp" "$Q/bin/cargo-queue.sh"
  cp "$here/tree-sync.py" "$Q/bin/tree-sync.py.tmp" && mv "$Q/bin/tree-sync.py.tmp" "$Q/bin/tree-sync.py"
  chmod +x "$Q/bin/cargo-queue.sh"
  printf '#!/bin/bash\nexec bash %q "$@"\n' "$Q/bin/cargo-queue.sh" > "$CH/bin/cargo.tmp" && chmod +x "$CH/bin/cargo.tmp"
  mv -f "$CH/bin/cargo.tmp" "$CH/bin/cargo"
  echo "cargo-queue installed: $CH/bin/cargo -> $Q/bin/cargo-queue.sh (undo: ln -sf rustup $CH/bin/cargo)"
}

runner(){
  exec 9>>"$Q/runner.lock"
  flock -n 9 || exit 0
  export NOCTIS_CARGO_DIRECT=1 CARGO_TARGET_DIR=${NOCTIS_CARGO_TARGET:-/home/claude/noctis-rs/target}
  # Builds get NOCTIS_CARGO_CPUS of the machine's CPUs (default 3), so the coding agents keep a core to work on.
  local cpus=${NOCTIS_CARGO_CPUS:-3} pin=()
  export CARGO_BUILD_JOBS=$cpus RUST_TEST_THREADS=$cpus
  command -v taskset >/dev/null && [ "$(nproc)" -gt "$cpus" ] && pin=(taskset -c "0-$((cpus - 1))")
  for j in "$Q"/jobs/*/; do  # jobs a dead runner left behind
    [ -f "$j/started" ] && [ ! -f "$j/status" ] && { echo "cargo-queue: the runner restarted; run the command again" >> "$j/log"; echo 125 > "$j/status"; }
  done
  find "$Q/jobs" -mindepth 1 -maxdepth 1 -mmin +1440 -exec rm -rf {} + 2>/dev/null
  [ "$(cat "$Q/work/.layout" 2>/dev/null)" = "$LAYOUT" ] || { rm -rf "$Q/work"; mkdir -p "$W"; echo "$LAYOUT" > "$Q/work/.layout"; }
  mkdir -p "$W"
  local idle=0 q key J root rel rc args a me=$Q/bin/cargo-queue.sh born
  born=$(stat -c %Y "$me")
  while true; do
    # A reinstalled cargo-queue.sh takes over between commands.
    # It starts from the caller-like environment, not this runner's exports (a variable a new version dropped must go).
    if [ "$(stat -c %Y "$me" 2>/dev/null)" != "$born" ]; then
      exec 9>&-
      mapfile -d '' keep < <(env -0 | grep -z -E '^(PATH|HOME|USER|LANG|TERM|CARGO_HOME|RUSTUP_HOME|NOCTIS_[A-Z_]+)=' | grep -z -v '^NOCTIS_CARGO_DIRECT=')
      exec env -i "${keep[@]}" bash "$me" --runner
    fi
    q=$(ls "$Q/queue" 2>/dev/null | sort | head -1)
    if [ -z "$q" ]; then idle=$((idle + 1)); [ $idle -gt 3600 ] && exit 0; sleep 1; continue; fi
    idle=0; key=${q#*.}; J=$Q/jobs/$key
    rm -f "$Q/queue/$q"
    [ -d "$J" ] && [ ! -f "$J/status" ] || continue
    touch "$J/started"
    root=$(cat "$J/root"); rel=$(cat "$J/rel")
    echo "$(date -u +%FT%TZ) start $root/$rel: $(tr '\0' ' ' < "$J/argv")" >> "$Q/runner.log"
    if python3 "$Q/bin/tree-sync.py" sync "$root" "$W" >> "$J/log" 2>&1 && python3 "$Q/bin/tree-sync.py" snap "$W" "$J/snap"; then
      args=()
      while IFS= read -r -d '' a; do args+=("${a//$root/$W}"); done < "$J/argv"
      ( cd "$W/$rel" || exit 125
        while IFS= read -r -d '' a; do export "$a"; done < "$J/env"
        exec "${pin[@]}" bash -c 'exec -a cargo "$0" "$@"' "$CH/bin/rustup" "${args[@]}" ) >> "$J/log" 2>&1 < /dev/null
      rc=$?
      python3 "$Q/bin/tree-sync.py" back "$W" "$root" "$J/snap" >> "$J/log" 2>&1 || rc=125
    else
      rc=125
    fi
    echo "$rc" > "$J/status"
    echo "$(date -u +%FT%TZ) done rc=$rc" >> "$Q/runner.log"
  done
}

case "$1" in
--install) install; exit ;;
--runner) runner; exit ;;
esac
[ -n "$NOCTIS_CARGO_DIRECT" ] && real "$@"
cwd=$(pwd -P)
root=$(git -C "$cwd" rev-parse --show-toplevel 2>/dev/null)
inscope=
for r in $ROOTS; do [ -n "$root" ] && [ "$root" = "$r" ] && inscope=1; done
[ -n "$inscope" ] || real "$@"
sub=$1; [[ $sub == +* ]] && sub=$2
case "$sub" in
''|metadata|locate-project|pkgid|tree|search|read-manifest|version|help|--version|-V|--help|-h|--list|-vV) real "$@" ;;
esac
# cargo hack (about 45 minutes in the queue) runs only from gates.sh, which runs it once per epic.
if [ "$sub" = hack ] && [ -z "$NOCTIS_GATES" ]; then
  echo "cargo-queue: skipped: cargo hack runs only in tools/story-loop/gates.sh, once per epic (maintainer rule); the story loop runs it" >&2
  exit 0
fi
rel=${cwd#"$root"}; rel=${rel#/}
idx=$(mktemp); trap 'rm -f "$idx"; [ -n "$sp" ] && kill "$sp" 2>/dev/null' EXIT
cp "$(git -C "$root" rev-parse --path-format=absolute --git-path index)" "$idx" 2>/dev/null
tree=$(cd "$root" && GIT_INDEX_FILE=$idx git add -A >/dev/null 2>&1 && GIT_INDEX_FILE=$idx git write-tree)
envf=$(mktemp)
env -0 | grep -z -E '^(RUST|CARGO_|TRYBUILD|INSTA)' | grep -z -v -E '^(CARGO_TARGET_DIR|CARGO_HOME|RUSTUP_HOME|RUSTUP_TOOLCHAIN)=' > "$envf"
key=$( { printf '%s\0' "$root" "$rel" "$tree" "$@"; cat "$envf"; } | md5sum | cut -c1-16)
J=$Q/jobs/$key
mkdir -p "$Q/queue" "$Q/jobs"
exec 8>>"$Q/lock"; flock 8
if [ -f "$J/status" ] && { [ -z "$tree" ] || [ "$(cat "$J/status")" != 0 ]; }; then rm -rf "$J"; fi
if [ ! -d "$J" ]; then
  mkdir -p "$J"; printf %s "$root" > "$J/root"; printf %s "$rel" > "$J/rel"; printf '%s\0' "$@" > "$J/argv"
  mv "$envf" "$J/env"; : > "$J/log"
  touch "$Q/queue/$(date +%s%N).$key"
  n=$(ls "$Q/queue" | wc -l); [ "$n" -gt 1 ] && echo "cargo-queue: waiting behind $((n - 1)) queued command(s)" >&2
elif [ -f "$J/status" ]; then
  echo "cargo-queue: replaying the result of the same command on the same tree" >&2
else
  echo "cargo-queue: attached to the same command, already queued or running" >&2
fi
rm -f "$envf"; flock -u 8; exec 8>&-
(setsid nohup bash "$Q/bin/cargo-queue.sh" --runner >> "$Q/runner.out" 2>&1 < /dev/null &)
sleep infinity & sp=$!
tail -c +1 -f --pid="$sp" "$J/log" | sed -u "s|$W|$root|g" & tp=$!
while [ ! -f "$J/status" ]; do
  sleep 1
  [ $((SECONDS % 30)) = 0 ] && (setsid nohup bash "$Q/bin/cargo-queue.sh" --runner >> "$Q/runner.out" 2>&1 < /dev/null &)
done
kill "$sp"; wait "$tp" 2>/dev/null; sp=
exit "$(cat "$J/status")"
