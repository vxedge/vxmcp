# Human confirmation for mutating tools (TASKS.md #3)

Status: draft for review, 2026-10-29. Supersedes the open-ended wording of `TASKS.md` #3.

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
2. Resolve the target the same way the RPC call will (e.g. canonicalize the workspace path,
   exactly as `tools::experiment` does today).
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
data. For example:

> Tear down experiment at `/…/vxperiments/experiments/exp-003-ws` (stops its gateway, zones,
> compute and services). This affects anyone using this host's testbed. Confirm?

The "affects anyone using this host" line is a fixed reminder, not a check. It reflects the
owner's shared-environment concern.

## Scope of the first PR

- The guard as a small reusable piece (e.g. `crate::confirm`) that any mutating tool calls,
  so each tool adds only its template and its RPC call.
- **One** tool built on it: `experiment_down` (`ExperimentService.Down`, workspace path
  argument like `experiment_status`, all stages). It's the most destructive call and the
  clearest test of the owner's fears.
- Update `AGENTS.md` (the guard now exists and every mutating tool must use it) and `TASKS.md`
  #3.
- Each further mutating tool is its own small PR afterwards.

## Testing

- Unit tests for the guard without a daemon: no-capability → refused; round 1 → ticket +
  elicitation; accept → runs; decline/cancel → not run; reused ticket, expired ticket, ticket
  from a different tool or different arguments, missing/forged `requestState` → refused.
- Live smoke test in Docker like PR #2/#3: a hand-driven MRTR exchange over stdio
  (round 1, then retry with `accept`, then a retry reusing the same ticket), plus a client
  request without elicitation capability. The MCP inspector can't be used: it doesn't speak
  2026-07-28.

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
