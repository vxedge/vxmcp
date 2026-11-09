//! vxmcp — MCP server exposing vxAPI to AI agent tooling. Every tool is a 1:1 wrapper over a
//! vxAPI RPC; no orchestration logic lives here (hard rule, AGENTS.md — the same one
//! `vxedge-cli` follows).
//!
//! Targets MCP spec 2026-07-28 on `rmcp` (the official Rust SDK): stateless request-response,
//! `ServerHandler::get_info()` advertises `ProtocolVersion::V_2026_07_28` explicitly. Results
//! are the RPC's response as typed JSON, and a vxedged refusal is a tool error carrying its
//! `ErrorReason` (`tools::out`). Mutating tools go through `crate::confirm`'s human
//! confirmation first (AGENTS.md).

use std::future::Future;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::server::{
        router::tool::ToolRouter,
        tool::{InputResponses, RequestState},
        wrapper::Parameters,
    },
    model::{
        CallToolResponse, CallToolResult, Implementation, ProtocolVersion, ServerCapabilities,
        ServerConfig,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::stdio,
};

mod confirm;
mod tools;

use confirm::{Confirmations, Gate};
use tools::events::EventsSubscribeArgs;
use tools::experiment::{ExperimentDownArgs, ExperimentStatusArgs, ExperimentUpArgs};
use tools::job::{JobCancelArgs, JobGetArgs, JobListArgs, JobWatchArgs};
use tools::measure::{MeasureArgs, MeasureStatusArgs};
use tools::noscheck::NosCheckArgs;
use tools::out::finish;
use tools::run::RunArgs;
use tools::stream::{LogsArgs, MetricsArgs, RecordsArgs};
use tools::svc::{SvcDownArgs, SvcStatusArgs, SvcUpArgs};
use tools::telemetry::CollectArgs;
use tools::traffic::{TrafficArgs, TrafficStatusArgs};
use tools::vehicle::{VehicleStageArgs, VehicleStatusArgs};
use tools::workspace::WorkspaceArgs;

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
    /// Pending human confirmations for mutating tools, shared by every clone of the server.
    confirmations: std::sync::Arc<Confirmations>,
    // Read by #[tool_handler]'s generated ServerHandler methods (list_tools/call_tool), not
    // directly by any code in this file — dead-code analysis doesn't trace through that macro
    // expansion, the same known false-positive rmcp's own examples silence identically
    // (`examples/servers/src/common/counter.rs`'s `#![allow(dead_code)]`).
    #[allow(dead_code)]
    tool_router: ToolRouter<VxmcpServer>,
}

/// Runs a mutating tool's one RPC only after the human confirmed exactly `args` (AGENTS.md).
/// `run` is built before the gate but not polled unless the gate says proceed.
impl VxmcpServer {
    #[allow(clippy::too_many_arguments)]
    async fn gated(
        &self,
        ctx: &RequestContext<RoleServer>,
        state: Option<String>,
        responses: Option<rmcp::model::InputResponses>,
        tool: &'static str,
        args: serde_json::Value,
        question: String,
        run: impl Future<Output = anyhow::Result<CallToolResult>>,
    ) -> Result<CallToolResponse, McpError> {
        match self.confirmations.gate(
            ctx.client_capabilities().as_ref(),
            tool,
            &args,
            &question,
            state,
            responses,
        )? {
            Gate::Ask(r) => Ok(r.into()),
            Gate::NotConfirmed(r) => Ok(r.into()),
            Gate::Proceed => finish(run.await).map(Into::into),
        }
    }
}

#[tool_router]
impl VxmcpServer {
    fn new(channel: tonic::transport::Channel) -> Self {
        Self {
            channel,
            confirmations: std::sync::Arc::new(Confirmations::new()),
            tool_router: Self::tool_router(),
        }
    }

    fn ch(&self) -> tonic::transport::Channel {
        self.channel.clone()
    }

    // ---- Read tools: no confirmation. ----

    #[tool(
        description = "What this host can run: the zone backends and part kinds available, and \
                        how many runs it holds at once. Call before choosing a workspace."
    )]
    async fn host_capabilities(&self) -> Result<CallToolResult, McpError> {
        finish(tools::workspace::capabilities(self.ch()).await)
    }

    #[tool(
        description = "Plan a workspace without changing anything: the checks experiment_up and \
                        vehicle_up make first, each with a typed reason if it fails, and the \
                        nodes they would provision."
    )]
    async fn workspace_plan(
        &self,
        Parameters(args): Parameters<WorkspaceArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::workspace::plan(self.ch(), args).await)
    }

    #[tool(description = "List the runs on this host: each run's id, workspace and start time.")]
    async fn run_list(&self) -> Result<CallToolResult, McpError> {
        finish(tools::run::list(self.ch()).await)
    }

    #[tool(description = "Get one run by id; omit the id to mean the only run on this host.")]
    async fn run_get(
        &self,
        Parameters(args): Parameters<RunArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::run::get(self.ch(), args).await)
    }

    #[tool(
        description = "Get the status of the experiment workspace at the given path: a \
                        top-level status plus each component's typed state."
    )]
    async fn experiment_status(
        &self,
        Parameters(args): Parameters<ExperimentStatusArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::experiment::status(self.ch(), args).await)
    }

    #[tool(
        description = "Get the status of the vehicle workspace at the given path: each stage \
                        and node, observed on the host. A running qemu-microvm node also carries \
                        `arch` (`x86_64`, `aarch64`) and `acceleration`: `kvm`, or `tcg` when the \
                        guest's CPU is emulated, in which case its results are functional only and \
                        must not be timed."
    )]
    async fn vehicle_status(
        &self,
        Parameters(args): Parameters<VehicleStatusArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::vehicle::status(self.ch(), args).await)
    }

    #[tool(
        description = "Get the gateway's observed status: slices, link impairments, ingress \
                        filters and tunnels as found on the kernel."
    )]
    async fn gateway_status(&self) -> Result<CallToolResult, McpError> {
        finish(tools::gateway::status(self.ch()).await)
    }

    #[tool(description = "List the zones a run declares, each with its observed, typed status.")]
    async fn zone_list(
        &self,
        Parameters(args): Parameters<RunArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::zone::list(self.ch(), args).await)
    }

    #[tool(
        description = "List the compute nodes a run declares, each with its observed, typed \
                        status."
    )]
    async fn compute_list(
        &self,
        Parameters(args): Parameters<RunArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::compute::list(self.ch(), args).await)
    }

    #[tool(
        description = "Get an experiment service's status (`ota` or `telemetry`). With a \
                        workspace, the service it declares and where it is placed; without one, \
                        whether it runs on this host."
    )]
    async fn svc_status(
        &self,
        Parameters(args): Parameters<SvcStatusArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::svc::status(self.ch(), args).await)
    }

    #[tool(
        description = "Get each traffic profile's state in a run: running, finished or failed, \
                        from the exit code its load generator left."
    )]
    async fn traffic_status(
        &self,
        Parameters(args): Parameters<TrafficStatusArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::traffic::status(self.ch(), args).await)
    }

    #[tool(
        description = "Get each measurement a workspace declares, with its entity \
                        (`measurements/<id>`) and kind: running, finished or failed."
    )]
    async fn measure_status(
        &self,
        Parameters(args): Parameters<MeasureStatusArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::measure::status(self.ch(), args).await)
    }

    #[tool(
        description = "Read a workspace's telemetry records (samples, gaps, phases) from a \
                        cursor, named by entity. `follow` listens for new ones for a window \
                        (default 10 s, at most 60 s). At most 500 per call; `lastSeq` is the next \
                        cursor.",
        annotations(read_only_hint = true)
    )]
    async fn telemetry_records(
        &self,
        Parameters(args): Parameters<RecordsArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::stream::records(self.ch(), args).await)
    }

    #[tool(
        description = "Collect the samples a workspace's telemetry service writes during a \
                        window (default 10 s, at most 60 s), optionally filtered by labels. At \
                        most 500 per call.",
        annotations(read_only_hint = true)
    )]
    async fn telemetry_metrics(
        &self,
        Parameters(args): Parameters<MetricsArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::stream::metrics(self.ch(), args).await)
    }

    #[tool(
        description = "Read the log lines a workspace's telemetry service read from each \
                        entity's files (serial consoles, hypervisor logs, service and traffic \
                        output), from a cursor, filtered by entity prefix and stream. `follow` \
                        listens for a window. At most 500 per call; `lastSeq` is the next \
                        cursor.",
        annotations(read_only_hint = true)
    )]
    async fn logs_follow(
        &self,
        Parameters(args): Parameters<LogsArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::stream::logs(self.ch(), args).await)
    }

    #[tool(description = "Get whether the workspace's NOS check instance runs in a run.")]
    async fn noscheck_status(
        &self,
        Parameters(args): Parameters<NosCheckArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::noscheck::status(self.ch(), args).await)
    }

    #[tool(
        description = "Get one snapshot of a job by id: kind, state, progress, message, typed \
                        failure reason and any artifact. Every mutating tool returns a job."
    )]
    async fn job_get(
        &self,
        Parameters(args): Parameters<JobGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::job::get(self.ch(), args).await)
    }

    #[tool(
        description = "List jobs, filtered by state, kind, run or parent job, newest first, in \
                        pages."
    )]
    async fn job_list(
        &self,
        Parameters(args): Parameters<JobListArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::job::list(self.ch(), args).await)
    }

    #[tool(
        description = "Follow a job by id until it succeeds, fails or is cancelled, or until \
                        the timeout passes (default 60s, max 300s). `end` says which; on \
                        `timed_out`, call again to keep watching."
    )]
    async fn job_watch(
        &self,
        Parameters(args): Parameters<JobWatchArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::job::watch(self.ch(), args).await)
    }

    #[tool(
        description = "Collect daemon events during a short window (default 10s, max 60s), \
                        optionally filtered by kind or family. Pass the previous result's \
                        `lastId` as `after_id` to resume with no gap."
    )]
    async fn events_subscribe(
        &self,
        Parameters(args): Parameters<EventsSubscribeArgs>,
    ) -> Result<CallToolResult, McpError> {
        finish(tools::events::subscribe(self.ch(), args).await)
    }

    // ---- Mutating tools: each asks the human first and returns a job for job_watch. ----

    #[tool(
        description = "Bring up the experiment at the given workspace path, all stages or the \
                        ones named. Asks the human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn experiment_up(
        &self,
        Parameters(args): Parameters<ExperimentUpArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        tools::out::check_stages(&args.stages)?;
        // Resolved once: the confirmation, the ticket and the RPC all use this exact string.
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::experiment::up_confirmation(&ws, &args.stages);
        let bound = serde_json::json!({ "workspace": ws, "stages": args.stages });
        let run = tools::experiment::up(self.ch(), ws, &args.stages);
        self.gated(
            &ctx,
            state,
            responses,
            "experiment_up",
            bound,
            question,
            run,
        )
        .await
    }

    #[tool(
        description = "Tear down the experiment at the given workspace path: its services, \
                        zones and this host's gateway. Only the active experiment can be torn \
                        down. Asks the human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn experiment_down(
        &self,
        Parameters(args): Parameters<ExperimentDownArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        tools::out::check_stages(&args.stages)?;
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::experiment::down_confirmation(&ws, &args.stages);
        let bound = serde_json::json!({ "workspace": ws, "stages": args.stages });
        let run = tools::experiment::down(self.ch(), ws, &args.stages);
        self.gated(
            &ctx,
            state,
            responses,
            "experiment_down",
            bound,
            question,
            run,
        )
        .await
    }

    #[tool(
        description = "Bring up the vehicle at the given workspace path, all stages or the ones \
                        named. Asks the human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn vehicle_up(
        &self,
        Parameters(args): Parameters<VehicleStageArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        tools::out::check_stages(&args.stages)?;
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::vehicle::up_confirmation(&ws, &args.stages);
        let bound = serde_json::json!({ "workspace": ws, "stages": args.stages });
        let run = tools::vehicle::up(self.ch(), ws, &args.stages);
        self.gated(&ctx, state, responses, "vehicle_up", bound, question, run)
            .await
    }

    #[tool(
        description = "Tear down the vehicle at the given workspace path, in reverse stage \
                        order. Only the active vehicle can be torn down. Asks the human user to \
                        confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn vehicle_down(
        &self,
        Parameters(args): Parameters<VehicleStageArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        tools::out::check_stages(&args.stages)?;
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::vehicle::down_confirmation(&ws, &args.stages);
        let bound = serde_json::json!({ "workspace": ws, "stages": args.stages });
        let run = tools::vehicle::down(self.ch(), ws, &args.stages);
        self.gated(&ctx, state, responses, "vehicle_down", bound, question, run)
            .await
    }

    #[tool(
        description = "Start traffic profiles a workspace declares, in a run that is up. A \
                        `request_id` makes a retry return the original job. Asks the human user \
                        to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn traffic_up(
        &self,
        Parameters(args): Parameters<TrafficArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::traffic::up_confirmation(&ws, &args.profiles);
        let bound = serde_json::json!({
            "workspace": ws, "profiles": args.profiles, "run_id": args.run_id,
            "request_id": args.request_id,
        });
        let run = tools::traffic::up(self.ch(), ws, args);
        self.gated(&ctx, state, responses, "traffic_up", bound, question, run)
            .await
    }

    #[tool(
        description = "Stop traffic profiles a workspace declares, in a run that is up. Asks \
                        the human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn traffic_down(
        &self,
        Parameters(args): Parameters<TrafficArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::traffic::down_confirmation(&ws, &args.profiles);
        let bound = serde_json::json!({
            "workspace": ws, "profiles": args.profiles, "run_id": args.run_id,
        });
        let run = tools::traffic::down(self.ch(), ws, args);
        self.gated(&ctx, state, responses, "traffic_down", bound, question, run)
            .await
    }

    #[tool(
        description = "Start measurements a workspace declares, in a run that is up: probes \
                        run inside the nodes. A `request_id` makes a retry return the original \
                        job. Asks the human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn measure_start(
        &self,
        Parameters(args): Parameters<MeasureArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::measure::start_confirmation(&ws, &args.ids);
        let bound = serde_json::json!({
            "workspace": ws, "ids": args.ids, "duration_s": args.duration_s,
            "run_id": args.run_id, "request_id": args.request_id,
        });
        let run = tools::measure::start(self.ch(), ws, args);
        self.gated(
            &ctx,
            state,
            responses,
            "measure_start",
            bound,
            question,
            run,
        )
        .await
    }

    #[tool(
        description = "Stop measurements a workspace declares, in a run that is up. Asks the \
                        human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn measure_stop(
        &self,
        Parameters(args): Parameters<MeasureArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::measure::stop_confirmation(&ws, &args.ids);
        let bound = serde_json::json!({
            "workspace": ws, "ids": args.ids, "run_id": args.run_id,
        });
        let run = tools::measure::stop(self.ch(), ws, args);
        self.gated(&ctx, state, responses, "measure_stop", bound, question, run)
            .await
    }

    #[tool(
        description = "Start an experiment service (`ota` or `telemetry`) that a workspace \
                        declares, in a run that is up. Asks the human user to confirm first. \
                        Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn svc_up(
        &self,
        Parameters(args): Parameters<SvcUpArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::svc::up_confirmation(&args.service_type, &ws);
        let bound = serde_json::json!({
            "service_type": args.service_type, "workspace": ws, "run_id": args.run_id,
        });
        let run = tools::svc::up(self.ch(), args.service_type, ws, args.run_id);
        self.gated(&ctx, state, responses, "svc_up", bound, question, run)
            .await
    }

    #[tool(
        description = "Stop an experiment service (`ota` or `telemetry`) on this host. Asks \
                        the human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn svc_down(
        &self,
        Parameters(args): Parameters<SvcDownArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let question = tools::svc::down_confirmation(&args.service_type);
        let bound = serde_json::json!({ "service_type": args.service_type, "run_id": args.run_id });
        let run = tools::svc::down(self.ch(), args);
        self.gated(&ctx, state, responses, "svc_down", bound, question, run)
            .await
    }

    #[tool(
        description = "Boot the NOS check instance a workspace declares, in a run that is up. \
                        Asks the human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn noscheck_up(
        &self,
        Parameters(args): Parameters<NosCheckArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::noscheck::confirmation("up", &ws);
        let bound = serde_json::json!({ "workspace": ws, "run_id": args.run_id });
        let run = tools::noscheck::up(self.ch(), ws, args);
        self.gated(&ctx, state, responses, "noscheck_up", bound, question, run)
            .await
    }

    #[tool(
        description = "Stop the NOS check instance a workspace declares. Asks the human user to \
                        confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn noscheck_down(
        &self,
        Parameters(args): Parameters<NosCheckArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::noscheck::confirmation("down", &ws);
        let bound = serde_json::json!({ "workspace": ws, "run_id": args.run_id });
        let run = tools::noscheck::down(self.ch(), ws, args);
        self.gated(
            &ctx,
            state,
            responses,
            "noscheck_down",
            bound,
            question,
            run,
        )
        .await
    }

    #[tool(
        description = "Verify the switch config against the gateway plan on a disposable NOS \
                        check instance, field by field. A `request_id` makes a retry return the \
                        original job. Asks the human user to confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn noscheck_verify(
        &self,
        Parameters(args): Parameters<NosCheckArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::noscheck::confirmation("verify", &ws);
        let bound = serde_json::json!({
            "workspace": ws, "run_id": args.run_id, "request_id": args.request_id,
        });
        let run = tools::noscheck::verify(self.ch(), ws, args);
        self.gated(
            &ctx,
            state,
            responses,
            "noscheck_verify",
            bound,
            question,
            run,
        )
        .await
    }

    #[tool(
        description = "Collect a workspace's telemetry into an archive on this host; the job's \
                        `artifact` gives its path and sha256. Asks the human user to confirm \
                        first. Returns the job.",
        annotations(destructive_hint = false)
    )]
    async fn telemetry_collect(
        &self,
        Parameters(args): Parameters<CollectArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let ws = tools::experiment::resolve_workspace(&args.workspace);
        let question = tools::telemetry::collect_confirmation(&ws, args.out_path.as_deref());
        let bound = serde_json::json!({ "workspace": ws, "out_path": args.out_path });
        let run = tools::telemetry::collect(self.ch(), ws, args.out_path);
        self.gated(
            &ctx,
            state,
            responses,
            "telemetry_collect",
            bound,
            question,
            run,
        )
        .await
    }

    #[tool(
        description = "Cancel a running job: it stops at its next step and its child jobs are \
                        cancelled too; a `down` cannot be cancelled. Asks the human user to \
                        confirm first. Returns the job.",
        annotations(destructive_hint = true)
    )]
    async fn job_cancel(
        &self,
        Parameters(args): Parameters<JobCancelArgs>,
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let question = tools::job::cancel_confirmation(&args.id);
        let bound = serde_json::json!({ "id": args.id });
        let run = tools::job::cancel(self.ch(), args);
        self.gated(&ctx, state, responses, "job_cancel", bound, question, run)
            .await
    }
}
#[tool_handler]
impl ServerHandler for VxmcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_instructions(
                "Thin, 1:1 wrapper over vxAPI — no orchestration logic. Results are typed JSON \
                 with enum values by name; a refusal is a tool error whose `error.reason` is an \
                 `ErrorReason` name such as ERROR_REASON_BUSY. 64-bit integers such as \
                 createdUnixMs are JSON strings, per the Protobuf JSON mapping. Start with host_capabilities and \
                 workspace_plan; mutating tools ask the user to confirm and return a job to \
                 follow with job_watch. Point VXEDGE_ENDPOINT at a running vxedged \
                 (unix:///path.sock) before calling any tool."
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

/// `vxmcp --version` (or `-V`) answers without a daemon, so a release archive can be checked
/// on its own; any other arguments are ignored, as before.
fn version_requested(mut args: impl Iterator<Item = String>) -> bool {
    args.nth(1).is_some_and(|a| a == "--version" || a == "-V")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if version_requested(std::env::args()) {
        println!("vxmcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

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

#[cfg(test)]
mod tests {
    use super::version_requested;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn version_flag_only_as_first_argument() {
        assert!(version_requested(args(&["vxmcp", "--version"])));
        assert!(version_requested(args(&["vxmcp", "-V"])));
        assert!(!version_requested(args(&["vxmcp"])));
        assert!(!version_requested(args(&["vxmcp", "serve", "--version"])));
    }
}
