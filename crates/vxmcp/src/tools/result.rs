//! `ResultService` tools, each one RPC, all read-only.
//! - `result_list` → `List`: archives by campaign or under a workspace's `results/`.
//! - `result_get` → `Get`: one archive by digest, from its own `metadata.json`.
//! - `result_verdict` → `Verdict`: the experiment's criteria judged over a campaign's archives,
//!   typed per criterion, with the trial values behind each, and the reference campaigns its
//!   criteria read.
//! - `result_compare` → `Compare`: a campaign against a reference campaign, per metric at each
//!   point and per criterion. The reference is one this host ran, or one recorded in a
//!   workspace's `results/`.
//! - `result_fetch` → `Fetch` (server streaming), behind `crate::confirm`: an archive's bytes,
//!   written to a file where this server runs and checked against the digest they were asked by,
//!   so an agent that shares no filesystem with the daemon can still re-hash a result.

use rmcp::model::CallToolResult;
use sha2::Digest;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use vxedge_api::v1::result_service_client::ResultServiceClient;
use vxedge_api::v1::{
    ResultCompareRequest, ResultFetchRequest, ResultGetRequest, ResultListRequest,
    ResultVerdictRequest,
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
    /// For criteria that read a reference campaign the experiment declares (`references`) and
    /// this host did not run, such as a frozen one: the workspace (a path, or a `sha256:`
    /// package digest) whose `results/<campaign>/campaign.json` records it.
    #[serde(default)]
    pub reference_workspace: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResultCompareArgs {
    /// The campaign compared.
    pub campaign_id: String,
    /// The campaign it is compared with. Both are judged by the first one's experiment.
    pub reference_campaign_id: String,
    /// For a reference this host did not run, such as a frozen campaign published with its
    /// archives: the workspace (a path, or a `sha256:` package digest) whose
    /// `results/<reference_campaign_id>/campaign.json` records it. Its archives must hash to that
    /// record.
    #[serde(default)]
    pub reference_workspace: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResultFetchArgs {
    /// `sha256:<hex>` of the archive, from `result_list` or a campaign's trials.
    pub digest: String,
    /// The file to write, where this server runs. A relative path is under the server's working
    /// folder. An existing file is replaced. You will be asked to confirm.
    pub out_path: String,
}

/// Where `result_fetch` writes: `out_path`, absolute, against this server's working folder.
pub fn fetch_path(out_path: &str) -> String {
    let p = std::path::Path::new(out_path);
    if p.is_absolute() {
        return out_path.to_string();
    }
    std::env::current_dir()
        .map(|d| d.join(p).display().to_string())
        .unwrap_or_else(|_| out_path.to_string())
}

/// The question the human sees before `result_fetch` writes.
pub fn fetch_confirmation(digest: &str, path: &str) -> String {
    format!(
        "Download the results archive `{digest}` from vxedged and write it to `{path}`, where \
         this server runs, replacing any file there. Nothing on the testbed changes. This affects \
         anyone using this host's testbed. Confirm?"
    )
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
            reference_workspace: args
                .reference_workspace
                .as_deref()
                .and_then(super::package::package_ref),
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
            reference_workspace: args
                .reference_workspace
                .as_deref()
                .and_then(super::package::package_ref),
        })
        .await?
        .into_inner();
    super::out::json(&c)
}

/// Streams the archive into `<path>.partial`, checks its sha256 against `digest`, then moves it
/// into place. A mismatch keeps nothing.
pub async fn fetch(
    channel: tonic::transport::Channel,
    digest: String,
    path: String,
) -> anyhow::Result<CallToolResult> {
    let mut stream = ResultServiceClient::new(channel)
        .fetch(ResultFetchRequest {
            digest: digest.clone(),
        })
        .await?
        .into_inner();
    let partial = format!("{path}.partial");
    let mut file = tokio::fs::File::create(&partial).await?;
    let (mut hasher, mut size) = (sha2::Sha256::new(), 0u64);
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(status) => {
                drop(file);
                let _ = tokio::fs::remove_file(&partial).await;
                return Err(status.into());
            }
        };
        hasher.update(&chunk.data);
        size += chunk.data.len() as u64;
        file.write_all(&chunk.data).await?;
    }
    file.flush().await?;
    drop(file);
    let actual = format!("sha256:{}", hex(&hasher.finalize()));
    if actual != digest {
        let _ = tokio::fs::remove_file(&partial).await;
        anyhow::bail!("the bytes vxedged sent hash to {actual}, not {digest}; nothing was kept");
    }
    tokio::fs::rename(&partial, &path).await?;
    Ok(CallToolResult::structured(serde_json::json!({
        "path": path,
        "digest": actual,
        "sizeBytes": size,
    })))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_writes_an_absolute_path_and_says_where() {
        assert_eq!(fetch_path("/r/a.zip"), "/r/a.zip");
        assert!(fetch_path("a.zip").ends_with("/a.zip"));
        assert!(std::path::Path::new(&fetch_path("a.zip")).is_absolute());
        let q = fetch_confirmation("sha256:ab", "/r/a.zip");
        assert!(q.contains("`sha256:ab`") && q.contains("`/r/a.zip`"));
        assert!(q.ends_with("This affects anyone using this host's testbed. Confirm?"));
    }

    #[test]
    fn fetch_args_need_a_digest_and_a_path() {
        let a: ResultFetchArgs =
            serde_json::from_value(serde_json::json!({"digest": "sha256:ab", "out_path": "x"}))
                .unwrap();
        assert_eq!((a.digest.as_str(), a.out_path.as_str()), ("sha256:ab", "x"));
        assert!(
            serde_json::from_value::<ResultFetchArgs>(serde_json::json!({"digest": "d"})).is_err()
        );
    }
}
