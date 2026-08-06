# TASKS.md — vxedge/vxmcp

Phase 3 (`docs/TASKS.md`) — frontend/tooling investment, not gate-blocking. Master spec:
`../docs/ARCHITECTURE.md` §5.1 (frontends, `vxmcp`'s own design paragraph).

## Phase 3

- [x] **Repo scaffolded, 2026-08-06**: workspace `Cargo.toml`, `crates/vxmcp`, CI
      (build/test/clippy/fmt/`cargo deny`), `AGENTS.md`/`CLAUDE.md`/`README.md`/`CHANGELOG.md`.
      Language decision (`docs/TASKS.md`, full account there): Rust on `rmcp` >=3.1.1 — checked
      the SDK's own release history before committing rather than trusting a same-week
      "beta/Tier-2" snapshot; `rmcp` reached spec-2026-07-28 Tier-1 conformance 2026-07-31 and
      shipped the exact MRTR/cache-hint features this design needs on 2026-08-05.
- [x] **`vxedge-api` wired as a git dependency**, pinned to a commit
      (`79f6be9a89c6e47e584750a1efe781497ca8d377`) rather than a branch — `vxmcp` isn't a
      member of `vxedge`'s own Cargo workspace, so this is the correct mechanism, not a
      workaround. Confirmed live: `cargo build`/`clippy`/`test`/`cargo deny check` all resolve
      and pass clean against the private repo. **`VXEDGE_CROSS_REPO_TOKEN` added to this
      repo's own secrets 2026-08-06** (same pattern `vxperiments`' CI already uses,
      `docs/TASKS.md` #18) — a human/admin action, done via `gh secret set` against the
      already-issued `vxperiments` PAT.
      CI needed three real fixes past that, found and fixed 2026-08-06 by reading each failing
      job's actual log rather than assuming the secret alone would be enough:
      1. The `deny` job's `git config --global url.insteadOf` step (copied from the `rust`
         job) has no effect inside `EmbarkStudios/cargo-deny-action@v2` — that action runs in
         its own Docker container (`action.yml`: `runs.using: docker`), which doesn't inherit
         the runner's global `~/.gitconfig`. Switched to the action's own dedicated
         `credentials` input.
      2. That `credentials` input's format matters and isn't obvious from `entrypoint.sh`
         alone (which just echoes it verbatim to `$HOME/.git-credentials`) — the action's
         README spells out the real requirement: a `user:pass` pair. A bare token with no
         username (`https://TOKEN@github.com`) writes to the file but git's credential-store
         lookup still fails to match it; confirmed live (secret substitution worked, auth
         still failed) before finding the README's example and switching to
         `https://x-access-token:${{ secrets... }}@github.com`.
      3. The `rust` job's own git-config approach was already correct (confirmed from its
         failure log: the URL rewrite took effect) — but a second, unrelated failure surfaced
         once auth passed: `vxedge-api`'s `build.rs` needs `protoc` to regenerate the tonic
         bindings, same as it does when `vxedge` builds itself in-tree, and `ubuntu-latest`
         doesn't ship it. Added the identical "Install protoc" step
         `vxedge/.github/workflows/ci.yml` already carries, rather than inventing a new
         approach.
      All three landed in one commit; awaiting the next CI run to confirm both jobs pass clean
      end to end.
- [x] **`experiment_status` — first tool, proves the wrapper pattern end to end.** A direct 1:1
      call to `ExperimentService.Status`, the same RPC `vxedge experiment status` calls.
      Reuses `vxedge-cli`'s own Unix-socket connection pattern
      (`Endpoint::connect_with_connector` over `tokio::net::UnixStream`) verbatim — copied, not
      reinvented, since it's already proven. Takes an explicit `workspace` path argument rather
      than resolving from a working directory the way the CLI does: an MCP tool call has no
      meaningful "current directory" of its own caller. `ServerHandler::get_info()` advertises
      `ProtocolVersion::V_2026_07_28` explicitly. Unit-tested (arg deserialization); not yet
      live-tested against a running `vxedged` or through an actual MCP client/inspector — see
      below.
### The open board — 5 items, in unblock order

| # | Task | State |
|---|---|---|
| 1 | Live end-to-end smoke test | **in progress** |
| 2 | Remaining read tools (`zone/compute list`, `job watch`, `events subscribe`, results-archive summary) | open |
| 3 | MRTR confirmation flow for mutating tools | open |
| 4 | Manifest JSON Schema integration (depends on `vxparts` `ctl`-class schema part) | open |
| 5 | Real deployment (systemd unit / process supervision) | open |

- [ ] **#1 — Live end-to-end smoke test**: `cargo run -p vxmcp` against a real `vxedged`, called
      through `npx @modelcontextprotocol/inspector` or a real agent client, confirming the tool
      actually round-trips (not just that it compiles and unit-tests). Needs a running
      `vxedged` + a real experiment workspace.
- [ ] **#2 — Read tools, the rest of the safe slice**: `zone/compute list`, `job watch`,
      `events subscribe`, a results-archive summary. Same shape as `experiment_status` — one
      module per vxAPI service under `crates/vxmcp/src/tools/`, one `#[tool]` method per RPC
      dispatching to it. No design question, no dependency on #3.
- [ ] **#3 — MRTR confirmation flow for mutating tools** (`up`/`down`/`traffic up`) — the design
      point this repo's own `AGENTS.md` states but hasn't built: return `InputRequiredResult`
      with an `InputRequest::Elicitation` asking the caller to confirm, verify the echoed
      `requestState` (caller-controlled input) before trusting it, complete on the retry. See
      `rmcp`'s own `examples/servers/src/mrtr.rs` for the reference pattern. Needs a design
      pass on exactly what confirmation text/schema each mutating tool asks for before wiring
      the RPCs themselves — not assumed here, and not started.
- [ ] **#4 — Publish the manifest JSON Schema as a `vxparts` `ctl`-class part**
      (`vxparts/TASKS.md` already tracks the schema-generation half, `parts/ctl/schema/
      config/manifest-schema`) — once it exists, `vxmcp`'s tool argument schemas for anything
      manifest-shaped could validate against the same source of truth instead of hand-writing
      `schemars`-derived structs that could drift from it. Not blocking #2, which takes simple
      scalar arguments (a path, an id) with nothing manifest-shaped yet.
- [ ] **#5 — Real deployment**: a systemd unit / process-supervision story once this leaves
      scaffold-and-prove-the-pattern stage — mirrors `vxedge-tray`'s own "pairs with a systemd
      unit for vxedged" note (`ARCHITECTURE.md` §5.1), not designed here.

## Explicitly not phase 3 (see `docs/ARCHITECTURE.md` §15's Phase 5)

- Remote transport (TCP + mTLS/OIDC) for `vxmcp` talking to a `vxedged` that isn't on the same
  host — deferred with every other frontend's remote-access story until a real remote consumer
  exists; `vxmcp` itself only ever needs the local Unix socket for now.
