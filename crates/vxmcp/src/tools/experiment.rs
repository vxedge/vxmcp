//! `ExperimentService` tools, each one RPC. An MCP call has no meaningful working directory, so
//! every tool takes an explicit `workspace` path, absolutized here ([`resolve_workspace`]) and
//! resolved by `vxedged` like any `Source::Path` ref.
//!
//! - `experiment_status` → `Status`.
//! - `experiment_up` / `experiment_down` → `Up` / `Down`, behind `crate::confirm`. The path is
//!   resolved once and that same string is confirmed and sent, so a ticket can't cover a
//!   different target than the RPC acts on. Down refuses unless the workspace is the experiment
//!   active on this host (vxedge `e3753be`); its confirmation says so.

use rmcp::model::CallToolResult;
use vxedge_api::v1::experiment_service_client::ExperimentServiceClient;
use vxedge_api::v1::manifest_ref::Source;
use vxedge_api::v1::{
    ExperimentDownRequest, ExperimentStatusRequest, ExperimentUpRequest, ManifestRef,
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExperimentStatusArgs {
    /// Path to the experiment workspace — the directory holding its documents and its descriptor
    /// (`vxp.yaml`). Relative paths are resolved against this server's
    /// own working directory, not the caller's.
    pub workspace: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExperimentUpArgs {
    /// Path to the experiment workspace to bring up — the directory holding its documents and
    /// its descriptor (`vxp.yaml`). A `vxp/1` package that does not
    /// match its `vxp.lock` is refused with `ERROR_REASON_LOCK_MISMATCH`, naming each difference.
    /// You will be asked to confirm.
    pub workspace: String,
    /// Stages to run, as `vxedge experiment up --stage` spells them: `gateway`, `zones`,
    /// `services`, `compute`, `signal-plane`. Omitted or empty means every stage.
    #[serde(default)]
    pub stages: Vec<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExperimentDownArgs {
    /// Path to the experiment workspace to tear down — the directory holding its documents and
    /// its descriptor. It must be the experiment active on this host; vxedged refuses anything else. You
    /// will be asked to confirm.
    pub workspace: String,
    /// Stages to tear down, spelled as for `experiment_up`. Omitted or empty means every stage,
    /// in reverse dependency order.
    #[serde(default)]
    pub stages: Vec<String>,
}

/// Absolutize a workspace path client-side so the daemon resolves it correctly regardless of
/// its own working directory — the exact reason `vxedge-cli`'s own `manifest_ref()` does the
/// same thing. A path that doesn't exist passes through unchanged for vxedged to report.
pub fn resolve_workspace(workspace: &str) -> String {
    std::path::Path::new(workspace)
        .canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| workspace.to_string())
}

/// A `Source::Path` ref for an already-resolved workspace path.
pub fn path_ref(abs: String) -> Option<ManifestRef> {
    Some(ManifestRef {
        source: Some(Source::Path(abs)),
        sha256: String::new(),
        // Unused for a Source::Path ref sent by an external caller — matches vxedge-cli's own
        // manifest_ref() and its comment on why.
        resolved_dir: String::new(),
    })
}

/// How a confirmation names the stages a call covers.
pub fn stage_words(stages: &[String]) -> String {
    if stages.is_empty() {
        "every stage".to_string()
    } else {
        format!("stages {}", stages.join(", "))
    }
}

/// The question the human sees before `experiment_up` runs, for an already-resolved path.
pub fn up_confirmation(resolved_workspace: &str, stages: &[String]) -> String {
    format!(
        "Bring up the experiment at `{resolved_workspace}` ({}): provisions this host's gateway \
         and the zones, services, compute and signal plane it declares. vxedged checks the \
         host first and refuses if it can't run it or another job holds it. This affects \
         anyone using this host's testbed. Confirm?",
        stage_words(stages)
    )
}

/// The question the human sees before `experiment_down` runs, for an already-resolved path.
/// Checked against vxedged's `ExperimentService.Down` (design doc: confirmation text matches
/// the daemon, not the RPC's name).
pub fn down_confirmation(resolved_workspace: &str, stages: &[String]) -> String {
    format!(
        "Tear down the experiment at `{resolved_workspace}` ({}): stops the services it \
         declares, its zones, and this host's gateway. vxedged refuses if it isn't the experiment \
         active on this host. This affects anyone using this host's testbed. Confirm?",
        stage_words(stages)
    )
}

pub async fn up(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    stages: &[String],
) -> anyhow::Result<CallToolResult> {
    let stages = super::out::stages(stages)?;
    let job = ExperimentServiceClient::new(channel)
        .up(ExperimentUpRequest {
            manifest: path_ref(resolved_workspace),
            stages,
            ..Default::default()
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn down(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    stages: &[String],
) -> anyhow::Result<CallToolResult> {
    let stages = super::out::stages(stages)?;
    let job = ExperimentServiceClient::new(channel)
        .down(ExperimentDownRequest {
            manifest: path_ref(resolved_workspace),
            stages,
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: ExperimentStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = ExperimentServiceClient::new(channel)
        .status(ExperimentStatusRequest {
            manifest: path_ref(resolve_workspace(&args.workspace)),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_args_deserialize_from_a_bare_workspace_field() {
        let args: ExperimentDownArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/tmp/exp-003-ws" })).unwrap();
        assert_eq!(args.workspace, "/tmp/exp-003-ws");
        assert!(args.stages.is_empty());
    }

    #[test]
    fn down_confirmation_names_the_workspace_and_what_it_stops() {
        assert_eq!(
            down_confirmation("/ws/exp-003-ws", &[]),
            "Tear down the experiment at `/ws/exp-003-ws` (every stage): stops the services it \
             declares, its zones, and this host's gateway. vxedged refuses if it isn't the \
             experiment active on this host. This affects anyone using this host's testbed. \
             Confirm?"
        );
    }

    #[test]
    fn up_confirmation_names_the_stages_and_ends_with_the_shared_host_line() {
        let text = up_confirmation("/ws/e", &["gateway".into(), "zones".into()]);
        assert!(text.starts_with("Bring up the experiment at `/ws/e` (stages gateway, zones)"));
        assert!(text.ends_with("This affects anyone using this host's testbed. Confirm?"));
    }

    #[test]
    fn resolve_workspace_absolutizes_an_existing_directory() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        assert_eq!(resolve_workspace("."), cwd.display().to_string());
    }

    #[test]
    fn resolve_workspace_passes_a_missing_path_through_unchanged() {
        // vxedged reports the real error for a path that doesn't exist; vxmcp doesn't guess.
        assert_eq!(resolve_workspace("/no/such/vxmcp-ws"), "/no/such/vxmcp-ws");
    }

    #[test]
    fn args_deserialize_from_a_bare_workspace_field() {
        let args: ExperimentStatusArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/tmp/exp-005" })).unwrap();
        assert_eq!(args.workspace, "/tmp/exp-005");
    }
}
