"""Exit 0 if the story being gated (the in-progress story of sprint-status.yaml) is the last open story of its epic.

Run from the repository root. A story counts as finished when it is done in the local sprint-status.yaml or in
origin/main's, so a lane also sees the stories other lanes have merged. Prints the decision to stderr.
"""
import re
import subprocess
import sys

PATH = "_bmad-output/implementation-artifacts/sprint-status.yaml"
STORY = re.compile(r"^\s+(\d+)-(\d+)-[\w-]+:\s*([\w-]+)", re.M)


def statuses(text):
    return {(e, n): st for e, n, st in STORY.findall(text)}


local = statuses(open(PATH).read())
subprocess.run(["git", "fetch", "-q", "origin", "main"], capture_output=True)
remote = subprocess.run(["git", "show", f"origin/main:{PATH}"], capture_output=True, text=True).stdout
main = statuses(remote)
current = [k for k, st in local.items() if st in ("in-progress", "review")]
if not current:
    print("epic-last: no in-progress story; cargo hack skipped", file=sys.stderr)
    sys.exit(1)
for epic, num in current:
    open_ = [f"{e}.{n}" for (e, n), st in local.items()
             if e == epic and n != num and st != "done" and main.get((e, n)) != "done"]
    if not open_:
        print(f"epic-last: {epic}.{num} is the last open story of epic {epic}; running cargo hack", file=sys.stderr)
        sys.exit(0)
    print(f"epic-last: epic {epic} still has {', '.join(open_)}; cargo hack skipped", file=sys.stderr)
sys.exit(1)
