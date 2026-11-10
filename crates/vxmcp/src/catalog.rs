//! Resources and prompts (`ARCHITECTURE.md` §5.1): what an agent can read without a tool call,
//! and recipes that are sequences of the tools, not new behaviour.
//!
//! **Resources**, each read with one RPC:
//! - `vxp://catalog`: the packages in this host's cache (`PackageService.List`).
//! - `vxp://{id}@{version}`, or `vxp://sha256:<hex>`: one cached package's typed descriptor
//!   (`PackageService.Inspect` by digest; an id and version are found in `List` first).
//! - `vxresult://{campaign_id}/verdict`: a campaign's verdict (`ResultService.Verdict`).
//!
//! **Prompts:** `reproduce_result`, `find_knee` and `compare_baselines`.

use rmcp::ErrorData as McpError;
use rmcp::model::{Prompt, PromptArgument, PromptMessage, Role};
use vxedge_api::v1::manifest_ref::Source;
use vxedge_api::v1::package_service_client::PackageServiceClient;
use vxedge_api::v1::result_service_client::ResultServiceClient;
use vxedge_api::v1::{
    ManifestRef, PackageInspectRequest, PackageListRequest, ResultVerdictRequest,
};

/// What a resource URI names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uri {
    Catalog,
    PackageDigest(String),
    Package { id: String, version: String },
    Verdict(String),
}

pub const CATALOG: &str = "vxp://catalog";
pub const PACKAGE_TEMPLATE: &str = "vxp://{id}@{version}";
pub const VERDICT_TEMPLATE: &str = "vxresult://{campaign_id}/verdict";

/// Parse a resource URI this server serves, or `None`. A package id may contain `/`
/// (`vxedge/h1-slice-isolation`), so the version follows the last `@`.
pub fn parse(uri: &str) -> Option<Uri> {
    if uri == CATALOG {
        return Some(Uri::Catalog);
    }
    if let Some(rest) = uri.strip_prefix("vxp://") {
        if rest.starts_with("sha256:") {
            return Some(Uri::PackageDigest(rest.to_string()));
        }
        let (id, version) = rest.rsplit_once('@')?;
        return (!id.is_empty() && !version.is_empty()).then(|| Uri::Package {
            id: id.to_string(),
            version: version.to_string(),
        });
    }
    let campaign = uri.strip_prefix("vxresult://")?.strip_suffix("/verdict")?;
    (!campaign.is_empty() && !campaign.contains('/')).then(|| Uri::Verdict(campaign.to_string()))
}

/// A refusal from `vxedged` while reading a resource: not found when it is one, with the typed
/// reason in `data` either way.
fn refused(status: tonic::Status) -> McpError {
    let body = crate::tools::out::rpc_error(&status).structured_content;
    match status.code() {
        tonic::Code::NotFound => McpError::resource_not_found(status.message().to_string(), body),
        _ => McpError::internal_error(status.message().to_string(), body),
    }
}

async fn inspect(
    channel: tonic::transport::Channel,
    digest: String,
) -> Result<serde_json::Value, McpError> {
    let info = PackageServiceClient::new(channel)
        .inspect(PackageInspectRequest {
            package: Some(ManifestRef {
                source: Some(Source::Package(digest)),
                ..Default::default()
            }),
        })
        .await
        .map_err(refused)?
        .into_inner();
    serde_json::to_value(info).map_err(|e| McpError::internal_error(e.to_string(), None))
}

/// The JSON a resource URI names, read with one RPC (two for a package named by id and
/// version: `List` finds its digest).
pub async fn read(
    channel: tonic::transport::Channel,
    uri: &str,
) -> Result<serde_json::Value, McpError> {
    let json = |v: Result<serde_json::Value, serde_json::Error>| {
        v.map_err(|e| McpError::internal_error(e.to_string(), None))
    };
    match parse(uri) {
        None => Err(McpError::resource_not_found(
            format!("`{uri}` is not a resource this server serves"),
            None,
        )),
        Some(Uri::Catalog) => {
            let list = PackageServiceClient::new(channel)
                .list(PackageListRequest {})
                .await
                .map_err(refused)?
                .into_inner();
            json(serde_json::to_value(list))
        }
        Some(Uri::PackageDigest(digest)) => inspect(channel, digest).await,
        Some(Uri::Package { id, version }) => {
            let list = PackageServiceClient::new(channel.clone())
                .list(PackageListRequest {})
                .await
                .map_err(refused)?
                .into_inner();
            let found = list
                .packages
                .into_iter()
                .find(|p| p.id == id && p.version == version)
                .ok_or_else(|| {
                    McpError::resource_not_found(
                        format!("no package `{id}` at `{version}` in this host's cache"),
                        None,
                    )
                })?;
            inspect(channel, found.digest).await
        }
        Some(Uri::Verdict(campaign_id)) => {
            let v = ResultServiceClient::new(channel)
                .verdict(ResultVerdictRequest { campaign_id })
                .await
                .map_err(refused)?
                .into_inner();
            json(serde_json::to_value(v))
        }
    }
}

fn arg(name: &str, description: &str, required: bool) -> PromptArgument {
    let mut a = PromptArgument::new(name).with_description(description);
    a.required = Some(required);
    a
}

/// The prompts this server offers.
pub fn prompts() -> Vec<Prompt> {
    vec![
        Prompt::new(
            "reproduce_result",
            Some(
                "Rerun an experiment's campaign and check it against a reference campaign: the \
                 verdict, and each metric's median against the reference's IQR.",
            ),
            Some(vec![
                arg("workspace", "Path to the experiment workspace.", true),
                arg(
                    "reference_campaign_id",
                    "The campaign to reproduce, from campaign_list.",
                    true,
                ),
                arg(
                    "profile",
                    "A profile the campaign declares, such as `quick`.",
                    false,
                ),
            ]),
        ),
        Prompt::new(
            "find_knee",
            Some(
                "Run a load sweep and find the first point where a metric leaves its low-load \
                 band.",
            ),
            Some(vec![
                arg(
                    "workspace",
                    "Path to the experiment workspace with a sweep.",
                    true,
                ),
                arg(
                    "parameter",
                    "The swept parameter, such as `load_mbps`.",
                    true,
                ),
                arg(
                    "metric",
                    "The metric id, from package_inspect's `metrics`.",
                    true,
                ),
            ]),
        ),
        Prompt::new(
            "compare_baselines",
            Some("Compare two finished campaigns, metric by metric and criterion by criterion."),
            Some(vec![
                arg("campaign_id", "The campaign compared.", true),
                arg(
                    "reference_campaign_id",
                    "The campaign it is compared with.",
                    true,
                ),
            ]),
        ),
    ]
}

fn get<'a>(args: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

/// The messages of prompt `name` with `args`, or why it cannot be built.
pub fn prompt(
    name: &str,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<PromptMessage>, String> {
    let need = |key: &str| get(args, key).ok_or(format!("`{name}` needs `{key}`"));
    let text = match name {
        "reproduce_result" => {
            let ws = need("workspace")?;
            let reference = need("reference_campaign_id")?;
            let profile = get(args, "profile")
                .map(|p| format!(" with profile `{p}`"))
                .unwrap_or_default();
            format!(
                "Reproduce campaign `{reference}` from the workspace `{ws}`{profile}.\n\
                 1. package_inspect `{ws}`: read its hypothesis, parameters, metrics and criteria.\n\
                 2. campaign_plan `{ws}`{profile}. Stop if it has problems or \
                 `vxmcpBudget.within` is false, and report why.\n\
                 3. campaign_start `{ws}`{profile} with a request_id of your choosing, so a retry \
                 resumes instead of starting over. The user confirms once.\n\
                 4. campaign_watch the campaign until `end` is `finished`; on `timed_out`, call it \
                 again.\n\
                 5. result_verdict the new campaign, and result_compare it with `{reference}`.\n\
                 6. Report the verdict's `outcome` and each criterion's, and how many metric \
                 points are `within` the reference's IQR, naming those outside. Branch on the \
                 typed outcomes, never on prose; `inconclusive` is not a pass."
            )
        }
        "find_knee" => {
            let ws = need("workspace")?;
            let parameter = need("parameter")?;
            let metric = need("metric")?;
            format!(
                "Find the knee of `{metric}` over the `{parameter}` sweep that `{ws}` declares.\n\
                 1. campaign_plan `{ws}`: list the sweep points of `{parameter}` and check the \
                 budget.\n\
                 2. campaign_start `{ws}` with a request_id, then campaign_watch until it \
                 finishes.\n\
                 3. result_verdict the campaign. Its `trials` give each trial's `{metric}` value \
                 with the trial's `{parameter}`.\n\
                 4. Per point, take the median across repetitions, and the IQR at the lowest \
                 point.\n\
                 5. The knee is the first point whose median is above the lowest point's median \
                 by more than that IQR, and stays above at every higher point. Report it with \
                 the medians, and say so if there is none or a trial was incomplete."
            )
        }
        "compare_baselines" => {
            let a = need("campaign_id")?;
            let b = need("reference_campaign_id")?;
            format!(
                "Compare campaign `{a}` with `{b}`.\n\
                 1. campaign_get both, and check each finished.\n\
                 2. result_compare `{a}` against `{b}`.\n\
                 3. Report `within` of the metric points, each point outside the reference's \
                 IQR with its delta, and each criterion whose outcome differs. Use the typed \
                 values only."
            )
        }
        _ => return Err(format!("no prompt `{name}`")),
    };
    Ok(vec![PromptMessage::new_text(Role::User, text)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_uris_parse() {
        assert_eq!(parse("vxp://catalog"), Some(Uri::Catalog));
        assert_eq!(
            parse("vxp://vxedge/h1-slice-isolation@1.2.0"),
            Some(Uri::Package {
                id: "vxedge/h1-slice-isolation".into(),
                version: "1.2.0".into()
            })
        );
        let d = format!("sha256:{}", "a".repeat(64));
        assert_eq!(parse(&format!("vxp://{d}")), Some(Uri::PackageDigest(d)));
        assert_eq!(
            parse("vxresult://c-1/verdict"),
            Some(Uri::Verdict("c-1".into()))
        );
        for bad in [
            "vxp://nover",
            "vxp://@1",
            "vxresult://c-1",
            "vxresult:///verdict",
            "file:///x",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn prompts_need_their_arguments_and_name_the_tools() {
        assert_eq!(prompts().len(), 3);
        let mut args = serde_json::Map::new();
        assert!(
            prompt("reproduce_result", &args)
                .unwrap_err()
                .contains("workspace")
        );
        args.insert("workspace".into(), "/ws/e".into());
        args.insert("reference_campaign_id".into(), "c-1".into());
        let text = format!("{:?}", prompt("reproduce_result", &args).unwrap());
        for tool in [
            "campaign_plan",
            "campaign_start",
            "result_verdict",
            "result_compare",
        ] {
            assert!(text.contains(tool), "{tool}");
        }
        assert!(prompt("nope", &args).is_err());
    }
}
