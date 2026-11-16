# Rules for anyone (or any agent) writing in this repo

## Comments

- A comment says **why** the code is the way it is, when the code cannot say
  it. Nothing else.
- No history. Never "this used to…", "before, X happened", "this exists
  because Y got shipped", "which is what the bug was", quotes of bug reports,
  or measurements from a debugging session. That belongs in the commit
  message.
- Keep it short: one line when one line does. A block of several lines needs
  a real reason (a trust boundary, a format, a non-obvious trade-off).
- Don't restate the code, the function name, or the obvious.
- A deliberate shortcut gets `shortcut: <the limit>, <when to upgrade>`.

## Docs

`docs/` is a public GitBook for anybody who plays, hosts or builds, not a
notebook for the people running the official servers.

- Use `example.org` names and placeholders (`<handle>`, `<your-ip>`), never a
  real domain, account, IP or personal detail from somebody's own setup.
- The one exception is the official Fireplace's address where the game
  really uses it (the client and Flame defaults).
- New pages go in `docs/SUMMARY.md`, or GitBook will not show them.
