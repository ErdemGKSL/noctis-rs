"""Print the story-loop workflow args for the stories that are not done yet.

usage: python3 tools/story-loop/stories.py [--epics FIRST-LAST] [--limit N]

Reads sprint-status.yaml for order and status and epics.md for titles. A story whose
spec is already `in-progress` (started, then interrupted) resumes at implementation.
"""

import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
IA = ROOT / "_bmad-output/implementation-artifacts"
EPICS = ROOT / "_bmad-output/planning-artifacts/epics.md"


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--epics", help="inclusive epic range, e.g. 7-9")
    p.add_argument("--limit", type=int, help="at most this many stories")
    a = p.parse_args()
    lo, hi = (map(int, a.epics.split("-")) if a.epics else (1, 10**6))

    titles = {
        m[1]: m[2].strip()
        for m in re.finditer(r"^### Story (\d+\.\d+): (.+)$", EPICS.read_text(), re.M)
    }
    stories = []
    for m in re.finditer(r"^  ((\d+)-(\d+)-[a-z0-9_-]+): (\S+)", (IA / "sprint-status.yaml").read_text(), re.M):
        key, epic, n, status = m[1], int(m[2]), m[3], m[4]
        if status == "done" or not lo <= epic <= hi:
            continue
        num = f"{epic}.{n}"
        title = titles.get(num, key)
        st = {
            "key": key,
            "id": f"{epic}-{n}",
            "num": num,
            "epic": epic,
            "title": title.replace("[Maintainer-gated] ", ""),
            "gated": "[Maintainer-gated]" in title,
        }
        spec = IA / f"spec-{key}.md"
        if spec.exists() and re.search(r"^status: '?in-progress", spec.read_text(), re.M):
            st["resumeAt"] = "implement"
        stories.append(st)
    if a.limit:
        stories = stories[: a.limit]
    print(json.dumps(stories, ensure_ascii=False))


if __name__ == "__main__":
    main()
