# vxmcp

An MCP server that exposes [vxAPI](https://github.com/vxedge/vxedge) to agent tooling such as
Claude Code. It is a thin, 1:1 wrapper: every tool calls exactly one of the gRPC RPCs the
`vxedge` CLI uses, and no multi-step workflow lives in the server. Sequencing calls is the
agent's job.

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![CI](https://github.com/vxedge/vxmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/vxedge/vxmcp/actions/workflows/ci.yml)

vxedge is an open, all-software testbed for mapping 5G network slices onto zonal and central software-defined-vehicle (SDV) E/E architectures on a single machine. Every published number ships with the manifest and image digests that produced it.

## Tools

Every result is the RPC's own response as typed JSON, in the canonical Protobuf mapping: enum
values by name (`"state": "JOB_STATE_FAILED"`), lowerCamelCase fields. A refusal from `vxedged`
is a tool error the agent can read, with the typed reason:

```json
{"error": {"code": "ABORTED", "reason": "ERROR_REASON_BUSY", "domain": "vxedge.v1",
           "message": "...", "metadata": {"job_id": "job-..."}}}
```

**Read tools**

| Tool | vxAPI RPC |
|---|---|
| `host_capabilities` | `HostService.Capabilities` |
| `workspace_plan` | `WorkspaceService.Plan` |
| `run_list`, `run_get` | `RunService.List`, `RunService.Get` |
| `experiment_status` | `ExperimentService.Status` |
| `vehicle_status` | `VehicleService.Status` |
| `gateway_status` | `GatewayService.Status` |
| `zone_list`, `compute_list` | `ZoneService.List`, `ComputeService.List` |
| `svc_status` | `SvcService.Status` |
| `traffic_status` | `TrafficService.Status` |
| `noscheck_status` | `NosCheckService.Status` |
| `job_get`, `job_list` | `JobService.Get`, `JobService.List` |
| `job_watch` | `JobService.Watch`, collected until the job ends or a timeout |
| `events_subscribe` | `EventService.Subscribe`, collected for a short window; resumes from `lastId` |

**Mutating tools**, each asking the user to confirm first and returning a job

| Tool | vxAPI RPC |
|---|---|
| `experiment_up`, `experiment_down` | `ExperimentService.Up`, `.Down` |
| `vehicle_up`, `vehicle_down` | `VehicleService.Up`, `.Down` |
| `svc_up`, `svc_down` | `SvcService.Up`, `.Down` |
| `traffic_up`, `traffic_down` | `TrafficService.Up`, `.Down` |
| `noscheck_up`, `noscheck_verify`, `noscheck_down` | `NosCheckService.Up`, `.Verify`, `.Down` |
| `telemetry_collect` | `TelemetryService.Collect` |
| `job_cancel` | `JobService.Cancel` |

Tools that act on a run take an optional `run_id` from `run_list`; omitted, it means the only
run. `traffic_up` and `noscheck_verify` take a `request_id`, so a retry returns the original
job instead of starting the work twice.

Mutating tools go through the human confirmation flow described in [`AGENTS.md`](AGENTS.md):
the server answers the first call with a form elicitation, and runs the RPC only when the user
accepts, on a single-use ticket bound to the exact arguments. A client that cannot ask the user
gets an error, never a silent run. Follow the returned job with `job_watch`. The project-wide
state of this frontend is `frontend.mcp` in
[`docs/FACTS.md`](https://github.com/vxedge/docs/blob/main/FACTS.md).

## MCP version

vxmcp targets MCP specification **2026-07-28** on [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk),
the official Rust SDK. A 2026-07-28 client needs no `initialize` handshake: each request
carries its protocol version in `_meta` and is served statelessly. `supported_protocol_versions`
is narrowed to 2026-07-28 only, because the SDK default accepts every revision it knows, which
let a legacy client negotiate the server down to 2025-11-25. With the narrowing, a legacy
client that opens with `initialize` is answered with 2026-07-28 and must accept it or
disconnect. The confirmation flow has been tested with Claude Code 2.1.312.

## Install

**Prebuilt (Linux x86-64 and ARM64):** download the latest release, check it, and unpack it.
Each release carries `SHA256SUMS` and build provenance.

```bash
gh release download -R vxedge/vxmcp --pattern '*'                                # archives and SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS                                          # must say OK
gh attestation verify vxmcp-*-x86_64-unknown-linux-gnu.tar.gz -R vxedge/vxmcp    # needs gh 2.49 or newer
tar xzf vxmcp-*-x86_64-unknown-linux-gnu.tar.gz                                   # vxmcp, LICENSE, NOTICE
```

Use the `aarch64` archive on ARM. The binary is built on Ubuntu 24.04 (glibc 2.39), so it runs
on 24.04 and newer.

**From source:** needs Rust (pinned in `rust-toolchain.toml`) and `protoc`, because
`vxedge-api` regenerates its gRPC bindings at build time.

```bash
cargo build --workspace --locked
cargo test --workspace
```

`vxedge-api` comes from [`vxedge`](https://github.com/vxedge/vxedge) as a git dependency pinned
to a commit (see `Cargo.toml`); it fetches without credentials.

## Running

vxmcp speaks MCP over stdio and talks to a running `vxedged` over a Unix socket
(`VXEDGE_ENDPOINT`, default `unix:///run/vxedge.sock`).

```bash
vxedged --listen unix:///tmp/vxedge.sock &
VXEDGE_ENDPOINT=unix:///tmp/vxedge.sock cargo run -p vxmcp

# interactive tool inspection
npx @modelcontextprotocol/inspector -e VXEDGE_ENDPOINT=unix:///tmp/vxedge.sock cargo run -p vxmcp
```

The inspector must speak MCP 2026-07-28; a release that only knows 2025-11-25 and earlier
refuses the server's answer ("Server's protocol version is not supported").

See the vxedge [getting started guide](https://github.com/vxedge/docs/blob/main/GETTING-STARTED.md)
for building and running `vxedged`.

### Add it to Claude Code

```bash
claude mcp add vxmcp -e VXEDGE_ENDPOINT=unix:///tmp/vxedge.sock -- /path/to/vxmcp
```

Use the absolute path of the unpacked or built binary. The mutating tools then show a
confirmation dialog in Claude Code before they run.

## Design

See [`AGENTS.md`](AGENTS.md) for the hard rule (no orchestration logic, every tool is one RPC),
the confirmation design for mutating tools, and the vxAPI dependency setup.

## Links

- Site: <https://vxedge.dev>
- Architecture: [`docs/ARCHITECTURE.md`](https://github.com/vxedge/docs/blob/main/ARCHITECTURE.md)
- Results: [`docs/RESULTS.md`](https://github.com/vxedge/docs/blob/main/RESULTS.md) · Roadmap: [`docs/ROADMAP.md`](https://github.com/vxedge/docs/blob/main/ROADMAP.md)
- Contributing: [CONTRIBUTING](https://github.com/vxedge/.github/blob/main/CONTRIBUTING.md) · Security: [`SECURITY.md`](SECURITY.md)
- Changelog: [`CHANGELOG.md`](CHANGELOG.md) · Citation: [`CITATION.cff`](CITATION.cff)
- License: Apache-2.0 ([`LICENSE`](LICENSE))
