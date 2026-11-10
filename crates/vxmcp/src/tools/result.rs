//! `ResultService` tools, each one RPC, all read-only.
//! - `result_list` → `List`: archives by campaign or under a workspace's `results/`.
//! - `result_get` → `Get`: one archive by digest, from its own `metadata.json`.
//! - `result_verdict` → `Verdict`: the experiment's criteria judged over a campaign's archives,
//!   typed per criterion, with the trial values behind each.
//! - `result_compare` → `Compare`: a campaign against a reference campaign, per metric at each
//!   point and per criterion.

use rmcp::model::CallToolResult;
use vxedge_api::v1::result_service_client::ResultServiceClient;
use vxedge_api::v1::{
    ResultCompareRequest, ResultGetRequest, ResultListRequest, ResultVerdictRequest,
};

use super::experiment::{path_ref, resolve_workspace};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResultListArgs {
    /// Only this campaign's trial archives.
    #[serde(default)]
    pub campaign_id: Option<String>,
    /// Also every archive under this workspace's `results/` folder.
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResultGetArgs {
    /// `sha256:<hex>` of the archive, from `result_list` or a campaign's trials.
    pub digest: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResultVerdictArgs {
    /// The campaign whose archives are judged, from `campaign_list`.
    pub campaign_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResultCompareArgs {
    /// The campaign compared.
    pub campaign_id: String,
    /// The campaign it is compared with. Both are judged by the first one's experiment.
    pub reference_campaign_id: String,
}

pub async fn list(
    channel: tonic::transport::Channel,
    args: ResultListArgs,
) -> anyhow::Result<CallToolResult> {
    let list = ResultServiceClient::new(channel)
        .list(ResultListRequest {
            campaign_id: args.campaign_id.unwrap_or_default(),
            workspace: args
                .workspace
                .map(|w| resolve_workspace(&w))
                .and_then(path_ref),
        })
        .await?
        .into_inner();
    super::out::json(&list)
}

pub async fn get(
    channel: tonic::transport::Channel,
    args: ResultGetArgs,
) -> anyhow::Result<CallToolResult> {
    let info = ResultServiceClient::new(channel)
        .get(ResultGetRequest {
            digest: args.digest,
        })
        .await?
        .into_inner();
    super::out::json(&info)
}

pub async fn verdict(
    channel: tonic::transport::Channel,
    args: ResultVerdictArgs,
) -> anyhow::Result<CallToolResult> {
    let v = ResultServiceClient::new(channel)
        .verdict(ResultVerdictRequest {
            campaign_id: args.campaign_id,
        })
        .await?
        .into_inner();
    super::out::json(&v)
}

pub async fn compare(
    channel: tonic::transport::Channel,
    args: ResultCompareArgs,
) -> anyhow::Result<CallToolResult> {
    let c = ResultServiceClient::new(channel)
        .compare(ResultCompareRequest {
            campaign_id: args.campaign_id,
            reference_campaign_id: args.reference_campaign_id,
        })
        .await?
        .into_inner();
    super::out::json(&c)
}
