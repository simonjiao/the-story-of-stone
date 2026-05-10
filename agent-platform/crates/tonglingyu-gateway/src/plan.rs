use crate::InternalProfiles;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) const PLAN_SCHEMA_VERSION: &str = "tonglingyu-runtime-step-plan-v1";
pub(crate) const PLAN_POLICY_VERSION: &str = "tonglingyu-plan-policy-v1";

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SearchPolicy {
    pub(crate) question_type: String,
    pub(crate) required_evidence_types: Vec<String>,
    pub(crate) planned_profiles: Vec<String>,
    pub(crate) blocked_controls: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuntimeStepPlan {
    pub(crate) schema_version: String,
    pub(crate) policy_version: String,
    pub(crate) question_type: String,
    pub(crate) required_evidence_types: Vec<String>,
    pub(crate) blocked_controls: Vec<String>,
    pub(crate) steps: Vec<RuntimePlanStep>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuntimePlanStep {
    pub(crate) step_id: String,
    pub(crate) profile: String,
    pub(crate) operation: String,
    pub(crate) required: bool,
    pub(crate) allowed_tools: Vec<String>,
}

impl RuntimeStepPlan {
    pub(crate) fn from_policy(profiles: &InternalProfiles, policy: &SearchPolicy) -> Self {
        let mut steps = vec![RuntimePlanStep {
            step_id: "step-01-text-search".to_string(),
            profile: profiles.text.clone(),
            operation: "text_evidence_search".to_string(),
            required: true,
            allowed_tools: vec!["tonglingyu.text.search".to_string()],
        }];
        if policy
            .required_evidence_types
            .iter()
            .any(|item| item == "commentary")
        {
            steps.push(RuntimePlanStep {
                step_id: "step-02-commentary-search".to_string(),
                profile: profiles.commentary.clone(),
                operation: "commentary_evidence_search".to_string(),
                required: true,
                allowed_tools: vec!["tonglingyu.commentary.search".to_string()],
            });
        }
        steps.push(RuntimePlanStep {
            step_id: step_id(steps.len() + 1, "package-create"),
            profile: profiles.main.clone(),
            operation: "evidence_package_create".to_string(),
            required: true,
            allowed_tools: vec!["tonglingyu.evidence.package.create".to_string()],
        });
        steps.push(RuntimePlanStep {
            step_id: step_id(steps.len() + 1, "draft-answer"),
            profile: profiles.main.clone(),
            operation: "draft_answer".to_string(),
            required: true,
            allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
        });
        steps.push(RuntimePlanStep {
            step_id: step_id(steps.len() + 1, "review-answer"),
            profile: profiles.reviewer.clone(),
            operation: "review_answer".to_string(),
            required: true,
            allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
        });
        Self {
            schema_version: PLAN_SCHEMA_VERSION.to_string(),
            policy_version: PLAN_POLICY_VERSION.to_string(),
            question_type: policy.question_type.clone(),
            required_evidence_types: policy.required_evidence_types.clone(),
            blocked_controls: policy.blocked_controls.clone(),
            steps,
        }
    }
}

pub(crate) fn search_policy(question: &str) -> SearchPolicy {
    let normalized = normalize_text(question);
    let mut required = BTreeSet::new();
    required.insert("base_text".to_string());
    let asks_commentary = question.contains("脂批")
        || question.contains("脂評")
        || question.contains("甲戌")
        || normalized.contains("脂批");
    let asks_version = question.contains("程甲")
        || question.contains("程乙")
        || question.contains("版本")
        || question.contains("前八十")
        || question.contains("后四十")
        || question.contains("後四十");
    if asks_commentary {
        required.insert("commentary".to_string());
    }
    if asks_version {
        required.insert("version_note".to_string());
    }
    let blocked_controls = blocked_prompt_controls(question);
    let question_type = if !blocked_controls.is_empty() {
        "control_injection"
    } else if asks_commentary {
        "commentary"
    } else if asks_version {
        "version"
    } else if question.contains("判词") || question.contains("判詞") || question.contains("诗")
    {
        "poem_or_judgement"
    } else {
        "base_text"
    };
    let mut profiles = vec![
        "honglou-text".to_string(),
        "honglou-main".to_string(),
        "honglou-reviewer".to_string(),
    ];
    if asks_commentary {
        profiles.insert(1, "honglou-commentary".to_string());
    }
    SearchPolicy {
        question_type: question_type.to_string(),
        required_evidence_types: required.into_iter().collect(),
        planned_profiles: profiles,
        blocked_controls,
    }
}

pub(crate) fn public_search_policy(policy: &SearchPolicy) -> Value {
    json!({
        "question_type": &policy.question_type,
        "required_evidence_types": &policy.required_evidence_types,
        "blocked_controls": &policy.blocked_controls,
    })
}

pub(crate) fn planned_profiles_for_policy(
    profiles: &InternalProfiles,
    policy: &SearchPolicy,
) -> Vec<String> {
    let mut planned = vec![profiles.text.clone()];
    if policy
        .required_evidence_types
        .iter()
        .any(|item| item == "commentary")
    {
        planned.push(profiles.commentary.clone());
    }
    planned.push(profiles.main.clone());
    planned.push(profiles.reviewer.clone());
    planned
}

fn blocked_prompt_controls(question: &str) -> Vec<String> {
    let controls = [
        ("跳过reviewer", "attempted_reviewer_bypass"),
        ("跳过 reviewer", "attempted_reviewer_bypass"),
        ("关闭审校", "attempted_reviewer_bypass"),
        ("不要审校", "attempted_reviewer_bypass"),
        ("skip reviewer", "attempted_reviewer_bypass"),
        ("disable_reviewer", "attempted_reviewer_bypass"),
        ("disable reviewer", "attempted_reviewer_bypass"),
        ("只凭模型记忆", "attempted_memory_only_answer"),
        ("不要证据", "attempted_evidence_bypass"),
        ("忽略证据", "attempted_evidence_bypass"),
        ("绕过证据", "attempted_evidence_bypass"),
        ("honglou-", "attempted_internal_agent_control"),
        ("内部 agent", "attempted_internal_agent_control"),
        ("内部Agent", "attempted_internal_agent_control"),
        ("内部配置", "attempted_internal_config_leak"),
        ("系统提示词", "attempted_internal_prompt_leak"),
        ("system prompt", "attempted_internal_prompt_leak"),
    ];
    let lowered = question.to_lowercase();
    controls
        .iter()
        .filter_map(|(needle, code)| {
            if lowered.contains(&needle.to_lowercase()) {
                Some((*code).to_string())
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_text(input: &str) -> String {
    let replacements = [
        ("紅", "红"),
        ("樓", "楼"),
        ("夢", "梦"),
        ("寶", "宝"),
        ("寳", "宝"),
        ("靈", "灵"),
        ("釵", "钗"),
        ("壽", "寿"),
        ("恆", "恒"),
        ("後", "后"),
        ("評", "评"),
    ];
    let mut output = input.to_lowercase();
    for (from, to) in replacements {
        output = output.replace(from, to);
    }
    output
}

fn step_id(index: usize, name: &str) -> String {
    format!("step-{index:02}-{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> InternalProfiles {
        InternalProfiles {
            main: "honglou-main".to_string(),
            text: "honglou-text".to_string(),
            commentary: "honglou-commentary".to_string(),
            reviewer: "honglou-reviewer".to_string(),
        }
    }

    #[test]
    fn public_policy_does_not_expose_internal_profiles() {
        let policy = search_policy("脂批如何评价通灵玉？");
        let public = public_search_policy(&policy);
        assert_eq!(public["question_type"], "commentary");
        assert!(public.get("planned_profiles").is_none());
    }

    #[test]
    fn runtime_step_plan_contains_required_reviewer() {
        let policy = search_policy("脂批如何评价通灵玉？");
        let plan = RuntimeStepPlan::from_policy(&profiles(), &policy);
        assert_eq!(plan.schema_version, PLAN_SCHEMA_VERSION);
        assert!(
            plan.steps
                .iter()
                .any(|step| step.profile == "honglou-reviewer" && step.required)
        );
        assert!(plan.steps.iter().any(|step| {
            step.allowed_tools
                .contains(&"tonglingyu.commentary.search".to_string())
        }));
    }
}
