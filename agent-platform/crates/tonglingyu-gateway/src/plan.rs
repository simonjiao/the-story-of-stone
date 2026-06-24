use crate::InternalProfiles;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use tonglingyu_runtime::{
    RuntimeWorkflowPlan, RuntimeWorkflowPlanInput, RuntimeWorkflowProfiles, normalize_for_search,
    runtime_workflow_plan,
};

#[cfg(test)]
use tonglingyu_runtime::{
    RUNTIME_WORKFLOW_PLAN_POLICY_VERSION as PLAN_POLICY_VERSION,
    RUNTIME_WORKFLOW_PLAN_SCHEMA_VERSION as PLAN_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SearchPolicy {
    pub(crate) question_type: String,
    pub(crate) required_evidence_types: Vec<String>,
    pub(crate) planned_profiles: Vec<String>,
    pub(crate) blocked_controls: Vec<String>,
}

pub(crate) struct RuntimeStepPlan;

impl RuntimeStepPlan {
    pub(crate) fn from_policy(
        profiles: &InternalProfiles,
        policy: &SearchPolicy,
    ) -> RuntimeWorkflowPlan {
        runtime_workflow_plan(RuntimeWorkflowPlanInput {
            question_type: policy.question_type.clone(),
            required_evidence_types: policy.required_evidence_types.clone(),
            blocked_controls: policy.blocked_controls.clone(),
            profiles: RuntimeWorkflowProfiles {
                main: profiles.main.clone(),
                text: profiles.text.clone(),
                commentary: profiles.commentary.clone(),
                reviewer: profiles.reviewer.clone(),
            },
        })
    }
}

pub(crate) fn search_policy(question: &str) -> SearchPolicy {
    let normalized = normalize_text(question);
    let blocked_controls = blocked_prompt_controls(question);
    let mut required = BTreeSet::new();
    let asks_commentary = question.contains("脂批")
        || question.contains("脂評")
        || question.contains("甲戌")
        || normalized.contains("脂批");
    let asks_named_edition = question.contains("程甲") || question.contains("程乙");
    let asks_version_boundary = question.contains("版本")
        || question.contains("前八十")
        || question.contains("后四十")
        || question.contains("後四十");
    let asks_version = asks_named_edition || asks_version_boundary;
    let lowered = question.to_lowercase();
    let clearly_out_of_scope = normalized.contains("量子计算机")
        || normalized.contains("现代人工智能")
        || normalized.contains("清朝以后的")
        || lowered.contains("system prompt");
    let control_only = !blocked_controls.is_empty()
        && !normalized.contains("通灵")
        && !normalized.contains("宝玉")
        && !normalized.contains("红楼")
        && !normalized.contains("黛玉");
    if asks_commentary {
        required.insert("commentary".to_string());
    }
    if asks_version_boundary {
        required.insert("version_note".to_string());
    }
    if !asks_commentary && !asks_version_boundary && !clearly_out_of_scope && !control_only {
        required.insert("base_text".to_string());
    }
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
    normalize_for_search(input)
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
        assert_eq!(plan.policy_version, PLAN_POLICY_VERSION);
        assert!(
            plan.steps.iter().all(|step| step.profile_contract_version
                == tonglingyu_runtime::PROFILE_CONTRACT_VERSION)
        );
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

    #[test]
    fn version_boundary_policy_requires_version_note_without_base_text() {
        let policy = search_policy("前八十回边界在哪里？");

        assert_eq!(policy.question_type, "version");
        assert!(
            policy
                .required_evidence_types
                .contains(&"version_note".to_string())
        );
        assert!(
            !policy
                .required_evidence_types
                .contains(&"base_text".to_string())
        );
    }

    #[test]
    fn named_edition_policy_keeps_base_text_without_forcing_version_note() {
        let policy = search_policy("程乙本第一回顽石文字在哪里？");

        assert_eq!(policy.question_type, "version");
        assert!(
            policy
                .required_evidence_types
                .contains(&"base_text".to_string())
        );
        assert!(
            !policy
                .required_evidence_types
                .contains(&"version_note".to_string())
        );
    }
}
