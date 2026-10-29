# Human Confirmation for Mutating Tools: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A reusable human-confirmation guard (MCP 2026-07-28 MRTR elicitation) and the first mutating tool built on it, `svc_down`.

**Architecture:** A new `crate::confirm` module owns a single-use, in-memory ticket map and one decision function, `Confirmations::gate`. A mutating tool calls `gate` with the client's capabilities, its own name, its resolved arguments, the confirmation text and the retry's `requestState`/`inputResponses`. `gate` answers *ask* (return `InputRequiredResult`), *proceed* (make the one RPC) or *not confirmed*. `svc_down` is then a thin tool: a fixed text, one `gate` call, one `SvcService.Down` call, and the returned `Job` rendered with `tools::job`'s renderer.

**Tech Stack:** Rust 2024, `rmcp` 3.1.1 (`transport-io`, default features), `tonic` 0.14, `vxedge-api` at `a78e7dd`, `getrandom` 0.4 (already in the tree).

**Spec:** `docs/superpowers/specs/2026-10-29-mrtr-confirmation-design.md` (approved 2026-10-29).

## Global Constraints

- Only a human confirms. No tool argument may skip or pre-answer the question.
- Fail closed: no form-elicitation capability → refuse with `McpError::missing_required_client_capability`.
- No unattended/CI mode.
- Ticket TTL: **5 minutes**. Single use: removed on the first retry that presents it, whatever the outcome.
- `requestState` is only ever a map key, never parsed for meaning. No signing (stdio, one process per client).
- A confirmed tool makes **one** vxAPI RPC and returns the `Job` without waiting for it.
- Confirmation text describes what the RPC really does (checked against `vxedged`), and ends with the fixed line `This affects anyone using this host's testbed. Confirm?`
- Keep `cargo build/test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` and `cargo deny check` clean. Locally, `PROTOC=/Users/fouzi/miniconda3/lib/python3.9/site-packages/torch/bin/protoc` (libprotoc 3.13) builds `vxedge-api`.
- Work in a separate git worktree on branch `feat/confirm-svc-down`, cut from `origin/docs/mrtr-confirmation-design` so the spec and this plan land in the same PR. Never in the shared `/Users/fouzi/github/vxedge/vxmcp` checkout.

## Review Focus

1. **A retry after vxmcp restarted** (ticket map empty): expect the "confirmation is invalid or expired, call the tool again" error, never a panic. Pinned by `unknown_ticket_is_refused` (Task 1).
2. **`inputResponses["confirm"]` is not a valid elicitation result** (malformed JSON, wrong shape): expect the same invalid-confirmation error, never an internal error. Pinned by `malformed_confirm_response_is_refused` (Task 1).
3. **An older client declaring `elicitation: {}`** (neither `form` nor `url`): per spec that means form support, so it must be asked, not refused. Pinned by `empty_elicitation_capability_counts_as_form` (Task 1).
4. **A client that declares only URL elicitation, or no capabilities at all** (`None`): expect the capability error. Pinned by `url_only_or_missing_capabilities_are_refused` (Task 1).
5. **The daemon rejects the call after the human confirmed** (e.g. unknown `service_type` → `INVALID_ARGUMENT` since `a78e7dd`): the error reaches the agent as a JSON-RPC error, and the ticket is gone, so it must confirm again. Pinned by the live test's `bogus` step (Task 3).

---

### Task 1: The confirmation guard (`crate::confirm`)

**Files:**
- Create: `crates/vxmcp/src/confirm.rs` (module plus its `#[cfg(test)] mod tests`)
- Modify: `crates/vxmcp/src/main.rs` (add `mod confirm;`)
- Modify: `Cargo.toml` (workspace dep `getrandom = "0.4"`), `crates/vxmcp/Cargo.toml` (`getrandom = { workspace = true }`), `Cargo.lock`

**Interfaces:**
- Produces:
  - `pub const CONFIRM_KEY: &str = "confirm";`
  - `pub const TICKET_TTL: std::time::Duration` = 300 s
  - `pub enum Gate { Ask(rmcp::model::InputRequiredResult), Proceed, NotConfirmed(rmcp::model::CallToolResult) }`
  - `pub struct Confirmations` with `pub fn new() -> Self` (also `Default`)
  - `pub fn gate(&self, caps: Option<&rmcp::model::ClientCapabilities>, tool: &'static str, args: &serde_json::Value, message: &str, request_state: Option<String>, input_responses: Option<rmcp::model::InputResponses>) -> Result<Gate, rmcp::ErrorData>`, delegating to a private `gate_at(&self, now: std::time::Instant, …same…)` so tests control time
  - Error text (exact): invalid confirmation → `McpError::invalid_params("confirmation is invalid or expired; call the tool again to be asked", None)`; declined/cancelled → `Gate::NotConfirmed(CallToolResult::success(vec![ContentBlock::text("not confirmed; nothing was run")]))`

- [ ] **Step 1: Write the failing tests** in `confirm.rs`. Helpers: `fn form_caps() -> ClientCapabilities` (elicitation with `form: Some(FormElicitationCapability::new())`), `fn accept() -> InputResponses` (`{CONFIRM_KEY: {"action":"accept"}}`), `fn answer(action: &str) -> InputResponses`. A round-1 call is `gate(Some(&form_caps()), "svc_down", &json!({"service_type":"ota"}), "Stop …", None, None)`; take the ticket from the returned `Gate::Ask(r)` as `r.request_state.unwrap()`. Tests:
  - `round_one_asks_with_one_form_elicitation_and_a_ticket`: `Gate::Ask`. `input_requests` has exactly key `"confirm"`, holding an `InputRequest` elicitation whose form message equals the passed text. `request_state` is `Some`, at least 32 chars.
  - `accept_with_the_same_call_proceeds`: round 1, then the same tool + args with that ticket and `accept()` → `Gate::Proceed`.
  - `decline_and_cancel_do_not_run`: `answer("decline")` and (from a fresh round 1) `answer("cancel")` → `Gate::NotConfirmed`, text `not confirmed; nothing was run`.
  - `a_ticket_works_once`: accept → `Proceed`, the same ticket + accept again → `Err` with the invalid-confirmation message.
  - `expired_ticket_is_refused`: round 1 via `gate_at(t0, …)`, retry via `gate_at(t0 + TICKET_TTL + 1s, …)` → `Err`.
  - `ticket_for_another_tool_or_other_args_is_refused`: a ticket from `"svc_down"`/`ota`, retried as `"svc_up"`/`ota` → `Err`. A fresh one retried with `{"service_type":"telemetry"}` → `Err`.
  - `unknown_ticket_is_refused`: `request_state: Some("not-a-ticket")` + `accept()` → `Err`.
  - `missing_confirm_response_is_refused`: valid ticket, `input_responses: None` → `Err`. Valid ticket, responses without the `"confirm"` key → `Err`.
  - `malformed_confirm_response_is_refused`: valid ticket, `{"confirm": "yes"}` and `{"confirm": {"action": 7}}` → `Err` (invalid-confirmation, not internal).
  - `empty_elicitation_capability_counts_as_form`: caps with `elicitation: Some(ElicitationCapability::new())` (both fields `None`) → `Gate::Ask`.
  - `url_only_or_missing_capabilities_are_refused`: caps `None`, caps without `elicitation`, and caps with only `url` → `Err` whose code is `ErrorCode::MISSING_REQUIRED_CLIENT_CAPABILITY`.

- [ ] **Step 2: Run to verify failure**
  Run: `PROTOC=… cargo test -p vxmcp confirm::`
  Expected: compile errors, `cannot find … Confirmations`/`Gate`.

- [ ] **Step 3: Implement `crate::confirm`**
  - Map: `std::sync::Mutex<HashMap<String, Ticket>>`, `struct Ticket { tool: &'static str, args: serde_json::Value, expires_at: Instant }`. No lock is held across an `.await` (`gate` is sync).
  - Ticket id: 16 bytes from `getrandom::fill`, lowercase hex (32 chars). On a `getrandom` error, return `McpError::internal_error`.
  - Purge expired tickets on each insert.
  - Capability rule: proceed to ask only if `caps.elicitation` is `Some(e)` and (`e.form.is_some()` or (`e.form.is_none() && e.url.is_none()`)). Otherwise return `missing_required_client_capability` with a `ClientCapabilities` whose `elicitation` is `Some(ElicitationCapability { form: Some(FormElicitationCapability::new()), url: None })`, built via `default()` + field assignment (types are `#[non_exhaustive]`).
  - Round 1 = `request_state.is_none()`: capability check, insert the ticket, return `Gate::Ask(InputRequiredResult::new(Some(map), Some(ticket)))`. `map` has `CONFIRM_KEY` → `InputRequest` elicitation built from `ElicitRequest::new(ElicitRequestParams::FormElicitationParams { meta: None, message, requested_schema: ElicitationSchema::new(BTreeMap::new()) })`.
  - Round 2: `remove` the ticket first. Then check expiry, `tool ==`, `args ==`, then parse `input_responses[CONFIRM_KEY]` with `serde_json::from_value::<ElicitResult>`. `Accept` → `Proceed`, `Decline`/`Cancel` → `NotConfirmed`. Any failure → the invalid-confirmation error. Skip the capability check on round 2, since the client already answered.

- [ ] **Step 4: Run to verify pass**
  Run: `PROTOC=… cargo test -p vxmcp confirm::` → all 11 pass. Then `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`: clean. (`mod confirm` is unused until Task 2. If clippy flags dead code, add `#[cfg_attr(not(test), allow(dead_code))]` on the module declaration and remove it in Task 2.)

- [ ] **Step 5: Commit**
  `git add Cargo.toml Cargo.lock crates/vxmcp/Cargo.toml crates/vxmcp/src/confirm.rs crates/vxmcp/src/main.rs && git commit -m "feat: confirmation guard for mutating tools (TASKS #3)"`

### Task 2: `svc_down`, the first guarded tool

**Files:**
- Modify: `crates/vxmcp/src/tools/svc.rs` (args, text, `down`), `crates/vxmcp/src/tools/job.rs` (make `render` `pub(crate)`), `crates/vxmcp/src/main.rs` (state field, tool method, instructions, module doc)

**Interfaces:**
- Consumes: `confirm::{Confirmations, Gate}` and `Confirmations::gate` (Task 1).
- Produces:
  - `pub struct SvcDownArgs { pub service_type: String }` (serde + schemars, doc: "Which experiment service to stop on this host: currently `ota` or `telemetry`. You will be asked to confirm.")
  - `pub fn down_confirmation(service_type: &str) -> String`
  - `pub async fn down(channel: tonic::transport::Channel, args: SvcDownArgs) -> anyhow::Result<CallToolResult>`
  - `VxmcpServer.confirmations: std::sync::Arc<confirm::Confirmations>`
  - Tool `svc_down` → `Result<rmcp::model::CallToolResponse, McpError>`

- [ ] **Step 1: Write the failing tests** in `svc.rs`'s test module:
  - `down_args_deserialize_from_a_bare_service_type_field`: `{"service_type":"ota"}` → `SvcDownArgs { service_type: "ota" }`.
  - `down_confirmation_names_the_service_and_the_shared_host`: `down_confirmation("ota")` equals exactly
    `"Stop the `ota` service on this host, whichever experiment started it. This affects anyone using this host's testbed. Confirm?"`

- [ ] **Step 2: Run to verify failure**
  Run: `PROTOC=… cargo test -p vxmcp svc::` → compile errors for `SvcDownArgs`/`down_confirmation`.

- [ ] **Step 3: Implement**
  - `svc.rs`: `down_confirmation` returns the exact string above. `down` sends `SvcDownRequest { service_type, experiment: None }` (the daemon ignores `experiment` for Down) and returns `CallToolResult::success` with `tools::job::render(&job)`. Extend the module doc: `svc_down` wraps `SvcService.Down`, which reads only `service_type` and stops that service on the host.
  - `main.rs`: add `confirmations: Arc<Confirmations>` to `VxmcpServer`, initialised in `new`. Tool method:
    `async fn svc_down(&self, Parameters(args): Parameters<SvcDownArgs>, RequestState(state): RequestState, InputResponses(responses): InputResponses, ctx: RequestContext<RoleServer>) -> Result<CallToolResponse, McpError>`.
    It calls `gate(ctx.client_capabilities().as_ref(), "svc_down", &json!({"service_type": args.service_type}), &down_confirmation(&args.service_type), state, responses)?`. `Ask(r)` → `Ok(r.into())`, `NotConfirmed(r)` → `Ok(r.into())`, `Proceed` → `tools::svc::down(...)`, with errors mapped like the other tools (`McpError::internal_error(e.to_string(), None)`), then `.into()`.
    Description: `"Stop an experiment service (`ota` or `telemetry`) on this host. Asks the human user to confirm first; fails if the client can't ask. Returns the job; follow it with job_watch."`
    Set `annotations(destructive_hint = true)` if the `#[tool]` macro accepts it in 3.1.1. If not, skip it (optional per spec).
  - Instructions string and module doc: add `svc_down` with "(mutating, asks the user to confirm)".

- [ ] **Step 4: Run to verify pass**
  `cargo test --workspace` (all pass, including the 13 new), clippy, fmt: clean. Then the modern `tools/list` check from PR #2 (placeholder socket, short relative path `f.sock` because of macOS's socket-path length limit): `svc_down` is listed with `required: ["service_type"]`, and its schema has no confirm-like property.

- [ ] **Step 5: Commit**
  `git commit -am "feat: svc_down, first tool behind human confirmation"` (add `svc.rs`/`job.rs`/`main.rs`)

### Task 3: Client check and live smoke test

**Files:**
- Modify: `TASKS.md` (record both results under #3)

**Interfaces:**
- Consumes: the built `vxmcp` from Task 2.

- [ ] **Step 1: Client check.** Find out, from Claude Code's own documentation (the claude-code-guide agent can look it up), whether Claude Code supports (a) MCP protocol 2026-07-28 requests and (b) form-mode elicitation via `InputRequiredResult`. Record the answer with its source in `TASKS.md` #3. It doesn't block the PR (spec decision 6).

- [ ] **Step 2: Build for the live test** in Docker `rust:1.96.1-bookworm` with `protobuf-compiler` and `python3`: `vxedged` + `vxedge-cli` at `vxedge` rev `a78e7dd` (read-only mount of `~/.cargo/git/checkouts/vxedge-*/a78e7dd`), and `vxmcp` from the worktree. The Docker VM disk is nearly full: set `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`, and check `df -h /` in a container first (needs about 3 GB free).

- [ ] **Step 3: Run a hand-driven MRTR exchange** (one long-lived `vxmcp` process over stdio, modern requests with `_meta` protocol version `2026-07-28` and `clientCapabilities: {"elicitation": {"form": {}}}`, against `vxedged` on a short socket path). Expected, in order:
  1. `svc_down {"service_type":"ota"}`, no state → `resultType: "input_required"` with `inputRequests.confirm` (form, the exact text) and a `requestState`.
  2. Same call + that `requestState` + `inputResponses: {"confirm": {"action":"accept"}}` → `resultType: "complete"`, text starting `job-` and containing `svc.down`.
  3. Repeat step 2 with the same `requestState` → JSON-RPC error `confirmation is invalid or expired; call the tool again to be asked`.
  4. A fresh round 1, then `{"action":"decline"}` → complete, text `not confirmed; nothing was run`.
  5. `clientCapabilities: {}` → JSON-RPC error code `MISSING_REQUIRED_CLIENT_CAPABILITY`.
  6. `{"service_type":"bogus"}`: round 1 asks, accept → JSON-RPC error carrying `vxedged`'s `INVALID_ARGUMENT` "unknown service_type" (Review Focus 5).
  7. `job_watch` on the job id from step 2 → a terminal state.

- [ ] **Step 4: Record results** in `TASKS.md` #3, same style as the PR #2/#3 entries. Remove the containers, volumes and image you created.

- [ ] **Step 5: Commit**
  `git commit -am "tasks: record client check and live test of svc_down confirmation"`

### Task 4: Docs and PR

**Files:**
- Modify: `AGENTS.md` (replace the "Read tools first, mutating tools once MRTR is reviewed" section), `TASKS.md` (#3 state, plus the `experiment_down` gap → vxedge/vxedge#24), `CHANGELOG.md` (`Added`: guard + `svc_down`), `README.md` (status line)

- [ ] **Step 1: AGENTS.md.** The new section states:
  - The guard exists (`crate::confirm`), and every mutating tool must go through `Confirmations::gate`. No argument may bypass it.
  - Fail closed without form elicitation. No unattended mode.
  - Confirmation text must describe what `vxedged` really does, checked against its code, and end with the fixed shared-host line.
  - Each mutating tool returns the `Job` and doesn't wait.
  - `experiment_down` waits on vxedge/vxedge#24.
- [ ] **Step 2: TASKS.md, CHANGELOG.md, README.md** updated as listed above. The README read-only tool list gains "and `svc_down` (asks the user to confirm)".
- [ ] **Step 3: Verify:** `cargo test --workspace`, clippy, fmt all clean. `grep -n "MRTR is reviewed" AGENTS.md` returns nothing.
- [ ] **Step 4: Commit, push and open the PR** on `vxedge/vxmcp` (REST API, since `gh pr edit` hits the classic-Projects bug). The body summarises the guard, `svc_down`, both test layers and the client-check result.
