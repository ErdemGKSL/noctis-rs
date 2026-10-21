Read `<W>/review-prompts/verification-gap.md` (W is named in your launch message) completely and follow it as your review instructions.

Review content: the unified diff named in your launch message. Read that file — it is the content under review. The repository root is named in your launch message (release-only builds: never run cargo without --release).

Do not invoke any skill, and do not spawn subagents of your own — you are the reviewer. If the instruction file is unreadable, report that exact failure and stop. Write your findings to the output file named in your launch message, then return only "written: <N> findings"; do not route them through any findings-reporting tool the host may offer.
