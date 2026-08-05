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
      and pass clean against the private repo. **CI needs `VXEDGE_CROSS_REPO_TOKEN` added to
      this repo's own secrets** (same pattern `vxperiments`' CI already uses,
      `docs/TASKS.md` #18) — not yet done, a human/admin action; until then the CI workflow
      file exists but its private-repo checkout step will fail.
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
- [ ] **Live end-to-end smoke test**: `cargo run -p vxmcp` against a real `vxedged`, called
      through `npx @modelcontextprotocol/inspector` or a real agent client, confirming the tool
      actually round-trips (not just that it compiles and unit-tests). Needs a running
      `vxedged` + a real experiment workspace, neither available in the environment that
      authored this scaffold.
- [ ] **Read tools, the rest of the safe slice**: `zone/compute list`, `job watch`,
      `events subscribe`, a results-archive summary. Same shape as `experiment_status` — one
      module per vxAPI service under `crates/vxmcp/src/tools/`, one `#[tool]` method per RPC
      dispatching to it. No design question, no dependency on the item below.
- [ ] **MRTR confirmation flow for mutating tools** (`up`/`down`/`traffic up`) — the design
      point this repo's own `AGENTS.md` states but hasn't built: return `InputRequiredResult`
      with an `InputRequest::Elicitation` asking the caller to confirm, verify the echoed
      `requestState` (caller-controlled input) before trusting it, complete on the retry. See
      `rmcp`'s own `examples/servers/src/mrtr.rs` for the reference pattern. Needs a design
      pass on exactly what confirmation text/schema each mutating tool asks for before wiring
      the RPCs themselves — not assumed here, and not started.
- [ ] **Publish the manifest JSON Schema as a `vxparts` `ctl`-class part**
      (`vxparts/TASKS.md` already tracks the schema-generation half, `parts/ctl/schema/
      config/manifest-schema`) — once it exists, `vxmcp`'s tool argument schemas for anything
      manifest-shaped could validate against the same source of truth instead of hand-writing
      `schemars`-derived structs that could drift from it. Not blocking the tools above, which
      take simple scalar arguments (a path, an id) with nothing manifest-shaped yet.
- [ ] **Real deployment**: a systemd unit / process-supervision story once this leaves
      scaffold-and-prove-the-pattern stage — mirrors `vxedge-tray`'s own "pairs with a systemd
      unit for vxedged" note (`ARCHITECTURE.md` §5.1), not designed here.

## Explicitly not phase 3 (see `docs/ARCHITECTURE.md` §15's Phase 5)

- Remote transport (TCP + mTLS/OIDC) for `vxmcp` talking to a `vxedged` that isn't on the same
  host — deferred with every other frontend's remote-access story until a real remote consumer
  exists; `vxmcp` itself only ever needs the local Unix socket for now.
