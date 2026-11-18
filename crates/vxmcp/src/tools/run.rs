//! `RunService` tools. A run is what `experiment up` or `vehicle up` brought up on this host.
//! - `run_list` → `List`: every run, with its id, workspace and start time.
//! - `run_get` → `Get`: one run; an omitted id means the only run.
//! - `run_status` → `Status`: every run's status, or the named runs', in one call.
//! - `run_down` → `Down`: several runs, or every run, down as one `run.down` job.

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;
use vxedge_api::v1::run_service_client::RunServiceClient;
use vxedge_api::v1::{
    Run, RunDownRequest, RunGetRequest, RunListRequest, RunRef, RunStatusRequest,
};

/// Optional run addressing, shared by every tool that acts on a run.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct RunArgs {
    /// The run id from `run_list`. Omit it to mean the only run on this host. vxedged refuses
    /// a stale id with `ERROR_REASON_RUN_MISMATCH`, and a call with no run up with
    /// `ERROR_REASON_NO_ACTIVE_RUN`.
    #[serde(default)]
    pub run_id: Option<String>,
}

/// The runs to report on, for `run_status`.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct RunStatusArgs {
    /// Run ids from `run_list`. Omit or leave empty for every run on this host. A named run that
    /// isn't up gets an entry with `error_reason` instead of failing the call.
    #[serde(default)]
    pub run_ids: Vec<String>,
}

/// The runs to take down, for `run_down`: `run_ids` or `all`, not both.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct RunDownArgs {
    /// Run ids from `run_list`. vxedged refuses the whole call if any of them isn't up.
    #[serde(default)]
    pub run_ids: Vec<String>,
    /// Every run on this host, as it stands when the job runs.
    #[serde(default)]
    pub all: bool,
}

fn refs(run_ids: Vec<String>) -> Vec<RunRef> {
    run_ids
        .into_iter()
        .map(|run_id| RunRef { run_id })
        .collect()
}

/// Checks `run_status`'s arguments: no empty or repeated id.
pub fn check_status(args: &RunStatusArgs) -> Result<(), McpError> {
    check_ids(&args.run_ids)
}

fn check_ids(run_ids: &[String]) -> Result<(), McpError> {
    let bad = |m: String| Err(McpError::invalid_params(m, None));
    if run_ids.iter().any(|r| r.is_empty()) {
        return bad("a run id in run_ids is empty".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    if let Some(r) = run_ids.iter().find(|r| !seen.insert(r.as_str())) {
        return bad(format!("run `{r}` is named twice in run_ids"));
    }
    Ok(())
}

/// Checks `run_down`'s arguments before the human is asked: `run_ids` or `all`, exactly one, and
/// no empty or repeated id.
pub fn check_down(args: &RunDownArgs) -> Result<(), McpError> {
    let bad = |m: &str| Err(McpError::invalid_params(m.to_string(), None));
    match (args.run_ids.is_empty(), args.all) {
        (true, false) => return bad("name the runs in run_ids, or set all"),
        (false, true) => return bad("set run_ids or all, not both"),
        _ => {}
    }
    check_ids(&args.run_ids)
}

/// One run as a confirmation names it: id, slot and workspace.
fn run_line(run: &Run) -> String {
    let slot = if run.instance.is_empty() {
        String::new()
    } else {
        format!("slot {}, ", run.instance)
    };
    format!("`{}` ({slot}{})", run.id, run.target_id)
}

/// The question `run_down` asks. `up` is what `run_list` answered when the question was put, or
/// why it could not be read; it names the runs, and is never what the job acts on.
pub fn down_confirmation(args: &RunDownArgs, up: &Result<Vec<Run>, String>) -> String {
    let which = if args.all {
        let now = match up {
            Ok(runs) if runs.is_empty() => "none is up now".to_string(),
            Ok(runs) => {
                let runs: Vec<String> = runs.iter().map(run_line).collect();
                format!("up now: {}", runs.join(", "))
            }
            Err(e) => format!("vxmcp could not list them: {e}"),
        };
        format!("every run on this host as it stands when the job runs ({now})")
    } else {
        let named: Vec<String> = args
            .run_ids
            .iter()
            .map(|id| match up {
                Ok(runs) => runs
                    .iter()
                    .find(|r| &r.id == id)
                    .map(run_line)
                    .unwrap_or_else(|| format!("`{id}` (not up now)")),
                Err(_) => format!("`{id}`"),
            })
            .collect();
        format!(
            "run{} {}",
            if named.len() == 1 { "" } else { "s" },
            named.join(", ")
        )
    };
    format!(
        "Tear down {which}, one after another: each stops its traffic and measurements, then its \
         services, nodes and gateway side. vxedged refuses the whole call, before touching \
         anything, if a running campaign holds any of their slots or a named run isn't up. This \
         affects anyone using this host's testbed. Confirm?"
    )
}

/// The runs up now, for `run_down`'s question.
pub async fn up_now(channel: tonic::transport::Channel) -> Result<Vec<Run>, String> {
    RunServiceClient::new(channel)
        .list(RunListRequest {})
        .await
        .map(|r| r.into_inner().runs)
        .map_err(|e| e.message().to_string())
}

pub async fn status(
    channel: tonic::transport::Channel,
    args: RunStatusArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = RunServiceClient::new(channel)
        .status(RunStatusRequest {
            runs: refs(args.run_ids),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

pub async fn down(
    channel: tonic::transport::Channel,
    args: RunDownArgs,
) -> anyhow::Result<CallToolResult> {
    let job = RunServiceClient::new(channel)
        .down(RunDownRequest {
            runs: refs(args.run_ids),
            all: args.all,
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

pub async fn list(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let resp = RunServiceClient::new(channel)
        .list(RunListRequest {})
        .await?
        .into_inner();
    super::out::json(&resp)
}

pub async fn get(
    channel: tonic::transport::Channel,
    args: RunArgs,
) -> anyhow::Result<CallToolResult> {
    let resp = RunServiceClient::new(channel)
        .get(RunGetRequest {
            run: super::out::run_ref(args.run_id),
        })
        .await?
        .into_inner();
    super::out::json(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, slot: &str, ws: &str) -> Run {
        Run {
            id: id.into(),
            instance: slot.into(),
            target_id: ws.into(),
            ..Default::default()
        }
    }

    #[test]
    fn run_down_takes_run_ids_or_all() {
        let ok = |v| check_down(&serde_json::from_value(v).unwrap()).is_ok();
        assert!(!ok(serde_json::json!({})));
        assert!(!ok(serde_json::json!({ "run_ids": ["r1"], "all": true })));
        assert!(!ok(serde_json::json!({ "run_ids": ["r1", ""] })));
        assert!(!ok(serde_json::json!({ "run_ids": ["r1", "r1"] })));
        assert!(ok(serde_json::json!({ "run_ids": ["r1", "r2"] })));
        assert!(ok(serde_json::json!({ "all": true })));
        let s: RunStatusArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(s.run_ids.is_empty() && check_status(&s).is_ok());
        let st = |v| check_status(&serde_json::from_value(v).unwrap()).is_ok();
        assert!(!st(serde_json::json!({ "run_ids": [""] })));
        assert!(!st(serde_json::json!({ "run_ids": ["r1", "r1"] })));
    }

    #[test]
    fn run_down_names_the_runs_it_asks_about() {
        let up = Ok(vec![run("r1", "0", "exp-a"), run("r2", "1", "exp-b")]);
        let all = RunDownArgs {
            all: true,
            ..Default::default()
        };
        let q = down_confirmation(&all, &up);
        assert!(
            q.starts_with(
                "Tear down every run on this host as it stands when the job runs (up now: \
                 `r1` (slot 0, exp-a), `r2` (slot 1, exp-b)), one after another"
            ),
            "{q}"
        );
        assert!(down_confirmation(&all, &Ok(vec![])).contains("(none is up now)"));
        assert!(
            down_confirmation(&all, &Err("unavailable".into()))
                .contains("(vxmcp could not list them: unavailable)")
        );
        let named = RunDownArgs {
            run_ids: vec!["r2".into(), "r9".into()],
            all: false,
        };
        assert!(
            down_confirmation(&named, &up)
                .starts_with("Tear down runs `r2` (slot 1, exp-b), `r9` (not up now), one after")
        );
        let one = RunDownArgs {
            run_ids: vec!["r1".into()],
            all: false,
        };
        let q = down_confirmation(&one, &Err("x".into()));
        assert!(q.starts_with("Tear down run `r1`, one after"), "{q}");
        assert!(q.ends_with("This affects anyone using this host's testbed. Confirm?"));
    }

    #[test]
    fn run_args_are_optional() {
        let a: RunArgs = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(a.run_id, None);
        let a: RunArgs = serde_json::from_value(serde_json::json!({ "run_id": "r1" })).unwrap();
        assert_eq!(a.run_id.as_deref(), Some("r1"));
    }

    /// An agent comparing two runs' workspace identities needs the rule each was computed under
    /// (vxedge 0.5.3, `ARCHITECTURE.md` §13): the value alone is not comparable across rules.
    #[test]
    fn a_run_names_its_workspace_identity_algorithm() {
        let run = vxedge_api::v1::Run {
            workspace_identity_algorithm: "vxws/2".into(),
            ..Default::default()
        };
        let v = serde_json::to_value(&run).unwrap();
        assert_eq!(v["workspaceIdentityAlgorithm"], "vxws/2", "{v}");
    }

    /// A run says which package it came from and which kernel it ran on (vxedge 0.7.0,
    /// `ARCHITECTURE.md` §13), so an agent cites results by package digest and keeps
    /// PREEMPT_RT and stock-kernel runs apart.
    #[test]
    fn a_run_names_its_package_and_kernel() {
        let run = vxedge_api::v1::Run {
            package: Some(vxedge_api::v1::PackageRef {
                id: "vxedge/demo".into(),
                version: "0.1.0".into(),
                digest: "sha256:abc".into(),
            }),
            package_locked: true,
            kernel: Some(vxedge_api::v1::Kernel {
                release: "6.12.0-rt".into(),
                preempt_rt: true,
            }),
            ..Default::default()
        };
        let v = serde_json::to_value(&run).unwrap();
        assert_eq!(v["package"]["digest"], "sha256:abc", "{v}");
        assert_eq!(v["packageLocked"], true, "{v}");
        assert_eq!(v["kernel"]["preemptRt"], true, "{v}");
    }
}
