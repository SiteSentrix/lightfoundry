# Security audit, 2026-10-17

A full pass over the repository: dependency advisories, secrets, `unsafe`, the file and command surface,
the local agent link (MCP), G-code and the machine link, the font download, the browser build and the GitHub
workflows. Method: `cargo audit` and pattern scans by hand; two independent read-only code reviews (one for
files, commands and the agent link, one for G-code, the machine link, networking, the browser build and the
workflows) whose findings were checked against the code. Not done: a dynamic test on real hardware, a
third-party review, or a dependency licence review.

Result: no critical issue, nothing that leaves the laser on or injects G-code, no known-vulnerable dependency
(`cargo audit`, 459 crates, 1334 advisories), no secrets in the tree.

## Fixed in this pass

| Finding                                                                                                   | Fix                                                                                       |
|-----------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------|
| Importing or opening a path such as `/dev/zero` read until memory ran out (size came from file metadata)   | `format::read_capped` reads at most the limit; used by open, SVG/DXF/picture import       |
| The agent-link token fell back to a time-seeded value where `/dev/urandom` is missing (Windows)            | OS generator through `getrandom`; no token means no link                                  |
| A client could hold a link slot for ever by sending one byte at a time before the token                    | One total deadline for the handshake                                                      |
| An array of many selected shapes could multiply into millions of copies                                    | The shapes times the copies is capped at 200,000                                          |
| After a failed write mid-job the link dropped the job without trying a soft reset                          | A soft reset is attempted first                                                           |
| The browser's serial input buffer had no bound                                                             | Keeps the newest 64 KiB                                                                   |
| Saving wrote through a predictable temp name that could be a planted link                                  | A fresh, unguessable name created with `create_new`; removed on failure                   |
| The browser page had no content security policy                                                            | CSP with a hash for the one inline script, same-origin connections only                   |
| `desktop.yml` gave every build job a write token                                                           | Builds read-only; only the release job can write                                          |
| Checkouts kept the token in `.git/config`                                                                  | `persist-credentials: false` (CI, Desktop, Fix workflows)                                 |
| No automated advisory or action updates                                                                    | `audit.yml` (weekly and on `Cargo.lock` changes) and `dependabot.yml`                     |
| The bug-fix workflow ran `cargo test` while holding the API key, on untrusted issue text                   | It now runs no builds or tests (the pull request's CI runs the gate); keep requiring a maintainer to apply the `claude-fix` label after reading the issue |

## Open, for a decision

1. **Actions are pinned to tags, not commit hashes** (`dtolnay/rust-toolchain@master` is a branch). Pinning
   needs the hashes; Dependabot then keeps them current.
2. **File-writing commands take any path** (`file.save`, `file.autosave`, `file.export_*`), so a client holding
   the link token, or a script, can overwrite any file the user can. Intended for a trusted agent; options are
   confining live-link writes to the document's folder or asking the person.
3. **Undo history has no byte budget** (200 steps, each a clone of the document), so a long loop of cheap
   commands on a very large document can use gigabytes.
4. **The link's queues are unbounded after the token is checked**, so a client that never reads its replies can
   grow memory.
5. **Autosave, recovery and recent-file lists use default permissions** (often world-readable on Linux). The
   link file is owner-only. Windows relies on the profile folder's permissions.
6. **Relative-origin jobs (current position) cannot check the bed**; a job near an edge can run into the frame
   unless the controller's soft limits are on. The app warns when laser mode is off; it does not block the job.
7. **Closing a browser tab mid-job** gives no chance to send a soft reset; the controller finishes its buffered
   moves. A warning or a `beforeunload` prompt while a job runs would help.
9. Two running instances share one link file; the first to quit removes the other's (reliability only).
