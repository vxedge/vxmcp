# Human confirmation for mutating tools (TASKS.md #3)

Status: approved by the owner, 2026-10-29. Supersedes the open-ended wording of `TASKS.md` #3.

## Why

`vxmcp` exposes only read tools so far. `AGENTS.md` blocks any mutating vxAPI RPC
(`up`/`down`/`traffic up`, …) from becoming a tool until a confirmation step exists, so an agent
can't chain them unsupervised. The owner's concrete fears are **losing data**, **disturbing
other people using the same environment**, and **causing a crash**. The guard exists to put a
human in front of each of those moments. It must not grow into a policy engine.

## Decisions (made by the owner)

1. **Only a human confirms.** The tool asks a question that the MCP client shows to its user.
   There is no argument (`confirm: true` or similar) the agent can set to skip it.
2. **Fail closed.** If the client cannot ask its user (no elicitation capability), every
   mutating tool refuses. It never proceeds silently.
3. **No unattended / CI mode for now.** If one is ever added, it is an operator-level launch
   setting, off by default, never something reachable through a tool call.
4. **Keep it simple**, specifically:
   - **Every** mutating tool gets the guard, `up` as well as `down`. No per-tool exceptions.
   - The question shows **exactly what is about to run**: tool name, resolved target, one fixed
     sentence on what the RPC does. Nothing is fetched to build it: no status lookup, no
     "archived?" check. That would mean a second RPC inside one tool (breaking the
     one-tool-per-RPC hard rule), and vxAPI has no archive status anyway. An agent that wants
     context calls the read tools first.
   - The retry is trusted through a **single-use, in-memory ticket** bound to the exact call.
5. **The first guarded tool is `svc_down`, not `experiment_down`** (decided after review; see
   "Known daemon gap" below).
6. **"Done" means a hand-driven MRTR exchange passes.** The first step of the work checks
   whether the owner's everyday MCP client can do 2026-07-28 MRTR form elicitation. If it
   can't, the tool still ships fail-closed and the owner decides then whether a second
   mechanism is worth adding. No fallback is built up front.

## Accepted limit

`vxmcp` cannot prove a human answered. A client could auto-accept. This guards against an
agent chaining mutating calls on its own initiative, not against a hostile client. That
matches the CLI-parity bar in `AGENTS.md`.

## The flow

Uses the MCP 2026-07-28 multi round-trip request (MRTR) mechanism as `rmcp` 3.1.1 implements
it.

**Round 1** (the call arrives with no `requestState`):
1. Check the client can elicit: `RequestContext::client_capabilities()` (works for both
   modern per-request `_meta` and legacy sessions). It needs form-mode elicitation. If missing,
   return `McpError::missing_required_client_capability(<caps with elicitation>)`, so the
   agent sees a precise reason.
2. Resolve the target the same way the RPC call will. For `svc_down` that is just the
   `service_type` string. For later tools that take a workspace, canonicalize the path exactly
   as `tools::experiment` does today.
3. Create a ticket: a random, unguessable id → `{ tool name, resolved arguments, expires_at }`,
   stored in a map held by `VxmcpServer`. TTL **5 minutes**.
4. Return `InputRequiredResult` with one form-mode `elicitation/create` request (key
   `confirm`) whose message is the confirmation text, and `requestState` = the ticket id.

**Round 2** (the client retries the same call with `requestState` + `inputResponses`):
1. **Remove** the ticket from the map (single use, whatever happens next).
2. Refuse unless all hold: the ticket existed, it hasn't expired, its tool name and resolved
   arguments equal this call's, and `inputResponses["confirm"]` is an elicitation result with
   `action: accept`. `decline`/`cancel` gets a plain "not confirmed, nothing was run" result.
   Any other failure (unknown, expired or mismatched ticket, missing response) is an error
   saying the confirmation is invalid and to call again.
3. Only then make the one vxAPI RPC, and return the `Job` it gives back as text (id, kind,
   state, using `tools::job`'s rendering). The tool does **not** wait for the job. The agent
   follows it with `job_watch`/`job_get`, which keeps the tool a single RPC.

The `requestState` value is caller-controlled input. It is only ever used as a map key and is
never parsed for meaning, so no signing is needed while each client has its own long-lived
stdio `vxmcp` process. *(If a remote/HTTP transport arrives (Phase 5, out of scope), tickets
must move to `rmcp`'s `RequestStateCodec`, which signs, binds and expires them, plus a
used-ticket set.)*

## Confirmation text

One fixed template per tool, filled with the resolved target. Plain, specific, no fetched
data, and it must describe **what the RPC actually does**, checked against `vxedged`'s
implementation, not the RPC's name. For `svc_down`:

> Stop the `ota` service on this host, whichever experiment started it. This affects anyone
> using this host's testbed. Confirm?

(`SvcService.Down` reads only `service_type` and stops that service on the host via the
daemon's runtime dir. It does not look at any workspace.) The "affects anyone using this host"
line is a fixed reminder, not a check. It reflects the owner's shared-environment concern.

## Known daemon gap: `experiment_down` waits

Found in review against `vxedged` at the pinned rev (`crates/vxedged/src/experiment.rs`,
`down()`): `ExperimentService.Down` stops the named workspace's **services**, but its zones
stage lists and stops **every zone of whichever experiment is active on the host**
(`stage_zones` → `ZoneService.List` with no filter), and its gateway stage brings down the
host's one gateway. A confirmation naming one workspace could therefore tear down someone
else's active experiment. `experiment_down` is not built until `vxedged` makes Down act only
on the named workspace, or refuse when it isn't the active one. That's a `vxedge` change plus a
pin bump, tracked in `TASKS.md`.

## Scope of the first PR

- The guard as a small reusable piece (e.g. `crate::confirm`) that any mutating tool calls,
  so each tool adds only its template and its RPC call.
- **One** tool built on it: `svc_down` (`SvcService.Down`, a single `service_type` argument
  like `svc_status`). Its effect is exactly what its confirmation says, and it can be tested
  live safely.
- Update `AGENTS.md` (the guard now exists, every mutating tool must use it, and confirmation
  text must match the daemon's real behaviour) and `TASKS.md` (#3 progress, plus the
  `experiment_down` daemon gap above).
- Each further mutating tool is its own small PR afterwards.

## Testing

- Unit tests for the guard without a daemon: no-capability → refused; round 1 → ticket +
  elicitation; accept → runs; decline/cancel → not run; reused ticket, expired ticket, ticket
  from a different tool or different arguments, missing/forged `requestState` → refused.
- **First:** check whether the owner's everyday MCP client (presumably Claude Code) supports
  2026-07-28 MRTR form elicitation. Record the answer in `TASKS.md`. It doesn't block the PR.
- Live smoke test in Docker like PR #2/#3: a hand-driven MRTR exchange over stdio for
  `svc_down` with `service_type: ota` (round 1, then retry with `accept`, then a retry reusing
  the same ticket, then `decline`), plus a request without elicitation capability. Stopping a
  service that isn't running is harmless in a throwaway container. The MCP inspector can't be
  used: it doesn't speak 2026-07-28.

## Non-goals

- Unattended/CI mode, per-tool risk levels, previews or dry runs, archive checks, a
  confirmation-free "safe" list.
- Proving that a human answered.
- Remote transport.

## Left to the builder

- Module layout and naming, the ticket id source (any CSPRNG-backed id; prefer a crate already
  in the dependency tree, and keep `cargo deny` clean), and the map type/locking.
- Exact elicitation form schema (an empty-properties form, or a single boolean). Either is fine
  as long as `accept` is the only thing that runs the call.
- Exact wording of each template and of the refusal messages, following the example above.
- Whether to also set MCP tool annotations (`destructiveHint` on mutating tools,
  `readOnlyHint` on the read tools). Cheap, and it helps clients, but not required.
- Purging expired tickets: any simple approach, e.g. on each insert.
