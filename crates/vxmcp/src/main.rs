//! vxmcp — MCP server exposing vxAPI to AI agent tooling. Every tool is a 1:1 wrapper over a
//! vxAPI RPC; no orchestration logic lives here (hard rule, AGENTS.md — the same one
//! `vxedge-cli` follows).
//!
//! Targets MCP spec 2026-07-28 on `rmcp` (the official Rust SDK): stateless request-response,
//! `ServerHandler::get_info()` advertises `ProtocolVersion::V_2026_07_28` explicitly. Read
//! tools only for now (`experiment_status`); mutating tools (`up`/`down`/`traffic up`) wait on
//! the MRTR confirmation flow being built and reviewed (AGENTS.md).

use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};

mod tools;

use tools::experiment::ExperimentStatusArgs;

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
}

#[tool_handler]
impl ServerHandler for VxmcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_instructions(
                "Thin, 1:1 wrapper over vxAPI — no orchestration logic. Read tools only for \
                 now: experiment_status. Point VXEDGE_ENDPOINT at a running vxedged \
                 (unix:///path.sock) before calling any tool."
                    .to_string(),
            )
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
