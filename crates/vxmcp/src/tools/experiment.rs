//! `experiment_status` — the first tool, proving the wrapper pattern end to end. A direct 1:1
//! call to `ExperimentService.Status`, the same RPC `vxedge experiment status` calls; the only
//! difference from the CLI is *how* the workspace is addressed. The CLI resolves it from its
//! own working directory (`vxedge_core::discover`, terraform-style); an MCP tool call has no
//! meaningful "current directory" of its own caller, so this takes an explicit `workspace`
//! argument instead and lets `vxedged` resolve it server-side from the absolutized path — the
//! daemon already does this resolution for a `Source::Path` ref regardless of caller, so no
//! client-side discovery logic needs duplicating here.

use rmcp::model::{CallToolResult, ContentBlock};
use vxedge_api::v1::experiment_service_client::ExperimentServiceClient;
use vxedge_api::v1::manifest_ref::Source;
use vxedge_api::v1::{ExperimentStatusRequest, ManifestRef};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExperimentStatusArgs {
    /// Path to the experiment workspace — the directory containing its `.vxp` file. Relative
    /// paths are resolved against this server's own working directory, not the caller's.
    pub workspace: String,
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: ExperimentStatusArgs,
) -> anyhow::Result<CallToolResult> {
    // Absolutize client-side so the daemon resolves it correctly regardless of its own working
    // directory — the exact reason `vxedge-cli`'s own `manifest_ref()` does the same thing.
    let abs = std::path::Path::new(&args.workspace)
        .canonicalize()
        .map(|p| p.display().to_string())
        .unwrap_or(args.workspace);

    let mut client = ExperimentServiceClient::new(channel);
    let resp = client
        .status(ExperimentStatusRequest {
            manifest: Some(ManifestRef {
                source: Some(Source::Path(abs)),
                sha256: String::new(),
                // Unused for a Source::Path ref sent by an external caller — matches
                // vxedge-cli's own manifest_ref() and its comment on why.
                resolved_dir: String::new(),
            }),
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
    fn args_deserialize_from_a_bare_workspace_field() {
        let args: ExperimentStatusArgs =
            serde_json::from_value(serde_json::json!({ "workspace": "/tmp/exp-005" })).unwrap();
        assert_eq!(args.workspace, "/tmp/exp-005");
    }
}
