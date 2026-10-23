"""Copies source trees between a lane and the cargo queue's work dir (see cargo-queue.sh).

usage: tree-sync.py sync <src> <dst>     make <dst> hold exactly the working-tree files of the git checkout <src>
                                         (tracked plus untracked, non-ignored), leaving <dst>/target alone
       tree-sync.py snap <dir> <file>    record every file of <dir> (except target/) with its size and mtime
       tree-sync.py back <dir> <dst> <file>  copy files of <dir> that changed since the snapshot to <dst>, and
                                         delete from <dst> the files <dir> lost (cargo fmt, --bless, Cargo.lock)

A file is rewritten only when its bytes or mode differ, so it gets a fresh mtime exactly when its content changed:
cargo decides freshness by mtime, and copying with the source's mtimes (rsync -a, tar) could hand it another lane's
older file and a stale build.
"""
import json
import os
import shutil
import stat
import subprocess
import sys


def walk(top):
    out = {}
    for root, dirs, files in os.walk(top):
        if root == top:
            dirs[:] = [d for d in dirs if d not in ("target", ".git")]
        for f in files:
            p = os.path.join(root, f)
            st = os.lstat(p)
            out[os.path.relpath(p, top)] = [st.st_size, st.st_mtime_ns, st.st_mode]
    return out


def put(s, d):
    """Copies s over d unless d already has the same bytes and mode."""
    st = os.lstat(s)
    if stat.S_ISLNK(st.st_mode):
        if os.path.islink(d) and os.readlink(d) == os.readlink(s):
            return
        if os.path.lexists(d):
            os.remove(d)
        os.makedirs(os.path.dirname(d), exist_ok=True)
        os.symlink(os.readlink(s), d)
        return
    try:
        if not os.path.islink(d) and os.lstat(d).st_mode == st.st_mode:
            with open(s, "rb") as a, open(d, "rb") as b:
                if a.read() == b.read():
                    return
    except OSError:
        pass
    if os.path.islink(d):
        os.remove(d)
    os.makedirs(os.path.dirname(d), exist_ok=True)
    shutil.copyfile(s, d)
    os.chmod(d, stat.S_IMODE(st.st_mode))


def prune_dirs(top):
    for root, dirs, files in os.walk(top, topdown=False):
        rel = os.path.relpath(root, top)
        if rel != "." and rel.split(os.sep)[0] not in ("target", ".git") and not os.listdir(root):
            os.rmdir(root)


mode = sys.argv[1]
if mode == "sync":
    src, dst = sys.argv[2], sys.argv[3]
    names = subprocess.run(
        ["git", "-C", src, "ls-files", "-z", "-co", "--exclude-standard"], check=True, capture_output=True
    ).stdout.split(b"\0")
    want = {os.fsdecode(n) for n in names if n}
    want = {n for n in want if os.path.lexists(os.path.join(src, n))}  # deleted but still tracked
    for p in walk(dst):
        if p not in want:
            os.remove(os.path.join(dst, p))
    for n in want:
        put(os.path.join(src, n), os.path.join(dst, n))
    prune_dirs(dst)
elif mode == "snap":
    with open(sys.argv[3], "w") as f:
        json.dump(walk(sys.argv[2]), f)
elif mode == "back":
    top, dst = sys.argv[2], sys.argv[3]
    with open(sys.argv[4]) as f:
        before = json.load(f)
    now = walk(top)
    for p, meta in now.items():
        if before.get(p) != meta:
            put(os.path.join(top, p), os.path.join(dst, p))
            print(f"cargo-queue: wrote back {p}", file=sys.stderr)
    for p in before:
        if p not in now and os.path.lexists(os.path.join(dst, p)):
            os.remove(os.path.join(dst, p))
            print(f"cargo-queue: removed {p}", file=sys.stderr)
else:
    sys.exit(f"unknown mode {mode}")
