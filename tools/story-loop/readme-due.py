"""Exit 0 if README.md's progress is due for a refresh (maintainer rule, 2026-10-25: every 3-4 merged stories).

Run from the repository root (a lane worktree or the main checkout) after a story is marked done. Due when the
stories done in the local sprint-status.yaml or in origin/main's (union) exceed the "Stories done: N" badge of
origin/main's README.md by 3 or more, or when the story just finished closed its epic. Prints the decision.
"""
import re
import subprocess
import sys

PATH = "_bmad-output/implementation-artifacts/sprint-status.yaml"
STORY = re.compile(r"^\s+(\d+)-(\d+)-[\w-]+:\s*([\w-]+)", re.M)


def show(path):
    return subprocess.run(["git", "show", f"origin/main:{path}"], capture_output=True, text=True).stdout


subprocess.run(["git", "fetch", "-q", "origin", "main"], capture_output=True)
local = {(e, n): st for e, n, st in STORY.findall(open(PATH).read())}
main = {(e, n): st for e, n, st in STORY.findall(show(PATH))}
done = {k for k in local.keys() | main.keys() if "done" in (local.get(k), main.get(k))}
badge = re.search(r"Stories done: (\d+)", show("README.md"))
shown = int(badge[1]) if badge else 0
new_epics = {e for e, _ in done - {k for k, st in main.items() if st == "done"}}
closed = [e for e in new_epics if all(k in done for k in local.keys() | main.keys() if k[0] == e)]
if len(done) - shown >= 3 or closed:
    print(f"readme-due: yes ({len(done)} done, badge shows {shown}, epics closed: {closed or 'none'})")
    sys.exit(0)
print(f"FAIL: readme-due: no ({len(done)} done, badge shows {shown})")
sys.exit(1)
