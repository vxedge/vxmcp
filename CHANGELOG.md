# Changelog

All notable changes to this repo are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows
[Semantic Versioning](https://semver.org/) — see `docs/VERSIONING.md` in the `docs` repo.

## [Unreleased]

### Added

- Human confirmation for mutating tools (`crate::confirm`): MCP 2026-07-28 multi round-trip
  form elicitation with a single-use, 5-minute ticket bound to the exact call; fails closed
  when the client can't ask.
- `svc_down` (`SvcService.Down`), the first mutating tool, behind that confirmation.
- `experiment_down` (`ExperimentService.Down`, all stages), behind the same confirmation; the
  workspace path is resolved once and that exact path is confirmed and sent.

- Streaming read tools `job_watch` (`JobService.Watch`, follows a job until it finishes or a
  timeout passes) and `events_subscribe` (`EventService.Subscribe`, collects live events for
  a short window) — each still one RPC, collected and returned as a single result.
- Read tools `gateway_status`, `zone_list`, `compute_list`, `svc_status`, `job_get` — each a
  direct 1:1 call to one vxAPI RPC (`GatewayService.Status`, `ZoneService.List`,
  `ComputeService.List`, `SvcService.Status`, `JobService.Get`).
- Open-source release preparation: `LICENSE` (Apache-2.0), `NOTICE`, rewritten `README.md`. Crate `license` metadata is now `Apache-2.0` (was `MIT OR Apache-2.0`).
- Repo scaffolding: workspace `Cargo.toml`, `crates/vxmcp`, CI (build/test/clippy/fmt/deny),
  `AGENTS.md`/`CLAUDE.md`/`TASKS.md` (2026-08-06).
- `experiment_status` — first tool, read-only, proves the wrapper pattern end to end: a direct
  1:1 call to `ExperimentService.Status` over the same Unix-socket connection pattern
  `vxedge-cli` already uses.

### Changed

- `vxedge-api` pin bumped to `vxedge` `a78e7dd`, which makes `vxedged` reject an unknown
  `service_type`; `svc_status`'s argument doc now says so. No vxAPI surface change.
- `vxedge-api` pin bumped to `vxedge` `e3753be`, where `ExperimentService.Down` refuses
  anything but the active experiment. No vxAPI surface change.
