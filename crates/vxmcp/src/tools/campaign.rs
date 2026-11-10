//! `CampaignService` tools, each one RPC.
//! - `campaign_plan` → `Plan`: the trials a campaign runs, its estimated wall time and every
//!   typed problem, plus whether it fits this server's budgets.
//! - `campaign_start` → `Start`, behind `crate::confirm`: once per campaign, not per trial. The
//!   confirmation names the trial count, the estimated wall time and the profile, which it reads
//!   with `Plan` first; a campaign with plan problems, or over a budget, is refused before the
//!   human is asked.
//! - `campaign_get` / `campaign_list` → `Get` / `List`: a campaign's state, each trial's state,
//!   phase and archive, and the verdict recorded when it finished.
//! - `campaign_watch` → `Watch` (server streaming), followed until the campaign ends or a
//!   caller-chosen timeout passes, as `job_watch` does.
//!
//! **Budgets** are this server's caps, so an agent cannot start a campaign larger than its
//! operator allows: `VXMCP_MAX_TRIALS` (default 100) and `VXMCP_MAX_CAMPAIGN_S` (estimated wall
//! time in seconds, default 28800).

use std::time::Duration;

use rmcp::model::CallToolResult;
use tokio_stream::{Stream, StreamExt};
use vxedge_api::v1::campaign_service_client::CampaignServiceClient;
use vxedge_api::v1::{
    CampaignGetRequest, CampaignListRequest, CampaignPlan, CampaignPlanRequest,
    CampaignStartRequest, CampaignState, CampaignStatus, CampaignWatchRequest, ErrorReason,
};

use super::experiment::path_ref;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CampaignPlanArgs {
    /// Path to the experiment workspace whose `campaign` block to expand.
    pub workspace: String,
    /// A profile the campaign declares, such as `quick`. Omitted means none.
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CampaignStartArgs {
    /// Path to the experiment workspace. You will be asked to confirm once for the whole
    /// campaign.
    pub workspace: String,
    #[serde(default)]
    pub profile: Option<String>,
    /// An id of your choosing. Retrying with the same id resumes that campaign and skips the
    /// trials whose archives exist.
    #[serde(default)]
    pub request_id: Option<String>,
    /// A label recorded with the campaign, after `vxmcp:`.
    #[serde(default)]
    pub caller: Option<String>,
    /// Skip each trial's `vxp.lock` check while authoring. Recorded with the campaign.
    #[serde(default)]
    pub unlocked: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CampaignIdArgs {
    /// The campaign id, from `campaign_list` or the job `campaign_start` returned.
    pub campaign_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CampaignWatchArgs {
    pub campaign_id: String,
    /// How long to follow it, in seconds: default 60, at most 300.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// This server's caps on a campaign.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    pub max_trials: u32,
    pub max_wall_s: f64,
}

impl Budget {
    pub fn from_env() -> Self {
        let read = |key: &str| std::env::var(key).ok().and_then(|v| v.parse::<f64>().ok());
        Budget {
            max_trials: read("VXMCP_MAX_TRIALS").map_or(100, |v| v as u32),
            max_wall_s: read("VXMCP_MAX_CAMPAIGN_S").unwrap_or(28_800.0),
        }
    }

    /// Why `plan` is over budget, if it is.
    pub fn exceeded(&self, plan: &CampaignPlan) -> Option<String> {
        let trials = plan.trials.len() as u32;
        if trials > self.max_trials {
            return Some(format!(
                "the campaign runs {trials} trials, more than this server's budget of {} \
                 (VXMCP_MAX_TRIALS)",
                self.max_trials
            ));
        }
        if plan.estimated_s > self.max_wall_s {
            return Some(format!(
                "the campaign is estimated at {:.0} s, more than this server's budget of {:.0} s \
                 (VXMCP_MAX_CAMPAIGN_S)",
                plan.estimated_s, self.max_wall_s
            ));
        }
        None
    }

    fn json(&self, plan: &CampaignPlan) -> serde_json::Value {
        let why = self.exceeded(plan);
        serde_json::json!({
            "maxTrials": self.max_trials,
            "maxCampaignS": self.max_wall_s,
            "within": why.is_none(),
            "why": why,
        })
    }
}

pub async fn fetch_plan(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    profile: Option<String>,
) -> Result<CampaignPlan, tonic::Status> {
    Ok(CampaignServiceClient::new(channel)
        .plan(CampaignPlanRequest {
            manifest: path_ref(resolved_workspace),
            profile: profile.unwrap_or_default(),
        })
        .await?
        .into_inner())
}

pub async fn plan(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: CampaignPlanArgs,
    budget: Budget,
) -> anyhow::Result<CallToolResult> {
    let plan = fetch_plan(channel, resolved_workspace, args.profile).await?;
    let mut value = serde_json::to_value(&plan)?;
    value["vxmcpBudget"] = budget.json(&plan);
    Ok(CallToolResult::structured(value))
}

/// Why `campaign_start` refuses `plan` before asking anyone: a plan problem, which `Start` would
/// refuse, or a budget.
pub fn refusal(plan: &CampaignPlan, budget: &Budget) -> Option<CallToolResult> {
    if let Some(p) = plan.problems.first() {
        let reason = ErrorReason::try_from(p.reason)
            .map(|r| r.as_str_name().to_string())
            .unwrap_or_default();
        return Some(CallToolResult::structured_error(serde_json::json!({
            "error": {
                "code": "FAILED_PRECONDITION",
                "reason": reason,
                "domain": "vxedge.v1",
                "message": p.detail,
                "metadata": { "subject": p.subject, "problems": plan.problems.len().to_string() },
            }
        })));
    }
    budget.exceeded(plan).map(|why| {
        CallToolResult::structured_error(serde_json::json!({
            "error": {
                "code": "RESOURCE_EXHAUSTED",
                "reason": "VXMCP_BUDGET_EXCEEDED",
                "domain": "vxmcp",
                "message": why,
                "metadata": {
                    "trials": plan.trials.len().to_string(),
                    "estimated_s": format!("{:.0}", plan.estimated_s),
                    "max_trials": budget.max_trials.to_string(),
                    "max_campaign_s": format!("{:.0}", budget.max_wall_s),
                },
            }
        }))
    })
}

/// The question the human sees once before `campaign_start` runs.
pub fn start_confirmation(resolved_workspace: &str, plan: &CampaignPlan) -> String {
    let profile = if plan.profile.is_empty() {
        "no profile".to_string()
    } else {
        format!("profile `{}`", plan.profile)
    };
    let minutes = (plan.estimated_s / 60.0).ceil();
    format!(
        "Run the campaign `{resolved_workspace}` declares, with {profile}: {} trials ({} points x \
         {} repetitions), each provisioned, measured, archived and torn down on its own, for \
         about {minutes} minutes. Only one campaign runs at a time on this host. This affects \
         anyone using this host's testbed. Confirm?",
        plan.trials.len(),
        plan.points,
        plan.repetitions
    )
}

pub async fn start(
    channel: tonic::transport::Channel,
    resolved_workspace: String,
    args: CampaignStartArgs,
) -> anyhow::Result<CallToolResult> {
    let caller = match args.caller {
        Some(label) if !label.is_empty() => format!("vxmcp:{label}"),
        _ => "vxmcp".to_string(),
    };
    let job = CampaignServiceClient::new(channel)
        .start(CampaignStartRequest {
            manifest: path_ref(resolved_workspace),
            profile: args.profile.unwrap_or_default(),
            request_id: args.request_id.unwrap_or_default(),
            caller,
            unlocked: args.unlocked,
            ..Default::default()
        })
        .await?
        .into_inner();
    super::out::json(&job)
}

pub async fn get(
    channel: tonic::transport::Channel,
    args: CampaignIdArgs,
) -> anyhow::Result<CallToolResult> {
    let status = CampaignServiceClient::new(channel)
        .get(CampaignGetRequest {
            campaign_id: args.campaign_id,
        })
        .await?
        .into_inner();
    super::out::json(&status)
}

pub async fn list(channel: tonic::transport::Channel) -> anyhow::Result<CallToolResult> {
    let list = CampaignServiceClient::new(channel)
        .list(CampaignListRequest {})
        .await?
        .into_inner();
    super::out::json(&list)
}

const DEFAULT_WATCH_SECS: u64 = 60;
const MAX_WATCH_SECS: u64 = 300;

fn is_terminal(state: i32) -> bool {
    matches!(
        CampaignState::try_from(state),
        Ok(CampaignState::Succeeded | CampaignState::Failed | CampaignState::Cancelled)
    )
}

/// How a watch ended, and the last status seen.
async fn follow<S>(
    mut stream: S,
    limit: Duration,
) -> Result<(&'static str, Option<CampaignStatus>, usize), tonic::Status>
where
    S: Stream<Item = Result<CampaignStatus, tonic::Status>> + Unpin,
{
    let deadline = tokio::time::Instant::now() + limit;
    let (mut last, mut updates) = (None, 0);
    let end = loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Err(_) => break "timed_out",
            Ok(None) => break "stream_ended",
            Ok(Some(Err(status))) => return Err(status),
            Ok(Some(Ok(s))) => {
                updates += 1;
                let done = is_terminal(s.state);
                last = Some(s);
                if done {
                    break "finished";
                }
            }
        }
    };
    Ok((end, last, updates))
}

pub async fn watch(
    channel: tonic::transport::Channel,
    args: CampaignWatchArgs,
) -> anyhow::Result<CallToolResult> {
    let secs = args
        .timeout_secs
        .unwrap_or(DEFAULT_WATCH_SECS)
        .clamp(1, MAX_WATCH_SECS);
    let stream = CampaignServiceClient::new(channel)
        .watch(CampaignWatchRequest {
            campaign_id: args.campaign_id,
        })
        .await?
        .into_inner();
    let (end, last, updates) = follow(stream, Duration::from_secs(secs)).await?;
    Ok(CallToolResult::structured(serde_json::json!({
        "end": end,
        "updates": updates,
        "timeoutSecs": secs,
        "status": last,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vxedge_api::v1::{CampaignTrial, PlanProblem};

    fn plan(trials: usize, estimated_s: f64) -> CampaignPlan {
        CampaignPlan {
            trials: vec![CampaignTrial::default(); trials],
            points: trials as u32,
            repetitions: 1,
            estimated_s,
            profile: "quick".into(),
            ..Default::default()
        }
    }

    #[test]
    fn budgets_refuse_too_many_trials_or_too_long_a_campaign() {
        let b = Budget {
            max_trials: 10,
            max_wall_s: 600.0,
        };
        assert_eq!(b.exceeded(&plan(10, 600.0)), None);
        assert!(b.exceeded(&plan(11, 60.0)).unwrap().contains("11 trials"));
        assert!(b.exceeded(&plan(2, 601.0)).unwrap().contains("601 s"));
        let refused = refusal(&plan(11, 60.0), &b).unwrap();
        assert_eq!(refused.is_error, Some(true));
        let body = refused.structured_content.unwrap();
        assert_eq!(body["error"]["reason"], "VXMCP_BUDGET_EXCEEDED");
    }

    #[test]
    fn a_plan_problem_is_refused_with_its_reason_before_anyone_is_asked() {
        let mut p = plan(2, 60.0);
        p.problems.push(PlanProblem {
            reason: ErrorReason::ParameterInvalid as i32,
            subject: "exp".into(),
            detail: "`campaign.sweep.load_mbps[0]`: 5000 is outside [0, 1000]".into(),
        });
        let b = Budget::from_env();
        let body = refusal(&p, &b).unwrap().structured_content.unwrap();
        assert_eq!(body["error"]["reason"], "ERROR_REASON_PARAMETER_INVALID");
    }

    #[test]
    fn the_confirmation_names_trials_wall_time_and_profile() {
        let q = start_confirmation("/ws/e", &plan(5, 610.0));
        assert!(q.contains("5 trials (5 points x 1 repetitions)"), "{q}");
        assert!(q.contains("about 11 minutes"), "{q}");
        assert!(q.contains("profile `quick`"), "{q}");
    }

    #[tokio::test]
    async fn a_watch_ends_when_the_campaign_does() {
        let s = |state: CampaignState| {
            Ok(CampaignStatus {
                state: state as i32,
                ..Default::default()
            })
        };
        let stream = tokio_stream::iter(vec![
            s(CampaignState::Running),
            s(CampaignState::Succeeded),
            s(CampaignState::Running),
        ]);
        let (end, last, updates) = follow(stream, Duration::from_secs(5)).await.unwrap();
        assert_eq!((end, updates), ("finished", 2));
        assert_eq!(last.unwrap().state, CampaignState::Succeeded as i32);
        let empty = tokio_stream::iter(Vec::<Result<CampaignStatus, tonic::Status>>::new());
        assert_eq!(
            follow(empty, Duration::from_secs(1)).await.unwrap().0,
            "stream_ended"
        );
    }
}
