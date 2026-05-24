use agent_core::{
    AgentCoreError, CoreResult, ErrorCode, ProfileContract as AgentProfileContract, RuntimeClient,
    RuntimeOutput, RuntimeProfileInput, RuntimeProfileMessage, RuntimeStep as AgentRuntimeStep,
    RuntimeStepPlan as AgentRuntimeStepPlan, RuntimeStepPlanInput as AgentRuntimeStepPlanInput,
    RuntimeStepPlanOwner, RuntimeToolCall, RuntimeToolExecutor, RuntimeToolPolicy,
    RuntimeToolResult, RuntimeToolSpec,
};
use agent_runtime::{
    HermesRuntimeClient, HermesRuntimeConfig, MinimalRuntimeClient,
    OpenAiCompatibleNetworkRuntimeClient, OpenAiCompatibleNetworkRuntimeConfig,
    RuntimeProfileRegistry,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use ferrous_opencc::{OpenCC, config::BuiltinConfig};
use futures_util::future::try_join_all;
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Instant, SystemTime},
};
use time::OffsetDateTime;

mod answer_composer;
mod evidence_slot_rules;
mod governance_rules;
mod ontology_aliases;
mod retrieval_rules;
mod upstream_bundle;

use answer_composer::{
    EvidenceSlotMatch, compose_slot_count_answer, direct_count_for_basis, public_quote_text,
    representative_matches, source_layer_for_card,
};
use evidence_slot_rules::{
    EvidenceSlotCountBasis, active_count_basis_for_question, evidence_slot_count_policy_value,
    evidence_slot_rule_values_for_ids, evidence_slot_rules_for_ids, explicit_total_count_for_basis,
    question_asks_for_count,
};
use governance_rules::{
    blocked_prompt_control_issues, claim_evidence_types_for_claim, claim_rules,
    draft_has_public_forbidden_term, draft_has_unsupported_term_without_evidence,
    draft_stops_for_user_opt_in, empty_evidence_review_issue,
    later_forty_boundary_missing_from_claims, later_forty_boundary_review_issue,
    preferred_answer_evidence_types, triggered_review_rule_issues,
};
use ontology_aliases::seed_aliases;
use upstream_bundle::{
    UPSTREAM_BUNDLE_SCHEMA_VERSION, evidence_card_is_later_forty, evidence_card_source_layer,
    extract_upstream_bundle_draft, filter_cards_for_source_scope, source_scope_policy_for_question,
    source_title_in_later_forty, text_mentions_later_forty_boundary,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceCard {
    pub evidence_id: String,
    pub evidence_type: String,
    pub source_id: String,
    pub source_title: String,
    pub source_url: String,
    pub revision_id: Option<i64>,
    pub block_id: String,
    pub text: String,
    pub support_scope: String,
    pub unsupported_scope: String,
    pub evidence_level: String,
    pub confidence: String,
    pub verification_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimEvidenceMap {
    pub claim_index: usize,
    pub claim: String,
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub knowledge_item_refs: Vec<ClaimKnowledgeItemRef>,
    pub forbidden_conclusions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimKnowledgeItemRef {
    pub item_id: String,
    pub state: KnowledgeState,
    pub evidence_ref: String,
    pub policy_version: String,
    pub policy_decision: String,
    pub calibration_report_ref: Option<String>,
    pub display_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRecord {
    pub status: String,
    pub severity: String,
    pub issues: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePackage {
    pub package_id: String,
    pub trace_id: String,
    pub question: String,
    pub cards: Vec<EvidenceCard>,
    pub claims: Vec<String>,
    pub claim_evidence_map: Vec<ClaimEvidenceMap>,
    #[serde(default = "default_knowledge_state_summary")]
    pub knowledge_state_summary: KnowledgeStateSummary,
    pub review: ReviewRecord,
}

pub const TOOL_CATALOG_VERSION: &str = "tonglingyu-readonly-tools-v1";
pub const PROFILE_CONTRACT_VERSION: &str = "tonglingyu-runtime-profiles-v1";
pub const KNOWLEDGE_BASE_SCHEMA_VERSION: &str = "tonglingyu-v1-sqlite-fts";
pub const KB_VERSION_DIFF_REPORT_SCHEMA_VERSION: &str = "tonglingyu-kb-version-diff-v1";
pub const RUNTIME_WORKFLOW_PLAN_SCHEMA_VERSION: &str = "tonglingyu-runtime-step-plan-v1";
pub const RUNTIME_WORKFLOW_PLAN_POLICY_VERSION: &str = "tonglingyu-plan-policy-v1";
pub const RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION: &str = "tonglingyu-rqa-report-v1";
pub const RETRIEVAL_QUALITY_REPORT_MAX_TERMS: usize = 24;
pub const RETRIEVAL_QUALITY_REPORT_MAX_SOURCE_REFS: usize = 32;
pub const QUERY_EXPANSIONS_PATH_ENV: &str = "TONGLINGYU_QUERY_EXPANSIONS_PATH";
pub const EVIDENCE_SLOT_RULES_PATH_ENV: &str = "TONGLINGYU_EVIDENCE_SLOT_RULES_PATH";
pub const GOVERNANCE_RULES_PATH_ENV: &str = "TONGLINGYU_GOVERNANCE_RULES_PATH";
pub const RETRIEVAL_RULES_PATH_ENV: &str = "TONGLINGYU_RETRIEVAL_RULES_PATH";
pub const ONTOLOGY_ALIASES_PATH_ENV: &str = "TONGLINGYU_ONTOLOGY_ALIASES_PATH";

const QUERY_EXPANSIONS_SCHEMA_VERSION: &str = "tonglingyu.query_expansions.v1";
const DEFAULT_QUERY_EXPANSIONS_JSON: &str = include_str!("../resources/query_expansions.json");
const AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES: usize = 8192;
const AGENT_RUNTIME_PROFILE_STEP_MAX_ATTEMPTS: usize = 2;
const UPSTREAM_EVIDENCE_BRIEF_CARD_LIMIT: usize = 5;
const UPSTREAM_EVIDENCE_BRIEF_TEXT_CHARS: usize = 120;
const UPSTREAM_EVIDENCE_BRIEF_SOURCE_TITLE_CHARS: usize = 80;
const UPSTREAM_EVIDENCE_BRIEF_SCOPE_CHARS: usize = 72;
const UPSTREAM_EVIDENCE_BRIEF_LIMITS_CHARS: usize = 96;
const UPSTREAM_EVIDENCE_BRIEF_MATCHED_TERM_LIMIT: usize = 4;
const UPSTREAM_EVIDENCE_BRIEF_MATCHED_TERM_CHARS: usize = 16;

pub trait TextNormalizer {
    fn normalize_for_search(&self, input: &str) -> String;
    fn normalize_query(&self, input: &str) -> String;
    fn normalize_alias(&self, input: &str) -> String;
    fn normalize_title(&self, input: &str) -> String;
}

#[derive(Debug, Clone, Copy)]
pub struct OpenCcTextNormalizer;

impl TextNormalizer for OpenCcTextNormalizer {
    fn normalize_for_search(&self, input: &str) -> String {
        let converted = t2s_opencc().convert(input);
        apply_project_normalization_overrides(&converted)
    }

    fn normalize_query(&self, input: &str) -> String {
        self.normalize_for_search(input)
    }

    fn normalize_alias(&self, input: &str) -> String {
        self.normalize_for_search(input)
    }

    fn normalize_title(&self, input: &str) -> String {
        self.normalize_for_search(input)
    }
}

static TEXT_NORMALIZER: OpenCcTextNormalizer = OpenCcTextNormalizer;
static T2S_OPENCC: OnceLock<OpenCC> = OnceLock::new();
static QUERY_EXPANSION_CATALOG_CACHE: OnceLock<Mutex<QueryExpansionCatalogCache>> = OnceLock::new();

fn text_normalizer() -> &'static dyn TextNormalizer {
    &TEXT_NORMALIZER
}

fn t2s_opencc() -> &'static OpenCC {
    T2S_OPENCC.get_or_init(|| {
        OpenCC::from_config(BuiltinConfig::T2s)
            .expect("embedded OpenCC t2s config must load for search normalization")
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalQuerySummary {
    pub question_sha256: String,
    pub question_char_count: usize,
    pub raw_question_included: bool,
    pub redacted_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalEvidenceTypeCoverage {
    pub required: Vec<String>,
    pub selected: Vec<String>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalExactMatchCoverage {
    pub term: String,
    pub matched: bool,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalSourceUsageRef {
    pub source_id: String,
    pub source_category: Option<String>,
    pub title: Option<String>,
    pub edition: Option<String>,
    pub source_url: Option<String>,
    pub fetched_at: Option<String>,
    pub source_hash: Option<String>,
    pub license: Option<String>,
    pub license_url: Option<String>,
    pub license_source_url: Option<String>,
    pub attribution: Option<String>,
    pub usage_boundary: String,
    pub metadata_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalSourceCoverageBoundary {
    pub source_ids: Vec<String>,
    pub source_categories: Vec<String>,
    pub edition_boundaries: Vec<String>,
    pub kb_schema_version: String,
    pub source_snapshot_status: String,
    pub facsimile_review_status: String,
    pub authoritative_edition_review_status: String,
    pub scholarly_collation_status: String,
    pub expert_collation_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalQualityReport {
    pub object: String,
    pub schema_version: String,
    pub tool_name: String,
    pub quality_status: String,
    pub production_ready: bool,
    pub truncated: bool,
    pub query_summary: RetrievalQuerySummary,
    pub expanded_terms: Vec<String>,
    pub protected_terms: Vec<String>,
    pub expanded_aliases: Vec<String>,
    pub normalized_match_channels: BTreeMap<String, usize>,
    pub candidate_count: usize,
    pub selected_count: usize,
    pub channel_distribution: BTreeMap<String, usize>,
    pub evidence_type_coverage: RetrievalEvidenceTypeCoverage,
    pub exact_match_coverage: Vec<RetrievalExactMatchCoverage>,
    pub expected_evidence_hit: Option<bool>,
    pub expected_evidence_status: String,
    pub source_coverage_boundary: RetrievalSourceCoverageBoundary,
    pub source_usage_refs: Vec<RetrievalSourceUsageRef>,
    pub issues: Vec<String>,
    pub recommended_follow_up: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub version: String,
    pub allowed_profiles: Vec<String>,
    pub effect_scope: String,
    pub input_contract: Value,
    pub output_contract: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileDescriptor {
    pub profile: String,
    pub version: String,
    pub role: String,
    pub allowed_tools: Vec<String>,
    pub input_contract: Value,
    pub output_contract: Value,
    pub safety_contract: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeBaseBuildReport {
    pub version_id: String,
    pub source_root: String,
    pub source_count: i64,
    pub block_count: i64,
    pub schema_version: String,
    pub built_at: String,
    pub source_snapshot_digest: String,
    pub kb_build_hash: String,
    pub diff_report: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStoreStats {
    pub sources: i64,
    pub blocks: i64,
    pub evidence_packages: i64,
    pub evidence_cards: i64,
    pub retrieval_failures: i64,
    pub governance_tasks: i64,
    pub knowledge_patch_proposals: i64,
    pub knowledge_items: i64,
    pub knowledge_item_state_history: i64,
    pub knowledge_calibration_reports: i64,
    pub knowledge_calibration_jobs: i64,
    pub knowledge_calibration_job_history: i64,
    pub audit_events: i64,
    pub review_status: BTreeMap<String, i64>,
    pub evidence_types: BTreeMap<String, i64>,
    pub retrieval_failure_status: BTreeMap<String, i64>,
    pub retrieval_failure_type: BTreeMap<String, i64>,
    pub governance_task_status: BTreeMap<String, i64>,
    pub governance_task_type: BTreeMap<String, i64>,
    pub governance_task_priority: BTreeMap<String, i64>,
    pub knowledge_item_state: BTreeMap<String, i64>,
    pub knowledge_item_kind: BTreeMap<String, i64>,
    pub knowledge_calibration_report_method: BTreeMap<String, i64>,
    pub knowledge_calibration_report_decision: BTreeMap<String, i64>,
    pub knowledge_calibration_job_status: BTreeMap<String, i64>,
    pub audit_event_types: BTreeMap<String, i64>,
}

pub const RETRIEVAL_FAILURE_SCHEMA_VERSION: &str = "tonglingyu-retrieval-failures-v1";
pub const RETRIEVAL_FAILURE_CLUSTER_SCHEMA_VERSION: &str =
    "tonglingyu-retrieval-failure-clusters-v1";
pub const RETRIEVAL_FAILURE_DEDUPE_MIGRATION: &str = "tonglingyu-retrieval-failure-dedupe-v1";
pub const RETRIEVAL_FAILURE_PRIVACY_MIGRATION: &str = "tonglingyu-retrieval-failure-privacy-v1";
pub const RQA_LIFECYCLE_POLICY_VERSION: &str = "tonglingyu-rqa-lifecycle-v1";
pub const RETRIEVAL_FAILURE_DEFAULT_PAGE_SIZE: usize = 50;
pub const RETRIEVAL_FAILURE_MAX_PAGE_SIZE: usize = 100;
pub const RETRIEVAL_FAILURE_CLUSTER_DEFAULT_LIMIT: usize = 200;
pub const RETRIEVAL_FAILURE_CLUSTER_MAX_LIMIT: usize = 500;
pub const KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION: &str =
    "tonglingyu-knowledge-governance-tasks-v2";
pub const KNOWLEDGE_GOVERNANCE_TASK_BACKFILL_MIGRATION: &str =
    "tonglingyu-knowledge-governance-tasks-backfill-v1";
pub const KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION: &str = "tonglingyu-knowledge-patch-proposals-v1";
pub const KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION: &str = "tonglingyu-knowledge-item-states-v1";
pub const KNOWLEDGE_ITEM_CALIBRATION_LINK_MIGRATION: &str =
    "tonglingyu-knowledge-item-calibration-links-v1";
pub const KNOWLEDGE_CALIBRATION_REPORT_SCHEMA_VERSION: &str =
    "tonglingyu-knowledge-calibration-report-v1";
pub const KNOWLEDGE_CALIBRATION_JOB_SCHEMA_VERSION: &str =
    "tonglingyu-knowledge-calibration-jobs-v1";
pub const KNOWLEDGE_CALIBRATION_PROFILE_ID: &str = "honglou-knowledge-calibrator";
pub const KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION: &str =
    "tonglingyu-knowledge-calibration-profile-v1";
pub const RUNTIME_CONTEXT_PACK_SCHEMA_VERSION: &str = "tonglingyu-scoped-context-v1";
pub const RUNTIME_CONTEXT_PROJECTION_SCHEMA_VERSION: &str = "tonglingyu-context-projection-v1";
pub const RUNTIME_CONTEXT_CONSUMER_TYPE: &str = "runtime_profile";
pub const TONGLINGYU_RUNTIME_ADAPTER: &str = "tonglingyu-runtime-adapter-v1";
pub const KNOWLEDGE_RUNTIME_POLICY_VERSION: &str = "tonglingyu-knowledge-runtime-policy-v1";
pub const KNOWLEDGE_RUNTIME_POLICY_SCHEMA_VERSION: &str =
    "tonglingyu-knowledge-runtime-policy-schema-v1";
pub const KNOWLEDGE_ITEM_HUMAN_REVIEW_SCHEMA_VERSION: &str =
    "tonglingyu-knowledge-item-human-review-v1";
pub const GOVERNANCE_TASK_DEFAULT_PAGE_SIZE: usize = 50;
pub const GOVERNANCE_TASK_MAX_PAGE_SIZE: usize = 100;
pub const KNOWLEDGE_ITEM_DEFAULT_PAGE_SIZE: usize = 50;
pub const KNOWLEDGE_ITEM_MAX_PAGE_SIZE: usize = 100;
pub const KNOWLEDGE_CALIBRATION_DEFAULT_LEASE_SECONDS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeState {
    SourceSnapshot,
    Candidate,
    SystemCalibrated,
    RuntimeUsable,
    HumanMarked,
    Rejected,
    Deprecated,
}

impl KnowledgeState {
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeState::SourceSnapshot => "source_snapshot",
            KnowledgeState::Candidate => "candidate",
            KnowledgeState::SystemCalibrated => "system_calibrated",
            KnowledgeState::RuntimeUsable => "runtime_usable",
            KnowledgeState::HumanMarked => "human_marked",
            KnowledgeState::Rejected => "rejected",
            KnowledgeState::Deprecated => "deprecated",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "source_snapshot" => Ok(Self::SourceSnapshot),
            "candidate" => Ok(Self::Candidate),
            "system_calibrated" => Ok(Self::SystemCalibrated),
            "runtime_usable" => Ok(Self::RuntimeUsable),
            "human_marked" => Ok(Self::HumanMarked),
            "rejected" => Ok(Self::Rejected),
            "deprecated" => Ok(Self::Deprecated),
            _ => Err(anyhow!("invalid knowledge item state {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeItemKind {
    Alias,
    Term,
    CommentaryLink,
    VersionNote,
    Person,
    Relationship,
    Event,
    Poem,
    EvaluationCase,
}

impl KnowledgeItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeItemKind::Alias => "alias",
            KnowledgeItemKind::Term => "term",
            KnowledgeItemKind::CommentaryLink => "commentary_link",
            KnowledgeItemKind::VersionNote => "version_note",
            KnowledgeItemKind::Person => "person",
            KnowledgeItemKind::Relationship => "relationship",
            KnowledgeItemKind::Event => "event",
            KnowledgeItemKind::Poem => "poem",
            KnowledgeItemKind::EvaluationCase => "evaluation_case",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "alias" => Ok(Self::Alias),
            "term" => Ok(Self::Term),
            "commentary_link" => Ok(Self::CommentaryLink),
            "version_note" => Ok(Self::VersionNote),
            "person" => Ok(Self::Person),
            "relationship" => Ok(Self::Relationship),
            "event" => Ok(Self::Event),
            "poem" => Ok(Self::Poem),
            "evaluation_case" => Ok(Self::EvaluationCase),
            _ => Err(anyhow!("invalid knowledge item kind {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeItemRecord {
    pub item_id: String,
    pub kind: KnowledgeItemKind,
    pub state: KnowledgeState,
    pub source_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub payload: Value,
    pub payload_sha256: String,
    pub schema_version: String,
    pub source_boundary: Option<Value>,
    pub calibration_report_ref: Option<String>,
    pub confidence: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
    pub state_version: i64,
}

#[derive(Debug, Clone)]
pub struct KnowledgeItemCreateInput {
    pub kind: KnowledgeItemKind,
    pub initial_state: KnowledgeState,
    pub source_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub payload: Value,
    pub schema_version: Option<String>,
    pub trace_id: String,
    pub actor: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct KnowledgeItemListInput {
    pub kind: Option<KnowledgeItemKind>,
    pub state: Option<KnowledgeState>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeItemListResult {
    pub object: String,
    pub schema_version: String,
    pub limit: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<KnowledgeItemRecord>,
}

#[derive(Debug, Clone)]
pub struct KnowledgeItemStateUpdateInput {
    pub new_state: KnowledgeState,
    pub trace_id: String,
    pub actor: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub expected_state_version: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeItemHumanReviewDecision {
    Accept,
    Reject,
    Deprecate,
}

impl KnowledgeItemHumanReviewDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::Reject => "reject",
            Self::Deprecate => "deprecate",
        }
    }

    pub fn target_state(self) -> KnowledgeState {
        match self {
            Self::Accept => KnowledgeState::HumanMarked,
            Self::Reject => KnowledgeState::Rejected,
            Self::Deprecate => KnowledgeState::Deprecated,
        }
    }

    pub fn task_status(self) -> &'static str {
        match self {
            Self::Accept => "accepted",
            Self::Reject | Self::Deprecate => "rejected",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "accept" | "accepted" | "human_marked" => Ok(Self::Accept),
            "reject" | "rejected" => Ok(Self::Reject),
            "deprecate" | "deprecated" => Ok(Self::Deprecate),
            _ => Err(anyhow!(
                "invalid knowledge item human review decision {value}"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KnowledgeItemHumanReviewInput {
    pub task_id: String,
    pub decision: KnowledgeItemHumanReviewDecision,
    pub trace_id: String,
    pub actor: String,
    pub reviewer: String,
    pub review_note: String,
    pub evidence_ref: String,
    pub expected_state_version: i64,
    pub expected_task_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeItemHumanReviewResult {
    pub object: String,
    pub schema_version: String,
    pub decision: KnowledgeItemHumanReviewDecision,
    pub item: KnowledgeItemRecord,
    pub task: KnowledgeGovernanceTaskRecord,
    pub kb_rebuild_required: bool,
    pub eval_diff_required: bool,
    pub release_gate_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeCalibrationMethod {
    Rule,
    Eval,
    Rqa,
    LlmEvidenceJudge,
}

impl KnowledgeCalibrationMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeCalibrationMethod::Rule => "rule",
            KnowledgeCalibrationMethod::Eval => "eval",
            KnowledgeCalibrationMethod::Rqa => "rqa",
            KnowledgeCalibrationMethod::LlmEvidenceJudge => "llm_evidence_judge",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "rule" => Ok(Self::Rule),
            "eval" => Ok(Self::Eval),
            "rqa" => Ok(Self::Rqa),
            "llm_evidence_judge" => Ok(Self::LlmEvidenceJudge),
            _ => Err(anyhow!("invalid knowledge calibration method {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeCalibrationDecision {
    SystemCalibrated,
    Rejected,
    KeepCandidate,
}

impl KnowledgeCalibrationDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeCalibrationDecision::SystemCalibrated => "system_calibrated",
            KnowledgeCalibrationDecision::Rejected => "rejected",
            KnowledgeCalibrationDecision::KeepCandidate => "keep_candidate",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "system_calibrated" => Ok(Self::SystemCalibrated),
            "rejected" => Ok(Self::Rejected),
            "keep_candidate" => Ok(Self::KeepCandidate),
            _ => Err(anyhow!("invalid knowledge calibration decision {value}")),
        }
    }

    fn target_state(self) -> Option<KnowledgeState> {
        match self {
            KnowledgeCalibrationDecision::SystemCalibrated => {
                Some(KnowledgeState::SystemCalibrated)
            }
            KnowledgeCalibrationDecision::Rejected => Some(KnowledgeState::Rejected),
            KnowledgeCalibrationDecision::KeepCandidate => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeCalibrationInputKind {
    SourceSnapshot,
    GovernanceTask,
    EvalMiss,
    RetrievalFailure,
}

impl KnowledgeCalibrationInputKind {
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeCalibrationInputKind::SourceSnapshot => "source_snapshot",
            KnowledgeCalibrationInputKind::GovernanceTask => "governance_task",
            KnowledgeCalibrationInputKind::EvalMiss => "eval_miss",
            KnowledgeCalibrationInputKind::RetrievalFailure => "retrieval_failure",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "source_snapshot" => Ok(Self::SourceSnapshot),
            "governance_task" => Ok(Self::GovernanceTask),
            "eval_miss" => Ok(Self::EvalMiss),
            "retrieval_failure" => Ok(Self::RetrievalFailure),
            _ => Err(anyhow!("invalid knowledge calibration input kind {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeCalibrationJobStatus {
    Queued,
    Running,
    Succeeded,
    RetryWaiting,
    Failed,
    Cancelled,
}

impl KnowledgeCalibrationJobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeCalibrationJobStatus::Queued => "queued",
            KnowledgeCalibrationJobStatus::Running => "running",
            KnowledgeCalibrationJobStatus::Succeeded => "succeeded",
            KnowledgeCalibrationJobStatus::RetryWaiting => "retry_waiting",
            KnowledgeCalibrationJobStatus::Failed => "failed",
            KnowledgeCalibrationJobStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "retry_waiting" => Ok(Self::RetryWaiting),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(anyhow!("invalid knowledge calibration job status {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationLlmConfig {
    pub profile_id: String,
    pub model_id: String,
    pub upstream_id: String,
    pub prompt_digest: String,
    pub tool_policy_digest: String,
    pub decoding: Value,
    pub timeout_secs: u64,
    pub retry_limit: u32,
    pub model_capability: String,
    pub reasoning_effort: String,
    pub profile_contract_version: String,
    pub config_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationRuleContext {
    pub source_id: String,
    pub block_id: String,
    pub required_evidence_type: String,
    pub exact_terms: Vec<String>,
    pub version_boundary: String,
    pub usage_boundary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationEvalContext {
    pub expected_evidence_hit: bool,
    pub forbidden_conclusion_hit: bool,
    pub reviewer_status: String,
    pub source_boundary_confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationRqaContext {
    pub retrieval_quality_issues: Vec<String>,
    pub blocking_quality_issues: Vec<String>,
    pub failure_cluster_refs: Vec<String>,
    pub governance_task_refs: Vec<String>,
    pub proposed_fix_refs: Vec<String>,
    pub rqa_report_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationLlmJudgeOutput {
    pub decision: KnowledgeCalibrationDecision,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
    pub source_boundary: Value,
    pub quality_issues: Vec<String>,
    pub forbidden_conclusion_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationRunInput {
    pub item_id: String,
    pub input_kind: KnowledgeCalibrationInputKind,
    pub input_ref: String,
    pub method: KnowledgeCalibrationMethod,
    pub trace_id: String,
    pub actor: String,
    pub llm_config: Option<KnowledgeCalibrationLlmConfig>,
    pub llm_judgement: Option<KnowledgeCalibrationLlmJudgeOutput>,
    pub rule_context: Option<KnowledgeCalibrationRuleContext>,
    pub eval_context: Option<KnowledgeCalibrationEvalContext>,
    pub rqa_context: Option<KnowledgeCalibrationRqaContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationReportRecord {
    pub report_id: String,
    pub report_ref: String,
    pub item_id: String,
    pub kind: KnowledgeItemKind,
    pub method: KnowledgeCalibrationMethod,
    pub decision: KnowledgeCalibrationDecision,
    pub confidence: f64,
    pub quality_issues: Vec<String>,
    pub source_refs: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub source_boundary: Value,
    pub coverage_matrix: Value,
    pub config_summary: Option<Value>,
    pub report_hash: String,
    pub schema_version: String,
    pub created_at: String,
    pub report: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationJobCreateInput {
    pub input_kind: KnowledgeCalibrationInputKind,
    pub input_ref: String,
    pub item_id: String,
    pub method: KnowledgeCalibrationMethod,
    pub trace_id: String,
    pub idempotency_key: String,
    pub config_digest: Option<String>,
    pub retry_limit: u32,
    pub concurrency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCalibrationJobRecord {
    pub job_id: String,
    pub status: KnowledgeCalibrationJobStatus,
    pub input_kind: KnowledgeCalibrationInputKind,
    pub input_ref: String,
    pub item_id: String,
    pub input_digest: String,
    pub idempotency_key: String,
    pub trace_id: String,
    pub method: KnowledgeCalibrationMethod,
    pub config_digest: Option<String>,
    pub retry_limit: u32,
    pub attempt_count: u32,
    pub concurrency_key: String,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<String>,
    pub heartbeat_at: Option<String>,
    pub report_id: Option<String>,
    pub last_error_sha256: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStateSummary {
    pub object: String,
    pub policy_version: String,
    pub selected_count: usize,
    pub runtime_usable_count: usize,
    pub human_marked_count: usize,
    pub system_calibrated_rejected_count: usize,
    pub rejected_or_deprecated_count: usize,
    pub candidate_or_source_snapshot_count: usize,
    pub runtime_policy_rejected_count: usize,
    pub safe_public_label: Option<String>,
    pub internal_governance_fields_included: bool,
}

impl Default for KnowledgeStateSummary {
    fn default() -> Self {
        Self {
            object: "tonglingyu.knowledge_state_summary".to_string(),
            policy_version: KNOWLEDGE_RUNTIME_POLICY_VERSION.to_string(),
            selected_count: 0,
            runtime_usable_count: 0,
            human_marked_count: 0,
            system_calibrated_rejected_count: 0,
            rejected_or_deprecated_count: 0,
            candidate_or_source_snapshot_count: 0,
            runtime_policy_rejected_count: 0,
            safe_public_label: None,
            internal_governance_fields_included: false,
        }
    }
}

fn default_knowledge_state_summary() -> KnowledgeStateSummary {
    KnowledgeStateSummary::default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeRuntimePromotionInput {
    pub trace_id: String,
    pub actor: String,
    pub reason: String,
    pub release_run_id: String,
    pub expires_at: Option<String>,
    pub expected_state_version: i64,
}

impl KnowledgeCalibrationLlmConfig {
    pub fn from_env() -> Result<Self> {
        let vars = knowledge_calibration_env_vars()
            .into_iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| (name.to_string(), value))
            })
            .collect::<BTreeMap<_, _>>();
        Self::from_env_map(&vars)
    }

    pub fn from_env_map(vars: &BTreeMap<String, String>) -> Result<Self> {
        let profile_id =
            required_calibration_env(vars, "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROFILE")?;
        if profile_id != KNOWLEDGE_CALIBRATION_PROFILE_ID {
            return Err(anyhow!(
                "knowledge calibration profile must be {KNOWLEDGE_CALIBRATION_PROFILE_ID}, got {profile_id}"
            ));
        }
        let profile_contract_version = required_calibration_env(
            vars,
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION",
        )?;
        if profile_contract_version != KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION {
            return Err(anyhow!(
                "knowledge calibration profile contract version must be {KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION}, got {profile_contract_version}"
            ));
        }
        let model_id = required_calibration_env(vars, "TONGLINGYU_KNOWLEDGE_CALIBRATION_MODEL")?;
        let upstream_id =
            required_calibration_env(vars, "TONGLINGYU_KNOWLEDGE_CALIBRATION_UPSTREAM_ID")?;
        let prompt_digest = validate_calibration_digest(&required_calibration_env(
            vars,
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROMPT_DIGEST",
        )?)?;
        let tool_policy_digest = validate_calibration_digest(&required_calibration_env(
            vars,
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_TOOL_POLICY_DIGEST",
        )?)?;
        let decoding_raw =
            required_calibration_env(vars, "TONGLINGYU_KNOWLEDGE_CALIBRATION_DECODING")?;
        let decoding = serde_json::from_str::<Value>(&decoding_raw)
            .with_context(|| "TONGLINGYU_KNOWLEDGE_CALIBRATION_DECODING must be JSON")?;
        if !decoding.is_object() {
            return Err(anyhow!(
                "TONGLINGYU_KNOWLEDGE_CALIBRATION_DECODING must be a JSON object"
            ));
        }
        let timeout_secs = parse_positive_u64_env(
            &required_calibration_env(vars, "TONGLINGYU_KNOWLEDGE_CALIBRATION_TIMEOUT_SECS")?,
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_TIMEOUT_SECS",
        )?;
        let retry_limit = parse_bounded_u32_env(
            &required_calibration_env(vars, "TONGLINGYU_KNOWLEDGE_CALIBRATION_RETRY_LIMIT")?,
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_RETRY_LIMIT",
            1,
            8,
        )?;
        let model_capability = validate_calibration_model_capability(&required_calibration_env(
            vars,
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_MODEL_CAPABILITY",
        )?)?;
        let reasoning_effort = validate_calibration_reasoning_effort(&required_calibration_env(
            vars,
            "TONGLINGYU_KNOWLEDGE_CALIBRATION_REASONING_EFFORT",
        )?)?;
        let mut config = Self {
            profile_id,
            model_id,
            upstream_id,
            prompt_digest,
            tool_policy_digest,
            decoding,
            timeout_secs,
            retry_limit,
            model_capability,
            reasoning_effort,
            profile_contract_version,
            config_digest: String::new(),
        };
        config.config_digest = hash_text(&serde_json::to_string(&canonical_json_value(
            &config.safe_summary_without_digest(),
        ))?);
        Ok(config)
    }

    pub fn safe_summary(&self) -> Value {
        let mut summary = self.safe_summary_without_digest();
        if let Some(object) = summary.as_object_mut() {
            object.insert("config_digest".to_string(), json!(&self.config_digest));
        }
        summary
    }

    fn safe_summary_without_digest(&self) -> Value {
        json!({
            "object": "tonglingyu.knowledge_calibration_llm_config_summary",
            "profile_id": self.profile_id,
            "model_id": self.model_id,
            "upstream_id": self.upstream_id,
            "prompt_digest": self.prompt_digest,
            "tool_policy_digest": self.tool_policy_digest,
            "decoding": self.decoding,
            "decoding_digest": hash_text(&serde_json::to_string(&canonical_json_value(&self.decoding)).unwrap_or_default()),
            "timeout_secs": self.timeout_secs,
            "retry_limit": self.retry_limit,
            "model_capability": self.model_capability,
            "reasoning_effort": self.reasoning_effort,
            "profile_contract_version": self.profile_contract_version,
            "env_var_names": knowledge_calibration_env_vars(),
            "contains_secret_values": false
        })
    }
}

pub fn knowledge_calibration_release_report(config: &KnowledgeCalibrationLlmConfig) -> Value {
    json!({
        "object": "tonglingyu.knowledge_calibration_release_report",
        "schema_version": KNOWLEDGE_CALIBRATION_REPORT_SCHEMA_VERSION,
        "profile_id": KNOWLEDGE_CALIBRATION_PROFILE_ID,
        "profile_contract_version": KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION,
        "llm_config": config.safe_summary(),
        "contains_secret_values": false,
        "runtime_usable_auto_promotion": false
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalFailureRecord {
    pub failure_id: String,
    pub trace_id: String,
    pub package_id: Option<String>,
    pub question_sha256: String,
    pub question_char_count: usize,
    pub question_summary: String,
    pub redacted_question_excerpt: String,
    pub kb_schema_version: String,
    pub kb_version_id: Option<String>,
    pub failure_type: String,
    pub redacted_query_terms: Vec<String>,
    pub required_evidence_types: Vec<String>,
    pub actual_evidence_types: Vec<String>,
    pub expected_evidence_ids: Vec<String>,
    pub selected_evidence_ids: Vec<String>,
    pub missing_evidence_types: Vec<String>,
    pub quality_issues: Vec<String>,
    pub agent_diagnosis: Option<String>,
    pub proposed_fix: Option<String>,
    pub human_review_status: String,
    pub reviewer: Option<String>,
    pub review_note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RetrievalFailureCreateInput {
    pub trace_id: String,
    pub package_id: Option<String>,
    pub question: String,
    pub quality_report: RetrievalQualityReport,
    pub selected_evidence_ids: Vec<String>,
    pub expected_evidence_ids: Vec<String>,
    pub agent_diagnosis: Option<String>,
    pub proposed_fix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalFailureView {
    SafeSummary,
    AdminDetail,
}

#[derive(Debug, Clone)]
pub struct RetrievalFailureListInput {
    pub human_review_status: Option<String>,
    pub failure_type: Option<String>,
    pub limit: usize,
    pub offset: usize,
    pub view: RetrievalFailureView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalFailureListResult {
    pub object: String,
    pub schema_version: String,
    pub limit: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct RetrievalFailureClusterInput {
    pub human_review_status: Option<String>,
    pub failure_type: Option<String>,
    pub min_cluster_size: usize,
    pub limit: usize,
    pub create_tasks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalFailureClusterResult {
    pub object: String,
    pub schema_version: String,
    pub scanned_failure_count: usize,
    pub cluster_count: usize,
    pub task_count: usize,
    pub min_cluster_size: usize,
    pub limit: usize,
    pub create_tasks: bool,
    pub clusters: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGovernanceTaskRecord {
    pub task_id: String,
    pub source_failure_id: Option<String>,
    pub source_entity_type: String,
    pub source_entity_id: String,
    pub trace_id: String,
    pub package_id: Option<String>,
    pub task_type: String,
    pub status: String,
    pub priority: String,
    pub agent_cluster_key: String,
    pub proposed_fix: String,
    pub reviewer: Option<String>,
    pub review_note: Option<String>,
    pub evidence_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub accepted_at: Option<String>,
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct KnowledgeGovernanceTaskCreateFromFailureInput {
    pub source_failure_id: String,
    pub task_type: Option<String>,
    pub priority: Option<String>,
    pub proposed_fix: Option<String>,
    pub agent_cluster_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KnowledgeGovernanceTaskCreateInput {
    pub source_entity_type: String,
    pub source_entity_id: String,
    pub trace_id: String,
    pub package_id: Option<String>,
    pub source_failure_id: Option<String>,
    pub task_type: String,
    pub priority: Option<String>,
    pub proposed_fix: Option<String>,
    pub agent_cluster_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KnowledgeGovernanceTaskListInput {
    pub status: Option<String>,
    pub task_type: Option<String>,
    pub priority: Option<String>,
    pub source_failure_id: Option<String>,
    pub source_entity_type: Option<String>,
    pub source_entity_id: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct KnowledgeGovernanceTaskUpdateInput {
    pub status: String,
    pub reviewer: Option<String>,
    pub review_note: Option<String>,
    pub evidence_ref: Option<String>,
    pub expected_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGovernanceTaskListResult {
    pub object: String,
    pub schema_version: String,
    pub limit: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct KnowledgePatchProposalCreateInput {
    pub proposal_type: String,
    pub trace_id: String,
    pub package_id: Option<String>,
    pub source_ref: Option<String>,
    pub payload: Value,
    pub created_by: Option<String>,
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePatchProposalRecord {
    pub proposal_id: String,
    pub proposal_type: String,
    pub trace_id: String,
    pub package_id: Option<String>,
    pub source_ref: String,
    pub payload: Value,
    pub payload_sha256: String,
    pub task_id: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct TonglingyuRuntimeStore {
    db_path: PathBuf,
}

impl TonglingyuRuntimeStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    pub fn open_connection(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("open runtime sqlite db {}", self.db_path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        init_runtime_schema(&conn)?;
        Ok(conn)
    }

    pub fn has_knowledge_base(&self) -> Result<bool> {
        has_knowledge_base(&self.db_path)
    }

    pub fn execute_workflow(&self, input: RuntimeWorkflowInput) -> Result<RuntimeWorkflowOutput> {
        let conn = self.open_connection()?;
        execute_runtime_workflow(&conn, input)
    }

    pub async fn execute_workflow_with_agent_runtime_steps(
        &self,
        input: RuntimeWorkflowInput,
    ) -> Result<RuntimeWorkflowOutput> {
        self.execute_workflow_with_agent_runtime_mode(
            input,
            TonglingyuAgentRuntimeMode::from_env()?,
        )
        .await
    }

    pub async fn execute_workflow_with_agent_runtime_mode(
        &self,
        input: RuntimeWorkflowInput,
        mode: TonglingyuAgentRuntimeMode,
    ) -> Result<RuntimeWorkflowOutput> {
        let registry =
            RuntimeProfileRegistry::new(agent_runtime_profile_contracts(&input.profiles));
        let runtime =
            tonglingyu_agent_runtime_client(mode, self.clone(), registry, &input.profiles)?;
        self.execute_workflow_with_agent_runtime_client(input, mode, runtime)
            .await
    }

    pub async fn execute_workflow_with_agent_runtime_client(
        &self,
        input: RuntimeWorkflowInput,
        mode: TonglingyuAgentRuntimeMode,
        runtime: Arc<dyn RuntimeClient>,
    ) -> Result<RuntimeWorkflowOutput> {
        let mut workflow = {
            let conn = self.open_connection()?;
            execute_runtime_workflow(&conn, input.clone())?
        };
        if let Err(error) = attach_agent_runtime_step_execution(
            &mut workflow,
            &input.profiles,
            &input.context,
            mode,
            runtime.clone(),
        )
        .await
        {
            self.record_agent_runtime_rejection(
                &workflow,
                mode,
                "agent_runtime_step_execution",
                &error,
            );
            return Err(error);
        }
        let agent_runtime_evidence_observations =
            apply_agent_runtime_evidence_outputs(&mut workflow, mode);
        let agent_runtime_package_observation =
            apply_agent_runtime_package_output(&mut workflow, mode);
        let mut agent_runtime_content_application =
            apply_agent_runtime_content_outputs(&mut workflow, mode);
        let repair_application = agent_runtime_content_application
            .filter(|application| should_repair_agent_runtime_draft(mode, Some(application)));
        if let Some(rejected_application) = repair_application {
            if let Err(error) = repair_agent_runtime_draft(
                &mut workflow,
                &input.profiles,
                &input.context,
                mode,
                runtime,
                &rejected_application,
            )
            .await
            {
                self.record_agent_runtime_rejection(
                    &workflow,
                    mode,
                    "agent_runtime_draft_repair",
                    &error,
                );
                return Err(error);
            }
            agent_runtime_content_application =
                apply_agent_runtime_content_outputs(&mut workflow, mode);
        }
        let agent_runtime_review_observation =
            apply_agent_runtime_reviewer_output(&mut workflow, mode);
        workflow.agent_runtime_summary = agent_runtime_execution_summary(
            mode,
            &workflow,
            agent_runtime_content_application.as_ref(),
        );
        workflow.stream_events = workflow_stream_events(
            &workflow.trace_id,
            &input.profiles.main,
            &workflow.package.package_id,
            &workflow.final_answer,
            &workflow.steps,
        );
        let conn = self.open_connection()?;
        for step in &workflow.steps {
            if let Some(agent_runtime) = &step.agent_runtime {
                append_runtime_audit_event(
                    &conn,
                    &workflow.trace_id,
                    "agent_runtime_profile_step_executed",
                    &json!({
                        "step_id": &step.step_id,
                        "profile": &step.profile,
                        "operation": &step.operation,
                        "agent_runtime": agent_runtime,
                    }),
                )?;
            }
        }
        append_runtime_audit_event(
            &conn,
            &workflow.trace_id,
            "agent_runtime_profile_execution_summarized",
            &workflow.agent_runtime_summary,
        )?;
        if let Some(application) = agent_runtime_content_application {
            let event_type = if application.draft_consumed {
                "agent_runtime_profile_draft_consumed"
            } else {
                "agent_runtime_profile_draft_rejected"
            };
            append_runtime_audit_event(
                &conn,
                &workflow.trace_id,
                event_type,
                &json!({
                    "answer_source": &workflow.answer_source,
                    "package_id": &workflow.package.package_id,
                    "review_status": &workflow.package.review.status,
                    "draft_profile": &input.profiles.main,
                    "runtime_mode": mode.as_str(),
                    "result_format": &application.result_format,
                    "draft_consumed": application.draft_consumed,
                    "rejected_reason": &application.rejected_reason,
                    "local_reviewer_enforced": true,
                    "content_used_for_final_answer": application.content_used_for_final_answer,
                }),
            )?;
        }
        for observation in agent_runtime_evidence_observations {
            append_runtime_audit_event(
                &conn,
                &workflow.trace_id,
                "agent_runtime_profile_evidence_observed",
                &json!({
                    "operation": &observation.operation,
                    "profile": &observation.profile,
                    "runtime_mode": mode.as_str(),
                    "result_format": &observation.result_format,
                    "evidence_ref_count": observation.evidence_ref_count,
                    "unknown_evidence_refs": &observation.unknown_evidence_refs,
                    "matches_runtime_evidence": observation.matches_runtime_evidence,
                    "rejected_reason": &observation.rejected_reason,
                    "local_evidence_enforced": true,
                }),
            )?;
        }
        if let Some(observation) = agent_runtime_package_observation {
            append_runtime_audit_event(
                &conn,
                &workflow.trace_id,
                "agent_runtime_profile_package_observed",
                &json!({
                    "package_id": &workflow.package.package_id,
                    "runtime_mode": mode.as_str(),
                    "result_format": &observation.result_format,
                    "observed_package_id": &observation.package_id,
                    "matches_runtime_package": observation.matches_runtime_package,
                    "rejected_reason": &observation.rejected_reason,
                    "local_package_enforced": true,
                }),
            )?;
        }
        if let Some(observation) = agent_runtime_review_observation {
            append_runtime_audit_event(
                &conn,
                &workflow.trace_id,
                "agent_runtime_profile_review_observed",
                &json!({
                    "package_id": &workflow.package.package_id,
                    "runtime_mode": mode.as_str(),
                    "result_format": &observation.result_format,
                    "review_status": &observation.review_status,
                    "local_review_status": &workflow.package.review.status,
                    "severity": &observation.severity,
                    "issue_count": observation.issue_count,
                    "required_revision_count": observation.required_revision_count,
                    "agrees_with_local_reviewer": observation.agrees_with_local_reviewer,
                    "local_reviewer_override": observation.local_reviewer_override,
                    "rejected_reason": &observation.rejected_reason,
                    "local_reviewer_enforced": true,
                }),
            )?;
        }
        if let Err(error) =
            validate_agent_runtime_execution_summary(mode, &workflow.agent_runtime_summary)
        {
            append_runtime_audit_event(
                &conn,
                &workflow.trace_id,
                "agent_runtime_profile_execution_rejected",
                &json!({
                    "runtime_mode": mode.as_str(),
                    "summary": &workflow.agent_runtime_summary,
                    "error": error.to_string(),
                }),
            )?;
            return Err(error);
        }
        Ok(workflow)
    }

    fn record_agent_runtime_rejection(
        &self,
        workflow: &RuntimeWorkflowOutput,
        mode: TonglingyuAgentRuntimeMode,
        failure_stage: &str,
        error: &anyhow::Error,
    ) {
        if let Ok(conn) = self.open_connection() {
            let executed_profile_step_count = workflow
                .steps
                .iter()
                .filter(|step| step.agent_runtime.is_some())
                .count();
            let provider_diagnostic = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<AgentCoreError>())
                .and_then(|error| error.diagnostic().cloned())
                .unwrap_or(Value::Null);
            let _ = append_runtime_audit_event(
                &conn,
                &workflow.trace_id,
                "agent_runtime_profile_execution_rejected",
                &json!({
                    "runtime_mode": mode.as_str(),
                    "failure_stage": failure_stage,
                    "profile_step_count": workflow.steps.len(),
                    "executed_profile_step_count": executed_profile_step_count,
                    "error": error.to_string(),
                    "provider_diagnostic": provider_diagnostic,
                    "local_governance_enforced": true,
                }),
            );
        }
    }

    pub fn execute_tool(&self, call: TonglingyuToolCall) -> Result<TonglingyuToolOutput> {
        let conn = self.open_connection()?;
        execute_tool(&conn, call)
    }

    pub fn search_cards(
        &self,
        question: &str,
        limit: usize,
        required_evidence_types: &[String],
    ) -> Result<Vec<EvidenceCard>> {
        match self.execute_tool(TonglingyuToolCall::TextSearch {
            question: question.to_string(),
            limit,
            required_evidence_types: required_evidence_types.to_vec(),
        })? {
            TonglingyuToolOutput::EvidenceCards { cards, .. } => Ok(cards),
            other => Err(anyhow!("unexpected runtime tool output: {:?}", other)),
        }
    }

    pub fn create_package(
        &self,
        trace_id: &str,
        question: &str,
        cards: Vec<EvidenceCard>,
    ) -> Result<EvidencePackage> {
        match self.execute_tool(TonglingyuToolCall::EvidencePackageCreate {
            trace_id: trace_id.to_string(),
            question: question.to_string(),
            cards,
        })? {
            TonglingyuToolOutput::EvidencePackage { package, .. } => Ok(*package),
            other => Err(anyhow!("unexpected runtime tool output: {:?}", other)),
        }
    }

    pub fn read_package(&self, package_id: &str) -> Result<Option<EvidencePackage>> {
        match self.execute_tool(TonglingyuToolCall::EvidencePackageRead {
            package_id: package_id.to_string(),
        })? {
            TonglingyuToolOutput::EvidencePackageRead { package, .. } => {
                Ok(package.map(|package| *package))
            }
            other => Err(anyhow!("unexpected runtime tool output: {:?}", other)),
        }
    }

    pub fn latest_package(&self) -> Result<Option<EvidencePackage>> {
        let conn = self.open_connection()?;
        latest_evidence_package_from_conn(&conn)
    }

    pub fn replay_package(&self, package_id: &str) -> Result<Option<Value>> {
        match self.execute_tool(TonglingyuToolCall::EvidencePackageReplay {
            package_id: package_id.to_string(),
        })? {
            TonglingyuToolOutput::EvidencePackageReplay { replay, .. } => Ok(replay),
            other => Err(anyhow!("unexpected runtime tool output: {:?}", other)),
        }
    }

    pub fn store_stats(&self) -> Result<RuntimeStoreStats> {
        let conn = self.open_connection()?;
        runtime_store_stats(&conn)
    }

    pub fn runtime_schema_migration_preflight(&self) -> Result<Value> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("open runtime sqlite db {}", self.db_path.display()))?;
        runtime_schema_migration_preflight(&conn)
    }

    pub fn package_ids_for_trace(&self, trace_id: &str) -> Result<Vec<String>> {
        let conn = self.open_connection()?;
        runtime_package_ids_for_trace(&conn, trace_id)
    }

    pub fn audit_events_for_trace(&self, trace_id: &str) -> Result<Vec<Value>> {
        let conn = self.open_connection()?;
        runtime_audit_events_for_trace(&conn, trace_id)
    }

    pub fn create_retrieval_failure(
        &self,
        input: RetrievalFailureCreateInput,
    ) -> Result<RetrievalFailureRecord> {
        let conn = self.open_connection()?;
        create_retrieval_failure(&conn, input)
    }

    pub fn list_retrieval_failures(
        &self,
        input: RetrievalFailureListInput,
    ) -> Result<RetrievalFailureListResult> {
        let conn = self.open_connection()?;
        list_retrieval_failures(&conn, input)
    }

    pub fn cluster_retrieval_failures(
        &self,
        input: RetrievalFailureClusterInput,
    ) -> Result<RetrievalFailureClusterResult> {
        let conn = self.open_connection()?;
        cluster_retrieval_failures(&conn, input)
    }

    pub fn list_retrieval_failures_for_trace(
        &self,
        trace_id: &str,
        view: RetrievalFailureView,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let conn = self.open_connection()?;
        list_retrieval_failures_for_trace(&conn, trace_id, view, limit)
    }

    pub fn list_retrieval_failures_for_package(
        &self,
        package_id: &str,
        view: RetrievalFailureView,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let conn = self.open_connection()?;
        list_retrieval_failures_for_package(&conn, package_id, view, limit)
    }

    pub fn read_retrieval_failure(
        &self,
        failure_id: &str,
        view: RetrievalFailureView,
    ) -> Result<Option<Value>> {
        let conn = self.open_connection()?;
        read_retrieval_failure(&conn, failure_id, view)
    }

    pub fn update_retrieval_failure_status(
        &self,
        failure_id: &str,
        human_review_status: &str,
        reviewer: Option<&str>,
        review_note: Option<&str>,
    ) -> Result<Option<RetrievalFailureRecord>> {
        let conn = self.open_connection()?;
        update_retrieval_failure_status(
            &conn,
            failure_id,
            human_review_status,
            reviewer,
            review_note,
        )
    }

    pub fn update_retrieval_failure_status_checked(
        &self,
        failure_id: &str,
        human_review_status: &str,
        reviewer: Option<&str>,
        review_note: Option<&str>,
        expected_updated_at: Option<&str>,
    ) -> Result<Option<RetrievalFailureRecord>> {
        let conn = self.open_connection()?;
        update_retrieval_failure_status_checked(
            &conn,
            failure_id,
            human_review_status,
            reviewer,
            review_note,
            expected_updated_at,
        )
    }

    pub fn create_governance_task_from_failure(
        &self,
        input: KnowledgeGovernanceTaskCreateFromFailureInput,
    ) -> Result<Option<KnowledgeGovernanceTaskRecord>> {
        let conn = self.open_connection()?;
        create_governance_task_from_failure(&conn, input)
    }

    pub fn create_governance_task(
        &self,
        input: KnowledgeGovernanceTaskCreateInput,
    ) -> Result<KnowledgeGovernanceTaskRecord> {
        let conn = self.open_connection()?;
        create_governance_task(&conn, input)
    }

    pub fn list_governance_tasks(
        &self,
        input: KnowledgeGovernanceTaskListInput,
    ) -> Result<KnowledgeGovernanceTaskListResult> {
        let conn = self.open_connection()?;
        list_governance_tasks(&conn, input)
    }

    pub fn list_governance_tasks_for_trace(
        &self,
        trace_id: &str,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let conn = self.open_connection()?;
        list_governance_tasks_for_trace(&conn, trace_id, limit)
    }

    pub fn list_governance_tasks_for_package(
        &self,
        package_id: &str,
        limit: usize,
    ) -> Result<Vec<Value>> {
        let conn = self.open_connection()?;
        list_governance_tasks_for_package(&conn, package_id, limit)
    }

    pub fn read_governance_task(&self, task_id: &str) -> Result<Option<Value>> {
        let conn = self.open_connection()?;
        read_governance_task(&conn, task_id)
    }

    pub fn update_governance_task(
        &self,
        task_id: &str,
        input: KnowledgeGovernanceTaskUpdateInput,
    ) -> Result<Option<KnowledgeGovernanceTaskRecord>> {
        let conn = self.open_connection()?;
        update_governance_task(&conn, task_id, input)
    }

    pub fn create_knowledge_item(
        &self,
        input: KnowledgeItemCreateInput,
    ) -> Result<KnowledgeItemRecord> {
        let conn = self.open_connection()?;
        create_knowledge_item(&conn, input)
    }

    pub fn read_knowledge_item(&self, item_id: &str) -> Result<Option<KnowledgeItemRecord>> {
        let conn = self.open_connection()?;
        read_knowledge_item(&conn, item_id)
    }

    pub fn list_knowledge_items(
        &self,
        input: KnowledgeItemListInput,
    ) -> Result<KnowledgeItemListResult> {
        let conn = self.open_connection()?;
        list_knowledge_items(&conn, input)
    }

    pub fn update_knowledge_item_state(
        &self,
        item_id: &str,
        input: KnowledgeItemStateUpdateInput,
    ) -> Result<Option<KnowledgeItemRecord>> {
        let conn = self.open_connection()?;
        update_knowledge_item_state(&conn, item_id, input)
    }

    pub fn promote_knowledge_item_runtime_usable(
        &self,
        item_id: &str,
        input: KnowledgeRuntimePromotionInput,
    ) -> Result<Option<KnowledgeItemRecord>> {
        let conn = self.open_connection()?;
        promote_knowledge_item_runtime_usable(&conn, item_id, input)
    }

    pub fn review_knowledge_item_human(
        &self,
        item_id: &str,
        input: KnowledgeItemHumanReviewInput,
    ) -> Result<Option<KnowledgeItemHumanReviewResult>> {
        let conn = self.open_connection()?;
        review_knowledge_item_human(&conn, item_id, input)
    }

    pub fn create_knowledge_calibration_job(
        &self,
        input: KnowledgeCalibrationJobCreateInput,
    ) -> Result<KnowledgeCalibrationJobRecord> {
        let conn = self.open_connection()?;
        create_knowledge_calibration_job(&conn, input)
    }

    pub fn lease_knowledge_calibration_job(
        &self,
        job_id: &str,
        lease_owner: &str,
        lease_seconds: u64,
    ) -> Result<Option<KnowledgeCalibrationJobRecord>> {
        let conn = self.open_connection()?;
        lease_knowledge_calibration_job(&conn, job_id, lease_owner, lease_seconds)
    }

    pub fn heartbeat_knowledge_calibration_job(
        &self,
        job_id: &str,
        lease_owner: &str,
    ) -> Result<Option<KnowledgeCalibrationJobRecord>> {
        let conn = self.open_connection()?;
        heartbeat_knowledge_calibration_job(&conn, job_id, lease_owner)
    }

    pub fn complete_knowledge_calibration_job(
        &self,
        job_id: &str,
        lease_owner: &str,
        report_id: &str,
    ) -> Result<Option<KnowledgeCalibrationJobRecord>> {
        let conn = self.open_connection()?;
        complete_knowledge_calibration_job(&conn, job_id, lease_owner, report_id)
    }

    pub fn fail_knowledge_calibration_job(
        &self,
        job_id: &str,
        lease_owner: &str,
        error: &str,
        retryable: bool,
    ) -> Result<Option<KnowledgeCalibrationJobRecord>> {
        let conn = self.open_connection()?;
        fail_knowledge_calibration_job(&conn, job_id, lease_owner, error, retryable)
    }

    pub fn run_knowledge_calibration_offline(
        &self,
        input: KnowledgeCalibrationRunInput,
    ) -> Result<KnowledgeCalibrationReportRecord> {
        let conn = self.open_connection()?;
        run_knowledge_calibration_offline(&conn, input)
    }

    pub fn read_knowledge_calibration_report(
        &self,
        report_id: &str,
    ) -> Result<Option<KnowledgeCalibrationReportRecord>> {
        let conn = self.open_connection()?;
        read_knowledge_calibration_report(&conn, report_id)
    }

    pub fn create_knowledge_patch_proposal(
        &self,
        input: KnowledgePatchProposalCreateInput,
    ) -> Result<Value> {
        let conn = self.open_connection()?;
        create_knowledge_patch_proposal(&conn, input)
    }

    pub fn rebuild_knowledge_base_from_snapshots(
        &self,
        source_root: &Path,
    ) -> Result<KnowledgeBaseBuildReport> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction()?;
        let report = rebuild_knowledge_base_from_snapshots(&tx, source_root)?;
        tx.commit()?;
        Ok(report)
    }

    pub fn record_kb_version_diff_eval_summaries(
        &self,
        report_id: &str,
        before_eval_summary: Option<Value>,
        after_eval_summary: Value,
    ) -> Result<Option<Value>> {
        let conn = self.open_connection()?;
        record_kb_version_diff_eval_summaries(
            &conn,
            report_id,
            before_eval_summary,
            after_eval_summary,
        )
    }

    pub fn prune_data(&self, retention_days: u32, dry_run: bool) -> Result<Value> {
        let conn = self.open_connection()?;
        prune_runtime_data(&conn, retention_days, dry_run)
    }
}

#[derive(Debug, Clone)]
pub struct TonglingyuRuntimeToolExecutor {
    store: TonglingyuRuntimeStore,
}

impl TonglingyuRuntimeToolExecutor {
    pub fn new(store: TonglingyuRuntimeStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl RuntimeToolExecutor for TonglingyuRuntimeToolExecutor {
    async fn execute_tool(
        &self,
        call: RuntimeToolCall,
        _spec: RuntimeToolSpec,
    ) -> CoreResult<RuntimeToolResult> {
        let tool_call = tonglingyu_tool_call_from_runtime(&call)?;
        let tool_output = self.store.execute_tool(tool_call).map_err(|_| {
            AgentCoreError::coded(
                ErrorCode::InternalError,
                "Tonglingyu runtime tool execution failed",
            )
        })?;
        let output_ref = runtime_tool_output_ref(&call, &tool_output);
        let output = serde_json::to_value(&tool_output).map_err(|_| {
            AgentCoreError::coded(
                ErrorCode::InternalError,
                "Tonglingyu runtime tool output was not serializable",
            )
        })?;
        Ok(RuntimeToolResult {
            call_id: call.call_id,
            profile_id: call.profile_id,
            tool_name: call.tool_name,
            output_ref: Some(output_ref),
            output,
            metadata: json!({
                "runtime_tool_executor": "tonglingyu-runtime-store",
                "tool_version": TOOL_CATALOG_VERSION,
                "trace_id": call.trace_id,
            }),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TonglingyuAgentRuntimeMode {
    #[default]
    Minimal,
    Hermes,
    OpenAiCompatibleNetwork,
}

impl TonglingyuAgentRuntimeMode {
    pub fn from_env() -> Result<Self> {
        if let Some(mode) = workflow_agent_runtime_mode_from_role_provider_source(&env_nonempty)? {
            return Ok(mode);
        }
        let value = std::env::var("TONGLINGYU_AGENT_RUNTIME_MODE").unwrap_or_default();
        match value.trim() {
            "" => Err(anyhow!(
                "workflow agent role provider config is required: {}",
                WORKFLOW_AGENT_ROLE_PROVIDER_ENVS.join(",")
            )),
            "openai-compatible-network" => Ok(Self::OpenAiCompatibleNetwork),
            other => Err(anyhow!(
                "TONGLINGYU_AGENT_RUNTIME_MODE={other} is not supported for workflow runtime; configure role provider backend openai-compatible-network"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Hermes => "hermes",
            Self::OpenAiCompatibleNetwork => "openai-compatible-network",
        }
    }
}

const WORKFLOW_AGENT_ROLE_PROVIDER_ENVS: &[&str] = &[
    "TONGLINGYU_AGENT_ROLE_TEXT_PROVIDER",
    "TONGLINGYU_AGENT_ROLE_PACKAGE_PROVIDER",
    "TONGLINGYU_AGENT_ROLE_DRAFT_PROVIDER",
    "TONGLINGYU_AGENT_ROLE_REVIEW_PROVIDER",
];

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn provider_profile_env_suffix(profile: &str) -> Result<String> {
    let trimmed = profile.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("agent provider profile name must not be empty"));
    }
    let mut output = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_uppercase());
        } else if ch == '_' || ch == '-' {
            output.push('_');
        } else {
            return Err(anyhow!(
                "agent provider profile name contains unsupported character: {profile}"
            ));
        }
    }
    Ok(output)
}

fn agent_provider_env_name(profile: &str, field: &str) -> Result<String> {
    Ok(format!(
        "TONGLINGYU_AGENT_PROVIDER_{}_{}",
        provider_profile_env_suffix(profile)?,
        field
    ))
}

fn required_agent_provider_env(profile: &str, field: &str) -> Result<String> {
    required_agent_provider_env_from(profile, field, &env_nonempty)
}

fn required_agent_provider_env_from(
    profile: &str,
    field: &str,
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<String> {
    let env_name = agent_provider_env_name(profile, field)?;
    get_env(&env_name).ok_or_else(|| anyhow!("{env_name} must be configured"))
}

fn workflow_agent_provider_profile_from_env() -> Result<Option<String>> {
    workflow_agent_provider_profile_from_source(&env_nonempty)
}

fn workflow_agent_provider_profile_from_source(
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<String>> {
    let configured = WORKFLOW_AGENT_ROLE_PROVIDER_ENVS
        .iter()
        .filter_map(|env_name| get_env(env_name).map(|value| (*env_name, value)))
        .collect::<Vec<_>>();
    if configured.is_empty() {
        return Ok(None);
    }
    if configured.len() != WORKFLOW_AGENT_ROLE_PROVIDER_ENVS.len() {
        let missing = WORKFLOW_AGENT_ROLE_PROVIDER_ENVS
            .iter()
            .filter(|env_name| get_env(env_name).is_none())
            .copied()
            .collect::<Vec<_>>();
        return Err(anyhow!(
            "workflow agent role provider config incomplete: missing {}",
            missing.join(",")
        ));
    }
    let profile = configured[0].1.clone();
    let mismatched = configured
        .iter()
        .filter(|(_, value)| value != &profile)
        .map(|(env_name, _)| *env_name)
        .collect::<Vec<_>>();
    if !mismatched.is_empty() {
        return Err(anyhow!(
            "workflow agent role providers must use one provider profile until per-step runtime routing is enabled: {}",
            mismatched.join(",")
        ));
    }
    Ok(Some(profile))
}

fn workflow_agent_runtime_mode_from_role_provider_source(
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<TonglingyuAgentRuntimeMode>> {
    let Some(profile) = workflow_agent_provider_profile_from_source(get_env)? else {
        return Ok(None);
    };
    let backend = required_agent_provider_env_from(&profile, "BACKEND", get_env)?;
    let backend = backend.trim();
    if backend == "openai-compatible-network" {
        Ok(Some(TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork))
    } else {
        Err(anyhow!(
            "workflow agent provider {profile} must use backend openai-compatible-network; got {backend}"
        ))
    }
}

fn env_positive_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowProfiles {
    pub main: String,
    pub text: String,
    pub commentary: String,
    pub reviewer: String,
}

impl Default for RuntimeWorkflowProfiles {
    fn default() -> Self {
        Self {
            main: "honglou-main".to_string(),
            text: "honglou-text".to_string(),
            commentary: "honglou-commentary".to_string(),
            reviewer: "honglou-reviewer".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowPlanInput {
    pub question_type: String,
    #[serde(default)]
    pub required_evidence_types: Vec<String>,
    #[serde(default)]
    pub blocked_controls: Vec<String>,
    pub profiles: RuntimeWorkflowProfiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowPlan {
    pub schema_version: String,
    pub policy_version: String,
    pub question_type: String,
    pub required_evidence_types: Vec<String>,
    pub blocked_controls: Vec<String>,
    pub steps: Vec<RuntimeWorkflowPlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowPlanStep {
    pub step_id: String,
    pub profile: String,
    pub profile_contract_version: String,
    pub operation: String,
    pub required: bool,
    pub allowed_tools: Vec<String>,
}

pub fn runtime_workflow_plan(input: RuntimeWorkflowPlanInput) -> RuntimeWorkflowPlan {
    let mut steps = vec![RuntimeWorkflowPlanStep {
        step_id: "step-01-text-search".to_string(),
        profile: input.profiles.text.clone(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "text_evidence_search".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.text.search".to_string()],
    }];
    if input
        .required_evidence_types
        .iter()
        .any(|item| item == "commentary")
    {
        steps.push(RuntimeWorkflowPlanStep {
            step_id: "step-02-commentary-search".to_string(),
            profile: input.profiles.commentary.clone(),
            profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
            operation: "commentary_evidence_search".to_string(),
            required: true,
            allowed_tools: vec!["tonglingyu.commentary.search".to_string()],
        });
    }
    steps.push(RuntimeWorkflowPlanStep {
        step_id: step_id(steps.len() + 1, "package-create"),
        profile: input.profiles.main.clone(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "evidence_package_create".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.evidence.package.create".to_string()],
    });
    steps.push(RuntimeWorkflowPlanStep {
        step_id: step_id(steps.len() + 1, "draft-answer"),
        profile: input.profiles.main.clone(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "draft_answer".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
    });
    steps.push(RuntimeWorkflowPlanStep {
        step_id: step_id(steps.len() + 1, "review-answer"),
        profile: input.profiles.reviewer.clone(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: "review_answer".to_string(),
        required: true,
        allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
    });
    RuntimeWorkflowPlan {
        schema_version: RUNTIME_WORKFLOW_PLAN_SCHEMA_VERSION.to_string(),
        policy_version: RUNTIME_WORKFLOW_PLAN_POLICY_VERSION.to_string(),
        question_type: input.question_type,
        required_evidence_types: input.required_evidence_types,
        blocked_controls: input.blocked_controls,
        steps,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowInput {
    pub trace_id: String,
    pub question: String,
    pub limit: usize,
    #[serde(default)]
    pub required_evidence_types: Vec<String>,
    pub profiles: RuntimeWorkflowProfiles,
    pub context: RuntimeContextContract,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRuntimePlanGateInput {
    pub trace_id: String,
    pub question: String,
    #[serde(default)]
    pub required_evidence_types: Vec<String>,
    pub profiles: RuntimeWorkflowProfiles,
    pub context: RuntimeContextContract,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeContextContract {
    pub trace_id: String,
    pub interaction_context_id: String,
    pub context_pack_ref: String,
    pub context_pack_schema_version: String,
    pub context_pack_digest: String,
    #[serde(default)]
    pub projections: Vec<RuntimeContextProjection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeContextProjection {
    pub context_projection_id: String,
    pub context_projection_ref: String,
    pub context_pack_ref: String,
    pub context_projection_schema_version: String,
    pub context_projection_digest: String,
    pub consumer_type: String,
    pub consumer_name: String,
    pub runtime_adapter: String,
    #[serde(default)]
    pub projection_payload: Value,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub forbidden_tools: Vec<String>,
    #[serde(default)]
    pub output_contract: Value,
    pub tool_policy_digest: String,
    pub output_contract_digest: String,
}

impl RuntimeContextContract {
    fn projection_for_consumer(&self, consumer_name: &str) -> Result<&RuntimeContextProjection> {
        self.projections
            .iter()
            .find(|projection| projection.consumer_name == consumer_name)
            .ok_or_else(|| anyhow!("context projection missing for consumer {consumer_name}"))
    }
}

impl RuntimeContextProjection {
    fn tool_policy_value(&self) -> Value {
        json!({
            "allowed_tools": &self.allowed_tools,
            "forbidden_tools": &self.forbidden_tools,
        })
    }

    fn digest_value(&self) -> Value {
        json!({
            "context_projection_id": &self.context_projection_id,
            "context_projection_ref": &self.context_projection_ref,
            "context_pack_ref": &self.context_pack_ref,
            "consumer_type": &self.consumer_type,
            "consumer_name": &self.consumer_name,
            "runtime_adapter": &self.runtime_adapter,
            "projection_payload": &self.projection_payload,
            "allowed_tools": &self.allowed_tools,
            "forbidden_tools": &self.forbidden_tools,
            "output_contract": &self.output_contract,
            "tool_policy_digest": &self.tool_policy_digest,
            "output_contract_digest": &self.output_contract_digest,
            "schema_version": &self.context_projection_schema_version,
        })
    }

    fn audit_contract(&self) -> Value {
        json!({
            "context_projection_id": &self.context_projection_id,
            "context_projection_ref": &self.context_projection_ref,
            "context_projection_schema_version": &self.context_projection_schema_version,
            "context_projection_digest": &self.context_projection_digest,
            "consumer_type": &self.consumer_type,
            "consumer_name": &self.consumer_name,
            "runtime_adapter": &self.runtime_adapter,
            "tool_policy_digest": &self.tool_policy_digest,
            "output_contract_digest": &self.output_contract_digest,
            "allowed_tools": &self.allowed_tools,
            "forbidden_tools": &self.forbidden_tools,
            "projection_payload_sha256": hash_json(&self.projection_payload),
        })
    }
}

fn validate_runtime_context_contract(
    input: &RuntimeWorkflowInput,
    plan: &RuntimeWorkflowPlan,
) -> Result<()> {
    let context = &input.context;
    if context.trace_id != input.trace_id {
        return Err(anyhow!(
            "runtime context trace_id does not match workflow trace"
        ));
    }
    if context.context_pack_ref.trim().is_empty() {
        return Err(anyhow!("runtime context missing context_pack_ref"));
    }
    if context.context_pack_schema_version != RUNTIME_CONTEXT_PACK_SCHEMA_VERSION {
        return Err(anyhow!("unsupported context pack schema version"));
    }
    if context.context_pack_digest.trim().is_empty() {
        return Err(anyhow!("runtime context missing context_pack_digest"));
    }
    if context.projections.is_empty() {
        return Err(anyhow!("runtime context missing context projections"));
    }
    let valid_consumers = [
        input.profiles.main.as_str(),
        input.profiles.text.as_str(),
        input.profiles.commentary.as_str(),
        input.profiles.reviewer.as_str(),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    for projection in &context.projections {
        validate_runtime_projection_contract(context, projection, &valid_consumers)?;
    }
    for step in &plan.steps {
        let projection = context.projection_for_consumer(&step.profile)?;
        validate_step_projection_binding(step, projection)?;
    }
    Ok(())
}

fn validate_runtime_projection_contract(
    context: &RuntimeContextContract,
    projection: &RuntimeContextProjection,
    valid_consumers: &BTreeSet<&str>,
) -> Result<()> {
    if projection.context_projection_ref.trim().is_empty() {
        return Err(anyhow!("runtime projection missing context_projection_ref"));
    }
    if projection.context_pack_ref != context.context_pack_ref {
        return Err(anyhow!(
            "context projection {} does not belong to current context pack",
            projection.context_projection_ref
        ));
    }
    if projection.context_projection_schema_version != RUNTIME_CONTEXT_PROJECTION_SCHEMA_VERSION {
        return Err(anyhow!("unsupported context projection schema version"));
    }
    if projection.context_projection_digest.trim().is_empty() {
        return Err(anyhow!("runtime projection missing digest"));
    }
    if hash_json(&projection.digest_value()) != projection.context_projection_digest {
        return Err(anyhow!(
            "runtime projection context_projection_digest mismatch"
        ));
    }
    if projection.consumer_type != RUNTIME_CONTEXT_CONSUMER_TYPE {
        return Err(anyhow!("unsupported runtime context consumer_type"));
    }
    if projection.runtime_adapter != TONGLINGYU_RUNTIME_ADAPTER {
        return Err(anyhow!("unsupported runtime adapter"));
    }
    if !valid_consumers.contains(projection.consumer_name.as_str()) {
        return Err(anyhow!(
            "unknown runtime context consumer {}",
            projection.consumer_name
        ));
    }
    if hash_json(&projection.tool_policy_value()) != projection.tool_policy_digest {
        return Err(anyhow!("runtime projection tool_policy_digest mismatch"));
    }
    if hash_json(&projection.output_contract) != projection.output_contract_digest {
        return Err(anyhow!(
            "runtime projection output_contract_digest mismatch"
        ));
    }
    Ok(())
}

fn validate_step_projection_binding(
    step: &RuntimeWorkflowPlanStep,
    projection: &RuntimeContextProjection,
) -> Result<()> {
    if projection.consumer_name != step.profile {
        return Err(anyhow!(
            "runtime step {} consumer projection mismatch",
            step.step_id
        ));
    }
    let allowed = projection
        .allowed_tools
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let forbidden = projection
        .forbidden_tools
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for tool in &step.allowed_tools {
        if forbidden.contains(tool.as_str()) {
            return Err(anyhow!(
                "runtime step {} requested forbidden tool {}",
                step.step_id,
                tool
            ));
        }
        if !allowed.contains(tool.as_str()) {
            return Err(anyhow!(
                "runtime step {} requested tool outside context projection: {}",
                step.step_id,
                tool
            ));
        }
    }
    Ok(())
}

fn validate_runtime_context_pack_digest(
    conn: &Connection,
    context: &RuntimeContextContract,
) -> Result<()> {
    if !sqlite_table_exists(conn, "context_packs")? {
        return Ok(());
    }
    let stored_digest = conn
        .query_row(
            "SELECT COALESCE(digest, '') FROM context_packs WHERE context_pack_ref = ?1 LIMIT 1",
            params![&context.context_pack_ref],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match stored_digest {
        Some(digest) if digest == context.context_pack_digest => Ok(()),
        Some(_) => Err(anyhow!("runtime context_pack_digest mismatch")),
        None if context.context_pack_ref.ends_with("/local")
            || context.context_pack_ref.ends_with("/test") =>
        {
            Ok(())
        }
        None => Err(anyhow!(
            "runtime context_pack_ref not found in context store"
        )),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRuntimePlanGateReport {
    pub status: String,
    pub trace_id: String,
    pub agent_runtime_client: String,
    pub profile_contract_version: String,
    pub profile_contract_count: usize,
    pub runtime_step_count: usize,
    pub requested_tools_by_profile: BTreeMap<String, Vec<String>>,
    pub runtime_step_plan: Value,
    pub runtime_step_outputs: Value,
    pub agent_runtime_output_ref: Option<String>,
    pub effective_tool_set: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowStepReport {
    pub step_id: String,
    pub profile: String,
    pub profile_contract_version: String,
    pub operation: String,
    pub status: String,
    pub required: bool,
    pub allowed_tools: Vec<String>,
    pub tool_calls: Vec<String>,
    pub input_ref: Option<String>,
    pub output_ref: String,
    pub duration_ms: u128,
    pub trace_id: String,
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_runtime: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowStreamEvent {
    pub sequence: u64,
    pub event_type: String,
    pub profile: String,
    pub trace_id: String,
    pub content_delta: Option<String>,
    pub output_ref: Option<String>,
    pub package_id: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkflowOutput {
    pub trace_id: String,
    pub question: String,
    pub package: EvidencePackage,
    pub draft_answer: String,
    pub final_answer: String,
    pub answer_source: String,
    #[serde(default = "default_agent_runtime_summary")]
    pub agent_runtime_summary: Value,
    pub steps: Vec<RuntimeWorkflowStepReport>,
    pub stream_events: Vec<RuntimeWorkflowStreamEvent>,
}

#[derive(Debug, Deserialize)]
struct SourceMetadata {
    source_id: String,
    source_category: String,
    format: Option<String>,
    title: Option<String>,
    work: Option<String>,
    edition: Option<String>,
    language: Option<String>,
    source_url: Option<String>,
    api_url: Option<String>,
    fetched_at: Option<String>,
    license: Option<String>,
    license_url: Option<String>,
    license_source_url: Option<String>,
    attribution: Option<String>,
    usage_boundary: Option<String>,
    notes: Option<String>,
    #[serde(default)]
    snapshot_contract: Value,
}

#[derive(Debug, Deserialize)]
struct ExtractionReport {
    documents: i64,
    blocks: i64,
    rare_char_annotations: Option<i64>,
    missing: i64,
    raw_html_files: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct DocumentRecord {
    source_id: String,
    section_id: String,
    section_index: Option<i64>,
    title: Option<String>,
    display_title: Option<String>,
    fullurl: Option<String>,
    pageid: Option<i64>,
    revision_id: Option<i64>,
    revision_timestamp: Option<String>,
    wikitext_sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct BlockRecord {
    block_id: String,
    block_index: i64,
    kind: String,
    revision_id: Option<i64>,
    section_id: String,
    source_id: String,
    source_title: String,
    source_url: String,
    tag: Option<String>,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum TonglingyuToolCall {
    #[serde(rename = "tonglingyu.text.search")]
    TextSearch {
        question: String,
        limit: usize,
        required_evidence_types: Vec<String>,
    },
    #[serde(rename = "tonglingyu.commentary.search")]
    CommentarySearch { question: String, limit: usize },
    #[serde(rename = "tonglingyu.evidence.package.create")]
    EvidencePackageCreate {
        trace_id: String,
        question: String,
        cards: Vec<EvidenceCard>,
    },
    #[serde(rename = "tonglingyu.evidence.package.read")]
    EvidencePackageRead { package_id: String },
    #[serde(rename = "tonglingyu.evidence.package.replay")]
    EvidencePackageReplay { package_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "object", rename_all = "snake_case")]
pub enum TonglingyuToolOutput {
    EvidenceCards {
        cards: Vec<EvidenceCard>,
        quality_report: Box<RetrievalQualityReport>,
        tool_version: String,
    },
    EvidencePackage {
        package: Box<EvidencePackage>,
        tool_version: String,
    },
    EvidencePackageRead {
        package: Option<Box<EvidencePackage>>,
        tool_version: String,
    },
    EvidencePackageReplay {
        replay: Option<Value>,
        tool_version: String,
    },
}

fn tonglingyu_tool_call_from_runtime(call: &RuntimeToolCall) -> CoreResult<TonglingyuToolCall> {
    match call.tool_name.as_str() {
        "tonglingyu.text.search" => Ok(TonglingyuToolCall::TextSearch {
            question: runtime_tool_string_arg(&call.arguments, "question")?,
            limit: runtime_tool_usize_arg(&call.arguments, "limit")?,
            required_evidence_types: runtime_tool_string_vec_arg(
                &call.arguments,
                "required_evidence_types",
            )?,
        }),
        "tonglingyu.commentary.search" => Ok(TonglingyuToolCall::CommentarySearch {
            question: runtime_tool_string_arg(&call.arguments, "question")?,
            limit: runtime_tool_usize_arg(&call.arguments, "limit")?,
        }),
        "tonglingyu.evidence.package.create" => Ok(TonglingyuToolCall::EvidencePackageCreate {
            trace_id: runtime_tool_string_arg(&call.arguments, "trace_id")?,
            question: runtime_tool_string_arg(&call.arguments, "question")?,
            cards: runtime_tool_cards_arg(&call.arguments, "cards")?,
        }),
        "tonglingyu.evidence.package.read" => Ok(TonglingyuToolCall::EvidencePackageRead {
            package_id: runtime_tool_string_arg(&call.arguments, "package_id")?,
        }),
        "tonglingyu.evidence.package.replay" => Ok(TonglingyuToolCall::EvidencePackageReplay {
            package_id: runtime_tool_string_arg(&call.arguments, "package_id")?,
        }),
        _ => Err(AgentCoreError::coded(
            ErrorCode::NotFound,
            "Tonglingyu runtime tool was not registered",
        )),
    }
}

fn runtime_tool_string_arg(arguments: &Value, field: &str) -> CoreResult<String> {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| runtime_tool_arg_error("missing or invalid string argument"))
}

fn runtime_tool_usize_arg(arguments: &Value, field: &str) -> CoreResult<usize> {
    let value = arguments
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| runtime_tool_arg_error("missing or invalid integer argument"))?;
    usize::try_from(value).map_err(|_| runtime_tool_arg_error("integer argument is too large"))
}

fn runtime_tool_string_vec_arg(arguments: &Value, field: &str) -> CoreResult<Vec<String>> {
    arguments
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| runtime_tool_arg_error("missing or invalid string array argument"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| runtime_tool_arg_error("invalid string array item"))
        })
        .collect()
}

fn runtime_tool_cards_arg(arguments: &Value, field: &str) -> CoreResult<Vec<EvidenceCard>> {
    let value = arguments
        .get(field)
        .cloned()
        .ok_or_else(|| runtime_tool_arg_error("missing evidence cards argument"))?;
    serde_json::from_value(value)
        .map_err(|_| runtime_tool_arg_error("invalid evidence cards argument"))
}

fn runtime_tool_arg_error(message: &'static str) -> AgentCoreError {
    AgentCoreError::coded(ErrorCode::Conflict, message)
}

fn runtime_tool_output_ref(call: &RuntimeToolCall, output: &TonglingyuToolOutput) -> String {
    match output {
        TonglingyuToolOutput::EvidenceCards { cards, .. } => {
            evidence_set_output_ref(&call.trace_id, &evidence_ids(cards))
        }
        TonglingyuToolOutput::EvidencePackage { package, .. } => {
            format!(
                "runtime://tonglingyu/{}/packages/{}",
                call.trace_id, package.package_id
            )
        }
        TonglingyuToolOutput::EvidencePackageRead {
            package: Some(package),
            ..
        } => {
            format!(
                "runtime://tonglingyu/{}/packages/{}",
                call.trace_id, package.package_id
            )
        }
        TonglingyuToolOutput::EvidencePackageReplay {
            replay: Some(replay),
            ..
        } => replay
            .get("package")
            .and_then(|package| package.get("package_id"))
            .and_then(Value::as_str)
            .map(|package_id| {
                format!(
                    "runtime://tonglingyu/{}/packages/{package_id}",
                    call.trace_id
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "runtime://tonglingyu/{}/tools/{}",
                    call.trace_id, call.call_id
                )
            }),
        _ => format!(
            "runtime://tonglingyu/{}/tools/{}",
            call.trace_id, call.call_id
        ),
    }
}

pub fn tool_catalog() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "tonglingyu.text.search".to_string(),
            version: TOOL_CATALOG_VERSION.to_string(),
            allowed_profiles: vec!["honglou-text".to_string()],
            effect_scope: "read_only_kb".to_string(),
            input_contract: json!({
                "required": ["question", "limit", "required_evidence_types"],
                "properties": {
                    "question": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1},
                    "required_evidence_types": {
                        "type": "array",
                        "items": {"enum": ["base_text", "commentary", "version_note"]}
                    }
                }
            }),
            output_contract: json!({
                "object": "evidence_cards",
                "required": ["cards", "quality_report"],
                "quality_report_schema": RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION,
                "quality_report_must_include": [
                    "candidate_count",
                    "selected_count",
                    "channel_distribution",
                    "evidence_type_coverage",
                    "exact_match_coverage",
                    "expected_evidence_status",
                    "protected_terms",
                    "expanded_aliases",
                    "source_coverage_boundary",
                    "source_usage_refs",
                    "query_summary",
                    "truncated"
                ],
                "preserves": ["original_text", "source_id", "source_url", "revision_id", "block_id"]
            }),
        },
        ToolDescriptor {
            name: "tonglingyu.commentary.search".to_string(),
            version: TOOL_CATALOG_VERSION.to_string(),
            allowed_profiles: vec!["honglou-commentary".to_string()],
            effect_scope: "read_only_kb".to_string(),
            input_contract: json!({
                "required": ["question", "limit"],
                "properties": {
                    "question": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1}
                }
            }),
            output_contract: json!({
                "object": "evidence_cards",
                "required": ["cards", "quality_report"],
                "required_evidence_type": "commentary",
                "quality_report_schema": RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION
            }),
        },
        ToolDescriptor {
            name: "tonglingyu.evidence.package.create".to_string(),
            version: TOOL_CATALOG_VERSION.to_string(),
            allowed_profiles: vec!["honglou-main".to_string()],
            effect_scope: "runtime_evidence_store_only".to_string(),
            input_contract: json!({
                "required": ["trace_id", "question", "cards"],
                "properties": {
                    "trace_id": {"type": "string"},
                    "question": {"type": "string"},
                    "cards": {"type": "array"}
                }
            }),
            output_contract: json!({"object": "evidence_package"}),
        },
        ToolDescriptor {
            name: "tonglingyu.evidence.package.read".to_string(),
            version: TOOL_CATALOG_VERSION.to_string(),
            allowed_profiles: vec![
                "honglou-main".to_string(),
                "honglou-reviewer".to_string(),
                "gateway-admin-proxy".to_string(),
            ],
            effect_scope: "read_only_runtime_evidence_store".to_string(),
            input_contract: json!({"required": ["package_id"]}),
            output_contract: json!({"object": "evidence_package"}),
        },
        ToolDescriptor {
            name: "tonglingyu.evidence.package.replay".to_string(),
            version: TOOL_CATALOG_VERSION.to_string(),
            allowed_profiles: vec!["gateway-admin-proxy".to_string()],
            effect_scope: "read_only_runtime_evidence_store".to_string(),
            input_contract: json!({"required": ["package_id"]}),
            output_contract: json!({"object": "tonglingyu.evidence_package_replay"}),
        },
    ]
}

pub fn profile_catalog() -> Vec<ProfileDescriptor> {
    vec![
        ProfileDescriptor {
            profile: "honglou-text".to_string(),
            version: PROFILE_CONTRACT_VERSION.to_string(),
            role: "正文、版本、人物和 source snapshot 证据检索 profile。".to_string(),
            allowed_tools: vec!["tonglingyu.text.search".to_string()],
            input_contract: json!({
                "required": ["question", "required_evidence_types", "trace_id"],
                "forbidden": ["system_prompt", "profile_override", "write_tools"]
            }),
            output_contract: json!({
                "required": ["evidence_observation"],
                "evidence_observation_required": ["evidence_refs", "evidence_analysis", "unsupported_scope"],
                "must_preserve": ["original_text", "source_id", "revision_id", "block_id"]
            }),
            safety_contract: json!({
                "no_final_answer": true,
                "no_secret_access": true,
                "no_write_tools": true
            }),
        },
        ProfileDescriptor {
            profile: "honglou-commentary".to_string(),
            version: PROFILE_CONTRACT_VERSION.to_string(),
            role: "脂批、评语和版本线索证据检索 profile。".to_string(),
            allowed_tools: vec!["tonglingyu.commentary.search".to_string()],
            input_contract: json!({
                "required": ["question", "trace_id"],
                "forbidden": ["system_prompt", "profile_override", "write_tools"]
            }),
            output_contract: json!({
                "required": ["evidence_observation"],
                "evidence_observation_required": ["commentary_refs", "commentary_analysis", "scope_notes"],
                "commentary_evidence_rank": "first_class"
            }),
            safety_contract: json!({
                "commentary_can_support_answer": true,
                "no_secret_access": true,
                "no_write_tools": true
            }),
        },
        ProfileDescriptor {
            profile: "honglou-main".to_string(),
            version: PROFILE_CONTRACT_VERSION.to_string(),
            role: "基于证据包组织受限回答的主 profile。".to_string(),
            allowed_tools: vec![
                "tonglingyu.evidence.package.create".to_string(),
                "tonglingyu.evidence.package.read".to_string(),
            ],
            input_contract: json!({
                "required": ["question", "trace_id", "evidence_refs"],
                "forbidden": ["skip_reviewer", "disable_reviewer", "system_prompt"]
            }),
            output_contract: json!({
                "required": [
                    "schema_version",
                    "package_id",
                    "source_scope_policy",
                    "draft_candidate",
                    "coverage_assessment",
                    "evidence_hints",
                    "retrieval_repair",
                    "out_of_scope_hints"
                ],
                "schema_version": UPSTREAM_BUNDLE_SCHEMA_VERSION,
                "package_id_source": "current_evidence_package",
                "source_scope_policy_source": "step_output_json.source_scope_policy",
                "draft_candidate_required": ["draft_answer", "package_id", "claim_statements"],
                "claim_statement_required": ["text", "evidence_refs"],
                "evidence_refs_source": "current_evidence_package_only",
                "must_include": ["support_scope", "unsupported_scope"]
            }),
            safety_contract: json!({
                "must_use_package_ref": true,
                "cannot_finalize_without_reviewer": true,
                "no_secret_access": true
            }),
        },
        ProfileDescriptor {
            profile: "honglou-reviewer".to_string(),
            version: PROFILE_CONTRACT_VERSION.to_string(),
            role: "审校草稿、claim 和证据包边界的 reviewer profile。".to_string(),
            allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
            input_contract: json!({
                "required": ["draft_answer", "package_id", "claim_statements", "trace_id"],
                "forbidden": ["disable_reviewer", "profile_override", "system_prompt"]
            }),
            output_contract: json!({
                "required": ["review_observation"],
                "review_observation_required": ["review_status", "issues", "severity", "required_revisions"],
                "review_status": ["passed", "needs_revision"]
            }),
            safety_contract: json!({
                "cannot_be_disabled_by_user": true,
                "must_downgrade_unsupported_claims": true,
                "no_secret_access": true
            }),
        },
    ]
}

pub fn knowledge_calibration_profile_descriptor() -> ProfileDescriptor {
    ProfileDescriptor {
        profile: KNOWLEDGE_CALIBRATION_PROFILE_ID.to_string(),
        version: KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION.to_string(),
        role: "内部知识治理校准 profile，仅作 evidence judge，不对普通用户暴露。".to_string(),
        allowed_tools: Vec::new(),
        input_contract: json!({
            "required": [
                "item_id",
                "kind",
                "source_refs",
                "evidence_refs",
                "payload_sha256",
                "calibration_method",
                "config_digest"
            ],
            "forbidden": [
                "system_prompt",
                "profile_override",
                "write_tools",
                "secret",
                "api_key",
                "raw_question",
                "unredacted_query"
            ]
        }),
        output_contract: json!({
            "required": [
                "decision",
                "confidence",
                "evidence_refs",
                "source_boundary",
                "quality_issues"
            ],
            "decision": ["system_calibrated", "rejected", "keep_candidate"],
            "cannot_write_fact_layer": true,
            "cannot_promote_runtime_usable": true,
            "must_not_include": ["raw_question", "unredacted_query", "secret", "api_key", "full_prompt"]
        }),
        safety_contract: json!({
            "internal_governance_only": true,
            "hidden_from_openwebui_model_list": true,
            "no_final_answer": true,
            "no_secret_access": true,
            "no_write_tools": true,
            "evidence_judge_only": true
        }),
    }
}

pub fn knowledge_calibration_profile_contract() -> AgentProfileContract {
    let descriptor = knowledge_calibration_profile_descriptor();
    let mut contract = AgentProfileContract::new(descriptor.profile, descriptor.version);
    contract.input_schema = descriptor.input_contract;
    contract.output_schema = descriptor.output_contract;
    contract.tool_policy = agent_runtime_tool_policy(Vec::new());
    contract.max_context_messages = Some(24);
    contract.max_runtime_seconds = Some(agent_runtime_profile_max_runtime_seconds());
    contract.safety_policy = descriptor.safety_contract;
    contract
}

pub fn agent_runtime_profile_contracts(
    profiles: &RuntimeWorkflowProfiles,
) -> Vec<AgentProfileContract> {
    let max_runtime_seconds = agent_runtime_profile_max_runtime_seconds();
    runtime_profile_descriptors(profiles)
        .into_iter()
        .map(|descriptor| {
            let mut contract =
                AgentProfileContract::new(descriptor.profile.clone(), descriptor.version.clone());
            contract.input_schema = agent_runtime_profile_input_schema();
            contract.output_schema = agent_runtime_output_schema();
            contract.tool_policy = agent_runtime_tool_policy(descriptor.allowed_tools.clone());
            contract.max_context_messages = Some(16);
            contract.max_runtime_seconds = Some(max_runtime_seconds);
            contract.safety_policy = json!({
                "deny_message_roles": ["tool"],
                "max_message_bytes": AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES
            });
            contract
        })
        .collect()
}

fn agent_runtime_profile_max_runtime_seconds() -> u64 {
    let generic = env_positive_u64("AGENT_RUNTIME_PROFILE_MAX_SECONDS", 30);
    env_positive_u64("TONGLINGYU_AGENT_RUNTIME_PROFILE_MAX_SECONDS", generic)
}

pub fn agent_runtime_step_plan(input: &AgentRuntimePlanGateInput) -> AgentRuntimeStepPlan {
    let descriptors = runtime_profile_descriptors(&input.profiles)
        .into_iter()
        .map(|descriptor| (descriptor.profile.clone(), descriptor))
        .collect::<BTreeMap<_, _>>();
    let workflow_plan = runtime_workflow_plan(RuntimeWorkflowPlanInput {
        question_type: "agent_runtime_plan_gate".to_string(),
        required_evidence_types: input.required_evidence_types.clone(),
        blocked_controls: Vec::new(),
        profiles: input.profiles.clone(),
    });
    let mut steps = Vec::new();
    let mut evidence_dependencies = Vec::new();
    let mut package_step_id = None;
    let mut draft_step_id = None;
    for plan_step in &workflow_plan.steps {
        let depends_on = match plan_step.operation.as_str() {
            "text_evidence_search" | "commentary_evidence_search" => Vec::new(),
            "evidence_package_create" => evidence_dependencies.clone(),
            "draft_answer" => package_step_id.iter().cloned().collect(),
            "review_answer" => [package_step_id.clone(), draft_step_id.clone()]
                .into_iter()
                .flatten()
                .collect(),
            _ => Vec::new(),
        };
        let mut runtime_step = agent_runtime_step_from_plan_step(
            plan_step,
            depends_on,
            descriptors.get(&plan_step.profile),
        );
        if let Ok(projection) = input.context.projection_for_consumer(&plan_step.profile) {
            runtime_step.input_ref = Some(projection.context_projection_ref.clone());
            runtime_step.metadata["context_contract"] = projection.audit_contract();
            runtime_step.metadata["context_pack_ref"] = json!(&input.context.context_pack_ref);
            runtime_step.metadata["context_pack_digest"] =
                json!(&input.context.context_pack_digest);
        }
        match plan_step.operation.as_str() {
            "text_evidence_search" | "commentary_evidence_search" => {
                evidence_dependencies.push(runtime_step.step_id.clone());
            }
            "evidence_package_create" => {
                package_step_id = Some(runtime_step.step_id.clone());
            }
            "draft_answer" => {
                draft_step_id = Some(runtime_step.step_id.clone());
            }
            _ => {}
        }
        steps.push(runtime_step);
    }

    let mut plan = AgentRuntimeStepPlan::new(input.trace_id.clone(), steps);
    plan.owner = RuntimeStepPlanOwner::DomainGateway;
    plan.metadata = json!({
        "runtime": "tonglingyu",
        "profile_contract_version": PROFILE_CONTRACT_VERSION,
        "context_pack_ref": &input.context.context_pack_ref,
        "context_pack_schema_version": &input.context.context_pack_schema_version,
        "context_pack_digest": &input.context.context_pack_digest,
        "context_projection_count": input.context.projections.len(),
        "question_chars": input.question.chars().count(),
        "question_sha256": hash_text(&input.question),
        "required_evidence_types": &input.required_evidence_types,
        "plan_gate": "agent-runtime-minimal",
    });
    plan
}

pub async fn execute_agent_runtime_plan_gate(
    input: AgentRuntimePlanGateInput,
) -> Result<AgentRuntimePlanGateReport> {
    let validation_plan = runtime_workflow_plan(RuntimeWorkflowPlanInput {
        question_type: "agent_runtime_plan_gate".to_string(),
        required_evidence_types: input.required_evidence_types.clone(),
        blocked_controls: Vec::new(),
        profiles: input.profiles.clone(),
    });
    validate_runtime_context_contract(
        &RuntimeWorkflowInput {
            trace_id: input.trace_id.clone(),
            question: input.question.clone(),
            limit: 1,
            required_evidence_types: input.required_evidence_types.clone(),
            profiles: input.profiles.clone(),
            context: input.context.clone(),
        },
        &validation_plan,
    )?;
    let contracts = agent_runtime_profile_contracts(&input.profiles);
    let requested_tools_by_profile = contracts
        .iter()
        .map(|contract| {
            (
                contract.profile_id.clone(),
                contract.tool_policy.allowed_tools.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let plan = agent_runtime_step_plan(&input);
    let runtime_step_count = plan.steps.len();
    let runtime = MinimalRuntimeClient::default();
    let output = runtime
        .execute_profile_step_plan(AgentRuntimeStepPlanInput {
            plan,
            messages: vec![RuntimeProfileMessage::new(
                "user",
                "tonglingyu runtime plan gate",
            )],
            metadata: json!({
                "runtime": "tonglingyu",
                "plan_gate": "agent-runtime-minimal",
                "question_chars": input.question.chars().count(),
                "question_sha256": hash_text(&input.question),
                "required_evidence_types": &input.required_evidence_types,
            }),
            profile_contracts: contracts.clone(),
            requested_tools_by_profile: requested_tools_by_profile.clone(),
            trace_id: input.trace_id.clone(),
        })
        .await?;
    Ok(AgentRuntimePlanGateReport {
        status: "passed".to_string(),
        trace_id: input.trace_id,
        agent_runtime_client: "minimal".to_string(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        profile_contract_count: contracts.len(),
        runtime_step_count,
        requested_tools_by_profile,
        runtime_step_plan: output
            .metadata
            .get("runtime_step_plan")
            .cloned()
            .unwrap_or_else(|| json!({})),
        runtime_step_outputs: output
            .metadata
            .get("runtime_step_outputs")
            .cloned()
            .unwrap_or_else(|| json!([])),
        agent_runtime_output_ref: output.result_ref,
        effective_tool_set: output
            .metadata
            .get("effective_tool_set")
            .cloned()
            .unwrap_or_else(|| json!([])),
    })
}

fn runtime_profile_descriptors(profiles: &RuntimeWorkflowProfiles) -> Vec<ProfileDescriptor> {
    profile_catalog()
        .into_iter()
        .map(|mut descriptor| {
            descriptor.profile = match descriptor.profile.as_str() {
                "honglou-text" => profiles.text.clone(),
                "honglou-commentary" => profiles.commentary.clone(),
                "honglou-main" => profiles.main.clone(),
                "honglou-reviewer" => profiles.reviewer.clone(),
                _ => descriptor.profile,
            };
            descriptor
        })
        .collect()
}

fn agent_runtime_step_from_plan_step(
    plan_step: &RuntimeWorkflowPlanStep,
    depends_on: Vec<String>,
    descriptor: Option<&ProfileDescriptor>,
) -> AgentRuntimeStep {
    let mut step = AgentRuntimeStep::new(
        plan_step.profile.clone(),
        PROFILE_CONTRACT_VERSION,
        json!({
            "runtime": "tonglingyu",
            "operation": &plan_step.operation,
            "domain_input_contract": descriptor.map(|item| item.input_contract.clone()),
            "domain_output_contract": descriptor.map(|item| item.output_contract.clone()),
            "domain_safety_contract": descriptor.map(|item| item.safety_contract.clone()),
        }),
    );
    step.step_id = plan_step.step_id.clone();
    step.depends_on = depends_on;
    step.tool_policy = agent_runtime_tool_policy(plan_step.allowed_tools.clone());
    step.output_contract = agent_runtime_output_schema();
    step
}

fn agent_runtime_tool_policy(allowed_tools: Vec<String>) -> RuntimeToolPolicy {
    let descriptors = tool_catalog()
        .into_iter()
        .map(|descriptor| (descriptor.name.clone(), descriptor))
        .collect::<BTreeMap<_, _>>();
    let mut policy = RuntimeToolPolicy::read_only(allowed_tools);
    policy.tool_specs = policy
        .allowed_tools
        .iter()
        .map(|tool| {
            descriptors
                .get(tool)
                .map(agent_runtime_tool_spec)
                .unwrap_or_else(|| RuntimeToolSpec::read_only(tool.clone()))
        })
        .collect();
    policy
}

fn agent_runtime_tool_spec(descriptor: &ToolDescriptor) -> RuntimeToolSpec {
    let mut spec = RuntimeToolSpec::read_only(descriptor.name.clone());
    spec.description = format!(
        "Tonglingyu Runtime read-only tool {} ({})",
        descriptor.name, descriptor.effect_scope
    );
    spec.input_schema = descriptor.input_contract.clone();
    spec.output_schema = descriptor.output_contract.clone();
    spec.output_ref_required = true;
    spec
}

fn agent_runtime_profile_input_schema() -> Value {
    json!({
        "type": "object",
        "required": ["kind", "profile_id", "messages", "metadata", "runtime_step", "requested_tools", "trace_id"],
        "properties": {
            "kind": {"enum": ["profile_step"]},
            "profile_id": {"type": "string"},
            "messages": {"type": "array", "minItems": 1},
            "metadata": {"type": "object"},
            "runtime_step": {"type": "object"},
            "requested_tools": {"type": "array"},
            "trace_id": {"type": "string"}
        }
    })
}

fn agent_runtime_output_schema() -> Value {
    json!({
        "type": "object",
        "required": ["result_summary", "result_ref", "metadata"],
        "properties": {
            "result_summary": {"type": "string"},
            "metadata": {"type": "object"}
        }
    })
}

pub fn execute_tool(conn: &Connection, call: TonglingyuToolCall) -> Result<TonglingyuToolOutput> {
    match call {
        TonglingyuToolCall::TextSearch {
            question,
            limit,
            required_evidence_types,
        } => {
            let search = search_evidence_result(conn, &question, limit, &required_evidence_types)?;
            let quality_report = retrieval_quality_report(
                conn,
                "tonglingyu.text.search",
                &question,
                &required_evidence_types,
                &search,
            )?;
            Ok(TonglingyuToolOutput::EvidenceCards {
                cards: search.cards,
                quality_report: Box::new(quality_report),
                tool_version: TOOL_CATALOG_VERSION.to_string(),
            })
        }
        TonglingyuToolCall::CommentarySearch { question, limit } => {
            let required_evidence_types = vec!["commentary".to_string()];
            let search = search_evidence_result(conn, &question, limit, &required_evidence_types)?;
            let quality_report = retrieval_quality_report(
                conn,
                "tonglingyu.commentary.search",
                &question,
                &required_evidence_types,
                &search,
            )?;
            Ok(TonglingyuToolOutput::EvidenceCards {
                cards: search.cards,
                quality_report: Box::new(quality_report),
                tool_version: TOOL_CATALOG_VERSION.to_string(),
            })
        }
        TonglingyuToolCall::EvidencePackageCreate {
            trace_id,
            question,
            cards,
        } => Ok(TonglingyuToolOutput::EvidencePackage {
            package: Box::new(create_evidence_package(conn, &trace_id, &question, cards)?),
            tool_version: TOOL_CATALOG_VERSION.to_string(),
        }),
        TonglingyuToolCall::EvidencePackageRead { package_id } => {
            let package = load_evidence_package_from_conn(conn, &package_id)?.map(Box::new);
            Ok(TonglingyuToolOutput::EvidencePackageRead {
                package,
                tool_version: TOOL_CATALOG_VERSION.to_string(),
            })
        }
        TonglingyuToolCall::EvidencePackageReplay { package_id } => {
            let replay = load_evidence_package_from_conn(conn, &package_id)?
                .map(|package| replay_package_json(&package));
            Ok(TonglingyuToolOutput::EvidencePackageReplay {
                replay,
                tool_version: TOOL_CATALOG_VERSION.to_string(),
            })
        }
    }
}

pub fn execute_runtime_workflow(
    conn: &Connection,
    input: RuntimeWorkflowInput,
) -> Result<RuntimeWorkflowOutput> {
    if input.limit == 0 {
        return Err(anyhow!("runtime workflow limit must be greater than 0"));
    }
    let workflow_plan = runtime_workflow_plan(RuntimeWorkflowPlanInput {
        question_type: "runtime_workflow".to_string(),
        required_evidence_types: input.required_evidence_types.clone(),
        blocked_controls: Vec::new(),
        profiles: input.profiles.clone(),
    });
    validate_runtime_context_contract(&input, &workflow_plan)?;
    validate_runtime_context_pack_digest(conn, &input.context)?;
    let mut steps = Vec::new();
    let mut cards = Vec::new();
    let mut retrieval_failure_candidates = Vec::<(RetrievalQualityReport, Vec<String>)>::new();
    let text_required_types = text_search_required_evidence_types(&input.required_evidence_types);
    let text_started = Instant::now();
    let (text_cards, text_quality_report) = match execute_tool(
        conn,
        TonglingyuToolCall::TextSearch {
            question: input.question.clone(),
            limit: input.limit,
            required_evidence_types: text_required_types,
        },
    )? {
        TonglingyuToolOutput::EvidenceCards {
            cards,
            quality_report,
            ..
        } => (cards, *quality_report),
        other => return Err(anyhow!("unexpected runtime tool output: {:?}", other)),
    };
    retrieval_failure_candidates.push((text_quality_report.clone(), evidence_ids(&text_cards)));
    cards = merge_cards(cards, text_cards.clone());
    let text_plan_step = workflow_plan_step(&workflow_plan, "text_evidence_search")?;
    steps.push(workflow_step_report(
        conn,
        WorkflowStepReportInput {
            trace_id: &input.trace_id,
            step_id: &text_plan_step.step_id,
            profile: &text_plan_step.profile,
            operation: &text_plan_step.operation,
            required: text_plan_step.required,
            allowed_tools: text_plan_step.allowed_tools.clone(),
            tool_calls: text_plan_step.allowed_tools.clone(),
            input_ref: None,
            duration_ms: elapsed_ms(text_started),
            output: json!({
            "object": "tonglingyu.text.evidence_analysis",
            "card_count": text_cards.len(),
            "evidence_ids": evidence_ids(&text_cards),
            "evidence_types": evidence_types(&text_cards),
            "quality_report": &text_quality_report,
            }),
            context: &input.context,
        },
    )?);

    if input
        .required_evidence_types
        .iter()
        .any(|item| item == "commentary")
    {
        let commentary_started = Instant::now();
        let (commentary_cards, commentary_quality_report) = match execute_tool(
            conn,
            TonglingyuToolCall::CommentarySearch {
                question: input.question.clone(),
                limit: input.limit,
            },
        )? {
            TonglingyuToolOutput::EvidenceCards {
                cards,
                quality_report,
                ..
            } => (cards, *quality_report),
            other => return Err(anyhow!("unexpected runtime tool output: {:?}", other)),
        };
        retrieval_failure_candidates.push((
            commentary_quality_report.clone(),
            evidence_ids(&commentary_cards),
        ));
        cards = merge_cards(cards, commentary_cards.clone());
        let commentary_plan_step =
            workflow_plan_step(&workflow_plan, "commentary_evidence_search")?;
        steps.push(workflow_step_report(
            conn,
            WorkflowStepReportInput {
                trace_id: &input.trace_id,
                step_id: &commentary_plan_step.step_id,
                profile: &commentary_plan_step.profile,
                operation: &commentary_plan_step.operation,
                required: commentary_plan_step.required,
                allowed_tools: commentary_plan_step.allowed_tools.clone(),
                tool_calls: commentary_plan_step.allowed_tools.clone(),
                input_ref: None,
                duration_ms: elapsed_ms(commentary_started),
                output: json!({
                "object": "tonglingyu.commentary.evidence_analysis",
                "card_count": commentary_cards.len(),
                "evidence_ids": evidence_ids(&commentary_cards),
                "evidence_types": evidence_types(&commentary_cards),
                "scope_notes": "commentary is first-class evidence within the default pre-80 scope; later-forty material still requires explicit scope",
                "quality_report": &commentary_quality_report,
            }),
                context: &input.context,
            },
        )?);
    }

    let source_scope_filter = filter_cards_for_source_scope(&input.question, cards);
    let source_scope_report = source_scope_filter.report;
    let package_started = Instant::now();
    let package = match execute_tool(
        conn,
        TonglingyuToolCall::EvidencePackageCreate {
            trace_id: input.trace_id.clone(),
            question: input.question.clone(),
            cards: source_scope_filter.included_cards,
        },
    )? {
        TonglingyuToolOutput::EvidencePackage { package, .. } => *package,
        other => return Err(anyhow!("unexpected runtime tool output: {:?}", other)),
    };
    for (quality_report, selected_evidence_ids) in retrieval_failure_candidates {
        record_retrieval_failure_if_needed(
            conn,
            &input.trace_id,
            &package.package_id,
            &input.question,
            quality_report,
            selected_evidence_ids,
        )?;
    }
    record_reviewer_failure_if_needed(conn, &input, &package)?;
    let package_plan_step = workflow_plan_step(&workflow_plan, "evidence_package_create")?;
    let package_step_id = package_plan_step.step_id.clone();
    let package_output_ref = workflow_output_ref(&input.trace_id, &package_step_id);
    steps.push(workflow_step_report(
        conn,
        WorkflowStepReportInput {
            trace_id: &input.trace_id,
            step_id: &package_step_id,
            profile: &package_plan_step.profile,
            operation: &package_plan_step.operation,
            required: package_plan_step.required,
            allowed_tools: package_plan_step.allowed_tools.clone(),
            tool_calls: package_plan_step.allowed_tools.clone(),
            input_ref: None,
            duration_ms: elapsed_ms(package_started),
            output: json!({
            "object": "tonglingyu.evidence.package_ref",
            "package_id": &package.package_id,
            "card_count": package.cards.len(),
            "claim_count": package.claims.len(),
            "review_status": &package.review.status,
            "source_scope_policy": &source_scope_report.policy,
            "out_of_scope_hints": &source_scope_report.out_of_scope_hints,
            }),
            context: &input.context,
        },
    )?);
    let draft_started = Instant::now();
    let draft_answer = local_answer(&input.question, &package);
    let count_question = question_asks_for_count(&input.question)?;
    let draft_plan_step = workflow_plan_step(&workflow_plan, "draft_answer")?;
    let draft_step_id = draft_plan_step.step_id.clone();
    steps.push(workflow_step_report(
        conn,
        WorkflowStepReportInput {
            trace_id: &input.trace_id,
            step_id: &draft_step_id,
            profile: &draft_plan_step.profile,
            operation: &draft_plan_step.operation,
            required: draft_plan_step.required,
            allowed_tools: draft_plan_step.allowed_tools.clone(),
            tool_calls: draft_plan_step.allowed_tools.clone(),
            input_ref: Some(package_output_ref.clone()),
            duration_ms: elapsed_ms(draft_started),
            output: json!({
            "object": "tonglingyu.draft_answer",
            "package_id": &package.package_id,
            "evidence_ids": evidence_ids(&package.cards),
            "evidence_brief": upstream_evidence_brief(&input.question, &package.cards),
            "evidence_slot_count_policy": evidence_slot_count_context_value(
                &input.question,
                &package.cards,
                count_question,
            )?,
            "claim_statements": &package.claims,
            "answer_source": "runtime_local_profile",
            "source_scope_policy": &source_scope_report.policy,
            "out_of_scope_hints": &source_scope_report.out_of_scope_hints,
            }),
            context: &input.context,
        },
    )?);
    let review_started = Instant::now();
    let final_answer = enforce_review(draft_answer.clone(), &package);
    let review_plan_step = workflow_plan_step(&workflow_plan, "review_answer")?;
    let review_step_id = review_plan_step.step_id.clone();
    steps.push(workflow_step_report(
        conn,
        WorkflowStepReportInput {
            trace_id: &input.trace_id,
            step_id: &review_step_id,
            profile: &review_plan_step.profile,
            operation: &review_plan_step.operation,
            required: review_plan_step.required,
            allowed_tools: review_plan_step.allowed_tools.clone(),
            tool_calls: review_plan_step.allowed_tools.clone(),
            input_ref: Some(package_output_ref),
            duration_ms: elapsed_ms(review_started),
            output: json!({
            "object": "tonglingyu.review_result",
            "package_id": &package.package_id,
            "draft_consumed": true,
            "claim_statements": &package.claims,
            "review": &package.review,
            "revision_applied": package.review.status != "passed",
            }),
            context: &input.context,
        },
    )?);
    let stream_events = workflow_stream_events(
        &input.trace_id,
        &input.profiles.main,
        &package.package_id,
        &final_answer,
        &steps,
    );
    Ok(RuntimeWorkflowOutput {
        trace_id: input.trace_id,
        question: input.question,
        package,
        draft_answer,
        final_answer,
        answer_source: "runtime_local_profile".to_string(),
        agent_runtime_summary: deterministic_agent_runtime_summary(steps.len()),
        steps,
        stream_events,
    })
}

async fn attach_agent_runtime_step_execution(
    workflow: &mut RuntimeWorkflowOutput,
    profiles: &RuntimeWorkflowProfiles,
    context: &RuntimeContextContract,
    mode: TonglingyuAgentRuntimeMode,
    runtime: Arc<dyn RuntimeClient>,
) -> Result<()> {
    let profile_contracts = agent_runtime_profile_contracts(profiles);
    let contracts = profile_contracts
        .into_iter()
        .map(|contract| (contract.profile_id.clone(), contract))
        .collect::<BTreeMap<_, _>>();
    let trace_id = workflow.trace_id.clone();
    let mut executions = Vec::with_capacity(workflow.steps.len());
    for (index, step) in workflow.steps.iter().cloned().enumerate() {
        let result_summary_contract = agent_runtime_result_summary_contract(&step);
        let contract = contracts
            .get(&step.profile)
            .cloned()
            .ok_or_else(|| anyhow!("runtime profile contract missing for {}", step.profile))?;
        let projection = context.projection_for_consumer(&step.profile)?.clone();
        executions.push(execute_agent_runtime_profile_step(
            index,
            step,
            trace_id.clone(),
            result_summary_contract.to_owned(),
            contract,
            projection,
            context.clone(),
            mode,
            runtime.clone(),
        ));
    }
    for execution in try_join_all(executions).await? {
        workflow.steps[execution.index].agent_runtime = Some(execution.agent_runtime);
    }
    Ok(())
}

struct AgentRuntimeStepExecution {
    index: usize,
    agent_runtime: Value,
}

struct AgentRuntimeProfileStepRun {
    output: RuntimeOutput,
    attempt_count: usize,
    retry_error_count: usize,
}

#[allow(clippy::too_many_arguments)]
async fn execute_agent_runtime_profile_step(
    index: usize,
    step: RuntimeWorkflowStepReport,
    trace_id: String,
    result_summary_contract: String,
    contract: AgentProfileContract,
    projection: RuntimeContextProjection,
    context: RuntimeContextContract,
    mode: TonglingyuAgentRuntimeMode,
    runtime: Arc<dyn RuntimeClient>,
) -> Result<AgentRuntimeStepExecution> {
    let runtime_step = agent_runtime_step_from_workflow_step(&step);
    let visible_question = projection
        .projection_payload
        .get("visible_question")
        .and_then(Value::as_str)
        .unwrap_or("");
    let message =
        agent_runtime_profile_step_message(&trace_id, &step, &projection, &result_summary_contract);
    let message_bytes = message.content.len();
    if message_bytes > AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES {
        return Err(anyhow!(
            "runtime profile message exceeded safety budget: step_id={} operation={} bytes={} limit={}",
            step.step_id,
            step.operation,
            message_bytes,
            AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES
        ));
    }
    let execution = execute_agent_runtime_profile_step_with_retry(
        &step,
        &trace_id,
        &message,
        &result_summary_contract,
        &contract,
        &runtime_step,
        &projection,
        &context,
        visible_question,
        message_bytes,
        runtime,
    )
    .await?;
    agent_runtime_step_execution_from_run(index, mode, &step, result_summary_contract, execution)
}

fn agent_runtime_step_execution_from_run(
    index: usize,
    mode: TonglingyuAgentRuntimeMode,
    step: &RuntimeWorkflowStepReport,
    result_summary_contract: String,
    execution: AgentRuntimeProfileStepRun,
) -> Result<AgentRuntimeStepExecution> {
    let output = execution.output;
    let mut tool_results = output
        .metadata
        .get("tool_results")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut tool_audit_events = output
        .metadata
        .get("tool_audit_events")
        .cloned()
        .unwrap_or_else(|| json!([]));
    host_enforce_missing_required_tool_results(
        mode,
        step,
        &mut tool_results,
        &mut tool_audit_events,
    )?;
    validate_agent_runtime_required_tools(mode, step, &tool_results)?;
    let tool_result_count = tool_results.as_array().map_or(0, Vec::len);
    let tool_audit_event_count = tool_audit_events.as_array().map_or(0, Vec::len);
    Ok(AgentRuntimeStepExecution {
        index,
        agent_runtime: json!({
            "client": mode.as_str(),
            "status": "executed",
            "content_source": "tonglingyu-deterministic-workflow",
            "executor_output_source": format!("agent-runtime-{}", mode.as_str()),
            "content_used_for_final_answer": false,
            "result_summary_contract": result_summary_contract,
            "result_ref": output.result_ref,
            "result_summary": output.result_summary,
            "tool_rounds": output
                .metadata
                .get("tool_rounds")
                .cloned()
                .unwrap_or(Value::Null),
            "tool_result_count": tool_result_count,
            "tool_audit_event_count": tool_audit_event_count,
            "tool_results": tool_results,
            "tool_audit_events": tool_audit_events,
            "schema_version": output
                .metadata
                .get("schema_version")
                .cloned()
                .unwrap_or(Value::Null),
            "provider_request": output
                .metadata
                .get("provider_request")
                .cloned()
                .unwrap_or(Value::Null),
            "attempt_count": execution.attempt_count,
            "retry_error_count": execution.retry_error_count,
            "max_attempts": AGENT_RUNTIME_PROFILE_STEP_MAX_ATTEMPTS,
            "provider_request_sha256": output
                .metadata
                .get("provider_request_sha256")
                .cloned()
                .unwrap_or(Value::Null),
            "provider_request_embedded": output
                .metadata
                .get("provider_request_embedded")
                .cloned()
                .unwrap_or(Value::Null),
            "effective_tool_set": output
                .metadata
                .get("effective_tool_set")
                .cloned()
                .unwrap_or_else(|| json!([])),
            "runtime_step": output
                .metadata
                .get("runtime_step")
                .cloned()
                .unwrap_or_else(|| json!({})),
        }),
    })
}

#[allow(clippy::too_many_arguments)]
async fn execute_agent_runtime_profile_step_with_retry(
    step: &RuntimeWorkflowStepReport,
    trace_id: &str,
    message: &RuntimeProfileMessage,
    result_summary_contract: &str,
    contract: &AgentProfileContract,
    runtime_step: &AgentRuntimeStep,
    projection: &RuntimeContextProjection,
    context: &RuntimeContextContract,
    visible_question: &str,
    message_bytes: usize,
    runtime: Arc<dyn RuntimeClient>,
) -> Result<AgentRuntimeProfileStepRun> {
    let mut attempt_count = 1;
    let mut retry_error_count = 0;
    loop {
        let input = runtime_profile_input_for_attempt(
            step,
            trace_id,
            message,
            result_summary_contract,
            contract,
            runtime_step,
            projection,
            context,
            visible_question,
            message_bytes,
            attempt_count,
        );
        match runtime.execute_profile_step(input).await {
            Ok(output) => {
                return Ok(AgentRuntimeProfileStepRun {
                    output,
                    attempt_count,
                    retry_error_count,
                });
            }
            Err(error) if attempt_count < AGENT_RUNTIME_PROFILE_STEP_MAX_ATTEMPTS => {
                retry_error_count += 1;
                attempt_count += 1;
                let _ = error;
            }
            Err(error) => {
                let error_text = error.to_string();
                return Err(anyhow::Error::new(error).context(format!(
                    "runtime profile step failed after {} attempts: step_id={} operation={} error={}",
                    attempt_count, step.step_id, step.operation, error_text
                )));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn runtime_profile_input_for_attempt(
    step: &RuntimeWorkflowStepReport,
    trace_id: &str,
    message: &RuntimeProfileMessage,
    result_summary_contract: &str,
    contract: &AgentProfileContract,
    runtime_step: &AgentRuntimeStep,
    projection: &RuntimeContextProjection,
    context: &RuntimeContextContract,
    visible_question: &str,
    message_bytes: usize,
    attempt: usize,
) -> RuntimeProfileInput {
    RuntimeProfileInput {
        profile_id: step.profile.clone(),
        messages: vec![message.clone()],
        metadata: json!({
            "runtime": "tonglingyu",
            "workflow_step_id": &step.step_id,
            "operation": &step.operation,
            "input_ref": &step.input_ref,
            "output_ref": &step.output_ref,
            "context_pack_ref": &context.context_pack_ref,
            "context_pack_schema_version": &context.context_pack_schema_version,
            "context_pack_digest": &context.context_pack_digest,
            "context_projection": projection.audit_contract(),
            "step_output": &step.output,
            "result_summary_contract": result_summary_contract,
            "question_chars": visible_question.chars().count(),
            "question_sha256": hash_text(visible_question),
            "message_bytes": message_bytes,
            "message_max_bytes": AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES,
            "content_source": "tonglingyu-deterministic-workflow",
            "attempt": attempt,
            "max_attempts": AGENT_RUNTIME_PROFILE_STEP_MAX_ATTEMPTS,
        }),
        profile_contract: Some(contract.clone()),
        runtime_step: Some(runtime_step.clone()),
        requested_tools: step.allowed_tools.clone(),
        trace_id: trace_id.to_string(),
    }
}

fn host_enforce_missing_required_tool_results(
    mode: TonglingyuAgentRuntimeMode,
    step: &RuntimeWorkflowStepReport,
    tool_results: &mut Value,
    tool_audit_events: &mut Value,
) -> Result<()> {
    if mode != TonglingyuAgentRuntimeMode::Hermes || !step.required || step.allowed_tools.is_empty()
    {
        return Ok(());
    }

    if !tool_results.is_array() {
        *tool_results = json!([]);
    }
    if !tool_audit_events.is_array() {
        *tool_audit_events = json!([]);
    }

    let executed_tools = tool_results
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("tool_name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    let missing_tools = step
        .allowed_tools
        .iter()
        .filter(|tool| !executed_tools.contains(tool.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if missing_tools.is_empty() {
        return Ok(());
    }

    let descriptors = tool_catalog()
        .into_iter()
        .map(|descriptor| (descriptor.name.clone(), descriptor))
        .collect::<BTreeMap<_, _>>();
    for tool_name in missing_tools {
        let output_ref = host_enforced_tool_output_ref(step, &tool_name)?;
        let output_schema = descriptors
            .get(&tool_name)
            .map(|descriptor| descriptor.output_contract.clone())
            .unwrap_or_else(|| json!({}));
        let call_id = format!(
            "host-required-{}-{}",
            step.step_id,
            tool_name.replace('.', "-")
        );
        let result = json!({
            "call_id": &call_id,
            "profile_id": &step.profile,
            "tool_name": &tool_name,
            "output_schema": &output_schema,
            "output_ref": &output_ref,
            "output_summary": summarize_runtime_step_output(&step.output),
            "trace_id": &step.trace_id,
            "host_enforced": true,
            "source": "tonglingyu-deterministic-workflow",
        });
        let call_event = json!({
            "event": "runtime_tool_call",
            "call_id": &call_id,
            "call_id_status": "validated",
            "profile_id": &step.profile,
            "tool_name": &tool_name,
            "tool_name_status": "validated",
            "trace_id": &step.trace_id,
            "host_enforced": true,
            "source": "tonglingyu-deterministic-workflow",
        });
        let result_event = json!({
            "event": "runtime_tool_result",
            "call_id": &call_id,
            "profile_id": &step.profile,
            "tool_name": &tool_name,
            "output_schema": &output_schema,
            "output_ref": &output_ref,
            "output_summary": summarize_runtime_step_output(&step.output),
            "trace_id": &step.trace_id,
            "host_enforced": true,
            "source": "tonglingyu-deterministic-workflow",
        });
        if let Some(items) = tool_results.as_array_mut() {
            items.push(result);
        }
        if let Some(items) = tool_audit_events.as_array_mut() {
            items.push(call_event);
            items.push(result_event);
        }
    }
    Ok(())
}

fn host_enforced_tool_output_ref(
    step: &RuntimeWorkflowStepReport,
    tool_name: &str,
) -> Result<String> {
    if matches!(
        tool_name,
        "tonglingyu.text.search" | "tonglingyu.commentary.search"
    ) {
        return evidence_tool_expected_output_ref(step);
    }
    if matches!(
        tool_name,
        "tonglingyu.evidence.package.create"
            | "tonglingyu.evidence.package.read"
            | "tonglingyu.evidence.package.replay"
    ) {
        let Some(package_id) = step
            .output
            .get("package_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        else {
            return Err(anyhow!(
                "Hermes runtime step {} ({}) package tool {} cannot be host-enforced without package_id",
                step.step_id,
                step.operation,
                tool_name
            ));
        };
        return Ok(format!(
            "runtime://tonglingyu/{}/packages/{package_id}",
            step.trace_id
        ));
    }
    Ok(format!(
        "runtime://tonglingyu/{}/tools/host-required-{}-{}",
        step.trace_id,
        step.step_id,
        tool_name.replace('.', "-")
    ))
}

fn summarize_runtime_step_output(value: &Value) -> String {
    match value {
        Value::Object(map) => format!("object_keys_len:{}", map.len()),
        Value::Array(items) => format!("array_len:{}", items.len()),
        Value::String(value) => format!("string_len:{}", value.chars().count()),
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(_) => "number".to_string(),
    }
}

fn validate_agent_runtime_required_tools(
    mode: TonglingyuAgentRuntimeMode,
    step: &RuntimeWorkflowStepReport,
    tool_results: &Value,
) -> Result<()> {
    if mode != TonglingyuAgentRuntimeMode::Hermes || !step.required || step.allowed_tools.is_empty()
    {
        return Ok(());
    }
    let executed_tools = tool_results
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("tool_name").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let missing_tools = step
        .allowed_tools
        .iter()
        .filter(|tool| !executed_tools.contains(tool.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_tools.is_empty() {
        return Err(anyhow!(
            "Hermes runtime step {} ({}) did not execute required tool(s): {}",
            step.step_id,
            step.operation,
            missing_tools.join(",")
        ));
    }

    let result_items = tool_results
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    for tool_name in &step.allowed_tools {
        let Some(result) = result_items
            .iter()
            .find(|item| item.get("tool_name").and_then(Value::as_str) == Some(tool_name.as_str()))
        else {
            continue;
        };
        let Some(output_ref) = result.get("output_ref").and_then(Value::as_str) else {
            return Err(anyhow!(
                "Hermes runtime step {} ({}) required tool {} did not return output_ref",
                step.step_id,
                step.operation,
                tool_name
            ));
        };
        validate_agent_runtime_tool_output_ref(step, tool_name, output_ref)?;
    }
    Ok(())
}

fn validate_agent_runtime_tool_output_ref(
    step: &RuntimeWorkflowStepReport,
    tool_name: &str,
    output_ref: &str,
) -> Result<()> {
    let trace_prefix = format!("runtime://tonglingyu/{}/", step.trace_id);
    if !output_ref.starts_with(&trace_prefix) {
        return Err(anyhow!(
            "Hermes runtime step {} ({}) tool {} returned invalid output_ref",
            step.step_id,
            step.operation,
            tool_name
        ));
    }
    if matches!(
        tool_name,
        "tonglingyu.text.search" | "tonglingyu.commentary.search"
    ) {
        let expected_ref = evidence_tool_expected_output_ref(step)?;
        if output_ref != expected_ref {
            return Err(anyhow!(
                "Hermes runtime step {} ({}) evidence tool {} returned mismatched output_ref",
                step.step_id,
                step.operation,
                tool_name
            ));
        }
        return Ok(());
    }
    if !matches!(
        tool_name,
        "tonglingyu.evidence.package.create"
            | "tonglingyu.evidence.package.read"
            | "tonglingyu.evidence.package.replay"
    ) {
        return Ok(());
    }
    let Some(package_id) = step
        .output
        .get("package_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Err(anyhow!(
            "Hermes runtime step {} ({}) package tool {} cannot be bound without package_id",
            step.step_id,
            step.operation,
            tool_name
        ));
    };
    let expected_ref = format!(
        "runtime://tonglingyu/{}/packages/{package_id}",
        step.trace_id
    );
    if output_ref != expected_ref {
        return Err(anyhow!(
            "Hermes runtime step {} ({}) package tool {} returned mismatched output_ref",
            step.step_id,
            step.operation,
            tool_name
        ));
    }
    Ok(())
}

fn evidence_tool_expected_output_ref(step: &RuntimeWorkflowStepReport) -> Result<String> {
    let evidence_ids = step
        .output
        .get("evidence_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow!(
                "Hermes runtime step {} ({}) evidence tool cannot be bound without evidence_ids",
                step.step_id,
                step.operation
            )
        })?
        .iter()
        .map(|value| {
            value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                anyhow!(
                    "Hermes runtime step {} ({}) evidence_ids must be strings",
                    step.step_id,
                    step.operation
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(evidence_set_output_ref(&step.trace_id, &evidence_ids))
}

fn tonglingyu_agent_runtime_client(
    mode: TonglingyuAgentRuntimeMode,
    store: TonglingyuRuntimeStore,
    registry: RuntimeProfileRegistry,
    profiles: &RuntimeWorkflowProfiles,
) -> Result<Arc<dyn RuntimeClient>> {
    let runtime_profile_ids = [
        profiles.text.as_str(),
        profiles.commentary.as_str(),
        profiles.main.as_str(),
        profiles.reviewer.as_str(),
    ];
    match mode {
        TonglingyuAgentRuntimeMode::Minimal => Ok(Arc::new(
            MinimalRuntimeClient::default().with_profile_registry(registry),
        )),
        TonglingyuAgentRuntimeMode::Hermes => {
            let client = match workflow_agent_provider_profile_from_env()? {
                Some(profile) => HermesRuntimeClient::new(hermes_config_from_provider_profile(
                    &profile,
                    &runtime_profile_ids,
                )?)?,
                None => HermesRuntimeClient::from_env()?,
            };
            Ok(Arc::new(
                client
                    .with_profile_registry(registry)
                    .with_tool_executor(Arc::new(TonglingyuRuntimeToolExecutor::new(store))),
            ))
        }
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork => {
            let client = match workflow_agent_provider_profile_from_env()? {
                Some(profile) => OpenAiCompatibleNetworkRuntimeClient::new(
                    openai_compatible_config_from_provider_profile(&profile, &runtime_profile_ids)?,
                )?,
                None => OpenAiCompatibleNetworkRuntimeClient::from_env()?,
            };
            Ok(Arc::new(client.with_profile_registry(registry)))
        }
    }
}

fn hermes_config_from_provider_profile(
    profile: &str,
    runtime_profiles: &[&str],
) -> Result<HermesRuntimeConfig> {
    let base_url = required_agent_provider_env(profile, "BASE_URL")?;
    let model = required_agent_provider_env(profile, "MODEL")?;
    let api_key_env = required_agent_provider_env(profile, "API_KEY_ENV")?;
    let api_key = env_nonempty(&api_key_env)
        .ok_or_else(|| anyhow!("{api_key_env} must be configured for {profile}"))?;
    let mut config = HermesRuntimeConfig::new(base_url, model.clone());
    config.api_key = Some(api_key);
    config.profile_models = runtime_profiles
        .iter()
        .map(|runtime_profile| ((*runtime_profile).to_string(), model.clone()))
        .collect();
    Ok(config)
}

fn openai_compatible_config_from_provider_profile(
    profile: &str,
    runtime_profiles: &[&str],
) -> Result<OpenAiCompatibleNetworkRuntimeConfig> {
    openai_compatible_config_from_provider_profile_source(profile, runtime_profiles, &env_nonempty)
}

fn openai_compatible_config_from_provider_profile_source(
    profile: &str,
    runtime_profiles: &[&str],
    get_env: &dyn Fn(&str) -> Option<String>,
) -> Result<OpenAiCompatibleNetworkRuntimeConfig> {
    let base_url = required_agent_provider_env_from(profile, "BASE_URL", get_env)?;
    let model = required_agent_provider_env_from(profile, "MODEL", get_env)?;
    let api_key_env = required_agent_provider_env_from(profile, "API_KEY_ENV", get_env)?;
    let api_key = get_env(&api_key_env)
        .ok_or_else(|| anyhow!("{api_key_env} must be configured for {profile}"))?;
    let mut config = OpenAiCompatibleNetworkRuntimeConfig::new(base_url, model.clone());
    config.api_key = Some(api_key);
    config.profile_models = runtime_profiles
        .iter()
        .map(|runtime_profile| ((*runtime_profile).to_string(), model.clone()))
        .collect();
    config.reasoning_split =
        optional_true_env_from("AGENT_RUNTIME_OPENAI_REASONING_SPLIT", get_env);
    Ok(config)
}

fn optional_true_env_from(name: &str, get_env: &dyn Fn(&str) -> Option<String>) -> Option<bool> {
    match get_env(name)?.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        _ => None,
    }
}

fn agent_runtime_profile_step_message(
    trace_id: &str,
    step: &RuntimeWorkflowStepReport,
    projection: &RuntimeContextProjection,
    result_summary_contract: &str,
) -> RuntimeProfileMessage {
    let max_compaction_level = if step.operation == "draft_answer" {
        3
    } else {
        0
    };
    let mut selected_content = String::new();
    for compaction_level in 0..=max_compaction_level {
        selected_content = agent_runtime_profile_step_message_content(
            trace_id,
            step,
            projection,
            result_summary_contract,
            compaction_level,
        );
        if selected_content.len() <= AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES {
            break;
        }
    }
    RuntimeProfileMessage::new("user", selected_content)
}

fn agent_runtime_profile_step_message_content(
    trace_id: &str,
    step: &RuntimeWorkflowStepReport,
    projection: &RuntimeContextProjection,
    result_summary_contract: &str,
    compaction_level: usize,
) -> String {
    let step_output = step_output_message_payload_with_compaction(step, compaction_level);
    format!(
        concat!(
            "Tonglingyu profile step execution context.\n",
            "Output rule: return exactly one non-empty JSON object as assistant content. Do not return an empty assistant message. Do not use markdown.\n",
            "trace_id: {trace_id}\n",
            "profile: {profile}\n",
            "operation: {operation}\n",
            "context_projection_ref: {context_projection_ref}\n",
            "context_projection_digest: {context_projection_digest}\n",
            "context_projection_payload_json: {context_projection_payload}\n",
            "input_ref: {input_ref}\n",
            "output_ref: {output_ref}\n",
            "allowed_tools: {allowed_tools}\n",
            "result_summary_contract: {result_summary_contract}\n",
            "step_output_json: {step_output}\n"
        ),
        trace_id = trace_id,
        profile = &step.profile,
        operation = &step.operation,
        context_projection_ref = &projection.context_projection_ref,
        context_projection_digest = &projection.context_projection_digest,
        context_projection_payload =
            serde_json::to_string(&context_projection_message_payload(projection))
                .unwrap_or_else(|_| "{}".to_string()),
        input_ref = step.input_ref.as_deref().unwrap_or("none"),
        output_ref = &step.output_ref,
        allowed_tools = step.allowed_tools.join(","),
        result_summary_contract = result_summary_contract,
        step_output = serde_json::to_string(&step_output).unwrap_or_else(|_| "{}".to_string()),
    )
}

fn context_projection_message_payload(projection: &RuntimeContextProjection) -> Value {
    let payload = &projection.projection_payload;
    let resolver = payload.get("resolver").unwrap_or(&Value::Null);
    json!({
        "object": "tonglingyu.context_projection_message_payload",
        "context_projection_ref": &projection.context_projection_ref,
        "context_projection_digest": &projection.context_projection_digest,
        "projection_payload_sha256": hash_json(payload),
        "consumer_name": &projection.consumer_name,
        "visible_question": json_trimmed_string(payload, "visible_question", 512),
        "resolved_question": resolver
            .get("resolved_question")
            .and_then(Value::as_str)
            .map(|value| trim_text(value, 512)),
        "session_summary": json_trimmed_string(payload, "session_summary", 512),
        "memory_usage_summary": payload
            .get("memory_usage_summary")
            .cloned()
            .unwrap_or(Value::Null),
        "resolver": json!({
            "strategy": resolver.get("strategy").cloned().unwrap_or(Value::Null),
            "needs_clarification": resolver
                .get("needs_clarification")
                .cloned()
                .unwrap_or(Value::Null),
            "referent_bindings": resolver
                .get("referent_bindings")
                .cloned()
                .unwrap_or_else(|| json!([])),
            "used_context_refs": resolver
                .get("used_context_refs")
                .cloned()
                .unwrap_or_else(|| json!([])),
        }),
        "tool_policy_digest": &projection.tool_policy_digest,
    })
}

fn json_trimmed_string(value: &Value, key: &str, max_chars: usize) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|item| trim_text(item, max_chars))
}

fn compact_draft_evidence_brief_for_message(
    evidence_brief: Value,
    compaction_level: usize,
) -> Value {
    if compaction_level == 0 {
        return evidence_brief;
    }
    let (card_limit, text_chars, source_title_chars, scope_chars, term_limit, term_chars) =
        match compaction_level {
            1 => (5, 96, 64, 56, 3, 14),
            2 => (4, 72, 48, 40, 2, 12),
            _ => (3, 40, 32, 0, 0, 0),
        };
    let Some(items) = evidence_brief.as_array() else {
        return json!([]);
    };
    Value::Array(
        items
            .iter()
            .take(card_limit)
            .filter_map(Value::as_object)
            .map(|item| {
                let mut compact = Map::new();
                insert_existing_value(&mut compact, item, "evidence_id");
                insert_existing_value(&mut compact, item, "evidence_type");
                insert_existing_value(&mut compact, item, "source_layer");
                insert_trimmed_string(&mut compact, item, "source_title", source_title_chars);
                insert_trimmed_string(&mut compact, item, "text", text_chars);
                insert_existing_value(&mut compact, item, "evidence_slots");
                compact.insert(
                    "evidence_slot_rules".to_string(),
                    compact_evidence_slot_rules_for_message(
                        item.get("evidence_slot_rules").unwrap_or(&Value::Null),
                        compaction_level,
                    ),
                );
                if term_limit > 0 {
                    compact.insert(
                        "matched_terms".to_string(),
                        compact_string_array_for_message(
                            item.get("matched_terms").unwrap_or(&Value::Null),
                            term_limit,
                            term_chars,
                        ),
                    );
                }
                if scope_chars > 0 {
                    insert_trimmed_string(&mut compact, item, "support_scope", scope_chars);
                    insert_trimmed_string(&mut compact, item, "unsupported_scope", scope_chars);
                }
                Value::Object(compact)
            })
            .collect(),
    )
}

fn compact_evidence_slot_count_policy_for_message(
    evidence_slot_count_policy: Value,
    compaction_level: usize,
) -> Value {
    if compaction_level == 0 || !evidence_slot_count_policy.is_object() {
        return evidence_slot_count_policy;
    }
    let active_basis = evidence_slot_count_policy
        .get("active_count_basis")
        .and_then(Value::as_object);
    let mut compact_basis = Map::new();
    if let Some(active_basis) = active_basis {
        insert_existing_value(&mut compact_basis, active_basis, "id");
        insert_existing_value(&mut compact_basis, active_basis, "label");
        insert_existing_value(&mut compact_basis, active_basis, "answer_unit");
        insert_existing_value(&mut compact_basis, active_basis, "answer_noun");
    }
    json!({
        "active_count_basis": Value::Object(compact_basis),
        "count_question": evidence_slot_count_policy
            .get("count_question")
            .cloned()
            .unwrap_or(Value::Bool(false)),
        "direct_count": evidence_slot_count_policy
            .get("direct_count")
            .cloned()
            .unwrap_or(Value::Null),
        "direct_slots": compact_count_context_slots_for_message(
            evidence_slot_count_policy
                .get("direct_slots")
                .unwrap_or(&Value::Null),
            compaction_level,
        ),
        "related_slots": compact_count_context_slots_for_message(
            evidence_slot_count_policy
                .get("related_slots")
                .unwrap_or(&Value::Null),
            compaction_level,
        ),
        "rule": "For count questions, use direct_count exactly; direct_slots are counted, related_slots are mentioned separately and do not change direct_count"
    })
}

fn compact_count_context_slots_for_message(slots: &Value, compaction_level: usize) -> Value {
    let Some(items) = slots.as_array() else {
        return json!([]);
    };
    let (limit, source_title_chars) = match compaction_level {
        1 => (5, 64),
        2 => (4, 48),
        _ => (3, 32),
    };
    Value::Array(
        items
            .iter()
            .take(limit)
            .filter_map(Value::as_object)
            .map(|slot| {
                let mut compact = Map::new();
                if compaction_level <= 2 {
                    insert_existing_value(&mut compact, slot, "id");
                }
                insert_existing_value(&mut compact, slot, "label");
                insert_existing_value(&mut compact, slot, "public_role_label");
                if compaction_level <= 2 {
                    insert_existing_value(&mut compact, slot, "counts_as");
                    insert_existing_value(&mut compact, slot, "display_group");
                    insert_existing_value(&mut compact, slot, "count_note");
                    insert_existing_value(&mut compact, slot, "source_layer");
                }
                insert_existing_value(&mut compact, slot, "evidence_id");
                insert_existing_value(&mut compact, slot, "source_cues");
                insert_trimmed_string(&mut compact, slot, "source_title", source_title_chars);
                Value::Object(compact)
            })
            .collect(),
    )
}

fn compact_evidence_slot_rules_for_message(rules: &Value, compaction_level: usize) -> Value {
    let Some(items) = rules.as_array() else {
        return json!([]);
    };
    Value::Array(
        items
            .iter()
            .filter_map(Value::as_object)
            .map(|rule| {
                let mut compact = Map::new();
                insert_existing_value(&mut compact, rule, "id");
                insert_existing_value(&mut compact, rule, "label");
                insert_existing_value(&mut compact, rule, "public_role_label");
                if compaction_level <= 1 {
                    insert_existing_value(&mut compact, rule, "role");
                }
                if compaction_level <= 2 {
                    insert_existing_value(&mut compact, rule, "counts_as");
                    insert_existing_value(&mut compact, rule, "display_group");
                    insert_existing_value(&mut compact, rule, "count_note");
                }
                Value::Object(compact)
            })
            .collect(),
    )
}

fn compact_string_array_for_message(value: &Value, max_items: usize, max_chars: usize) -> Value {
    let Some(items) = value.as_array() else {
        return json!([]);
    };
    Value::Array(
        items
            .iter()
            .filter_map(Value::as_str)
            .take(max_items)
            .map(|item| Value::String(trim_text(item, max_chars)))
            .collect(),
    )
}

fn insert_existing_value(map: &mut Map<String, Value>, source: &Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        map.insert(key.to_string(), value.clone());
    }
}

fn insert_trimmed_string(
    map: &mut Map<String, Value>,
    source: &Map<String, Value>,
    key: &str,
    max_chars: usize,
) {
    if max_chars == 0 {
        return;
    }
    if let Some(value) = source.get(key).and_then(Value::as_str) {
        map.insert(key.to_string(), Value::String(trim_text(value, max_chars)));
    }
}

fn step_output_message_payload_with_compaction(
    step: &RuntimeWorkflowStepReport,
    compaction_level: usize,
) -> Value {
    let object = step
        .output
        .get("object")
        .cloned()
        .unwrap_or_else(|| json!("tonglingyu.runtime_step_output"));
    let package_id = step
        .output
        .get("package_id")
        .cloned()
        .unwrap_or(Value::Null);
    let output_evidence_ids = step
        .output
        .get("evidence_ids")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let evidence_brief = step
        .output
        .get("evidence_brief")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let evidence_brief = if step.operation == "draft_answer" {
        compact_draft_evidence_brief_for_message(evidence_brief, compaction_level)
    } else {
        evidence_brief
    };
    let evidence_ids = if step.operation == "draft_answer" {
        evidence_ids_from_evidence_brief(&evidence_brief).unwrap_or(output_evidence_ids)
    } else {
        output_evidence_ids
    };
    let evidence_types = step
        .output
        .get("evidence_types")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let review = step
        .output
        .get("review")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let review_status = step
        .output
        .get("review_status")
        .cloned()
        .or_else(|| review.get("status").cloned())
        .unwrap_or(Value::Null);
    let source_scope_policy = step
        .output
        .get("source_scope_policy")
        .cloned()
        .unwrap_or(Value::Null);
    let evidence_slot_count_policy = step
        .output
        .get("evidence_slot_count_policy")
        .cloned()
        .unwrap_or(Value::Null);
    let evidence_slot_count_policy = if step.operation == "draft_answer" {
        compact_evidence_slot_count_policy_for_message(evidence_slot_count_policy, compaction_level)
    } else {
        evidence_slot_count_policy
    };
    let out_of_scope_hint_count = step
        .output
        .get("out_of_scope_hints")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    match step.operation.as_str() {
        "draft_answer" => json!({
            "object": object,
            "operation": &step.operation,
            "output_ref": &step.output_ref,
            "package_id": package_id,
            "evidence_ids": evidence_ids,
            "evidence_brief": evidence_brief,
            "evidence_slot_count_policy": evidence_slot_count_policy,
            "source_scope_policy": source_scope_policy,
        }),
        "review_answer" => json!({
            "object": object,
            "operation": &step.operation,
            "output_ref": &step.output_ref,
            "package_id": package_id,
            "review_status": review_status,
            "review_severity": review.get("severity").cloned().unwrap_or(Value::Null),
            "review_issue_count": review
                .get("issues")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            "draft_consumed": step.output.get("draft_consumed").cloned().unwrap_or(Value::Null),
            "revision_applied": step.output.get("revision_applied").cloned().unwrap_or(Value::Null),
        }),
        "evidence_package_create" => json!({
            "object": object,
            "operation": &step.operation,
            "output_ref": &step.output_ref,
            "package_id": package_id,
            "card_count": step.output.get("card_count").cloned().unwrap_or(Value::Null),
            "claim_count": step.output.get("claim_count").cloned().unwrap_or(Value::Null),
            "review_status": review_status,
            "source_scope_policy": source_scope_policy,
            "out_of_scope_hint_count": out_of_scope_hint_count,
        }),
        "text_evidence_search" | "commentary_evidence_search" => json!({
            "object": object,
            "operation": &step.operation,
            "output_ref": &step.output_ref,
            "card_count": step.output.get("card_count").cloned().unwrap_or(Value::Null),
            "evidence_types": evidence_types,
            "evidence_set_ref": evidence_set_ref_from_output(&step.trace_id, &step.output),
            "evidence_ref_policy": "do_not_echo_runtime_ids",
        }),
        _ => json!({
            "object": object,
            "operation": &step.operation,
            "output_ref": &step.output_ref,
            "package_id": package_id,
            "card_count": step.output.get("card_count").cloned().unwrap_or(Value::Null),
            "evidence_ids": evidence_ids,
            "evidence_types": evidence_types,
        }),
    }
}

fn evidence_ids_from_evidence_brief(evidence_brief: &Value) -> Option<Value> {
    let ids = evidence_brief
        .as_array()?
        .iter()
        .filter_map(|item| item.get("evidence_id").and_then(Value::as_str))
        .map(|item| json!(item))
        .collect::<Vec<_>>();
    Some(Value::Array(ids))
}

fn evidence_set_ref_from_output(trace_id: &str, output: &Value) -> Option<String> {
    let evidence_ids = output
        .get("evidence_ids")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    Some(evidence_set_output_ref(trace_id, &evidence_ids))
}

fn agent_runtime_result_summary_contract(step: &RuntimeWorkflowStepReport) -> &'static str {
    match step.operation.as_str() {
        "draft_answer" => {
            "Return exactly one non-empty JSON object with this shape: {\"schema_version\":\"tonglingyu-upstream-bundle-v1\",\"package_id\":\"...\",\"source_scope_policy\":{},\"draft_candidate\":{\"draft_answer\":\"...\",\"package_id\":\"...\",\"claim_statements\":[{\"text\":\"...\",\"evidence_refs\":[...]}]},\"coverage_assessment\":{\"status\":\"passed|partial|insufficient\",\"missing_in_scope_slots\":[],\"out_of_scope_slots\":[]},\"evidence_hints\":[],\"retrieval_repair\":{\"recommended\":false,\"queries\":[]},\"out_of_scope_hints\":[]}. Copy step_output_json.source_scope_policy exactly. Use only step_output_json.evidence_brief and step_output_json.evidence_slot_count_policy; evidence_refs must come from step_output_json.evidence_ids. Commentary evidence is first-class in scope. If later_forty_allowed=false, ignore later-forty source layers. For count questions, if evidence_slot_count_policy.direct_count is present, use that number exactly; direct_slots are counted evidence, related_slots are separate clues that must not change direct_count. Every direct_slots or related_slots item named in draft_answer must carry one of its source_cues or a source_title cue in the same sentence. For fate, ending, or interpretive questions, coverage_assessment.status is passed when the draft gives the strongest answer supported inside source_scope_policy and states the evidence boundary; do not mark partial merely because excluded later-forty or unavailable final narrative details could say more. Use partial only when missing in-scope evidence prevents even a bounded answer. The visible draft_answer must name public event/source labels from evidence_slot_rules.label or count policy slot labels and embed a short source or phrase cue; do not expose internal terms such as evidence slot, slot id, package_id, trace_id, context_pack, claim_statements, or result_summary. Do not answer only with generic phrases such as 'some evidence' or 'related clues'. Local reviewer remains authoritative. Do not add nested result_summary."
        }
        "review_answer" => {
            "Return exactly one non-empty JSON object with this shape: {\"review_observation\":{\"review_status\":\"passed|needs_revision\",\"severity\":\"...\",\"issues\":[],\"required_revisions\":[]}}. This is observation only; local reviewer enforcement remains authoritative. Do not add another result_summary key."
        }
        "text_evidence_search" => {
            "Return exactly one non-empty JSON object with this shape: {\"evidence_observation\":{\"evidence_refs\":[],\"evidence_analysis\":\"short observation\",\"unsupported_scope\":\"none|short boundary\"}}. step_output_json does not expose runtime evidence ids; keep evidence_refs empty. Do not write a final answer. Do not add another result_summary key."
        }
        "commentary_evidence_search" => {
            "Return exactly one non-empty JSON object with this shape: {\"evidence_observation\":{\"commentary_refs\":[],\"commentary_analysis\":\"short observation\",\"scope_notes\":\"short boundary\"}}. step_output_json does not expose runtime evidence ids; keep commentary_refs empty. Commentary is first-class evidence within the default pre-80 scope; later-forty material still requires explicit scope. Do not add another result_summary key."
        }
        "evidence_package_create" => {
            "Return exactly one non-empty JSON object with this shape: {\"package_observation\":{\"package_id\":\"...\",\"summary\":\"short observation\"}}. package_id must come from step_output_json; do not invent package ids. Do not add another result_summary key."
        }
        _ => {
            "Return result_summary as a concise step observation that preserves the step output boundary."
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AgentRuntimeContentApplication {
    draft_consumed: bool,
    content_used_for_final_answer: bool,
    result_format: &'static str,
    rejected_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct AgentRuntimeEvidenceObservation {
    operation: String,
    profile: String,
    result_format: &'static str,
    evidence_ref_count: usize,
    unknown_evidence_refs: Vec<String>,
    matches_runtime_evidence: bool,
    rejected_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct AgentRuntimePackageObservation {
    result_format: &'static str,
    package_id: Option<String>,
    matches_runtime_package: bool,
    rejected_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct AgentRuntimeReviewObservation {
    result_format: &'static str,
    review_status: Option<String>,
    severity: Option<String>,
    issue_count: Option<usize>,
    required_revision_count: Option<usize>,
    agrees_with_local_reviewer: bool,
    local_reviewer_override: bool,
    rejected_reason: Option<&'static str>,
}

fn default_agent_runtime_summary() -> Value {
    deterministic_agent_runtime_summary(0)
}

fn deterministic_agent_runtime_summary(profile_step_count: usize) -> Value {
    json!({
        "mode": "not_attached",
        "profile_execution_status": "deterministic_workflow_only",
        "profile_step_count": profile_step_count,
        "executed_profile_step_count": 0,
        "tool_result_count": 0,
        "tool_audit_event_count": 0,
        "profile_observation_complete": false,
        "profile_content_execution_complete": false,
        "local_governance_enforced": true,
    })
}

fn agent_runtime_profile_observation_mode(mode: TonglingyuAgentRuntimeMode) -> bool {
    matches!(
        mode,
        TonglingyuAgentRuntimeMode::Hermes | TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork
    )
}

fn agent_runtime_profile_content_source(mode: TonglingyuAgentRuntimeMode, suffix: &str) -> String {
    let source = match mode {
        TonglingyuAgentRuntimeMode::Hermes => "hermes",
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork => "openai-compatible",
        TonglingyuAgentRuntimeMode::Minimal => "minimal",
    };
    format!("agent-runtime-{source}-{suffix}")
}

fn agent_runtime_profile_answer_source(mode: TonglingyuAgentRuntimeMode, suffix: &str) -> String {
    let source = match mode {
        TonglingyuAgentRuntimeMode::Hermes => "hermes",
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork => "openai_compatible",
        TonglingyuAgentRuntimeMode::Minimal => "minimal",
    };
    format!("agent_runtime_{source}_profile_{suffix}")
}

fn agent_runtime_profile_draft_source(mode: TonglingyuAgentRuntimeMode) -> String {
    let source = match mode {
        TonglingyuAgentRuntimeMode::Hermes => "hermes",
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork => "openai_compatible",
        TonglingyuAgentRuntimeMode::Minimal => "minimal",
    };
    format!("agent_runtime_{source}_profile")
}

fn should_repair_agent_runtime_draft(
    mode: TonglingyuAgentRuntimeMode,
    application: Option<&AgentRuntimeContentApplication>,
) -> bool {
    agent_runtime_profile_observation_mode(mode)
        && application.is_some_and(|value| !value.draft_consumed && value.rejected_reason.is_some())
}

#[allow(clippy::too_many_arguments)]
async fn repair_agent_runtime_draft(
    workflow: &mut RuntimeWorkflowOutput,
    profiles: &RuntimeWorkflowProfiles,
    context: &RuntimeContextContract,
    mode: TonglingyuAgentRuntimeMode,
    runtime: Arc<dyn RuntimeClient>,
    rejected_application: &AgentRuntimeContentApplication,
) -> Result<()> {
    let profile_contracts = agent_runtime_profile_contracts(profiles);
    let contracts = profile_contracts
        .into_iter()
        .map(|contract| (contract.profile_id.clone(), contract))
        .collect::<BTreeMap<_, _>>();
    let Some((draft_step_index, draft_step)) = workflow
        .steps
        .iter()
        .cloned()
        .enumerate()
        .find(|(_, step)| step.operation == "draft_answer")
    else {
        return Err(anyhow!("runtime profile draft repair missing draft step"));
    };
    let rejected_reason = rejected_application
        .rejected_reason
        .ok_or_else(|| anyhow!("runtime profile draft repair missing rejected reason"))?;
    let contract = contracts.get(&draft_step.profile).cloned().ok_or_else(|| {
        anyhow!(
            "runtime profile contract missing for draft repair {}",
            draft_step.profile
        )
    })?;
    let projection = context
        .projection_for_consumer(&draft_step.profile)?
        .clone();
    let result_summary_contract = agent_runtime_result_summary_contract(&draft_step).to_owned();
    let runtime_step = agent_runtime_step_from_workflow_step(&draft_step);
    let visible_question = projection
        .projection_payload
        .get("visible_question")
        .and_then(Value::as_str)
        .unwrap_or("");
    let initial_rejection =
        agent_runtime_draft_repair_initial_state(&draft_step, rejected_application);
    let message = agent_runtime_profile_step_repair_message(
        &workflow.trace_id,
        &draft_step,
        &projection,
        &result_summary_contract,
        rejected_reason,
    );
    let message_bytes = message.content.len();
    if message_bytes > AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES {
        return Err(anyhow!(
            "runtime profile draft repair message exceeded safety budget: step_id={} operation={} bytes={} limit={}",
            draft_step.step_id,
            draft_step.operation,
            message_bytes,
            AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES
        ));
    }
    let execution = execute_agent_runtime_profile_step_with_retry(
        &draft_step,
        &workflow.trace_id,
        &message,
        &result_summary_contract,
        &contract,
        &runtime_step,
        &projection,
        context,
        visible_question,
        message_bytes,
        runtime,
    )
    .await
    .with_context(|| {
        format!(
            "runtime profile draft repair failed: step_id={} rejected_reason={rejected_reason}",
            draft_step.step_id
        )
    })?;
    let mut repaired = agent_runtime_step_execution_from_run(
        draft_step_index,
        mode,
        &draft_step,
        result_summary_contract,
        execution,
    )?;
    if let Some(agent_runtime) = repaired.agent_runtime.as_object_mut() {
        agent_runtime.insert(
            "draft_repair".to_string(),
            json!({
                "phase": "draft_governance_repair",
                "attempted": true,
                "repair_attempt_count": 1,
                "initial_rejection": initial_rejection,
            }),
        );
    }
    let step = &mut workflow.steps[draft_step_index];
    step.agent_runtime = Some(repaired.agent_runtime);
    step.output["agent_runtime_draft_repair_attempted"] = json!(true);
    step.output["agent_runtime_initial_draft_rejected_reason"] = json!(rejected_reason);
    Ok(())
}

fn agent_runtime_draft_repair_initial_state(
    step: &RuntimeWorkflowStepReport,
    application: &AgentRuntimeContentApplication,
) -> Value {
    let agent_runtime = step.agent_runtime.as_ref();
    json!({
        "rejected_reason": application.rejected_reason,
        "result_format": application.result_format,
        "content_used_for_final_answer": application.content_used_for_final_answer,
        "result_ref": agent_runtime
            .and_then(|value| value.get("result_ref"))
            .cloned()
            .unwrap_or(Value::Null),
        "provider_request_sha256": agent_runtime
            .and_then(|value| value.get("provider_request_sha256"))
            .cloned()
            .unwrap_or(Value::Null),
        "content_source": agent_runtime
            .and_then(|value| value.get("content_source"))
            .cloned()
            .unwrap_or(Value::Null),
    })
}

fn agent_runtime_profile_step_repair_message(
    trace_id: &str,
    step: &RuntimeWorkflowStepReport,
    projection: &RuntimeContextProjection,
    result_summary_contract: &str,
    rejected_reason: &str,
) -> RuntimeProfileMessage {
    let max_compaction_level = if step.operation == "draft_answer" {
        3
    } else {
        0
    };
    let mut selected_content = String::new();
    for compaction_level in 0..=max_compaction_level {
        selected_content = agent_runtime_profile_step_repair_message_content(
            trace_id,
            step,
            projection,
            result_summary_contract,
            rejected_reason,
            compaction_level,
        );
        if selected_content.len() <= AGENT_RUNTIME_PROFILE_MESSAGE_MAX_BYTES {
            break;
        }
    }
    RuntimeProfileMessage::new("user", selected_content)
}

fn agent_runtime_profile_step_repair_message_content(
    trace_id: &str,
    step: &RuntimeWorkflowStepReport,
    projection: &RuntimeContextProjection,
    result_summary_contract: &str,
    rejected_reason: &str,
    compaction_level: usize,
) -> String {
    let projection_payload = compact_repair_context_projection_payload_for_message(projection);
    let step_output = step_output_message_payload_with_compaction(step, compaction_level);
    let repair_context = json!({
        "object": "tonglingyu.draft_repair_context",
        "rejected_reason": rejected_reason,
        "required_action": "return a full replacement upstream bundle that satisfies the same result_summary_contract",
        "package_binding": "package_id, source_scope_policy, and evidence_refs must come only from step_output_json",
        "count_rule": "for count questions, use evidence_slot_count_policy.direct_count exactly when present; direct_slots are counted evidence and related_slots are separate clues",
        "public_answer_rule": "the visible draft_answer must use public labels from evidence_slot_rules.label or evidence_slot_count_policy slot labels; every named direct_slots or related_slots item must include one of its source_cues/source_title cues in the same sentence; avoid internal runtime terms",
        "coverage_rule": "for fate, ending, or interpretive questions, status=passed is correct when the answer is bounded to source_scope_policy and states what the evidence can and cannot prove; excluded later-forty material is out_of_scope, not missing_in_scope",
        "failure_boundary": "if the repaired bundle cannot satisfy these constraints, return coverage_assessment.status=insufficient with concrete missing_in_scope_slots",
    });
    format!(
        "Tonglingyu profile draft repair context.\nOutput rule: return exactly one non-empty JSON object as assistant content. Do not return an empty assistant message. Do not use markdown.\ntrace_id: {trace_id}\nprofile: {}\noperation: {}\ncontext_projection_payload_json: {}\nresult_summary_contract: {result_summary_contract}\nstep_output_json: {}\ndraft_repair_context_json: {}\nrepair_instruction: Fix the rejected draft without changing the evidence package boundary. Return only the replacement JSON object.\n",
        step.profile,
        step.operation,
        serde_json::to_string(&projection_payload).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string(&step_output).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string(&repair_context).unwrap_or_else(|_| "{}".to_string())
    )
}

fn compact_repair_context_projection_payload_for_message(
    projection: &RuntimeContextProjection,
) -> Value {
    let mut compact = Map::new();
    if let Some(source) = projection.projection_payload.as_object() {
        insert_trimmed_string(&mut compact, source, "visible_question", 160);
        insert_trimmed_string(&mut compact, source, "resolved_question", 160);
        insert_trimmed_string(&mut compact, source, "session_summary", 220);
    }
    compact.insert(
        "context_projection_digest".to_string(),
        Value::String(projection.context_projection_digest.clone()),
    );
    compact.insert(
        "projection_payload_sha256".to_string(),
        Value::String(hash_json(&projection.projection_payload)),
    );
    Value::Object(compact)
}

fn agent_runtime_execution_summary(
    mode: TonglingyuAgentRuntimeMode,
    workflow: &RuntimeWorkflowOutput,
    application: Option<&AgentRuntimeContentApplication>,
) -> Value {
    let profile_step_count = workflow.steps.len();
    let executed_profile_step_count = workflow
        .steps
        .iter()
        .filter(|step| {
            step.agent_runtime
                .as_ref()
                .is_some_and(|value| value.get("status") == Some(&json!("executed")))
        })
        .count();
    let tool_result_count = workflow
        .steps
        .iter()
        .filter_map(|step| {
            step.agent_runtime
                .as_ref()
                .and_then(|value| value.get("tool_result_count"))
                .and_then(Value::as_u64)
        })
        .sum::<u64>();
    let tool_audit_event_count = workflow
        .steps
        .iter()
        .filter_map(|step| {
            step.agent_runtime
                .as_ref()
                .and_then(|value| value.get("tool_audit_event_count"))
                .and_then(Value::as_u64)
        })
        .sum::<u64>();

    let evidence_steps = workflow
        .steps
        .iter()
        .filter(|step| {
            matches!(
                step.operation.as_str(),
                "text_evidence_search" | "commentary_evidence_search"
            )
        })
        .collect::<Vec<_>>();
    let evidence_observation_count = evidence_steps
        .iter()
        .filter(|step| {
            step.agent_runtime
                .as_ref()
                .and_then(|value| value.get("evidence_observation"))
                .is_some()
        })
        .count();
    let evidence_matches_local = !evidence_steps.is_empty()
        && evidence_steps.iter().all(|step| {
            let observation = step
                .agent_runtime
                .as_ref()
                .and_then(|value| value.get("evidence_observation"));
            observation.and_then(|value| value.get("matches_runtime_evidence"))
                == Some(&json!(true))
                && observation.and_then(|value| value.get("local_evidence_enforced"))
                    == Some(&json!(true))
        });
    let package_matches_local = workflow.steps.iter().any(|step| {
        step.operation == "evidence_package_create"
            && step
                .agent_runtime
                .as_ref()
                .and_then(|value| value.get("package_observation"))
                .is_some_and(|observation| {
                    observation.get("matches_runtime_package") == Some(&json!(true))
                        && observation.get("local_package_enforced") == Some(&json!(true))
                })
    });
    let review_local_enforced = workflow.steps.iter().any(|step| {
        step.operation == "review_answer"
            && step
                .agent_runtime
                .as_ref()
                .and_then(|value| value.get("review_observation"))
                .is_some_and(|observation| {
                    observation.get("local_reviewer_enforced") == Some(&json!(true))
                })
    });
    let draft_consumed = application.is_some_and(|value| value.draft_consumed);
    let content_used_for_final_answer =
        application.is_some_and(|value| value.content_used_for_final_answer);
    let local_answer_used_for_final_answer = agent_runtime_profile_observation_mode(mode)
        && !content_used_for_final_answer
        && workflow.answer_source == "runtime_local_profile";
    let draft_governance_completed = application.is_some_and(|value| value.draft_consumed);
    let evidence_governance_completed =
        evidence_matches_local || local_answer_used_for_final_answer;
    let profile_observation_complete = agent_runtime_profile_observation_mode(mode)
        && evidence_governance_completed
        && package_matches_local
        && draft_governance_completed
        && review_local_enforced;
    let profile_content_execution_complete =
        agent_runtime_profile_observation_mode(mode) && profile_observation_complete;
    let profile_execution_status = match mode {
        TonglingyuAgentRuntimeMode::Minimal => "minimal_envelope_only",
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork if profile_observation_complete => {
            "openai_compatible_profile_observed_with_local_governance"
        }
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork if draft_consumed => {
            "openai_compatible_profile_partial_with_local_governance"
        }
        TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork => {
            "openai_compatible_profile_incomplete_local_governance"
        }
        TonglingyuAgentRuntimeMode::Hermes if profile_observation_complete => {
            "hermes_profile_observed_with_local_governance"
        }
        TonglingyuAgentRuntimeMode::Hermes if draft_consumed => {
            "hermes_profile_partial_with_local_governance"
        }
        TonglingyuAgentRuntimeMode::Hermes => "hermes_profile_incomplete_local_governance",
    };

    json!({
        "mode": mode.as_str(),
        "profile_execution_status": profile_execution_status,
        "profile_step_count": profile_step_count,
        "executed_profile_step_count": executed_profile_step_count,
        "tool_result_count": tool_result_count,
        "tool_audit_event_count": tool_audit_event_count,
        "evidence_observation_count": evidence_observation_count,
        "evidence_matches_local": evidence_matches_local,
        "package_matches_local": package_matches_local,
        "draft_consumed": draft_consumed,
        "draft_governance_completed": draft_governance_completed,
        "content_used_for_final_answer": content_used_for_final_answer,
        "review_local_enforced": review_local_enforced,
        "profile_observation_complete": profile_observation_complete,
        "profile_content_execution_complete": profile_content_execution_complete,
        "local_governance_enforced": true,
        "answer_source": &workflow.answer_source,
    })
}

fn validate_agent_runtime_execution_summary(
    mode: TonglingyuAgentRuntimeMode,
    summary: &Value,
) -> Result<()> {
    let status = summary
        .get("profile_execution_status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if mode == TonglingyuAgentRuntimeMode::OpenAiCompatibleNetwork {
        let complete = summary
            .get("profile_observation_complete")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if complete && status == "openai_compatible_profile_observed_with_local_governance" {
            return Ok(());
        }
        return Err(anyhow!(
            "OpenAI-compatible runtime profile observation incomplete: {status}"
        ));
    }
    if mode != TonglingyuAgentRuntimeMode::Hermes {
        return Ok(());
    }
    let complete = summary
        .get("profile_content_execution_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let tool_result_count = summary
        .get("tool_result_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let tool_audit_event_count = summary
        .get("tool_audit_event_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if complete && status == "hermes_profile_observed_with_local_governance" {
        if tool_result_count > 0 && tool_audit_event_count < tool_result_count {
            return Err(anyhow!(
                "Hermes runtime profile execution missing tool audit events: {tool_audit_event_count}/{tool_result_count}"
            ));
        }
        return Ok(());
    }
    Err(anyhow!(
        "Hermes runtime profile execution incomplete: {status}"
    ))
}

fn apply_agent_runtime_evidence_outputs(
    workflow: &mut RuntimeWorkflowOutput,
    mode: TonglingyuAgentRuntimeMode,
) -> Vec<AgentRuntimeEvidenceObservation> {
    if !agent_runtime_profile_observation_mode(mode) {
        return Vec::new();
    }
    let mut observations = Vec::new();
    for step in &mut workflow.steps {
        if !matches!(
            step.operation.as_str(),
            "text_evidence_search" | "commentary_evidence_search"
        ) {
            continue;
        }
        let Some(summary) = step
            .agent_runtime
            .as_ref()
            .and_then(|value| value.get("result_summary"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let expected_evidence_ids = step
            .output
            .get("evidence_ids")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let observation = extract_agent_runtime_evidence_observation(
            summary,
            &step.operation,
            &step.profile,
            &expected_evidence_ids,
        );
        step.output["agent_runtime_evidence_observed"] = json!(true);
        step.output["agent_runtime_evidence_result_format"] = json!(observation.result_format);
        step.output["agent_runtime_evidence_ref_count"] = json!(observation.evidence_ref_count);
        step.output["agent_runtime_unknown_evidence_refs"] =
            json!(&observation.unknown_evidence_refs);
        step.output["agent_runtime_evidence_matches_local"] =
            json!(observation.matches_runtime_evidence);
        step.output["agent_runtime_evidence_rejected_reason"] = json!(observation.rejected_reason);
        if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut) {
            agent_runtime.insert(
                "content_source".to_string(),
                json!(agent_runtime_profile_content_source(
                    mode,
                    "evidence-observation"
                )),
            );
            agent_runtime.insert(
                "evidence_observation".to_string(),
                json!({
                    "result_format": observation.result_format,
                    "evidence_ref_count": observation.evidence_ref_count,
                    "unknown_evidence_refs": &observation.unknown_evidence_refs,
                    "matches_runtime_evidence": observation.matches_runtime_evidence,
                    "rejected_reason": &observation.rejected_reason,
                    "local_evidence_enforced": true,
                }),
            );
        }
        observations.push(observation);
    }
    observations
}

fn extract_agent_runtime_evidence_observation(
    result_summary: &str,
    operation: &str,
    profile: &str,
    expected_evidence_ids: &[String],
) -> AgentRuntimeEvidenceObservation {
    let trimmed = result_summary.trim();
    let Some(value) = parse_agent_runtime_summary_value(trimmed) else {
        return rejected_evidence_observation(
            operation,
            profile,
            "text",
            Some("unsupported_text_evidence"),
        );
    };
    let Some(object) = object_or_named_child(&value, "evidence_observation") else {
        return rejected_evidence_observation(
            operation,
            profile,
            "json",
            Some("unsupported_json_evidence"),
        );
    };
    let refs_key = if operation == "commentary_evidence_search" {
        "commentary_refs"
    } else {
        "evidence_refs"
    };
    let Some(refs) = object.get(refs_key).and_then(Value::as_array) else {
        return rejected_evidence_observation(
            operation,
            profile,
            "json",
            Some("evidence_refs_missing"),
        );
    };
    let evidence_refs = refs
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let expected = expected_evidence_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let unknown_evidence_refs = evidence_refs
        .iter()
        .filter(|value| !expected.contains(value.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    AgentRuntimeEvidenceObservation {
        operation: operation.to_string(),
        profile: profile.to_string(),
        result_format: "json",
        evidence_ref_count: evidence_refs.len(),
        matches_runtime_evidence: unknown_evidence_refs.is_empty(),
        rejected_reason: if unknown_evidence_refs.is_empty() {
            None
        } else {
            Some("unknown_evidence_ref")
        },
        unknown_evidence_refs,
    }
}

fn rejected_evidence_observation(
    operation: &str,
    profile: &str,
    result_format: &'static str,
    rejected_reason: Option<&'static str>,
) -> AgentRuntimeEvidenceObservation {
    AgentRuntimeEvidenceObservation {
        operation: operation.to_string(),
        profile: profile.to_string(),
        result_format,
        evidence_ref_count: 0,
        unknown_evidence_refs: Vec::new(),
        matches_runtime_evidence: false,
        rejected_reason,
    }
}

fn apply_agent_runtime_package_output(
    workflow: &mut RuntimeWorkflowOutput,
    mode: TonglingyuAgentRuntimeMode,
) -> Option<AgentRuntimePackageObservation> {
    if !agent_runtime_profile_observation_mode(mode) {
        return None;
    }
    let (package_step_index, summary) =
        workflow
            .steps
            .iter()
            .enumerate()
            .find_map(|(index, step)| {
                if step.operation != "evidence_package_create" {
                    return None;
                }
                let summary = step
                    .agent_runtime
                    .as_ref()
                    .and_then(|value| value.get("result_summary"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                Some((index, summary.to_string()))
            })?;
    let observation =
        extract_agent_runtime_package_observation(&summary, &workflow.package.package_id);
    if let Some(step) = workflow.steps.get_mut(package_step_index) {
        step.output["agent_runtime_package_observed"] = json!(true);
        step.output["agent_runtime_package_result_format"] = json!(observation.result_format);
        step.output["agent_runtime_observed_package_id"] = json!(observation.package_id);
        step.output["agent_runtime_package_matches_local"] =
            json!(observation.matches_runtime_package);
        step.output["agent_runtime_package_rejected_reason"] = json!(observation.rejected_reason);
        if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut) {
            agent_runtime.insert(
                "content_source".to_string(),
                json!(agent_runtime_profile_content_source(
                    mode,
                    "package-observation"
                )),
            );
            agent_runtime.insert(
                "package_observation".to_string(),
                json!({
                    "result_format": observation.result_format,
                    "package_id": &observation.package_id,
                    "local_package_id": &workflow.package.package_id,
                    "matches_runtime_package": observation.matches_runtime_package,
                    "rejected_reason": &observation.rejected_reason,
                    "local_package_enforced": true,
                }),
            );
        }
    }
    Some(observation)
}

fn extract_agent_runtime_package_observation(
    result_summary: &str,
    expected_package_id: &str,
) -> AgentRuntimePackageObservation {
    let trimmed = result_summary.trim();
    let Some(value) = parse_agent_runtime_summary_value(trimmed) else {
        let package_id = package_id_from_text(trimmed, expected_package_id);
        let rejected_reason = match package_id.as_deref() {
            None => Some("package_id_missing"),
            Some(value) if value != expected_package_id => Some("package_id_mismatch"),
            Some(_) => None,
        };
        return AgentRuntimePackageObservation {
            result_format: "text",
            matches_runtime_package: rejected_reason.is_none(),
            package_id,
            rejected_reason,
        };
    };
    let Some(object) = object_or_named_child(&value, "package_observation") else {
        return AgentRuntimePackageObservation {
            result_format: "json",
            package_id: None,
            matches_runtime_package: false,
            rejected_reason: Some("unsupported_json_package"),
        };
    };
    let package_id = object
        .get("package_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let rejected_reason = match package_id.as_deref() {
        None => Some("package_id_missing"),
        Some(value) if value != expected_package_id => Some("package_id_mismatch"),
        Some(_) => None,
    };
    AgentRuntimePackageObservation {
        result_format: "json",
        matches_runtime_package: rejected_reason.is_none(),
        package_id,
        rejected_reason,
    }
}

fn apply_agent_runtime_content_outputs(
    workflow: &mut RuntimeWorkflowOutput,
    mode: TonglingyuAgentRuntimeMode,
) -> Option<AgentRuntimeContentApplication> {
    if !agent_runtime_profile_observation_mode(mode) {
        return None;
    }
    let (draft_step_index, extraction) =
        workflow
            .steps
            .iter()
            .enumerate()
            .find_map(|(index, step)| {
                if step.operation != "draft_answer" {
                    return None;
                }
                let summary = step
                    .agent_runtime
                    .as_ref()
                    .and_then(|value| value.get("result_summary"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                Some((
                    index,
                    extract_upstream_bundle_draft(
                        summary,
                        &workflow.package.package_id,
                        &source_scope_policy_for_question(&workflow.question),
                        &workflow
                            .package
                            .cards
                            .iter()
                            .map(|card| card.evidence_id.clone())
                            .collect::<BTreeSet<_>>(),
                    ),
                ))
            })?;

    if let Some(reason) = extraction.rejected_reason {
        if let Some(step) = workflow.steps.get_mut(draft_step_index) {
            step.output["agent_runtime_draft_consumed"] = json!(false);
            step.output["agent_runtime_result_format"] = json!(extraction.result_format);
            step.output["agent_runtime_draft_rejected_reason"] = json!(reason);
            step.output["agent_runtime_package_id"] = json!(extraction.package_id);
            step.output["agent_runtime_coverage_status"] = json!(extraction.coverage_status);
            step.output["agent_runtime_evidence_hint_count"] =
                json!(extraction.evidence_hint_count);
            step.output["agent_runtime_out_of_scope_hint_count"] =
                json!(extraction.out_of_scope_hint_count);
            if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut)
            {
                agent_runtime.insert(
                    "content_source".to_string(),
                    json!(agent_runtime_profile_content_source(
                        mode,
                        "profile-rejected"
                    )),
                );
                agent_runtime.insert("content_used_for_final_answer".to_string(), json!(false));
                agent_runtime.insert(
                    "content_application".to_string(),
                    json!({
                        "answer_source": &workflow.answer_source,
                        "local_reviewer_enforced": true,
                        "review_status": &workflow.package.review.status,
                        "result_format": extraction.result_format,
                        "draft_consumed": false,
                        "rejected_reason": reason,
                        "content_used_for_final_answer": false,
                    }),
                );
            }
        }
        return Some(AgentRuntimeContentApplication {
            draft_consumed: false,
            content_used_for_final_answer: false,
            result_format: extraction.result_format,
            rejected_reason: Some(reason),
        });
    }

    let draft = extraction.draft_answer?;
    if let Some(reason) = agent_runtime_draft_evidence_boundary_rejection(
        &workflow.package.question,
        &draft,
        &workflow.package.cards,
    ) {
        if let Some(step) = workflow.steps.get_mut(draft_step_index) {
            step.output["agent_runtime_draft_consumed"] = json!(false);
            step.output["agent_runtime_result_format"] = json!(extraction.result_format);
            step.output["agent_runtime_draft_rejected_reason"] = json!(reason);
            step.output["agent_runtime_package_id"] = json!(extraction.package_id);
            if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut)
            {
                agent_runtime.insert(
                    "content_source".to_string(),
                    json!(agent_runtime_profile_content_source(
                        mode,
                        "profile-evidence-boundary-rejected"
                    )),
                );
                agent_runtime.insert("content_used_for_final_answer".to_string(), json!(false));
                agent_runtime.insert(
                    "content_application".to_string(),
                    json!({
                        "answer_source": &workflow.answer_source,
                        "local_reviewer_enforced": true,
                        "review_status": &workflow.package.review.status,
                        "result_format": extraction.result_format,
                        "draft_consumed": false,
                        "rejected_reason": reason,
                        "content_used_for_final_answer": false,
                    }),
                );
            }
        }
        return Some(AgentRuntimeContentApplication {
            draft_consumed: false,
            content_used_for_final_answer: false,
            result_format: extraction.result_format,
            rejected_reason: Some(reason),
        });
    }

    workflow.draft_answer = draft.clone();
    workflow.final_answer = enforce_review(draft, &workflow.package);
    let content_used_for_final_answer = workflow.package.review.status == "passed";
    workflow.answer_source = if content_used_for_final_answer {
        agent_runtime_profile_answer_source(mode, "with_local_review")
    } else {
        agent_runtime_profile_answer_source(mode, "rejected_by_local_review")
    };
    let draft_source = agent_runtime_profile_draft_source(mode);
    if let Some(step) = workflow.steps.get_mut(draft_step_index) {
        step.output["answer_source"] = json!(&draft_source);
        step.output["agent_runtime_draft_consumed"] = json!(true);
        step.output["agent_runtime_content_used_for_final_answer"] =
            json!(content_used_for_final_answer);
        step.output["agent_runtime_draft_rejected_reason"] = Value::Null;
        step.output["agent_runtime_result_format"] = json!(extraction.result_format);
        step.output["agent_runtime_package_id"] = json!(extraction.package_id);
        step.output["agent_runtime_claim_statement_count"] =
            json!(extraction.claim_statement_count);
        step.output["agent_runtime_coverage_status"] = json!(extraction.coverage_status);
        step.output["agent_runtime_evidence_hint_count"] = json!(extraction.evidence_hint_count);
        step.output["agent_runtime_retrieval_repair_recommended"] =
            json!(extraction.retrieval_repair_recommended);
        step.output["agent_runtime_out_of_scope_hint_count"] =
            json!(extraction.out_of_scope_hint_count);
        if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut) {
            agent_runtime.insert(
                "content_source".to_string(),
                json!(agent_runtime_profile_content_source(mode, "profile")),
            );
            agent_runtime.insert(
                "content_used_for_final_answer".to_string(),
                json!(content_used_for_final_answer),
            );
            agent_runtime.insert(
                "content_application".to_string(),
                json!({
                    "answer_source": &workflow.answer_source,
                    "local_reviewer_enforced": true,
                    "review_status": &workflow.package.review.status,
                    "result_format": extraction.result_format,
                    "package_id": extraction.package_id,
                    "claim_statement_count": extraction.claim_statement_count,
                    "coverage_status": extraction.coverage_status,
                    "evidence_hint_count": extraction.evidence_hint_count,
                    "retrieval_repair_recommended": extraction.retrieval_repair_recommended,
                    "out_of_scope_hint_count": extraction.out_of_scope_hint_count,
                    "draft_consumed": true,
                    "content_used_for_final_answer": content_used_for_final_answer,
                }),
            );
        }
    }
    if let Some(step) = workflow
        .steps
        .iter_mut()
        .find(|step| step.operation == "review_answer")
    {
        step.output["draft_source"] = json!(&draft_source);
        step.output["final_answer_source"] = json!(&workflow.answer_source);
        step.output["local_reviewer_enforced"] = json!(true);
    }
    Some(AgentRuntimeContentApplication {
        draft_consumed: true,
        content_used_for_final_answer,
        result_format: extraction.result_format,
        rejected_reason: None,
    })
}

fn agent_runtime_draft_evidence_boundary_rejection(
    question: &str,
    draft: &str,
    cards: &[EvidenceCard],
) -> Option<&'static str> {
    if cards.is_empty() {
        return None;
    }
    let evidence_text = cards
        .iter()
        .map(|card| normalize_text(&card.text))
        .collect::<Vec<_>>()
        .join("\n");
    let draft_text = normalize_text(draft);
    if draft_stops_for_user_opt_in(&draft_text).unwrap_or(true) {
        return Some("draft_stops_for_user_opt_in");
    }
    if cards_include_later_forty(cards) && !text_mentions_later_forty_boundary(&draft_text) {
        return Some("draft_missing_later_forty_boundary");
    }
    if draft_count_conflicts_with_evidence_slots(question, &draft_text, cards) {
        return Some("draft_count_conflicts_with_evidence_events");
    }
    if draft_exposes_internal_evidence_slot_ids(question, &draft_text, cards) {
        return Some("draft_exposes_internal_evidence_slot_id");
    }
    if draft_has_public_forbidden_term(&draft_text).unwrap_or(true) {
        return Some("draft_exposes_internal_public_term");
    }
    if draft_negates_direct_evidence_slot(question, &draft_text, cards) {
        return Some("draft_negates_direct_evidence_slot_count");
    }
    if draft_lacks_embedded_slot_evidence(question, &draft_text, cards) {
        return Some("draft_missing_embedded_evidence_anchor");
    }
    if draft_lacks_embedded_slot_source(question, &draft_text, cards) {
        return Some("draft_missing_embedded_evidence_source");
    }
    if draft_has_unsupported_term_without_evidence(&draft_text, &evidence_text).unwrap_or(true) {
        return Some("draft_claim_exceeds_evidence_boundary");
    }
    None
}

fn draft_exposes_internal_evidence_slot_ids(
    question: &str,
    draft_text: &str,
    cards: &[EvidenceCard],
) -> bool {
    count_slot_representatives_for_boundary(question, cards).is_some_and(|matches| {
        matches.iter().any(|item| {
            let slot_id = normalize_text(&item.slot_id);
            !slot_id.is_empty() && draft_text.contains(&slot_id)
        })
    })
}

fn draft_lacks_embedded_slot_evidence(
    question: &str,
    draft_text: &str,
    cards: &[EvidenceCard],
) -> bool {
    count_slot_representatives_for_boundary(question, cards)
        .is_some_and(|matches| labels_missing_from_draft(draft_text, &matches))
}

fn labels_missing_from_draft(draft_text: &str, matches: &[EvidenceSlotMatch]) -> bool {
    matches.iter().any(|item| {
        let label = normalize_text(&item.label);
        !label.is_empty() && !draft_text.contains(&label)
    })
}

fn draft_lacks_embedded_slot_source(
    question: &str,
    draft_text: &str,
    cards: &[EvidenceCard],
) -> bool {
    count_slot_representatives_for_boundary(question, cards).is_some_and(|matches| {
        matches
            .iter()
            .any(|item| !source_cue_present_for_slot_match(draft_text, item))
    })
}

fn source_cue_present_for_slot_match(draft_text: &str, item: &EvidenceSlotMatch) -> bool {
    public_source_cues(&item.source_title)
        .iter()
        .map(|cue| normalize_text(cue))
        .filter(|cue| !cue.is_empty())
        .any(|cue| draft_text.contains(&cue))
}

fn public_source_cues(source_title: &str) -> Vec<String> {
    let mut cues = Vec::new();
    let title = source_title.trim();
    if !title.is_empty() {
        cues.push(title.to_string());
    }
    if let Some(tail) = title
        .rsplit('/')
        .next()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        cues.push(tail.to_string());
    }
    if let Some(chapter_no) = extract_chapter_no(title) {
        cues.push(format!("第{chapter_no}回"));
        cues.push(format!("第{chapter_no:03}回"));
    }
    cues.sort();
    cues.dedup();
    cues
}

fn evidence_slot_count_context_value(
    question: &str,
    cards: &[EvidenceCard],
    count_question: bool,
) -> Result<Value> {
    let mut policy = evidence_slot_count_policy_value(question, count_question)?;
    if !count_question {
        return Ok(policy);
    }
    let Some(active_basis) = active_count_basis_for_question(question, true)? else {
        return Ok(policy);
    };
    let slot_matches = evidence_slot_matches_for_cards(question, cards)?;
    let direct = representative_matches(&slot_matches, |item| {
        item.counts_as.iter().any(|basis| basis == &active_basis.id)
    });
    let related = representative_matches(&slot_matches, |item| {
        !item.counts_as.iter().any(|basis| basis == &active_basis.id)
            && item.display_group != "unclassified"
    });
    if let Some(object) = policy.as_object_mut() {
        object.insert("direct_count".to_string(), json!(direct.len()));
        object.insert(
            "direct_slots".to_string(),
            Value::Array(
                direct
                    .iter()
                    .map(evidence_slot_match_count_context_value)
                    .collect(),
            ),
        );
        object.insert(
            "related_slots".to_string(),
            Value::Array(
                related
                    .iter()
                    .map(evidence_slot_match_count_context_value)
                    .collect(),
            ),
        );
    }
    Ok(policy)
}

fn evidence_slot_match_count_context_value(item: &EvidenceSlotMatch) -> Value {
    json!({
        "id": &item.slot_id,
        "label": &item.label,
        "public_role_label": &item.public_role_label,
        "counts_as": &item.counts_as,
        "display_group": &item.display_group,
        "count_note": &item.count_note,
        "evidence_id": &item.evidence_id,
        "source_cues": public_source_cues(&item.source_title),
        "source_layer": &item.source_layer,
        "source_title": &item.source_title,
    })
}

fn count_slot_representatives_for_boundary(
    question: &str,
    cards: &[EvidenceCard],
) -> Option<Vec<EvidenceSlotMatch>> {
    if !question_asks_for_count(question).unwrap_or(false) || cards_include_later_forty(cards) {
        return None;
    }
    let active_basis = active_count_basis_for_question(question, true)
        .ok()
        .flatten()?;
    let slot_matches = evidence_slot_matches_for_cards(question, cards).unwrap_or_default();
    let direct = representative_matches(&slot_matches, |item| {
        item.counts_as.iter().any(|basis| basis == &active_basis.id)
    });
    if direct.is_empty() {
        return None;
    }
    let related = representative_matches(&slot_matches, |item| {
        !item.counts_as.iter().any(|basis| basis == &active_basis.id)
            && item.display_group != "unclassified"
    });
    let mut matches = direct;
    matches.extend(related);
    Some(matches)
}

fn draft_count_conflicts_with_evidence_slots(
    question: &str,
    draft_text: &str,
    cards: &[EvidenceCard],
) -> bool {
    if !question_asks_for_count(question).unwrap_or(false) {
        return false;
    }
    let Some(active_basis) = active_count_basis_for_question(question, true)
        .ok()
        .flatten()
    else {
        return false;
    };
    let slot_matches = evidence_slot_matches_for_cards(question, cards).unwrap_or_default();
    let direct_count = direct_count_for_basis(&active_basis, &slot_matches);
    if direct_count == 0 {
        return false;
    }
    explicit_total_count_for_basis(draft_text, &active_basis)
        .is_some_and(|count| count != direct_count)
}

fn draft_negates_direct_evidence_slot(
    question: &str,
    draft_text: &str,
    cards: &[EvidenceCard],
) -> bool {
    if !question_asks_for_count(question).unwrap_or(false) {
        return false;
    }
    let Some(active_basis) = active_count_basis_for_question(question, true)
        .ok()
        .flatten()
    else {
        return false;
    };
    let slot_matches = evidence_slot_matches_for_cards(question, cards).unwrap_or_default();
    representative_matches(&slot_matches, |item| {
        item.counts_as.iter().any(|basis| basis == &active_basis.id)
    })
    .iter()
    .any(|item| direct_slot_is_negated_after_label(draft_text, item, &active_basis))
}

fn direct_slot_is_negated_after_label(
    draft_text: &str,
    item: &EvidenceSlotMatch,
    active_basis: &EvidenceSlotCountBasis,
) -> bool {
    let label = normalize_text(&item.label);
    if label.is_empty() {
        return false;
    }
    let negation_terms = active_basis
        .direct_slot_negation_terms
        .iter()
        .map(|term| normalize_text(term))
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if negation_terms.is_empty() {
        return false;
    }
    draft_text.match_indices(&label).any(|(index, _)| {
        let clause = direct_slot_clause_after_label(draft_text, index);
        negation_terms.iter().any(|term| clause.contains(term))
    })
}

fn direct_slot_clause_after_label(draft_text: &str, label_index: usize) -> String {
    draft_text[label_index..]
        .chars()
        .take_while(|ch| !matches!(*ch, '；' | ';' | '。' | '！' | '!' | '？' | '?' | '\n'))
        .collect()
}

fn parse_agent_runtime_summary_value(trimmed: &str) -> Option<Value> {
    let value = serde_json::from_str::<Value>(trimmed).ok()?;
    let inner = value
        .as_object()
        .and_then(|object| object.get("result_summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(inner) = inner {
        return serde_json::from_str::<Value>(inner)
            .ok()
            .or_else(|| Some(json!(inner)));
    }
    Some(value)
}

fn object_or_named_child<'a>(
    value: &'a Value,
    child_key: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
    let object = value.as_object()?;
    object
        .get(child_key)
        .and_then(Value::as_object)
        .or(Some(object))
}

fn package_id_from_text(text: &str, expected_package_id: &str) -> Option<String> {
    if text.contains(expected_package_id) {
        return Some(expected_package_id.to_string());
    }
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
        .find(|token| token.starts_with("pkg-"))
        .map(ToOwned::to_owned)
}

fn apply_agent_runtime_reviewer_output(
    workflow: &mut RuntimeWorkflowOutput,
    mode: TonglingyuAgentRuntimeMode,
) -> Option<AgentRuntimeReviewObservation> {
    if !agent_runtime_profile_observation_mode(mode) {
        return None;
    }
    let (review_step_index, summary) =
        workflow
            .steps
            .iter()
            .enumerate()
            .find_map(|(index, step)| {
                if step.operation != "review_answer" {
                    return None;
                }
                let summary = step
                    .agent_runtime
                    .as_ref()
                    .and_then(|value| value.get("result_summary"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                Some((index, summary.to_string()))
            })?;
    let observation = extract_agent_runtime_review_observation(
        &summary,
        &workflow.package.review.status,
        &workflow.package.review.severity,
    );
    if let Some(step) = workflow.steps.get_mut(review_step_index) {
        step.output["agent_runtime_review_observed"] = json!(true);
        step.output["agent_runtime_review_result_format"] = json!(observation.result_format);
        step.output["agent_runtime_review_status"] = json!(observation.review_status);
        step.output["agent_runtime_review_severity"] = json!(observation.severity);
        step.output["agent_runtime_review_issue_count"] = json!(observation.issue_count);
        step.output["agent_runtime_required_revision_count"] =
            json!(observation.required_revision_count);
        step.output["agent_runtime_review_agrees_with_local"] =
            json!(observation.agrees_with_local_reviewer);
        step.output["agent_runtime_local_reviewer_override"] =
            json!(observation.local_reviewer_override);
        step.output["agent_runtime_review_rejected_reason"] = json!(observation.rejected_reason);
        if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut) {
            agent_runtime.insert(
                "content_source".to_string(),
                json!(agent_runtime_profile_content_source(
                    mode,
                    "review-observation"
                )),
            );
            agent_runtime.insert(
                "review_observation".to_string(),
                json!({
                    "result_format": observation.result_format,
                    "review_status": &observation.review_status,
                    "local_review_status": &workflow.package.review.status,
                    "severity": &observation.severity,
                    "issue_count": observation.issue_count,
                    "required_revision_count": observation.required_revision_count,
                    "agrees_with_local_reviewer": observation.agrees_with_local_reviewer,
                    "local_reviewer_override": observation.local_reviewer_override,
                    "rejected_reason": &observation.rejected_reason,
                    "local_reviewer_enforced": true,
                }),
            );
        }
    }
    Some(observation)
}

fn extract_agent_runtime_review_observation(
    result_summary: &str,
    local_review_status: &str,
    local_review_severity: &str,
) -> AgentRuntimeReviewObservation {
    let trimmed = result_summary.trim();
    let Some(value) = parse_agent_runtime_summary_value(trimmed) else {
        return rejected_review_observation("text", Some("unsupported_text_review"));
    };
    let Some(object) = object_or_named_child(&value, "review_observation") else {
        return rejected_review_observation("json", Some("unsupported_json_review"));
    };
    let review_status = object
        .get("review_status")
        .or_else(|| object.get("status"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if !matches!(review_status.as_deref(), Some("passed" | "needs_revision")) {
        return AgentRuntimeReviewObservation {
            result_format: "json",
            review_status,
            severity: object
                .get("severity")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            issue_count: object.get("issues").and_then(Value::as_array).map(Vec::len),
            required_revision_count: object
                .get("required_revisions")
                .and_then(Value::as_array)
                .map(Vec::len),
            agrees_with_local_reviewer: false,
            local_reviewer_override: false,
            rejected_reason: Some("invalid_review_status"),
        };
    }
    let severity = object
        .get("severity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let issue_count = object.get("issues").and_then(Value::as_array).map(Vec::len);
    let required_revision_count = object
        .get("required_revisions")
        .and_then(Value::as_array)
        .map(Vec::len);
    let agrees_with_local_reviewer = review_status.as_deref() == Some(local_review_status)
        && severity
            .as_deref()
            .is_none_or(|value| value == local_review_severity);
    AgentRuntimeReviewObservation {
        result_format: "json",
        review_status,
        severity,
        issue_count,
        required_revision_count,
        agrees_with_local_reviewer,
        local_reviewer_override: !agrees_with_local_reviewer,
        rejected_reason: None,
    }
}

fn rejected_review_observation(
    result_format: &'static str,
    rejected_reason: Option<&'static str>,
) -> AgentRuntimeReviewObservation {
    AgentRuntimeReviewObservation {
        result_format,
        review_status: None,
        severity: None,
        issue_count: None,
        required_revision_count: None,
        agrees_with_local_reviewer: false,
        local_reviewer_override: false,
        rejected_reason,
    }
}

fn agent_runtime_step_from_workflow_step(step: &RuntimeWorkflowStepReport) -> AgentRuntimeStep {
    let mut runtime_step = AgentRuntimeStep::new(
        step.profile.clone(),
        PROFILE_CONTRACT_VERSION,
        json!({
            "runtime": "tonglingyu",
            "workflow_step_id": &step.step_id,
            "operation": &step.operation,
            "input_ref": &step.input_ref,
            "output_ref": &step.output_ref,
            "context_contract": step.output
                .get("context_contract")
                .cloned()
                .unwrap_or_else(|| json!({})),
            "content_source": "tonglingyu-deterministic-workflow",
        }),
    );
    runtime_step.step_id = format!("agent-runtime-{}", step.step_id);
    runtime_step.input_ref = step.input_ref.clone();
    runtime_step.output_ref = Some(step.output_ref.clone());
    runtime_step.tool_policy = agent_runtime_tool_policy(step.allowed_tools.clone());
    runtime_step.output_contract = agent_runtime_output_schema();
    runtime_step
}

fn workflow_plan_step<'a>(
    plan: &'a RuntimeWorkflowPlan,
    operation: &str,
) -> Result<&'a RuntimeWorkflowPlanStep> {
    plan.steps
        .iter()
        .find(|step| step.operation == operation)
        .ok_or_else(|| anyhow!("runtime workflow plan missing operation {operation}"))
}

struct WorkflowStepReportInput<'a> {
    trace_id: &'a str,
    step_id: &'a str,
    profile: &'a str,
    operation: &'a str,
    required: bool,
    allowed_tools: Vec<String>,
    tool_calls: Vec<String>,
    input_ref: Option<String>,
    duration_ms: u128,
    output: Value,
    context: &'a RuntimeContextContract,
}

fn workflow_step_report(
    conn: &Connection,
    input: WorkflowStepReportInput<'_>,
) -> Result<RuntimeWorkflowStepReport> {
    let projection = input.context.projection_for_consumer(input.profile)?;
    let report = RuntimeWorkflowStepReport {
        step_id: input.step_id.to_string(),
        profile: input.profile.to_string(),
        profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
        operation: input.operation.to_string(),
        status: "completed".to_string(),
        required: input.required,
        allowed_tools: input.allowed_tools,
        tool_calls: input.tool_calls,
        input_ref: input.input_ref,
        output_ref: workflow_output_ref(input.trace_id, input.step_id),
        duration_ms: input.duration_ms,
        trace_id: input.trace_id.to_string(),
        output: output_with_context_contract(input.output, input.context, projection),
        agent_runtime: None,
    };
    append_runtime_audit_event(
        conn,
        input.trace_id,
        "runtime_profile_step_completed",
        &json!({
            "step_id": &report.step_id,
            "profile": &report.profile,
            "operation": &report.operation,
            "status": &report.status,
            "allowed_tools": &report.allowed_tools,
            "tool_calls": &report.tool_calls,
            "input_ref": &report.input_ref,
            "output_ref": &report.output_ref,
            "duration_ms": report.duration_ms,
            "context_pack_ref": &input.context.context_pack_ref,
            "context_pack_schema_version": &input.context.context_pack_schema_version,
            "context_pack_digest": &input.context.context_pack_digest,
            "context_projection": projection.audit_contract(),
        }),
    )?;
    Ok(report)
}

fn output_with_context_contract(
    mut output: Value,
    context: &RuntimeContextContract,
    projection: &RuntimeContextProjection,
) -> Value {
    let contract = json!({
        "context_pack_ref": &context.context_pack_ref,
        "context_pack_schema_version": &context.context_pack_schema_version,
        "context_pack_digest": &context.context_pack_digest,
        "context_projection": projection.audit_contract(),
    });
    if let Some(object) = output.as_object_mut() {
        object.insert("context_contract".to_string(), contract);
        output
    } else {
        json!({
            "value": output,
            "context_contract": contract,
        })
    }
}

fn workflow_output_ref(trace_id: &str, step_id: &str) -> String {
    format!("runtime://tonglingyu/{trace_id}/{step_id}")
}

fn step_id(index: usize, name: &str) -> String {
    format!("step-{index:02}-{name}")
}

fn merge_cards(mut left: Vec<EvidenceCard>, right: Vec<EvidenceCard>) -> Vec<EvidenceCard> {
    let mut seen = left
        .iter()
        .map(|card| card.block_id.clone())
        .collect::<HashSet<_>>();
    for card in right {
        if seen.insert(card.block_id.clone()) {
            left.push(card);
        }
    }
    left
}

fn evidence_ids(cards: &[EvidenceCard]) -> Vec<String> {
    cards.iter().map(|card| card.evidence_id.clone()).collect()
}

fn evidence_set_output_ref(trace_id: &str, evidence_ids: &[String]) -> String {
    let mut ids = evidence_ids.to_vec();
    ids.sort();
    let digest = hash_text(&ids.join("\n"));
    format!("runtime://tonglingyu/{trace_id}/evidence/{digest}")
}

fn evidence_types(cards: &[EvidenceCard]) -> Vec<String> {
    cards
        .iter()
        .map(|card| card.evidence_type.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn elapsed_ms(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

fn workflow_stream_events(
    trace_id: &str,
    final_profile: &str,
    package_id: &str,
    final_answer: &str,
    steps: &[RuntimeWorkflowStepReport],
) -> Vec<RuntimeWorkflowStreamEvent> {
    let mut events = Vec::new();
    events.push(RuntimeWorkflowStreamEvent {
        sequence: 0,
        event_type: "started".to_string(),
        profile: final_profile.to_string(),
        trace_id: trace_id.to_string(),
        content_delta: None,
        output_ref: None,
        package_id: Some(package_id.to_string()),
        metadata: json!({"runtime": "tonglingyu", "stream_source": "runtime_workflow"}),
    });
    for step in steps {
        events.push(RuntimeWorkflowStreamEvent {
            sequence: events.len() as u64,
            event_type: "step_completed".to_string(),
            profile: step.profile.clone(),
            trace_id: trace_id.to_string(),
            content_delta: None,
            output_ref: Some(step.output_ref.clone()),
            package_id: Some(package_id.to_string()),
            metadata: json!({
                "step_id": &step.step_id,
                "operation": &step.operation,
                "duration_ms": step.duration_ms,
                "allowed_tools": &step.allowed_tools,
                "agent_runtime": step.agent_runtime.as_ref().map(|value| json!({
                    "client": value.get("client").cloned().unwrap_or(Value::Null),
                    "status": value.get("status").cloned().unwrap_or(Value::Null),
                    "content_source": value.get("content_source").cloned().unwrap_or(Value::Null),
                    "content_used_for_final_answer": value
                        .get("content_used_for_final_answer")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "tool_rounds": value.get("tool_rounds").cloned().unwrap_or(Value::Null),
                    "tool_result_count": value
                        .get("tool_result_count")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "tool_audit_event_count": value
                        .get("tool_audit_event_count")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "evidence_observation": value
                        .get("evidence_observation")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "package_observation": value
                        .get("package_observation")
                        .cloned()
                        .unwrap_or(Value::Null),
                    "review_observation": value
                        .get("review_observation")
                        .cloned()
                        .unwrap_or(Value::Null),
                })),
            }),
        });
    }
    for chunk in text_stream_chunks(final_answer, 96) {
        events.push(RuntimeWorkflowStreamEvent {
            sequence: events.len() as u64,
            event_type: "content_delta".to_string(),
            profile: final_profile.to_string(),
            trace_id: trace_id.to_string(),
            content_delta: Some(chunk),
            output_ref: None,
            package_id: Some(package_id.to_string()),
            metadata: json!({"runtime": "tonglingyu"}),
        });
    }
    events.push(RuntimeWorkflowStreamEvent {
        sequence: events.len() as u64,
        event_type: "final_output".to_string(),
        profile: final_profile.to_string(),
        trace_id: trace_id.to_string(),
        content_delta: None,
        output_ref: steps.last().map(|step| step.output_ref.clone()),
        package_id: Some(package_id.to_string()),
        metadata: json!({"runtime": "tonglingyu"}),
    });
    events
}

fn text_stream_chunks(content: &str, max_chars: usize) -> Vec<String> {
    let chars = content.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars
        .chunks(max_chars)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}

pub fn init_runtime_schema(conn: &Connection) -> Result<()> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = apply_runtime_schema(conn);
        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        apply_runtime_schema(conn)
    }
}

fn apply_runtime_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            migration_id TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS evidence_claim_links (
            package_id TEXT NOT NULL,
            claim_index INTEGER NOT NULL,
            evidence_id TEXT NOT NULL,
            support_relation TEXT NOT NULL,
            PRIMARY KEY(package_id, claim_index, evidence_id)
        );

        CREATE TABLE IF NOT EXISTS evidence_claim_knowledge_links (
            package_id TEXT NOT NULL,
            claim_index INTEGER NOT NULL,
            evidence_id TEXT NOT NULL,
            item_id TEXT NOT NULL,
            state TEXT NOT NULL,
            policy_version TEXT NOT NULL,
            policy_decision TEXT NOT NULL,
            calibration_report_ref TEXT,
            display_label TEXT,
            created_at TEXT NOT NULL,
            PRIMARY KEY(package_id, claim_index, evidence_id, item_id)
        );

        CREATE TABLE IF NOT EXISTS evidence_cards (
            evidence_id TEXT PRIMARY KEY,
            package_id TEXT,
            evidence_type TEXT NOT NULL,
            source_id TEXT NOT NULL,
            block_id TEXT NOT NULL,
            support_scope TEXT NOT NULL,
            unsupported_scope TEXT NOT NULL,
            evidence_level TEXT NOT NULL,
            confidence TEXT NOT NULL,
            verification_status TEXT NOT NULL,
            evidence_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS evidence_packages (
            package_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            question TEXT NOT NULL,
            claim_statements_json TEXT NOT NULL,
            evidence_ids_json TEXT NOT NULL,
            review_status TEXT NOT NULL,
            review_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS review_records (
            review_id TEXT PRIMARY KEY,
            package_id TEXT NOT NULL REFERENCES evidence_packages(package_id),
            status TEXT NOT NULL,
            severity TEXT NOT NULL,
            issues_json TEXT NOT NULL,
            summary TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS audit_events (
            event_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS rqa_lifecycle_tombstones (
            tombstone_id TEXT PRIMARY KEY,
            object_type TEXT NOT NULL,
            object_id_sha256 TEXT NOT NULL,
            action TEXT NOT NULL,
            reason TEXT NOT NULL,
            policy_version TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS retrieval_failures (
            failure_id TEXT PRIMARY KEY,
            trace_id TEXT NOT NULL,
            package_id TEXT,
            question_sha256 TEXT NOT NULL,
            question_char_count INTEGER NOT NULL,
            question_summary TEXT NOT NULL,
            redacted_question_excerpt TEXT NOT NULL DEFAULT '',
            kb_schema_version TEXT NOT NULL,
            kb_version_id TEXT,
            failure_type TEXT NOT NULL,
            redacted_query_terms_json TEXT NOT NULL,
            required_evidence_types_json TEXT NOT NULL,
            actual_evidence_types_json TEXT NOT NULL,
            expected_evidence_ids_json TEXT NOT NULL,
            selected_evidence_ids_json TEXT NOT NULL,
            missing_evidence_types_json TEXT NOT NULL,
            quality_issues_json TEXT NOT NULL,
            agent_diagnosis TEXT,
            proposed_fix TEXT,
            human_review_status TEXT NOT NULL,
            reviewer TEXT,
            review_note TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            resolved_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_evidence_cards_package ON evidence_cards(package_id);
        CREATE INDEX IF NOT EXISTS idx_evidence_claim_knowledge_links_package
            ON evidence_claim_knowledge_links(package_id, claim_index);
        CREATE INDEX IF NOT EXISTS idx_evidence_claim_knowledge_links_item
            ON evidence_claim_knowledge_links(item_id);
        CREATE INDEX IF NOT EXISTS idx_audit_events_trace ON audit_events(trace_id);
        CREATE INDEX IF NOT EXISTS idx_rqa_lifecycle_tombstones_object
            ON rqa_lifecycle_tombstones(object_type, action, created_at);
        CREATE INDEX IF NOT EXISTS idx_rqa_lifecycle_tombstones_hash
            ON rqa_lifecycle_tombstones(object_id_sha256);
        CREATE INDEX IF NOT EXISTS idx_retrieval_failures_trace ON retrieval_failures(trace_id);
        CREATE INDEX IF NOT EXISTS idx_retrieval_failures_package ON retrieval_failures(package_id);
        CREATE INDEX IF NOT EXISTS idx_retrieval_failures_status ON retrieval_failures(human_review_status);
        CREATE INDEX IF NOT EXISTS idx_retrieval_failures_type ON retrieval_failures(failure_type);
        CREATE INDEX IF NOT EXISTS idx_retrieval_failures_created ON retrieval_failures(created_at);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_retrieval_failures_dedupe
            ON retrieval_failures(trace_id, IFNULL(package_id, ''), failure_type);
        "#,
    )?;
    migrate_retrieval_failure_privacy_schema(conn)?;
    conn.execute_batch(knowledge_governance_task_create_table_sql())?;
    migrate_knowledge_governance_task_source_entity_schema(conn)?;
    conn.execute_batch(knowledge_governance_task_indexes_sql())?;
    conn.execute_batch(knowledge_patch_proposal_schema_sql())?;
    conn.execute_batch(knowledge_item_state_schema_sql())?;
    migrate_knowledge_item_calibration_columns(conn)?;
    conn.execute_batch(knowledge_calibration_schema_sql())?;
    let backfilled_governance_tasks = conn.execute(
        r#"
        INSERT OR IGNORE INTO knowledge_governance_tasks (
            task_id, source_failure_id, source_entity_type, source_entity_id,
            trace_id, package_id, task_type, status, priority, agent_cluster_key,
            proposed_fix, reviewer, review_note, evidence_ref, created_at,
            updated_at, accepted_at, closed_at
        )
        SELECT
            'kgt-' || lower(hex(randomblob(16))),
            failure_id,
            'retrieval_failure',
            failure_id,
            trace_id,
            package_id,
            CASE
                WHEN failure_type = 'source_usage_metadata_incomplete'
                    THEN 'source_metadata_fix'
                WHEN failure_type = 'expected_evidence_missing'
                    THEN 'expected_evidence_fix'
                WHEN failure_type = 'reviewer_evidence_insufficient'
                    THEN 'expert_review'
                ELSE 'retrieval_policy_fix'
            END,
            'open',
            'p0',
            'rf:' || failure_type || ':q:' || substr(question_sha256, 1, 16),
            COALESCE(proposed_fix, agent_diagnosis, 'review_retrieval_failure_and_prepare_human_fix'),
            NULL,
            NULL,
            NULL,
            ?1,
            ?1,
            NULL,
            NULL
        FROM retrieval_failures
        WHERE human_review_status IN ('open', 'in_review')
        "#,
        params![now_rfc3339()],
    )?;
    if backfilled_governance_tasks > 0 {
        append_runtime_audit_event(
            conn,
            "governance-backfill",
            "governance_tasks_backfilled",
            &json!({
                "task_count": backfilled_governance_tasks,
                "source": "runtime_schema_migration",
                "schema_version": KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION,
            }),
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params!["tonglingyu-runtime-schema-v1", now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![RETRIEVAL_FAILURE_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![RETRIEVAL_FAILURE_DEDUPE_MIGRATION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![RETRIEVAL_FAILURE_PRIVACY_MIGRATION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![KNOWLEDGE_GOVERNANCE_TASK_BACKFILL_MIGRATION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![KNOWLEDGE_ITEM_CALIBRATION_LINK_MIGRATION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![KNOWLEDGE_CALIBRATION_REPORT_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![KNOWLEDGE_CALIBRATION_JOB_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![KNOWLEDGE_RUNTIME_POLICY_SCHEMA_VERSION, now_rfc3339()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params![RQA_LIFECYCLE_POLICY_VERSION, now_rfc3339()],
    )?;
    Ok(())
}

fn knowledge_governance_task_create_table_sql() -> &'static str {
    r#"
    CREATE TABLE IF NOT EXISTS knowledge_governance_tasks (
        task_id TEXT PRIMARY KEY,
        source_failure_id TEXT REFERENCES retrieval_failures(failure_id),
        source_entity_type TEXT NOT NULL,
        source_entity_id TEXT NOT NULL,
        trace_id TEXT NOT NULL,
        package_id TEXT,
        task_type TEXT NOT NULL,
        status TEXT NOT NULL,
        priority TEXT NOT NULL,
        agent_cluster_key TEXT NOT NULL,
        proposed_fix TEXT NOT NULL,
        reviewer TEXT,
        review_note TEXT,
        evidence_ref TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        accepted_at TEXT,
        closed_at TEXT
    );
    "#
}

fn knowledge_governance_task_indexes_sql() -> &'static str {
    r#"
    CREATE UNIQUE INDEX IF NOT EXISTS idx_governance_tasks_failure_type
        ON knowledge_governance_tasks(source_failure_id, task_type);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_governance_tasks_entity_type
        ON knowledge_governance_tasks(source_entity_type, source_entity_id, task_type);
    CREATE INDEX IF NOT EXISTS idx_governance_tasks_entity
        ON knowledge_governance_tasks(source_entity_type, source_entity_id);
    CREATE INDEX IF NOT EXISTS idx_governance_tasks_trace
        ON knowledge_governance_tasks(trace_id);
    CREATE INDEX IF NOT EXISTS idx_governance_tasks_package
        ON knowledge_governance_tasks(package_id);
    CREATE INDEX IF NOT EXISTS idx_governance_tasks_status
        ON knowledge_governance_tasks(status);
    CREATE INDEX IF NOT EXISTS idx_governance_tasks_type
        ON knowledge_governance_tasks(task_type);
    CREATE INDEX IF NOT EXISTS idx_governance_tasks_priority
        ON knowledge_governance_tasks(priority);
    CREATE INDEX IF NOT EXISTS idx_governance_tasks_updated
        ON knowledge_governance_tasks(updated_at);
    "#
}

fn knowledge_patch_proposal_schema_sql() -> &'static str {
    r#"
    CREATE TABLE IF NOT EXISTS knowledge_patch_proposals (
        proposal_id TEXT PRIMARY KEY,
        proposal_type TEXT NOT NULL,
        trace_id TEXT NOT NULL,
        package_id TEXT,
        source_ref TEXT NOT NULL,
        payload_json TEXT NOT NULL,
        payload_sha256 TEXT NOT NULL,
        task_id TEXT NOT NULL REFERENCES knowledge_governance_tasks(task_id),
        created_by TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(proposal_type, source_ref, payload_sha256)
    );
    CREATE INDEX IF NOT EXISTS idx_knowledge_patch_proposals_type
        ON knowledge_patch_proposals(proposal_type);
    CREATE INDEX IF NOT EXISTS idx_knowledge_patch_proposals_trace
        ON knowledge_patch_proposals(trace_id);
    CREATE INDEX IF NOT EXISTS idx_knowledge_patch_proposals_package
        ON knowledge_patch_proposals(package_id);
    CREATE INDEX IF NOT EXISTS idx_knowledge_patch_proposals_task
        ON knowledge_patch_proposals(task_id);
    CREATE INDEX IF NOT EXISTS idx_knowledge_patch_proposals_updated
        ON knowledge_patch_proposals(updated_at);
    "#
}

fn knowledge_item_state_schema_sql() -> &'static str {
    r#"
    CREATE TABLE IF NOT EXISTS knowledge_items (
        item_id TEXT PRIMARY KEY,
        kind TEXT NOT NULL,
        state TEXT NOT NULL,
        source_refs_json TEXT NOT NULL,
        evidence_refs_json TEXT NOT NULL,
        payload_json TEXT NOT NULL,
        payload_sha256 TEXT NOT NULL,
        schema_version TEXT NOT NULL,
        source_boundary_json TEXT,
        calibration_report_ref TEXT,
        confidence REAL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        state_version INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS knowledge_item_state_history (
        history_id TEXT PRIMARY KEY,
        item_id TEXT NOT NULL REFERENCES knowledge_items(item_id),
        previous_state TEXT,
        new_state TEXT NOT NULL,
        actor TEXT NOT NULL,
        reason_sha256 TEXT NOT NULL,
        evidence_refs_json TEXT NOT NULL,
        state_version INTEGER NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_knowledge_items_kind
        ON knowledge_items(kind);
    CREATE INDEX IF NOT EXISTS idx_knowledge_items_state
        ON knowledge_items(state);
    CREATE INDEX IF NOT EXISTS idx_knowledge_items_updated
        ON knowledge_items(updated_at, item_id);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_items_identity
        ON knowledge_items(kind, payload_sha256, source_refs_json);
    CREATE INDEX IF NOT EXISTS idx_knowledge_item_state_history_item
        ON knowledge_item_state_history(item_id, state_version);
    CREATE INDEX IF NOT EXISTS idx_knowledge_item_state_history_created
        ON knowledge_item_state_history(created_at);
    "#
}

fn migrate_knowledge_item_calibration_columns(conn: &Connection) -> Result<()> {
    if !sqlite_table_exists(conn, "knowledge_items")? {
        return Ok(());
    }
    let columns = sqlite_table_columns(conn, "knowledge_items")?;
    for (column, definition) in [
        ("source_boundary_json", "TEXT"),
        ("calibration_report_ref", "TEXT"),
        ("confidence", "REAL"),
    ] {
        if !columns.contains(column) {
            conn.execute(
                &format!("ALTER TABLE knowledge_items ADD COLUMN {column} {definition}"),
                [],
            )?;
        }
    }
    Ok(())
}

fn knowledge_calibration_schema_sql() -> &'static str {
    r#"
    CREATE TABLE IF NOT EXISTS knowledge_calibration_reports (
        report_id TEXT PRIMARY KEY,
        report_ref TEXT NOT NULL UNIQUE,
        item_id TEXT NOT NULL REFERENCES knowledge_items(item_id),
        kind TEXT NOT NULL,
        method TEXT NOT NULL,
        decision TEXT NOT NULL,
        confidence REAL NOT NULL,
        quality_issues_json TEXT NOT NULL,
        source_refs_json TEXT NOT NULL,
        evidence_refs_json TEXT NOT NULL,
        source_boundary_json TEXT NOT NULL,
        coverage_matrix_json TEXT NOT NULL,
        config_summary_json TEXT,
        report_json TEXT NOT NULL,
        report_hash TEXT NOT NULL,
        schema_version TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS knowledge_calibration_jobs (
        job_id TEXT PRIMARY KEY,
        status TEXT NOT NULL,
        input_kind TEXT NOT NULL,
        input_ref TEXT NOT NULL,
        item_id TEXT NOT NULL REFERENCES knowledge_items(item_id),
        input_digest TEXT NOT NULL,
        idempotency_key TEXT NOT NULL UNIQUE,
        trace_id TEXT NOT NULL,
        method TEXT NOT NULL,
        config_digest TEXT,
        retry_limit INTEGER NOT NULL,
        attempt_count INTEGER NOT NULL,
        concurrency_key TEXT NOT NULL,
        lease_owner TEXT,
        lease_expires_at TEXT,
        heartbeat_at TEXT,
        report_id TEXT REFERENCES knowledge_calibration_reports(report_id),
        last_error_sha256 TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS knowledge_calibration_job_history (
        history_id TEXT PRIMARY KEY,
        job_id TEXT NOT NULL REFERENCES knowledge_calibration_jobs(job_id),
        previous_status TEXT,
        new_status TEXT NOT NULL,
        actor TEXT NOT NULL,
        reason_sha256 TEXT NOT NULL,
        lease_owner TEXT,
        report_id TEXT,
        attempt_count INTEGER NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_knowledge_calibration_reports_item
        ON knowledge_calibration_reports(item_id, created_at);
    CREATE INDEX IF NOT EXISTS idx_knowledge_calibration_reports_method
        ON knowledge_calibration_reports(method);
    CREATE INDEX IF NOT EXISTS idx_knowledge_calibration_reports_decision
        ON knowledge_calibration_reports(decision);
    CREATE INDEX IF NOT EXISTS idx_knowledge_calibration_jobs_status
        ON knowledge_calibration_jobs(status);
    CREATE INDEX IF NOT EXISTS idx_knowledge_calibration_jobs_item
        ON knowledge_calibration_jobs(item_id);
    CREATE INDEX IF NOT EXISTS idx_knowledge_calibration_jobs_concurrency
        ON knowledge_calibration_jobs(concurrency_key, status);
    CREATE INDEX IF NOT EXISTS idx_knowledge_calibration_jobs_trace
        ON knowledge_calibration_jobs(trace_id);
    CREATE INDEX IF NOT EXISTS idx_knowledge_calibration_job_history_job
        ON knowledge_calibration_job_history(job_id, created_at);
    "#
}

fn migrate_retrieval_failure_privacy_schema(conn: &Connection) -> Result<()> {
    if !sqlite_table_exists(conn, "retrieval_failures")? {
        return Ok(());
    }
    let columns = sqlite_table_columns(conn, "retrieval_failures")?;
    if columns.contains("question") {
        return Err(anyhow!(
            "retrieval_failures contains raw question column; manual privacy migration required"
        ));
    }
    if !columns.contains("redacted_question_excerpt") {
        conn.execute_batch(
            r#"
            ALTER TABLE retrieval_failures
                ADD COLUMN redacted_question_excerpt TEXT NOT NULL DEFAULT '';
            UPDATE retrieval_failures
                SET redacted_question_excerpt = question_summary
                WHERE redacted_question_excerpt = '';
            "#,
        )?;
    }
    Ok(())
}

fn migrate_knowledge_governance_task_source_entity_schema(conn: &Connection) -> Result<()> {
    if !sqlite_table_exists(conn, "knowledge_governance_tasks")? {
        return Ok(());
    }
    let columns = sqlite_table_columns(conn, "knowledge_governance_tasks")?;
    if columns.contains("source_entity_type") && columns.contains("source_entity_id") {
        return Ok(());
    }
    let required_legacy_columns = [
        "task_id",
        "source_failure_id",
        "trace_id",
        "package_id",
        "task_type",
        "status",
        "priority",
        "agent_cluster_key",
        "proposed_fix",
        "reviewer",
        "review_note",
        "evidence_ref",
        "created_at",
        "updated_at",
        "accepted_at",
        "closed_at",
    ];
    let missing_columns = required_legacy_columns
        .iter()
        .filter(|column| !columns.contains(**column))
        .copied()
        .collect::<Vec<_>>();
    if !missing_columns.is_empty() {
        return Err(anyhow!(
            "knowledge_governance_tasks cannot migrate to source entity schema; missing legacy columns: {}",
            missing_columns.join(",")
        ));
    }
    if sqlite_table_exists(
        conn,
        "knowledge_governance_tasks_legacy_source_entity_migration",
    )? {
        return Err(anyhow!(
            "knowledge governance task source entity migration scratch table already exists"
        ));
    }

    conn.execute_batch(
        r#"
        DROP INDEX IF EXISTS idx_governance_tasks_failure_type;
        DROP INDEX IF EXISTS idx_governance_tasks_entity_type;
        DROP INDEX IF EXISTS idx_governance_tasks_entity;
        DROP INDEX IF EXISTS idx_governance_tasks_trace;
        DROP INDEX IF EXISTS idx_governance_tasks_package;
        DROP INDEX IF EXISTS idx_governance_tasks_status;
        DROP INDEX IF EXISTS idx_governance_tasks_type;
        DROP INDEX IF EXISTS idx_governance_tasks_priority;
        DROP INDEX IF EXISTS idx_governance_tasks_updated;
        ALTER TABLE knowledge_governance_tasks
            RENAME TO knowledge_governance_tasks_legacy_source_entity_migration;
        "#,
    )?;
    conn.execute_batch(knowledge_governance_task_create_table_sql())?;
    conn.execute_batch(
        r#"
        INSERT INTO knowledge_governance_tasks (
            task_id, source_failure_id, source_entity_type, source_entity_id,
            trace_id, package_id, task_type, status, priority, agent_cluster_key,
            proposed_fix, reviewer, review_note, evidence_ref, created_at,
            updated_at, accepted_at, closed_at
        )
        SELECT
            task_id,
            source_failure_id,
            'retrieval_failure',
            source_failure_id,
            trace_id,
            package_id,
            task_type,
            status,
            priority,
            agent_cluster_key,
            proposed_fix,
            reviewer,
            review_note,
            evidence_ref,
            created_at,
            updated_at,
            accepted_at,
            closed_at
        FROM knowledge_governance_tasks_legacy_source_entity_migration;

        DROP TABLE knowledge_governance_tasks_legacy_source_entity_migration;
        "#,
    )?;
    Ok(())
}

pub fn runtime_schema_migration_preflight(conn: &Connection) -> Result<Value> {
    let required_migrations = runtime_schema_required_migrations();
    let applied_migrations = if sqlite_table_exists(conn, "schema_migrations")? {
        collect_schema_migrations(conn)?
    } else {
        Vec::new()
    };
    let applied_set = applied_migrations.iter().cloned().collect::<BTreeSet<_>>();
    let pending_migrations = required_migrations
        .iter()
        .filter(|migration| !applied_set.contains(*migration))
        .map(|migration| migration.to_string())
        .collect::<Vec<_>>();
    Ok(json!({
        "object": "tonglingyu.runtime_schema_migration_preflight",
        "required_migrations": required_migrations,
        "applied_migrations": applied_migrations,
        "pending_migrations": pending_migrations,
        "will_rebuild_knowledge_base": false,
        "will_delete_runtime_data": false,
        "contains_secret_values": false,
    }))
}

fn runtime_schema_required_migrations() -> Vec<String> {
    vec![
        "tonglingyu-runtime-schema-v1".to_string(),
        RETRIEVAL_FAILURE_SCHEMA_VERSION.to_string(),
        RETRIEVAL_FAILURE_DEDUPE_MIGRATION.to_string(),
        RETRIEVAL_FAILURE_PRIVACY_MIGRATION.to_string(),
        KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION.to_string(),
        KNOWLEDGE_GOVERNANCE_TASK_BACKFILL_MIGRATION.to_string(),
        KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION.to_string(),
        KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION.to_string(),
        KNOWLEDGE_ITEM_CALIBRATION_LINK_MIGRATION.to_string(),
        KNOWLEDGE_CALIBRATION_REPORT_SCHEMA_VERSION.to_string(),
        KNOWLEDGE_CALIBRATION_JOB_SCHEMA_VERSION.to_string(),
        KNOWLEDGE_RUNTIME_POLICY_SCHEMA_VERSION.to_string(),
        RQA_LIFECYCLE_POLICY_VERSION.to_string(),
    ]
}

fn collect_schema_migrations(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT migration_id FROM schema_migrations ORDER BY migration_id")?;
    stmt.query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn has_knowledge_base(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let conn =
        Connection::open(path).with_context(|| format!("open sqlite db {}", path.display()))?;
    let count: Option<i64> = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='kb_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if count.unwrap_or_default() == 0 {
        return Ok(false);
    }
    let sources: i64 = conn
        .query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))
        .unwrap_or_default();
    Ok(sources > 0)
}

pub fn runtime_store_stats(conn: &Connection) -> Result<RuntimeStoreStats> {
    Ok(RuntimeStoreStats {
        sources: table_count(conn, "sources")?,
        blocks: table_count(conn, "blocks")?,
        evidence_packages: table_count(conn, "evidence_packages")?,
        evidence_cards: table_count(conn, "evidence_cards")?,
        retrieval_failures: table_count(conn, "retrieval_failures")?,
        governance_tasks: table_count(conn, "knowledge_governance_tasks")?,
        knowledge_patch_proposals: table_count(conn, "knowledge_patch_proposals")?,
        knowledge_items: table_count(conn, "knowledge_items")?,
        knowledge_item_state_history: table_count(conn, "knowledge_item_state_history")?,
        knowledge_calibration_reports: table_count(conn, "knowledge_calibration_reports")?,
        knowledge_calibration_jobs: table_count(conn, "knowledge_calibration_jobs")?,
        knowledge_calibration_job_history: table_count(conn, "knowledge_calibration_job_history")?,
        audit_events: table_count(conn, "audit_events")?,
        review_status: grouped_count_map(
            conn,
            "SELECT review_status, COUNT(*) FROM evidence_packages GROUP BY review_status",
        )?,
        evidence_types: grouped_count_map(
            conn,
            "SELECT evidence_type, COUNT(*) FROM evidence_cards GROUP BY evidence_type",
        )?,
        retrieval_failure_status: grouped_count_map(
            conn,
            "SELECT human_review_status, COUNT(*) FROM retrieval_failures GROUP BY human_review_status",
        )?,
        retrieval_failure_type: grouped_count_map(
            conn,
            "SELECT failure_type, COUNT(*) FROM retrieval_failures GROUP BY failure_type",
        )?,
        governance_task_status: grouped_count_map(
            conn,
            "SELECT status, COUNT(*) FROM knowledge_governance_tasks GROUP BY status",
        )?,
        governance_task_type: grouped_count_map(
            conn,
            "SELECT task_type, COUNT(*) FROM knowledge_governance_tasks GROUP BY task_type",
        )?,
        governance_task_priority: grouped_count_map(
            conn,
            "SELECT priority, COUNT(*) FROM knowledge_governance_tasks GROUP BY priority",
        )?,
        knowledge_item_state: grouped_count_map(
            conn,
            "SELECT state, COUNT(*) FROM knowledge_items GROUP BY state",
        )?,
        knowledge_item_kind: grouped_count_map(
            conn,
            "SELECT kind, COUNT(*) FROM knowledge_items GROUP BY kind",
        )?,
        knowledge_calibration_report_method: grouped_count_map(
            conn,
            "SELECT method, COUNT(*) FROM knowledge_calibration_reports GROUP BY method",
        )?,
        knowledge_calibration_report_decision: grouped_count_map(
            conn,
            "SELECT decision, COUNT(*) FROM knowledge_calibration_reports GROUP BY decision",
        )?,
        knowledge_calibration_job_status: grouped_count_map(
            conn,
            "SELECT status, COUNT(*) FROM knowledge_calibration_jobs GROUP BY status",
        )?,
        audit_event_types: grouped_count_map(
            conn,
            "SELECT event_type, COUNT(*) FROM audit_events GROUP BY event_type",
        )?,
    })
}

pub fn runtime_package_ids_for_trace(conn: &Connection, trace_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT package_id FROM evidence_packages WHERE trace_id = ?1")?;
    stmt.query_map(params![trace_id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn runtime_audit_events_for_trace(conn: &Connection, trace_id: &str) -> Result<Vec<Value>> {
    load_rows_json(
        conn,
        "SELECT event_id, event_type, payload_json, created_at FROM audit_events WHERE trace_id = ?1 ORDER BY created_at, event_id",
        trace_id,
    )
}

pub fn create_retrieval_failure(
    conn: &Connection,
    input: RetrievalFailureCreateInput,
) -> Result<RetrievalFailureRecord> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = create_retrieval_failure_inner(conn, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        create_retrieval_failure_inner(conn, input)
    }
}

fn create_retrieval_failure_inner(
    conn: &Connection,
    input: RetrievalFailureCreateInput,
) -> Result<RetrievalFailureRecord> {
    let expected_missing =
        expected_evidence_missing(&input.expected_evidence_ids, &input.selected_evidence_ids);
    if input.quality_report.production_ready && expected_missing.is_empty() {
        return Err(anyhow!(
            "retrieval failure requires a non-production-ready quality report"
        ));
    }
    if input.quality_report.query_summary.question_sha256 != hash_text(&input.question) {
        return Err(anyhow!(
            "retrieval failure question hash does not match quality report"
        ));
    }
    let failure_id = format!("rf-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    let mut quality_issues = input.quality_report.issues.clone();
    if !expected_missing.is_empty() {
        quality_issues.push(format!(
            "expected_evidence_missing:{}",
            expected_missing.join(",")
        ));
    }
    let failure_type = retrieval_failure_type(&quality_issues);
    if let Some(existing) = load_retrieval_failure_by_dedupe(
        conn,
        &input.trace_id,
        input.package_id.as_deref(),
        &failure_type,
    )? {
        if governance_task_required_for_failure(&existing) {
            let _ = ensure_governance_task_for_failure(conn, &existing)?;
        }
        return Ok(existing);
    }
    let question_summary = retrieval_failure_question_summary(&input.quality_report);
    let redacted_question_excerpt = retrieval_failure_redacted_excerpt(&input.quality_report);
    let kb_version_id = latest_kb_version_id(conn)?;
    let agent_diagnosis = input.agent_diagnosis.unwrap_or_else(|| {
        format!(
            "quality_status={}; production_ready={}; issue_count={}",
            input.quality_report.quality_status,
            input.quality_report.production_ready,
            quality_issues.len()
        )
    });
    let proposed_fix = input.proposed_fix.or_else(|| {
        if input.quality_report.recommended_follow_up.is_empty() {
            None
        } else {
            Some(input.quality_report.recommended_follow_up.join(","))
        }
    });
    conn.execute(
        r#"
        INSERT INTO retrieval_failures (
            failure_id, trace_id, package_id, question_sha256, question_char_count,
            question_summary, redacted_question_excerpt, kb_schema_version,
            kb_version_id, failure_type, redacted_query_terms_json, required_evidence_types_json,
            actual_evidence_types_json, expected_evidence_ids_json,
            selected_evidence_ids_json, missing_evidence_types_json,
            quality_issues_json, agent_diagnosis, proposed_fix, human_review_status,
            reviewer, review_note, created_at, updated_at, resolved_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
            ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25
        )
        "#,
        params![
            failure_id,
            input.trace_id,
            input.package_id,
            input.quality_report.query_summary.question_sha256,
            input.quality_report.query_summary.question_char_count as i64,
            question_summary,
            redacted_question_excerpt,
            input
                .quality_report
                .source_coverage_boundary
                .kb_schema_version,
            kb_version_id,
            failure_type,
            serde_json::to_string(&input.quality_report.query_summary.redacted_terms)?,
            serde_json::to_string(&input.quality_report.evidence_type_coverage.required)?,
            serde_json::to_string(&input.quality_report.evidence_type_coverage.selected)?,
            serde_json::to_string(&input.expected_evidence_ids)?,
            serde_json::to_string(&input.selected_evidence_ids)?,
            serde_json::to_string(&input.quality_report.evidence_type_coverage.missing)?,
            serde_json::to_string(&quality_issues)?,
            agent_diagnosis,
            proposed_fix,
            "open",
            Option::<String>::None,
            Option::<String>::None,
            now,
            now,
            Option::<String>::None,
        ],
    )?;
    let record = load_retrieval_failure(conn, &failure_id)?
        .ok_or_else(|| anyhow!("retrieval failure was not readable after insert"))?;
    let governance_task = ensure_governance_task_for_failure(conn, &record)?;
    append_runtime_audit_event(
        conn,
        &record.trace_id,
        "retrieval_failure_recorded",
        &json!({
            "failure_id": &record.failure_id,
            "package_id": &record.package_id,
            "failure_type": &record.failure_type,
            "human_review_status": &record.human_review_status,
            "question_sha256": &record.question_sha256,
            "missing_evidence_types": &record.missing_evidence_types,
            "quality_issue_count": record.quality_issues.len(),
            "governance_task_id": governance_task.as_ref().map(|task| &task.task_id),
        }),
    )?;
    Ok(record)
}

pub fn list_retrieval_failures(
    conn: &Connection,
    input: RetrievalFailureListInput,
) -> Result<RetrievalFailureListResult> {
    let limit = retrieval_failure_page_limit(input.limit);
    let offset = input.offset;
    let limit_i64 = limit as i64;
    let offset_i64 = offset as i64;
    let base_sql = retrieval_failure_select_sql();
    let records = match (&input.human_review_status, &input.failure_type) {
        (Some(status), Some(failure_type)) => {
            validate_human_review_status(status)?;
            query_retrieval_failure_records(
                conn,
                &format!(
                    "{base_sql} WHERE human_review_status = ?1 AND failure_type = ?2 ORDER BY created_at DESC, failure_id DESC LIMIT ?3 OFFSET ?4"
                ),
                &[status as &dyn ToSql, failure_type, &limit_i64, &offset_i64],
            )?
        }
        (Some(status), None) => {
            validate_human_review_status(status)?;
            query_retrieval_failure_records(
                conn,
                &format!(
                    "{base_sql} WHERE human_review_status = ?1 ORDER BY created_at DESC, failure_id DESC LIMIT ?2 OFFSET ?3"
                ),
                &[status as &dyn ToSql, &limit_i64, &offset_i64],
            )?
        }
        (None, Some(failure_type)) => query_retrieval_failure_records(
            conn,
            &format!(
                "{base_sql} WHERE failure_type = ?1 ORDER BY created_at DESC, failure_id DESC LIMIT ?2 OFFSET ?3"
            ),
            &[failure_type as &dyn ToSql, &limit_i64, &offset_i64],
        )?,
        (None, None) => query_retrieval_failure_records(
            conn,
            &format!("{base_sql} ORDER BY created_at DESC, failure_id DESC LIMIT ?1 OFFSET ?2"),
            &[&limit_i64 as &dyn ToSql, &offset_i64],
        )?,
    };
    let next_offset = if records.len() == limit {
        Some(offset + limit)
    } else {
        None
    };
    Ok(RetrievalFailureListResult {
        object: "tonglingyu.retrieval_failure_list".to_string(),
        schema_version: RETRIEVAL_FAILURE_SCHEMA_VERSION.to_string(),
        limit,
        offset,
        next_offset,
        items: records
            .iter()
            .map(|record| retrieval_failure_record_json(record, input.view))
            .collect(),
    })
}

pub fn cluster_retrieval_failures(
    conn: &Connection,
    input: RetrievalFailureClusterInput,
) -> Result<RetrievalFailureClusterResult> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = cluster_retrieval_failures_inner(conn, input);
        match result {
            Ok(result) => {
                conn.execute_batch("COMMIT")?;
                Ok(result)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        cluster_retrieval_failures_inner(conn, input)
    }
}

fn cluster_retrieval_failures_inner(
    conn: &Connection,
    input: RetrievalFailureClusterInput,
) -> Result<RetrievalFailureClusterResult> {
    if let Some(status) = input.human_review_status.as_ref() {
        validate_human_review_status(status)?;
    }
    let limit = retrieval_failure_cluster_limit(input.limit);
    let min_cluster_size = input.min_cluster_size.max(2);
    let failures = query_retrieval_failures_for_clustering(
        conn,
        input.human_review_status.as_deref(),
        input.failure_type.as_deref(),
        limit,
    )?;
    let scanned_failure_count = failures.len();
    let mut grouped = BTreeMap::<String, Vec<RetrievalFailureRecord>>::new();
    for failure in failures {
        let cluster_key = retrieval_failure_cluster_key(&failure);
        grouped.entry(cluster_key).or_default().push(failure);
    }

    let mut clusters = Vec::new();
    let mut task_count = 0_usize;
    for (cluster_key, mut failures) in grouped {
        if failures.len() < min_cluster_size {
            continue;
        }
        failures.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.failure_id.cmp(&right.failure_id))
        });
        let proposed_fix = retrieval_failure_cluster_proposed_fix(&failures);
        let task = if input.create_tasks {
            let task = create_governance_task(
                conn,
                KnowledgeGovernanceTaskCreateInput {
                    source_entity_type: "retrieval_failure_cluster".to_string(),
                    source_entity_id: cluster_key.clone(),
                    trace_id: format!("cluster-{}", &hash_text(&cluster_key)[..16]),
                    package_id: None,
                    source_failure_id: None,
                    task_type: default_governance_task_type(&failures[0]),
                    priority: Some("p0".to_string()),
                    proposed_fix: Some(proposed_fix.clone()),
                    agent_cluster_key: Some(cluster_key.clone()),
                },
            )?;
            task_count += 1;
            Some(task)
        } else {
            None
        };
        let cluster_value =
            retrieval_failure_cluster_json(&cluster_key, &failures, &proposed_fix, task.as_ref());
        append_runtime_audit_event(
            conn,
            &format!("cluster-{}", &hash_text(&cluster_key)[..16]),
            "retrieval_failures_clustered",
            &json!({
                "cluster_key": &cluster_key,
                "failure_type": &failures[0].failure_type,
                "failure_count": failures.len(),
                "task_id": task.as_ref().map(|task| &task.task_id),
                "task_attached": task.is_some(),
                "direct_fact_mutation": false,
                "proposed_fix_sha256": hash_text(&proposed_fix),
                "representative_failure_id_sha256": hash_text(&failures[0].failure_id),
            }),
        )?;
        clusters.push(cluster_value);
    }
    let cluster_count = clusters.len();
    Ok(RetrievalFailureClusterResult {
        object: "tonglingyu.retrieval_failure_cluster_result".to_string(),
        schema_version: RETRIEVAL_FAILURE_CLUSTER_SCHEMA_VERSION.to_string(),
        scanned_failure_count,
        cluster_count,
        task_count,
        min_cluster_size,
        limit,
        create_tasks: input.create_tasks,
        clusters,
    })
}

pub fn read_retrieval_failure(
    conn: &Connection,
    failure_id: &str,
    view: RetrievalFailureView,
) -> Result<Option<Value>> {
    Ok(load_retrieval_failure(conn, failure_id)?
        .as_ref()
        .map(|record| retrieval_failure_record_json(record, view)))
}

pub fn list_retrieval_failures_for_trace(
    conn: &Connection,
    trace_id: &str,
    view: RetrievalFailureView,
    limit: usize,
) -> Result<Vec<Value>> {
    let limit = retrieval_failure_page_limit(limit);
    let limit_i64 = limit as i64;
    let sql = format!(
        "{} WHERE trace_id = ?1 ORDER BY created_at DESC, failure_id DESC LIMIT ?2",
        retrieval_failure_select_sql()
    );
    Ok(
        query_retrieval_failure_records(conn, &sql, &[&trace_id, &limit_i64])?
            .iter()
            .map(|record| retrieval_failure_record_json(record, view))
            .collect(),
    )
}

pub fn list_retrieval_failures_for_package(
    conn: &Connection,
    package_id: &str,
    view: RetrievalFailureView,
    limit: usize,
) -> Result<Vec<Value>> {
    let limit = retrieval_failure_page_limit(limit);
    let limit_i64 = limit as i64;
    let sql = format!(
        "{} WHERE package_id = ?1 ORDER BY created_at DESC, failure_id DESC LIMIT ?2",
        retrieval_failure_select_sql()
    );
    Ok(
        query_retrieval_failure_records(conn, &sql, &[&package_id, &limit_i64])?
            .iter()
            .map(|record| retrieval_failure_record_json(record, view))
            .collect(),
    )
}

pub fn update_retrieval_failure_status(
    conn: &Connection,
    failure_id: &str,
    human_review_status: &str,
    reviewer: Option<&str>,
    review_note: Option<&str>,
) -> Result<Option<RetrievalFailureRecord>> {
    update_retrieval_failure_status_checked(
        conn,
        failure_id,
        human_review_status,
        reviewer,
        review_note,
        None,
    )
}

pub fn update_retrieval_failure_status_checked(
    conn: &Connection,
    failure_id: &str,
    human_review_status: &str,
    reviewer: Option<&str>,
    review_note: Option<&str>,
    expected_updated_at: Option<&str>,
) -> Result<Option<RetrievalFailureRecord>> {
    validate_human_review_status(human_review_status)?;
    let now = now_rfc3339();
    let resolved_at = if matches!(human_review_status, "resolved" | "wontfix") {
        Some(now.clone())
    } else {
        None
    };
    let reviewer = reviewer.and_then(|value| bounded_optional_text(value, 80));
    let review_note = review_note.and_then(|value| bounded_optional_text(value, 480));
    let current = load_retrieval_failure(conn, failure_id)?;
    let previous_status = current
        .as_ref()
        .map(|record| record.human_review_status.clone());
    if let Some(current) = current.as_ref() {
        let same_payload = current.human_review_status == human_review_status
            && current.reviewer == reviewer
            && current.review_note == review_note;
        if same_payload && expected_updated_at.is_none() {
            return Ok(Some(current.clone()));
        }
    }
    let updated = conn.execute(
        r#"
        UPDATE retrieval_failures
        SET human_review_status = ?2,
            reviewer = ?3,
            review_note = ?4,
            updated_at = ?5,
            resolved_at = ?6
        WHERE failure_id = ?1
          AND (?7 IS NULL OR updated_at = ?7)
        "#,
        params![
            failure_id,
            human_review_status,
            reviewer,
            review_note,
            now,
            resolved_at,
            expected_updated_at,
        ],
    )?;
    if updated == 0 {
        if expected_updated_at.is_some() && load_retrieval_failure(conn, failure_id)?.is_some() {
            return Err(anyhow!("retrieval failure update conflict"));
        }
        return Ok(None);
    }
    let record = load_retrieval_failure(conn, failure_id)?
        .ok_or_else(|| anyhow!("retrieval failure disappeared after update"))?;
    append_runtime_audit_event(
        conn,
        &record.trace_id,
        "retrieval_failure_status_updated",
        &json!({
            "failure_id": &record.failure_id,
            "previous_status": previous_status,
            "new_status": &record.human_review_status,
            "human_review_status": &record.human_review_status,
            "reviewer": &record.reviewer,
            "review_note_sha256": record.review_note.as_deref().map(hash_text),
            "status_history": {
                "previous_status": previous_status,
                "new_status": &record.human_review_status,
                "reason_sha256": record.review_note.as_deref().map(hash_text),
                "timestamp": &record.updated_at,
            },
            "resolved_at": &record.resolved_at,
        }),
    )?;
    Ok(Some(record))
}

pub fn create_governance_task_from_failure(
    conn: &Connection,
    input: KnowledgeGovernanceTaskCreateFromFailureInput,
) -> Result<Option<KnowledgeGovernanceTaskRecord>> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = create_governance_task_from_failure_inner(conn, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        create_governance_task_from_failure_inner(conn, input)
    }
}

fn create_governance_task_from_failure_inner(
    conn: &Connection,
    input: KnowledgeGovernanceTaskCreateFromFailureInput,
) -> Result<Option<KnowledgeGovernanceTaskRecord>> {
    let Some(failure) = load_retrieval_failure(conn, &input.source_failure_id)? else {
        return Ok(None);
    };
    create_governance_task_for_failure_inner(conn, &failure, input).map(Some)
}

fn ensure_governance_task_for_failure(
    conn: &Connection,
    failure: &RetrievalFailureRecord,
) -> Result<Option<KnowledgeGovernanceTaskRecord>> {
    if !governance_task_required_for_failure(failure) {
        return Ok(None);
    }
    create_governance_task_for_failure_inner(
        conn,
        failure,
        KnowledgeGovernanceTaskCreateFromFailureInput {
            source_failure_id: failure.failure_id.clone(),
            task_type: None,
            priority: None,
            proposed_fix: None,
            agent_cluster_key: None,
        },
    )
    .map(Some)
}

fn create_governance_task_for_failure_inner(
    conn: &Connection,
    failure: &RetrievalFailureRecord,
    input: KnowledgeGovernanceTaskCreateFromFailureInput,
) -> Result<KnowledgeGovernanceTaskRecord> {
    let task_type = input
        .task_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_governance_task_type(failure));
    validate_governance_task_type(&task_type)?;
    let priority = input
        .priority
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_governance_task_priority(failure));
    validate_governance_task_priority(&priority)?;
    if let Some(existing) = load_governance_task_by_entity_type(
        conn,
        "retrieval_failure",
        &failure.failure_id,
        &task_type,
    )? {
        return Ok(existing);
    }
    let agent_cluster_key = input
        .agent_cluster_key
        .as_deref()
        .and_then(|value| bounded_optional_text(value, 160))
        .unwrap_or_else(|| default_governance_cluster_key(failure));
    let proposed_fix = input
        .proposed_fix
        .as_deref()
        .and_then(|value| bounded_optional_text(value, 480))
        .or_else(|| {
            failure
                .proposed_fix
                .as_deref()
                .and_then(|value| bounded_optional_text(value, 480))
        })
        .or_else(|| {
            failure
                .agent_diagnosis
                .as_deref()
                .and_then(|value| bounded_optional_text(value, 480))
        })
        .unwrap_or_else(|| "review_retrieval_failure_and_prepare_human_fix".to_string());
    let task_id = format!("kgt-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO knowledge_governance_tasks (
            task_id, source_failure_id, source_entity_type, source_entity_id,
            trace_id, package_id, task_type, status, priority, agent_cluster_key,
            proposed_fix, reviewer, review_note, evidence_ref, created_at,
            updated_at, accepted_at, closed_at
        ) VALUES (
            ?1, ?2, 'retrieval_failure', ?2, ?3, ?4, ?5, 'open', ?6, ?7, ?8,
            NULL, NULL, NULL, ?9, ?9, NULL, NULL
        )
        "#,
        params![
            &task_id,
            &failure.failure_id,
            &failure.trace_id,
            &failure.package_id,
            &task_type,
            &priority,
            &agent_cluster_key,
            &proposed_fix,
            &now,
        ],
    )?;
    let record = load_governance_task(conn, &task_id)?
        .ok_or_else(|| anyhow!("governance task was not readable after insert"))?;
    append_runtime_audit_event(
        conn,
        &record.trace_id,
        "governance_task_created",
        &json!({
            "task_id": &record.task_id,
            "source_failure_id": &record.source_failure_id,
            "source_entity_type": &record.source_entity_type,
            "source_entity_id_sha256": hash_text(&record.source_entity_id),
            "package_id": &record.package_id,
            "task_type": &record.task_type,
            "status": &record.status,
            "priority": &record.priority,
            "agent_cluster_key": &record.agent_cluster_key,
            "proposed_fix_sha256": hash_text(&record.proposed_fix),
        }),
    )?;
    Ok(record)
}

pub fn create_governance_task(
    conn: &Connection,
    input: KnowledgeGovernanceTaskCreateInput,
) -> Result<KnowledgeGovernanceTaskRecord> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = create_governance_task_inner(conn, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        create_governance_task_inner(conn, input)
    }
}

fn create_governance_task_inner(
    conn: &Connection,
    input: KnowledgeGovernanceTaskCreateInput,
) -> Result<KnowledgeGovernanceTaskRecord> {
    validate_governance_source_entity_type(&input.source_entity_type)?;
    validate_governance_task_type(&input.task_type)?;
    let priority = input
        .priority
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "p0".to_string());
    validate_governance_task_priority(&priority)?;
    let source_entity_id = input.source_entity_id.trim();
    if source_entity_id.is_empty() {
        return Err(anyhow!("governance task source_entity_id is required"));
    }
    let trace_id = input.trace_id.trim();
    if trace_id.is_empty() {
        return Err(anyhow!("governance task trace_id is required"));
    }
    if input.source_entity_type == "retrieval_failure" && input.source_failure_id.is_none() {
        return Err(anyhow!(
            "retrieval_failure governance task requires source_failure_id"
        ));
    }
    if let Some(existing) = load_governance_task_by_entity_type(
        conn,
        &input.source_entity_type,
        source_entity_id,
        &input.task_type,
    )? {
        return Ok(existing);
    }
    let agent_cluster_key = input
        .agent_cluster_key
        .as_deref()
        .and_then(|value| bounded_optional_text(value, 160))
        .unwrap_or_else(|| {
            format!(
                "{}:{}:{}",
                input.source_entity_type,
                &hash_text(source_entity_id)[..12],
                input.task_type
            )
        });
    let proposed_fix = input
        .proposed_fix
        .as_deref()
        .and_then(|value| bounded_optional_text(value, 480))
        .unwrap_or_else(|| "expert_review_requested_without_knowledge_base_mutation".to_string());
    let task_id = format!("kgt-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO knowledge_governance_tasks (
            task_id, source_failure_id, source_entity_type, source_entity_id,
            trace_id, package_id, task_type, status, priority, agent_cluster_key,
            proposed_fix, reviewer, review_note, evidence_ref, created_at,
            updated_at, accepted_at, closed_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?9, ?10,
            NULL, NULL, NULL, ?11, ?11, NULL, NULL
        )
        "#,
        params![
            &task_id,
            &input.source_failure_id,
            &input.source_entity_type,
            &source_entity_id,
            &trace_id,
            &input.package_id,
            &input.task_type,
            &priority,
            &agent_cluster_key,
            &proposed_fix,
            &now,
        ],
    )?;
    let record = load_governance_task(conn, &task_id)?
        .ok_or_else(|| anyhow!("governance task was not readable after insert"))?;
    append_runtime_audit_event(
        conn,
        &record.trace_id,
        "governance_task_created",
        &json!({
            "task_id": &record.task_id,
            "source_failure_id": &record.source_failure_id,
            "source_entity_type": &record.source_entity_type,
            "source_entity_id_sha256": hash_text(&record.source_entity_id),
            "package_id": &record.package_id,
            "task_type": &record.task_type,
            "status": &record.status,
            "priority": &record.priority,
            "agent_cluster_key": &record.agent_cluster_key,
            "proposed_fix_sha256": hash_text(&record.proposed_fix),
        }),
    )?;
    Ok(record)
}

pub fn list_governance_tasks(
    conn: &Connection,
    input: KnowledgeGovernanceTaskListInput,
) -> Result<KnowledgeGovernanceTaskListResult> {
    let limit = governance_task_page_limit(input.limit);
    let offset = input.offset;
    let mut predicates = Vec::new();
    let mut sql_params = Vec::<rusqlite::types::Value>::new();
    if let Some(status) = input.status.as_ref() {
        validate_governance_task_status(status)?;
        predicates.push("status = ?".to_string());
        sql_params.push(rusqlite::types::Value::Text(status.clone()));
    }
    if let Some(task_type) = input.task_type.as_ref() {
        validate_governance_task_type(task_type)?;
        predicates.push("task_type = ?".to_string());
        sql_params.push(rusqlite::types::Value::Text(task_type.clone()));
    }
    if let Some(priority) = input.priority.as_ref() {
        validate_governance_task_priority(priority)?;
        predicates.push("priority = ?".to_string());
        sql_params.push(rusqlite::types::Value::Text(priority.clone()));
    }
    if let Some(source_failure_id) = input.source_failure_id.as_ref() {
        predicates.push("source_failure_id = ?".to_string());
        sql_params.push(rusqlite::types::Value::Text(source_failure_id.clone()));
    }
    if let Some(source_entity_type) = input.source_entity_type.as_ref() {
        validate_governance_source_entity_type(source_entity_type)?;
        predicates.push("source_entity_type = ?".to_string());
        sql_params.push(rusqlite::types::Value::Text(source_entity_type.clone()));
    }
    if let Some(source_entity_id) = input.source_entity_id.as_ref() {
        predicates.push("source_entity_id = ?".to_string());
        sql_params.push(rusqlite::types::Value::Text(source_entity_id.clone()));
    }
    let mut sql = governance_task_select_sql().to_string();
    if !predicates.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&predicates.join(" AND "));
    }
    sql.push_str(" ORDER BY updated_at DESC, task_id DESC LIMIT ? OFFSET ?");
    sql_params.push(rusqlite::types::Value::Integer(limit as i64));
    sql_params.push(rusqlite::types::Value::Integer(offset as i64));
    let records = query_governance_task_records_dynamic(conn, &sql, sql_params)?;
    let next_offset = if records.len() == limit {
        Some(offset + limit)
    } else {
        None
    };
    Ok(KnowledgeGovernanceTaskListResult {
        object: "tonglingyu.knowledge_governance_task_list".to_string(),
        schema_version: KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION.to_string(),
        limit,
        offset,
        next_offset,
        items: records.iter().map(governance_task_record_json).collect(),
    })
}

pub fn read_governance_task(conn: &Connection, task_id: &str) -> Result<Option<Value>> {
    Ok(load_governance_task(conn, task_id)?
        .as_ref()
        .map(governance_task_record_json))
}

pub fn list_governance_tasks_for_trace(
    conn: &Connection,
    trace_id: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    let limit = governance_task_page_limit(limit);
    let limit_i64 = limit as i64;
    let sql = format!(
        "{} WHERE trace_id = ?1 ORDER BY updated_at DESC, task_id DESC LIMIT ?2",
        governance_task_select_sql()
    );
    Ok(
        query_governance_task_records(conn, &sql, &[&trace_id, &limit_i64])?
            .iter()
            .map(governance_task_record_json)
            .collect(),
    )
}

pub fn list_governance_tasks_for_package(
    conn: &Connection,
    package_id: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    let limit = governance_task_page_limit(limit);
    let limit_i64 = limit as i64;
    let sql = format!(
        "{} WHERE package_id = ?1 ORDER BY updated_at DESC, task_id DESC LIMIT ?2",
        governance_task_select_sql()
    );
    Ok(
        query_governance_task_records(conn, &sql, &[&package_id, &limit_i64])?
            .iter()
            .map(governance_task_record_json)
            .collect(),
    )
}

pub fn update_governance_task(
    conn: &Connection,
    task_id: &str,
    input: KnowledgeGovernanceTaskUpdateInput,
) -> Result<Option<KnowledgeGovernanceTaskRecord>> {
    validate_governance_task_status(&input.status)?;
    let reviewer = input
        .reviewer
        .as_deref()
        .and_then(|value| bounded_optional_text(value, 80));
    let review_note = input
        .review_note
        .as_deref()
        .and_then(|value| bounded_optional_text(value, 480));
    let evidence_ref = input
        .evidence_ref
        .as_deref()
        .and_then(|value| bounded_optional_text(value, 240));
    if input.status == "accepted"
        && (reviewer.is_none() || review_note.is_none() || evidence_ref.is_none())
    {
        return Err(anyhow!(
            "accepted governance task requires reviewer, review_note, and evidence_ref"
        ));
    }
    if matches!(input.status.as_str(), "closed" | "rejected")
        && (reviewer.is_none() || review_note.is_none())
    {
        return Err(anyhow!(
            "closed or rejected governance task requires reviewer and review_note"
        ));
    }
    let current = load_governance_task(conn, task_id)?;
    let previous_status = current.as_ref().map(|record| record.status.clone());
    if let Some(current) = current.as_ref() {
        let same_payload = current.status == input.status
            && current.reviewer == reviewer
            && current.review_note == review_note
            && current.evidence_ref == evidence_ref;
        if same_payload && input.expected_updated_at.is_none() {
            return Ok(Some(current.clone()));
        }
    }
    let now = now_rfc3339();
    let updated = conn.execute(
        r#"
        UPDATE knowledge_governance_tasks
        SET status = ?2,
            reviewer = ?3,
            review_note = ?4,
            evidence_ref = ?5,
            updated_at = ?6,
            accepted_at = CASE
                WHEN ?2 = 'accepted' THEN COALESCE(accepted_at, ?6)
                WHEN ?2 IN ('open', 'in_review') THEN NULL
                ELSE accepted_at
            END,
            closed_at = CASE
                WHEN ?2 IN ('closed', 'rejected') THEN COALESCE(closed_at, ?6)
                WHEN ?2 IN ('open', 'in_review', 'accepted') THEN NULL
                ELSE closed_at
            END
        WHERE task_id = ?1
          AND (?7 IS NULL OR updated_at = ?7)
        "#,
        params![
            task_id,
            input.status,
            reviewer,
            review_note,
            evidence_ref,
            now,
            input.expected_updated_at,
        ],
    )?;
    if updated == 0 {
        if input.expected_updated_at.is_some() && load_governance_task(conn, task_id)?.is_some() {
            return Err(anyhow!("governance task update conflict"));
        }
        return Ok(None);
    }
    let record = load_governance_task(conn, task_id)?
        .ok_or_else(|| anyhow!("governance task disappeared after update"))?;
    append_runtime_audit_event(
        conn,
        &record.trace_id,
        "governance_task_status_updated",
        &json!({
            "task_id": &record.task_id,
            "source_failure_id": &record.source_failure_id,
            "source_entity_type": &record.source_entity_type,
            "source_entity_id_sha256": hash_text(&record.source_entity_id),
            "previous_status": previous_status,
            "new_status": &record.status,
            "status": &record.status,
            "priority": &record.priority,
            "reviewer": &record.reviewer,
            "review_note_sha256": record.review_note.as_deref().map(hash_text),
            "evidence_ref_sha256": record.evidence_ref.as_deref().map(hash_text),
            "status_history": {
                "previous_status": previous_status,
                "new_status": &record.status,
                "reason_sha256": record.review_note.as_deref().map(hash_text),
                "timestamp": &record.updated_at,
            },
            "accepted_at": &record.accepted_at,
            "closed_at": &record.closed_at,
        }),
    )?;
    Ok(Some(record))
}

pub fn create_knowledge_item(
    conn: &Connection,
    input: KnowledgeItemCreateInput,
) -> Result<KnowledgeItemRecord> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = create_knowledge_item_inner(conn, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        create_knowledge_item_inner(conn, input)
    }
}

fn create_knowledge_item_inner(
    conn: &Connection,
    input: KnowledgeItemCreateInput,
) -> Result<KnowledgeItemRecord> {
    let source_refs = normalize_knowledge_refs("source_refs", input.source_refs)?;
    let evidence_refs = normalize_knowledge_refs("evidence_refs", input.evidence_refs)?;
    let actor = validate_knowledge_item_actor(&input.actor)?;
    let reason = validate_knowledge_item_reason(&input.reason)?;
    let trace_id = validate_knowledge_item_trace_id(&input.trace_id)?;
    let schema_version = input
        .schema_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION.to_string());
    let payload = canonical_json_value(&input.payload);
    if !payload.is_object() {
        return Err(anyhow!("knowledge item payload must be a JSON object"));
    }
    let payload_json = serde_json::to_string(&payload)?;
    if payload_json.len() > 16_384 {
        return Err(anyhow!("knowledge item payload exceeds 16384 byte limit"));
    }
    let source_refs_json = serde_json::to_string(&source_refs)?;
    let evidence_refs_json = serde_json::to_string(&evidence_refs)?;
    let payload_sha256 = hash_text(&payload_json);
    let item_id = stable_knowledge_item_id(input.kind, &source_refs_json, &payload_sha256);
    if let Some(existing) = load_knowledge_item(conn, &item_id)? {
        return Ok(existing);
    }
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO knowledge_items (
            item_id, kind, state, source_refs_json, evidence_refs_json,
            payload_json, payload_sha256, schema_version, created_at, updated_at,
            state_version
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, 1)
        "#,
        params![
            item_id,
            input.kind.as_str(),
            input.initial_state.as_str(),
            source_refs_json,
            evidence_refs_json,
            payload_json,
            payload_sha256,
            schema_version,
            now,
        ],
    )?;
    insert_knowledge_item_state_history(
        conn,
        KnowledgeItemStateHistoryInsert {
            item_id: &item_id,
            previous_state: None,
            new_state: input.initial_state,
            actor: &actor,
            reason: &reason,
            evidence_refs: &evidence_refs,
            state_version: 1,
            created_at: &now,
        },
    )?;
    let record = load_knowledge_item(conn, &item_id)?
        .ok_or_else(|| anyhow!("knowledge item was not readable after insert"))?;
    append_runtime_audit_event(
        conn,
        &trace_id,
        "knowledge_item_created",
        &json!({
            "item_id": &record.item_id,
            "kind": record.kind.as_str(),
            "state": record.state.as_str(),
            "state_version": record.state_version,
            "actor": actor,
            "reason_sha256": hash_text(&reason),
            "source_ref_count": record.source_refs.len(),
            "evidence_ref_count": record.evidence_refs.len(),
            "payload_sha256": &record.payload_sha256,
            "schema_version": &record.schema_version,
        }),
    )?;
    Ok(record)
}

pub fn read_knowledge_item(
    conn: &Connection,
    item_id: &str,
) -> Result<Option<KnowledgeItemRecord>> {
    load_knowledge_item(conn, item_id)
}

pub fn list_knowledge_items(
    conn: &Connection,
    input: KnowledgeItemListInput,
) -> Result<KnowledgeItemListResult> {
    let limit = knowledge_item_page_limit(input.limit);
    let offset = input.offset;
    let limit_i64 = limit as i64;
    let offset_i64 = offset as i64;
    let base_sql = knowledge_item_select_sql();
    let records = match (input.kind, input.state) {
        (Some(kind), Some(state)) => query_knowledge_item_records(
            conn,
            &format!(
                "{base_sql} WHERE kind = ?1 AND state = ?2 ORDER BY updated_at DESC, item_id DESC LIMIT ?3 OFFSET ?4"
            ),
            &[
                &kind.as_str() as &dyn ToSql,
                &state.as_str(),
                &limit_i64,
                &offset_i64,
            ],
        )?,
        (Some(kind), None) => query_knowledge_item_records(
            conn,
            &format!(
                "{base_sql} WHERE kind = ?1 ORDER BY updated_at DESC, item_id DESC LIMIT ?2 OFFSET ?3"
            ),
            &[&kind.as_str() as &dyn ToSql, &limit_i64, &offset_i64],
        )?,
        (None, Some(state)) => query_knowledge_item_records(
            conn,
            &format!(
                "{base_sql} WHERE state = ?1 ORDER BY updated_at DESC, item_id DESC LIMIT ?2 OFFSET ?3"
            ),
            &[&state.as_str() as &dyn ToSql, &limit_i64, &offset_i64],
        )?,
        (None, None) => query_knowledge_item_records(
            conn,
            &format!("{base_sql} ORDER BY updated_at DESC, item_id DESC LIMIT ?1 OFFSET ?2"),
            &[&limit_i64 as &dyn ToSql, &offset_i64],
        )?,
    };
    let next_offset = if records.len() == limit {
        Some(offset + limit)
    } else {
        None
    };
    Ok(KnowledgeItemListResult {
        object: "tonglingyu.knowledge_item_list".to_string(),
        schema_version: KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION.to_string(),
        limit,
        offset,
        next_offset,
        items: records,
    })
}

pub fn update_knowledge_item_state(
    conn: &Connection,
    item_id: &str,
    input: KnowledgeItemStateUpdateInput,
) -> Result<Option<KnowledgeItemRecord>> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = update_knowledge_item_state_inner(conn, item_id, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        update_knowledge_item_state_inner(conn, item_id, input)
    }
}

fn update_knowledge_item_state_inner(
    conn: &Connection,
    item_id: &str,
    input: KnowledgeItemStateUpdateInput,
) -> Result<Option<KnowledgeItemRecord>> {
    let current = match load_knowledge_item(conn, item_id)? {
        Some(record) => record,
        None => return Ok(None),
    };
    if current.state_version != input.expected_state_version {
        return Err(anyhow!("knowledge item state update conflict"));
    }
    let actor = validate_knowledge_item_actor(&input.actor)?;
    let reason = validate_knowledge_item_reason(&input.reason)?;
    let trace_id = validate_knowledge_item_trace_id(&input.trace_id)?;
    let evidence_refs = normalize_knowledge_refs("evidence_refs", input.evidence_refs)?;
    if input.new_state == KnowledgeState::HumanMarked {
        return Err(anyhow!(
            "human_marked knowledge item state requires human review action"
        ));
    }
    if current.state == input.new_state {
        return Ok(Some(current));
    }
    let evidence_refs_json = serde_json::to_string(&evidence_refs)?;
    let next_state_version = current.state_version + 1;
    let now = now_rfc3339();
    let updated = conn.execute(
        r#"
        UPDATE knowledge_items
        SET state = ?2,
            evidence_refs_json = ?3,
            updated_at = ?4,
            state_version = ?5
        WHERE item_id = ?1 AND state_version = ?6
        "#,
        params![
            item_id,
            input.new_state.as_str(),
            evidence_refs_json,
            now,
            next_state_version,
            input.expected_state_version,
        ],
    )?;
    if updated == 0 {
        return Err(anyhow!("knowledge item state update conflict"));
    }
    insert_knowledge_item_state_history(
        conn,
        KnowledgeItemStateHistoryInsert {
            item_id,
            previous_state: Some(current.state),
            new_state: input.new_state,
            actor: &actor,
            reason: &reason,
            evidence_refs: &evidence_refs,
            state_version: next_state_version,
            created_at: &now,
        },
    )?;
    let record = load_knowledge_item(conn, item_id)?
        .ok_or_else(|| anyhow!("knowledge item disappeared after state update"))?;
    append_runtime_audit_event(
        conn,
        &trace_id,
        "knowledge_item_state_updated",
        &json!({
            "item_id": &record.item_id,
            "kind": record.kind.as_str(),
            "previous_state": current.state.as_str(),
            "new_state": record.state.as_str(),
            "state_version": record.state_version,
            "actor": actor,
            "reason_sha256": hash_text(&reason),
            "source_ref_count": record.source_refs.len(),
            "evidence_ref_count": record.evidence_refs.len(),
            "payload_sha256": &record.payload_sha256,
            "schema_version": &record.schema_version,
        }),
    )?;
    Ok(Some(record))
}

pub fn promote_knowledge_item_runtime_usable(
    conn: &Connection,
    item_id: &str,
    input: KnowledgeRuntimePromotionInput,
) -> Result<Option<KnowledgeItemRecord>> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = promote_knowledge_item_runtime_usable_inner(conn, item_id, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        promote_knowledge_item_runtime_usable_inner(conn, item_id, input)
    }
}

fn promote_knowledge_item_runtime_usable_inner(
    conn: &Connection,
    item_id: &str,
    input: KnowledgeRuntimePromotionInput,
) -> Result<Option<KnowledgeItemRecord>> {
    let current = match load_knowledge_item(conn, item_id)? {
        Some(record) => record,
        None => return Ok(None),
    };
    if current.state_version != input.expected_state_version {
        return Err(anyhow!("knowledge item runtime promotion conflict"));
    }
    if current.state != KnowledgeState::SystemCalibrated {
        return Err(anyhow!(
            "runtime promotion requires system_calibrated item, got {}",
            current.state.as_str()
        ));
    }
    if current.evidence_refs.is_empty()
        || current.source_boundary.is_none()
        || current.calibration_report_ref.is_none()
        || current.confidence.unwrap_or_default() < 0.8
    {
        return Err(anyhow!(
            "runtime promotion requires evidence refs, source boundary, calibration report ref and confidence"
        ));
    }
    let actor = validate_knowledge_item_actor(&input.actor)?;
    let reason = validate_knowledge_item_reason(&input.reason)?;
    let trace_id = validate_knowledge_item_trace_id(&input.trace_id)?;
    let release_run_id = bounded_optional_text(&input.release_run_id, 160)
        .ok_or_else(|| anyhow!("runtime promotion release_run_id is required"))?;
    if let Some(expires_at) = input.expires_at.as_deref() {
        let expires_at = bounded_optional_text(expires_at, 80)
            .ok_or_else(|| anyhow!("runtime promotion expires_at is invalid"))?;
        if expires_at <= now_rfc3339() {
            return Err(anyhow!("runtime promotion expires_at is already expired"));
        }
    }
    let mut payload = canonical_json_value(&current.payload);
    let payload_object = payload
        .as_object_mut()
        .ok_or_else(|| anyhow!("knowledge item payload must be an object"))?;
    let now = now_rfc3339();
    payload_object.insert(
        "runtime_policy".to_string(),
        json!({
            "policy_version": KNOWLEDGE_RUNTIME_POLICY_VERSION,
            "release_run_id": release_run_id,
            "promoted_at": now,
            "expires_at": input.expires_at,
            "calibration_report_ref": current.calibration_report_ref,
            "source_boundary_sha256": hash_text(&serde_json::to_string(&current.source_boundary)?),
        }),
    );
    let payload = canonical_json_value(&payload);
    let payload_json = serde_json::to_string(&payload)?;
    let payload_sha256 = hash_text(&payload_json);
    let next_state_version = current.state_version + 1;
    conn.execute(
        r#"
        UPDATE knowledge_items
        SET state = ?2,
            payload_json = ?3,
            payload_sha256 = ?4,
            updated_at = ?5,
            state_version = ?6
        WHERE item_id = ?1 AND state_version = ?7
        "#,
        params![
            item_id,
            KnowledgeState::RuntimeUsable.as_str(),
            payload_json,
            payload_sha256,
            &now,
            next_state_version,
            input.expected_state_version,
        ],
    )?;
    insert_knowledge_item_state_history(
        conn,
        KnowledgeItemStateHistoryInsert {
            item_id,
            previous_state: Some(current.state),
            new_state: KnowledgeState::RuntimeUsable,
            actor: &actor,
            reason: &reason,
            evidence_refs: &current.evidence_refs,
            state_version: next_state_version,
            created_at: &now,
        },
    )?;
    let record = load_knowledge_item(conn, item_id)?
        .ok_or_else(|| anyhow!("knowledge item disappeared after runtime promotion"))?;
    append_runtime_audit_event(
        conn,
        &trace_id,
        "knowledge_item_runtime_policy_promoted",
        &json!({
            "item_id": &record.item_id,
            "previous_state": current.state.as_str(),
            "new_state": record.state.as_str(),
            "policy_version": KNOWLEDGE_RUNTIME_POLICY_VERSION,
            "release_run_id_sha256": hash_text(&release_run_id),
            "calibration_report_ref": record.calibration_report_ref,
            "confidence": record.confidence,
            "actor": actor,
            "reason_sha256": hash_text(&reason),
        }),
    )?;
    Ok(Some(record))
}

pub fn review_knowledge_item_human(
    conn: &Connection,
    item_id: &str,
    input: KnowledgeItemHumanReviewInput,
) -> Result<Option<KnowledgeItemHumanReviewResult>> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = review_knowledge_item_human_inner(conn, item_id, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        review_knowledge_item_human_inner(conn, item_id, input)
    }
}

fn review_knowledge_item_human_inner(
    conn: &Connection,
    item_id: &str,
    input: KnowledgeItemHumanReviewInput,
) -> Result<Option<KnowledgeItemHumanReviewResult>> {
    let current = match load_knowledge_item(conn, item_id)? {
        Some(record) => record,
        None => return Ok(None),
    };
    let task_id = bounded_optional_text(&input.task_id, 160)
        .ok_or_else(|| anyhow!("knowledge item human review task_id is required"))?;
    let task = load_governance_task(conn, &task_id)?
        .ok_or_else(|| anyhow!("knowledge item human review governance task not found"))?;
    if task.source_entity_type != "knowledge_item" || task.source_entity_id != item_id {
        return Err(anyhow!(
            "knowledge item human review task must target the reviewed knowledge item"
        ));
    }
    if !matches!(
        task.status.as_str(),
        "open" | "in_review" | "accepted" | "rejected"
    ) {
        return Err(anyhow!(
            "knowledge item human review task status {} is not reviewable",
            task.status
        ));
    }
    let target_state = input.decision.target_state();
    let target_task_status = input.decision.task_status();
    let actor = validate_knowledge_item_actor(&input.actor)?;
    let trace_id = validate_knowledge_item_trace_id(&input.trace_id)?;
    let reviewer = validate_knowledge_review_reviewer(&input.reviewer)?;
    let review_note = validate_knowledge_item_reason(&input.review_note)?;
    let evidence_ref = validate_knowledge_review_evidence_ref(&input.evidence_ref)?;
    if task.trace_id != trace_id {
        return Err(anyhow!(
            "knowledge item human review trace_id must match governance task"
        ));
    }
    let task_already_matches = task.status == target_task_status
        && task.reviewer.as_deref() == Some(reviewer.as_str())
        && task.review_note.as_deref() == Some(review_note.as_str())
        && task.evidence_ref.as_deref() == Some(evidence_ref.as_str());
    if current.state == target_state {
        if task_already_matches
            && (input.expected_state_version == current.state_version
                || input.expected_state_version + 1 == current.state_version)
        {
            return Ok(Some(KnowledgeItemHumanReviewResult {
                object: "tonglingyu.knowledge_item_human_review".to_string(),
                schema_version: KNOWLEDGE_ITEM_HUMAN_REVIEW_SCHEMA_VERSION.to_string(),
                decision: input.decision,
                item: current,
                task,
                kb_rebuild_required: true,
                eval_diff_required: true,
                release_gate_required: true,
            }));
        }
        return Err(anyhow!(
            "knowledge item human review has already been recorded with different metadata"
        ));
    }
    if current.state_version != input.expected_state_version {
        return Err(anyhow!("knowledge item human review conflict"));
    }
    if input.decision == KnowledgeItemHumanReviewDecision::Accept
        && matches!(
            current.state,
            KnowledgeState::Rejected | KnowledgeState::Deprecated
        )
    {
        return Err(anyhow!(
            "rejected or deprecated knowledge item cannot be accepted as human_marked"
        ));
    }
    if matches!(task.status.as_str(), "accepted" | "rejected") && !task_already_matches {
        return Err(anyhow!(
            "knowledge item human review task already has a different terminal decision"
        ));
    }

    let mut evidence_refs = current.evidence_refs.clone();
    evidence_refs.push(evidence_ref.clone());
    let evidence_refs = normalize_knowledge_refs("evidence_refs", evidence_refs)?;
    let evidence_refs_json = serde_json::to_string(&evidence_refs)?;
    let mut payload = canonical_json_value(&current.payload);
    let payload_object = payload
        .as_object_mut()
        .ok_or_else(|| anyhow!("knowledge item payload must be an object"))?;
    let now = now_rfc3339();
    payload_object.insert(
        "human_review".to_string(),
        json!({
            "schema_version": KNOWLEDGE_ITEM_HUMAN_REVIEW_SCHEMA_VERSION,
            "decision": input.decision.as_str(),
            "target_state": target_state.as_str(),
            "task_id": &task_id,
            "reviewer": &reviewer,
            "review_note_sha256": hash_text(&review_note),
            "evidence_ref": &evidence_ref,
            "reviewed_at": &now,
            "actor": &actor,
            "kb_rebuild_required": true,
            "eval_diff_required": true,
            "release_gate_required": true,
        }),
    );
    let payload = canonical_json_value(&payload);
    let payload_json = serde_json::to_string(&payload)?;
    let payload_sha256 = hash_text(&payload_json);
    let next_state_version = current.state_version + 1;
    let updated = conn.execute(
        r#"
        UPDATE knowledge_items
        SET state = ?2,
            evidence_refs_json = ?3,
            payload_json = ?4,
            payload_sha256 = ?5,
            updated_at = ?6,
            state_version = ?7
        WHERE item_id = ?1 AND state_version = ?8
        "#,
        params![
            item_id,
            target_state.as_str(),
            evidence_refs_json,
            payload_json,
            payload_sha256,
            &now,
            next_state_version,
            input.expected_state_version,
        ],
    )?;
    if updated == 0 {
        return Err(anyhow!("knowledge item human review conflict"));
    }
    let reason = format!(
        "human_review_decision={}; task_id={}; note={}",
        input.decision.as_str(),
        task_id,
        review_note
    );
    insert_knowledge_item_state_history(
        conn,
        KnowledgeItemStateHistoryInsert {
            item_id,
            previous_state: Some(current.state),
            new_state: target_state,
            actor: &actor,
            reason: &reason,
            evidence_refs: &evidence_refs,
            state_version: next_state_version,
            created_at: &now,
        },
    )?;
    let updated_task = update_governance_task(
        conn,
        &task_id,
        KnowledgeGovernanceTaskUpdateInput {
            status: target_task_status.to_string(),
            reviewer: Some(reviewer.clone()),
            review_note: Some(review_note.clone()),
            evidence_ref: Some(evidence_ref.clone()),
            expected_updated_at: input.expected_task_updated_at,
        },
    )?
    .ok_or_else(|| anyhow!("knowledge item human review governance task not found"))?;
    let record = load_knowledge_item(conn, item_id)?
        .ok_or_else(|| anyhow!("knowledge item disappeared after human review"))?;
    append_runtime_audit_event(
        conn,
        &trace_id,
        "knowledge_item_human_reviewed",
        &json!({
            "item_id": &record.item_id,
            "kind": record.kind.as_str(),
            "task_id": &updated_task.task_id,
            "decision": input.decision.as_str(),
            "previous_state": current.state.as_str(),
            "new_state": record.state.as_str(),
            "state_version": record.state_version,
            "actor": actor,
            "reviewer": reviewer,
            "review_note_sha256": hash_text(&review_note),
            "evidence_ref_sha256": hash_text(&evidence_ref),
            "source_ref_count": record.source_refs.len(),
            "evidence_ref_count": record.evidence_refs.len(),
            "payload_sha256": &record.payload_sha256,
            "schema_version": &record.schema_version,
            "kb_rebuild_required": true,
            "eval_diff_required": true,
            "release_gate_required": true,
        }),
    )?;
    Ok(Some(KnowledgeItemHumanReviewResult {
        object: "tonglingyu.knowledge_item_human_review".to_string(),
        schema_version: KNOWLEDGE_ITEM_HUMAN_REVIEW_SCHEMA_VERSION.to_string(),
        decision: input.decision,
        item: record,
        task: updated_task,
        kb_rebuild_required: true,
        eval_diff_required: true,
        release_gate_required: true,
    }))
}

pub async fn execute_knowledge_calibration_llm_evidence_judge(
    runtime: Arc<dyn RuntimeClient>,
    item: &KnowledgeItemRecord,
    config: &KnowledgeCalibrationLlmConfig,
    trace_id: &str,
) -> Result<KnowledgeCalibrationLlmJudgeOutput> {
    if config.profile_id != KNOWLEDGE_CALIBRATION_PROFILE_ID {
        return Err(anyhow!(
            "knowledge calibration LLM config profile must be {KNOWLEDGE_CALIBRATION_PROFILE_ID}"
        ));
    }
    let contract = knowledge_calibration_profile_contract();
    let output = runtime
        .execute_profile_step(RuntimeProfileInput {
            profile_id: KNOWLEDGE_CALIBRATION_PROFILE_ID.to_string(),
            messages: vec![RuntimeProfileMessage::new(
                "user",
                serde_json::to_string(&json!({
                    "object": "tonglingyu.knowledge_calibration_llm_input",
                    "item_id": item.item_id,
                    "kind": item.kind.as_str(),
                    "state": item.state.as_str(),
                    "source_refs": item.source_refs,
                    "evidence_refs": item.evidence_refs,
                    "payload_sha256": item.payload_sha256,
                    "payload": item.payload,
                    "calibration_method": KnowledgeCalibrationMethod::LlmEvidenceJudge.as_str(),
                    "config_digest": config.config_digest,
                    "profile_contract_version": KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION,
                    "instruction": "judge only whether the candidate is supported by the supplied evidence refs; return decision/confidence/evidence_refs/source_boundary/quality_issues JSON; do not write facts",
                }))?,
            )],
            metadata: json!({
                "runtime": "tonglingyu",
                "operation": "knowledge_calibration_llm_evidence_judge",
                "llm_config": config.safe_summary(),
                "contains_secret_values": false,
            }),
            profile_contract: Some(contract),
            runtime_step: None,
            requested_tools: Vec::new(),
            trace_id: trace_id.to_string(),
        })
        .await
        .map_err(|error| anyhow!("knowledge calibration LLM evidence judge failed: {error}"))?;
    parse_knowledge_calibration_llm_judge_output(&output.result_summary)
}

pub fn parse_knowledge_calibration_llm_judge_output(
    result_summary: &str,
) -> Result<KnowledgeCalibrationLlmJudgeOutput> {
    let trimmed = result_summary.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("knowledge calibration LLM judge output is empty"));
    }
    let value = parse_agent_runtime_summary_value(trimmed)
        .ok_or_else(|| anyhow!("knowledge calibration LLM judge output must be JSON"))?;
    validate_calibration_privacy(&value)?;
    let object = object_or_named_child(&value, "llm_evidence_judge")
        .ok_or_else(|| anyhow!("knowledge calibration LLM judge output must be an object"))?;
    let decision = KnowledgeCalibrationDecision::parse(
        object
            .get("decision")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("knowledge calibration LLM judge decision missing"))?,
    )?;
    let confidence = object
        .get("confidence")
        .and_then(Value::as_f64)
        .ok_or_else(|| anyhow!("knowledge calibration LLM judge confidence missing"))?;
    let evidence_refs = object
        .get("evidence_refs")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("knowledge calibration LLM judge evidence_refs missing"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .and_then(|text| bounded_optional_text(text, 240))
                .ok_or_else(|| {
                    anyhow!("knowledge calibration LLM judge evidence_refs must be strings")
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let source_boundary = object
        .get("source_boundary")
        .cloned()
        .ok_or_else(|| anyhow!("knowledge calibration LLM judge source_boundary missing"))?;
    let quality_issues = object
        .get("quality_issues")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .and_then(|text| bounded_optional_text(text, 240))
                        .ok_or_else(|| {
                            anyhow!(
                                "knowledge calibration LLM judge quality_issues must be strings"
                            )
                        })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let forbidden_conclusion_detected = object
        .get("forbidden_conclusion_detected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(KnowledgeCalibrationLlmJudgeOutput {
        decision,
        confidence,
        evidence_refs,
        source_boundary,
        quality_issues,
        forbidden_conclusion_detected,
    })
}

pub fn run_knowledge_calibration_offline(
    conn: &Connection,
    input: KnowledgeCalibrationRunInput,
) -> Result<KnowledgeCalibrationReportRecord> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = run_knowledge_calibration_offline_inner(conn, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        run_knowledge_calibration_offline_inner(conn, input)
    }
}

fn run_knowledge_calibration_offline_inner(
    conn: &Connection,
    input: KnowledgeCalibrationRunInput,
) -> Result<KnowledgeCalibrationReportRecord> {
    let item_id = bounded_optional_text(&input.item_id, 160)
        .ok_or_else(|| anyhow!("knowledge calibration item_id is required"))?;
    let input_ref = bounded_optional_text(&input.input_ref, 240)
        .ok_or_else(|| anyhow!("knowledge calibration input_ref is required"))?;
    let trace_id = validate_knowledge_item_trace_id(&input.trace_id)?;
    let actor = validate_knowledge_item_actor(&input.actor)?;
    let item = load_knowledge_item(conn, &item_id)?
        .ok_or_else(|| anyhow!("knowledge calibration item not found: {item_id}"))?;
    if item.state != KnowledgeState::Candidate {
        return Err(anyhow!(
            "knowledge calibration requires candidate item, got {}",
            item.state.as_str()
        ));
    }
    let outcome = evaluate_knowledge_calibration(&item, &input)?;
    let report = insert_knowledge_calibration_report(
        conn,
        &item,
        input.input_kind,
        &input_ref,
        input.method,
        &outcome,
        &trace_id,
        &actor,
    )?;
    if let Some(target_state) = report.decision.target_state() {
        update_knowledge_item_state_from_calibration(
            conn,
            &item,
            target_state,
            &report,
            &trace_id,
            &actor,
        )?;
    } else {
        append_runtime_audit_event(
            conn,
            &trace_id,
            "knowledge_calibration_candidate_kept",
            &json!({
                "item_id": &item.item_id,
                "report_id": &report.report_id,
                "report_ref": &report.report_ref,
                "method": report.method.as_str(),
                "decision": report.decision.as_str(),
                "quality_issue_count": report.quality_issues.len(),
                "actor": actor,
            }),
        )?;
    }
    Ok(report)
}

struct KnowledgeCalibrationOutcome {
    decision: KnowledgeCalibrationDecision,
    confidence: f64,
    evidence_refs: Vec<String>,
    source_boundary: Value,
    quality_issues: Vec<String>,
    coverage_matrix: Value,
    config_summary: Option<Value>,
    method_detail: Value,
}

fn evaluate_knowledge_calibration(
    item: &KnowledgeItemRecord,
    input: &KnowledgeCalibrationRunInput,
) -> Result<KnowledgeCalibrationOutcome> {
    let mut quality_issues = Vec::new();
    let mut evidence_refs = item.evidence_refs.clone();
    let source_boundary;
    let confidence;
    let mut decision;
    let mut forbidden_conclusion = false;
    let mut reviewer_downgrade = false;
    let mut llm_config_missing = false;
    let method_detail;
    let mut config_summary = None;

    match input.method {
        KnowledgeCalibrationMethod::Rule => {
            let context = input
                .rule_context
                .as_ref()
                .ok_or_else(|| anyhow!("rule calibration requires rule_context"))?;
            source_boundary = json!({
                "source_id": bounded_optional_text(&context.source_id, 160),
                "block_id": bounded_optional_text(&context.block_id, 200),
                "required_evidence_type": bounded_optional_text(&context.required_evidence_type, 80),
                "version_boundary": bounded_optional_text(&context.version_boundary, 240),
                "usage_boundary": bounded_optional_text(&context.usage_boundary, 240),
            });
            if bounded_optional_text(&context.source_id, 160).is_none() {
                quality_issues.push("source_id_missing".to_string());
            }
            if bounded_optional_text(&context.block_id, 200).is_none() {
                quality_issues.push("block_id_missing".to_string());
            }
            if bounded_optional_text(&context.required_evidence_type, 80).is_none() {
                quality_issues.push("required_evidence_type_missing".to_string());
            }
            if bounded_optional_text(&context.version_boundary, 240).is_none() {
                quality_issues.push("version_boundary_missing".to_string());
            }
            if bounded_optional_text(&context.usage_boundary, 240).is_none() {
                quality_issues.push("usage_boundary_missing".to_string());
            }
            let payload_text = item.payload.to_string();
            for term in &context.exact_terms {
                let Some(term) = bounded_optional_text(term, 120) else {
                    quality_issues.push("exact_term_missing".to_string());
                    continue;
                };
                if !payload_text.contains(&term) {
                    quality_issues.push(format!("exact_term_not_in_payload:{}", hash_text(&term)));
                }
            }
            if !context.block_id.trim().is_empty() {
                evidence_refs.push(format!("block://{}", context.block_id.trim()));
            }
            decision = if quality_issues.is_empty() {
                KnowledgeCalibrationDecision::SystemCalibrated
            } else {
                KnowledgeCalibrationDecision::KeepCandidate
            };
            confidence = if quality_issues.is_empty() {
                0.91
            } else {
                0.35
            };
            method_detail = json!({
                "rule_context": {
                    "source_id": context.source_id,
                    "block_id": context.block_id,
                    "required_evidence_type": context.required_evidence_type,
                    "exact_term_count": context.exact_terms.len(),
                    "version_boundary_present": !context.version_boundary.trim().is_empty(),
                    "usage_boundary_present": !context.usage_boundary.trim().is_empty(),
                }
            });
        }
        KnowledgeCalibrationMethod::Eval => {
            let context = input
                .eval_context
                .as_ref()
                .ok_or_else(|| anyhow!("eval calibration requires eval_context"))?;
            if !context.expected_evidence_hit {
                quality_issues.push("expected_evidence_miss".to_string());
            }
            if context.forbidden_conclusion_hit {
                quality_issues.push("forbidden_conclusion_hit".to_string());
                forbidden_conclusion = true;
            }
            if context.reviewer_status != "passed" {
                quality_issues.push("reviewer_status_not_passed".to_string());
                reviewer_downgrade = true;
            }
            if !context.source_boundary_confirmed {
                quality_issues.push("source_boundary_not_confirmed".to_string());
            }
            source_boundary = json!({
                "eval_expected_evidence_hit": context.expected_evidence_hit,
                "source_boundary_confirmed": context.source_boundary_confirmed,
                "reviewer_status": context.reviewer_status,
            });
            decision = if context.forbidden_conclusion_hit || context.reviewer_status != "passed" {
                KnowledgeCalibrationDecision::Rejected
            } else if quality_issues.is_empty() {
                KnowledgeCalibrationDecision::SystemCalibrated
            } else {
                KnowledgeCalibrationDecision::KeepCandidate
            };
            confidence = if quality_issues.is_empty() {
                0.88
            } else {
                0.42
            };
            method_detail = json!({
                "eval_context": {
                    "expected_evidence_hit": context.expected_evidence_hit,
                    "forbidden_conclusion_hit": context.forbidden_conclusion_hit,
                    "reviewer_status": context.reviewer_status,
                    "source_boundary_confirmed": context.source_boundary_confirmed,
                }
            });
        }
        KnowledgeCalibrationMethod::Rqa => {
            let context = input
                .rqa_context
                .as_ref()
                .ok_or_else(|| anyhow!("RQA calibration requires rqa_context"))?;
            quality_issues.extend(
                context
                    .blocking_quality_issues
                    .iter()
                    .filter_map(|issue| bounded_optional_text(issue, 160)),
            );
            if context.failure_cluster_refs.is_empty() {
                quality_issues.push("failure_cluster_ref_missing".to_string());
            }
            if context.governance_task_refs.is_empty() {
                quality_issues.push("governance_task_ref_missing".to_string());
            }
            if context.proposed_fix_refs.is_empty() {
                quality_issues.push("proposed_fix_ref_missing".to_string());
            }
            source_boundary = json!({
                "rqa_report_refs": context.rqa_report_refs,
                "failure_cluster_refs": context.failure_cluster_refs,
                "governance_task_refs": context.governance_task_refs,
                "proposed_fix_refs": context.proposed_fix_refs,
                "retrieval_quality_issue_count": context.retrieval_quality_issues.len(),
            });
            evidence_refs.extend(context.rqa_report_refs.iter().cloned());
            decision = if quality_issues.is_empty() {
                KnowledgeCalibrationDecision::SystemCalibrated
            } else {
                KnowledgeCalibrationDecision::KeepCandidate
            };
            confidence = if quality_issues.is_empty() { 0.83 } else { 0.4 };
            method_detail = json!({
                "rqa_context": {
                    "retrieval_quality_issues": context.retrieval_quality_issues,
                    "blocking_quality_issue_count": context.blocking_quality_issues.len(),
                    "failure_cluster_refs": context.failure_cluster_refs,
                    "governance_task_refs": context.governance_task_refs,
                    "proposed_fix_refs": context.proposed_fix_refs,
                    "rqa_report_refs": context.rqa_report_refs,
                }
            });
        }
        KnowledgeCalibrationMethod::LlmEvidenceJudge => {
            let config = input.llm_config.as_ref().ok_or_else(|| {
                llm_config_missing = true;
                anyhow!("LLM evidence judge calibration requires configured LLM")
            })?;
            let judgement = input
                .llm_judgement
                .as_ref()
                .ok_or_else(|| anyhow!("LLM evidence judge calibration requires judge output"))?;
            config_summary = Some(config.safe_summary());
            decision = judgement.decision;
            confidence = judgement.confidence;
            evidence_refs.extend(judgement.evidence_refs.clone());
            source_boundary = judgement.source_boundary.clone();
            quality_issues.extend(judgement.quality_issues.clone());
            forbidden_conclusion = judgement.forbidden_conclusion_detected;
            if judgement.forbidden_conclusion_detected {
                decision = KnowledgeCalibrationDecision::Rejected;
                quality_issues.push("llm_forbidden_conclusion_detected".to_string());
            }
            method_detail = json!({
                "llm_evidence_judge": {
                    "profile_id": config.profile_id,
                    "profile_contract_version": config.profile_contract_version,
                    "config_digest": config.config_digest,
                    "model_capability": config.model_capability,
                    "reasoning_effort": config.reasoning_effort,
                    "raw_output_stored": false,
                }
            });
        }
    }

    if !(0.0..=1.0).contains(&confidence) {
        return Err(anyhow!(
            "knowledge calibration confidence must be between 0 and 1"
        ));
    }
    let evidence_refs = normalize_knowledge_refs("calibration evidence_refs", evidence_refs)?;
    if decision == KnowledgeCalibrationDecision::SystemCalibrated {
        if confidence < 0.8 {
            quality_issues.push("confidence_below_system_calibrated_threshold".to_string());
            decision = KnowledgeCalibrationDecision::KeepCandidate;
        }
        if !source_boundary_is_nonempty(&source_boundary) {
            quality_issues.push("source_boundary_missing".to_string());
            decision = KnowledgeCalibrationDecision::KeepCandidate;
        }
        if evidence_refs.is_empty() {
            quality_issues.push("evidence_ref_missing".to_string());
            decision = KnowledgeCalibrationDecision::KeepCandidate;
        }
    }
    let coverage_matrix = knowledge_calibration_coverage_matrix(
        item.kind,
        input.method,
        decision,
        confidence,
        source_boundary_is_nonempty(&source_boundary),
        !evidence_refs.is_empty(),
        forbidden_conclusion,
        reviewer_downgrade,
        llm_config_missing,
    );
    Ok(KnowledgeCalibrationOutcome {
        decision,
        confidence,
        evidence_refs,
        source_boundary: canonical_json_value(&source_boundary),
        quality_issues: normalized_calibration_issues(quality_issues)?,
        coverage_matrix,
        config_summary,
        method_detail,
    })
}

#[allow(clippy::too_many_arguments)]
fn knowledge_calibration_coverage_matrix(
    kind: KnowledgeItemKind,
    method: KnowledgeCalibrationMethod,
    decision: KnowledgeCalibrationDecision,
    confidence: f64,
    source_boundary_present: bool,
    evidence_ref_present: bool,
    forbidden_conclusion: bool,
    reviewer_downgrade: bool,
    llm_config_missing: bool,
) -> Value {
    json!({
        "object": "tonglingyu.knowledge_calibration_coverage_matrix",
        "schema_version": KNOWLEDGE_CALIBRATION_REPORT_SCHEMA_VERSION,
        "kind": kind.as_str(),
        "method": method.as_str(),
        "decision": decision.as_str(),
        "source_boundary_present": source_boundary_present,
        "evidence_ref_present": evidence_ref_present,
        "low_confidence": confidence < 0.8,
        "forbidden_conclusion": forbidden_conclusion,
        "reviewer_downgrade": reviewer_downgrade,
        "llm_config_missing": llm_config_missing,
        "runtime_policy_rejected": decision == KnowledgeCalibrationDecision::SystemCalibrated,
        "runtime_policy_reason": "system_calibrated_requires_explicit_runtime_policy_release_run",
        "runtime_usable_auto_promotion": false,
        "groups": [
            kind.as_str(),
            if source_boundary_present { "source_boundary_present" } else { "source_boundary_missing" },
            if confidence < 0.8 { "low_confidence" } else { "confidence_ok" },
            if forbidden_conclusion { "forbidden_conclusion" } else { "no_forbidden_conclusion" },
            if reviewer_downgrade { "reviewer_downgrade" } else { "reviewer_ok_or_not_applicable" },
            if llm_config_missing { "llm_config_missing" } else { "llm_config_bound_or_not_applicable" },
            "runtime_policy_rejected_until_release"
        ]
    })
}

fn normalized_calibration_issues(issues: Vec<String>) -> Result<Vec<String>> {
    let issues = issues
        .into_iter()
        .filter_map(|issue| bounded_optional_text(&issue, 240))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if issues.len() > 64 {
        return Err(anyhow!(
            "knowledge calibration quality issues exceed 64 entries"
        ));
    }
    Ok(issues)
}

fn source_boundary_is_nonempty(value: &Value) -> bool {
    value.as_object().is_some_and(|object| !object.is_empty())
}

#[allow(clippy::too_many_arguments)]
fn insert_knowledge_calibration_report(
    conn: &Connection,
    item: &KnowledgeItemRecord,
    input_kind: KnowledgeCalibrationInputKind,
    input_ref: &str,
    method: KnowledgeCalibrationMethod,
    outcome: &KnowledgeCalibrationOutcome,
    trace_id: &str,
    actor: &str,
) -> Result<KnowledgeCalibrationReportRecord> {
    if outcome.decision == KnowledgeCalibrationDecision::SystemCalibrated {
        if outcome.evidence_refs.is_empty() {
            return Err(anyhow!(
                "system_calibrated knowledge calibration requires evidence refs"
            ));
        }
        if !source_boundary_is_nonempty(&outcome.source_boundary) {
            return Err(anyhow!(
                "system_calibrated knowledge calibration requires source boundary"
            ));
        }
    }
    let report_id = format!("kcr-{}", uuid::Uuid::now_v7().simple());
    let report_ref = format!("runtime://tonglingyu/knowledge/calibration_reports/{report_id}");
    let created_at = now_rfc3339();
    let config_summary = outcome.config_summary.clone();
    if let Some(summary) = &config_summary {
        validate_calibration_privacy(summary)?;
    }
    let report = canonical_json_value(&json!({
        "object": "tonglingyu.knowledge_calibration_report",
        "schema_version": KNOWLEDGE_CALIBRATION_REPORT_SCHEMA_VERSION,
        "report_id": &report_id,
        "report_ref": &report_ref,
        "item_id": &item.item_id,
        "kind": item.kind.as_str(),
        "method": method.as_str(),
        "decision": outcome.decision.as_str(),
        "confidence": outcome.confidence,
        "quality_issues": outcome.quality_issues,
        "source_refs": item.source_refs,
        "evidence_refs": outcome.evidence_refs,
        "source_boundary": outcome.source_boundary,
        "input_kind": input_kind.as_str(),
        "input_ref_sha256": hash_text(input_ref),
        "payload_sha256": item.payload_sha256,
        "calibration_method_detail": outcome.method_detail,
        "coverage_matrix": outcome.coverage_matrix,
        "config_summary": config_summary,
        "raw_question_stored": false,
        "unredacted_query_stored": false,
        "secret_values_stored": false,
        "full_prompt_stored": false,
        "fact_layer_mutated": false,
        "runtime_usable_auto_promotion": false,
        "created_at": &created_at,
    }));
    validate_calibration_privacy(&report)?;
    let report_hash = hash_text(&serde_json::to_string(&report)?);
    conn.execute(
        r#"
        INSERT INTO knowledge_calibration_reports (
            report_id, report_ref, item_id, kind, method, decision, confidence,
            quality_issues_json, source_refs_json, evidence_refs_json,
            source_boundary_json, coverage_matrix_json, config_summary_json,
            report_json, report_hash, schema_version, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
        "#,
        params![
            &report_id,
            &report_ref,
            &item.item_id,
            item.kind.as_str(),
            method.as_str(),
            outcome.decision.as_str(),
            outcome.confidence,
            serde_json::to_string(&outcome.quality_issues)?,
            serde_json::to_string(&item.source_refs)?,
            serde_json::to_string(&outcome.evidence_refs)?,
            serde_json::to_string(&outcome.source_boundary)?,
            serde_json::to_string(&outcome.coverage_matrix)?,
            optional_json_string(config_summary.as_ref())?,
            serde_json::to_string(&report)?,
            &report_hash,
            KNOWLEDGE_CALIBRATION_REPORT_SCHEMA_VERSION,
            &created_at,
        ],
    )?;
    append_runtime_audit_event(
        conn,
        trace_id,
        "knowledge_calibration_report_created",
        &json!({
            "report_id": &report_id,
            "report_ref": &report_ref,
            "item_id": &item.item_id,
            "kind": item.kind.as_str(),
            "method": method.as_str(),
            "decision": outcome.decision.as_str(),
            "confidence": outcome.confidence,
            "quality_issue_count": outcome.quality_issues.len(),
            "report_hash": &report_hash,
            "config_digest": config_summary.as_ref().and_then(|value| value.get("config_digest")).and_then(Value::as_str),
            "contains_secret_values": false,
            "actor": actor,
        }),
    )?;
    read_knowledge_calibration_report(conn, &report_id)?
        .ok_or_else(|| anyhow!("knowledge calibration report was not readable after insert"))
}

fn update_knowledge_item_state_from_calibration(
    conn: &Connection,
    item: &KnowledgeItemRecord,
    target_state: KnowledgeState,
    report: &KnowledgeCalibrationReportRecord,
    trace_id: &str,
    actor: &str,
) -> Result<Option<KnowledgeItemRecord>> {
    if target_state == KnowledgeState::SystemCalibrated && report.report_ref.trim().is_empty() {
        return Err(anyhow!(
            "system_calibrated knowledge item requires calibration report ref"
        ));
    }
    let now = now_rfc3339();
    let next_state_version = item.state_version + 1;
    let updated = conn.execute(
        r#"
        UPDATE knowledge_items
        SET state = ?2,
            evidence_refs_json = ?3,
            source_boundary_json = ?4,
            calibration_report_ref = ?5,
            confidence = ?6,
            updated_at = ?7,
            state_version = ?8
        WHERE item_id = ?1 AND state_version = ?9
        "#,
        params![
            &item.item_id,
            target_state.as_str(),
            serde_json::to_string(&report.evidence_refs)?,
            serde_json::to_string(&report.source_boundary)?,
            &report.report_ref,
            report.confidence,
            &now,
            next_state_version,
            item.state_version,
        ],
    )?;
    if updated == 0 {
        return Err(anyhow!("knowledge calibration state update conflict"));
    }
    let reason = format!(
        "knowledge calibration {} via {} report {}",
        report.decision.as_str(),
        report.method.as_str(),
        report.report_ref
    );
    insert_knowledge_item_state_history(
        conn,
        KnowledgeItemStateHistoryInsert {
            item_id: &item.item_id,
            previous_state: Some(item.state),
            new_state: target_state,
            actor,
            reason: &reason,
            evidence_refs: &report.evidence_refs,
            state_version: next_state_version,
            created_at: &now,
        },
    )?;
    let record = load_knowledge_item(conn, &item.item_id)?
        .ok_or_else(|| anyhow!("knowledge item disappeared after calibration state update"))?;
    append_runtime_audit_event(
        conn,
        trace_id,
        "knowledge_item_state_updated",
        &json!({
            "item_id": &record.item_id,
            "kind": record.kind.as_str(),
            "previous_state": item.state.as_str(),
            "new_state": record.state.as_str(),
            "state_version": record.state_version,
            "actor": actor,
            "reason_sha256": hash_text(&reason),
            "evidence_ref_count": record.evidence_refs.len(),
            "source_boundary_present": record.source_boundary.is_some(),
            "calibration_report_ref": record.calibration_report_ref,
            "confidence": record.confidence,
            "runtime_usable_auto_promotion": false,
        }),
    )?;
    Ok(Some(record))
}

pub fn read_knowledge_calibration_report(
    conn: &Connection,
    report_id: &str,
) -> Result<Option<KnowledgeCalibrationReportRecord>> {
    conn.query_row(
        r#"
        SELECT
            report_id, report_ref, item_id, kind, method, decision, confidence,
            quality_issues_json, source_refs_json, evidence_refs_json,
            source_boundary_json, coverage_matrix_json, config_summary_json,
            report_json, report_hash, schema_version, created_at
        FROM knowledge_calibration_reports
        WHERE report_id = ?1
        "#,
        params![report_id],
        knowledge_calibration_report_sql_row,
    )
    .optional()?
    .map(knowledge_calibration_report_from_sql_row)
    .transpose()
}

struct KnowledgeCalibrationReportSqlRow {
    report_id: String,
    report_ref: String,
    item_id: String,
    kind: String,
    method: String,
    decision: String,
    confidence: f64,
    quality_issues_json: String,
    source_refs_json: String,
    evidence_refs_json: String,
    source_boundary_json: String,
    coverage_matrix_json: String,
    config_summary_json: Option<String>,
    report_json: String,
    report_hash: String,
    schema_version: String,
    created_at: String,
}

fn knowledge_calibration_report_sql_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<KnowledgeCalibrationReportSqlRow> {
    Ok(KnowledgeCalibrationReportSqlRow {
        report_id: row.get(0)?,
        report_ref: row.get(1)?,
        item_id: row.get(2)?,
        kind: row.get(3)?,
        method: row.get(4)?,
        decision: row.get(5)?,
        confidence: row.get(6)?,
        quality_issues_json: row.get(7)?,
        source_refs_json: row.get(8)?,
        evidence_refs_json: row.get(9)?,
        source_boundary_json: row.get(10)?,
        coverage_matrix_json: row.get(11)?,
        config_summary_json: row.get(12)?,
        report_json: row.get(13)?,
        report_hash: row.get(14)?,
        schema_version: row.get(15)?,
        created_at: row.get(16)?,
    })
}

fn knowledge_calibration_report_from_sql_row(
    row: KnowledgeCalibrationReportSqlRow,
) -> Result<KnowledgeCalibrationReportRecord> {
    Ok(KnowledgeCalibrationReportRecord {
        report_id: row.report_id,
        report_ref: row.report_ref,
        item_id: row.item_id,
        kind: KnowledgeItemKind::parse(&row.kind)?,
        method: KnowledgeCalibrationMethod::parse(&row.method)?,
        decision: KnowledgeCalibrationDecision::parse(&row.decision)?,
        confidence: row.confidence,
        quality_issues: serde_json::from_str(&row.quality_issues_json)?,
        source_refs: serde_json::from_str(&row.source_refs_json)?,
        evidence_refs: serde_json::from_str(&row.evidence_refs_json)?,
        source_boundary: serde_json::from_str(&row.source_boundary_json)?,
        coverage_matrix: serde_json::from_str(&row.coverage_matrix_json)?,
        config_summary: row
            .config_summary_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        report_hash: row.report_hash,
        schema_version: row.schema_version,
        created_at: row.created_at,
        report: serde_json::from_str(&row.report_json)?,
    })
}

fn validate_calibration_privacy(value: &Value) -> Result<()> {
    validate_calibration_privacy_inner(value, "")
}

fn validate_calibration_privacy_inner(value: &Value, path: &str) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let lower = key.to_ascii_lowercase();
                if key != "contains_secret_values"
                    && matches!(
                        lower.as_str(),
                        "api_key"
                            | "password"
                            | "token"
                            | "secret"
                            | "raw_question"
                            | "unredacted_query"
                            | "full_prompt"
                            | "system_prompt"
                    )
                {
                    return Err(anyhow!(
                        "knowledge calibration report contains forbidden private field {path}.{key}"
                    ));
                }
                validate_calibration_privacy_inner(child, key)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                validate_calibration_privacy_inner(item, path)?;
            }
        }
        Value::String(text) if text.contains("sk-") || text.contains("-----BEGIN") => {
            return Err(anyhow!(
                "knowledge calibration report contains secret-looking string at {path}"
            ));
        }
        _ => {}
    }
    Ok(())
}

pub fn create_knowledge_calibration_job(
    conn: &Connection,
    input: KnowledgeCalibrationJobCreateInput,
) -> Result<KnowledgeCalibrationJobRecord> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = create_knowledge_calibration_job_inner(conn, input);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        create_knowledge_calibration_job_inner(conn, input)
    }
}

fn create_knowledge_calibration_job_inner(
    conn: &Connection,
    input: KnowledgeCalibrationJobCreateInput,
) -> Result<KnowledgeCalibrationJobRecord> {
    let item = load_knowledge_item(conn, &input.item_id)?.ok_or_else(|| {
        anyhow!(
            "knowledge calibration job item not found: {}",
            input.item_id
        )
    })?;
    if item.state != KnowledgeState::Candidate {
        return Err(anyhow!(
            "knowledge calibration job requires candidate item, got {}",
            item.state.as_str()
        ));
    }
    let input_ref = bounded_optional_text(&input.input_ref, 240)
        .ok_or_else(|| anyhow!("knowledge calibration job input_ref is required"))?;
    let idempotency_key = bounded_optional_text(&input.idempotency_key, 180)
        .ok_or_else(|| anyhow!("knowledge calibration job idempotency_key is required"))?;
    if let Some(existing) = load_knowledge_calibration_job_by_idempotency(conn, &idempotency_key)? {
        return Ok(existing);
    }
    let trace_id = validate_knowledge_item_trace_id(&input.trace_id)?;
    let concurrency_key = bounded_optional_text(&input.concurrency_key, 180)
        .ok_or_else(|| anyhow!("knowledge calibration job concurrency_key is required"))?;
    let retry_limit = input.retry_limit.clamp(1, 8);
    let input_digest = hash_text(&serde_json::to_string(&canonical_json_value(&json!({
        "input_kind": input.input_kind.as_str(),
        "input_ref": input_ref,
        "item_id": item.item_id,
        "method": input.method.as_str(),
        "payload_sha256": item.payload_sha256,
        "config_digest": input.config_digest,
    })))?);
    let job_id = format!("kcj-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO knowledge_calibration_jobs (
            job_id, status, input_kind, input_ref, item_id, input_digest,
            idempotency_key, trace_id, method, config_digest, retry_limit,
            attempt_count, concurrency_key, lease_owner, lease_expires_at,
            heartbeat_at, report_id, last_error_sha256, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12,
            NULL, NULL, NULL, NULL, NULL, ?13, ?13
        )
        "#,
        params![
            &job_id,
            KnowledgeCalibrationJobStatus::Queued.as_str(),
            input.input_kind.as_str(),
            &input_ref,
            &item.item_id,
            &input_digest,
            &idempotency_key,
            &trace_id,
            input.method.as_str(),
            &input.config_digest,
            retry_limit,
            &concurrency_key,
            &now,
        ],
    )?;
    insert_knowledge_calibration_job_history(
        conn,
        &job_id,
        None,
        KnowledgeCalibrationJobStatus::Queued,
        "system",
        "knowledge calibration job created",
        None,
        None,
        0,
        &now,
    )?;
    append_runtime_audit_event(
        conn,
        &trace_id,
        "knowledge_calibration_job_created",
        &json!({
            "job_id": &job_id,
            "item_id": &item.item_id,
            "input_kind": input.input_kind.as_str(),
            "input_ref_sha256": hash_text(&input_ref),
            "input_digest": &input_digest,
            "idempotency_key_sha256": hash_text(&idempotency_key),
            "method": input.method.as_str(),
            "config_digest": input.config_digest,
            "retry_limit": retry_limit,
            "concurrency_key_sha256": hash_text(&concurrency_key),
        }),
    )?;
    load_knowledge_calibration_job(conn, &job_id)?
        .ok_or_else(|| anyhow!("knowledge calibration job was not readable after insert"))
}

pub fn lease_knowledge_calibration_job(
    conn: &Connection,
    job_id: &str,
    lease_owner: &str,
    lease_seconds: u64,
) -> Result<Option<KnowledgeCalibrationJobRecord>> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result =
            lease_knowledge_calibration_job_inner(conn, job_id, lease_owner, lease_seconds);
        match result {
            Ok(record) => {
                conn.execute_batch("COMMIT")?;
                Ok(record)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        lease_knowledge_calibration_job_inner(conn, job_id, lease_owner, lease_seconds)
    }
}

fn lease_knowledge_calibration_job_inner(
    conn: &Connection,
    job_id: &str,
    lease_owner: &str,
    lease_seconds: u64,
) -> Result<Option<KnowledgeCalibrationJobRecord>> {
    let Some(job) = load_knowledge_calibration_job(conn, job_id)? else {
        return Ok(None);
    };
    let owner = bounded_optional_text(lease_owner, 120)
        .ok_or_else(|| anyhow!("knowledge calibration lease_owner is required"))?;
    let now = now_rfc3339();
    if !matches!(
        job.status,
        KnowledgeCalibrationJobStatus::Queued
            | KnowledgeCalibrationJobStatus::RetryWaiting
            | KnowledgeCalibrationJobStatus::Running
    ) {
        return Err(anyhow!(
            "knowledge calibration job {} is not leasable from status {}",
            job.job_id,
            job.status.as_str()
        ));
    }
    if job.status == KnowledgeCalibrationJobStatus::Running
        && job
            .lease_expires_at
            .as_deref()
            .is_some_and(|expires_at| expires_at > now.as_str())
        && job.lease_owner.as_deref() != Some(owner.as_str())
    {
        return Err(anyhow!("knowledge calibration job lease is still active"));
    }
    ensure_no_active_calibration_job_for_concurrency_key(
        conn,
        &job.concurrency_key,
        &job.job_id,
        &now,
    )?;
    let lease_seconds = lease_seconds.max(1);
    let lease_expires_at = rfc3339_after_seconds(lease_seconds)?;
    let attempt_count = if job.status == KnowledgeCalibrationJobStatus::Running {
        job.attempt_count
    } else {
        job.attempt_count + 1
    };
    conn.execute(
        r#"
        UPDATE knowledge_calibration_jobs
        SET status = ?2,
            lease_owner = ?3,
            lease_expires_at = ?4,
            heartbeat_at = ?5,
            attempt_count = ?6,
            updated_at = ?5
        WHERE job_id = ?1
        "#,
        params![
            &job.job_id,
            KnowledgeCalibrationJobStatus::Running.as_str(),
            &owner,
            &lease_expires_at,
            &now,
            attempt_count,
        ],
    )?;
    insert_knowledge_calibration_job_history(
        conn,
        &job.job_id,
        Some(job.status),
        KnowledgeCalibrationJobStatus::Running,
        &owner,
        "knowledge calibration job leased",
        Some(&owner),
        job.report_id.as_deref(),
        attempt_count,
        &now,
    )?;
    append_runtime_audit_event(
        conn,
        &job.trace_id,
        "knowledge_calibration_job_leased",
        &json!({
            "job_id": &job.job_id,
            "item_id": &job.item_id,
            "previous_status": job.status.as_str(),
            "new_status": KnowledgeCalibrationJobStatus::Running.as_str(),
            "lease_owner_sha256": hash_text(&owner),
            "lease_expires_at": lease_expires_at,
            "attempt_count": attempt_count,
            "concurrency_key_sha256": hash_text(&job.concurrency_key),
        }),
    )?;
    load_knowledge_calibration_job(conn, &job.job_id)
}

pub fn heartbeat_knowledge_calibration_job(
    conn: &Connection,
    job_id: &str,
    lease_owner: &str,
) -> Result<Option<KnowledgeCalibrationJobRecord>> {
    let Some(job) = load_knowledge_calibration_job(conn, job_id)? else {
        return Ok(None);
    };
    let owner = bounded_optional_text(lease_owner, 120)
        .ok_or_else(|| anyhow!("knowledge calibration lease_owner is required"))?;
    ensure_calibration_job_lease_owner(&job, &owner)?;
    let now = now_rfc3339();
    let lease_expires_at = rfc3339_after_seconds(KNOWLEDGE_CALIBRATION_DEFAULT_LEASE_SECONDS)?;
    conn.execute(
        r#"
        UPDATE knowledge_calibration_jobs
        SET heartbeat_at = ?2,
            lease_expires_at = ?3,
            updated_at = ?2
        WHERE job_id = ?1
        "#,
        params![&job.job_id, &now, &lease_expires_at],
    )?;
    append_runtime_audit_event(
        conn,
        &job.trace_id,
        "knowledge_calibration_job_heartbeat",
        &json!({
            "job_id": &job.job_id,
            "lease_owner_sha256": hash_text(&owner),
            "lease_expires_at": lease_expires_at,
        }),
    )?;
    load_knowledge_calibration_job(conn, &job.job_id)
}

pub fn complete_knowledge_calibration_job(
    conn: &Connection,
    job_id: &str,
    lease_owner: &str,
    report_id: &str,
) -> Result<Option<KnowledgeCalibrationJobRecord>> {
    let Some(job) = load_knowledge_calibration_job(conn, job_id)? else {
        return Ok(None);
    };
    let owner = bounded_optional_text(lease_owner, 120)
        .ok_or_else(|| anyhow!("knowledge calibration lease_owner is required"))?;
    ensure_calibration_job_lease_owner(&job, &owner)?;
    let report = read_knowledge_calibration_report(conn, report_id)?
        .ok_or_else(|| anyhow!("knowledge calibration report not found: {report_id}"))?;
    if report.item_id != job.item_id {
        return Err(anyhow!("knowledge calibration job report item mismatch"));
    }
    let now = now_rfc3339();
    conn.execute(
        r#"
        UPDATE knowledge_calibration_jobs
        SET status = ?2,
            lease_owner = NULL,
            lease_expires_at = NULL,
            heartbeat_at = ?3,
            report_id = ?4,
            updated_at = ?3
        WHERE job_id = ?1
        "#,
        params![
            &job.job_id,
            KnowledgeCalibrationJobStatus::Succeeded.as_str(),
            &now,
            &report.report_id,
        ],
    )?;
    insert_knowledge_calibration_job_history(
        conn,
        &job.job_id,
        Some(job.status),
        KnowledgeCalibrationJobStatus::Succeeded,
        &owner,
        "knowledge calibration job completed",
        Some(&owner),
        Some(&report.report_id),
        job.attempt_count,
        &now,
    )?;
    append_runtime_audit_event(
        conn,
        &job.trace_id,
        "knowledge_calibration_job_completed",
        &json!({
            "job_id": &job.job_id,
            "item_id": &job.item_id,
            "report_id": &report.report_id,
            "report_ref": &report.report_ref,
            "lease_owner_sha256": hash_text(&owner),
            "attempt_count": job.attempt_count,
        }),
    )?;
    load_knowledge_calibration_job(conn, &job.job_id)
}

pub fn fail_knowledge_calibration_job(
    conn: &Connection,
    job_id: &str,
    lease_owner: &str,
    error: &str,
    retryable: bool,
) -> Result<Option<KnowledgeCalibrationJobRecord>> {
    let Some(job) = load_knowledge_calibration_job(conn, job_id)? else {
        return Ok(None);
    };
    let owner = bounded_optional_text(lease_owner, 120)
        .ok_or_else(|| anyhow!("knowledge calibration lease_owner is required"))?;
    ensure_calibration_job_lease_owner(&job, &owner)?;
    let error_text = bounded_optional_text(error, 480)
        .ok_or_else(|| anyhow!("knowledge calibration failure error is required"))?;
    let next_status = if retryable && job.attempt_count < job.retry_limit {
        KnowledgeCalibrationJobStatus::RetryWaiting
    } else {
        KnowledgeCalibrationJobStatus::Failed
    };
    let now = now_rfc3339();
    conn.execute(
        r#"
        UPDATE knowledge_calibration_jobs
        SET status = ?2,
            lease_owner = NULL,
            lease_expires_at = NULL,
            heartbeat_at = ?3,
            last_error_sha256 = ?4,
            updated_at = ?3
        WHERE job_id = ?1
        "#,
        params![
            &job.job_id,
            next_status.as_str(),
            &now,
            hash_text(&error_text)
        ],
    )?;
    insert_knowledge_calibration_job_history(
        conn,
        &job.job_id,
        Some(job.status),
        next_status,
        &owner,
        &error_text,
        Some(&owner),
        job.report_id.as_deref(),
        job.attempt_count,
        &now,
    )?;
    append_runtime_audit_event(
        conn,
        &job.trace_id,
        "knowledge_calibration_job_failed",
        &json!({
            "job_id": &job.job_id,
            "item_id": &job.item_id,
            "new_status": next_status.as_str(),
            "retryable": retryable,
            "attempt_count": job.attempt_count,
            "retry_limit": job.retry_limit,
            "error_sha256": hash_text(&error_text),
            "lease_owner_sha256": hash_text(&owner),
        }),
    )?;
    load_knowledge_calibration_job(conn, &job.job_id)
}

fn ensure_calibration_job_lease_owner(
    job: &KnowledgeCalibrationJobRecord,
    owner: &str,
) -> Result<()> {
    if job.status != KnowledgeCalibrationJobStatus::Running {
        return Err(anyhow!("knowledge calibration job is not running"));
    }
    if job.lease_owner.as_deref() != Some(owner) {
        return Err(anyhow!("knowledge calibration job lease owner mismatch"));
    }
    Ok(())
}

fn ensure_no_active_calibration_job_for_concurrency_key(
    conn: &Connection,
    concurrency_key: &str,
    job_id: &str,
    now: &str,
) -> Result<()> {
    let active_count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM knowledge_calibration_jobs
        WHERE concurrency_key = ?1
          AND job_id <> ?2
          AND status = 'running'
          AND lease_expires_at > ?3
        "#,
        params![concurrency_key, job_id, now],
        |row| row.get(0),
    )?;
    if active_count > 0 {
        return Err(anyhow!(
            "knowledge calibration concurrency limit reached for key"
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_knowledge_calibration_job_history(
    conn: &Connection,
    job_id: &str,
    previous_status: Option<KnowledgeCalibrationJobStatus>,
    new_status: KnowledgeCalibrationJobStatus,
    actor: &str,
    reason: &str,
    lease_owner: Option<&str>,
    report_id: Option<&str>,
    attempt_count: u32,
    created_at: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO knowledge_calibration_job_history (
            history_id, job_id, previous_status, new_status, actor, reason_sha256,
            lease_owner, report_id, attempt_count, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            format!("kcjh-{}", uuid::Uuid::now_v7().simple()),
            job_id,
            previous_status.map(KnowledgeCalibrationJobStatus::as_str),
            new_status.as_str(),
            actor,
            hash_text(reason),
            lease_owner,
            report_id,
            attempt_count,
            created_at,
        ],
    )?;
    Ok(())
}

fn load_knowledge_calibration_job_by_idempotency(
    conn: &Connection,
    idempotency_key: &str,
) -> Result<Option<KnowledgeCalibrationJobRecord>> {
    conn.query_row(
        &format!(
            "{} WHERE idempotency_key = ?1",
            knowledge_calibration_job_select_sql()
        ),
        params![idempotency_key],
        knowledge_calibration_job_sql_row,
    )
    .optional()?
    .map(knowledge_calibration_job_from_sql_row)
    .transpose()
}

fn load_knowledge_calibration_job(
    conn: &Connection,
    job_id: &str,
) -> Result<Option<KnowledgeCalibrationJobRecord>> {
    conn.query_row(
        &format!(
            "{} WHERE job_id = ?1",
            knowledge_calibration_job_select_sql()
        ),
        params![job_id],
        knowledge_calibration_job_sql_row,
    )
    .optional()?
    .map(knowledge_calibration_job_from_sql_row)
    .transpose()
}

fn knowledge_calibration_job_select_sql() -> &'static str {
    r#"
    SELECT
        job_id, status, input_kind, input_ref, item_id, input_digest,
        idempotency_key, trace_id, method, config_digest, retry_limit,
        attempt_count, concurrency_key, lease_owner, lease_expires_at,
        heartbeat_at, report_id, last_error_sha256, created_at, updated_at
    FROM knowledge_calibration_jobs
    "#
}

struct KnowledgeCalibrationJobSqlRow {
    job_id: String,
    status: String,
    input_kind: String,
    input_ref: String,
    item_id: String,
    input_digest: String,
    idempotency_key: String,
    trace_id: String,
    method: String,
    config_digest: Option<String>,
    retry_limit: u32,
    attempt_count: u32,
    concurrency_key: String,
    lease_owner: Option<String>,
    lease_expires_at: Option<String>,
    heartbeat_at: Option<String>,
    report_id: Option<String>,
    last_error_sha256: Option<String>,
    created_at: String,
    updated_at: String,
}

fn knowledge_calibration_job_sql_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<KnowledgeCalibrationJobSqlRow> {
    Ok(KnowledgeCalibrationJobSqlRow {
        job_id: row.get(0)?,
        status: row.get(1)?,
        input_kind: row.get(2)?,
        input_ref: row.get(3)?,
        item_id: row.get(4)?,
        input_digest: row.get(5)?,
        idempotency_key: row.get(6)?,
        trace_id: row.get(7)?,
        method: row.get(8)?,
        config_digest: row.get(9)?,
        retry_limit: row.get(10)?,
        attempt_count: row.get(11)?,
        concurrency_key: row.get(12)?,
        lease_owner: row.get(13)?,
        lease_expires_at: row.get(14)?,
        heartbeat_at: row.get(15)?,
        report_id: row.get(16)?,
        last_error_sha256: row.get(17)?,
        created_at: row.get(18)?,
        updated_at: row.get(19)?,
    })
}

fn knowledge_calibration_job_from_sql_row(
    row: KnowledgeCalibrationJobSqlRow,
) -> Result<KnowledgeCalibrationJobRecord> {
    Ok(KnowledgeCalibrationJobRecord {
        job_id: row.job_id,
        status: KnowledgeCalibrationJobStatus::parse(&row.status)?,
        input_kind: KnowledgeCalibrationInputKind::parse(&row.input_kind)?,
        input_ref: row.input_ref,
        item_id: row.item_id,
        input_digest: row.input_digest,
        idempotency_key: row.idempotency_key,
        trace_id: row.trace_id,
        method: KnowledgeCalibrationMethod::parse(&row.method)?,
        config_digest: row.config_digest,
        retry_limit: row.retry_limit,
        attempt_count: row.attempt_count,
        concurrency_key: row.concurrency_key,
        lease_owner: row.lease_owner,
        lease_expires_at: row.lease_expires_at,
        heartbeat_at: row.heartbeat_at,
        report_id: row.report_id,
        last_error_sha256: row.last_error_sha256,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn rfc3339_after_seconds(seconds: u64) -> Result<String> {
    let duration = time::Duration::seconds(i64::try_from(seconds).unwrap_or(i64::MAX));
    Ok((OffsetDateTime::now_utc() + duration)
        .format(&time::format_description::well_known::Rfc3339)?)
}

pub fn create_knowledge_patch_proposal(
    conn: &Connection,
    input: KnowledgePatchProposalCreateInput,
) -> Result<Value> {
    if conn.is_autocommit() {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = create_knowledge_patch_proposal_inner(conn, input);
        match result {
            Ok(value) => {
                conn.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    } else {
        create_knowledge_patch_proposal_inner(conn, input)
    }
}

fn create_knowledge_patch_proposal_inner(
    conn: &Connection,
    input: KnowledgePatchProposalCreateInput,
) -> Result<Value> {
    let proposal_type = normalize_knowledge_patch_proposal_type(&input.proposal_type)?;
    let trace_id = input.trace_id.trim();
    if trace_id.is_empty() {
        return Err(anyhow!("knowledge patch proposal trace_id is required"));
    }
    let package_id = input
        .package_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let source_ref = input
        .source_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            package_id
                .as_ref()
                .map(|package_id| format!("package:{package_id}"))
        })
        .unwrap_or_else(|| format!("trace:{trace_id}"));
    let payload = canonical_json_value(&input.payload);
    validate_knowledge_patch_proposal_payload(&proposal_type, &payload)?;
    let payload_json = serde_json::to_string(&payload)?;
    if payload_json.len() > 8_192 {
        return Err(anyhow!(
            "knowledge patch proposal payload exceeds 8192 byte limit"
        ));
    }
    let payload_sha256 = hash_text(&payload_json);
    if let Some(existing) = load_knowledge_patch_proposal_by_identity(
        conn,
        &proposal_type,
        &source_ref,
        &payload_sha256,
    )? {
        let task = load_governance_task(conn, &existing.task_id)?
            .ok_or_else(|| anyhow!("knowledge patch proposal task is missing"))?;
        return Ok(knowledge_patch_proposal_create_json(&existing, &task));
    }

    let proposal_id = format!("kpp-{}", uuid::Uuid::now_v7().simple());
    let task_type = knowledge_patch_proposal_task_type(&proposal_type);
    let priority = input.priority.unwrap_or_else(|| "p1".to_string());
    validate_governance_task_priority(&priority)?;
    let proposed_fix = format!(
        "knowledge_patch_proposal; proposal_type={}; payload_sha256={}; no_direct_fact_mutation=true",
        proposal_type, payload_sha256
    );
    let task = create_governance_task_inner(
        conn,
        KnowledgeGovernanceTaskCreateInput {
            source_entity_type: "knowledge_patch_proposal".to_string(),
            source_entity_id: proposal_id.clone(),
            trace_id: trace_id.to_string(),
            package_id: package_id.clone(),
            source_failure_id: None,
            task_type: task_type.to_string(),
            priority: Some(priority),
            proposed_fix: Some(proposed_fix),
            agent_cluster_key: Some(format!(
                "knowledge_patch_proposal:{}:{}",
                proposal_type,
                &payload_sha256[..12]
            )),
        },
    )?;
    let created_by = input
        .created_by
        .as_deref()
        .and_then(|value| bounded_optional_text(value, 80));
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO knowledge_patch_proposals (
            proposal_id, proposal_type, trace_id, package_id, source_ref,
            payload_json, payload_sha256, task_id, created_by, created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10
        )
        "#,
        params![
            &proposal_id,
            &proposal_type,
            trace_id,
            &package_id,
            &source_ref,
            &payload_json,
            &payload_sha256,
            &task.task_id,
            &created_by,
            &now,
        ],
    )?;
    let proposal = load_knowledge_patch_proposal(conn, &proposal_id)?
        .ok_or_else(|| anyhow!("knowledge patch proposal was not readable after insert"))?;
    append_runtime_audit_event(
        conn,
        &proposal.trace_id,
        "knowledge_patch_proposal_created",
        &json!({
            "proposal_id": &proposal.proposal_id,
            "proposal_type": &proposal.proposal_type,
            "source_ref_sha256": hash_text(&proposal.source_ref),
            "payload_sha256": &proposal.payload_sha256,
            "task_id": &proposal.task_id,
            "task_type": &task.task_type,
            "package_id": &proposal.package_id,
            "created_by": &proposal.created_by,
            "direct_fact_mutation": false,
        }),
    )?;
    Ok(knowledge_patch_proposal_create_json(&proposal, &task))
}

fn record_retrieval_failure_if_needed(
    conn: &Connection,
    trace_id: &str,
    package_id: &str,
    question: &str,
    quality_report: RetrievalQualityReport,
    selected_evidence_ids: Vec<String>,
) -> Result<Option<RetrievalFailureRecord>> {
    if quality_report.production_ready {
        return Ok(None);
    }
    create_retrieval_failure(
        conn,
        RetrievalFailureCreateInput {
            trace_id: trace_id.to_string(),
            package_id: Some(package_id.to_string()),
            question: question.to_string(),
            quality_report,
            selected_evidence_ids,
            expected_evidence_ids: Vec::new(),
            agent_diagnosis: None,
            proposed_fix: None,
        },
    )
    .map(Some)
}

fn record_reviewer_failure_if_needed(
    conn: &Connection,
    input: &RuntimeWorkflowInput,
    package: &EvidencePackage,
) -> Result<Option<RetrievalFailureRecord>> {
    if package.review.status == "passed" {
        return Ok(None);
    }
    create_retrieval_failure(
        conn,
        RetrievalFailureCreateInput {
            trace_id: input.trace_id.clone(),
            package_id: Some(package.package_id.clone()),
            question: input.question.clone(),
            quality_report: reviewer_retrieval_quality_report(input, package),
            selected_evidence_ids: evidence_ids(&package.cards),
            expected_evidence_ids: Vec::new(),
            agent_diagnosis: Some(format!(
                "local_reviewer_status={}; severity={}; issue_count={}",
                package.review.status,
                package.review.severity,
                package.review.issues.len()
            )),
            proposed_fix: Some("revise_claims_or_add_supporting_evidence".to_string()),
        },
    )
    .map(Some)
}

fn reviewer_retrieval_quality_report(
    input: &RuntimeWorkflowInput,
    package: &EvidencePackage,
) -> RetrievalQualityReport {
    let selected_evidence_types = evidence_types(&package.cards);
    let selected_set = selected_evidence_types
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let required = input
        .required_evidence_types
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let missing = required
        .iter()
        .filter(|item| !selected_set.contains(*item))
        .cloned()
        .collect::<Vec<_>>();
    let redacted_terms = redacted_terms_from_question(&input.question);
    let mut issues = vec!["reviewer_evidence_insufficient".to_string()];
    issues.extend(
        package
            .review
            .issues
            .iter()
            .map(|issue| format!("reviewer_issue:{}", trim_text(issue, 120))),
    );
    RetrievalQualityReport {
        object: "tonglingyu.retrieval_quality_report".to_string(),
        schema_version: RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION.to_string(),
        tool_name: "tonglingyu.review_answer".to_string(),
        quality_status: "failed".to_string(),
        production_ready: false,
        truncated: false,
        query_summary: RetrievalQuerySummary {
            question_sha256: hash_text(&input.question),
            question_char_count: input.question.chars().count(),
            raw_question_included: false,
            redacted_terms: redacted_terms.clone(),
        },
        expanded_terms: redacted_terms,
        protected_terms: Vec::new(),
        expanded_aliases: Vec::new(),
        normalized_match_channels: BTreeMap::new(),
        candidate_count: package.cards.len(),
        selected_count: package.cards.len(),
        channel_distribution: retrieval_channel_distribution(&package.cards),
        evidence_type_coverage: RetrievalEvidenceTypeCoverage {
            required,
            selected: selected_evidence_types,
            missing,
        },
        exact_match_coverage: Vec::new(),
        expected_evidence_hit: None,
        expected_evidence_status: "not_applicable_review".to_string(),
        source_coverage_boundary: RetrievalSourceCoverageBoundary {
            source_ids: package
                .cards
                .iter()
                .map(|card| card.source_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            source_categories: Vec::new(),
            edition_boundaries: Vec::new(),
            kb_schema_version: KNOWLEDGE_BASE_SCHEMA_VERSION.to_string(),
            source_snapshot_status: if package.cards.is_empty() {
                "no_source_selected".to_string()
            } else {
                "source_snapshot_ready".to_string()
            },
            facsimile_review_status: "not_reviewed".to_string(),
            authoritative_edition_review_status: "not_reviewed".to_string(),
            scholarly_collation_status: "not_scholarly_collated".to_string(),
            expert_collation_status: "not_reviewed".to_string(),
        },
        source_usage_refs: Vec::new(),
        issues,
        recommended_follow_up: vec!["revise_claims_or_add_supporting_evidence".to_string()],
    }
}

fn redacted_terms_from_question(question: &str) -> Vec<String> {
    let mut term_set = BTreeSet::new();
    for term in sensitive_query_terms(question) {
        term_set.insert(redacted_query_term(&term));
        if term_set.len() >= RETRIEVAL_QUALITY_REPORT_MAX_TERMS {
            break;
        }
    }
    for term in cjk_tokens(question) {
        if term_set.len() >= RETRIEVAL_QUALITY_REPORT_MAX_TERMS {
            break;
        }
        term_set.insert(redacted_query_term(&term));
    }
    let mut terms = term_set.into_iter().collect::<Vec<_>>();
    if terms.is_empty() {
        terms.push(format!("sha256:{}", &hash_text(question)[..12]));
    }
    terms
}

fn sensitive_query_terms(question: &str) -> Vec<String> {
    question
        .split(query_term_delimiter)
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .filter(|term| looks_sensitive_query_term(term))
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn query_term_delimiter(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '，' | '。'
                | '、'
                | '；'
                | '！'
                | '？'
                | ','
                | ';'
                | '"'
                | '\''
                | '“'
                | '”'
                | '‘'
                | '’'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
        )
}

fn retrieval_failure_type(issues: &[String]) -> String {
    if issues.iter().any(|issue| issue == "no_evidence_selected") {
        "no_evidence_selected".to_string()
    } else if issues
        .iter()
        .any(|issue| issue.starts_with("expected_evidence_missing:"))
    {
        "expected_evidence_missing".to_string()
    } else if issues
        .iter()
        .any(|issue| issue.starts_with("missing_required_evidence_type:"))
    {
        "missing_required_evidence_type".to_string()
    } else if issues
        .iter()
        .any(|issue| issue.starts_with("required_exact_term_not_selected:"))
    {
        "exact_term_missing".to_string()
    } else if issues
        .iter()
        .any(|issue| issue.starts_with("source_usage_metadata_incomplete:"))
    {
        "source_usage_metadata_incomplete".to_string()
    } else if issues
        .iter()
        .any(|issue| issue == "reviewer_evidence_insufficient")
    {
        "reviewer_evidence_insufficient".to_string()
    } else if issues.iter().any(|issue| issue == "restore_drill_canary") {
        "restore_drill_canary".to_string()
    } else {
        "quality_report_not_passed".to_string()
    }
}

fn expected_evidence_missing(expected: &[String], selected: &[String]) -> Vec<String> {
    let selected = selected.iter().cloned().collect::<BTreeSet<_>>();
    expected
        .iter()
        .filter(|evidence_id| !selected.contains(*evidence_id))
        .cloned()
        .collect()
}

fn retrieval_failure_question_summary(report: &RetrievalQualityReport) -> String {
    if report.query_summary.redacted_terms.is_empty() {
        return format!("sha256:{}", &report.query_summary.question_sha256[..12]);
    }
    trim_text(&report.query_summary.redacted_terms.join(" "), 200)
}

fn retrieval_failure_redacted_excerpt(report: &RetrievalQualityReport) -> String {
    if report.query_summary.redacted_terms.is_empty() {
        return format!("sha256:{}", &report.query_summary.question_sha256[..12]);
    }
    trim_text(&report.query_summary.redacted_terms.join(" "), 120)
}

fn retrieval_failure_page_limit(requested: usize) -> usize {
    if requested == 0 {
        RETRIEVAL_FAILURE_DEFAULT_PAGE_SIZE
    } else {
        requested.min(RETRIEVAL_FAILURE_MAX_PAGE_SIZE)
    }
}

fn validate_human_review_status(status: &str) -> Result<()> {
    if matches!(status, "open" | "in_review" | "resolved" | "wontfix") {
        Ok(())
    } else {
        Err(anyhow!(
            "invalid retrieval failure human_review_status {status}"
        ))
    }
}

fn bounded_optional_text(value: &str, max_chars: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trim_text(trimmed, max_chars))
    }
}

fn latest_kb_version_id(conn: &Connection) -> Result<Option<String>> {
    if !sqlite_table_exists(conn, "kb_version")? {
        return Ok(None);
    }
    conn.query_row(
        "SELECT version_id FROM kb_version ORDER BY built_at DESC, version_id DESC LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(Into::into)
}

fn sqlite_table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            params![table],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

fn sqlite_table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    stmt.query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<HashSet<_>, _>>()
        .map_err(Into::into)
}

fn retrieval_failure_select_sql() -> &'static str {
    r#"
    SELECT
        failure_id, trace_id, package_id, question_sha256, question_char_count,
        question_summary, redacted_question_excerpt, kb_schema_version,
        kb_version_id, failure_type, redacted_query_terms_json, required_evidence_types_json,
        actual_evidence_types_json, expected_evidence_ids_json,
        selected_evidence_ids_json, missing_evidence_types_json,
        quality_issues_json, agent_diagnosis, proposed_fix, human_review_status,
        reviewer, review_note, created_at, updated_at, resolved_at
    FROM retrieval_failures
    "#
}

fn load_retrieval_failure(
    conn: &Connection,
    failure_id: &str,
) -> Result<Option<RetrievalFailureRecord>> {
    let sql = format!("{} WHERE failure_id = ?1", retrieval_failure_select_sql());
    let mut records = query_retrieval_failure_records(conn, &sql, &[&failure_id])?;
    Ok(records.pop())
}

fn load_retrieval_failure_by_dedupe(
    conn: &Connection,
    trace_id: &str,
    package_id: Option<&str>,
    failure_type: &str,
) -> Result<Option<RetrievalFailureRecord>> {
    let sql = format!(
        "{} WHERE trace_id = ?1 AND IFNULL(package_id, '') = ?2 AND failure_type = ?3 LIMIT 1",
        retrieval_failure_select_sql()
    );
    let package_id = package_id.unwrap_or("");
    let mut records =
        query_retrieval_failure_records(conn, &sql, &[&trace_id, &package_id, &failure_type])?;
    Ok(records.pop())
}

fn query_retrieval_failure_records(
    conn: &Connection,
    sql: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<RetrievalFailureRecord>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, retrieval_failure_sql_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(retrieval_failure_record_from_sql_row)
        .collect()
}

#[derive(Debug)]
struct RetrievalFailureSqlRow {
    failure_id: String,
    trace_id: String,
    package_id: Option<String>,
    question_sha256: String,
    question_char_count: i64,
    question_summary: String,
    redacted_question_excerpt: String,
    kb_schema_version: String,
    kb_version_id: Option<String>,
    failure_type: String,
    redacted_query_terms_json: String,
    required_evidence_types_json: String,
    actual_evidence_types_json: String,
    expected_evidence_ids_json: String,
    selected_evidence_ids_json: String,
    missing_evidence_types_json: String,
    quality_issues_json: String,
    agent_diagnosis: Option<String>,
    proposed_fix: Option<String>,
    human_review_status: String,
    reviewer: Option<String>,
    review_note: Option<String>,
    created_at: String,
    updated_at: String,
    resolved_at: Option<String>,
}

fn retrieval_failure_sql_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RetrievalFailureSqlRow> {
    Ok(RetrievalFailureSqlRow {
        failure_id: row.get(0)?,
        trace_id: row.get(1)?,
        package_id: row.get(2)?,
        question_sha256: row.get(3)?,
        question_char_count: row.get(4)?,
        question_summary: row.get(5)?,
        redacted_question_excerpt: row.get(6)?,
        kb_schema_version: row.get(7)?,
        kb_version_id: row.get(8)?,
        failure_type: row.get(9)?,
        redacted_query_terms_json: row.get(10)?,
        required_evidence_types_json: row.get(11)?,
        actual_evidence_types_json: row.get(12)?,
        expected_evidence_ids_json: row.get(13)?,
        selected_evidence_ids_json: row.get(14)?,
        missing_evidence_types_json: row.get(15)?,
        quality_issues_json: row.get(16)?,
        agent_diagnosis: row.get(17)?,
        proposed_fix: row.get(18)?,
        human_review_status: row.get(19)?,
        reviewer: row.get(20)?,
        review_note: row.get(21)?,
        created_at: row.get(22)?,
        updated_at: row.get(23)?,
        resolved_at: row.get(24)?,
    })
}

fn retrieval_failure_record_from_sql_row(
    row: RetrievalFailureSqlRow,
) -> Result<RetrievalFailureRecord> {
    Ok(RetrievalFailureRecord {
        failure_id: row.failure_id,
        trace_id: row.trace_id,
        package_id: row.package_id,
        question_sha256: row.question_sha256,
        question_char_count: usize::try_from(row.question_char_count)
            .context("retrieval failure question_char_count is negative")?,
        question_summary: row.question_summary,
        redacted_question_excerpt: row.redacted_question_excerpt,
        kb_schema_version: row.kb_schema_version,
        kb_version_id: row.kb_version_id,
        failure_type: row.failure_type,
        redacted_query_terms: serde_json::from_str(&row.redacted_query_terms_json)?,
        required_evidence_types: serde_json::from_str(&row.required_evidence_types_json)?,
        actual_evidence_types: serde_json::from_str(&row.actual_evidence_types_json)?,
        expected_evidence_ids: serde_json::from_str(&row.expected_evidence_ids_json)?,
        selected_evidence_ids: serde_json::from_str(&row.selected_evidence_ids_json)?,
        missing_evidence_types: serde_json::from_str(&row.missing_evidence_types_json)?,
        quality_issues: serde_json::from_str(&row.quality_issues_json)?,
        agent_diagnosis: row.agent_diagnosis,
        proposed_fix: row.proposed_fix,
        human_review_status: row.human_review_status,
        reviewer: row.reviewer,
        review_note: row.review_note,
        created_at: row.created_at,
        updated_at: row.updated_at,
        resolved_at: row.resolved_at,
    })
}

fn retrieval_failure_record_json(
    record: &RetrievalFailureRecord,
    view: RetrievalFailureView,
) -> Value {
    match view {
        RetrievalFailureView::AdminDetail => json!({
            "object": "tonglingyu.retrieval_failure",
            "view": "admin_detail",
            "schema_version": RETRIEVAL_FAILURE_SCHEMA_VERSION,
            "failure_id": &record.failure_id,
            "trace_id": &record.trace_id,
            "package_id": &record.package_id,
            "question_sha256": &record.question_sha256,
            "question_char_count": record.question_char_count,
            "question_summary": &record.question_summary,
            "redacted_question_excerpt": &record.redacted_question_excerpt,
            "kb_schema_version": &record.kb_schema_version,
            "kb_version_id": &record.kb_version_id,
            "failure_type": &record.failure_type,
            "redacted_query_terms": &record.redacted_query_terms,
            "required_evidence_types": &record.required_evidence_types,
            "actual_evidence_types": &record.actual_evidence_types,
            "expected_evidence_ids": &record.expected_evidence_ids,
            "selected_evidence_ids": &record.selected_evidence_ids,
            "missing_evidence_types": &record.missing_evidence_types,
            "quality_issues": &record.quality_issues,
            "agent_diagnosis": &record.agent_diagnosis,
            "proposed_fix": &record.proposed_fix,
            "human_review_status": &record.human_review_status,
            "reviewer": &record.reviewer,
            "review_note": &record.review_note,
            "created_at": &record.created_at,
            "updated_at": &record.updated_at,
            "resolved_at": &record.resolved_at,
        }),
        RetrievalFailureView::SafeSummary => json!({
            "object": "tonglingyu.retrieval_failure",
            "view": "safe_summary",
            "schema_version": RETRIEVAL_FAILURE_SCHEMA_VERSION,
            "failure_id": &record.failure_id,
            "question_sha256": &record.question_sha256,
            "question_char_count": record.question_char_count,
            "question_summary": &record.question_summary,
            "redacted_question_excerpt": &record.redacted_question_excerpt,
            "kb_schema_version": &record.kb_schema_version,
            "failure_type": &record.failure_type,
            "redacted_query_terms": &record.redacted_query_terms,
            "missing_evidence_types": &record.missing_evidence_types,
            "quality_issue_count": record.quality_issues.len(),
            "human_review_status": &record.human_review_status,
            "created_at": &record.created_at,
            "updated_at": &record.updated_at,
            "resolved_at": &record.resolved_at,
        }),
    }
}

fn query_retrieval_failures_for_clustering(
    conn: &Connection,
    human_review_status: Option<&str>,
    failure_type: Option<&str>,
    limit: usize,
) -> Result<Vec<RetrievalFailureRecord>> {
    let limit_i64 = retrieval_failure_cluster_limit(limit) as i64;
    match (human_review_status, failure_type) {
        (Some(status), Some(failure_type)) => {
            validate_human_review_status(status)?;
            query_retrieval_failure_records(
                conn,
                &format!(
                    "{} WHERE human_review_status = ?1 AND failure_type = ?2 ORDER BY updated_at DESC, failure_id DESC LIMIT ?3",
                    retrieval_failure_select_sql()
                ),
                &[&status, &failure_type, &limit_i64],
            )
        }
        (Some(status), None) => {
            validate_human_review_status(status)?;
            query_retrieval_failure_records(
                conn,
                &format!(
                    "{} WHERE human_review_status = ?1 ORDER BY updated_at DESC, failure_id DESC LIMIT ?2",
                    retrieval_failure_select_sql()
                ),
                &[&status, &limit_i64],
            )
        }
        (None, Some(failure_type)) => query_retrieval_failure_records(
            conn,
            &format!(
                "{} WHERE human_review_status IN ('open', 'in_review') AND failure_type = ?1 ORDER BY updated_at DESC, failure_id DESC LIMIT ?2",
                retrieval_failure_select_sql()
            ),
            &[&failure_type, &limit_i64],
        ),
        (None, None) => query_retrieval_failure_records(
            conn,
            &format!(
                "{} WHERE human_review_status IN ('open', 'in_review') ORDER BY updated_at DESC, failure_id DESC LIMIT ?1",
                retrieval_failure_select_sql()
            ),
            &[&limit_i64],
        ),
    }
}

fn retrieval_failure_cluster_key(failure: &RetrievalFailureRecord) -> String {
    let missing = normalized_list_digest(&failure.missing_evidence_types);
    let required = normalized_list_digest(&failure.required_evidence_types);
    let quality = normalized_list_digest(&retrieval_failure_quality_issue_families(failure));
    let kb = failure.kb_version_id.as_deref().unwrap_or("unknown-kb");
    format!(
        "rfc:{}:kb:{}:missing:{}:required:{}:issues:{}",
        failure.failure_type,
        &hash_text(kb)[..12],
        &missing[..12],
        &required[..12],
        &quality[..12]
    )
}

fn retrieval_failure_quality_issue_families(failure: &RetrievalFailureRecord) -> Vec<String> {
    let mut families = failure
        .quality_issues
        .iter()
        .map(|issue| {
            issue
                .split_once(':')
                .map(|(family, _)| family)
                .unwrap_or(issue)
                .trim()
                .to_string()
        })
        .filter(|issue| !issue.is_empty())
        .collect::<Vec<_>>();
    families.sort();
    families.dedup();
    families
}

fn normalized_list_digest(values: &[String]) -> String {
    let mut normalized = values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    hash_text(&normalized.join("|"))
}

fn retrieval_failure_cluster_limit(requested: usize) -> usize {
    if requested == 0 {
        RETRIEVAL_FAILURE_CLUSTER_DEFAULT_LIMIT
    } else {
        requested.min(RETRIEVAL_FAILURE_CLUSTER_MAX_LIMIT)
    }
}

fn retrieval_failure_cluster_proposed_fix(failures: &[RetrievalFailureRecord]) -> String {
    let representative = &failures[0];
    let missing = normalized_display_list(&representative.missing_evidence_types, 6);
    let required = normalized_display_list(&representative.required_evidence_types, 6);
    let issue_families =
        normalized_display_list(&retrieval_failure_quality_issue_families(representative), 6);
    let recommendation = match representative.failure_type.as_str() {
        "source_usage_metadata_incomplete" => {
            "complete source usage metadata and attribution before accepting affected answers"
        }
        "expected_evidence_missing" => {
            "review expected evidence coverage and adjust retrieval policy or source coverage"
        }
        "reviewer_evidence_insufficient" => {
            "route representative packages to expert review and revise evidence mapping"
        }
        _ => "review retrieval policy, exact-term handling, and source selection for this cluster",
    };
    format!(
        "agent_cluster_proposed_fix; no_direct_fact_mutation=true; failure_type={}; failure_count={}; missing_evidence_types={}; required_evidence_types={}; issue_families={}; recommended_action={}",
        representative.failure_type,
        failures.len(),
        missing,
        required,
        issue_families,
        recommendation
    )
}

fn normalized_display_list(values: &[String], limit: usize) -> String {
    let mut normalized = values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        "none".to_string()
    } else {
        normalized
            .into_iter()
            .take(limit)
            .collect::<Vec<_>>()
            .join("|")
    }
}

fn retrieval_failure_cluster_json(
    cluster_key: &str,
    failures: &[RetrievalFailureRecord],
    proposed_fix: &str,
    task: Option<&KnowledgeGovernanceTaskRecord>,
) -> Value {
    let representative = &failures[0];
    let failure_ids = failures
        .iter()
        .take(25)
        .map(|failure| failure.failure_id.clone())
        .collect::<Vec<_>>();
    let trace_ids = failures
        .iter()
        .take(25)
        .map(|failure| failure.trace_id.clone())
        .collect::<BTreeSet<_>>();
    let package_ids = failures
        .iter()
        .filter_map(|failure| failure.package_id.clone())
        .take(25)
        .collect::<BTreeSet<_>>();
    json!({
        "object": "tonglingyu.retrieval_failure_cluster",
        "schema_version": RETRIEVAL_FAILURE_CLUSTER_SCHEMA_VERSION,
        "cluster_key": cluster_key,
        "failure_type": &representative.failure_type,
        "failure_count": failures.len(),
        "failure_ids": failure_ids,
        "failure_ids_truncated": failures.len() > 25,
        "trace_ids": trace_ids.into_iter().collect::<Vec<_>>(),
        "package_ids": package_ids.into_iter().collect::<Vec<_>>(),
        "kb_version_id": &representative.kb_version_id,
        "missing_evidence_types": &representative.missing_evidence_types,
        "required_evidence_types": &representative.required_evidence_types,
        "quality_issue_families": retrieval_failure_quality_issue_families(representative),
        "proposed_fix": proposed_fix,
        "direct_fact_mutation": false,
        "task": task.map(governance_task_record_json),
    })
}

fn governance_task_required_for_failure(failure: &RetrievalFailureRecord) -> bool {
    matches!(failure.human_review_status.as_str(), "open" | "in_review")
}

fn default_governance_task_type(failure: &RetrievalFailureRecord) -> String {
    match failure.failure_type.as_str() {
        "source_usage_metadata_incomplete" => "source_metadata_fix",
        "expected_evidence_missing" => "expected_evidence_fix",
        "restore_drill_canary" => "expert_review",
        "reviewer_evidence_insufficient" => "expert_review",
        _ => "retrieval_policy_fix",
    }
    .to_string()
}

fn default_governance_task_priority(failure: &RetrievalFailureRecord) -> String {
    if failure.failure_type == "restore_drill_canary" {
        "p1"
    } else {
        "p0"
    }
    .to_string()
}

fn default_governance_cluster_key(failure: &RetrievalFailureRecord) -> String {
    let missing_digest = hash_text(&failure.missing_evidence_types.join("|"));
    format!(
        "rf:{}:q:{}:missing:{}",
        failure.failure_type,
        &failure.question_sha256[..16],
        &missing_digest[..12]
    )
}

fn governance_task_page_limit(requested: usize) -> usize {
    if requested == 0 {
        GOVERNANCE_TASK_DEFAULT_PAGE_SIZE
    } else {
        requested.min(GOVERNANCE_TASK_MAX_PAGE_SIZE)
    }
}

fn validate_governance_task_type(task_type: &str) -> Result<()> {
    if matches!(
        task_type,
        "source_metadata_fix"
            | "expected_evidence_fix"
            | "retrieval_policy_fix"
            | "alias_term_review"
            | "commentary_link_review"
            | "version_note_review"
            | "expert_review"
    ) {
        Ok(())
    } else {
        Err(anyhow!("invalid governance task_type {task_type}"))
    }
}

fn validate_governance_task_status(status: &str) -> Result<()> {
    if matches!(
        status,
        "open" | "in_review" | "accepted" | "rejected" | "closed"
    ) {
        Ok(())
    } else {
        Err(anyhow!("invalid governance task status {status}"))
    }
}

fn validate_governance_task_priority(priority: &str) -> Result<()> {
    if matches!(priority, "p0" | "p1" | "p2") {
        Ok(())
    } else {
        Err(anyhow!("invalid governance task priority {priority}"))
    }
}

fn validate_governance_source_entity_type(source_entity_type: &str) -> Result<()> {
    if matches!(
        source_entity_type,
        "retrieval_failure"
            | "retrieval_failure_cluster"
            | "trace"
            | "package"
            | "knowledge_item"
            | "eval_miss"
            | "user_feedback"
            | "knowledge_patch_proposal"
    ) {
        Ok(())
    } else {
        Err(anyhow!(
            "invalid governance task source_entity_type {source_entity_type}"
        ))
    }
}

fn governance_task_select_sql() -> &'static str {
    r#"
    SELECT
        task_id, source_failure_id, source_entity_type, source_entity_id,
        trace_id, package_id, task_type, status, priority, agent_cluster_key,
        proposed_fix, reviewer, review_note, evidence_ref, created_at,
        updated_at, accepted_at, closed_at
    FROM knowledge_governance_tasks
    "#
}

fn load_governance_task(
    conn: &Connection,
    task_id: &str,
) -> Result<Option<KnowledgeGovernanceTaskRecord>> {
    let sql = format!("{} WHERE task_id = ?1", governance_task_select_sql());
    let mut records = query_governance_task_records(conn, &sql, &[&task_id])?;
    Ok(records.pop())
}

fn load_governance_task_by_entity_type(
    conn: &Connection,
    source_entity_type: &str,
    source_entity_id: &str,
    task_type: &str,
) -> Result<Option<KnowledgeGovernanceTaskRecord>> {
    let sql = format!(
        "{} WHERE source_entity_type = ?1 AND source_entity_id = ?2 AND task_type = ?3 LIMIT 1",
        governance_task_select_sql()
    );
    let mut records = query_governance_task_records(
        conn,
        &sql,
        &[&source_entity_type, &source_entity_id, &task_type],
    )?;
    Ok(records.pop())
}

fn query_governance_task_records(
    conn: &Connection,
    sql: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<KnowledgeGovernanceTaskRecord>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, governance_task_sql_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(governance_task_record_from_sql_row)
        .collect()
}

fn query_governance_task_records_dynamic(
    conn: &Connection,
    sql: &str,
    params: Vec<rusqlite::types::Value>,
) -> Result<Vec<KnowledgeGovernanceTaskRecord>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter()),
        governance_task_sql_row,
    )?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(governance_task_record_from_sql_row)
        .collect()
}

#[derive(Debug)]
struct GovernanceTaskSqlRow {
    task_id: String,
    source_failure_id: Option<String>,
    source_entity_type: String,
    source_entity_id: String,
    trace_id: String,
    package_id: Option<String>,
    task_type: String,
    status: String,
    priority: String,
    agent_cluster_key: String,
    proposed_fix: String,
    reviewer: Option<String>,
    review_note: Option<String>,
    evidence_ref: Option<String>,
    created_at: String,
    updated_at: String,
    accepted_at: Option<String>,
    closed_at: Option<String>,
}

fn governance_task_sql_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GovernanceTaskSqlRow> {
    Ok(GovernanceTaskSqlRow {
        task_id: row.get(0)?,
        source_failure_id: row.get(1)?,
        source_entity_type: row.get(2)?,
        source_entity_id: row.get(3)?,
        trace_id: row.get(4)?,
        package_id: row.get(5)?,
        task_type: row.get(6)?,
        status: row.get(7)?,
        priority: row.get(8)?,
        agent_cluster_key: row.get(9)?,
        proposed_fix: row.get(10)?,
        reviewer: row.get(11)?,
        review_note: row.get(12)?,
        evidence_ref: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
        accepted_at: row.get(16)?,
        closed_at: row.get(17)?,
    })
}

fn governance_task_record_from_sql_row(
    row: GovernanceTaskSqlRow,
) -> Result<KnowledgeGovernanceTaskRecord> {
    Ok(KnowledgeGovernanceTaskRecord {
        task_id: row.task_id,
        source_failure_id: row.source_failure_id,
        source_entity_type: row.source_entity_type,
        source_entity_id: row.source_entity_id,
        trace_id: row.trace_id,
        package_id: row.package_id,
        task_type: row.task_type,
        status: row.status,
        priority: row.priority,
        agent_cluster_key: row.agent_cluster_key,
        proposed_fix: row.proposed_fix,
        reviewer: row.reviewer,
        review_note: row.review_note,
        evidence_ref: row.evidence_ref,
        created_at: row.created_at,
        updated_at: row.updated_at,
        accepted_at: row.accepted_at,
        closed_at: row.closed_at,
    })
}

fn governance_task_record_json(record: &KnowledgeGovernanceTaskRecord) -> Value {
    json!({
        "object": "tonglingyu.knowledge_governance_task",
        "schema_version": KNOWLEDGE_GOVERNANCE_TASK_SCHEMA_VERSION,
        "task_id": &record.task_id,
        "source_failure_id": &record.source_failure_id,
        "source_entity_type": &record.source_entity_type,
        "source_entity_id": &record.source_entity_id,
        "trace_id": &record.trace_id,
        "package_id": &record.package_id,
        "task_type": &record.task_type,
        "status": &record.status,
        "priority": &record.priority,
        "agent_cluster_key": &record.agent_cluster_key,
        "proposed_fix": &record.proposed_fix,
        "reviewer": &record.reviewer,
        "review_note": &record.review_note,
        "evidence_ref": &record.evidence_ref,
        "created_at": &record.created_at,
        "updated_at": &record.updated_at,
        "accepted_at": &record.accepted_at,
        "closed_at": &record.closed_at,
    })
}

fn normalize_knowledge_patch_proposal_type(proposal_type: &str) -> Result<String> {
    let normalized = proposal_type.trim().to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "alias" | "term" | "commentary_link" | "version_note"
    ) {
        Ok(normalized)
    } else {
        Err(anyhow!(
            "invalid knowledge patch proposal_type {proposal_type}"
        ))
    }
}

fn knowledge_patch_proposal_task_type(proposal_type: &str) -> &'static str {
    match proposal_type {
        "alias" | "term" => "alias_term_review",
        "commentary_link" => "commentary_link_review",
        "version_note" => "version_note_review",
        _ => "expert_review",
    }
}

fn validate_knowledge_patch_proposal_payload(proposal_type: &str, payload: &Value) -> Result<()> {
    if !payload.is_object() {
        return Err(anyhow!(
            "knowledge patch proposal payload must be a JSON object"
        ));
    }
    match proposal_type {
        "alias" => {
            required_payload_string(payload, "alias")?;
            required_payload_string(payload, "target_ref")?;
        }
        "term" => {
            required_payload_string(payload, "term")?;
            required_payload_string(payload, "usage_boundary")?;
        }
        "commentary_link" => {
            required_payload_string(payload, "commentary_ref")?;
            required_payload_string(payload, "block_id")?;
        }
        "version_note" => {
            required_payload_string(payload, "source_id")?;
            required_payload_string(payload, "note")?;
        }
        _ => {
            return Err(anyhow!(
                "invalid knowledge patch proposal_type {proposal_type}"
            ));
        }
    }
    Ok(())
}

fn required_payload_string<'a>(payload: &'a Value, field: &str) -> Result<&'a str> {
    let value = payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("knowledge patch proposal payload requires {field}"))?;
    if value.chars().count() > 1_000 {
        return Err(anyhow!(
            "knowledge patch proposal payload field {field} exceeds 1000 chars"
        ));
    }
    Ok(value)
}

fn canonical_json_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonical_json_value).collect()),
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            for key in map.keys().collect::<BTreeSet<_>>() {
                if let Some(value) = map.get(key) {
                    sorted.insert(key.clone(), canonical_json_value(value));
                }
            }
            Value::Object(sorted)
        }
        other => other.clone(),
    }
}

fn knowledge_item_page_limit(requested: usize) -> usize {
    if requested == 0 {
        KNOWLEDGE_ITEM_DEFAULT_PAGE_SIZE
    } else {
        requested.min(KNOWLEDGE_ITEM_MAX_PAGE_SIZE)
    }
}

fn normalize_knowledge_refs(name: &str, refs: Vec<String>) -> Result<Vec<String>> {
    let normalized = refs
        .into_iter()
        .filter_map(|value| bounded_optional_text(&value, 240))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return Err(anyhow!("knowledge item {name} must not be empty"));
    }
    if normalized.len() > 64 {
        return Err(anyhow!("knowledge item {name} exceeds 64 entries"));
    }
    Ok(normalized)
}

fn knowledge_calibration_env_vars() -> Vec<&'static str> {
    vec![
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROFILE",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROFILE_CONTRACT_VERSION",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_MODEL",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_UPSTREAM_ID",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_PROMPT_DIGEST",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_TOOL_POLICY_DIGEST",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_DECODING",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_TIMEOUT_SECS",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_RETRY_LIMIT",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_MODEL_CAPABILITY",
        "TONGLINGYU_KNOWLEDGE_CALIBRATION_REASONING_EFFORT",
    ]
}

fn required_calibration_env(vars: &BTreeMap<String, String>, name: &str) -> Result<String> {
    vars.get(name)
        .and_then(|value| bounded_optional_text(value, 512))
        .ok_or_else(|| anyhow!("missing required knowledge calibration config {name}"))
}

fn parse_positive_u64_env(value: &str, name: &str) -> Result<u64> {
    let parsed = value
        .parse::<u64>()
        .with_context(|| format!("{name} must be a positive integer"))?;
    if parsed == 0 {
        return Err(anyhow!("{name} must be positive"));
    }
    Ok(parsed)
}

fn parse_bounded_u32_env(value: &str, name: &str, min: u32, max: u32) -> Result<u32> {
    let parsed = value
        .parse::<u32>()
        .with_context(|| format!("{name} must be an integer"))?;
    if parsed < min || parsed > max {
        return Err(anyhow!("{name} must be between {min} and {max}"));
    }
    Ok(parsed)
}

fn validate_calibration_digest(value: &str) -> Result<String> {
    let digest = value.trim();
    let digest = digest.strip_prefix("sha256:").unwrap_or(digest);
    if digest.len() != 64 || !digest.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "knowledge calibration digest must be a sha256 hex digest"
        ));
    }
    Ok(digest.to_ascii_lowercase())
}

fn validate_calibration_model_capability(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "complex" | "advanced" | "frontier" | "high_reasoning"
    ) {
        Ok(normalized)
    } else {
        Err(anyhow!(
            "knowledge calibration requires complex model capability"
        ))
    }
}

fn validate_calibration_reasoning_effort(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "high" | "xhigh") {
        Ok(normalized)
    } else {
        Err(anyhow!(
            "knowledge calibration requires high or xhigh reasoning effort"
        ))
    }
}

fn validate_knowledge_item_actor(actor: &str) -> Result<String> {
    bounded_optional_text(actor, 80).ok_or_else(|| anyhow!("knowledge item actor is required"))
}

fn validate_knowledge_item_reason(reason: &str) -> Result<String> {
    bounded_optional_text(reason, 480).ok_or_else(|| anyhow!("knowledge item reason is required"))
}

fn validate_knowledge_item_trace_id(trace_id: &str) -> Result<String> {
    bounded_optional_text(trace_id, 160)
        .ok_or_else(|| anyhow!("knowledge item trace_id is required"))
}

fn validate_knowledge_review_reviewer(reviewer: &str) -> Result<String> {
    bounded_optional_text(reviewer, 80)
        .ok_or_else(|| anyhow!("knowledge item human review reviewer is required"))
}

fn validate_knowledge_review_evidence_ref(evidence_ref: &str) -> Result<String> {
    bounded_optional_text(evidence_ref, 240)
        .ok_or_else(|| anyhow!("knowledge item human review evidence_ref is required"))
}

fn stable_knowledge_item_id(
    kind: KnowledgeItemKind,
    source_refs_json: &str,
    payload_sha256: &str,
) -> String {
    let identity = format!(
        "kind={};source_refs={};payload_sha256={}",
        kind.as_str(),
        source_refs_json,
        payload_sha256
    );
    let digest = hash_text(&identity);
    format!("ki-{}-{}", kind.as_str().replace('_', "-"), &digest[..24])
}

struct KnowledgeItemStateHistoryInsert<'a> {
    item_id: &'a str,
    previous_state: Option<KnowledgeState>,
    new_state: KnowledgeState,
    actor: &'a str,
    reason: &'a str,
    evidence_refs: &'a [String],
    state_version: i64,
    created_at: &'a str,
}

fn insert_knowledge_item_state_history(
    conn: &Connection,
    input: KnowledgeItemStateHistoryInsert<'_>,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO knowledge_item_state_history (
            history_id, item_id, previous_state, new_state, actor, reason_sha256,
            evidence_refs_json, state_version, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            format!("kish-{}", uuid::Uuid::now_v7().simple()),
            input.item_id,
            input.previous_state.map(KnowledgeState::as_str),
            input.new_state.as_str(),
            input.actor,
            hash_text(input.reason),
            serde_json::to_string(input.evidence_refs)?,
            input.state_version,
            input.created_at,
        ],
    )?;
    Ok(())
}

fn knowledge_item_select_sql() -> &'static str {
    r#"
    SELECT
        item_id, kind, state, source_refs_json, evidence_refs_json, payload_json,
        payload_sha256, schema_version, source_boundary_json, calibration_report_ref,
        confidence, created_at, updated_at, state_version
    FROM knowledge_items
    "#
}

fn load_knowledge_item(conn: &Connection, item_id: &str) -> Result<Option<KnowledgeItemRecord>> {
    let sql = format!("{} WHERE item_id = ?1", knowledge_item_select_sql());
    conn.query_row(&sql, params![item_id], knowledge_item_sql_row)
        .optional()?
        .map(knowledge_item_record_from_sql_row)
        .transpose()
}

fn query_knowledge_item_records(
    conn: &Connection,
    sql: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<KnowledgeItemRecord>> {
    let mut stmt = conn.prepare(sql)?;
    stmt.query_map(params, knowledge_item_sql_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(knowledge_item_record_from_sql_row)
        .collect()
}

struct KnowledgeItemSqlRow {
    item_id: String,
    kind: String,
    state: String,
    source_refs_json: String,
    evidence_refs_json: String,
    payload_json: String,
    payload_sha256: String,
    schema_version: String,
    source_boundary_json: Option<String>,
    calibration_report_ref: Option<String>,
    confidence: Option<f64>,
    created_at: String,
    updated_at: String,
    state_version: i64,
}

fn knowledge_item_sql_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeItemSqlRow> {
    Ok(KnowledgeItemSqlRow {
        item_id: row.get(0)?,
        kind: row.get(1)?,
        state: row.get(2)?,
        source_refs_json: row.get(3)?,
        evidence_refs_json: row.get(4)?,
        payload_json: row.get(5)?,
        payload_sha256: row.get(6)?,
        schema_version: row.get(7)?,
        source_boundary_json: row.get(8)?,
        calibration_report_ref: row.get(9)?,
        confidence: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        state_version: row.get(13)?,
    })
}

fn knowledge_item_record_from_sql_row(row: KnowledgeItemSqlRow) -> Result<KnowledgeItemRecord> {
    let source_boundary = row
        .source_boundary_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    Ok(KnowledgeItemRecord {
        item_id: row.item_id,
        kind: KnowledgeItemKind::parse(&row.kind)?,
        state: KnowledgeState::parse(&row.state)?,
        source_refs: serde_json::from_str(&row.source_refs_json)?,
        evidence_refs: serde_json::from_str(&row.evidence_refs_json)?,
        payload: serde_json::from_str(&row.payload_json)?,
        payload_sha256: row.payload_sha256,
        schema_version: row.schema_version,
        source_boundary,
        calibration_report_ref: row.calibration_report_ref,
        confidence: row.confidence,
        created_at: row.created_at,
        updated_at: row.updated_at,
        state_version: row.state_version,
    })
}

fn knowledge_patch_proposal_select_sql() -> &'static str {
    r#"
    SELECT
        proposal_id, proposal_type, trace_id, package_id, source_ref,
        payload_json, payload_sha256, task_id, created_by, created_at,
        updated_at
    FROM knowledge_patch_proposals
    "#
}

fn load_knowledge_patch_proposal(
    conn: &Connection,
    proposal_id: &str,
) -> Result<Option<KnowledgePatchProposalRecord>> {
    let sql = format!(
        "{} WHERE proposal_id = ?1",
        knowledge_patch_proposal_select_sql()
    );
    let mut records = query_knowledge_patch_proposal_records(conn, &sql, &[&proposal_id])?;
    Ok(records.pop())
}

fn load_knowledge_patch_proposal_by_identity(
    conn: &Connection,
    proposal_type: &str,
    source_ref: &str,
    payload_sha256: &str,
) -> Result<Option<KnowledgePatchProposalRecord>> {
    let sql = format!(
        "{} WHERE proposal_type = ?1 AND source_ref = ?2 AND payload_sha256 = ?3 LIMIT 1",
        knowledge_patch_proposal_select_sql()
    );
    let mut records = query_knowledge_patch_proposal_records(
        conn,
        &sql,
        &[&proposal_type, &source_ref, &payload_sha256],
    )?;
    Ok(records.pop())
}

fn query_knowledge_patch_proposal_records(
    conn: &Connection,
    sql: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<KnowledgePatchProposalRecord>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
        ))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(
            |(
                proposal_id,
                proposal_type,
                trace_id,
                package_id,
                source_ref,
                payload_json,
                payload_sha256,
                task_id,
                created_by,
                created_at,
                updated_at,
            )| {
                let payload = serde_json::from_str(&payload_json)
                    .with_context(|| format!("parse payload_json for {proposal_id}"))?;
                Ok(KnowledgePatchProposalRecord {
                    proposal_id,
                    proposal_type,
                    trace_id,
                    package_id,
                    source_ref,
                    payload,
                    payload_sha256,
                    task_id,
                    created_by,
                    created_at,
                    updated_at,
                })
            },
        )
        .collect()
}

fn knowledge_patch_proposal_record_json(record: &KnowledgePatchProposalRecord) -> Value {
    json!({
        "object": "tonglingyu.knowledge_patch_proposal",
        "schema_version": KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION,
        "proposal_id": &record.proposal_id,
        "proposal_type": &record.proposal_type,
        "trace_id": &record.trace_id,
        "package_id": &record.package_id,
        "source_ref": &record.source_ref,
        "payload": &record.payload,
        "payload_sha256": &record.payload_sha256,
        "task_id": &record.task_id,
        "created_by": &record.created_by,
        "created_at": &record.created_at,
        "updated_at": &record.updated_at,
        "direct_fact_mutation": false,
    })
}

fn knowledge_patch_proposal_create_json(
    proposal: &KnowledgePatchProposalRecord,
    task: &KnowledgeGovernanceTaskRecord,
) -> Value {
    json!({
        "object": "tonglingyu.knowledge_patch_proposal_create",
        "schema_version": KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION,
        "proposal": knowledge_patch_proposal_record_json(proposal),
        "task": governance_task_record_json(task),
        "direct_fact_mutation": false,
    })
}

pub fn prune_runtime_data(conn: &Connection, retention_days: u32, dry_run: bool) -> Result<Value> {
    if retention_days == 0 {
        return Ok(json!({
            "object": "tonglingyu.runtime_prune_report",
            "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
            "status": "disabled",
            "retention_days": retention_days,
            "dry_run": dry_run,
            "secret_values_printed": false,
        }));
    }
    let cutoff = (OffsetDateTime::now_utc() - time::Duration::days(retention_days as i64))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
    if dry_run {
        let plan = build_rqa_prune_plan(conn, &cutoff)?;
        return Ok(json!({
            "object": "tonglingyu.runtime_prune_report",
            "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
            "status": "dry_run",
            "retention_days": retention_days,
            "cutoff": cutoff,
            "counts": rqa_prune_counts(&plan, 0),
            "protected_refs": {
                "active_trace_refs": plan.active_refs.trace_ids.len(),
                "active_package_refs": plan.active_refs.package_ids.len(),
            },
            "secret_values_printed": false,
        }));
    }
    run_immediate_transaction(conn, |tx| {
        let plan = build_rqa_prune_plan(tx, &cutoff)?;
        let mut tombstones = 0_usize;
        for package in &plan.prunable_packages {
            append_rqa_lifecycle_tombstone(
                tx,
                "evidence_package",
                &package.package_id,
                "retention_prune",
                "retention_expired",
                &json!({
                    "object_type": "evidence_package",
                    "object_id_sha256": hash_text(&package.package_id),
                    "trace_id_sha256": hash_text(&package.trace_id),
                    "retention_days": retention_days,
                    "cutoff": cutoff,
                    "deleted_child_tables": [
                        "evidence_claim_links",
                        "review_records",
                        "evidence_cards",
                        "evidence_packages",
                    ],
                    "raw_question_included": false,
                    "secret_values_printed": false,
                }),
            )?;
            tombstones += 1;
            tx.execute(
                "DELETE FROM evidence_claim_links WHERE package_id = ?1",
                params![&package.package_id],
            )?;
            tx.execute(
                "DELETE FROM review_records WHERE package_id = ?1",
                params![&package.package_id],
            )?;
            tx.execute(
                "DELETE FROM evidence_cards WHERE package_id = ?1",
                params![&package.package_id],
            )?;
            tx.execute(
                "DELETE FROM evidence_packages WHERE package_id = ?1",
                params![&package.package_id],
            )?;
        }
        if !plan.prunable_audit_events.is_empty() {
            append_rqa_lifecycle_tombstone(
                tx,
                "audit_event_batch",
                &format!(
                    "audit_events:{}:{}",
                    cutoff,
                    plan.prunable_audit_events.len()
                ),
                "retention_prune",
                "retention_expired",
                &json!({
                    "object_type": "audit_event_batch",
                    "event_count": plan.prunable_audit_events.len(),
                    "protected_event_count": plan.protected_audit_events.len(),
                    "retention_days": retention_days,
                    "cutoff": cutoff,
                    "raw_payload_included": false,
                    "secret_values_printed": false,
                }),
            )?;
            tombstones += 1;
            for event in &plan.prunable_audit_events {
                tx.execute(
                    "DELETE FROM audit_events WHERE event_id = ?1",
                    params![&event.event_id],
                )?;
            }
        }
        append_runtime_audit_event(
            tx,
            "retention-prune",
            "rqa_retention_pruned",
            &json!({
                "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
                "retention_days": retention_days,
                "cutoff": cutoff,
                "counts": {
                    "packages": plan.prunable_packages.len(),
                    "protected_packages": plan.protected_packages.len(),
                    "audit_events": plan.prunable_audit_events.len(),
                    "protected_audit_events": plan.protected_audit_events.len(),
                    "tombstones": tombstones,
                },
                "secret_values_printed": false,
            }),
        )?;
        Ok(json!({
            "object": "tonglingyu.runtime_prune_report",
            "lifecycle_policy_version": RQA_LIFECYCLE_POLICY_VERSION,
            "status": "pruned",
            "retention_days": retention_days,
            "cutoff": cutoff,
            "counts": rqa_prune_counts(&plan, tombstones),
            "protected_refs": {
                "active_trace_refs": plan.active_refs.trace_ids.len(),
                "active_package_refs": plan.active_refs.package_ids.len(),
            },
            "secret_values_printed": false,
        }))
    })
}

#[derive(Debug)]
struct RqaPrunePlan {
    active_refs: RqaRetentionRefs,
    prunable_packages: Vec<EvidencePackageRetentionRef>,
    protected_packages: Vec<EvidencePackageRetentionRef>,
    prunable_audit_events: Vec<AuditEventRetentionRef>,
    protected_audit_events: Vec<AuditEventRetentionRef>,
}

fn build_rqa_prune_plan(conn: &Connection, cutoff: &str) -> Result<RqaPrunePlan> {
    let active_refs = active_rqa_retention_refs(conn)?;
    let old_packages = old_evidence_package_refs(conn, cutoff)?;
    let mut prunable_packages = Vec::new();
    let mut protected_packages = Vec::new();
    for package in old_packages {
        if active_refs.package_ids.contains(&package.package_id)
            || active_refs.trace_ids.contains(&package.trace_id)
        {
            protected_packages.push(package);
        } else {
            prunable_packages.push(package);
        }
    }
    let old_audit_events = old_audit_event_refs(conn, cutoff)?;
    let mut prunable_audit_events = Vec::new();
    let mut protected_audit_events = Vec::new();
    for event in old_audit_events {
        if active_refs.trace_ids.contains(&event.trace_id) {
            protected_audit_events.push(event);
        } else {
            prunable_audit_events.push(event);
        }
    }
    Ok(RqaPrunePlan {
        active_refs,
        prunable_packages,
        protected_packages,
        prunable_audit_events,
        protected_audit_events,
    })
}

fn rqa_prune_counts(plan: &RqaPrunePlan, tombstones: usize) -> Value {
    json!({
        "package_candidates": plan.prunable_packages.len() + plan.protected_packages.len(),
        "packages": plan.prunable_packages.len(),
        "protected_packages": plan.protected_packages.len(),
        "audit_event_candidates": plan.prunable_audit_events.len() + plan.protected_audit_events.len(),
        "audit_events": plan.prunable_audit_events.len(),
        "protected_audit_events": plan.protected_audit_events.len(),
        "tombstone_candidates": plan.prunable_packages.len()
            + usize::from(!plan.prunable_audit_events.is_empty()),
        "tombstones": tombstones,
    })
}

fn run_immediate_transaction<T>(
    conn: &Connection,
    work: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match work(conn) {
        Ok(value) => {
            if let Err(error) = conn.execute_batch("COMMIT") {
                let _ = conn.execute_batch("ROLLBACK");
                Err(error.into())
            } else {
                Ok(value)
            }
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

#[derive(Debug)]
struct RqaRetentionRefs {
    trace_ids: BTreeSet<String>,
    package_ids: BTreeSet<String>,
}

#[derive(Debug)]
struct EvidencePackageRetentionRef {
    package_id: String,
    trace_id: String,
}

#[derive(Debug)]
struct AuditEventRetentionRef {
    event_id: String,
    trace_id: String,
}

fn active_rqa_retention_refs(conn: &Connection) -> Result<RqaRetentionRefs> {
    let mut refs = RqaRetentionRefs {
        trace_ids: BTreeSet::new(),
        package_ids: BTreeSet::new(),
    };
    collect_rqa_trace_package_refs(
        conn,
        r#"
        SELECT trace_id, package_id
        FROM retrieval_failures
        WHERE human_review_status IN ('open', 'in_review')
        "#,
        &mut refs,
    )?;
    collect_rqa_trace_package_refs(
        conn,
        r#"
        SELECT trace_id, package_id
        FROM knowledge_governance_tasks
        WHERE status IN ('open', 'in_review', 'accepted')
        "#,
        &mut refs,
    )?;
    Ok(refs)
}

fn collect_rqa_trace_package_refs(
    conn: &Connection,
    sql: &str,
    refs: &mut RqaRetentionRefs,
) -> Result<()> {
    let mut stmt = conn.prepare(sql)?;
    for row in stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })? {
        let (trace_id, package_id) = row?;
        refs.trace_ids.insert(trace_id);
        if let Some(package_id) = package_id.filter(|value| !value.is_empty()) {
            refs.package_ids.insert(package_id);
        }
    }
    Ok(())
}

fn old_evidence_package_refs(
    conn: &Connection,
    cutoff: &str,
) -> Result<Vec<EvidencePackageRetentionRef>> {
    let mut stmt =
        conn.prepare("SELECT package_id, trace_id FROM evidence_packages WHERE created_at < ?1")?;
    stmt.query_map(params![cutoff], |row| {
        Ok(EvidencePackageRetentionRef {
            package_id: row.get(0)?,
            trace_id: row.get(1)?,
        })
    })?
    .collect::<std::result::Result<Vec<_>, _>>()
    .map_err(Into::into)
}

fn old_audit_event_refs(conn: &Connection, cutoff: &str) -> Result<Vec<AuditEventRetentionRef>> {
    let mut stmt =
        conn.prepare("SELECT event_id, trace_id FROM audit_events WHERE created_at < ?1")?;
    stmt.query_map(params![cutoff], |row| {
        Ok(AuditEventRetentionRef {
            event_id: row.get(0)?,
            trace_id: row.get(1)?,
        })
    })?
    .collect::<std::result::Result<Vec<_>, _>>()
    .map_err(Into::into)
}

pub fn append_rqa_lifecycle_tombstone(
    conn: &Connection,
    object_type: &str,
    object_id: &str,
    action: &str,
    reason: &str,
    payload: &Value,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO rqa_lifecycle_tombstones (
            tombstone_id, object_type, object_id_sha256, action, reason,
            policy_version, payload_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            format!("rqa-tombstone-{}", uuid::Uuid::now_v7().simple()),
            object_type,
            hash_text(object_id),
            action,
            reason,
            RQA_LIFECYCLE_POLICY_VERSION,
            serde_json::to_string(payload)?,
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn init_knowledge_base_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sources (
            source_id TEXT PRIMARY KEY,
            source_category TEXT NOT NULL,
            format TEXT,
            title TEXT,
            work TEXT,
            edition TEXT,
            language TEXT,
            source_url TEXT,
            api_url TEXT,
            fetched_at TEXT,
            license TEXT,
            license_url TEXT,
            license_source_url TEXT,
            attribution TEXT,
            usage_boundary TEXT,
            notes TEXT,
            snapshot_contract_json TEXT NOT NULL,
            source_hash TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS source_documents (
            section_id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL REFERENCES sources(source_id),
            section_index INTEGER,
            title TEXT,
            display_title TEXT,
            fullurl TEXT,
            pageid INTEGER,
            revision_id INTEGER,
            revision_timestamp TEXT,
            wikitext_sha256 TEXT
        );

        CREATE TABLE IF NOT EXISTS editions (
            edition_id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL REFERENCES sources(source_id),
            edition_label TEXT NOT NULL,
            version_system TEXT NOT NULL,
            usage_limit TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chapters (
            chapter_id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL REFERENCES sources(source_id),
            chapter_no INTEGER,
            title TEXT NOT NULL,
            version_range TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS blocks (
            block_id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL REFERENCES sources(source_id),
            section_id TEXT NOT NULL,
            source_title TEXT NOT NULL,
            normalized_source_title TEXT NOT NULL DEFAULT '',
            source_url TEXT NOT NULL,
            revision_id INTEGER,
            block_index INTEGER NOT NULL,
            kind TEXT NOT NULL,
            tag TEXT,
            text TEXT NOT NULL,
            normalized_text TEXT NOT NULL,
            evidence_type TEXT NOT NULL,
            chapter_no INTEGER
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS blocks_fts USING fts5(
            block_id UNINDEXED,
            source_id UNINDEXED,
            source_title,
            text,
            normalized_text,
            tokenize = 'unicode61'
        );

        CREATE TABLE IF NOT EXISTS rare_char_annotations (
            annotation_id TEXT PRIMARY KEY,
            block_id TEXT NOT NULL REFERENCES blocks(block_id),
            source_id TEXT NOT NULL REFERENCES sources(source_id),
            character TEXT NOT NULL,
            reading TEXT,
            note TEXT,
            provenance TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS commentaries (
            commentary_id TEXT PRIMARY KEY,
            block_id TEXT NOT NULL REFERENCES blocks(block_id),
            source_id TEXT NOT NULL REFERENCES sources(source_id),
            commentary_text TEXT NOT NULL,
            commentary_type TEXT NOT NULL,
            version_label TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS version_notes (
            version_note_id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL REFERENCES sources(source_id),
            note TEXT NOT NULL,
            source_status TEXT NOT NULL,
            usage_limit TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS version_differences (
            difference_id TEXT PRIMARY KEY,
            left_block_id TEXT,
            right_block_id TEXT,
            scope TEXT NOT NULL,
            evidence_level TEXT NOT NULL,
            note TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS terms (
            term_id TEXT PRIMARY KEY,
            term TEXT NOT NULL,
            category TEXT,
            usage_boundary TEXT NOT NULL,
            note TEXT,
            source_ref TEXT NOT NULL,
            accepted_task_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(term, usage_boundary)
        );

        CREATE TABLE IF NOT EXISTS commentary_links (
            link_id TEXT PRIMARY KEY,
            commentary_ref TEXT NOT NULL,
            block_id TEXT NOT NULL REFERENCES blocks(block_id),
            source_ref TEXT NOT NULL,
            usage_boundary TEXT NOT NULL,
            accepted_task_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(commentary_ref, block_id)
        );

        CREATE TABLE IF NOT EXISTS people (
            person_id TEXT PRIMARY KEY,
            canonical_name TEXT NOT NULL,
            description TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS aliases (
            alias TEXT PRIMARY KEY,
            normalized_alias TEXT NOT NULL DEFAULT '',
            person_id TEXT NOT NULL REFERENCES people(person_id),
            scope TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS relationships (
            relationship_id TEXT PRIMARY KEY,
            subject_person_id TEXT NOT NULL,
            object_person_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            evidence_block_id TEXT,
            evidence_level TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS events (
            event_id TEXT PRIMARY KEY,
            event_name TEXT NOT NULL,
            chapter_no INTEGER,
            evidence_block_id TEXT,
            theme_tags TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS poems (
            poem_id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            source_block_id TEXT NOT NULL,
            topic TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS kb_version (
            version_id TEXT PRIMARY KEY,
            source_root TEXT NOT NULL,
            source_count INTEGER NOT NULL,
            block_count INTEGER NOT NULL,
            schema_version TEXT NOT NULL,
            built_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS kb_version_diff_reports (
            report_id TEXT PRIMARY KEY,
            schema_version TEXT NOT NULL,
            before_version_id TEXT,
            after_version_id TEXT NOT NULL,
            source_root TEXT NOT NULL,
            before_summary_json TEXT,
            after_summary_json TEXT NOT NULL,
            diff_json TEXT NOT NULL,
            eval_before_summary_json TEXT,
            eval_after_summary_json TEXT,
            eval_diff_json TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS knowledge_patch_applications (
            application_id TEXT PRIMARY KEY,
            proposal_id TEXT NOT NULL,
            task_id TEXT NOT NULL,
            proposal_type TEXT NOT NULL,
            target_table TEXT NOT NULL,
            target_key TEXT NOT NULL,
            payload_sha256 TEXT NOT NULL,
            source_ref TEXT NOT NULL,
            evidence_ref TEXT NOT NULL,
            applied_at TEXT NOT NULL,
            UNIQUE(proposal_id, target_table, target_key)
        );

        CREATE INDEX IF NOT EXISTS idx_blocks_source ON blocks(source_id);
        CREATE INDEX IF NOT EXISTS idx_blocks_chapter ON blocks(chapter_no);
        CREATE INDEX IF NOT EXISTS idx_blocks_type ON blocks(evidence_type);
        CREATE INDEX IF NOT EXISTS idx_commentaries_source ON commentaries(source_id);
        CREATE INDEX IF NOT EXISTS idx_terms_term ON terms(term);
        CREATE INDEX IF NOT EXISTS idx_commentary_links_block ON commentary_links(block_id);
        CREATE INDEX IF NOT EXISTS idx_knowledge_patch_applications_task
            ON knowledge_patch_applications(task_id);
        CREATE INDEX IF NOT EXISTS idx_kb_version_diff_reports_after
            ON kb_version_diff_reports(after_version_id);
        CREATE INDEX IF NOT EXISTS idx_kb_version_diff_reports_created
            ON kb_version_diff_reports(created_at);
        "#,
    )?;
    ensure_source_metadata_columns(conn)?;
    ensure_search_normalization_columns(conn)?;
    Ok(())
}

fn table_column_names(conn: &Connection, table: &str) -> Result<BTreeSet<String>> {
    conn.prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .map_err(Into::into)
}

fn ensure_source_metadata_columns(conn: &Connection) -> Result<()> {
    let existing = table_column_names(conn, "sources")?;
    for column in [
        "source_url",
        "license",
        "license_url",
        "license_source_url",
        "attribution",
        "usage_boundary",
    ] {
        if !existing.contains(column) {
            conn.execute(&format!("ALTER TABLE sources ADD COLUMN {column} TEXT"), [])?;
        }
    }
    Ok(())
}

fn ensure_search_normalization_columns(conn: &Connection) -> Result<()> {
    let block_columns = table_column_names(conn, "blocks")?;
    if !block_columns.contains("normalized_source_title") {
        conn.execute(
            "ALTER TABLE blocks ADD COLUMN normalized_source_title TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    let alias_columns = table_column_names(conn, "aliases")?;
    if !alias_columns.contains("normalized_alias") {
        conn.execute(
            "ALTER TABLE aliases ADD COLUMN normalized_alias TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_blocks_normalized_source_title ON blocks(normalized_source_title)",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_aliases_normalized_alias ON aliases(normalized_alias)",
        [],
    )?;
    backfill_search_normalization_columns(conn)?;
    Ok(())
}

fn backfill_search_normalization_columns(conn: &Connection) -> Result<()> {
    let block_rows = {
        let mut stmt = conn.prepare(
            r#"
            SELECT block_id, source_title
            FROM blocks
            WHERE normalized_source_title IS NULL OR normalized_source_title = ''
            "#,
        )?;
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (block_id, source_title) in block_rows {
        conn.execute(
            "UPDATE blocks SET normalized_source_title = ?1 WHERE block_id = ?2",
            params![normalize_title(&source_title), block_id],
        )?;
    }
    let alias_rows = {
        let mut stmt = conn.prepare(
            r#"
            SELECT alias
            FROM aliases
            WHERE normalized_alias IS NULL OR normalized_alias = ''
            "#,
        )?;
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for alias in alias_rows {
        conn.execute(
            "UPDATE aliases SET normalized_alias = ?1 WHERE alias = ?2",
            params![normalize_alias(&alias), alias],
        )?;
    }
    Ok(())
}

pub fn backfill_source_metadata_from_snapshots(
    conn: &Connection,
    source_root: &Path,
    apply: bool,
) -> Result<Value> {
    let source_dirs = list_source_dirs(source_root)?;
    let mut metadata_by_source = BTreeMap::new();
    let mut metadata_errors = Vec::new();
    for source_dir in source_dirs {
        let metadata: SourceMetadata = read_json(&source_dir.join("metadata/source.json"))?;
        for (field, value) in [
            ("source_url", metadata.source_url.as_deref()),
            ("license", metadata.license.as_deref()),
            ("license_url", metadata.license_url.as_deref()),
            ("license_source_url", metadata.license_source_url.as_deref()),
            ("attribution", metadata.attribution.as_deref()),
            ("usage_boundary", metadata.usage_boundary.as_deref()),
        ] {
            if value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                metadata_errors.push(format!("{}.{}_missing", metadata.source_id, field));
            }
        }
        metadata_by_source.insert(metadata.source_id.clone(), metadata);
    }
    if metadata_by_source.is_empty() {
        metadata_errors.push("source_metadata_empty".to_string());
    }

    let columns_before = table_column_names(conn, "sources")?;
    let missing_columns_before = [
        "source_url",
        "license",
        "license_url",
        "license_source_url",
        "attribution",
        "usage_boundary",
    ]
    .iter()
    .filter(|column| !columns_before.contains(**column))
    .copied()
    .collect::<Vec<_>>();
    let db_source_ids = conn
        .prepare("SELECT source_id FROM sources ORDER BY source_id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let missing_db_metadata = db_source_ids
        .iter()
        .filter(|source_id| !metadata_by_source.contains_key(*source_id))
        .cloned()
        .collect::<Vec<_>>();
    let mut errors = metadata_errors.clone();
    for source_id in &missing_db_metadata {
        errors.push(format!("metadata_not_found_for_db_source:{source_id}"));
    }

    let mut updated_source_count = 0usize;
    if apply && errors.is_empty() {
        init_knowledge_base_schema(conn)?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            for source_id in &db_source_ids {
                let Some(metadata) = metadata_by_source.get(source_id) else {
                    continue;
                };
                conn.execute(
                    r#"
                    UPDATE sources
                    SET source_url = ?1,
                        license = ?2,
                        license_url = ?3,
                        license_source_url = ?4,
                        attribution = ?5,
                        usage_boundary = ?6
                    WHERE source_id = ?7
                    "#,
                    params![
                        metadata.source_url.as_deref(),
                        metadata.license.as_deref(),
                        metadata.license_url.as_deref(),
                        metadata.license_source_url.as_deref(),
                        metadata.attribution.as_deref(),
                        metadata.usage_boundary.as_deref(),
                        source_id,
                    ],
                )?;
                updated_source_count += 1;
            }
            Ok(())
        })();
        if result.is_ok() {
            conn.execute_batch("COMMIT")?;
        } else {
            conn.execute_batch("ROLLBACK")?;
            result?;
        }
    }

    let columns_after = table_column_names(conn, "sources")?;
    let missing_columns_after = [
        "source_url",
        "license",
        "license_url",
        "license_source_url",
        "attribution",
        "usage_boundary",
    ]
    .iter()
    .filter(|column| !columns_after.contains(**column))
    .copied()
    .collect::<Vec<_>>();
    let missing_values_after = if missing_columns_after.is_empty() {
        let mut result = BTreeMap::new();
        for column in [
            "source_url",
            "license",
            "license_url",
            "license_source_url",
            "attribution",
            "usage_boundary",
        ] {
            let count = conn.query_row(
                &format!(
                    "SELECT count(*) FROM sources WHERE {column} IS NULL OR trim({column}) = ''"
                ),
                [],
                |row| row.get::<_, i64>(0),
            )?;
            result.insert(column.to_string(), count);
        }
        result
    } else {
        BTreeMap::new()
    };
    if apply {
        if !missing_columns_after.is_empty() {
            errors.push("source_metadata_columns_missing_after_apply".to_string());
        }
        if missing_values_after.values().any(|count| *count > 0) {
            errors.push("source_metadata_missing_after_apply".to_string());
        }
    }

    Ok(json!({
        "object": "tonglingyu.kb_source_metadata_backfill",
        "schema_version": 1,
        "status": if errors.is_empty() { "ok" } else { "failed" },
        "applied": apply,
        "source_root": source_root.display().to_string(),
        "metadata_source_count": metadata_by_source.len(),
        "db_source_count": db_source_ids.len(),
        "missing_columns_before": missing_columns_before,
        "missing_columns_after": missing_columns_after,
        "missing_values_after": missing_values_after,
        "updated_source_count": updated_source_count,
        "additive_only": true,
        "errors": errors,
        "secret_values_printed": false,
    }))
}

pub fn rebuild_knowledge_base_from_snapshots(
    conn: &Connection,
    source_root: &Path,
) -> Result<KnowledgeBaseBuildReport> {
    init_runtime_schema(conn)?;
    init_knowledge_base_schema(conn)?;
    let source_dirs = list_source_dirs(source_root)?;
    if source_dirs.is_empty() {
        return Err(anyhow!(
            "no source snapshots found under {}",
            source_root.display()
        ));
    }
    let before_summary = knowledge_base_summary(conn)?;
    clear_knowledge_base_rows(conn)?;
    seed_aliases(conn)?;
    for source_dir in source_dirs {
        load_source_snapshot(conn, &source_dir)?;
    }
    let patch_application_report = apply_accepted_knowledge_patch_proposals(conn)?;
    let mut report = write_kb_version(conn, source_root)?;
    let after_summary = knowledge_base_summary(conn)?
        .ok_or_else(|| anyhow!("knowledge base summary missing after rebuild"))?;
    report.diff_report = write_kb_version_diff_report(
        conn,
        before_summary,
        after_summary,
        patch_application_report,
    )?;
    Ok(report)
}

fn clear_knowledge_base_rows(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        DELETE FROM evidence_claim_links;
        DELETE FROM audit_events;
        DELETE FROM review_records;
        DELETE FROM evidence_cards;
        DELETE FROM evidence_packages;
        DELETE FROM poems;
        DELETE FROM events;
        DELETE FROM relationships;
        DELETE FROM aliases;
        DELETE FROM people;
        DELETE FROM version_differences;
        DELETE FROM terms;
        DELETE FROM commentary_links;
        DELETE FROM knowledge_patch_applications;
        DELETE FROM version_notes;
        DELETE FROM commentaries;
        DELETE FROM rare_char_annotations;
        DELETE FROM blocks_fts;
        DELETE FROM blocks;
        DELETE FROM chapters;
        DELETE FROM editions;
        DELETE FROM source_documents;
        DELETE FROM sources;
        DELETE FROM kb_version;
        "#,
    )?;
    Ok(())
}

fn list_source_dirs(root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let path = entry?.path();
        if path.is_dir() && path.join("metadata/source.json").is_file() {
            dirs.push(path);
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn load_source_snapshot(conn: &Connection, source_dir: &Path) -> Result<()> {
    let source_path = source_dir.join("metadata/source.json");
    let report_path = source_dir.join("metadata/extraction_report.json");
    let documents_path = source_dir.join("documents/documents.jsonl");
    let blocks_path = source_dir.join("documents/blocks.jsonl");

    let source: SourceMetadata = read_json(&source_path)?;
    let report: ExtractionReport = read_json(&report_path)?;
    if report.missing != 0 {
        return Err(anyhow!("{} has missing pages", source.source_id));
    }
    if report.raw_html_files.unwrap_or_default() != 0 {
        return Err(anyhow!(
            "{} contains raw_html files in current M1 contract",
            source.source_id
        ));
    }
    let source_hash = hash_files([&source_path, &report_path, &documents_path, &blocks_path])?;
    let source_usage_boundary = source
        .usage_boundary
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            retrieval_rules::usage_limit(&source.source_category)
                .expect("retrieval usage limit rules must load")
        });
    conn.execute(
        r#"
        INSERT INTO sources (
            source_id, source_category, format, title, work, edition, language,
            source_url, api_url, fetched_at, license, license_url,
            license_source_url, attribution, usage_boundary, notes,
            snapshot_contract_json, source_hash
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
        "#,
        params![
            source.source_id,
            source.source_category,
            source.format,
            source.title,
            source.work,
            source.edition,
            source.language,
            source.source_url,
            source.api_url,
            source.fetched_at,
            source.license,
            source.license_url,
            source.license_source_url,
            source.attribution,
            source_usage_boundary,
            source.notes,
            serde_json::to_string(&source.snapshot_contract)?,
            source_hash
        ],
    )?;

    conn.execute(
        "INSERT INTO editions (edition_id, source_id, edition_label, version_system, usage_limit) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            format!("edition:{}", source.source_id),
            source.source_id,
            source.edition.unwrap_or_else(|| "未标注版本".to_string()),
            retrieval_rules::version_system(&source.source_id)?,
            source_usage_boundary,
        ],
    )?;
    conn.execute(
        "INSERT INTO version_notes (version_note_id, source_id, note, source_status, usage_limit) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            format!("version-note:{}", source.source_id),
            source.source_id,
            source.notes.unwrap_or_else(|| "第一批 Wikisource source snapshot".to_string()),
            "source_snapshot_ready",
            source_usage_boundary,
        ],
    )?;

    let mut document_count = 0_i64;
    for document in read_jsonl::<DocumentRecord>(&documents_path)? {
        document_count += 1;
        conn.execute(
            r#"
            INSERT INTO source_documents (
                section_id, source_id, section_index, title, display_title, fullurl,
                pageid, revision_id, revision_timestamp, wikitext_sha256
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
            params![
                document.section_id,
                document.source_id,
                document.section_index,
                document.title,
                document.display_title,
                document.fullurl,
                document.pageid,
                document.revision_id,
                document.revision_timestamp,
                document.wikitext_sha256,
            ],
        )?;
    }
    if document_count != report.documents {
        return Err(anyhow!(
            "{} document count mismatch: report={} loaded={}",
            source.source_id,
            report.documents,
            document_count
        ));
    }

    let mut block_count = 0_i64;
    let mut seen_chapters = HashSet::new();
    let mut commentary_count = 0_i64;
    for block in read_jsonl::<BlockRecord>(&blocks_path)? {
        block_count += 1;
        let normalized_text = normalize_text(&block.text);
        let normalized_source_title = normalize_title(&block.source_title);
        let evidence_type = evidence_type(&source.source_category, &source.source_id, &block)?;
        let chapter_no = extract_chapter_no(&block.source_title);
        if let Some(no) = chapter_no {
            let chapter_id = format!("{}:chapter:{no:03}", source.source_id);
            if seen_chapters.insert(chapter_id.clone()) {
                conn.execute(
                    "INSERT OR IGNORE INTO chapters (chapter_id, source_id, chapter_no, title, version_range) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![chapter_id, source.source_id, no, block.source_title, version_range(no)],
                )?;
            }
        }
        conn.execute(
            r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, normalized_source_title,
                source_url, revision_id, block_index, kind, tag, text, normalized_text,
                evidence_type, chapter_no
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            "#,
            params![
                block.block_id,
                block.source_id,
                block.section_id,
                block.source_title,
                normalized_source_title,
                block.source_url,
                block.revision_id,
                block.block_index,
                block.kind,
                block.tag,
                block.text,
                normalized_text,
                evidence_type,
                chapter_no,
            ],
        )?;
        conn.execute(
            "INSERT INTO blocks_fts (block_id, source_id, source_title, text, normalized_text) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![block.block_id, block.source_id, block.source_title, block.text, normalized_text],
        )?;
        if evidence_type == "commentary" && useful_text(&block.text) {
            commentary_count += 1;
            conn.execute(
                "INSERT INTO commentaries (commentary_id, block_id, source_id, commentary_text, commentary_type, version_label) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    format!("commentary:{}:{commentary_count}", source.source_id),
                    block.block_id,
                    block.source_id,
                    block.text,
                    commentary_type(&block.text),
                    retrieval_rules::version_system(&source.source_id)?,
                ],
            )?;
        }
    }
    if block_count != report.blocks {
        return Err(anyhow!(
            "{} block count mismatch: report={} loaded={}",
            source.source_id,
            report.blocks,
            block_count
        ));
    }
    let _rare_count = report.rare_char_annotations.unwrap_or_default();
    Ok(())
}

struct AcceptedKnowledgePatchProposal {
    record: KnowledgePatchProposalRecord,
    evidence_ref: String,
}

fn apply_accepted_knowledge_patch_proposals(conn: &Connection) -> Result<Value> {
    if !sqlite_table_exists(conn, "knowledge_patch_proposals")?
        || !sqlite_table_exists(conn, "knowledge_governance_tasks")?
    {
        return Ok(json!({
            "object": "tonglingyu.knowledge_patch_application_report",
            "accepted_proposal_count": 0,
            "applied_count": 0,
            "by_type": {},
            "applications": [],
        }));
    }
    let proposals = accepted_knowledge_patch_proposals(conn)?;
    let mut by_type = BTreeMap::<String, i64>::new();
    let mut applications = Vec::new();
    for proposal in &proposals {
        let (target_table, target_key) = apply_accepted_knowledge_patch_proposal(conn, proposal)?;
        *by_type
            .entry(proposal.record.proposal_type.clone())
            .or_default() += 1;
        record_knowledge_patch_application(conn, proposal, &target_table, &target_key)?;
        applications.push(json!({
            "proposal_id": &proposal.record.proposal_id,
            "task_id": &proposal.record.task_id,
            "proposal_type": &proposal.record.proposal_type,
            "target_table": target_table,
            "target_key": target_key,
            "payload_sha256": &proposal.record.payload_sha256,
            "source_ref_sha256": hash_text(&proposal.record.source_ref),
            "evidence_ref_sha256": hash_text(&proposal.evidence_ref),
        }));
    }
    let report = json!({
        "object": "tonglingyu.knowledge_patch_application_report",
        "accepted_proposal_count": proposals.len(),
        "applied_count": applications.len(),
        "by_type": by_type,
        "applications": applications,
        "direct_agent_fact_mutation": false,
    });
    if !proposals.is_empty() {
        append_runtime_audit_event(
            conn,
            "kb-rebuild",
            "knowledge_patch_proposals_applied",
            &json!({
                "accepted_proposal_count": proposals.len(),
                "applied_count": report["applied_count"],
                "by_type": report["by_type"],
                "direct_agent_fact_mutation": false,
            }),
        )?;
    }
    Ok(report)
}

fn accepted_knowledge_patch_proposals(
    conn: &Connection,
) -> Result<Vec<AcceptedKnowledgePatchProposal>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            p.proposal_id, p.proposal_type, p.trace_id, p.package_id,
            p.source_ref, p.payload_json, p.payload_sha256, p.task_id,
            p.created_by, p.created_at, p.updated_at, t.evidence_ref
        FROM knowledge_patch_proposals AS p
        JOIN knowledge_governance_tasks AS t ON t.task_id = p.task_id
        WHERE t.status = 'accepted'
        ORDER BY COALESCE(t.accepted_at, t.updated_at), p.proposal_id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let payload_json: String = row.get(5)?;
        let payload = serde_json::from_str(&payload_json)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        Ok(AcceptedKnowledgePatchProposal {
            record: KnowledgePatchProposalRecord {
                proposal_id: row.get(0)?,
                proposal_type: row.get(1)?,
                trace_id: row.get(2)?,
                package_id: row.get(3)?,
                source_ref: row.get(4)?,
                payload,
                payload_sha256: row.get(6)?,
                task_id: row.get(7)?,
                created_by: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            },
            evidence_ref: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
        })
    })?;
    let proposals = rows
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .collect::<Vec<_>>();
    for proposal in &proposals {
        if proposal.evidence_ref.trim().is_empty() {
            return Err(anyhow!(
                "accepted knowledge patch proposal {} is missing evidence_ref",
                proposal.record.proposal_id
            ));
        }
    }
    Ok(proposals)
}

fn apply_accepted_knowledge_patch_proposal(
    conn: &Connection,
    proposal: &AcceptedKnowledgePatchProposal,
) -> Result<(String, String)> {
    match proposal.record.proposal_type.as_str() {
        "alias" => apply_alias_patch(conn, proposal),
        "term" => apply_term_patch(conn, proposal),
        "commentary_link" => apply_commentary_link_patch(conn, proposal),
        "version_note" => apply_version_note_patch(conn, proposal),
        proposal_type => Err(anyhow!(
            "unsupported accepted knowledge patch proposal_type {proposal_type}"
        )),
    }
}

fn apply_alias_patch(
    conn: &Connection,
    proposal: &AcceptedKnowledgePatchProposal,
) -> Result<(String, String)> {
    let payload = &proposal.record.payload;
    let alias = required_payload_string(payload, "alias")?.to_string();
    let person_id = required_payload_string(payload, "target_ref")?.to_string();
    ensure_row_exists(
        conn,
        "people",
        "person_id",
        &person_id,
        "accepted alias proposal target person does not exist",
    )?;
    let normalized_alias = normalize_alias(&alias);
    let existing_person_ids = conn
        .prepare(
            "SELECT DISTINCT person_id FROM aliases WHERE alias = ?1 OR normalized_alias = ?2",
        )?
        .query_map(params![&alias, &normalized_alias], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if !existing_person_ids.is_empty() {
        if existing_person_ids.iter().any(|item| item != &person_id) {
            return Err(anyhow!(
                "accepted alias proposal conflicts with existing alias {alias}"
            ));
        }
    } else {
        let scope = payload_optional_string(payload, "scope", 240).unwrap_or_else(|| {
            format!(
                "accepted_knowledge_patch:{}",
                proposal.record.task_id.as_str()
            )
        });
        conn.execute(
            "INSERT INTO aliases (alias, normalized_alias, person_id, scope) VALUES (?1, ?2, ?3, ?4)",
            params![&alias, normalized_alias, &person_id, scope],
        )?;
    }
    Ok(("aliases".to_string(), alias))
}

fn apply_term_patch(
    conn: &Connection,
    proposal: &AcceptedKnowledgePatchProposal,
) -> Result<(String, String)> {
    let payload = &proposal.record.payload;
    let term = required_payload_string(payload, "term")?.to_string();
    let usage_boundary = required_payload_string(payload, "usage_boundary")?.to_string();
    let category = payload_optional_string(payload, "category", 120);
    let note = payload_optional_string(payload, "note", 480);
    let term_id = format!("term:proposal:{}", proposal.record.proposal_id);
    conn.execute(
        r#"
        INSERT OR IGNORE INTO terms (
            term_id, term, category, usage_boundary, note, source_ref,
            accepted_task_id, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            &term_id,
            &term,
            &category,
            &usage_boundary,
            &note,
            &proposal.record.source_ref,
            &proposal.record.task_id,
            now_rfc3339(),
        ],
    )?;
    Ok(("terms".to_string(), term))
}

fn apply_commentary_link_patch(
    conn: &Connection,
    proposal: &AcceptedKnowledgePatchProposal,
) -> Result<(String, String)> {
    let payload = &proposal.record.payload;
    let commentary_ref = required_payload_string(payload, "commentary_ref")?.to_string();
    let block_id = required_payload_string(payload, "block_id")?.to_string();
    ensure_row_exists(
        conn,
        "blocks",
        "block_id",
        &block_id,
        "accepted commentary link proposal block does not exist",
    )?;
    let usage_boundary = payload_optional_string(payload, "usage_boundary", 480)
        .unwrap_or_else(|| "accepted expert-reviewed commentary link".to_string());
    let link_id = format!("commentary-link:proposal:{}", proposal.record.proposal_id);
    conn.execute(
        r#"
        INSERT OR IGNORE INTO commentary_links (
            link_id, commentary_ref, block_id, source_ref, usage_boundary,
            accepted_task_id, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            &link_id,
            &commentary_ref,
            &block_id,
            &proposal.record.source_ref,
            &usage_boundary,
            &proposal.record.task_id,
            now_rfc3339(),
        ],
    )?;
    Ok(("commentary_links".to_string(), link_id))
}

fn apply_version_note_patch(
    conn: &Connection,
    proposal: &AcceptedKnowledgePatchProposal,
) -> Result<(String, String)> {
    let payload = &proposal.record.payload;
    let source_id = required_payload_string(payload, "source_id")?.to_string();
    let note = required_payload_string(payload, "note")?.to_string();
    ensure_row_exists(
        conn,
        "sources",
        "source_id",
        &source_id,
        "accepted version note proposal source does not exist",
    )?;
    let source_status = payload_optional_string(payload, "source_status", 120)
        .unwrap_or_else(|| "accepted_knowledge_patch".to_string());
    let usage_limit = payload_optional_string(payload, "usage_boundary", 480)
        .unwrap_or_else(|| "accepted expert-reviewed version note".to_string());
    let version_note_id = format!("version-note:proposal:{}", proposal.record.proposal_id);
    conn.execute(
        r#"
        INSERT OR IGNORE INTO version_notes (
            version_note_id, source_id, note, source_status, usage_limit
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
        params![
            &version_note_id,
            &source_id,
            &note,
            &source_status,
            &usage_limit,
        ],
    )?;
    Ok(("version_notes".to_string(), version_note_id))
}

fn record_knowledge_patch_application(
    conn: &Connection,
    proposal: &AcceptedKnowledgePatchProposal,
    target_table: &str,
    target_key: &str,
) -> Result<()> {
    conn.execute(
        r#"
        INSERT OR IGNORE INTO knowledge_patch_applications (
            application_id, proposal_id, task_id, proposal_type, target_table,
            target_key, payload_sha256, source_ref, evidence_ref, applied_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        "#,
        params![
            format!("kpa-{}", uuid::Uuid::now_v7().simple()),
            &proposal.record.proposal_id,
            &proposal.record.task_id,
            &proposal.record.proposal_type,
            target_table,
            target_key,
            &proposal.record.payload_sha256,
            &proposal.record.source_ref,
            &proposal.evidence_ref,
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

fn payload_optional_string(payload: &Value, field: &str, max_chars: usize) -> Option<String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(max_chars).collect::<String>())
}

fn ensure_row_exists(
    conn: &Connection,
    table: &str,
    column: &str,
    value: &str,
    message: &str,
) -> Result<()> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1");
    let count: i64 = conn.query_row(&sql, params![value], |row| row.get(0))?;
    if count == 0 {
        Err(anyhow!("{message}: {value}"))
    } else {
        Ok(())
    }
}

fn write_kb_version(conn: &Connection, source_root: &Path) -> Result<KnowledgeBaseBuildReport> {
    let source_count: i64 = conn.query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))?;
    let block_count: i64 = conn.query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))?;
    let version_id = format!("kb-{}", uuid::Uuid::now_v7().simple());
    let built_at = now_rfc3339();
    conn.execute(
        "INSERT INTO kb_version (version_id, source_root, source_count, block_count, schema_version, built_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            &version_id,
            source_root.display().to_string(),
            source_count,
            block_count,
            KNOWLEDGE_BASE_SCHEMA_VERSION,
            &built_at,
        ],
    )?;
    let summary = knowledge_base_summary(conn)?
        .ok_or_else(|| anyhow!("knowledge base summary missing after version write"))?;
    Ok(KnowledgeBaseBuildReport {
        version_id,
        source_root: source_root.display().to_string(),
        source_count,
        block_count,
        schema_version: KNOWLEDGE_BASE_SCHEMA_VERSION.to_string(),
        built_at,
        source_snapshot_digest: summary["source_snapshot_digest"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        kb_build_hash: summary["kb_build_hash"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        diff_report: Value::Null,
    })
}

fn knowledge_base_summary(conn: &Connection) -> Result<Option<Value>> {
    if !sqlite_table_exists(conn, "kb_version")? {
        return Ok(None);
    }
    let Some(kb_version) = latest_kb_version_json(conn)? else {
        return Ok(None);
    };
    let source_hashes = source_hash_refs(conn)?;
    let knowledge_calibration_report_refs = knowledge_calibration_report_refs(conn)?;
    let knowledge_state = knowledge_state_kb_summary(conn)?;
    let source_snapshot_digest = hash_text(&serde_json::to_string(&source_hashes)?);
    let counts = json!({
        "sources": table_count_if_exists(conn, "sources")?,
        "blocks": table_count_if_exists(conn, "blocks")?,
        "commentaries": table_count_if_exists(conn, "commentaries")?,
        "version_notes": table_count_if_exists(conn, "version_notes")?,
        "aliases": table_count_if_exists(conn, "aliases")?,
        "terms": table_count_if_exists(conn, "terms")?,
        "commentary_links": table_count_if_exists(conn, "commentary_links")?,
        "knowledge_patch_applications": table_count_if_exists(conn, "knowledge_patch_applications")?,
        "knowledge_calibration_reports": table_count_if_exists(conn, "knowledge_calibration_reports")?,
        "knowledge_calibration_jobs": table_count_if_exists(conn, "knowledge_calibration_jobs")?,
        "rare_char_annotations": table_count_if_exists(conn, "rare_char_annotations")?,
    });
    let kb_build_hash = hash_text(&serde_json::to_string(&canonical_json_value(&json!({
        "kb_version": kb_version,
        "source_snapshot_digest": source_snapshot_digest,
        "counts": counts,
    })))?);
    Ok(Some(json!({
        "object": "tonglingyu.kb_version_summary",
        "schema_version": KNOWLEDGE_BASE_SCHEMA_VERSION,
        "kb_version": latest_kb_version_json(conn)?.expect("kb_version checked"),
        "counts": counts,
        "source_hashes": source_hashes,
        "knowledge_calibration_report_refs": knowledge_calibration_report_refs,
        "knowledge_state": knowledge_state,
        "source_snapshot_digest": source_snapshot_digest,
        "kb_build_hash": kb_build_hash,
    })))
}

fn latest_kb_version_json(conn: &Connection) -> Result<Option<Value>> {
    if !sqlite_table_exists(conn, "kb_version")? {
        return Ok(None);
    }
    conn.query_row(
        r#"
        SELECT version_id, source_root, source_count, block_count, schema_version, built_at
        FROM kb_version
        ORDER BY built_at DESC, version_id DESC
        LIMIT 1
        "#,
        [],
        |row| {
            Ok(json!({
                "version_id": row.get::<_, String>(0)?,
                "source_root": row.get::<_, String>(1)?,
                "source_count": row.get::<_, i64>(2)?,
                "block_count": row.get::<_, i64>(3)?,
                "schema_version": row.get::<_, String>(4)?,
                "built_at": row.get::<_, String>(5)?,
            }))
        },
    )
    .optional()
    .map_err(Into::into)
}

fn source_hash_refs(conn: &Connection) -> Result<Vec<Value>> {
    if !sqlite_table_exists(conn, "sources")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        r#"
        SELECT source_id, source_hash
        FROM sources
        ORDER BY source_id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(json!({
            "source_id": row.get::<_, String>(0)?,
            "source_hash": row.get::<_, String>(1)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn knowledge_calibration_report_refs(conn: &Connection) -> Result<Vec<Value>> {
    if !sqlite_table_exists(conn, "knowledge_calibration_reports")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        r#"
        SELECT report_id, report_ref, item_id, kind, method, decision, report_hash, created_at
        FROM knowledge_calibration_reports
        ORDER BY created_at DESC, report_id DESC
        LIMIT 50
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(json!({
            "report_id": row.get::<_, String>(0)?,
            "report_ref": row.get::<_, String>(1)?,
            "item_id": row.get::<_, String>(2)?,
            "kind": row.get::<_, String>(3)?,
            "method": row.get::<_, String>(4)?,
            "decision": row.get::<_, String>(5)?,
            "report_hash": row.get::<_, String>(6)?,
            "created_at": row.get::<_, String>(7)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn knowledge_state_kb_summary(conn: &Connection) -> Result<Value> {
    let state_counts = knowledge_item_state_counts(conn)?;
    let by_kind = knowledge_item_kind_state_counts(conn)?;
    let state_change_refs = knowledge_item_state_change_refs(conn, 100)?;
    let runtime_policy_promotion_summary = knowledge_runtime_policy_promotion_summary(conn)?;
    let calibration_job_summary = knowledge_calibration_job_summary(conn)?;
    let unresolved_gaps = knowledge_unresolved_gap_summary(&state_counts);
    Ok(json!({
        "object": "tonglingyu.knowledge_state_kb_summary",
        "schema_version": KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION,
        "runtime_policy_version": KNOWLEDGE_RUNTIME_POLICY_VERSION,
        "state_counts": state_counts,
        "by_kind": by_kind,
        "state_change_refs": state_change_refs,
        "runtime_policy_promotion_summary": runtime_policy_promotion_summary,
        "calibration_job_summary": calibration_job_summary,
        "unresolved_gaps": unresolved_gaps,
        "summary_sha256": hash_text(&serde_json::to_string(&canonical_json_value(&json!({
            "state_counts": state_counts,
            "by_kind": by_kind,
            "runtime_policy_promotion_summary": runtime_policy_promotion_summary,
            "calibration_job_summary": calibration_job_summary,
            "unresolved_gaps": unresolved_gaps,
        })))?),
    }))
}

fn knowledge_item_state_counts(conn: &Connection) -> Result<Value> {
    let mut counts = serde_json::Map::new();
    for state in [
        KnowledgeState::SourceSnapshot,
        KnowledgeState::Candidate,
        KnowledgeState::SystemCalibrated,
        KnowledgeState::RuntimeUsable,
        KnowledgeState::HumanMarked,
        KnowledgeState::Rejected,
        KnowledgeState::Deprecated,
    ] {
        counts.insert(state.as_str().to_string(), json!(0));
    }
    if sqlite_table_exists(conn, "knowledge_items")? {
        let mut stmt =
            conn.prepare("SELECT state, COUNT(*) FROM knowledge_items GROUP BY state")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (state, count) = row?;
            counts.insert(state, json!(count));
        }
    }
    let runtime_usable = counts
        .get(KnowledgeState::RuntimeUsable.as_str())
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let human_marked = counts
        .get(KnowledgeState::HumanMarked.as_str())
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let system_calibrated = counts
        .get(KnowledgeState::SystemCalibrated.as_str())
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let rejected = counts
        .get(KnowledgeState::Rejected.as_str())
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let deprecated = counts
        .get(KnowledgeState::Deprecated.as_str())
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let candidate = counts
        .get(KnowledgeState::Candidate.as_str())
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let source_snapshot = counts
        .get(KnowledgeState::SourceSnapshot.as_str())
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Ok(json!({
        "object": "tonglingyu.knowledge_state_counts",
        "states": Value::Object(counts),
        "runtime_usable_count": runtime_usable,
        "human_marked_count": human_marked,
        "system_calibrated_count": system_calibrated,
        "rejected_or_deprecated_count": rejected + deprecated,
        "candidate_or_source_snapshot_count": candidate + source_snapshot,
        "total_count": runtime_usable
            + human_marked
            + system_calibrated
            + rejected
            + deprecated
            + candidate
            + source_snapshot,
    }))
}

fn knowledge_item_kind_state_counts(conn: &Connection) -> Result<Value> {
    if !sqlite_table_exists(conn, "knowledge_items")? {
        return Ok(json!({}));
    }
    let mut by_kind = serde_json::Map::new();
    let mut stmt = conn.prepare(
        r#"
        SELECT kind, state, COUNT(*)
        FROM knowledge_items
        GROUP BY kind, state
        ORDER BY kind, state
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (kind, state, count) = row?;
        let entry = by_kind
            .entry(kind)
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if let Some(object) = entry.as_object_mut() {
            object.insert(state, json!(count));
        }
    }
    Ok(Value::Object(by_kind))
}

fn knowledge_item_state_change_refs(conn: &Connection, limit: usize) -> Result<Vec<Value>> {
    if !sqlite_table_exists(conn, "knowledge_item_state_history")?
        || !sqlite_table_exists(conn, "knowledge_items")?
    {
        return Ok(Vec::new());
    }
    let limit_i64 = limit as i64;
    let mut stmt = conn.prepare(
        r#"
        SELECT
            h.history_id, h.item_id, h.previous_state, h.new_state, h.actor,
            h.reason_sha256, h.evidence_refs_json, h.state_version, h.created_at,
            i.kind, i.source_refs_json, i.calibration_report_ref, i.payload_json
        FROM knowledge_item_state_history AS h
        JOIN knowledge_items AS i ON i.item_id = h.item_id
        ORDER BY h.created_at DESC, h.history_id DESC
        LIMIT ?1
        "#,
    )?;
    let rows = stmt.query_map(params![limit_i64], |row| {
        let evidence_refs_json: String = row.get(6)?;
        let source_refs_json: String = row.get(10)?;
        let payload_json: String = row.get(12)?;
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            evidence_refs_json,
            row.get::<_, i64>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            source_refs_json,
            row.get::<_, Option<String>>(11)?,
            payload_json,
        ))
    })?;
    let mut changes = Vec::new();
    for row in rows {
        let (
            history_id,
            item_id,
            previous_state,
            new_state,
            actor,
            reason_sha256,
            evidence_refs_json,
            state_version,
            created_at,
            kind,
            source_refs_json,
            calibration_report_ref,
            payload_json,
        ) = row?;
        let payload = serde_json::from_str::<Value>(&payload_json).unwrap_or(Value::Null);
        changes.push(json!({
            "history_id": history_id,
            "item_id": item_id,
            "kind": kind,
            "previous_state": previous_state,
            "new_state": new_state,
            "state_version": state_version,
            "actor": actor,
            "reason_sha256": reason_sha256,
            "source_refs": serde_json::from_str::<Value>(&source_refs_json).unwrap_or(Value::Null),
            "evidence_refs": serde_json::from_str::<Value>(&evidence_refs_json).unwrap_or(Value::Null),
            "calibration_report_ref": calibration_report_ref,
            "human_review_ref": payload.get("human_review").cloned().unwrap_or(Value::Null),
            "runtime_policy_ref": payload.get("runtime_policy").cloned().unwrap_or(Value::Null),
            "audit_refs": audit_refs_for_knowledge_item(conn, &item_id, 8)?,
            "created_at": created_at,
        }));
    }
    Ok(changes)
}

fn audit_refs_for_knowledge_item(
    conn: &Connection,
    item_id: &str,
    limit: usize,
) -> Result<Vec<Value>> {
    if !sqlite_table_exists(conn, "audit_events")? {
        return Ok(Vec::new());
    }
    let like_pattern = format!("%{item_id}%");
    let limit_i64 = limit as i64;
    let mut stmt = conn.prepare(
        r#"
        SELECT event_id, trace_id, event_type, created_at
        FROM audit_events
        WHERE payload_json LIKE ?1
        ORDER BY created_at DESC, event_id DESC
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map(params![like_pattern, limit_i64], |row| {
        Ok(json!({
            "event_id": row.get::<_, String>(0)?,
            "trace_id": row.get::<_, String>(1)?,
            "event_type": row.get::<_, String>(2)?,
            "created_at": row.get::<_, String>(3)?,
        }))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn knowledge_runtime_policy_promotion_summary(conn: &Connection) -> Result<Value> {
    if !sqlite_table_exists(conn, "knowledge_items")? {
        return Ok(json!({
            "object": "tonglingyu.knowledge_runtime_policy_promotion_summary",
            "policy_version": KNOWLEDGE_RUNTIME_POLICY_VERSION,
            "runtime_usable_count": 0,
            "by_kind": {},
            "release_run_refs": [],
        }));
    }
    let mut runtime_usable_count = 0_i64;
    let mut by_kind = BTreeMap::<String, i64>::new();
    let mut release_run_refs = BTreeSet::<String>::new();
    let mut stmt = conn.prepare(
        r#"
        SELECT kind, payload_json
        FROM knowledge_items
        WHERE state = ?1
        "#,
    )?;
    let rows = stmt.query_map(params![KnowledgeState::RuntimeUsable.as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (kind, payload_json) = row?;
        runtime_usable_count += 1;
        *by_kind.entry(kind).or_default() += 1;
        let payload = serde_json::from_str::<Value>(&payload_json).unwrap_or(Value::Null);
        if let Some(release_run_id) = payload
            .get("runtime_policy")
            .and_then(|policy| policy.get("release_run_id"))
            .and_then(Value::as_str)
        {
            release_run_refs.insert(format!("sha256:{}", hash_text(release_run_id)));
        }
    }
    Ok(json!({
        "object": "tonglingyu.knowledge_runtime_policy_promotion_summary",
        "policy_version": KNOWLEDGE_RUNTIME_POLICY_VERSION,
        "runtime_usable_count": runtime_usable_count,
        "by_kind": by_kind,
        "release_run_refs": release_run_refs.into_iter().collect::<Vec<_>>(),
    }))
}

fn knowledge_calibration_job_summary(conn: &Connection) -> Result<Value> {
    if !sqlite_table_exists(conn, "knowledge_calibration_jobs")? {
        return Ok(json!({
            "object": "tonglingyu.knowledge_calibration_job_summary",
            "total": 0,
            "by_status": {},
            "failed_or_retry_waiting": 0,
        }));
    }
    let mut by_status = BTreeMap::<String, i64>::new();
    let mut stmt =
        conn.prepare("SELECT status, COUNT(*) FROM knowledge_calibration_jobs GROUP BY status")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut total = 0_i64;
    for row in rows {
        let (status, count) = row?;
        total += count;
        by_status.insert(status, count);
    }
    let failed_or_retry_waiting = by_status.get("failed").copied().unwrap_or(0)
        + by_status.get("retry_waiting").copied().unwrap_or(0);
    Ok(json!({
        "object": "tonglingyu.knowledge_calibration_job_summary",
        "total": total,
        "by_status": by_status,
        "failed_or_retry_waiting": failed_or_retry_waiting,
    }))
}

fn knowledge_unresolved_gap_summary(state_counts: &Value) -> Value {
    let states = state_counts.get("states").and_then(Value::as_object);
    let count = |state: KnowledgeState| -> i64 {
        states
            .and_then(|items| items.get(state.as_str()))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    json!({
        "candidate_or_source_snapshot": count(KnowledgeState::Candidate)
            + count(KnowledgeState::SourceSnapshot),
        "system_calibrated_not_runtime_usable": count(KnowledgeState::SystemCalibrated),
        "rejected_or_deprecated": count(KnowledgeState::Rejected)
            + count(KnowledgeState::Deprecated),
    })
}

fn table_count_if_exists(conn: &Connection, table: &str) -> Result<i64> {
    if sqlite_table_exists(conn, table)? {
        table_count(conn, table)
    } else {
        Ok(0)
    }
}

fn write_kb_version_diff_report(
    conn: &Connection,
    before_summary: Option<Value>,
    after_summary: Value,
    patch_application_report: Value,
) -> Result<Value> {
    let after_version_id = after_summary["kb_version"]["version_id"]
        .as_str()
        .ok_or_else(|| anyhow!("after kb version summary missing version_id"))?
        .to_string();
    let before_version_id = before_summary
        .as_ref()
        .and_then(|summary| summary["kb_version"]["version_id"].as_str())
        .map(ToOwned::to_owned);
    let source_root = after_summary["kb_version"]["source_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let mut diff = kb_version_summary_diff(before_summary.as_ref(), &after_summary);
    if let Some(object) = diff.as_object_mut() {
        object.insert(
            "knowledge_patch_application".to_string(),
            patch_application_report,
        );
    }
    let report_id = format!("kb-diff-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    conn.execute(
        r#"
        INSERT INTO kb_version_diff_reports (
            report_id, schema_version, before_version_id, after_version_id,
            source_root, before_summary_json, after_summary_json, diff_json,
            eval_before_summary_json, eval_after_summary_json, eval_diff_json,
            created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, ?9, ?9
        )
        "#,
        params![
            &report_id,
            KB_VERSION_DIFF_REPORT_SCHEMA_VERSION,
            &before_version_id,
            &after_version_id,
            &source_root,
            optional_json_string(before_summary.as_ref())?,
            serde_json::to_string(&after_summary)?,
            serde_json::to_string(&diff)?,
            &now,
        ],
    )?;
    load_kb_version_diff_report(conn, &report_id)?
        .ok_or_else(|| anyhow!("kb version diff report was not readable after insert"))
}

pub fn record_kb_version_diff_eval_summaries(
    conn: &Connection,
    report_id: &str,
    before_eval_summary: Option<Value>,
    after_eval_summary: Value,
) -> Result<Option<Value>> {
    if !after_eval_summary.is_object() {
        return Err(anyhow!("after eval quality summary must be a JSON object"));
    }
    if before_eval_summary
        .as_ref()
        .is_some_and(|summary| !summary.is_object())
    {
        return Err(anyhow!("before eval quality summary must be a JSON object"));
    }
    let eval_diff = eval_quality_summary_diff(before_eval_summary.as_ref(), &after_eval_summary);
    let updated_at = now_rfc3339();
    let updated = conn.execute(
        r#"
        UPDATE kb_version_diff_reports
        SET eval_before_summary_json = ?2,
            eval_after_summary_json = ?3,
            eval_diff_json = ?4,
            updated_at = ?5
        WHERE report_id = ?1
        "#,
        params![
            report_id,
            optional_json_string(before_eval_summary.as_ref())?,
            serde_json::to_string(&after_eval_summary)?,
            serde_json::to_string(&eval_diff)?,
            updated_at,
        ],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    load_kb_version_diff_report(conn, report_id)
}

fn optional_json_string(value: Option<&Value>) -> Result<Option<String>> {
    value
        .map(serde_json::to_string)
        .transpose()
        .map_err(Into::into)
}

fn load_kb_version_diff_report(conn: &Connection, report_id: &str) -> Result<Option<Value>> {
    if !sqlite_table_exists(conn, "kb_version_diff_reports")? {
        return Ok(None);
    }
    conn.query_row(
        r#"
        SELECT report_id, schema_version, before_version_id, after_version_id,
               source_root, before_summary_json, after_summary_json, diff_json,
               eval_before_summary_json, eval_after_summary_json, eval_diff_json,
               created_at, updated_at
        FROM kb_version_diff_reports
        WHERE report_id = ?1
        "#,
        params![report_id],
        kb_version_diff_report_json_from_row,
    )
    .optional()
    .map_err(Into::into)
}

fn kb_version_diff_report_json_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let before_summary_json: Option<String> = row.get(5)?;
    let after_summary_json: String = row.get(6)?;
    let diff_json: String = row.get(7)?;
    let eval_before_summary_json: Option<String> = row.get(8)?;
    let eval_after_summary_json: Option<String> = row.get(9)?;
    let eval_diff_json: Option<String> = row.get(10)?;
    Ok(json!({
        "object": "tonglingyu.kb_version_diff_report",
        "report_id": row.get::<_, String>(0)?,
        "schema_version": row.get::<_, String>(1)?,
        "before_version_id": row.get::<_, Option<String>>(2)?,
        "after_version_id": row.get::<_, String>(3)?,
        "source_root": row.get::<_, String>(4)?,
        "before_summary": parse_optional_json_for_sql(before_summary_json)?,
        "after_summary": parse_json_for_sql(after_summary_json)?,
        "diff": parse_json_for_sql(diff_json)?,
        "eval_before_summary": parse_optional_json_for_sql(eval_before_summary_json)?,
        "eval_after_summary": parse_optional_json_for_sql(eval_after_summary_json)?,
        "eval_diff": parse_optional_json_for_sql(eval_diff_json)?,
        "created_at": row.get::<_, String>(11)?,
        "updated_at": row.get::<_, String>(12)?,
    }))
}

fn parse_json_for_sql(data: String) -> rusqlite::Result<Value> {
    serde_json::from_str(&data)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn parse_optional_json_for_sql(data: Option<String>) -> rusqlite::Result<Value> {
    match data {
        Some(data) => parse_json_for_sql(data),
        None => Ok(Value::Null),
    }
}

fn kb_version_summary_diff(before: Option<&Value>, after: &Value) -> Value {
    let before_counts = before
        .and_then(|summary| summary.get("counts"))
        .and_then(Value::as_object);
    let after_counts = after.get("counts").and_then(Value::as_object);
    let mut count_diff = serde_json::Map::new();
    for key in [
        "sources",
        "blocks",
        "commentaries",
        "version_notes",
        "aliases",
        "terms",
        "commentary_links",
        "knowledge_patch_applications",
        "rare_char_annotations",
    ] {
        let before_value = before_counts
            .and_then(|counts| counts.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let after_value = after_counts
            .and_then(|counts| counts.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        count_diff.insert(
            key.to_string(),
            json!({
                "before": before_value,
                "after": after_value,
                "delta": after_value - before_value,
            }),
        );
    }
    let before_sources = source_hash_map(before);
    let after_sources = source_hash_map(Some(after));
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged_count = 0_usize;
    for (source_id, after_hash) in &after_sources {
        match before_sources.get(source_id) {
            Some(before_hash) if before_hash == after_hash => unchanged_count += 1,
            Some(before_hash) => changed.push(json!({
                "source_id": source_id,
                "before_hash": before_hash,
                "after_hash": after_hash,
            })),
            None => added.push(json!({
                "source_id": source_id,
                "after_hash": after_hash,
            })),
        }
    }
    for (source_id, before_hash) in &before_sources {
        if !after_sources.contains_key(source_id) {
            removed.push(json!({
                "source_id": source_id,
                "before_hash": before_hash,
            }));
        }
    }
    json!({
        "object": "tonglingyu.kb_version_diff",
        "schema_version": KB_VERSION_DIFF_REPORT_SCHEMA_VERSION,
        "before_version_id": before
            .and_then(|summary| summary["kb_version"]["version_id"].as_str()),
        "after_version_id": after["kb_version"]["version_id"].as_str(),
        "source_snapshot_digest_changed": before
            .and_then(|summary| summary["source_snapshot_digest"].as_str())
            .is_some_and(|before_digest| {
                Some(before_digest) != after["source_snapshot_digest"].as_str()
            }),
        "kb_build_hash_changed": before
            .and_then(|summary| summary["kb_build_hash"].as_str())
            .is_some_and(|before_hash| Some(before_hash) != after["kb_build_hash"].as_str()),
        "counts": Value::Object(count_diff),
        "knowledge_state": knowledge_state_summary_diff(before, after),
        "sources": {
            "added": added,
            "removed": removed,
            "changed": changed,
            "unchanged_count": unchanged_count,
        },
    })
}

fn knowledge_state_summary_diff(before: Option<&Value>, after: &Value) -> Value {
    let before_knowledge = before.and_then(|summary| summary.get("knowledge_state"));
    let after_knowledge = after.get("knowledge_state");
    let mut state_count_diff = serde_json::Map::new();
    for state in [
        KnowledgeState::SourceSnapshot,
        KnowledgeState::Candidate,
        KnowledgeState::SystemCalibrated,
        KnowledgeState::RuntimeUsable,
        KnowledgeState::HumanMarked,
        KnowledgeState::Rejected,
        KnowledgeState::Deprecated,
    ] {
        let before_value = before_knowledge
            .and_then(|knowledge| knowledge.get("state_counts"))
            .and_then(|counts| counts.get("states"))
            .and_then(|states| states.get(state.as_str()))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let after_value = after_knowledge
            .and_then(|knowledge| knowledge.get("state_counts"))
            .and_then(|counts| counts.get("states"))
            .and_then(|states| states.get(state.as_str()))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        state_count_diff.insert(
            state.as_str().to_string(),
            json!({
                "before": before_value,
                "after": after_value,
                "delta": after_value - before_value,
            }),
        );
    }
    json!({
        "object": "tonglingyu.knowledge_state_kb_diff",
        "schema_version": KNOWLEDGE_ITEM_STATE_SCHEMA_VERSION,
        "runtime_policy_version": KNOWLEDGE_RUNTIME_POLICY_VERSION,
        "state_counts": Value::Object(state_count_diff),
        "by_kind": after_knowledge
            .and_then(|knowledge| knowledge.get("by_kind"))
            .cloned()
            .unwrap_or_else(|| json!({})),
        "state_change_refs": after_knowledge
            .and_then(|knowledge| knowledge.get("state_change_refs"))
            .cloned()
            .unwrap_or_else(|| json!([])),
        "runtime_policy_promotion_summary": after_knowledge
            .and_then(|knowledge| knowledge.get("runtime_policy_promotion_summary"))
            .cloned()
            .unwrap_or(Value::Null),
        "calibration_job_summary": after_knowledge
            .and_then(|knowledge| knowledge.get("calibration_job_summary"))
            .cloned()
            .unwrap_or(Value::Null),
        "unresolved_gaps": after_knowledge
            .and_then(|knowledge| knowledge.get("unresolved_gaps"))
            .cloned()
            .unwrap_or(Value::Null),
    })
}

fn source_hash_map(summary: Option<&Value>) -> BTreeMap<String, String> {
    summary
        .and_then(|summary| summary.get("source_hashes"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some((
                item.get("source_id")?.as_str()?.to_string(),
                item.get("source_hash")?.as_str()?.to_string(),
            ))
        })
        .collect()
}

fn eval_quality_summary_diff(before: Option<&Value>, after: &Value) -> Value {
    let mut metric_diff = serde_json::Map::new();
    for key in [
        "quality_report_coverage",
        "quality_report_production_ready",
        "eval_case_classification",
        "expected_evidence_hit_at_8",
        "required_type_coverage",
        "exact_term_coverage",
        "source_boundary_confirmation_avoided",
        "forbidden_conclusion_avoided",
        "reviewer_status_matched",
        "knowledge_state_quality",
    ] {
        metric_diff.insert(
            key.to_string(),
            json!({
                "before": before.and_then(|summary| summary.get(key)).cloned(),
                "after": after.get(key).cloned(),
            }),
        );
    }
    json!({
        "object": "tonglingyu.eval_quality_summary_diff",
        "schema_version": KB_VERSION_DIFF_REPORT_SCHEMA_VERSION,
        "before_status": before.and_then(|summary| summary["status"].as_str()),
        "after_status": after["status"].as_str(),
        "before_blockers": before
            .and_then(|summary| summary.get("blockers"))
            .cloned()
            .unwrap_or(Value::Null),
        "after_blockers": after.get("blockers").cloned().unwrap_or(Value::Null),
        "metrics": Value::Object(metric_diff),
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let data = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&data).with_context(|| format!("parse {}", path.display()))
}

fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>> {
    let file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut records = Vec::new();
    for (line_no, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str(&line)
            .with_context(|| format!("parse {}:{}", path.display(), line_no + 1))?;
        records.push(record);
    }
    Ok(records)
}

fn hash_files<'a>(paths: impl IntoIterator<Item = &'a std::path::PathBuf>) -> Result<String> {
    let mut hasher = Sha256::new();
    for path in paths {
        hasher.update(path.display().to_string().as_bytes());
        hasher.update(fs::read(path).with_context(|| format!("hash {}", path.display()))?);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_text(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn hash_json(input: &Value) -> String {
    hash_text(&serde_json::to_string(input).unwrap_or_else(|_| "null".to_string()))
}

fn table_count(conn: &Connection, table: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(Into::into)
}

fn grouped_count_map(conn: &Connection, sql: &str) -> Result<BTreeMap<String, i64>> {
    let mut map = BTreeMap::new();
    for (key, count) in grouped_count_pairs(conn, sql)? {
        map.insert(key, count);
    }
    Ok(map)
}

fn grouped_count_pairs(conn: &Connection, sql: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(sql)?;
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?
    .collect::<std::result::Result<Vec<_>, _>>()
    .map_err(Into::into)
}

fn load_rows_json(conn: &Connection, sql: &str, trace_id: &str) -> Result<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let column_names = stmt
        .column_names()
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let rows = stmt.query_map(params![trace_id], |row| {
        let mut object = serde_json::Map::new();
        for (index, name) in column_names.iter().enumerate() {
            let value: Option<String> = row.get(index)?;
            if name.ends_with("_json") {
                object.insert(
                    name.trim_end_matches("_json").to_string(),
                    value
                        .as_deref()
                        .and_then(|item| serde_json::from_str::<Value>(item).ok())
                        .unwrap_or(Value::Null),
                );
            } else {
                object.insert(
                    name.clone(),
                    value.map(Value::String).unwrap_or(Value::Null),
                );
            }
        }
        Ok(Value::Object(object))
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn create_evidence_package(
    conn: &Connection,
    trace_id: &str,
    question: &str,
    cards: Vec<EvidenceCard>,
) -> Result<EvidencePackage> {
    let claims = claims_from_cards(question, &cards);
    let knowledge_policy = runtime_knowledge_policy_index(conn, &cards)?;
    let claim_evidence_map =
        claim_evidence_map_with_knowledge(&claims, &cards, &knowledge_policy.refs_by_evidence_id);
    let mut review = review(question, &cards, &claims);
    apply_knowledge_state_review(&mut review, &knowledge_policy.summary);
    let package_id = format!("pkg-{}", uuid::Uuid::now_v7().simple());
    let now = now_rfc3339();
    let evidence_ids: Vec<_> = cards.iter().map(|card| card.evidence_id.clone()).collect();
    conn.execute(
        "INSERT INTO evidence_packages (package_id, trace_id, question, claim_statements_json, evidence_ids_json, review_status, review_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            package_id,
            trace_id,
            question,
            serde_json::to_string(&claims)?,
            serde_json::to_string(&evidence_ids)?,
            review.status,
            serde_json::to_string(&review)?,
            now,
        ],
    )?;
    for card in &cards {
        conn.execute(
            "INSERT INTO evidence_cards (evidence_id, package_id, evidence_type, source_id, block_id, support_scope, unsupported_scope, evidence_level, confidence, verification_status, evidence_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                card.evidence_id,
                package_id,
                card.evidence_type,
                card.source_id,
                card.block_id,
                card.support_scope,
                card.unsupported_scope,
                card.evidence_level,
                card.confidence,
                card.verification_status,
                serde_json::to_string(card)?,
                now,
            ],
        )?;
    }
    conn.execute(
        "INSERT INTO review_records (review_id, package_id, status, severity, issues_json, summary, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            format!("review-{}", uuid::Uuid::now_v7().simple()),
            package_id,
            review.status,
            review.severity,
            serde_json::to_string(&review.issues)?,
            review.summary,
            now,
        ],
    )?;
    for item in &claim_evidence_map {
        for evidence_id in &item.evidence_ids {
            conn.execute(
                "INSERT INTO evidence_claim_links (package_id, claim_index, evidence_id, support_relation) VALUES (?1, ?2, ?3, ?4)",
                params![package_id, item.claim_index as i64, evidence_id, "supports_scope_limited_claim"],
            )?;
        }
        for knowledge_ref in &item.knowledge_item_refs {
            conn.execute(
                r#"
                INSERT INTO evidence_claim_knowledge_links (
                    package_id, claim_index, evidence_id, item_id, state,
                    policy_version, policy_decision, calibration_report_ref,
                    display_label, created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    &package_id,
                    item.claim_index as i64,
                    &knowledge_ref.evidence_ref,
                    &knowledge_ref.item_id,
                    knowledge_ref.state.as_str(),
                    &knowledge_ref.policy_version,
                    &knowledge_ref.policy_decision,
                    &knowledge_ref.calibration_report_ref,
                    &knowledge_ref.display_label,
                    &now,
                ],
            )?;
        }
    }
    append_runtime_audit_event(
        conn,
        trace_id,
        "evidence_package_created",
        &json!({
            "package_id": &package_id,
            "question": question,
            "evidence_count": evidence_ids.len(),
            "evidence_ids": &evidence_ids,
            "claim_evidence_map": &claim_evidence_map,
            "knowledge_state_summary": &knowledge_policy.summary,
        }),
    )?;
    append_runtime_audit_event(
        conn,
        trace_id,
        "review_completed",
        &json!({
            "package_id": &package_id,
            "status": &review.status,
            "severity": &review.severity,
            "issues": &review.issues,
            "summary": &review.summary,
            "knowledge_state_summary": &knowledge_policy.summary,
        }),
    )?;
    Ok(EvidencePackage {
        package_id,
        trace_id: trace_id.to_string(),
        question: question.to_string(),
        cards,
        claims,
        claim_evidence_map,
        knowledge_state_summary: knowledge_policy.summary,
        review,
    })
}

pub fn load_evidence_package(db: &Path, package_id: &str) -> Result<Option<EvidencePackage>> {
    let conn = Connection::open(db)?;
    load_evidence_package_from_conn(&conn, package_id)
}

pub fn load_evidence_package_from_conn(
    conn: &Connection,
    package_id: &str,
) -> Result<Option<EvidencePackage>> {
    let package: Option<(String, String, String, String, String, String)> = conn
        .query_row(
            "SELECT package_id, trace_id, question, claim_statements_json, evidence_ids_json, review_json FROM evidence_packages WHERE package_id = ?1",
            params![package_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .optional()?;
    let Some((package_id, trace_id, question, claims_json, evidence_ids_json, review_json)) =
        package
    else {
        return Ok(None);
    };
    let evidence_ids: Vec<String> = serde_json::from_str(&evidence_ids_json)?;
    let mut stmt = conn
        .prepare("SELECT evidence_id, evidence_json FROM evidence_cards WHERE package_id = ?1")?;
    let mut cards_by_id = BTreeMap::new();
    for row in stmt.query_map(params![&package_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (evidence_id, evidence_json) = row?;
        cards_by_id.insert(
            evidence_id,
            serde_json::from_str::<EvidenceCard>(&evidence_json)?,
        );
    }
    let mut cards = Vec::new();
    for evidence_id in &evidence_ids {
        let card = cards_by_id.remove(evidence_id).ok_or_else(|| {
            anyhow!(
                "evidence package {} is missing stored card {}",
                package_id,
                evidence_id
            )
        })?;
        cards.push(card);
    }
    if let Some(extra_id) = cards_by_id.keys().next() {
        return Err(anyhow!(
            "evidence package {} has unstated stored card {}",
            package_id,
            extra_id
        ));
    }
    let claims: Vec<String> = serde_json::from_str(&claims_json)?;
    let mut claim_evidence_ids: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut link_stmt = conn.prepare(
        "SELECT claim_index, evidence_id FROM evidence_claim_links WHERE package_id = ?1 ORDER BY claim_index, evidence_id",
    )?;
    for row in link_stmt.query_map(params![&package_id], |row| {
        Ok((row.get::<_, i64>(0)? as usize, row.get::<_, String>(1)?))
    })? {
        let (claim_index, evidence_id) = row?;
        claim_evidence_ids
            .entry(claim_index)
            .or_default()
            .push(evidence_id);
    }
    let mut claim_knowledge_refs: BTreeMap<usize, Vec<ClaimKnowledgeItemRef>> = BTreeMap::new();
    if sqlite_table_exists(conn, "evidence_claim_knowledge_links")? {
        let mut stmt = conn.prepare(
            r#"
            SELECT claim_index, evidence_id, item_id, state, policy_version,
                   policy_decision, calibration_report_ref, display_label
            FROM evidence_claim_knowledge_links
            WHERE package_id = ?1
            ORDER BY claim_index, evidence_id, item_id
            "#,
        )?;
        for row in stmt.query_map(params![&package_id], |row| {
            Ok((
                row.get::<_, i64>(0)? as usize,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })? {
            let (
                claim_index,
                evidence_ref,
                item_id,
                state,
                policy_version,
                policy_decision,
                calibration_report_ref,
                display_label,
            ) = row?;
            claim_knowledge_refs
                .entry(claim_index)
                .or_default()
                .push(ClaimKnowledgeItemRef {
                    item_id,
                    state: KnowledgeState::parse(&state)?,
                    evidence_ref,
                    policy_version,
                    policy_decision,
                    calibration_report_ref,
                    display_label,
                });
        }
    }
    let claim_evidence_map = if claim_evidence_ids.is_empty() {
        claim_evidence_map(&claims, &cards)
    } else {
        claims
            .iter()
            .enumerate()
            .map(|(claim_index, claim)| ClaimEvidenceMap {
                claim_index,
                claim: claim.clone(),
                evidence_ids: claim_evidence_ids.remove(&claim_index).unwrap_or_default(),
                knowledge_item_refs: claim_knowledge_refs
                    .remove(&claim_index)
                    .unwrap_or_default(),
                forbidden_conclusions: cards
                    .iter()
                    .map(|card| card.unsupported_scope.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            })
            .collect()
    };
    let knowledge_state_summary = knowledge_state_summary_from_claim_maps(&claim_evidence_map);
    Ok(Some(EvidencePackage {
        package_id,
        trace_id,
        question,
        cards,
        claims,
        claim_evidence_map,
        knowledge_state_summary,
        review: serde_json::from_str(&review_json)?,
    }))
}

pub fn latest_evidence_package_from_conn(conn: &Connection) -> Result<Option<EvidencePackage>> {
    let package_id = conn
        .query_row(
            "SELECT package_id FROM evidence_packages ORDER BY created_at DESC, package_id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    match package_id {
        Some(package_id) => load_evidence_package_from_conn(conn, &package_id),
        None => Ok(None),
    }
}

pub fn search_evidence(
    conn: &Connection,
    question: &str,
    limit: usize,
    required_evidence_types: &[String],
) -> Result<Vec<EvidenceCard>> {
    Ok(search_evidence_result(conn, question, limit, required_evidence_types)?.cards)
}

fn search_evidence_result(
    conn: &Connection,
    question: &str,
    limit: usize,
    required_evidence_types: &[String],
) -> Result<SearchEvidenceResult> {
    let extracted_query_terms = extract_query_terms(conn, question)?;
    let terms = extracted_query_terms.terms;
    let exact_terms = required_exact_terms(question)?;
    let mut scored: BTreeMap<String, (i64, EvidenceCard)> = BTreeMap::new();
    let mut candidate_block_ids = BTreeSet::new();
    let mut match_channel_blocks = BTreeMap::<String, BTreeSet<String>>::new();
    for term in &terms {
        for block in query_blocks_like(conn, term, limit * 4)? {
            candidate_block_ids.insert(block.block_id.clone());
            record_match_channels(&mut match_channel_blocks, &block, term);
            let score = score_block(question, term, &block);
            let card = evidence_card_from_block_with_focus(block, term);
            scored
                .entry(card.block_id.clone())
                .and_modify(|(existing, _)| *existing += score)
                .or_insert((score, card));
        }
    }
    if scored.is_empty() {
        for block in query_blocks_like(conn, question, limit * 2)? {
            candidate_block_ids.insert(block.block_id.clone());
            record_match_channels(&mut match_channel_blocks, &block, question);
            let card = evidence_card_from_block_with_focus(block, question);
            scored.insert(card.block_id.clone(), (1, card));
        }
    }
    let mut ranked: Vec<_> = scored.into_values().collect();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.block_id.cmp(&right.1.block_id))
    });
    ranked.truncate(limit);
    let ranked_cards = ranked.into_iter().map(|(_, card)| card).collect::<Vec<_>>();
    let mut exact_cards = Vec::new();
    let mut exact_seen = HashSet::new();
    for exact_term in &exact_terms {
        for block in query_blocks_exact_text(conn, exact_term, limit * 8)? {
            if !block.text.contains(exact_term)
                && !block.normalized_text.contains(&normalize_text(exact_term))
            {
                continue;
            }
            candidate_block_ids.insert(block.block_id.clone());
            record_match_channels(&mut match_channel_blocks, &block, exact_term);
            let card = evidence_card_from_block_with_focus(block, exact_term);
            if exact_seen.insert(card.block_id.clone()) {
                exact_cards.push(card);
                break;
            }
        }
    }
    let protected_count = exact_cards.len();
    let mut seen = exact_seen;
    let mut cards = exact_cards;
    cards.extend(
        ranked_cards
            .into_iter()
            .filter(|card| seen.insert(card.block_id.clone())),
    );
    for required_type in required_evidence_types {
        if cards
            .iter()
            .any(|card| card.evidence_type == required_type.as_str())
        {
            continue;
        }
        for term in &terms {
            for block in query_blocks_like(conn, term, limit * 8)? {
                candidate_block_ids.insert(block.block_id.clone());
                record_match_channels(&mut match_channel_blocks, &block, term);
                let card = evidence_card_from_block(block);
                if card.evidence_type == *required_type && seen.insert(card.block_id.clone()) {
                    cards.insert(0, card);
                    break;
                }
            }
            if cards
                .iter()
                .any(|card| card.evidence_type == required_type.as_str())
            {
                break;
            }
        }
    }
    cards.truncate(
        limit
            .max(required_evidence_types.len())
            .max(protected_count),
    );
    let match_channel_counts = match_channel_blocks
        .into_iter()
        .map(|(channel, block_ids)| (channel, block_ids.len()))
        .collect();
    Ok(SearchEvidenceResult {
        cards,
        expanded_terms: terms,
        expanded_aliases: extracted_query_terms.aliases,
        match_channel_counts,
        exact_terms,
        candidate_count: candidate_block_ids.len(),
    })
}

fn record_match_channels(
    channels: &mut BTreeMap<String, BTreeSet<String>>,
    block: &SearchBlockRecord,
    term: &str,
) {
    for channel in block_match_channels(block, term) {
        channels
            .entry(channel)
            .or_default()
            .insert(block.block_id.clone());
    }
}

fn block_match_channels(block: &SearchBlockRecord, term: &str) -> Vec<String> {
    let normalized_term = normalize_text(term);
    let mut channels = Vec::new();
    if block.text.contains(term) {
        channels.push("raw_text".to_string());
    }
    if block.source_title.contains(term) {
        channels.push("raw_source_title".to_string());
    }
    if block.normalized_text.contains(&normalized_term) {
        channels.push("normalized_text".to_string());
    }
    if block.normalized_source_title.contains(&normalized_term) {
        channels.push("normalized_source_title".to_string());
    }
    channels
}

fn text_search_required_evidence_types(required_evidence_types: &[String]) -> Vec<String> {
    required_evidence_types
        .iter()
        .filter(|item| item.as_str() != "commentary")
        .cloned()
        .collect()
}

fn retrieval_quality_report(
    conn: &Connection,
    tool_name: &str,
    question: &str,
    required_evidence_types: &[String],
    search: &SearchEvidenceResult,
) -> Result<RetrievalQualityReport> {
    let mut redacted_term_set = redacted_terms_from_question(question)
        .into_iter()
        .collect::<BTreeSet<_>>();
    for term in &search.expanded_terms {
        if redacted_term_set.len() >= RETRIEVAL_QUALITY_REPORT_MAX_TERMS {
            break;
        }
        redacted_term_set.insert(redacted_query_term(term));
    }
    let redacted_terms = redacted_term_set.into_iter().collect::<Vec<_>>();
    let protected_terms = search
        .exact_terms
        .iter()
        .map(|term| redacted_query_term(term))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let expanded_aliases = search
        .expanded_aliases
        .iter()
        .take(RETRIEVAL_QUALITY_REPORT_MAX_TERMS)
        .map(|term| redacted_query_term(term))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let evidence_type_coverage =
        retrieval_evidence_type_coverage(required_evidence_types, &search.cards);
    let exact_match_coverage = retrieval_exact_match_coverage(&search.exact_terms, &search.cards);
    let source_usage_refs_all = retrieval_source_usage_refs(conn, &search.cards)?;
    let source_usage_refs = source_usage_refs_all
        .iter()
        .take(RETRIEVAL_QUALITY_REPORT_MAX_SOURCE_REFS)
        .cloned()
        .collect::<Vec<_>>();
    let source_coverage_boundary =
        retrieval_source_coverage_boundary(&search.cards, &source_usage_refs_all);
    let issues = retrieval_quality_issues(
        search,
        &evidence_type_coverage,
        &exact_match_coverage,
        &source_usage_refs_all,
    );
    let blocking_quality_issue = issues.iter().any(|issue| {
        issue == "no_evidence_selected"
            || issue.starts_with("missing_required_evidence_type:")
            || issue.starts_with("required_exact_term_not_selected:")
    });
    let quality_status = if blocking_quality_issue {
        "failed"
    } else if issues.is_empty() {
        "passed"
    } else {
        "needs_attention"
    };
    Ok(RetrievalQualityReport {
        object: "tonglingyu.retrieval_quality_report".to_string(),
        schema_version: RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION.to_string(),
        tool_name: tool_name.to_string(),
        quality_status: quality_status.to_string(),
        production_ready: issues.is_empty(),
        truncated: search.expanded_terms.len() > RETRIEVAL_QUALITY_REPORT_MAX_TERMS
            || search.expanded_aliases.len() > RETRIEVAL_QUALITY_REPORT_MAX_TERMS
            || source_usage_refs_all.len() > RETRIEVAL_QUALITY_REPORT_MAX_SOURCE_REFS,
        query_summary: RetrievalQuerySummary {
            question_sha256: hash_text(question),
            question_char_count: question.chars().count(),
            raw_question_included: false,
            redacted_terms: redacted_terms.clone(),
        },
        expanded_terms: redacted_terms,
        protected_terms,
        expanded_aliases,
        normalized_match_channels: search.match_channel_counts.clone(),
        candidate_count: search.candidate_count,
        selected_count: search.cards.len(),
        channel_distribution: retrieval_channel_distribution(&search.cards),
        evidence_type_coverage,
        exact_match_coverage,
        expected_evidence_hit: None,
        expected_evidence_status: "not_applicable_runtime_search".to_string(),
        source_coverage_boundary,
        source_usage_refs,
        issues: issues.clone(),
        recommended_follow_up: retrieval_recommended_follow_up(&issues),
    })
}

fn retrieval_evidence_type_coverage(
    required_evidence_types: &[String],
    cards: &[EvidenceCard],
) -> RetrievalEvidenceTypeCoverage {
    let required = required_evidence_types
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let selected = evidence_types(cards);
    let selected_set = selected.iter().cloned().collect::<BTreeSet<_>>();
    let missing = required
        .iter()
        .filter(|required_type| !selected_set.contains(*required_type))
        .cloned()
        .collect::<Vec<_>>();
    RetrievalEvidenceTypeCoverage {
        required,
        selected,
        missing,
    }
}

fn retrieval_exact_match_coverage(
    exact_terms: &[String],
    cards: &[EvidenceCard],
) -> Vec<RetrievalExactMatchCoverage> {
    exact_terms
        .iter()
        .map(|term| {
            let normalized_term = normalize_text(term);
            let evidence_ids = cards
                .iter()
                .filter(|card| {
                    card.text.contains(term)
                        || normalize_text(&card.text).contains(&normalized_term)
                })
                .map(|card| card.evidence_id.clone())
                .collect::<Vec<_>>();
            RetrievalExactMatchCoverage {
                term: term.clone(),
                matched: !evidence_ids.is_empty(),
                evidence_ids,
            }
        })
        .collect()
}

fn retrieval_channel_distribution(cards: &[EvidenceCard]) -> BTreeMap<String, usize> {
    let mut distribution = BTreeMap::new();
    for card in cards {
        *distribution.entry(card.evidence_type.clone()).or_insert(0) += 1;
    }
    distribution
}

fn retrieval_source_usage_refs(
    conn: &Connection,
    cards: &[EvidenceCard],
) -> Result<Vec<RetrievalSourceUsageRef>> {
    let mut refs = Vec::new();
    for source_id in cards
        .iter()
        .map(|card| card.source_id.clone())
        .collect::<BTreeSet<_>>()
    {
        refs.push(retrieval_source_usage_ref(conn, &source_id)?);
    }
    Ok(refs)
}

fn retrieval_source_usage_ref(
    conn: &Connection,
    source_id: &str,
) -> Result<RetrievalSourceUsageRef> {
    let row = conn
        .query_row(
            r#"
            SELECT
                s.source_category,
                s.title,
                s.edition,
                s.source_url,
                s.fetched_at,
                s.snapshot_contract_json,
                s.source_hash,
                s.license,
                s.license_url,
                s.license_source_url,
                s.attribution,
                s.usage_boundary,
                vn.usage_limit
            FROM sources s
            LEFT JOIN version_notes vn ON vn.source_id = s.source_id
            WHERE s.source_id = ?1
            LIMIT 1
            "#,
            params![source_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            },
        )
        .optional()?;
    let Some((
        source_category,
        title,
        edition,
        source_url,
        fetched_at,
        snapshot_contract_json,
        source_hash,
        source_license,
        source_license_url,
        source_license_source_url,
        source_attribution,
        source_usage_boundary,
        usage_limit,
    )) = row
    else {
        return Ok(RetrievalSourceUsageRef {
            source_id: source_id.to_string(),
            source_category: None,
            title: None,
            edition: None,
            source_url: None,
            fetched_at: None,
            source_hash: None,
            license: None,
            license_url: None,
            license_source_url: None,
            attribution: None,
            usage_boundary: "source metadata missing; cannot enter production evidence chain"
                .to_string(),
            metadata_status: "missing_source_metadata".to_string(),
        });
    };
    let snapshot_contract =
        serde_json::from_str::<Value>(&snapshot_contract_json).unwrap_or_else(|_| json!({}));
    let license = source_license.or_else(|| {
        snapshot_text_field(
            &snapshot_contract,
            &["license", "license_id", "license_note", "licence", "rights"],
        )
    });
    let license_url = source_license_url.or_else(|| {
        snapshot_text_field(
            &snapshot_contract,
            &["license_url", "license_uri", "rights_url"],
        )
    });
    let license_source_url = source_license_source_url.or_else(|| {
        snapshot_text_field(
            &snapshot_contract,
            &[
                "license_source_url",
                "rights_source_url",
                "copyright_policy_url",
            ],
        )
    });
    let attribution = source_attribution.or_else(|| {
        snapshot_text_field(
            &snapshot_contract,
            &["attribution", "attribution_note", "citation"],
        )
    });
    let metadata_usage_boundary = source_usage_boundary.or_else(|| {
        snapshot_text_field(
            &snapshot_contract,
            &["usage_boundary", "usage_limit", "source_usage_boundary"],
        )
    });
    let mut missing = Vec::new();
    if license.is_none() {
        missing.push("license");
    }
    if license_url.is_none() {
        missing.push("license_url");
    }
    if attribution.is_none() {
        missing.push("attribution");
    }
    if metadata_usage_boundary.is_none() {
        missing.push("usage_boundary");
    }
    let metadata_status = if missing.is_empty() {
        "complete".to_string()
    } else {
        format!("missing_{}_metadata", missing.join("_and_"))
    };
    Ok(RetrievalSourceUsageRef {
        source_id: source_id.to_string(),
        source_category: Some(source_category),
        title,
        edition,
        source_url,
        fetched_at,
        source_hash: Some(source_hash),
        license,
        license_url,
        license_source_url,
        attribution,
        usage_boundary: metadata_usage_boundary.or(usage_limit).unwrap_or_else(|| {
            retrieval_rules::usage_limit_for_source_id(source_id)
                .expect("retrieval usage limit rules must load")
        }),
        metadata_status,
    })
}

fn snapshot_text_field(snapshot_contract: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| {
            snapshot_contract
                .get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            keys.iter().find_map(|key| {
                snapshot_contract
                    .get("metadata")
                    .and_then(|metadata| metadata.get(*key))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
        })
}

fn retrieval_source_coverage_boundary(
    cards: &[EvidenceCard],
    refs: &[RetrievalSourceUsageRef],
) -> RetrievalSourceCoverageBoundary {
    RetrievalSourceCoverageBoundary {
        source_ids: cards
            .iter()
            .map(|card| card.source_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        source_categories: refs
            .iter()
            .filter_map(|item| item.source_category.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        edition_boundaries: refs
            .iter()
            .filter_map(|item| item.edition.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        kb_schema_version: KNOWLEDGE_BASE_SCHEMA_VERSION.to_string(),
        source_snapshot_status: if refs
            .iter()
            .any(|item| item.metadata_status == "missing_source_metadata")
        {
            "source_snapshot_metadata_missing".to_string()
        } else if cards.is_empty() {
            "no_source_selected".to_string()
        } else {
            "source_snapshot_ready".to_string()
        },
        facsimile_review_status: "not_reviewed".to_string(),
        authoritative_edition_review_status: "not_reviewed".to_string(),
        scholarly_collation_status: "not_scholarly_collated".to_string(),
        expert_collation_status: "not_reviewed".to_string(),
    }
}

fn retrieval_quality_issues(
    search: &SearchEvidenceResult,
    evidence_type_coverage: &RetrievalEvidenceTypeCoverage,
    exact_match_coverage: &[RetrievalExactMatchCoverage],
    source_usage_refs: &[RetrievalSourceUsageRef],
) -> Vec<String> {
    let mut issues = Vec::new();
    if search.cards.is_empty() {
        issues.push("no_evidence_selected".to_string());
    }
    for missing in &evidence_type_coverage.missing {
        issues.push(format!("missing_required_evidence_type:{missing}"));
    }
    for coverage in exact_match_coverage {
        if !coverage.matched {
            issues.push(format!(
                "required_exact_term_not_selected:{}",
                redacted_query_term(&coverage.term)
            ));
        }
    }
    for source_ref in source_usage_refs {
        if source_ref.metadata_status != "complete" {
            issues.push(format!(
                "source_usage_metadata_incomplete:{}:{}",
                source_ref.source_id, source_ref.metadata_status
            ));
        }
    }
    issues
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn retrieval_recommended_follow_up(issues: &[String]) -> Vec<String> {
    let mut follow_up = BTreeSet::new();
    for issue in issues {
        if issue == "no_evidence_selected" {
            follow_up.insert("expand_source_snapshot_or_return_evidence_insufficient".to_string());
        } else if issue.starts_with("missing_required_evidence_type:") {
            follow_up.insert("add_or_reindex_required_evidence_type".to_string());
        } else if issue.starts_with("required_exact_term_not_selected:") {
            follow_up.insert("verify_exact_term_source_snapshot_and_alias_index".to_string());
        } else if issue.starts_with("source_usage_metadata_incomplete:") {
            follow_up.insert(
                "add_machine_readable_source_license_usage_attribution_metadata".to_string(),
            );
        }
    }
    follow_up.into_iter().collect()
}

fn redacted_query_term(term: &str) -> String {
    let trimmed = term.trim();
    if trimmed.is_empty() {
        return "[empty]".to_string();
    }
    if looks_sensitive_query_term(trimmed) {
        return format!("sha256:{}", &hash_text(trimmed)[..12]);
    }
    trim_text(trimmed, 32)
}

fn looks_sensitive_query_term(term: &str) -> bool {
    let lower = term.to_ascii_lowercase();
    if lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("api-key")
        || lower.contains("access_key")
        || lower.contains("access-key")
        || lower.contains("key=")
        || lower.contains("credential")
        || lower.contains("sk-")
        || term.contains('@')
    {
        return true;
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return true;
    }
    let digit_count = term.chars().filter(|ch| ch.is_ascii_digit()).count();
    if digit_count >= 10
        && term
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '-' | '(' | ')'))
    {
        return true;
    }
    let ascii_alnum = term.chars().filter(|ch| ch.is_ascii_alphanumeric()).count();
    ascii_alnum >= 20 && ascii_alnum * 2 >= term.chars().count()
}

pub fn package_json(package: &EvidencePackage) -> Value {
    let evidence_ids: Vec<_> = package
        .cards
        .iter()
        .map(|card| card.evidence_id.as_str())
        .collect();
    json!({
        "package_id": &package.package_id,
        "trace_id": &package.trace_id,
        "question": &package.question,
        "claims": &package.claims,
        "claim_evidence_map": public_claim_evidence_map(&package.claim_evidence_map),
        "knowledge_state_summary": public_knowledge_state_summary(&package.knowledge_state_summary),
        "evidence_ids": evidence_ids,
        "cards": &package.cards,
        "review": public_review_record(&package.review),
    })
}

fn upstream_evidence_brief(question: &str, cards: &[EvidenceCard]) -> Vec<Value> {
    let focus_terms = upstream_evidence_focus_terms(question);
    let mut indexed_cards = cards.iter().enumerate().collect::<Vec<_>>();
    indexed_cards.sort_by_key(|(index, card)| {
        (
            std::cmp::Reverse(matched_evidence_focus_terms(card, &focus_terms).len()),
            *index,
        )
    });
    indexed_cards
        .iter()
        .take(UPSTREAM_EVIDENCE_BRIEF_CARD_LIMIT)
        .map(|card| {
            let card = card.1;
            let text_excerpt = upstream_evidence_text_excerpt(card, &focus_terms);
            let matched_terms = matched_evidence_focus_terms(card, &focus_terms)
                .into_iter()
                .map(|term| trim_text(&term, UPSTREAM_EVIDENCE_BRIEF_MATCHED_TERM_CHARS))
                .take(UPSTREAM_EVIDENCE_BRIEF_MATCHED_TERM_LIMIT)
                .collect::<Vec<_>>();
            let evidence_slots = matched_query_expansion_evidence_slot_ids_for_card(question, card)
                .unwrap_or_default();
            let evidence_slot_rules =
                evidence_slot_rule_values_for_ids(&evidence_slots).unwrap_or_default();
            json!({
                "evidence_id": &card.evidence_id,
                "evidence_type": &card.evidence_type,
                "source_layer": evidence_card_source_layer(card),
                "source_title": trim_text(&card.source_title, UPSTREAM_EVIDENCE_BRIEF_SOURCE_TITLE_CHARS),
                "text": text_excerpt,
                "text_is_excerpt": true,
                "matched_terms": matched_terms,
                "evidence_slots": evidence_slots,
                "evidence_slot_rules": evidence_slot_rules,
                "support_scope": trim_text(&card.support_scope, UPSTREAM_EVIDENCE_BRIEF_SCOPE_CHARS),
                "unsupported_scope": trim_text(&card.unsupported_scope, UPSTREAM_EVIDENCE_BRIEF_LIMITS_CHARS),
            })
        })
        .collect()
}

fn upstream_evidence_focus_terms(question: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let normalized = normalize_query(question);
    if let Ok(catalog) = query_expansion_catalog() {
        apply_query_expansion_exact_terms(&catalog, question, &normalized, &mut terms);
        apply_query_expansion_evidence_slot_terms(&catalog, question, &normalized, &mut terms);
    }
    for token in cjk_tokens(question) {
        if token.chars().count() >= 2 && token.chars().count() <= 10 {
            push_term(&mut terms, &token);
        }
        for focus_term in cjk_focus_terms(&token) {
            if focus_term.chars().count() >= 2 && focus_term.chars().count() <= 10 {
                push_term(&mut terms, &focus_term);
            }
        }
    }
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    terms
}

fn upstream_evidence_text_excerpt(card: &EvidenceCard, focus_terms: &[String]) -> String {
    for term in focus_terms {
        if evidence_text_contains_focus(&card.text, term) {
            return trim_text_around(&card.text, term, UPSTREAM_EVIDENCE_BRIEF_TEXT_CHARS);
        }
    }
    trim_text(&card.text, UPSTREAM_EVIDENCE_BRIEF_TEXT_CHARS)
}

fn evidence_text_contains_focus(text: &str, focus: &str) -> bool {
    text.contains(focus) || normalize_text(text).contains(&normalize_text(focus))
}

fn matched_evidence_focus_terms(card: &EvidenceCard, focus_terms: &[String]) -> Vec<String> {
    let mut matched = Vec::new();
    for term in focus_terms {
        if evidence_text_contains_focus(&card.text, term) {
            push_term(&mut matched, term);
        }
    }
    matched
}

fn public_claim_evidence_map(claim_evidence_map: &[ClaimEvidenceMap]) -> Vec<Value> {
    claim_evidence_map
        .iter()
        .map(|claim| {
            let knowledge_state_labels = claim
                .knowledge_item_refs
                .iter()
                .filter_map(|item| item.display_label.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            json!({
                "claim_index": claim.claim_index,
                "claim": &claim.claim,
                "evidence_ids": &claim.evidence_ids,
                "knowledge_state_labels": knowledge_state_labels,
                "forbidden_conclusions": &claim.forbidden_conclusions,
            })
        })
        .collect()
}

fn public_knowledge_state_summary(summary: &KnowledgeStateSummary) -> Value {
    json!({
        "object": &summary.object,
        "selected_count": summary.selected_count,
        "registered_count": summary.runtime_usable_count,
        "human_reviewed_count": summary.human_marked_count,
        "safe_public_label": &summary.safe_public_label,
        "internal_governance_fields_included": false,
    })
}

fn public_review_record(review: &ReviewRecord) -> Value {
    let issues = review
        .issues
        .iter()
        .map(|issue| {
            if issue.contains("runtime_usable")
                || issue.contains("system_calibrated")
                || issue.contains("human_marked")
            {
                "部分知识条目尚未满足运行证据要求，已降级处理。".to_string()
            } else {
                issue.clone()
            }
        })
        .collect::<Vec<_>>();
    json!({
        "status": &review.status,
        "severity": &review.severity,
        "issues": issues,
        "summary": &review.summary,
    })
}

pub fn replay_package_json(package: &EvidencePackage) -> Value {
    json!({
        "object": "tonglingyu.evidence_package_replay",
        "package": package_json(package),
        "answer": replay_answer(package),
        "deterministic": true,
        "answer_source": "local_replay_no_upstream",
    })
}

pub fn replay_answer(package: &EvidencePackage) -> String {
    enforce_review(local_answer(&package.question, package), package)
}

pub fn claims_from_cards(question: &str, cards: &[EvidenceCard]) -> Vec<String> {
    let Ok(rules) = claim_rules() else {
        return vec!["治理规则目录不可用，不能给出确定结论。".to_string()];
    };
    if cards.is_empty() {
        return vec![rules.empty_evidence];
    }
    let mut claims = Vec::new();
    if matched_query_expansion_evidence_slots(question, cards).is_ok_and(|slots| !slots.is_empty())
    {
        claims.push(rules.slot_count_rule.clone());
        if active_count_basis_for_question(
            question,
            question_asks_for_count(question).unwrap_or(false),
        )
        .ok()
        .flatten()
        .is_some()
        {
            claims.push(rules.inactive_count_basis.clone());
        }
    }
    if cards_include_later_forty(cards) {
        claims.push(rules.later_forty_boundary.clone());
    }
    if cards.iter().any(|card| card.evidence_type == "commentary") {
        claims.push(rules.commentary_scope.clone());
    }
    if cards.iter().any(|card| card.evidence_type == "base_text") {
        claims.push(rules.base_text_scope.clone());
    }
    if claims.is_empty() {
        claims.push(rules.default_scope.clone());
    }
    claims
}

pub fn review(question: &str, cards: &[EvidenceCard], claims: &[String]) -> ReviewRecord {
    let mut issues = Vec::new();
    match blocked_prompt_control_issues(question) {
        Ok(control_issues) => issues.extend(control_issues),
        Err(error) => issues.push(format!("governance_rules_unavailable:{error}")),
    }
    if cards.is_empty() {
        issues.push(
            empty_evidence_review_issue()
                .unwrap_or_else(|error| format!("governance_rules_unavailable:{error}")),
        );
    }
    if later_forty_boundary_missing_from_claims(cards, claims) {
        issues.push(
            later_forty_boundary_review_issue()
                .unwrap_or_else(|error| format!("governance_rules_unavailable:{error}")),
        );
    }
    match triggered_review_rule_issues(question, cards, claims) {
        Ok(rule_issues) => issues.extend(rule_issues),
        Err(error) => issues.push(format!("governance_rules_unavailable:{error}")),
    }
    let status = if issues.is_empty() {
        "passed"
    } else {
        "needs_revision"
    };
    let severity = if cards.is_empty() {
        "high"
    } else if issues.is_empty() {
        "none"
    } else {
        "medium"
    };
    let summary = if issues.is_empty() {
        format!("reviewer 通过：{} 条结论声明均有证据包约束。", claims.len())
    } else {
        format!("reviewer 要求谨慎降级：{} 个问题。", issues.len())
    };
    ReviewRecord {
        status: status.to_string(),
        severity: severity.to_string(),
        issues,
        summary,
    }
}

pub fn local_answer(question: &str, package: &EvidencePackage) -> String {
    if package.cards.is_empty() {
        return "我暂时找不到足够的原文依据，不能可靠回答这个问题。".to_string();
    }
    if let Some(answer) = local_slot_count_answer(question, package) {
        return answer;
    }
    if let Some(answer) = preferred_evidence_answer(question, package) {
        return answer;
    }
    let mut answer = String::new();
    if package.knowledge_state_summary.human_marked_count > 0 {
        answer.push_str("人工标记资料显示，可以这样回答：\n\n");
    } else if package.knowledge_state_summary.runtime_usable_count > 0 {
        answer.push_str("基于当前已登记资料，可以这样回答：\n\n");
    } else {
        answer.push_str("根据目前可检索到的文本，可以这样回答：\n\n");
    }
    answer.push_str("目前能支持回答的主要材料如下，结论只限于这些文本直接能说明的范围。\n\n");
    if cards_include_later_forty(&package.cards) {
        answer.push_str(
            "注意：以下包含第八十一回及以后（后四十回）材料；这类材料必须显式标注为后四十回内容，未标注时不能作为证据或参考。\n\n",
        );
    }
    let display_cards = answer_display_cards(&package.question, &package.cards, 4);
    if display_cards.is_empty() {
        answer.push_str("当前命中的证据片段过短或不完整，不能可靠展示为回答依据。\n");
        return answer;
    }
    for (index, card) in display_cards.iter().enumerate() {
        answer.push_str(&format!(
            "{}. {}：{}\n",
            index + 1,
            evidence_card_source_label(card),
            answer_evidence_excerpt(&package.question, card)
        ));
    }
    answer
}

fn preferred_evidence_answer(question: &str, package: &EvidencePackage) -> Option<String> {
    let preferred_types = preferred_answer_evidence_types(question).ok()?;
    if preferred_types.is_empty() {
        return None;
    }
    let display_cards = answer_display_cards(&package.question, &package.cards, 2);
    let primary = display_cards.first()?;
    if !preferred_types
        .iter()
        .any(|item| item == &primary.evidence_type)
    {
        return None;
    }
    let evidence_label = evidence_card_layer_label(primary);
    let source_label = evidence_card_source_label(primary);
    let excerpt = answer_evidence_excerpt(&package.question, primary);
    if excerpt.trim().is_empty() {
        return None;
    }

    let mut answer = String::new();
    if cards_include_later_forty(&package.cards) {
        answer.push_str(
            "注意：以下包含第八十一回及以后（后四十回）材料；这类材料必须显式标注为后四十回内容，未标注时不能作为证据或参考。\n\n",
        );
    }
    answer.push_str(&format!(
        "有。{}里最直接可用的是 {}：{}。\n\n",
        evidence_label,
        source_label,
        quoted_excerpt(&excerpt)
    ));
    answer.push_str("用法上，");
    answer.push_str(&sentence_without_terminal_punctuation(
        &primary.support_scope,
    ));
    answer.push_str("；边界是：");
    answer.push_str(&sentence_without_terminal_punctuation(
        &primary.unsupported_scope,
    ));
    answer.push('。');
    Some(answer)
}

fn local_slot_count_answer(question: &str, package: &EvidencePackage) -> Option<String> {
    let source_scope_policy = source_scope_policy_for_question(question);
    if cards_include_later_forty(&package.cards) && !source_scope_policy.later_forty_allowed {
        return None;
    }
    let active_basis =
        active_count_basis_for_question(question, question_asks_for_count(question).ok()?)
            .ok()
            .flatten()?;
    let slot_matches = evidence_slot_matches_for_cards(question, &package.cards)
        .ok()
        .filter(|matches| !matches.is_empty())?;
    compose_slot_count_answer(package, &active_basis, &slot_matches)
}

fn answer_display_cards<'a>(
    question: &str,
    cards: &'a [EvidenceCard],
    limit: usize,
) -> Vec<&'a EvidenceCard> {
    let mut display_cards = Vec::new();
    let mut signatures = Vec::new();
    let preferred_types = preferred_answer_evidence_types(question).unwrap_or_default();
    let preferred_available = !preferred_types.is_empty()
        && cards.iter().any(|card| {
            preferred_types
                .iter()
                .any(|item| item == &card.evidence_type)
        });
    let focus_terms = upstream_evidence_focus_terms(question);
    let mut candidates = cards
        .iter()
        .filter(|card| {
            !preferred_available
                || preferred_types
                    .iter()
                    .any(|item| item == &card.evidence_type)
        })
        .collect::<Vec<_>>();
    let mut ranked_candidates = candidates
        .drain(..)
        .map(|card| (answer_card_rank(question, card, &focus_terms), card))
        .collect::<Vec<_>>();
    if preferred_available {
        ranked_candidates.sort_by_key(|(rank, _)| std::cmp::Reverse(*rank));
    }
    let best_rank = ranked_candidates
        .first()
        .map(|(rank, _)| *rank)
        .unwrap_or_default();
    for (rank, card) in ranked_candidates {
        if preferred_available && best_rank >= 20 && rank * 100 < best_rank * 50 {
            continue;
        }
        if !evidence_card_presentable_in_answer(card) {
            continue;
        }
        let signature = AnswerEvidenceSignature::from_card(card);
        if signatures
            .iter()
            .any(|existing| answer_evidence_duplicate(existing, &signature))
        {
            continue;
        }
        signatures.push(signature);
        display_cards.push(card);
        if display_cards.len() >= limit {
            break;
        }
    }
    display_cards
}

fn answer_card_rank(question: &str, card: &EvidenceCard, focus_terms: &[String]) -> i64 {
    let mut score = 0;
    let normalized = normalize_text(&card.text);
    for term in focus_terms {
        if !term.trim().is_empty() && evidence_text_contains_focus(&card.text, term) {
            score += 20 + term.chars().count() as i64;
        } else if !term.trim().is_empty() && normalized.contains(&normalize_text(term)) {
            score += 10;
        }
    }
    if card.evidence_type == "commentary" {
        score += 5;
    }
    if let Ok(ranking) = retrieval_rules::ranking_rules() {
        if retrieval_rules::contains_any_term(question, &ranking.commentary_question_terms)
            && retrieval_rules::contains_any_raw(
                &card.source_id,
                &ranking.commentary_source_id_terms,
            )
        {
            score += 15;
        }
        if retrieval_rules::contains_any_term(question, &ranking.fate_question_terms) {
            score += 12 * matching_rule_term_count(&card.text, &ranking.fate_text_terms) as i64;
        }
        for boost in ranking.version_source_boosts {
            if retrieval_rules::contains_any_term(question, &boost.question_terms)
                && retrieval_rules::contains_any_raw(&card.source_id, &boost.source_id_terms)
            {
                score += boost.score;
            }
        }
    }
    score
}

fn matching_rule_term_count(text: &str, terms: &[String]) -> usize {
    let normalized = normalize_text(text);
    terms
        .iter()
        .filter(|term| {
            let term = term.trim();
            !term.is_empty() && (text.contains(term) || normalized.contains(&normalize_text(term)))
        })
        .count()
}

fn answer_evidence_excerpt(question: &str, card: &EvidenceCard) -> String {
    let focus_terms = upstream_evidence_focus_terms(question);
    let raw_excerpt = upstream_evidence_text_excerpt(card, &focus_terms);
    let cleaned = public_quote_text(&raw_excerpt);
    trim_text(&cleaned, 180)
}

fn evidence_card_presentable_in_answer(card: &EvidenceCard) -> bool {
    !evidence_text_is_broken_shell(&card.text) && !evidence_text_is_navigation_index(&card.text)
}

#[derive(Debug)]
struct AnswerEvidenceSignature {
    namespace: String,
    compact_text: String,
    shingles: BTreeSet<String>,
}

impl AnswerEvidenceSignature {
    fn from_card(card: &EvidenceCard) -> Self {
        let namespace = normalize_text(card.evidence_type.trim());
        let compact_text = compact_evidence_text(&card.text);
        let shingles = text_shingles(&compact_text, 4);
        Self {
            namespace,
            compact_text,
            shingles,
        }
    }
}

fn answer_evidence_duplicate(
    existing: &AnswerEvidenceSignature,
    candidate: &AnswerEvidenceSignature,
) -> bool {
    if existing.namespace != candidate.namespace {
        return false;
    }
    if existing.compact_text == candidate.compact_text {
        return true;
    }
    let existing_len = existing.compact_text.chars().count();
    let candidate_len = candidate.compact_text.chars().count();
    let min_len = existing_len.min(candidate_len);
    if min_len < 24 {
        return false;
    }
    if existing.compact_text.contains(&candidate.compact_text)
        || candidate.compact_text.contains(&existing.compact_text)
    {
        return true;
    }
    let smaller_shingle_count = existing.shingles.len().min(candidate.shingles.len());
    if smaller_shingle_count < 12 {
        return false;
    }
    let shared = existing.shingles.intersection(&candidate.shingles).count();
    let required_overlap_percent = if min_len >= 160 {
        55
    } else if min_len >= 80 {
        70
    } else {
        82
    };
    shared * 100 >= smaller_shingle_count * required_overlap_percent
}

fn compact_evidence_text(text: &str) -> String {
    normalize_text(text)
        .chars()
        .filter(|ch| !ch.is_whitespace() && !text_punctuation(*ch))
        .collect()
}

fn text_shingles(text: &str, width: usize) -> BTreeSet<String> {
    let chars = text.chars().collect::<Vec<_>>();
    if width == 0 || chars.len() < width {
        return BTreeSet::new();
    }
    chars
        .windows(width)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn evidence_text_is_broken_shell(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let substantive_count = trimmed
        .chars()
        .filter(|ch| !ch.is_whitespace() && !text_punctuation(*ch))
        .count();
    if substantive_count == 0 {
        return true;
    }
    retrieval_rules::evidence_text_is_broken_shell(trimmed, substantive_count).unwrap_or(true)
}

fn evidence_text_is_navigation_index(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let wiki_subpage_link_count = trimmed.matches("[[/").count();
    let chapter_link_count = trimmed.matches("|第").count();
    let chapter_title_count = trimmed.matches('回').count();
    let section_separator_count = trimmed.matches("{{***}}").count();
    (wiki_subpage_link_count >= 3 && chapter_link_count >= 3 && chapter_title_count >= 6)
        || (section_separator_count >= 2
            && wiki_subpage_link_count >= 2
            && chapter_title_count >= 6)
}

fn text_punctuation(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '，' | '。'
                | '：'
                | '；'
                | '、'
                | '？'
                | '！'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '「'
                | '」'
                | '『'
                | '』'
                | '（'
                | '）'
                | '《'
                | '》'
                | '【'
                | '】'
                | '…'
        )
}

fn cards_include_later_forty(cards: &[EvidenceCard]) -> bool {
    cards.iter().any(evidence_card_is_later_forty)
}

fn evidence_card_source_label(card: &EvidenceCard) -> String {
    if evidence_card_is_later_forty(card) && !text_mentions_later_forty_boundary(&card.source_title)
    {
        format!("{}（后四十回）", card.source_title)
    } else {
        card.source_title.clone()
    }
}

fn evidence_card_layer_label(card: &EvidenceCard) -> String {
    retrieval_rules::source_layer_label(&evidence_card_source_layer(card))
        .unwrap_or_else(|_| card.evidence_level.clone())
}

fn quoted_excerpt(text: &str) -> String {
    format!("「{}」", sentence_without_terminal_punctuation(text))
}

fn sentence_without_terminal_punctuation(text: &str) -> String {
    text.trim()
        .trim_end_matches(['。', '；', ';', '.', '！', '!', '？', '?'])
        .to_string()
}

fn normalized_primary_focus(question: &str) -> Option<String> {
    let mut terms = Vec::new();
    for token in cjk_tokens(question) {
        let focus_terms = cjk_focus_terms(&token);
        let source_terms = if focus_terms.is_empty() {
            vec![token]
        } else {
            focus_terms
        };
        for term in source_terms {
            let normalized = normalize_query(&term);
            if normalized.chars().count() >= 2
                && normalized.chars().count() <= 8
                && !generic_question_term(&normalized)
            {
                push_term(&mut terms, &normalized);
            }
        }
    }
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    terms.into_iter().next()
}

fn generic_question_term(term: &str) -> bool {
    retrieval_rules::generic_question_term(term).unwrap_or(false)
}

pub fn enforce_review(draft: String, package: &EvidencePackage) -> String {
    if package.review.status == "passed" {
        return draft;
    }
    format!(
        "这个问题目前缺少足够证据支持：{}\n\n{}",
        package.review.issues.join("；"),
        local_answer(&package.question, package)
    )
}

struct RuntimeKnowledgePolicyIndex {
    summary: KnowledgeStateSummary,
    refs_by_evidence_id: BTreeMap<String, Vec<ClaimKnowledgeItemRef>>,
}

fn runtime_knowledge_policy_index(
    conn: &Connection,
    cards: &[EvidenceCard],
) -> Result<RuntimeKnowledgePolicyIndex> {
    let mut refs_by_evidence_id = BTreeMap::<String, Vec<ClaimKnowledgeItemRef>>::new();
    let mut summary = KnowledgeStateSummary::default();
    if cards.is_empty() || !sqlite_table_exists(conn, "knowledge_items")? {
        return Ok(RuntimeKnowledgePolicyIndex {
            summary,
            refs_by_evidence_id,
        });
    }
    let card_refs = cards
        .iter()
        .map(|card| (card.evidence_id.clone(), card_knowledge_ref_set(card)))
        .collect::<Vec<_>>();
    let records = query_knowledge_item_records(
        conn,
        &format!(
            "{} ORDER BY updated_at DESC, item_id DESC",
            knowledge_item_select_sql()
        ),
        &[],
    )?;
    for record in records {
        let matched_evidence_ids = card_refs
            .iter()
            .filter(|(_, refs)| {
                record
                    .evidence_refs
                    .iter()
                    .any(|item_ref| refs.contains(item_ref))
            })
            .map(|(evidence_id, _)| evidence_id.clone())
            .collect::<Vec<_>>();
        if matched_evidence_ids.is_empty() {
            continue;
        }
        match record.state {
            KnowledgeState::RuntimeUsable | KnowledgeState::HumanMarked => {
                if let Some(policy_ref) = runtime_policy_ref_for_item(&record)? {
                    summary.selected_count += 1;
                    if record.state == KnowledgeState::RuntimeUsable {
                        summary.runtime_usable_count += 1;
                    } else {
                        summary.human_marked_count += 1;
                    }
                    for evidence_id in matched_evidence_ids {
                        let mut policy_ref = policy_ref.clone();
                        policy_ref.evidence_ref = evidence_id.clone();
                        refs_by_evidence_id
                            .entry(evidence_id)
                            .or_default()
                            .push(policy_ref);
                    }
                } else {
                    summary.runtime_policy_rejected_count += 1;
                }
            }
            KnowledgeState::SystemCalibrated => {
                summary.system_calibrated_rejected_count += 1;
                summary.runtime_policy_rejected_count += 1;
            }
            KnowledgeState::Rejected | KnowledgeState::Deprecated => {
                summary.rejected_or_deprecated_count += 1;
            }
            KnowledgeState::Candidate | KnowledgeState::SourceSnapshot => {
                summary.candidate_or_source_snapshot_count += 1;
                summary.runtime_policy_rejected_count += 1;
            }
        }
    }
    summary.safe_public_label = if summary.human_marked_count > 0 {
        Some("人工标记".to_string())
    } else if summary.runtime_usable_count > 0 {
        Some("基于当前已登记资料".to_string())
    } else {
        None
    };
    Ok(RuntimeKnowledgePolicyIndex {
        summary,
        refs_by_evidence_id,
    })
}

fn runtime_policy_ref_for_item(
    item: &KnowledgeItemRecord,
) -> Result<Option<ClaimKnowledgeItemRef>> {
    if item.evidence_refs.is_empty() || item.source_boundary.is_none() {
        return Ok(None);
    }
    if item.state == KnowledgeState::RuntimeUsable {
        if item.calibration_report_ref.is_none() || item.confidence.unwrap_or_default() < 0.8 {
            return Ok(None);
        }
        let Some(policy) = item.payload.get("runtime_policy") else {
            return Ok(None);
        };
        if policy.get("policy_version").and_then(Value::as_str)
            != Some(KNOWLEDGE_RUNTIME_POLICY_VERSION)
        {
            return Ok(None);
        }
        if policy
            .get("release_run_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Ok(None);
        }
        if let Some(expires_at) = policy.get("expires_at").and_then(Value::as_str)
            && expires_at <= now_rfc3339().as_str()
        {
            return Ok(None);
        }
    }
    let display_label = if item.state == KnowledgeState::HumanMarked {
        Some("人工标记".to_string())
    } else {
        Some("基于当前已登记资料".to_string())
    };
    Ok(Some(ClaimKnowledgeItemRef {
        item_id: item.item_id.clone(),
        state: item.state,
        evidence_ref: String::new(),
        policy_version: KNOWLEDGE_RUNTIME_POLICY_VERSION.to_string(),
        policy_decision: "selected".to_string(),
        calibration_report_ref: item.calibration_report_ref.clone(),
        display_label,
    }))
}

fn card_knowledge_ref_set(card: &EvidenceCard) -> BTreeSet<String> {
    [
        card.evidence_id.clone(),
        card.block_id.clone(),
        format!("block://{}", card.block_id),
        card.source_id.clone(),
        format!("source://{}", card.source_id),
    ]
    .into_iter()
    .collect()
}

fn apply_knowledge_state_review(review: &mut ReviewRecord, summary: &KnowledgeStateSummary) {
    if summary.runtime_policy_rejected_count == 0 && summary.rejected_or_deprecated_count == 0 {
        return;
    }
    review
        .issues
        .push("存在未进入 runtime_usable 的知识条目，不能作为运行证据使用。".to_string());
    review.status = "needs_revision".to_string();
    if review.severity == "none" {
        review.severity = "medium".to_string();
    }
    review.summary = format!("reviewer 要求谨慎降级：{} 个问题。", review.issues.len());
}

fn knowledge_state_summary_from_claim_maps(
    claim_maps: &[ClaimEvidenceMap],
) -> KnowledgeStateSummary {
    let mut summary = KnowledgeStateSummary::default();
    let mut seen = BTreeSet::new();
    for item in claim_maps
        .iter()
        .flat_map(|claim| claim.knowledge_item_refs.iter())
    {
        if !seen.insert(item.item_id.clone()) {
            continue;
        }
        summary.selected_count += 1;
        match item.state {
            KnowledgeState::RuntimeUsable => summary.runtime_usable_count += 1,
            KnowledgeState::HumanMarked => summary.human_marked_count += 1,
            _ => summary.runtime_policy_rejected_count += 1,
        }
    }
    summary.safe_public_label = if summary.human_marked_count > 0 {
        Some("人工标记".to_string())
    } else if summary.runtime_usable_count > 0 {
        Some("基于当前已登记资料".to_string())
    } else {
        None
    };
    summary
}

fn claim_evidence_map(claims: &[String], cards: &[EvidenceCard]) -> Vec<ClaimEvidenceMap> {
    claim_evidence_map_with_knowledge(claims, cards, &BTreeMap::new())
}

fn claim_evidence_map_with_knowledge(
    claims: &[String],
    cards: &[EvidenceCard],
    knowledge_refs_by_evidence_id: &BTreeMap<String, Vec<ClaimKnowledgeItemRef>>,
) -> Vec<ClaimEvidenceMap> {
    claims
        .iter()
        .enumerate()
        .map(|(claim_index, claim)| {
            let evidence_ids = cards
                .iter()
                .filter(|card| {
                    claim_evidence_types_for_claim(claim)
                        .ok()
                        .flatten()
                        .is_none_or(|evidence_types| evidence_types.contains(&card.evidence_type))
                })
                .map(|card| card.evidence_id.clone())
                .collect::<Vec<_>>();
            let knowledge_item_refs = evidence_ids
                .iter()
                .flat_map(|evidence_id| {
                    knowledge_refs_by_evidence_id
                        .get(evidence_id)
                        .into_iter()
                        .flatten()
                        .cloned()
                })
                .map(|item| (format!("{}:{}", item.item_id, item.evidence_ref), item))
                .collect::<BTreeMap<_, _>>();
            let knowledge_item_refs = knowledge_item_refs.into_values().collect::<Vec<_>>();
            let forbidden_conclusions = if cards.is_empty() {
                vec!["不能给出确定结论。".to_string()]
            } else {
                cards
                    .iter()
                    .map(|card| card.unsupported_scope.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            };
            ClaimEvidenceMap {
                claim_index,
                claim: claim.clone(),
                evidence_ids,
                knowledge_item_refs,
                forbidden_conclusions,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Deserialize)]
struct SearchBlockRecord {
    block_id: String,
    kind: String,
    revision_id: Option<i64>,
    source_id: String,
    source_title: String,
    normalized_source_title: String,
    source_url: String,
    text: String,
    normalized_text: String,
}

#[derive(Debug, Clone)]
struct SearchEvidenceResult {
    cards: Vec<EvidenceCard>,
    expanded_terms: Vec<String>,
    expanded_aliases: Vec<String>,
    match_channel_counts: BTreeMap<String, usize>,
    exact_terms: Vec<String>,
    candidate_count: usize,
}

#[derive(Debug, Clone)]
struct ExtractedQueryTerms {
    terms: Vec<String>,
    aliases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryExpansionCatalog {
    schema_version: String,
    catalog_version: String,
    #[serde(default)]
    entries: Vec<QueryExpansionEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryExpansionEntry {
    id: String,
    trigger: QueryExpansionTrigger,
    #[serde(default)]
    terms: Vec<String>,
    #[serde(default)]
    exact_terms: Vec<String>,
    #[serde(default)]
    evidence_slots: Vec<QueryExpansionEvidenceSlot>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryExpansionEvidenceSlot {
    id: String,
    terms: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryExpansionTrigger {
    #[serde(default)]
    any: Vec<String>,
    #[serde(default)]
    all: Vec<String>,
    #[serde(default)]
    all_any: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
struct QueryExpansionCatalogCache {
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    len: u64,
    catalog: QueryExpansionCatalog,
}

impl Default for QueryExpansionCatalogCache {
    fn default() -> Self {
        Self {
            path: None,
            modified: None,
            len: 0,
            catalog: parse_query_expansion_catalog(DEFAULT_QUERY_EXPANSIONS_JSON)
                .expect("embedded query expansion catalog must parse"),
        }
    }
}

impl QueryExpansionCatalogCache {
    fn catalog(&mut self, path: Option<PathBuf>) -> Result<QueryExpansionCatalog> {
        let Some(path) = path else {
            if self.path.is_some() {
                *self = Self::default();
            }
            return Ok(self.catalog.clone());
        };
        let metadata = fs::metadata(&path).with_context(|| {
            format!(
                "{}={} is not readable",
                QUERY_EXPANSIONS_PATH_ENV,
                path.display()
            )
        })?;
        let modified = metadata.modified().ok();
        let len = metadata.len();
        if self.path.as_ref() == Some(&path) && self.modified == modified && self.len == len {
            return Ok(self.catalog.clone());
        }
        let source = fs::read_to_string(&path).with_context(|| {
            format!(
                "{}={} could not be read",
                QUERY_EXPANSIONS_PATH_ENV,
                path.display()
            )
        })?;
        let catalog = parse_query_expansion_catalog(&source).with_context(|| {
            format!(
                "{}={} is not a valid query expansion catalog",
                QUERY_EXPANSIONS_PATH_ENV,
                path.display()
            )
        })?;
        self.path = Some(path);
        self.modified = modified;
        self.len = len;
        self.catalog = catalog.clone();
        Ok(catalog)
    }
}

fn configured_query_expansions_path() -> Option<PathBuf> {
    std::env::var(QUERY_EXPANSIONS_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn query_expansion_catalog() -> Result<QueryExpansionCatalog> {
    let path = configured_query_expansions_path();
    let cache = QUERY_EXPANSION_CATALOG_CACHE
        .get_or_init(|| Mutex::new(QueryExpansionCatalogCache::default()));
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow!("query expansion catalog cache is poisoned"))?;
    cache.catalog(path)
}

fn parse_query_expansion_catalog(source: &str) -> Result<QueryExpansionCatalog> {
    let catalog: QueryExpansionCatalog =
        serde_json::from_str(source).context("query expansion catalog must be JSON")?;
    if catalog.schema_version != QUERY_EXPANSIONS_SCHEMA_VERSION {
        return Err(anyhow!(
            "query expansion catalog schema_version must be {}",
            QUERY_EXPANSIONS_SCHEMA_VERSION
        ));
    }
    if catalog.catalog_version.trim().is_empty() {
        return Err(anyhow!(
            "query expansion catalog catalog_version is required"
        ));
    }
    for entry in &catalog.entries {
        if entry.id.trim().is_empty() {
            return Err(anyhow!("query expansion entry id is required"));
        }
        if entry.terms.is_empty() && entry.exact_terms.is_empty() {
            return Err(anyhow!(
                "query expansion entry {} must define terms or exact_terms",
                entry.id
            ));
        }
        if entry.terms.iter().all(|term| term.trim().is_empty())
            && entry.exact_terms.iter().all(|term| term.trim().is_empty())
        {
            return Err(anyhow!(
                "query expansion entry {} must define non-empty terms or exact_terms",
                entry.id
            ));
        }
        if entry.trigger.any.is_empty()
            && entry.trigger.all.is_empty()
            && entry.trigger.all_any.is_empty()
        {
            return Err(anyhow!(
                "query expansion entry {} must define a trigger",
                entry.id
            ));
        }
        for alternatives in &entry.trigger.all_any {
            if alternatives.is_empty() {
                return Err(anyhow!(
                    "query expansion entry {} has an empty all_any trigger group",
                    entry.id
                ));
            }
            if alternatives.iter().all(|term| term.trim().is_empty()) {
                return Err(anyhow!(
                    "query expansion entry {} has an all_any trigger group without non-empty terms",
                    entry.id
                ));
            }
        }
        for slot in &entry.evidence_slots {
            if slot.id.trim().is_empty() {
                return Err(anyhow!(
                    "query expansion entry {} has an evidence slot without id",
                    entry.id
                ));
            }
            if slot.terms.is_empty() || slot.terms.iter().all(|term| term.trim().is_empty()) {
                return Err(anyhow!(
                    "query expansion entry {} evidence slot {} must define non-empty terms",
                    entry.id,
                    slot.id
                ));
            }
        }
    }
    Ok(catalog)
}

fn required_exact_terms(question: &str) -> Result<Vec<String>> {
    let catalog = query_expansion_catalog()?;
    let normalized = normalize_query(question);
    let mut terms = Vec::new();
    apply_query_expansion_exact_terms(&catalog, question, &normalized, &mut terms);
    Ok(terms)
}

fn apply_query_expansion_terms(
    catalog: &QueryExpansionCatalog,
    question: &str,
    normalized: &str,
    terms: &mut Vec<String>,
) {
    for entry in &catalog.entries {
        if query_expansion_entry_matches(entry, question, normalized) {
            for term in &entry.terms {
                push_term(terms, term);
            }
        }
    }
}

fn apply_query_expansion_exact_terms(
    catalog: &QueryExpansionCatalog,
    question: &str,
    normalized: &str,
    terms: &mut Vec<String>,
) {
    for entry in &catalog.entries {
        if query_expansion_entry_matches(entry, question, normalized) {
            for term in &entry.exact_terms {
                push_term(terms, term);
            }
        }
    }
}

fn apply_query_expansion_evidence_slot_terms(
    catalog: &QueryExpansionCatalog,
    question: &str,
    normalized: &str,
    terms: &mut Vec<String>,
) {
    for entry in &catalog.entries {
        if query_expansion_entry_matches(entry, question, normalized) {
            for slot in &entry.evidence_slots {
                for term in &slot.terms {
                    push_term(terms, term);
                }
            }
        }
    }
}

fn matched_query_expansion_evidence_slots(
    question: &str,
    cards: &[EvidenceCard],
) -> Result<BTreeSet<String>> {
    let catalog = query_expansion_catalog()?;
    let normalized = normalize_query(question);
    let mut slots = BTreeSet::new();
    for entry in &catalog.entries {
        if !query_expansion_entry_matches(entry, question, &normalized) {
            continue;
        }
        for slot in &entry.evidence_slots {
            if slot.terms.iter().any(|term| {
                cards
                    .iter()
                    .any(|card| evidence_text_contains_focus(&card.text, term))
            }) {
                slots.insert(slot.id.clone());
            }
        }
    }
    Ok(slots)
}

fn evidence_slot_matches_for_cards(
    question: &str,
    cards: &[EvidenceCard],
) -> Result<Vec<EvidenceSlotMatch>> {
    let mut matches = Vec::new();
    for card in cards {
        let slot_matches = matched_query_expansion_evidence_slot_matches_for_card(question, card)?;
        for (slot_id, matched_terms) in slot_matches {
            let rules = evidence_slot_rules_for_ids(std::slice::from_ref(&slot_id))?;
            for rule in rules {
                matches.push(EvidenceSlotMatch::from_rule(
                    &card.evidence_id,
                    &card.source_title,
                    &source_layer_for_card(card),
                    &card.text,
                    matched_terms.clone(),
                    rule,
                ));
            }
        }
    }
    Ok(matches)
}

fn matched_query_expansion_evidence_slot_ids_for_card(
    question: &str,
    card: &EvidenceCard,
) -> Result<Vec<String>> {
    Ok(
        matched_query_expansion_evidence_slot_matches_for_card(question, card)?
            .into_iter()
            .map(|(slot_id, _)| slot_id)
            .collect(),
    )
}

fn matched_query_expansion_evidence_slot_matches_for_card(
    question: &str,
    card: &EvidenceCard,
) -> Result<Vec<(String, Vec<String>)>> {
    let catalog = query_expansion_catalog()?;
    let normalized = normalize_query(question);
    let mut slots = Vec::new();
    for entry in &catalog.entries {
        if !query_expansion_entry_matches(entry, question, &normalized) {
            continue;
        }
        for slot in &entry.evidence_slots {
            let matched_terms = slot
                .terms
                .iter()
                .filter(|term| evidence_text_contains_focus(&card.text, term))
                .cloned()
                .collect::<Vec<_>>();
            if !matched_terms.is_empty() && !slots.iter().any(|(slot_id, _)| slot_id == &slot.id) {
                slots.push((slot.id.clone(), matched_terms));
            }
        }
    }
    Ok(slots)
}

fn query_expansion_entry_matches(
    entry: &QueryExpansionEntry,
    question: &str,
    normalized: &str,
) -> bool {
    if !entry.trigger.any.is_empty()
        && !entry
            .trigger
            .any
            .iter()
            .any(|term| query_matches_expansion_term(question, normalized, term))
    {
        return false;
    }
    if !entry
        .trigger
        .all
        .iter()
        .all(|term| query_matches_expansion_term(question, normalized, term))
    {
        return false;
    }
    if !entry.trigger.all_any.iter().all(|alternatives| {
        alternatives
            .iter()
            .any(|term| query_matches_expansion_term(question, normalized, term))
    }) {
        return false;
    }
    !entry.trigger.any.is_empty()
        || !entry.trigger.all.is_empty()
        || !entry.trigger.all_any.is_empty()
}

fn query_matches_expansion_term(question: &str, normalized: &str, term: &str) -> bool {
    let term = term.trim();
    if term.is_empty() {
        return false;
    }
    if query_expansion_trigger_requires_raw_match(term) {
        return question.contains(term);
    }
    question.contains(term) || normalized.contains(&normalize_query(term))
}

fn query_expansion_trigger_requires_raw_match(term: &str) -> bool {
    term.contains('寳')
}

fn extract_query_terms(conn: &Connection, question: &str) -> Result<ExtractedQueryTerms> {
    let mut terms = Vec::new();
    let mut aliases = Vec::new();
    let mut canonical_person_ids = Vec::new();
    let normalized = normalize_query(question);
    let catalog = query_expansion_catalog()?;
    apply_query_expansion_terms(&catalog, question, &normalized, &mut terms);

    let mut stmt = conn.prepare(
        "SELECT alias, normalized_alias, person_id FROM aliases ORDER BY LENGTH(alias) DESC, alias",
    )?;
    let alias_rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for alias in alias_rows {
        let (alias, normalized_alias, person_id) = alias?;
        let normalized_alias = if normalized_alias.trim().is_empty() {
            normalize_alias(&alias)
        } else {
            normalized_alias
        };
        if question.contains(&alias)
            || normalized.contains(&normalized_alias)
            || normalize_alias(question).contains(&normalized_alias)
        {
            push_term(&mut terms, &alias);
            push_term(&mut terms, &normalized_alias);
            push_term(&mut aliases, &alias);
            push_term(&mut aliases, &normalized_alias);
            push_term(&mut canonical_person_ids, &person_id);
        }
    }
    expand_canonical_person_terms(conn, &canonical_person_ids, &mut terms, &mut aliases)?;

    for token in cjk_tokens(question) {
        if token.chars().count() >= 2 && token.chars().count() <= 8 {
            push_term(&mut terms, &token);
        }
        for focus_term in cjk_focus_terms(&token) {
            if focus_term.chars().count() >= 2 && focus_term.chars().count() <= 8 {
                push_term(&mut terms, &focus_term);
            }
        }
        for focus_term in cjk_person_short_forms(&token) {
            if focus_term.chars().count() >= 2 && focus_term.chars().count() <= 8 {
                push_term(&mut terms, &focus_term);
            }
        }
    }
    if terms.is_empty() && question.chars().count() <= 24 {
        push_term(&mut terms, question);
    }
    Ok(ExtractedQueryTerms { terms, aliases })
}

fn expand_canonical_person_terms(
    conn: &Connection,
    person_ids: &[String],
    terms: &mut Vec<String>,
    aliases: &mut Vec<String>,
) -> Result<()> {
    for person_id in person_ids {
        let canonical_name: Option<String> = conn
            .query_row(
                "SELECT canonical_name FROM people WHERE person_id = ?1",
                params![person_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(canonical_name) = canonical_name {
            push_term(terms, &canonical_name);
            push_term(terms, &normalize_alias(&canonical_name));
            push_term(aliases, &canonical_name);
            push_term(aliases, &normalize_alias(&canonical_name));
        }
        let mut stmt = conn.prepare(
            "SELECT alias, normalized_alias FROM aliases WHERE person_id = ?1 ORDER BY LENGTH(alias) DESC, alias",
        )?;
        let rows = stmt.query_map(params![person_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (alias, normalized_alias) = row?;
            let normalized_alias = if normalized_alias.trim().is_empty() {
                normalize_alias(&alias)
            } else {
                normalized_alias
            };
            push_term(terms, &alias);
            push_term(terms, &normalized_alias);
            push_term(aliases, &alias);
            push_term(aliases, &normalized_alias);
        }
    }
    Ok(())
}

fn query_blocks_like(
    conn: &Connection,
    term: &str,
    limit: usize,
) -> Result<Vec<SearchBlockRecord>> {
    let like = format!("%{}%", term.replace('%', "\\%").replace('_', "\\_"));
    let normalized_like = format!(
        "%{}%",
        normalize_text(term).replace('%', "\\%").replace('_', "\\_")
    );
    let mut stmt = conn.prepare(
        r#"
        SELECT block_id, kind, revision_id, source_id, source_title,
               normalized_source_title, source_url, text, normalized_text
        FROM blocks
        WHERE text LIKE ?1 ESCAPE '\'
           OR source_title LIKE ?1 ESCAPE '\'
           OR normalized_text LIKE ?2 ESCAPE '\'
           OR normalized_source_title LIKE ?2 ESCAPE '\'
        ORDER BY
          CASE
            WHEN text LIKE ?1 ESCAPE '\' THEN 1
            WHEN normalized_text LIKE ?2 ESCAPE '\' THEN 2
            WHEN source_title LIKE ?1 ESCAPE '\' THEN 3
            WHEN normalized_source_title LIKE ?2 ESCAPE '\' THEN 4
            ELSE 5
          END,
          CASE evidence_type
            WHEN 'base_text' THEN 1
            WHEN 'commentary' THEN 2
            WHEN 'version_note' THEN 3
            ELSE 4
          END,
          CASE kind
            WHEN 'heading' THEN 3
            WHEN 'poem' THEN 2
            ELSE 1
          END,
          CASE WHEN LENGTH(text) <= 16 THEN 2 ELSE 1 END,
          LENGTH(text) ASC
        LIMIT ?3
        "#,
    )?;
    let rows = stmt.query_map(params![like, normalized_like, limit as i64], |row| {
        Ok(SearchBlockRecord {
            block_id: row.get(0)?,
            kind: row.get(1)?,
            revision_id: row.get(2)?,
            source_id: row.get(3)?,
            source_title: row.get(4)?,
            normalized_source_title: row.get(5)?,
            source_url: row.get(6)?,
            text: row.get(7)?,
            normalized_text: row.get(8)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn query_blocks_exact_text(
    conn: &Connection,
    term: &str,
    limit: usize,
) -> Result<Vec<SearchBlockRecord>> {
    let like = format!("%{}%", term.replace('%', "\\%").replace('_', "\\_"));
    let mut stmt = conn.prepare(
        r#"
        SELECT block_id, kind, revision_id, source_id, source_title,
               normalized_source_title, source_url, text, normalized_text
        FROM blocks
        WHERE text LIKE ?1 ESCAPE '\'
           OR normalized_text LIKE ?2 ESCAPE '\'
        ORDER BY LENGTH(text) ASC
        LIMIT ?3
        "#,
    )?;
    let normalized_like = format!(
        "%{}%",
        normalize_text(term).replace('%', "\\%").replace('_', "\\_")
    );
    let rows = stmt.query_map(params![like, normalized_like, limit as i64], |row| {
        Ok(SearchBlockRecord {
            block_id: row.get(0)?,
            kind: row.get(1)?,
            revision_id: row.get(2)?,
            source_id: row.get(3)?,
            source_title: row.get(4)?,
            normalized_source_title: row.get(5)?,
            source_url: row.get(6)?,
            text: row.get(7)?,
            normalized_text: row.get(8)?,
        })
    })?;
    let mut rows = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    rows.sort_by_key(|block| {
        (
            retrieval_rules::exact_source_priority_rank(&block.source_id).unwrap_or(usize::MAX),
            block.text.chars().count(),
        )
    });
    Ok(rows)
}

fn evidence_card_from_block(block: SearchBlockRecord) -> EvidenceCard {
    evidence_card_from_block_text(block, None)
}

fn evidence_card_from_block_with_focus(block: SearchBlockRecord, focus: &str) -> EvidenceCard {
    evidence_card_from_block_text(block, Some(focus))
}

fn evidence_card_from_block_text(block: SearchBlockRecord, focus: Option<&str>) -> EvidenceCard {
    let is_later_forty = source_title_in_later_forty(&block.source_title);
    let evidence_type = retrieval_rules::classify_evidence_type("", &block.source_id, &block.text)
        .expect("retrieval source classification rules must load");
    let scope = retrieval_rules::evidence_type_scope(&evidence_type)
        .expect("retrieval evidence type scope rules must load");
    let (mut support_scope, mut unsupported_scope, evidence_level, confidence) = (
        scope.support_scope,
        scope.unsupported_scope,
        scope.evidence_level,
        scope.confidence,
    );
    if is_later_forty {
        support_scope = format!("第八十一回及以后（后四十回）边界：{support_scope}");
        unsupported_scope = format!(
            "必须显式标注为第八十一回及以后（后四十回）内容；未标注时不能作为证据或参考。{unsupported_scope}"
        );
    }
    EvidenceCard {
        evidence_id: format!("ev-{}", uuid::Uuid::now_v7().simple()),
        evidence_type,
        source_id: block.source_id,
        source_title: block.source_title,
        source_url: block.source_url,
        revision_id: block.revision_id,
        block_id: block.block_id,
        text: match focus {
            Some(focus) => trim_text_around(&block.text, focus, 520),
            None => trim_text(&block.text, 520),
        },
        support_scope,
        unsupported_scope,
        evidence_level,
        confidence,
        verification_status: "source_snapshot_ready_not_scholarly_collated".to_string(),
    }
}

fn score_block(question: &str, term: &str, block: &SearchBlockRecord) -> i64 {
    let ranking = retrieval_rules::ranking_rules().expect("retrieval ranking rules must load");
    let mut score = 1;
    let normalized_term = normalize_text(term);
    let intro_question =
        retrieval_rules::contains_any_term(question, &ranking.intro_question_terms);
    let normalized_focus = if intro_question {
        normalized_primary_focus(question)
    } else {
        None
    };
    if block.text.contains(term) {
        score += 18;
    }
    if block.normalized_text.contains(&normalized_term) {
        score += 12;
    }
    if block.source_title.contains(term) {
        score += 5;
    }
    if block.normalized_source_title.contains(&normalized_term) {
        score += 3;
    }
    if let Some(focus) = normalized_focus.as_deref() {
        let focus_len = focus.chars().count();
        if focus_len >= 3 && normalized_term == focus {
            if block.normalized_text.contains(focus) {
                score += 45;
            }
            if block.normalized_source_title.contains(focus) {
                score += 12;
            }
        } else if focus_len >= 3
            && normalized_term.chars().count() < focus_len
            && !block.normalized_text.contains(focus)
            && !block.normalized_source_title.contains(focus)
        {
            score -= 10;
        }
    }
    if retrieval_rules::contains_any_term(question, &ranking.commentary_question_terms)
        && retrieval_rules::contains_any_raw(&block.source_id, &ranking.commentary_source_id_terms)
    {
        score += 8;
    }
    for boost in &ranking.version_source_boosts {
        if retrieval_rules::contains_any_term(question, &boost.question_terms)
            && retrieval_rules::contains_any_raw(&block.source_id, &boost.source_id_terms)
        {
            score += boost.score;
        }
    }
    if block.kind == "heading" {
        score -= 12;
    }
    let text_len = block.text.chars().count();
    if text_len <= 8 {
        score -= 18;
    } else if text_len <= 24 {
        score -= 8;
    } else if text_len >= 80 {
        score += 6;
    }
    if intro_question && block.kind != "heading" && text_len >= 40 {
        score += 12;
    }
    let asks_inscription =
        retrieval_rules::contains_any_term(question, &ranking.inscription_question_terms);
    let looks_like_inscription =
        retrieval_rules::contains_any_term(&block.text, &ranking.inscription_text_terms);
    if asks_inscription && looks_like_inscription {
        score += 50;
    } else if retrieval_rules::contains_any_term(term, &ranking.tonglingyu_terms)
        && looks_like_inscription
    {
        score += 20;
    }
    let asks_fate = retrieval_rules::contains_any_term(question, &ranking.fate_question_terms);
    let looks_like_fate = retrieval_rules::contains_any_term(&block.text, &ranking.fate_text_terms);
    if asks_fate && looks_like_fate {
        score += 55;
        if retrieval_rules::contains_any_term(term, &ranking.fate_text_terms) {
            score += 20;
        }
    }
    score
}

fn evidence_type(source_category: &str, source_id: &str, block: &BlockRecord) -> Result<String> {
    retrieval_rules::classify_evidence_type(source_category, source_id, &block.text)
}

pub fn normalize_for_search(input: &str) -> String {
    text_normalizer().normalize_for_search(input)
}

fn normalize_text(input: &str) -> String {
    normalize_for_search(input)
}

fn normalize_query(input: &str) -> String {
    text_normalizer().normalize_query(input)
}

fn normalize_alias(input: &str) -> String {
    text_normalizer().normalize_alias(input)
}

fn normalize_title(input: &str) -> String {
    text_normalizer().normalize_title(input)
}

fn apply_project_normalization_overrides(input: &str) -> String {
    let replacements = [
        ("寳", "宝"),
        ("玉寶靈通", "玉宝灵通"),
        ("僊", "仙"),
        ("冩", "写"),
        ("衆", "众"),
        ("裏", "里"),
        ("裡", "里"),
        ("檯", "台"),
        ("恒", "恒"),
    ];
    let mut output = input.to_lowercase();
    for (from, to) in replacements {
        output = output.replace(from, to);
    }
    output
}

fn useful_text(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed != "----" && !trimmed.starts_with("[[../")
}

fn version_range(chapter_no: i64) -> &'static str {
    if chapter_no <= 80 {
        "前八十回"
    } else {
        "后四十回"
    }
}

fn commentary_type(text: &str) -> &'static str {
    if text.contains("{{~|") || text.contains("[") {
        "inline_commentary"
    } else {
        "commentary_text"
    }
}

fn extract_chapter_no(title: &str) -> Option<i64> {
    if let Some(after_di) = title.split('第').nth(1) {
        let value = after_di.split('回').next()?;
        if let Some(number) = parse_chapter_number_value(value) {
            return Some(number);
        }
    }
    title
        .rsplit(['/', '_'])
        .next()
        .map(str::trim)
        .and_then(parse_chapter_number_value)
}

fn parse_chapter_number_value(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let value = value
        .strip_prefix('第')
        .unwrap_or(value)
        .strip_suffix('回')
        .unwrap_or(value)
        .trim();
    if value.is_empty() {
        return None;
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return value.parse().ok();
    }
    if !value.chars().all(chinese_number_char) {
        return None;
    }
    chinese_number(value)
}

fn chinese_number_char(ch: char) -> bool {
    matches!(
        ch,
        '零' | '一'
            | '二'
            | '兩'
            | '两'
            | '三'
            | '四'
            | '五'
            | '六'
            | '七'
            | '八'
            | '九'
            | '十'
            | '百'
    )
}

fn chinese_number(value: &str) -> Option<i64> {
    let value = value.replace('零', "");
    if value.is_empty() {
        return None;
    }
    if let Some((hundred, rest)) = value.split_once('百') {
        let hundreds = if hundred.is_empty() {
            1
        } else {
            chinese_digit(hundred.chars().next()?)?
        };
        return Some(hundreds * 100 + chinese_under_100(rest).unwrap_or(0));
    }
    chinese_under_100(&value)
}

fn chinese_under_100(value: &str) -> Option<i64> {
    if value.is_empty() {
        return Some(0);
    }
    if let Some((tens, ones)) = value.split_once('十') {
        let ten_value = if tens.is_empty() {
            1
        } else {
            chinese_digit(tens.chars().next()?)?
        };
        let one_value = if ones.is_empty() {
            0
        } else {
            chinese_digit(ones.chars().next()?)?
        };
        return Some(ten_value * 10 + one_value);
    }
    chinese_digit(value.chars().next()?)
}

fn chinese_digit(ch: char) -> Option<i64> {
    match ch {
        '一' => Some(1),
        '二' | '兩' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn cjk_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in input.chars() {
        if is_cjk(ch) {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.extend(split_cjk_token(&current));
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.extend(split_cjk_token(&current));
    }
    tokens
}

fn split_cjk_token(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 8 {
        return vec![token.to_string()];
    }
    chars
        .windows(4)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn cjk_focus_terms(token: &str) -> Vec<String> {
    let prefixes = [
        "请介绍一下",
        "請介紹一下",
        "介绍一下",
        "介紹一下",
        "请介绍",
        "請介紹",
        "介绍",
        "介紹",
        "说说",
        "說說",
        "讲讲",
        "講講",
        "讲一下",
        "講一下",
        "解释",
        "解釋",
        "分析",
        "概述",
        "简述",
        "簡述",
        "说明",
        "說明",
        "谈谈",
        "談談",
    ];
    let suffixes = [
        "是什么",
        "是什麼",
        "是谁",
        "是誰",
        "怎么样",
        "怎樣",
        "如何",
        "介绍",
        "介紹",
        "生平",
        "人物",
    ];
    let mut focus = token.trim().to_string();
    let mut changed = true;
    while changed {
        changed = false;
        for prefix in prefixes {
            if let Some(stripped) = focus.strip_prefix(prefix) {
                focus = stripped.trim().to_string();
                changed = true;
                break;
            }
        }
        if changed {
            continue;
        }
        for suffix in suffixes {
            if let Some(stripped) = focus.strip_suffix(suffix) {
                focus = stripped.trim().to_string();
                changed = true;
                break;
            }
        }
    }
    if focus.is_empty() || focus == token {
        Vec::new()
    } else {
        vec![focus]
    }
}

fn cjk_person_short_forms(token: &str) -> Vec<String> {
    let focus_terms = cjk_focus_terms(token);
    let source_terms = if focus_terms.is_empty() {
        vec![token.trim().to_string()]
    } else {
        focus_terms
    };
    source_terms
        .into_iter()
        .filter_map(|term| {
            let chars = term.chars().collect::<Vec<_>>();
            if chars.len() == 3 && chars.iter().all(|ch| is_cjk(*ch)) {
                Some(chars[1..].iter().collect::<String>())
            } else {
                None
            }
        })
        .collect()
}

fn is_cjk(ch: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&ch)
        || ('\u{3400}'..='\u{4dbf}').contains(&ch)
        || ('\u{20000}'..='\u{2a6df}').contains(&ch)
        || ('\u{2a700}'..='\u{2b73f}').contains(&ch)
        || ('\u{2b740}'..='\u{2b81f}').contains(&ch)
        || ('\u{2b820}'..='\u{2ceaf}').contains(&ch)
}

fn push_term(terms: &mut Vec<String>, term: &str) {
    let term = term.trim();
    if !term.is_empty() && !terms.iter().any(|item| item == term) {
        terms.push(term.to_string());
    }
}

fn trim_text(text: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= max_chars {
            output.push_str("...");
            break;
        }
        output.push(ch);
    }
    output
}

fn trim_text_around(text: &str, focus: &str, max_chars: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let focus_index = if let Some(byte_index) = text.find(focus) {
        text[..byte_index].chars().count()
    } else if let Some(index) = normalized_focus_char_index(text, focus) {
        index
    } else {
        return trim_text(text, max_chars);
    };
    let half = max_chars / 2;
    let start = focus_index.saturating_sub(half);
    let end = (start + max_chars).min(chars.len());
    let mut output = String::new();
    if start > 0 {
        output.push_str("...");
    }
    for ch in &chars[start..end] {
        output.push(*ch);
    }
    if end < chars.len() {
        output.push_str("...");
    }
    output
}

fn normalized_focus_char_index(text: &str, focus: &str) -> Option<usize> {
    let normalized_focus = normalize_text(focus);
    if normalized_focus.is_empty() {
        return None;
    }
    let normalized_text = normalize_text(text);
    let byte_index = normalized_text.find(&normalized_focus)?;
    Some(normalized_text[..byte_index].chars().count())
}

pub fn append_runtime_audit_event(
    conn: &Connection,
    trace_id: &str,
    event_type: &str,
    payload: &Value,
) -> Result<()> {
    conn.execute(
        "INSERT INTO audit_events (event_id, trace_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            format!("audit-{}", uuid::Uuid::now_v7().simple()),
            trace_id,
            event_type,
            serde_json::to_string(payload)?,
            now_rfc3339(),
        ],
    )?;
    Ok(())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
mod tests;
