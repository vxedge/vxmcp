# AGENTS.md — vxedge/vxmcp

An MCP server exposing vxAPI to AI agent tooling (Claude Code and others) — the agent-tooling
equivalent of the `vxedge` CLI: a thin, 1:1 client over the same RPCs, addressed by an LLM
agent's tool-call surface instead of a terminal. Master spec: `../docs/ARCHITECTURE.md` §5.1
(frontends), §15 (phase 3, language decision). Phase 3, not gate-blocking.

## Hard rule (same one every other frontend follows)

**No orchestration logic lives here.** Every MCP tool is a direct wrapper over exactly one
vxAPI RPC (`GatewayService`/`ZoneService`/`ComputeService`/`ExperimentService`/
`TrafficService`/`SvcService`/`JobService`/`EventService`) — never a multi-step workflow baked
into the server. If a capability needs multiple RPCs sequenced with judgment calls in between,
that sequencing belongs to the agent calling the tools, not to a tool itself. This is the same
rule `vxedge-cli`'s own `AGENTS.md` states for the CLI; `vxmcp` doesn't get an exception for
being agent-facing rather than human-facing.

## Layout

```
crates/
  vxmcp/           # the server binary + its tools
    src/
      main.rs      # transport setup (stdio today), ServerHandler impl, get_info()
      tools/       # one module per vxAPI service; #[tool_router] impl blocks
```

Single crate for now — a `vxmcp-common` split is premature until there's something to share
that a second crate would actually need (`rust-patterns`: three similar lines beats a
speculative abstraction).

## Dependency on vxAPI

`vxedge-api` (generated tonic bindings, `vxedge/crates/vxedge-api`) is a **git dependency
pinned to a commit** (`Cargo.toml`), not a path dependency — `vxmcp` is a separate repo, not a
member of `vxedge`'s own Cargo workspace. Bump the pinned `rev` deliberately when vxAPI changes
land that this repo needs; never track a branch. **Never hand-edit generated code, and never
re-derive the RPC surface** — `vxedge-api` is the one and only source of it, the same rule
`vxedge/AGENTS.md` states for every other consumer.

Connecting to `vxedged` reuses the exact same Unix-socket dance `vxedge-cli` already proved
(`tonic::transport::Endpoint` + a `tower::service_fn` connector over `tokio::net::UnixStream`)
— copy that pattern, don't invent a second one.

## MCP spec target: 2026-07-28, on `rmcp` >=3.1.1

Decided 2026-08-06 (`docs/ROADMAP.md`, full account there — checked `rmcp`'s own release
history before committing, not assumed). Stateless request-response, no `initialize`/
`initialized` handshake — `ServerHandler::get_info()` sets
`ProtocolVersion::V_2026_07_28` explicitly. **Multi Round-Trip Requests** are the mechanism
for a mutating tool that needs its caller to confirm before running: return
`InputRequiredResult` with an `InputRequest::Elicitation`, verify the echoed `requestState`
before trusting it (it's caller-controlled input, same trust boundary as any other request
field), then complete on the retry. That's built as `crate::confirm` — use it, don't
re-implement it per tool (see "Mutating tools" below).

**The "no `initialize` handshake" claim above is specifically about *modern* clients** —
confirmed against the spec text itself
(`modelcontextprotocol.io/specification/2026-07-28/basic/lifecycle`), not just `rmcp`'s API
surface, after a live test briefly looked like it contradicted this (`vxmcp/TASKS.md` #1's
follow-up, 2026-08-06). The spec keeps `initialize` as a *legacy* (2025-11-25-and-earlier)
mechanism for backward compatibility; a modern client instead carries `protocolVersion` and
`clientCapabilities` in every request's `_meta` and gets served with no session at all. Two
things follow, both already true of `VxmcpServer`, kept true deliberately rather than by
accident:
- **`ServerHandler::supported_protocol_versions()` must stay narrowed to
  `&[ProtocolVersion::V_2026_07_28]`.** `rmcp`'s own default returns *every* protocol version
  the SDK knows, so its default `initialize` handler will echo back whatever version a legacy
  client requests — found live when a real MCP inspector client silently negotiated down to
  `2025-11-25`. Narrowing it means a legacy client instead gets an honest `2026-07-28` answer
  and must accept it or disconnect, never talk this server down to an older revision.
- A legacy-flow client (one that still opens with `initialize`) is not thereby unsupported —
  `rmcp` answers it correctly once the version above is pinned — but the modern, handshake-free
  path is the one this design is actually built for. Don't take a working `initialize`-based
  test as proof of "no handshake"; verify the handshake-free path directly (one request, no
  `initialize`, protocol version in `_meta`) when spec conformance is the thing being checked.

## Mutating tools: human confirmation first, always

Design: `docs/superpowers/specs/2026-10-29-mrtr-confirmation-design.md` (owner-approved
2026-10-29). Read tools carry no question; every **mutating** tool goes through
`crate::confirm` before its one RPC, so an agent can't chain them unsupervised into a
destructive sequence (the CLI's own explicit verbs are the parity bar).

- **Every** mutating tool calls `Confirmations::gate` with its own name, its *resolved*
  arguments and its confirmation text, and runs the RPC only on `Gate::Proceed`. No exceptions
  for "harmless" ones, and **no argument may skip or pre-answer the question** — only a human
  confirms.
- **Fail closed**: no form-elicitation capability → `MISSING_REQUIRED_CLIENT_CAPABILITY`, never
  a silent run. No unattended/CI mode; if one is ever added it's an operator launch setting,
  off by default, never reachable through a tool call.
- **Confirmation text says what `vxedged` actually does**, checked against its implementation,
  not the RPC's name. Example: at the pinned rev, `ExperimentService.Down` tears down whichever
  experiment is active, not the one named. A fix is proposed in vxedge PR #24 (open, not merged
  as of 2026-10-29); **don't build `experiment_down` until it merges and the pin is bumped**.
  Every text ends with `This affects anyone using this host's testbed. Confirm?`
- A confirmed tool returns the `Job` immediately; the agent follows it with
  `job_watch`/`job_get`. One RPC per tool still holds.
- `svc_down` is the reference implementation.

## Rules

- Rust only, matching `docs/ARCHITECTURE.md` §15's language policy for control-plane-adjacent
  services — this is that, even though its caller is an LLM rather than a human.
- `cargo build --workspace` / `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`
- `cargo deny check` (advisories/licenses/sources) — the `vxedge-api` git dependency needs an
  `allow-git` entry in `deny.toml`; don't drop it when bumping the pinned rev.
- Smoke test: `cargo run -p vxmcp` against a running `vxedged --listen unix:///tmp/vxedge.sock`;
  `npx @modelcontextprotocol/inspector cargo run -p vxmcp` for interactive tool inspection.

## Project skills

`.claude/skills/`: none vendored yet — `vxedge`'s own `vxapi-protobuf-design` skill is the
relevant one for anything touching the RPC surface this server wraps; read it from that repo
until this one earns its own.
