//! One module per vxAPI service wrapped as tools — kept separate from `main.rs`'s own
//! `#[tool_router] impl` so a tool's actual RPC-calling logic is unit-testable without needing
//! a live `rmcp` server around it (`main.rs`'s tool methods stay a thin dispatch to these).

pub mod compute;
pub mod events;
pub mod experiment;
pub mod gateway;
pub mod job;
pub mod svc;
pub mod zone;
