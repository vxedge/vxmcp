# Changelog

All notable changes to this repo are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows
[Semantic Versioning](https://semver.org/); see `docs/VERSIONING.md` in the `docs` repo.

## [Unreleased]

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
- `vxedge-api` pinned to the 0.5.0 contract, with its `serde` feature.

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
