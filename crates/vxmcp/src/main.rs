//! vxmcp — MCP server exposing vxAPI to AI agent tooling. Every tool is a 1:1 wrapper over a
//! vxAPI RPC; no orchestration logic lives here (hard rule, AGENTS.md — the same one
//! `vxedge-cli` follows).
//!
//! Targets MCP spec 2026-07-28 on `rmcp` (the official Rust SDK): stateless request-response,
//! `ServerHandler::get_info()` advertises `ProtocolVersion::V_2026_07_28` explicitly. Read
//! tools only for now (`experiment_status`, `gateway_status`, `zone_list`, `compute_list`,
//! `svc_status`, `job_get`, `job_watch`, `events_subscribe`); mutating tools (`up`/`down`/`traffic up`) wait on the MRTR
//! confirmation flow being built and reviewed (AGENTS.md).

use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};

// Unused outside tests until svc_down lands (Task 2 of the #3 plan); removed there.
#[cfg_attr(not(test), allow(dead_code))]
mod confirm;
mod tools;

use tools::events::EventsSubscribeArgs;
use tools::experiment::ExperimentStatusArgs;
use tools::job::{JobGetArgs, JobWatchArgs};
use tools::svc::SvcStatusArgs;

/// Where to reach `vxedged` — same default and `unix://`-only contract `vxedge-cli` uses
/// (`vxedge/crates/vxedge-cli/src/main.rs`'s own `connect()`, copied here rather than shared,
/// since the two repos don't share a Cargo workspace to put a common helper crate in).
const DEFAULT_ENDPOINT: &str = "unix:///run/vxedge.sock";

fn endpoint_from_env() -> String {
    std::env::var("VXEDGE_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string())
}

async fn connect(endpoint: &str) -> Result<tonic::transport::Channel, anyhow::Error> {
    use hyper_util::rt::TokioIo;
    use tokio::net::UnixStream;
    use tonic::transport::{Endpoint, Uri};
    use tower::service_fn;

    let Some(path) = endpoint.strip_prefix("unix://") else {
        anyhow::bail!("unsupported endpoint `{endpoint}` — only unix:// is supported for now");
    };
    let path = path.to_string();
    // The URI is ignored for UDS; the connector below does the real work — identical to
    // vxedge-cli's own `connect()`.
    let channel = Endpoint::try_from("http://vxedge.uds")?
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = path.clone();
            async move { Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(path).await?)) }
        }))
        .await?;
    Ok(channel)
}

#[derive(Clone)]
struct VxmcpServer {
    channel: tonic::transport::Channel,
    // Read by #[tool_handler]'s generated ServerHandler methods (list_tools/call_tool), not
    // directly by any code in this file — dead-code analysis doesn't trace through that macro
    // expansion, the same known false-positive rmcp's own examples silence identically
    // (`examples/servers/src/common/counter.rs`'s `#![allow(dead_code)]`).
    #[allow(dead_code)]
    tool_router: ToolRouter<VxmcpServer>,
}

#[tool_router]
impl VxmcpServer {
    fn new(channel: tonic::transport::Channel) -> Self {
        Self {
            channel,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Get the status of the experiment workspace at the given path (the same \
                        status `vxedge experiment status` reports, run from that directory) — a \
                        top-level status plus per-component (gateway, zones, services) state."
    )]
    async fn experiment_status(
        &self,
        Parameters(args): Parameters<ExperimentStatusArgs>,
    ) -> Result<rmcp::model::CallToolResult, McpError> {
        tools::experiment::status(self.channel.clone(), args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(
        description = "Get the gateway's observed status (the same status `vxedge status` \
                        reports) — a top-level status plus per-slice state, link impairments, \
                        ingress filters and tunnels actually observed on the kernel."
    )]
    async fn gateway_status(&self) -> Result<rmcp::model::CallToolResult, McpError> {
        tools::gateway::status(self.channel.clone())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(
        description = "List the zones the active experiment declares, with each zone's \
                        observed state (the same list `vxedge zone list` reports)."
    )]
    async fn zone_list(&self) -> Result<rmcp::model::CallToolResult, McpError> {
        tools::zone::list(self.channel.clone())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(
        description = "List the compute nodes the active experiment declares, with each \
                        node's observed state (the same list `vxedge compute list` reports)."
    )]
    async fn compute_list(&self) -> Result<rmcp::model::CallToolResult, McpError> {
        tools::compute::list(self.channel.clone())
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(
        description = "Get whether an experiment service (`ota` or `telemetry`) is running \
                        on this host."
    )]
    async fn svc_status(
        &self,
        Parameters(args): Parameters<SvcStatusArgs>,
    ) -> Result<rmcp::model::CallToolResult, McpError> {
        tools::svc::status(self.channel.clone(), args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(
        description = "Get one snapshot of a job by id — its kind, state, progress, latest \
                        message and any error. Every mutating vxAPI call returns a job id."
    )]
    async fn job_get(
        &self,
        Parameters(args): Parameters<JobGetArgs>,
    ) -> Result<rmcp::model::CallToolResult, McpError> {
        tools::job::get(self.channel.clone(), args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(
        description = "Follow a job by id until it succeeds, fails or is cancelled, or until \
                        the timeout passes (default 60s, max 300s). Returns each progress step \
                        seen and the final state; on a timeout, call again to keep watching."
    )]
    async fn job_watch(
        &self,
        Parameters(args): Parameters<JobWatchArgs>,
    ) -> Result<rmcp::model::CallToolResult, McpError> {
        tools::job::watch(self.channel.clone(), args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }

    #[tool(
        description = "Collect daemon events (state transitions, audit, reconcile) emitted \
                        during a short window (default 10s, max 60s), optionally filtered by \
                        kind. Live only: events from before the call are not returned."
    )]
    async fn events_subscribe(
        &self,
        Parameters(args): Parameters<EventsSubscribeArgs>,
    ) -> Result<rmcp::model::CallToolResult, McpError> {
        tools::events::subscribe(self.channel.clone(), args)
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }
}

#[tool_handler]
impl ServerHandler for VxmcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_instructions(
                "Thin, 1:1 wrapper over vxAPI — no orchestration logic. Read tools only for \
                 now: experiment_status, gateway_status, zone_list, compute_list, svc_status, \
                 job_get, job_watch, events_subscribe. Point VXEDGE_ENDPOINT at a running vxedged (unix:///path.sock) \
                 before calling any tool."
                    .to_string(),
            )
    }

    /// Narrows `rmcp`'s default `initialize` negotiation (which otherwise echoes back
    /// whatever version the client requests, out of every version this SDK knows —
    /// found live 2026-08-06, a real MCP inspector client negotiated down to 2025-11-25
    /// silently) to exactly the one version `AGENTS.md`/`ARCHITECTURE.md` §5.1 commit this
    /// server to. A client that asks for anything else gets `get_info()`'s
    /// `V_2026_07_28` back instead of an echo — per spec, its choice then is to accept that
    /// answer or disconnect, not to talk this server into an older protocol.
    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        std::borrow::Cow::Borrowed(&[ProtocolVersion::V_2026_07_28])
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr) // stdout is the MCP transport's own wire, same
        // discipline vxagents' binaries already follow for their own JSON stdout contract
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let endpoint = endpoint_from_env();
    tracing::info!(%endpoint, "connecting to vxedged");
    let channel = connect(&endpoint).await?;

    let server = VxmcpServer::new(channel)
        .serve(stdio())
        .await
        .inspect_err(|e| {
            tracing::error!(error = ?e, "serving error");
        })?;

    server.waiting().await?;
    Ok(())
}
