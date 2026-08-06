# vxmcp

MCP server exposing [vxAPI](https://github.com/vxedge/vxedge) to AI agent tooling (Claude
Code and others) — a thin, 1:1 wrapper over the same gRPC RPCs the `vxedge` CLI uses, never a
multi-step workflow baked into the server. Targets MCP spec 2026-07-28 on
[`rmcp`](https://github.com/modelcontextprotocol/rust-sdk), the official Rust SDK.

Part of the [vxedge](https://github.com/vxedge) program — see
[`docs/ARCHITECTURE.md`](https://github.com/vxedge/docs/blob/main/ARCHITECTURE.md) §5.1 for
where this sits among the other frontends (CLI, web dashboard), and
[`docs/TASKS.md`](https://github.com/vxedge/docs/blob/main/TASKS.md) for the phase-3 board
item this repo tracks.

## Status

Scaffolded 2026-08-06. First tool (`experiment_status`, read-only) proves the wrapper pattern
end to end; the rest of the read-tool surface and the MRTR-gated mutating tools follow — see
[`TASKS.md`](TASKS.md).

## Building

```bash
cargo build --workspace
cargo test --workspace
```

Needs read access to the private `vxedge` repo (a git dependency on `vxedge-api`, pinned to a
commit — see `Cargo.toml`).

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
