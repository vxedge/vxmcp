# vxmcp

An MCP server that exposes [vxAPI](https://github.com/vxedge/vxedge) to AI agent tooling. It is a thin, 1:1 wrapper over the same gRPC RPCs the `vxedge` CLI uses, and never bakes a multi-step workflow into the server. It targets MCP spec 2026-07-28 on [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk), the official Rust SDK.

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![CI](https://github.com/vxedge/vxmcp/actions/workflows/ci.yml/badge.svg)](https://github.com/vxedge/vxmcp/actions/workflows/ci.yml)

vxedge is an open, all-software testbed for mapping 5G network slices onto zonal and central software-defined-vehicle (SDV) E/E architectures on a single machine. Every published number ships with the manifest and image digests that produced it.

## Status

Scaffolded 2026-08-06. First tool (`experiment_status`, read-only) proves the wrapper pattern
end to end; the rest of the read-tool surface and the MRTR-gated mutating tools follow — see
[`TASKS.md`](TASKS.md).

## Building

```bash
cargo build --workspace
cargo test --workspace
```

Depends on `vxedge-api` from [`vxedge`](https://github.com/vxedge/vxedge) via a git dependency pinned to a commit (see `Cargo.toml`).

## Running

```bash
# against a real vxedged, local Unix socket
vxedged --listen unix:///tmp/vxedge.sock &
cargo run -p vxmcp -- --endpoint unix:///tmp/vxedge.sock

# interactive tool inspection
npx @modelcontextprotocol/inspector cargo run -p vxmcp -- --endpoint unix:///tmp/vxedge.sock
```

## Design

See [`AGENTS.md`](AGENTS.md) for the hard rule (no orchestration logic — every tool is one
RPC), the MRTR confirmation design for mutating tools, and the vxAPI dependency setup.

## Links

- Site: <https://vxedge.dev>
- Architecture: [`docs/ARCHITECTURE.md`](https://github.com/vxedge/docs/blob/main/ARCHITECTURE.md)
- Results: [`docs/RESULTS.md`](https://github.com/vxedge/docs/blob/main/RESULTS.md) · Roadmap: [`docs/ROADMAP.md`](https://github.com/vxedge/docs/blob/main/ROADMAP.md)
- Contributing: [CONTRIBUTING](https://github.com/vxedge/.github/blob/main/CONTRIBUTING.md) · Security: [SECURITY](https://github.com/vxedge/.github/blob/main/SECURITY.md)
- License: Apache-2.0 ([`LICENSE`](LICENSE))
