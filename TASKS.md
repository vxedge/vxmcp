# TASKS.md — vxedge/vxmcp

Phase 3 (`docs/ROADMAP.md`) — frontend/tooling investment, not gate-blocking. Master spec:
`../docs/ARCHITECTURE.md` §5.1 (frontends, `vxmcp`'s own design paragraph).

## Phase 3

- [x] **Repo scaffolded, 2026-08-06**: workspace `Cargo.toml`, `crates/vxmcp`, CI
      (build/test/clippy/fmt/`cargo deny`), `AGENTS.md`/`CLAUDE.md`/`README.md`/`CHANGELOG.md`.
      Language decision (`docs/ROADMAP.md`, full account there): Rust on `rmcp` >=3.1.1 — checked
      the SDK's own release history before committing rather than trusting a same-week
      "beta/Tier-2" snapshot; `rmcp` reached spec-2026-07-28 Tier-1 conformance 2026-07-31 and
      shipped the exact MRTR/cache-hint features this design needs on 2026-08-05.
- [x] **`vxedge-api` wired as a git dependency**, pinned to a commit
      (`79f6be9a89c6e47e584750a1efe781497ca8d377`; bumped 2026-10-29 to
      `a78e7ddada05ca4e8b28b63f97301e04e4023c4e` for the `svc_status` fix — the vxAPI proto
      diff across those 23 commits is one comment, so the RPC surface is unchanged) rather
      than a branch — `vxmcp` isn't a
      member of `vxedge`'s own Cargo workspace, so this is the correct mechanism, not a
      workaround. Confirmed live: `cargo build`/`clippy`/`test`/`cargo deny check` all resolve
      and pass clean against the private repo. **`VXEDGE_CROSS_REPO_TOKEN` added to this
      repo's own secrets 2026-08-06** (same pattern `vxperiments`' CI already uses,
      `docs/ROADMAP.md` #18) — a human/admin action, done via `gh secret set` against the
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
      **Follow-up, 2026-10-28:** the only CI run still on record (2026-09-24, on the public-
      release commit, which also dropped the token/credentials steps from `ci.yml`) failed —
      the `rust` job couldn't fetch the pinned `vxedge-api` rev (`failed to authenticate`), and
      this repo has no secrets set any more. `vxedge` is public now and the pinned rev is on
      its `main`; an anonymous `cargo fetch --locked` (clean `CARGO_HOME`, no git credentials,
      no global gitconfig) resolves it cleanly, so no token is needed. The next CI run confirms
      it end to end.
- [x] **`experiment_status` — first tool, proves the wrapper pattern end to end.** A direct 1:1
      call to `ExperimentService.Status`, the same RPC `vxedge experiment status` calls.
      Reuses `vxedge-cli`'s own Unix-socket connection pattern
      (`Endpoint::connect_with_connector` over `tokio::net::UnixStream`) verbatim — copied, not
      reinvented, since it's already proven. Takes an explicit `workspace` path argument rather
      than resolving from a working directory the way the CLI does: an MCP tool call has no
      meaningful "current directory" of its own caller. `ServerHandler::get_info()` advertises
      `ProtocolVersion::V_2026_07_28` explicitly.
- [x] **#1 — Live end-to-end smoke test, done 2026-08-06.** Built `vxedged`/`vxedge-cli`
      release binaries, ran a real `vxedged` on a scratch Unix socket
      (`unix:///tmp/vxmcp-smoke.sock`, unprivileged, no gateway calls needed for this), pointed
      it at `vxperiments/experiments/exp-001-ws` (a real workspace, not a fixture). Confirmed
      with the CLI directly first (`vxedge experiment status` — `exp-001: 1 resource(s) /
      vxgateway [1] no gateway provisioned`), then drove `vxmcp` through
      `npx @modelcontextprotocol/inspector --cli` (the real MCP inspector, not a hand-rolled
      client): `tools/list` returns `experiment_status` with its real generated JSON Schema;
      `tools/call` against the live workspace returns `vxgateway [state=1] no gateway
      provisioned` — matches the CLI's own report, confirming the whole chain (MCP client →
      `vxmcp` → `vxedged` → real workspace) round-trips correctly. Also checked the error path
      (nonexistent workspace): propagates as a proper JSON-RPC error with `vxedged`'s own
      honest message (`cannot read manifest ... No such file or directory`), not a swallowed or
      generic failure.
      **Follow-up, resolved 2026-08-06 — protocol-version enforcement, done properly.** The
      inspector's client had negotiated down to `protocolVersion: "2025-11-25"` in an
      `initialize` handshake, initially logged above as a possible contradiction of this
      repo's "stateless, no handshake" claim. Root-caused instead of left as a flag:
      1. Read `rmcp` 3.1.1's own source
         (`~/.cargo/registry/.../rmcp-3.1.1/src/service/server.rs`,
         `negotiate_protocol_version`): the server's default `initialize` handler **echoes
         back whatever version the client requests**, as long as it's in
         `supported_protocol_versions()` — which defaults to *every* version the SDK knows
         (2024-11-05 through 2026-07-28). `VxmcpServer` only set `get_info()`'s protocol
         version, never narrowed the supported set, so any legacy client got served silently.
         Fixed: `ServerHandler::supported_protocol_versions()` now returns
         `&[ProtocolVersion::V_2026_07_28]` only (`crates/vxmcp/src/main.rs`) — a client
         requesting anything else gets `2026-07-28` back instead of an echo, per spec
         (its choice then is to accept or disconnect, not talk this server down).
      2. Fetched the actual spec text
         (`modelcontextprotocol.io/specification/2026-07-28/basic/lifecycle`) rather than
         trust `rmcp`'s API surface or the earlier flag: **the `initialize` handshake is
         explicitly scoped to "legacy" versions (2025-11-25 and earlier) only.** "Modern"
         (2026-07-28+) clients skip it entirely — every request carries `protocolVersion` and
         `clientCapabilities` in `_meta`, server accepts/rejects per request, no session, no
         negotiation. This repo's own "stateless, no handshake" claim was accurate all along;
         the earlier flag was a testing artifact (both the inspector and the first manual
         script used the legacy `initialize` entry point), not a doc bug.
      3. Verified all three cases live against a real `vxedged` + `exp-001-ws`, raw JSON-RPC
         over stdio (no inspector — v2.1.0 doesn't implement modern 2026-07-28 requests
         itself, confirmed by it rejecting the narrowed server's honest `2026-07-28` answer
         with "Server's protocol version is not supported"): (a) a legacy client requesting
         `2025-11-25` now gets refused down to `2026-07-28` and must disconnect — narrowing
         confirmed working; (b) a legacy client explicitly requesting `2026-07-28` completes
         the (legacy-shaped) handshake and gets correct live data back; (c) a genuine modern
         client — one `tools/call` request, no `initialize`, `_meta` carrying
         `io.modelcontextprotocol/protocolVersion` + `clientCapabilities` — gets served
         directly (`peer_info=None` in the log: no session ever created) with correct live
         data, `resultType: "complete"` confirming the 2026-07-28 wire shape is genuinely
         active, not just the version string. `vxmcp` now provably speaks — and only speaks —
         real MCP spec 2026-07-28, in both its legacy-compatible and modern-native forms.
### The open board — 4 items, in unblock order

| # | Task | State |
|---|---|---|
| 2 | Remaining read tools (`zone/compute list`, `job watch`, `events subscribe`, results-archive summary) | in progress — read tools done; results-archive summary open (needs a vxAPI RPC) |
| 3 | MRTR confirmation flow for mutating tools | in progress — guard + `svc_down` done; more tools one PR each; `experiment_down` waits on a pin bump to vxedge `e3753be` |
| 4 | Manifest JSON Schema integration (depends on `vxparts` `ctl`-class schema part) | open |
| 5 | Real deployment (systemd unit / process supervision) | open |

- [ ] **#2 — Read tools, the rest of the safe slice**: `zone/compute list`, `job watch`,
      `events subscribe`, a results-archive summary. Same shape as `experiment_status` — one
      module per vxAPI service under `crates/vxmcp/src/tools/`, one `#[tool]` method per RPC
      dispatching to it. No design question, no dependency on #3.
  - [x] **Unary read tools, 2026-10-28**: `gateway_status` (`GatewayService.Status`),
        `zone_list` (`ZoneService.List`), `compute_list` (`ComputeService.List`), `svc_status`
        (`SvcService.Status`), `job_get` (`JobService.Get`). Checked against the pinned
        `vxedge-api` rev's protos and `vxedged`'s own `add_service` list before wiring
        each one, not assumed from the proto alone. `job_get` is the one-snapshot way to
        check on a job; `job_watch` below follows one. Output keeps `experiment_status`'s
        `[state=N]` ordinal convention for resource states; job states are named, matching
        `vxedge-cli`'s own `state_name()` — the one place the CLI already prints names.
        `svc_status` has no CLI verb yet (the CLI only wraps `SvcService.Up`/`Down`) but the
        daemon serves it; it sends `service_type` only, since that's the one field `vxedged`
        reads, and keeps the accepted values out of a schema enum that could drift from the
        daemon's own list. Found live (2026-10-28): `vxedged` then only rejected an *empty*
        `service_type` — an unknown one (`bogus`) answered `svc-bogus [state=1] bogus service
        not running` instead of an error. **Fixed daemon-side 2026-10-29** (vxedge PR #21,
        `a78e7dd`): `SvcService` Status/Up/Down now answer an unknown type with
        `INVALID_ARGUMENT` naming the accepted ones, and this repo's pin was bumped to that
        commit, with the tool's doc updated to match. A daemon older than `a78e7dd` still
        shows the old behaviour.
  - [x] **Live smoke test, 2026-10-28** — same method as #1, but on Linux in Docker
        (`rust:1.96.1-bookworm`, since `vxedged` uses netlink and doesn't build on macOS):
        `vxedged` + `vxedge-cli` built at the pinned rev, `vxmcp` at this branch, `vxedged`
        unprivileged on a scratch Unix socket, `vxperiments/experiments/exp-001-ws` mounted
        read-only. Every call sent as a modern 2026-07-28 request (no `initialize`,
        protocol version in `_meta`), each compared against the CLI's own answer:
        `gateway_status` → `vxgateway [state=1] no gateway provisioned` (matches
        `vxedge status`); `zone_list`/`compute_list` → JSON-RPC error carrying `vxedged`'s
        own `FailedPrecondition` "no active experiment … bring one up first" (identical to
        `vxedge zone/compute list`'s error — nothing is up, and bringing one up is a
        mutating call, so the non-empty and empty-list paths stay covered by unit tests
        only); `svc_status` `ota`/`telemetry` → `[state=1] … service not running`;
        `job_get` on an unknown id → JSON-RPC error with `vxedged`'s own `NotFound` "unknown job" message;
        `experiment_status` still matches `vxedge experiment status`.
  - **Not wired, deliberately: `vehicle_status`.** `VehicleService.Status` exists in the
    proto, but `vxedged` never registers a `VehicleService` (`vxedged/src/main.rs`'s
    `add_service` list, pinned rev) — a tool would only ever return `Unimplemented`. Wire it
    when the daemon serves it.
  - [x] **`job_watch` / `events_subscribe`, 2026-10-29** — the two server-streaming RPCs
        (`JobService.Watch`, `EventService.Subscribe`). Decided: collect, then return once —
        an MCP tool call returns a single result, and this works with every client (progress
        notifications don't render everywhere; poll-only would have left events with no tool).
        `job_watch` follows the job until a terminal state or a caller-chosen timeout
        (default 60s, cap 300s), returning the distinct progress steps (deduped the same way
        `vxedge-cli`'s `watch_job()` does) plus the final snapshot; a timeout says so and
        leaves re-watching to the agent. `events_subscribe` collects live events for a window
        (default 10s, cap 60s, stops at 200) with an optional `kinds` filter passed through
        to the daemon — the bus keeps no history, so only events emitted during the call
        come back. Collection is generic over the stream, so the terminal / timeout / cap /
        early-end paths are unit-tested without a daemon.
        **Live smoke test, 2026-10-29**, Linux in Docker, modern 2026-07-28 requests. The
        daemon was the `vxedged` + `vxedge-cli` build from `vxedge` PR #21's branch (current
        `main` + the `svc_status` fix, already built in another session's volume and mounted
        read-only — the Docker VM's disk was too full to rebuild at the pinned rev), so it is
        newer than this repo's pinned `vxedge-api`; Job and Event messages are unchanged
        between the two. `events_subscribe` (6s) caught all three live `job.state` events —
        `pending` → `running` → `failed` — for a real job started meanwhile by
        `vxedge ota up` in `exp-003-ws` (fails cleanly: no `vxota` binary in the container).
        `job_watch` on that job returned `[ 30%] svc.up starting ota service` plus the
        final `failed` snapshot and its error, matching the CLI's own output line for
        line; `job_get` returned the same snapshot. Unknown id → `vxedged`'s `NotFound`; a
        `kinds` filter matching nothing → `no events in 2s`. Not exercised live: the
        timeout-while-running path (that job failed instantly) — unit-tested only.
  - [ ] **Results-archive summary — no RPC behind it.** No vxAPI service exposes the results
        archive, so a tool here would break this repo's one-tool-per-RPC hard rule. Needs a
        vxAPI RPC in `vxedge` first, or drop it from this list.
- [ ] **#3 — MRTR confirmation flow for mutating tools** (`up`/`down`/`traffic up`). **Guard
      built 2026-10-29** (`crate::confirm`, design in
      `docs/superpowers/specs/2026-10-29-mrtr-confirmation-design.md`) with `svc_down` as the
      first tool; each further mutating tool is its own small PR. **`experiment_down` waits**
      on a pin bump: at the pinned `a78e7dd`, Down tears down whichever experiment is active,
      not the one named. vxedge PR #24 (merged 2026-10-29 as `e3753be`) makes it refuse
      instead, so bump the pin to `e3753be` or later first. Original note: the design
      point this repo's own `AGENTS.md` states but hasn't built: return `InputRequiredResult`
      with an `InputRequest::Elicitation` asking the caller to confirm, verify the echoed
      `requestState` (caller-controlled input) before trusting it, complete on the retry. See
      `rmcp`'s own `examples/servers/src/mrtr.rs` for the reference pattern. Needs a design
      pass on exactly what confirmation text/schema each mutating tool asks for before wiring
      the RPCs themselves — not assumed here, and not started.
  - [x] **Live smoke test of `svc_down`, 2026-10-29** — Linux in Docker (`rust:1.96.1-bookworm`),
        `vxedged` + `vxedge-cli` built at the pinned `a78e7dd`, one long-lived `vxmcp` process
        driven by hand over stdio with modern 2026-07-28 requests (`clientCapabilities:
        {"elicitation": {"form": {}}}`). In order: round 1 → `input_required` with one form
        elicitation (`confirm`) carrying the exact confirmation text and a 32-char ticket;
        retry with `accept` → `job-00000000 svc.down: pending [0%]`; the same ticket again →
        `-32602 confirmation is invalid or expired; call the tool again to be asked`; a fresh
        round 1 + `decline` → `not confirmed; nothing was run`; `clientCapabilities: {}` →
        `-32021 Missing required client capability`; `service_type: bogus`, accepted → the
        daemon's own `INVALID_ARGUMENT` "unknown service_type `bogus` (expected "ota" or
        "telemetry")" as a JSON-RPC error (the human is asked before the daemon validates; the
        ticket is spent either way); `job_watch` on the job → `succeeded [100%] ota service
        down`.
  - **Client check, 2026-10-29 — Claude Code (from its docs, not tested live).** Per
        `code.claude.com/docs/en/mcp.md` ("MCP client runtimes", "Respond to MCP elicitation
        requests", "MCP tasks …"): its v2 runtime speaks 2026-07-28 and declares
        `elicitation: {form: {}, url: {}}`, shows form elicitations as a dialog, but **does not
        document SEP-2322 `input_required` results on `tools/call`** (no `InputRequiredResult`,
        `requestState` or retry with `inputResponses`). What it documents instead: the MCP
        *tasks* extension (`resultType: "task"`, questions answered via `tasks/update`) and
        server-initiated `elicitation/create` on an open session, both in interactive
        terminal sessions only. So with Claude Code as it's documented today, `svc_down`
        is asked for confirmation but the client can't complete the round trip — safe
        (nothing runs without an `accept`), but unusable there. Per the design (decision 6),
        whether to add a second mechanism is the owner's call; not built.
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
