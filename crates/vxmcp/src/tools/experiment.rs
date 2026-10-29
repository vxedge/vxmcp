//! `experiment_status` — the first tool, proving the wrapper pattern end to end. A direct 1:1
//! call to `ExperimentService.Status`, the same RPC `vxedge experiment status` calls; the only
//! difference from the CLI is *how* the workspace is addressed. The CLI resolves it from its
//! own working directory (`vxedge_core::discover`, terraform-style); an MCP tool call has no
//! meaningful "current directory" of its own caller, so this takes an explicit `workspace`
//! argument instead and lets `vxedged` resolve it server-side from the absolutized path — the
//! daemon already does this resolution for a `Source::Path` ref regardless of caller, so no
//! client-side discovery logic needs duplicating here.
//!
//! `experiment_down` wraps `ExperimentService.Down`, all stages, behind `crate::confirm`. At the
//! pinned vxedge rev (`e3753be`) Down stops the services the workspace declares, the active
//! experiment's zones and the host's gateway, and refuses unless the workspace *is* the active
//! experiment — its confirmation says exactly that. The path is resolved once
//! ([`resolve_workspace`]) and that same string is both confirmed and sent, so a ticket can't
//! cover a different target than the RPC acts on.

use rmcp::model::{CallToolResult, ContentBlock};
use vxedge_api::v1::experiment_service_client::ExperimentServiceClient;
use vxedge_api::v1::manifest_ref::Source;
use vxedge_api::v1::{ExperimentDownRequest, ExperimentStatusRequest, ManifestRef};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExperimentStatusArgs {
    /// Path to the experiment workspace — the directory containing its `.vxp` file. Relative
    /// paths are resolved against this server's own working directory, not the caller's.
    pub workspace: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExperimentDownArgs {
    /// Path to the experiment workspace to tear down — the directory containing its `.vxp`
    /// file. It must be the experiment active on this host; vxedged refuses anything else. You
    /// will be asked to confirm.
    pub workspace: String,
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

fn path_ref(abs: String) -> Option<ManifestRef> {
    Some(ManifestRef {
        source: Some(Source::Path(abs)),
        sha256: String::new(),
        // Unused for a Source::Path ref sent by an external caller — matches vxedge-cli's own
        // manifest_ref() and its comment on why.
        resolved_dir: String::new(),
    })
}

/// The question the human sees before `experiment_down` runs, for an already-resolved path.
/// Checked against vxedged's `ExperimentService.Down` at `e3753be` (design doc: confirmation
/// text matches the daemon, not the RPC's name).
pub fn down_confirmation(resolved_workspace: &str) -> String {
    format!(
        "Tear down the experiment at `{resolved_workspace}`: stops the services it declares, its \
         zones, and this host's gateway. vxedged refuses if it isn't the experiment active on \
         this host. This affects anyone using this host's testbed. Confirm?"
    )
}

/// Takes the *resolved* path — the same value the confirmation was bound to.
pub async fn down(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
) -> anyhow::Result<CallToolResult> {
    let mut client = ExperimentServiceClient::new(channel);
    let job = client
        .down(ExperimentDownRequest {
            manifest: path_ref(resolved_workspace),
            // Empty = all stages, in reverse dependency order.
            stages: Vec::new(),
        })
        .await?
        .into_inner();
    Ok(CallToolResult::success(vec![ContentBlock::text(
        super::job::render(&job),
    )]))
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: ExperimentStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let mut client = ExperimentServiceClient::new(channel);
    let resp = client
        .status(ExperimentStatusRequest {
            manifest: path_ref(resolve_workspace(&args.workspace)),
        })
        .await?
        .into_inner();

    let top = resp.status.clone().unwrap_or_default();
    let mut lines = vec![format!("{}: {}", top.vxid, top.detail)];
    for c in &resp.components {
        // Same `state={}` convention vxedge-cli's own status output already uses throughout —
        // a bare ResourceState ordinal, not yet a named string anywhere in this codebase.
        // Matching it here rather than diverging in just one frontend.
        lines.push(format!("  {} [state={}] {}", c.vxid, c.state, c.detail));
    }

    Ok(CallToolResult::success(vec![ContentBlock::text(
        lines.join("\n"),
    )]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_args_deserialize_from_a_bare_workspace_field() {
        let args: ExperimentDownArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/tmp/exp-003-ws" })).unwrap();
        assert_eq!(args.workspace, "/tmp/exp-003-ws");
    }

    #[test]
    fn down_confirmation_names_the_workspace_and_what_it_stops() {
        assert_eq!(
            down_confirmation("/ws/exp-003-ws"),
            "Tear down the experiment at `/ws/exp-003-ws`: stops the services it declares, its \
             zones, and this host's gateway. vxedged refuses if it isn't the experiment active on \
             this host. This affects anyone using this host's testbed. Confirm?"
        );
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
