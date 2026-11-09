//! One module per vxAPI service wrapped as tools — kept separate from `main.rs`'s own
//! `#[tool_router] impl` so a tool's actual RPC-calling logic is unit-testable without needing
//! a live `rmcp` server around it (`main.rs`'s tool methods stay a thin dispatch to these).
//! `out` holds what every tool returns: typed JSON, and refusals with their `ErrorReason`.

pub mod compute;
pub mod events;
pub mod experiment;
pub mod gateway;
pub mod job;
pub mod measure;
pub mod noscheck;
pub mod out;
pub mod run;
pub mod stream;
pub mod svc;
pub mod telemetry;
pub mod traffic;
pub mod vehicle;
pub mod workspace;
pub mod zone;
