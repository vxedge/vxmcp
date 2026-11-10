# Changelog

All notable changes to this repo are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows
[Semantic Versioning](https://semver.org/); see `docs/VERSIONING.md` in the `docs` repo.

## [Unreleased]

## [0.4.1] - 2026-11-10

What the done-means gate found missing when an agent runs a pulled package end to end: the
campaign tools now take a cached package's digest, and `result_fetch` downloads an archive.
Follows vxedge 0.15.1 (`vxedge-api` pinned to `v0.15.1`). Every other tool is unchanged.

### Added

- **`result_fetch`** (`ResultService.Fetch`): downloads a results archive by digest into a file
  where vxmcp runs, checks the bytes against the digest, and returns the path, digest and size.
  An agent that shares no filesystem with vxedged can now re-hash a result and rerun it from the
  package the archive carries. It asks the human to confirm, since it writes a file.

### Fixed

- **`campaign_plan` and `campaign_start` on a pulled package.** Both took a path only, so a
  package pulled by digest could not be planned or run through vxmcp. They now take
  `sha256:<hex>` too, a package in this host's cache, as `package_inspect` does.

Both found by the done-means gate (X8.4).

### Changed

- **Follows vxedge 0.15.1** (`vxedge-api` pinned to `v0.15.1`): `campaign_start` retried with
  the same `request_id` while its campaign runs returns that campaign's job instead of
  `ERROR_REASON_BUSY`, and a retry with another profile is refused
  `ERROR_REASON_PARAMETER_INVALID`.

## [0.4.0] - 2026-11-10

Follows vxedge 0.15.0: a package's scenario is readable as a resource, and the package and host
tools carry the scenario and the host's simulators. `vxedge-api` is pinned to `v0.15.0`. Every
tool is unchanged.

### Added

- **Follows vxedge 0.15.0** (`vxedge-api` pinned to `v0.15.0`). The new resource
  `vxp://{id}@{version}/scenario` (or `vxp://sha256:<hex>/scenario`) returns a cached package's
  scenario typed: road, storyboard, simulator, clock, seed, actors, entities, and storyboard
  parameters with bounds. `package_inspect` and the package resource carry it too, and
  `host_capabilities` lists the simulators on the host.

## [0.3.0] - 2026-11-10

Follows vxedge 0.14.0: an agent can now run a whole campaign and read its typed verdict.
- **Inspect** a package, without running it.
- **Plan** its campaign, checked against this server's budgets.
- **Start** it with one confirmation for all its trials, then **follow** it.
- **Judge** the result and compare it with a reference.

Packages, campaign verdicts and the package catalog are also resources, and three prompts are
recipes over the tools. Every read tool now carries `readOnlyHint`. `vxedge-api` is pinned to
`v0.14.0`. Existing tools are unchanged, apart from two new optional arguments.

### Added

- **Follows vxedge 0.14.0** (`vxedge-api` pinned to `v0.14.0`), with tools for the services it
  adds:
  - **Packages:** `package_inspect` and `package_list`, and `package_pull`, which asks for
    confirmation.
  - **Campaigns:** `campaign_plan`, `campaign_get`, `campaign_list` and `campaign_watch`, and
    `campaign_start`, which asks once per campaign. Its confirmation names the trial count,
    points, repetitions, estimated minutes and profile, read from `Plan`.
  - **Results:** `result_list`, `result_get`, `result_verdict` and `result_compare`. An agent
    branches on a verdict's typed `outcome` per criterion.
- **Campaign budgets:** `campaign_start` refuses a campaign whose plan has a problem, with that
  problem's `ErrorReason`, before asking anyone. It also refuses one over this server's caps,
  `VXMCP_MAX_TRIALS` (default 100) and `VXMCP_MAX_CAMPAIGN_S` (default 28800), as
  `VXMCP_BUDGET_EXCEEDED`. `campaign_plan` reports whether a campaign fits as `vxmcpBudget`.
- **Resources** an agent reads without a tool call:
  - `vxp://catalog`: the package cache;
  - `vxp://{id}@{version}` or `vxp://sha256:<hex>`: a cached package's typed descriptor;
  - `vxresult://{campaign_id}/verdict`: a campaign's verdict.

  Each is one vxAPI read, and an unknown one is a not-found error carrying the typed refusal.
- **Prompts** that are recipes over the tools: `reproduce_result`, `find_knee` and
  `compare_baselines`.
- **New arguments:** `traffic_up` takes `duration_s`, and `telemetry_collect` takes
  `archive_version`, the fields vxedge 0.13.0 added to those requests.

### Changed

- Every read tool carries `readOnlyHint`, so a client can run them without asking.

## [0.2.6] - 2026-11-09

Follows vxedge 0.12.0. An agent can now run an experiment's declared measurements (`measure_start`, `measure_stop`,
`measure_status`) and read the live streams: telemetry records, metric samples and entity log lines, each by cursor
or for a short window. `vxedge-api` is pinned to `v0.12.0`; every existing tool is unchanged.

### Added

- Follows vxedge 0.12.0 (`vxedge-api` pinned to `v0.12.0`). New tools for the RPCs it adds:
  `measure_status`, `measure_start` and `measure_stop` (`MeasurementService`; start and stop ask
  for confirmation), and three read tools over the live streams, `telemetry_records`
  (`StreamRecords`), `telemetry_metrics` (`StreamMetrics`) and `logs_follow`
  (`LogService.Follow`). Each stream tool collects for a window of up to 60 s and at most 500
  items, and the record and log tools return `lastSeq` as the next cursor.

## [0.2.5] - 2026-11-08

Follows vxedge 0.10.0. `vehicle_status` and the other node-status tools now show each running `qemu-microvm`
node's `arch` (`x86_64`, `aarch64`) and `acceleration` (`kvm`, or `tcg` when the guest's CPU is emulated), so an
agent can tell a result it may time from one it may not. `vehicle_status`'s description says so. Tools, schemas
and responses are otherwise unchanged.

### Changed

- `vxedge-api` follows vxedge v0.10.0 (`e0a102c`).

## [0.2.4] - 2026-11-08

Follows vxedge 0.9.0. Against a 0.9.0 daemon, vxmcp 0.2.3 failed every tool that carries a node backend
with a JSON-RPC error, because it could not render the new `attach` backend; 0.2.4 names it. Tools,
schemas and responses are otherwise unchanged.

### Changed

- `vxedge-api` follows vxedge v0.9.0 (`ab25853`). Tool results name the `attach` zone backend
  (`NODE_BACKEND_ATTACH`) and its payload kind (`PAYLOAD_KIND_ATTACH_ADAPTER`), and `host_capabilities` lists
  the `attach` part kind.

### Fixed

- A daemon at vxedge 0.9.0 or later no longer fails `host_capabilities`, `vehicle_status` and the other tools
  that carry a node backend: the 0.7.0 bindings could not render the new enum value and the call ended in a
  JSON-RPC error (`Invalid variant 6`).

## [0.2.3] - 2026-11-06

Follows vxedge 0.7.0. `experiment_up` and `vehicle_up` now meet the daemon's lock check: a `vxp/1`
package that does not match its `vxp.lock` is refused with `ERROR_REASON_LOCK_MISMATCH`, naming each
difference, and there is no tool option to run one unlocked yet. `run_get` and `run_list` show the
package a run came from (id, version, package digest, whether it ran locked) and the kernel it ran
on. Older daemons leave those fields empty. The workspace argument descriptions name the `vxp.yaml`
descriptor. Also rmcp 3.9.0 and the Rust 1.99.1 toolchain; tools, schemas and responses are
otherwise unchanged.

### Changed

- `vxedge-api` moves from vxedge v0.5.3 to v0.7.0. `experiment_up`, `vehicle_up` and
  `workspace_plan` send the new `unlocked` field as false, so a `vxp/1` package that does not match
  its `vxp.lock` is refused with `ERROR_REASON_LOCK_MISMATCH`, which the tool error carries; there
  is no tool option to run unlocked yet. A `Run` from `run_get` and `run_list` carries `package`
  (`id`, `version`, `digest`), `packageLocked` and `kernel` (`release`, `preemptRt`). The workspace
  argument descriptions name the `vxp.yaml`
  descriptor (or a legacy `<name>.vxp`) instead of only a `.vxp` file, and `experiment_up` and
  `vehicle_up` say that a stale lock is refused.
- CI uses `actions/checkout@v7`, the major this repo's other workflows already run, instead of v4.
- `rmcp` 3.1.1 -> 3.9.0 (and `rmcp-macros`). 3.9.0 renames the type `get_info` returns from `ServerInfo` to `ServerConfig` (the same type, `InitializeResult`); the old name is deprecated, so vxmcp uses the new one. The update adds `sha2` 0.11 and its RustCrypto helper crates as transitive dependencies. Tools, schemas and responses are unchanged.
- Lockfile update: the semver-compatible dependency updates `cargo update` offers (58 crates). No `Cargo.toml` change and no source change; build, test, clippy `-D warnings`, fmt and `cargo deny` pass. `rmcp` moved separately (above).
- The toolchain moves from Rust 1.96.1 to 1.99.1 (`rust-toolchain.toml`, `rust-version` 1.99). Build, test, clippy `-D warnings`, fmt and `cargo deny` pass unchanged; no source changes were needed.

## [0.2.2] - 2026-11-04

Follows vxedge 0.5.3: `run_list` and `run_get` name the rule each run's workspace identity was
computed under, so an agent compares identities only when they share it. Older daemons leave the
field empty.

### Changed

- `vxedge-api` pinned to vxedge `v0.5.3`, so a `Run` carries `workspaceIdentityAlgorithm`
  (`vxws/2`, or `vxws/1` for a run recorded before 0.5.3).

## [0.2.1] - 2026-11-04

Follows vxedge 0.5.2: `host_capabilities` now reports the GPU and answers from the same checks as
`up`, `traffic_up` refuses up front with `ERROR_REASON_ENDPOINT_NOT_ON_HOST` when an endpoint is
inside a node, and status tools answer about four times faster. Needs `vxedged` 0.5.2 or newer for
those; older 0.5.x daemons still work.

### Changed

- `vxedge-api` pinned to vxedge `v0.5.2`, so results carry `HostCapabilities.gpu` and the new
  `ErrorReason` value by name.

## [0.2.0] - 2026-11-04

The agent surface for vxAPI 0.5.0. An agent can run a whole trial through vxmcp alone (plan, bring
up, follow jobs, read typed status, run traffic, collect the evidence, recover, tear down),
branching only on typed fields: every result is the RPC's response as JSON with enum values by
name, and every refusal carries its `ErrorReason`. 29 tools, mutating ones still behind the human
confirmation. The vxedge 0.5.0 release gate ran through this server
([evidence](https://github.com/vxedge/vxperiments/tree/main/results/vxapi-0.5.0-gate-2026-11-04)).
Needs `vxedged` 0.5.0 or newer.

### Added

- Read tools: `host_capabilities`, `workspace_plan`, `run_list`, `run_get`, `vehicle_status`,
  `traffic_status`, `noscheck_status`, `job_list`.
- Mutating tools, each behind the human confirmation: `experiment_up`, `vehicle_up`,
  `vehicle_down`, `svc_up`, `traffic_up`, `traffic_down`, `noscheck_up`, `noscheck_verify`,
  `noscheck_down`, `telemetry_collect`, `job_cancel`. With the existing tools, an agent can run
  a full experiment through vxmcp alone.
- `run_id` on every tool that acts on a run, `stages` on the experiment and vehicle verbs, and
  `request_id` on `traffic_up` and `noscheck_verify`.
- `events_subscribe` takes `after_id` and returns `lastId`, so a follow-up call resumes with no
  gap. `svc_status` takes an optional `workspace`.

### Changed

- Every result is the RPC's response as typed JSON with enum values by name, in
  `structuredContent` and as text, instead of `[state=N]` lines.
- A refusal from `vxedged` is a tool error the model can read, carrying its gRPC code and
  `ErrorReason`, instead of a JSON-RPC internal error.
- `vxedge-api` pinned to vxedge `v0.5.0`, with its `serde` feature.

## [0.1.0] - 2026-10-29

First release: a stdio MCP server over vxAPI, targeting MCP specification 2026-07-28.

### Added

- Read tools, each a direct 1:1 call to one vxAPI RPC: `experiment_status`
  (`ExperimentService.Status`), `gateway_status` (`GatewayService.Status`), `zone_list`
  (`ZoneService.List`), `compute_list` (`ComputeService.List`), `svc_status`
  (`SvcService.Status`), `job_get` (`JobService.Get`).
- Streaming read tools `job_watch` (`JobService.Watch`, follows a job until it finishes or a
  timeout passes) and `events_subscribe` (`EventService.Subscribe`, collects live events for
  a short window). Each is still one RPC, collected and returned as a single result.
- Human confirmation for mutating tools (`crate::confirm`): MCP 2026-07-28 multi round-trip
  form elicitation with a single-use, 5-minute ticket bound to the exact call; fails closed
  when the client can't ask.
- `svc_down` (`SvcService.Down`), the first mutating tool, behind that confirmation.
- `experiment_down` (`ExperimentService.Down`, all stages), behind the same confirmation; the
  workspace path is resolved once and that exact path is confirmed and sent.
- `vxmcp --version` prints the version and exits without contacting `vxedged`.
- Release workflow: tag-triggered builds for x86_64 and aarch64 Linux (native Ubuntu 24.04
  runners, `--locked`), `vxmcp-<version>-<target>.tar.gz` archives with `SHA256SUMS` and
  build provenance attestations.
- `SECURITY.md`, `CITATION.cff`.
- Open-source release preparation: `LICENSE` (Apache-2.0), `NOTICE`, rewritten `README.md`.
  Crate `license` metadata is now `Apache-2.0` (was `MIT OR Apache-2.0`).
- Repo scaffolding: workspace `Cargo.toml`, `crates/vxmcp`, CI (build/test/clippy/fmt/deny),
  `AGENTS.md`/`CLAUDE.md`/`TASKS.md` (2026-08-06).

### Changed

- `vxedge-api` pin bumped to `vxedge` `a78e7dd`, which makes `vxedged` reject an unknown
  `service_type`; `svc_status`'s argument doc now says so. No vxAPI surface change.
- `vxedge-api` pin bumped to `vxedge` `e3753be`, where `ExperimentService.Down` refuses
  anything but the active experiment. No vxAPI surface change.
