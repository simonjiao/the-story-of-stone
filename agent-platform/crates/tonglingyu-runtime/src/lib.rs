use agent_core::{
    AgentCoreError, CoreResult, ErrorCode, ProfileContract as AgentProfileContract, RuntimeClient,
    RuntimeProfileInput, RuntimeProfileMessage, RuntimeStep as AgentRuntimeStep,
    RuntimeStepPlan as AgentRuntimeStepPlan, RuntimeStepPlanInput as AgentRuntimeStepPlanInput,
    RuntimeStepPlanOwner, RuntimeToolCall, RuntimeToolExecutor, RuntimeToolPolicy,
    RuntimeToolResult, RuntimeToolSpec,
};
use agent_runtime::{HermesRuntimeClient, MinimalRuntimeClient, RuntimeProfileRegistry};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures_util::future::try_join_all;
use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use time::OffsetDateTime;

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
    pub forbidden_conclusions: Vec<String>,
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
    pub audit_events: i64,
    pub review_status: BTreeMap<String, i64>,
    pub evidence_types: BTreeMap<String, i64>,
    pub retrieval_failure_status: BTreeMap<String, i64>,
    pub retrieval_failure_type: BTreeMap<String, i64>,
    pub governance_task_status: BTreeMap<String, i64>,
    pub governance_task_type: BTreeMap<String, i64>,
    pub governance_task_priority: BTreeMap<String, i64>,
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
pub const GOVERNANCE_TASK_DEFAULT_PAGE_SIZE: usize = 50;
pub const GOVERNANCE_TASK_MAX_PAGE_SIZE: usize = 100;

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
        let runtime = tonglingyu_agent_runtime_client(mode, self.clone(), registry)?;
        self.execute_workflow_with_agent_runtime_client(input, mode, runtime)
            .await
    }

    async fn execute_workflow_with_agent_runtime_client(
        &self,
        input: RuntimeWorkflowInput,
        mode: TonglingyuAgentRuntimeMode,
        runtime: Arc<dyn RuntimeClient>,
    ) -> Result<RuntimeWorkflowOutput> {
        let mut workflow = {
            let conn = self.open_connection()?;
            execute_runtime_workflow(&conn, input.clone())?
        };
        if let Err(error) =
            attach_agent_runtime_step_execution(&mut workflow, &input.profiles, mode, runtime).await
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
        let agent_runtime_content_application =
            apply_agent_runtime_content_outputs(&mut workflow, mode);
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
}

impl TonglingyuAgentRuntimeMode {
    pub fn from_env() -> Result<Self> {
        let value = std::env::var("TONGLINGYU_AGENT_RUNTIME_MODE")
            .unwrap_or_else(|_| "minimal".to_string());
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "minimal" => Ok(Self::Minimal),
            "hermes" => Ok(Self::Hermes),
            other => Err(anyhow!("unsupported TONGLINGYU_AGENT_RUNTIME_MODE={other}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Hermes => "hermes",
        }
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRuntimePlanGateInput {
    pub trace_id: String,
    pub question: String,
    #[serde(default)]
    pub required_evidence_types: Vec<String>,
    pub profiles: RuntimeWorkflowProfiles,
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
                "evidence_observation_required": ["commentary_refs", "commentary_analysis", "base_text_limits"],
                "must_label": ["commentary", "version_note"]
            }),
            safety_contract: json!({
                "cannot_prove_base_text_fact_alone": true,
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
                "required": ["draft_candidate"],
                "draft_candidate_required": ["draft_answer", "package_id", "claim_statements"],
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
                "max_message_bytes": 8192
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
        let runtime_step = agent_runtime_step_from_plan_step(
            plan_step,
            depends_on,
            descriptors.get(&plan_step.profile),
        );
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
                "base_text_limits": "commentary evidence cannot prove base text facts alone",
                "quality_report": &commentary_quality_report,
                }),
            },
        )?);
    }

    let package_started = Instant::now();
    let package = match execute_tool(
        conn,
        TonglingyuToolCall::EvidencePackageCreate {
            trace_id: input.trace_id.clone(),
            question: input.question.clone(),
            cards,
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
            }),
        },
    )?);
    let draft_started = Instant::now();
    let draft_answer = local_answer(&input.question, &package);
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
            "claim_statements": &package.claims,
            "answer_source": "runtime_local_profile",
            }),
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
    mode: TonglingyuAgentRuntimeMode,
    runtime: Arc<dyn RuntimeClient>,
) -> Result<()> {
    let profile_contracts = agent_runtime_profile_contracts(profiles);
    let contracts = profile_contracts
        .into_iter()
        .map(|contract| (contract.profile_id.clone(), contract))
        .collect::<BTreeMap<_, _>>();
    let trace_id = workflow.trace_id.clone();
    let question = workflow.question.clone();
    let mut executions = Vec::with_capacity(workflow.steps.len());
    for (index, step) in workflow.steps.iter().cloned().enumerate() {
        let result_summary_contract = agent_runtime_result_summary_contract(&step);
        let contract = contracts
            .get(&step.profile)
            .cloned()
            .ok_or_else(|| anyhow!("runtime profile contract missing for {}", step.profile))?;
        executions.push(execute_agent_runtime_profile_step(
            index,
            step,
            trace_id.clone(),
            question.clone(),
            result_summary_contract.to_owned(),
            contract,
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

#[allow(clippy::too_many_arguments)]
async fn execute_agent_runtime_profile_step(
    index: usize,
    step: RuntimeWorkflowStepReport,
    trace_id: String,
    question: String,
    result_summary_contract: String,
    contract: AgentProfileContract,
    mode: TonglingyuAgentRuntimeMode,
    runtime: Arc<dyn RuntimeClient>,
) -> Result<AgentRuntimeStepExecution> {
    let runtime_step = agent_runtime_step_from_workflow_step(&step);
    let output = runtime
        .execute_profile_step(RuntimeProfileInput {
            profile_id: step.profile.clone(),
            messages: vec![agent_runtime_profile_step_message(
                &trace_id,
                &question,
                &step,
                &result_summary_contract,
            )],
            metadata: json!({
                "runtime": "tonglingyu",
                "workflow_step_id": &step.step_id,
                "operation": &step.operation,
                "input_ref": &step.input_ref,
                "output_ref": &step.output_ref,
                "step_output": &step.output,
                "result_summary_contract": &result_summary_contract,
                "question_chars": question.chars().count(),
                "question_sha256": hash_text(&question),
                "content_source": "tonglingyu-deterministic-workflow",
            }),
            profile_contract: Some(contract),
            runtime_step: Some(runtime_step),
            requested_tools: step.allowed_tools.clone(),
            trace_id,
        })
        .await?;
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
        &step,
        &mut tool_results,
        &mut tool_audit_events,
    )?;
    validate_agent_runtime_required_tools(mode, &step, &tool_results)?;
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
) -> Result<Arc<dyn RuntimeClient>> {
    match mode {
        TonglingyuAgentRuntimeMode::Minimal => Ok(Arc::new(
            MinimalRuntimeClient::default().with_profile_registry(registry),
        )),
        TonglingyuAgentRuntimeMode::Hermes => Ok(Arc::new(
            HermesRuntimeClient::from_env()?
                .with_profile_registry(registry)
                .with_tool_executor(Arc::new(TonglingyuRuntimeToolExecutor::new(store))),
        )),
    }
}

fn agent_runtime_profile_step_message(
    trace_id: &str,
    question: &str,
    step: &RuntimeWorkflowStepReport,
    result_summary_contract: &str,
) -> RuntimeProfileMessage {
    RuntimeProfileMessage::new(
        "user",
        format!(
            concat!(
                "Tonglingyu profile step execution context.\n",
                "trace_id: {trace_id}\n",
                "profile: {profile}\n",
                "operation: {operation}\n",
                "question: {question}\n",
                "input_ref: {input_ref}\n",
                "output_ref: {output_ref}\n",
                "allowed_tools: {allowed_tools}\n",
                "result_summary_contract: {result_summary_contract}\n",
                "step_output_json: {step_output}\n"
            ),
            trace_id = trace_id,
            profile = &step.profile,
            operation = &step.operation,
            question = question,
            input_ref = step.input_ref.as_deref().unwrap_or("none"),
            output_ref = &step.output_ref,
            allowed_tools = step.allowed_tools.join(","),
            result_summary_contract = result_summary_contract,
            step_output = serde_json::to_string(&step.output).unwrap_or_else(|_| "{}".to_string()),
        ),
    )
}

fn agent_runtime_result_summary_contract(step: &RuntimeWorkflowStepReport) -> &'static str {
    match step.operation.as_str() {
        "draft_answer" => {
            "The runtime envelope already has result_summary. Put this JSON object string inside it: {\"draft_candidate\":{\"draft_answer\":\"...\",\"package_id\":\"...\",\"claim_statements\":[...]}}. package_id must match step_output_json.package_id; local reviewer remains required. Do not add another result_summary key."
        }
        "review_answer" => {
            "The runtime envelope already has result_summary. Put this JSON object string inside it: {\"review_observation\":{\"review_status\":\"passed|needs_revision\",\"severity\":\"...\",\"issues\":[],\"required_revisions\":[]}}. This is observation only; local reviewer enforcement remains authoritative. Do not add another result_summary key."
        }
        "text_evidence_search" => {
            "The runtime envelope already has result_summary. Put this JSON object string inside it: {\"evidence_observation\":{\"evidence_refs\":[...],\"evidence_analysis\":\"...\",\"unsupported_scope\":\"...\"}}. evidence_refs must come from step_output_json.evidence_ids; do not write a final answer. Do not add another result_summary key."
        }
        "commentary_evidence_search" => {
            "The runtime envelope already has result_summary. Put this JSON object string inside it: {\"evidence_observation\":{\"commentary_refs\":[...],\"commentary_analysis\":\"...\",\"base_text_limits\":\"...\"}}. commentary_refs must come from step_output_json.evidence_ids; do not prove base-text facts from commentary alone. Do not add another result_summary key."
        }
        "evidence_package_create" => {
            "The runtime envelope already has result_summary. Put this JSON object string inside it: {\"package_observation\":{\"package_id\":\"...\",\"summary\":\"...\"}}. package_id must come from step_output_json; do not invent package ids. Do not add another result_summary key."
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
struct AgentRuntimeDraftExtraction {
    draft_answer: Option<String>,
    result_format: &'static str,
    package_id: Option<String>,
    claim_statement_count: Option<usize>,
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
        "hermes_content_execution_complete": false,
        "local_governance_enforced": true,
    })
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
    let hermes_content_execution_complete = mode == TonglingyuAgentRuntimeMode::Hermes
        && evidence_matches_local
        && package_matches_local
        && draft_consumed
        && review_local_enforced;
    let profile_execution_status = match mode {
        TonglingyuAgentRuntimeMode::Minimal => "minimal_envelope_only",
        TonglingyuAgentRuntimeMode::Hermes if hermes_content_execution_complete => {
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
        "content_used_for_final_answer": content_used_for_final_answer,
        "review_local_enforced": review_local_enforced,
        "hermes_content_execution_complete": hermes_content_execution_complete,
        "local_governance_enforced": true,
        "answer_source": &workflow.answer_source,
    })
}

fn validate_agent_runtime_execution_summary(
    mode: TonglingyuAgentRuntimeMode,
    summary: &Value,
) -> Result<()> {
    if mode != TonglingyuAgentRuntimeMode::Hermes {
        return Ok(());
    }
    let complete = summary
        .get("hermes_content_execution_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let status = summary
        .get("profile_execution_status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
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
    if mode != TonglingyuAgentRuntimeMode::Hermes {
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
                json!("agent-runtime-hermes-evidence-observation"),
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
    if mode != TonglingyuAgentRuntimeMode::Hermes {
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
                json!("agent-runtime-hermes-package-observation"),
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
    if mode != TonglingyuAgentRuntimeMode::Hermes {
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
                    extract_agent_runtime_draft(summary, &workflow.package.package_id),
                ))
            })?;

    if let Some(reason) = extraction.rejected_reason {
        if let Some(step) = workflow.steps.get_mut(draft_step_index) {
            step.output["agent_runtime_draft_consumed"] = json!(false);
            step.output["agent_runtime_result_format"] = json!(extraction.result_format);
            step.output["agent_runtime_draft_rejected_reason"] = json!(reason);
            step.output["agent_runtime_package_id"] = json!(extraction.package_id);
            if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut)
            {
                agent_runtime.insert(
                    "content_source".to_string(),
                    json!("agent-runtime-hermes-profile-rejected"),
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

    workflow.draft_answer = draft.clone();
    workflow.final_answer = enforce_review(draft, &workflow.package);
    let content_used_for_final_answer = workflow.package.review.status == "passed";
    workflow.answer_source = if content_used_for_final_answer {
        "agent_runtime_hermes_profile_with_local_review".to_string()
    } else {
        "agent_runtime_hermes_profile_rejected_by_local_review".to_string()
    };
    if let Some(step) = workflow.steps.get_mut(draft_step_index) {
        step.output["answer_source"] = json!("agent_runtime_hermes_profile");
        step.output["agent_runtime_draft_consumed"] = json!(true);
        step.output["agent_runtime_content_used_for_final_answer"] =
            json!(content_used_for_final_answer);
        step.output["agent_runtime_result_format"] = json!(extraction.result_format);
        step.output["agent_runtime_package_id"] = json!(extraction.package_id);
        step.output["agent_runtime_claim_statement_count"] =
            json!(extraction.claim_statement_count);
        if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut) {
            agent_runtime.insert(
                "content_source".to_string(),
                json!("agent-runtime-hermes-profile"),
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
        step.output["draft_source"] = json!("agent_runtime_hermes_profile");
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

fn extract_agent_runtime_draft(
    result_summary: &str,
    expected_package_id: &str,
) -> AgentRuntimeDraftExtraction {
    let trimmed = result_summary.trim();
    let Some(value) = parse_agent_runtime_summary_value(trimmed) else {
        return AgentRuntimeDraftExtraction {
            draft_answer: Some(trimmed.to_string()),
            result_format: "text",
            package_id: None,
            claim_statement_count: None,
            rejected_reason: None,
        };
    };

    if let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return AgentRuntimeDraftExtraction {
            draft_answer: Some(text.to_string()),
            result_format: "json_string",
            package_id: None,
            claim_statement_count: None,
            rejected_reason: None,
        };
    }

    let Some(object) = object_or_named_child(&value, "draft_candidate") else {
        return AgentRuntimeDraftExtraction {
            draft_answer: None,
            result_format: "json",
            package_id: None,
            claim_statement_count: None,
            rejected_reason: Some("unsupported_json_draft"),
        };
    };
    let package_id = object
        .get("package_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let claim_statement_count = object
        .get("claim_statements")
        .and_then(Value::as_array)
        .map(Vec::len);
    if package_id.is_none() {
        return AgentRuntimeDraftExtraction {
            draft_answer: None,
            result_format: "json",
            package_id,
            claim_statement_count,
            rejected_reason: Some("package_id_missing"),
        };
    }
    if package_id
        .as_deref()
        .is_some_and(|value| value != expected_package_id)
    {
        return AgentRuntimeDraftExtraction {
            draft_answer: None,
            result_format: "json",
            package_id,
            claim_statement_count,
            rejected_reason: Some("package_id_mismatch"),
        };
    }
    let draft_answer = object
        .get("draft_answer")
        .or_else(|| object.get("answer"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let rejected_reason = if draft_answer.is_some() {
        None
    } else {
        Some("draft_answer_missing")
    };
    AgentRuntimeDraftExtraction {
        draft_answer,
        result_format: "json",
        package_id,
        claim_statement_count,
        rejected_reason,
    }
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
    if mode != TonglingyuAgentRuntimeMode::Hermes {
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
                json!("agent-runtime-hermes-review-observation"),
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
}

fn workflow_step_report(
    conn: &Connection,
    input: WorkflowStepReportInput<'_>,
) -> Result<RuntimeWorkflowStepReport> {
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
        output: input.output,
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
        }),
    )?;
    Ok(report)
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
        .unwrap_or_else(|| usage_limit(&source.source_category).to_string());
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
            version_system(&source.source_id),
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
        let evidence_type = evidence_type(&source.source_category, &source.source_id, &block);
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
                block_id, source_id, section_id, source_title, source_url, revision_id,
                block_index, kind, tag, text, normalized_text, evidence_type, chapter_no
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                block.block_id,
                block.source_id,
                block.section_id,
                block.source_title,
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
                    version_system(&source.source_id),
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
    let existing_person_id: Option<String> = conn
        .query_row(
            "SELECT person_id FROM aliases WHERE alias = ?1",
            params![&alias],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing_person_id) = existing_person_id {
        if existing_person_id != person_id {
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
            "INSERT INTO aliases (alias, person_id, scope) VALUES (?1, ?2, ?3)",
            params![&alias, &person_id, scope],
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
        "sources": {
            "added": added,
            "removed": removed,
            "changed": changed,
            "unchanged_count": unchanged_count,
        },
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

fn seed_aliases(conn: &Connection) -> Result<()> {
    let people = [
        (
            "person:baoyu",
            "贾宝玉",
            "核心人物，通灵玉持有者。",
            &["宝玉", "寶玉", "宝二爷", "寳玉"][..],
        ),
        (
            "person:daiyu",
            "林黛玉",
            "核心人物，金陵十二钗之一。",
            &["黛玉", "林姑娘", "颦儿", "顰兒"][..],
        ),
        (
            "person:baochai",
            "薛宝钗",
            "核心人物，金陵十二钗之一。",
            &["宝钗", "寶釵", "宝姐姐", "薛姑娘"][..],
        ),
        (
            "person:wangxifeng",
            "王熙凤",
            "贾府管家人物。",
            &["凤姐", "鳳姐", "凤姐儿", "璉二奶奶"][..],
        ),
        (
            "person:jiazheng",
            "贾政",
            "贾宝玉之父。",
            &["贾政", "賈政"][..],
        ),
        (
            "person:jiamu",
            "贾母",
            "贾府长辈。",
            &["贾母", "賈母", "老太太"][..],
        ),
        (
            "person:wangfuren",
            "王夫人",
            "贾宝玉之母。",
            &["王夫人", "太太"][..],
        ),
        (
            "person:xiren",
            "袭人",
            "贾宝玉身边丫鬟。",
            &["袭人", "襲人"][..],
        ),
        ("person:qingwen", "晴雯", "贾宝玉身边丫鬟。", &["晴雯"][..]),
        (
            "person:xiangyun",
            "史湘云",
            "金陵十二钗之一。",
            &["湘云", "湘雲", "云妹妹"][..],
        ),
        (
            "person:tanchun",
            "贾探春",
            "金陵十二钗之一。",
            &["探春", "三姑娘"][..],
        ),
        (
            "person:yuanchun",
            "贾元春",
            "金陵十二钗之一。",
            &["元春", "元妃"][..],
        ),
        (
            "person:yingchun",
            "贾迎春",
            "金陵十二钗之一。",
            &["迎春", "二姑娘"][..],
        ),
        (
            "person:xichun",
            "贾惜春",
            "金陵十二钗之一。",
            &["惜春", "四姑娘"][..],
        ),
        (
            "person:qiaojie",
            "巧姐",
            "金陵十二钗之一。",
            &["巧姐", "巧姐儿"][..],
        ),
        (
            "person:liwan",
            "李纨",
            "金陵十二钗之一。",
            &["李纨", "李紈", "宫裁", "宮裁"][..],
        ),
        ("person:miaoyu", "妙玉", "金陵十二钗之一。", &["妙玉"][..]),
        (
            "person:keqing",
            "秦可卿",
            "金陵十二钗之一。",
            &["秦可卿", "可卿"][..],
        ),
    ];
    for (person_id, name, description, aliases) in people {
        conn.execute(
            "INSERT INTO people (person_id, canonical_name, description) VALUES (?1, ?2, ?3)",
            params![person_id, name, description],
        )?;
        for alias in aliases {
            conn.execute(
                "INSERT INTO aliases (alias, person_id, scope) VALUES (?1, ?2, ?3)",
                params![alias, person_id, "v1_seed_alias"],
            )?;
        }
    }
    Ok(())
}

pub fn create_evidence_package(
    conn: &Connection,
    trace_id: &str,
    question: &str,
    cards: Vec<EvidenceCard>,
) -> Result<EvidencePackage> {
    let claims = claims_from_cards(question, &cards);
    let claim_evidence_map = claim_evidence_map(&claims, &cards);
    let review = review(question, &cards, &claims);
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
        }),
    )?;
    Ok(EvidencePackage {
        package_id,
        trace_id: trace_id.to_string(),
        question: question.to_string(),
        cards,
        claims,
        claim_evidence_map,
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
                forbidden_conclusions: cards
                    .iter()
                    .map(|card| card.unsupported_scope.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
            })
            .collect()
    };
    Ok(Some(EvidencePackage {
        package_id,
        trace_id,
        question,
        cards,
        claims,
        claim_evidence_map,
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
    let exact_terms = required_exact_terms(question)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut scored: BTreeMap<String, (i64, EvidenceCard)> = BTreeMap::new();
    let mut candidate_block_ids = BTreeSet::new();
    for term in &terms {
        for block in query_blocks_like(conn, term, limit * 4)? {
            candidate_block_ids.insert(block.block_id.clone());
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
    let mut cards = ranked.into_iter().map(|(_, card)| card).collect::<Vec<_>>();
    let mut seen = cards
        .iter()
        .map(|card| card.block_id.clone())
        .collect::<HashSet<_>>();
    for exact_term in &exact_terms {
        for block in query_blocks_exact_text(conn, exact_term, limit * 8)? {
            if !block.text.contains(exact_term) {
                continue;
            }
            candidate_block_ids.insert(block.block_id.clone());
            let card = evidence_card_from_block(block);
            if seen.insert(card.block_id.clone()) {
                cards.insert(0, card);
                break;
            }
        }
    }
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
    cards.truncate(limit.max(required_evidence_types.len()));
    Ok(SearchEvidenceResult {
        cards,
        expanded_terms: terms,
        expanded_aliases: extracted_query_terms.aliases,
        exact_terms,
        candidate_count: candidate_block_ids.len(),
    })
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
            let evidence_ids = cards
                .iter()
                .filter(|card| card.text.contains(term))
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
        usage_boundary: metadata_usage_boundary
            .or(usage_limit)
            .unwrap_or_else(|| usage_limit_for_unknown_source(source_id)),
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

fn usage_limit_for_unknown_source(source_id: &str) -> String {
    if source_id.contains("zhiyanzhai") || source_id.contains("jiaxu") {
        "只能作为脂批、版本或评语证据候选；不能单独证明正文事实。".to_string()
    } else {
        "可作为正文或版本对照证据候选；不声明完成学术校勘。".to_string()
    }
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
        "claim_evidence_map": &package.claim_evidence_map,
        "evidence_ids": evidence_ids,
        "cards": &package.cards,
        "review": &package.review,
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
    if cards.is_empty() {
        return vec!["当前知识库未找到可追溯证据，不能给出确定结论。".to_string()];
    }
    let mut claims = Vec::new();
    if question.contains("通灵玉") || question.contains("通靈玉") {
        claims.push("通灵玉相关回答必须回到第八回等具体文本证据，并区分正文与脂批。".to_string());
    }
    if cards.iter().any(|card| card.evidence_type == "commentary") {
        claims.push("命中的脂批材料只能作为脂批或版本线索，不能当作正文事实。".to_string());
    }
    if cards.iter().any(|card| card.evidence_type == "base_text") {
        claims.push("命中的正文材料可支持相应版本和位置中的直接文本事实。".to_string());
    }
    if claims.is_empty() {
        claims.push("回答只能在已命中证据的支持范围内表述。".to_string());
    }
    claims
}

pub fn review(question: &str, cards: &[EvidenceCard], claims: &[String]) -> ReviewRecord {
    let mut issues = Vec::new();
    for control in blocked_prompt_controls(question) {
        issues.push(format!("用户请求包含受控内部流程绕过企图：{control}。"));
    }
    if cards.is_empty() {
        issues.push("未命中可追溯证据，必须返回证据不足。".to_string());
    }
    let asks_commentary_material =
        question.contains("脂批") || question.contains("脂評") || question.contains("甲戌");
    let asks_body_text_fact = question.contains("正文")
        || question.contains("情节")
        || (!asks_commentary_material && question.contains("原文"));
    if cards.iter().all(|card| card.evidence_type == "commentary") && asks_body_text_fact {
        issues.push("当前证据全为脂批，不能回答为正文直接事实。".to_string());
    }
    if (question.contains("结局") || question.contains("命运"))
        && !cards.iter().any(|card| card.evidence_type == "base_text")
    {
        issues.push("人物命运问题缺少正文证据，必须标注限制。".to_string());
    }
    if (question.contains("嫁给")
        || question.contains("北静王")
        || question.contains("北靜王")
        || question.contains("断定")
        || question.contains("必然")
        || question.contains("一定"))
        && cards.iter().all(|card| {
            !card.text.contains("北静王")
                && !card.text.contains("北靜王")
                && !card.text.contains("嫁")
                && !card.text.contains("断定")
        })
    {
        issues.push("问题含高风险结论或过度断言，当前证据不能支持确定表述。".to_string());
    }
    if question.contains("量子")
        || question.contains("现代程序员")
        || question.contains("程序员")
        || question.to_lowercase().contains("modern programmer")
    {
        issues.push("问题含现代外部概念，当前资料不能作为可追溯证据支持。".to_string());
    }
    if question.contains("内部配置")
        || question.contains("系统提示词")
        || question.to_lowercase().contains("system prompt")
    {
        issues.push("请求涉及内部配置或系统提示词，必须拒绝泄露。".to_string());
    }
    if asks_commentary_material && !cards.iter().any(|card| card.evidence_type == "commentary") {
        issues.push("脂批或甲戌相关问题缺少脂批证据，必须标注限制。".to_string());
    }
    if (question.contains("程甲")
        || question.contains("程乙")
        || question.contains("版本")
        || question.contains("前八十")
        || question.contains("后四十")
        || question.contains("後四十"))
        && !cards.iter().any(|card| {
            card.evidence_type == "version_note"
                || card.source_id.contains("chengjia")
                || card.source_id.contains("chengyi")
        })
    {
        issues.push("版本边界问题缺少版本证据，必须标注限制。".to_string());
    }
    if question.contains("影印")
        || question.contains("校注")
        || question.contains("校勘")
        || question.contains("专家")
        || question.contains("權威")
        || question.contains("权威")
    {
        issues.push(
            "source coverage boundary 缺少影印件、权威校注本或专家校勘复核，必须降级为资料不足。"
                .to_string(),
        );
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
        return format!(
            "证据不足：当前第一批 Wikisource source snapshot 没有命中可追溯证据，不能仅凭模型记忆回答。\n\n证据包：{}\nreviewer：{}",
            package.package_id, package.review.summary
        );
    }
    let mut answer = String::new();
    answer.push_str("根据当前第一批 Wikisource source snapshot，只能作如下有边界的回答：\n\n");
    if question.contains("通灵玉") || question.contains("通靈玉") || question.contains("莫失莫忘")
    {
        answer.push_str("通灵玉相关文本需要以第八回等具体 block 为依据；若涉及铭文，命中的证据显示“莫失莫忘，仙寿恒昌”等字样。不同来源可能记录字形或图式细节差异，不能把本批 snapshot 视为影印校勘完成。\n\n");
    } else {
        answer.push_str("已命中若干正文、脂批或版本证据。下面列出最靠前的证据，回答只能在这些证据的支持范围内成立。\n\n");
    }
    for (index, card) in package.cards.iter().take(4).enumerate() {
        answer.push_str(&format!(
            "{}. [{}] {}：{}\n   来源：{}；revision_id={:?}\n   不支持：{}\n",
            index + 1,
            card.evidence_level,
            card.source_title,
            card.text,
            card.source_id,
            card.revision_id,
            card.unsupported_scope
        ));
    }
    answer.push_str(&format!(
        "\n证据包：{}\nreviewer：{}",
        package.package_id, package.review.summary
    ));
    answer
}

pub fn enforce_review(draft: String, package: &EvidencePackage) -> String {
    if package.review.status == "passed" {
        return draft;
    }
    format!(
        "证据不足或需要降级：{}\n\n{}\n\n证据包：{}",
        package.review.issues.join("；"),
        local_answer(&package.question, package),
        package.package_id
    )
}

fn claim_evidence_map(claims: &[String], cards: &[EvidenceCard]) -> Vec<ClaimEvidenceMap> {
    claims
        .iter()
        .enumerate()
        .map(|(claim_index, claim)| {
            let evidence_ids = cards
                .iter()
                .filter(|card| {
                    if claim.contains("脂批") {
                        card.evidence_type == "commentary"
                    } else if claim.contains("正文") || claim.contains("通灵玉") {
                        card.evidence_type == "base_text" || card.evidence_type == "version_note"
                    } else {
                        true
                    }
                })
                .map(|card| card.evidence_id.clone())
                .collect::<Vec<_>>();
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
    source_url: String,
    text: String,
}

#[derive(Debug, Clone)]
struct SearchEvidenceResult {
    cards: Vec<EvidenceCard>,
    expanded_terms: Vec<String>,
    expanded_aliases: Vec<String>,
    exact_terms: Vec<String>,
    candidate_count: usize,
}

#[derive(Debug, Clone)]
struct ExtractedQueryTerms {
    terms: Vec<String>,
    aliases: Vec<String>,
}

fn required_exact_terms(question: &str) -> Vec<&'static str> {
    let mut terms = Vec::new();
    let normalized = normalize_text(question);
    if normalized.contains("通灵玉") && normalized.contains('字') {
        terms.push("莫失莫忘");
        terms.push("一除邪祟");
    }
    if normalized.contains("青埂") {
        terms.push("青埂");
    }
    if question.contains("寳玉") {
        terms.push("寳玉");
    }
    if question.contains("寳釵") {
        terms.push("寳釵");
    }
    terms
}

fn extract_query_terms(conn: &Connection, question: &str) -> Result<ExtractedQueryTerms> {
    let mut terms = Vec::new();
    let mut aliases = Vec::new();
    let normalized = normalize_text(question);
    let seed_terms = [
        ("通灵玉", "通靈玉"),
        ("通灵宝玉", "通靈寶玉"),
        ("莫失莫忘", "莫失莫忘"),
        ("仙寿恒昌", "仙壽恒昌"),
        ("一除邪祟", "一除邪祟"),
        ("二疗冤疾", "二療冤疾"),
        ("三知祸福", "三知禍福"),
        ("石头", "石頭"),
        ("顽石", "頑石"),
        ("寳玉", "寳玉"),
        ("青埂峰", "青埂峰"),
        ("金陵十二钗", "金陵十二釵"),
        ("判词", "判詞"),
        ("葬花", "葬花"),
        ("好了歌", "好了歌"),
        ("太虚幻境", "太虛幻境"),
        ("脂批", "脂批"),
        ("甲戌", "甲戌"),
        ("程甲", "程甲"),
        ("程乙", "程乙"),
        ("前八十回", "前八十回"),
        ("后四十回", "後四十回"),
        ("第八十一回", "第八十一回"),
        ("宝玉", "寶玉"),
        ("黛玉", "黛玉"),
        ("宝钗", "寶釵"),
        ("凤姐", "鳳姐"),
        ("贾母", "賈母"),
        ("袭人", "襲人"),
        ("李纨", "李紈"),
        ("女娲", "女媧"),
        ("补天", "補天"),
        ("甄士隐", "甄士隱"),
        ("贾雨村", "賈雨村"),
        ("冷子兴", "冷子興"),
        ("刘姥姥", "劉姥姥"),
        ("大观园", "大觀園"),
        ("怡红院", "怡紅院"),
        ("潇湘馆", "瀟湘館"),
        ("蘅芜苑", "蘅蕪苑"),
        ("荣国府", "榮國府"),
        ("宁国府", "寧國府"),
        ("贾府", "賈府"),
        ("薛蟠", "薛蟠"),
        ("香菱", "香菱"),
        ("平儿", "平兒"),
        ("尤氏", "尤氏"),
        ("贾琏", "賈璉"),
        ("秦钟", "秦鐘"),
        ("北静王", "北靜王"),
        ("金陵", "金陵"),
        ("红楼梦", "紅樓夢"),
        ("风月宝鉴", "風月寶鑒"),
        ("芙蓉女儿", "芙蓉女兒"),
        ("桃花社", "桃花社"),
        ("海棠", "海棠"),
        ("菊花", "菊花"),
        ("灯谜", "燈謎"),
        ("省亲", "省親"),
        ("第八回", "第八回"),
        ("第一回", "第一回"),
        ("脂砚斋", "脂硯齋"),
    ];
    for (simple, traditional) in seed_terms {
        if question.contains(simple)
            || question.contains(traditional)
            || normalized.contains(&normalize_text(simple))
        {
            push_term(&mut terms, simple);
            push_term(&mut terms, traditional);
        }
    }
    let asks_inscription = question.contains('字')
        || question.contains("铭")
        || question.contains("銘")
        || question.contains("写")
        || question.contains("寫");
    let asks_tonglingyu =
        question.contains("通灵玉") || question.contains("通靈玉") || normalized.contains("通灵玉");
    if asks_inscription && asks_tonglingyu {
        for term in [
            "莫失莫忘",
            "仙寿恒昌",
            "仙壽恒昌",
            "一除邪祟",
            "二疗冤疾",
            "二療冤疾",
            "三知祸福",
            "三知禍福",
        ] {
            push_term(&mut terms, term);
        }
    }
    if question.contains("顽石") || question.contains("頑石") {
        push_term(&mut terms, "石頭");
        push_term(&mut terms, "石头");
    }
    if question.contains("后四十") || question.contains("後四十") {
        push_term(&mut terms, "第八十一回");
        push_term(&mut terms, "第081回");
        push_term(&mut terms, "八十一");
    }

    let mut stmt = conn.prepare("SELECT alias FROM aliases")?;
    let alias_rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for alias in alias_rows {
        let alias = alias?;
        if question.contains(&alias) || normalized.contains(&normalize_text(&alias)) {
            push_term(&mut terms, &alias);
            push_term(&mut aliases, &alias);
        }
    }

    for token in cjk_tokens(question) {
        if token.chars().count() >= 2 && token.chars().count() <= 8 {
            push_term(&mut terms, &token);
        }
    }
    if terms.is_empty() && question.chars().count() <= 24 {
        push_term(&mut terms, question);
    }
    Ok(ExtractedQueryTerms { terms, aliases })
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
        SELECT block_id, kind, revision_id, source_id, source_title, source_url, text
        FROM blocks
        WHERE text LIKE ?1 ESCAPE '\'
           OR source_title LIKE ?1 ESCAPE '\'
           OR normalized_text LIKE ?2 ESCAPE '\'
        ORDER BY
          CASE evidence_type
            WHEN 'base_text' THEN 1
            WHEN 'commentary' THEN 2
            WHEN 'version_note' THEN 3
            ELSE 4
          END,
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
            source_url: row.get(5)?,
            text: row.get(6)?,
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
        SELECT block_id, kind, revision_id, source_id, source_title, source_url, text
        FROM blocks
        WHERE text LIKE ?1 ESCAPE '\'
        ORDER BY
          CASE
            WHEN source_id = 'hongloumeng-wikisource-120' THEN 1
            WHEN source_id LIKE '%chengjia%' THEN 2
            WHEN source_id LIKE '%chengyi%' THEN 3
            ELSE 4
          END,
          LENGTH(text) ASC
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map(params![like, limit as i64], |row| {
        Ok(SearchBlockRecord {
            block_id: row.get(0)?,
            kind: row.get(1)?,
            revision_id: row.get(2)?,
            source_id: row.get(3)?,
            source_title: row.get(4)?,
            source_url: row.get(5)?,
            text: row.get(6)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn evidence_card_from_block(block: SearchBlockRecord) -> EvidenceCard {
    evidence_card_from_block_text(block, None)
}

fn evidence_card_from_block_with_focus(block: SearchBlockRecord, focus: &str) -> EvidenceCard {
    evidence_card_from_block_text(block, Some(focus))
}

fn evidence_card_from_block_text(block: SearchBlockRecord, focus: Option<&str>) -> EvidenceCard {
    let evidence_type =
        if block.source_id.contains("zhiyanzhai") || block.source_id.contains("jiaxu") {
            "commentary"
        } else if block.text.contains("程甲")
            || block.text.contains("程乙")
            || block.text.contains("脂評本")
        {
            "version_note"
        } else {
            "base_text"
        };
    let (support_scope, unsupported_scope, evidence_level, confidence) = match evidence_type {
        "commentary" => (
            "可支持脂批、评语或版本线索层面的说明；必须标注为脂批来源。".to_string(),
            "不能单独证明正文事实，也不能扩展为所有版本共同结论。".to_string(),
            "脂批提示".to_string(),
            "medium".to_string(),
        ),
        "version_note" => (
            "可支持版本边界、整理来源或版本系统说明。".to_string(),
            "不能单独证明情节事实，不能替代影印或权威校注本校勘。".to_string(),
            "版本边界".to_string(),
            "medium".to_string(),
        ),
        _ => (
            "可支持该版本该 block 中直接出现的原文事实或文本定位。".to_string(),
            "不能证明未出现的情节、人物命运定论或其他版本必然相同。".to_string(),
            "正文直接".to_string(),
            "high".to_string(),
        ),
    };
    EvidenceCard {
        evidence_id: format!("ev-{}", uuid::Uuid::now_v7().simple()),
        evidence_type: evidence_type.to_string(),
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
    let mut score = 1;
    if block.text.contains(term) {
        score += 10;
    }
    if normalize_text(&block.text).contains(&normalize_text(term)) {
        score += 8;
    }
    if block.source_title.contains(term) {
        score += 5;
    }
    if question.contains("脂批")
        && (block.source_id.contains("zhiyanzhai") || block.source_id.contains("jiaxu"))
    {
        score += 8;
    }
    if question.contains("程甲") && block.source_id.contains("chengjia") {
        score += 40;
    }
    if question.contains("程乙") && block.source_id.contains("chengyi") {
        score += 40;
    }
    if block.kind == "heading" {
        score -= 2;
    }
    let asks_inscription = question.contains('字')
        || question.contains("铭")
        || question.contains("銘")
        || question.contains("写")
        || question.contains("寫");
    let looks_like_inscription = block.text.contains("莫失莫忘")
        || block.text.contains("仙壽")
        || block.text.contains("仙寿")
        || block.text.contains("一除邪祟")
        || block.text.contains("二療冤疾")
        || block.text.contains("二疗冤疾")
        || block.text.contains("三知禍福")
        || block.text.contains("三知祸福");
    if asks_inscription && looks_like_inscription {
        score += 50;
    } else if (term.contains("通灵") || term.contains("通靈")) && looks_like_inscription {
        score += 20;
    }
    score
}

fn evidence_type(source_category: &str, source_id: &str, block: &BlockRecord) -> &'static str {
    if source_category == "commentary_material"
        || source_id.contains("zhiyanzhai")
        || source_id.contains("jiaxu")
    {
        "commentary"
    } else if block.text.contains("程甲")
        || block.text.contains("程乙")
        || block.text.contains("脂評")
        || block.text.contains("版本")
    {
        "version_note"
    } else {
        "base_text"
    }
}

fn normalize_text(input: &str) -> String {
    let replacements = [
        ("紅", "红"),
        ("樓", "楼"),
        ("夢", "梦"),
        ("寶", "宝"),
        ("寳", "宝"),
        ("賈", "贾"),
        ("襲", "袭"),
        ("紈", "纨"),
        ("媧", "娲"),
        ("隱", "隐"),
        ("興", "兴"),
        ("劉", "刘"),
        ("觀", "观"),
        ("園", "园"),
        ("院", "院"),
        ("瀟", "潇"),
        ("館", "馆"),
        ("蕪", "芜"),
        ("榮", "荣"),
        ("國", "国"),
        ("寧", "宁"),
        ("兒", "儿"),
        ("璉", "琏"),
        ("鐘", "钟"),
        ("靜", "静"),
        ("鑒", "鉴"),
        ("補", "补"),
        ("燈", "灯"),
        ("親", "亲"),
        ("鎖", "锁"),
        ("玉寶靈通", "玉宝灵通"),
        ("靈", "灵"),
        ("釵", "钗"),
        ("鳳", "凤"),
        ("壽", "寿"),
        ("恆", "恒"),
        ("恒", "恒"),
        ("僊", "仙"),
        ("癒", "愈"),
        ("療", "疗"),
        ("禍", "祸"),
        ("硯", "砚"),
        ("齋", "斋"),
        ("評", "评"),
        ("衆", "众"),
        ("眾", "众"),
        ("裏", "里"),
        ("裡", "里"),
        ("説", "说"),
        ("說", "说"),
        ("冩", "写"),
        ("臺", "台"),
        ("檯", "台"),
        ("後", "后"),
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

fn version_system(source_id: &str) -> &'static str {
    if source_id.contains("chengjia") {
        "程甲本"
    } else if source_id.contains("chengyi") {
        "程乙本"
    } else if source_id.contains("jiaxu") {
        "甲戌本脂评"
    } else if source_id.contains("zhiyanzhai") {
        "脂砚斋重评整理资料"
    } else {
        "Wikisource 120回汇校本"
    }
}

fn usage_limit(source_category: &str) -> &'static str {
    if source_category == "commentary_material" {
        "只能作为脂批、版本或评语证据候选；不能单独证明正文事实。"
    } else {
        "可作为正文或版本对照证据候选；不声明完成学术校勘。"
    }
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
    let after_di = title.split('第').nth(1)?;
    let value = after_di.split('回').next()?;
    if value.is_empty() {
        return None;
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return value.parse().ok();
    }
    chinese_number(value)
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
    let Some(byte_index) = text.find(focus) else {
        return trim_text(text, max_chars);
    };
    let focus_index = text[..byte_index].chars().count();
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
mod tests {
    use super::*;
    use agent_core::{RuntimeOutput, RuntimeRunInput, RuntimeSessionInput};

    #[derive(Debug, Default)]
    struct DraftRuntimeClient;

    #[derive(Debug, Default)]
    struct NoToolRuntimeClient;

    #[derive(Debug, Default)]
    struct BadOutputRefRuntimeClient;

    #[derive(Debug, Default)]
    struct IncompleteHermesContentRuntimeClient;

    #[derive(Debug, Default)]
    struct MissingToolAuditRuntimeClient;

    #[derive(Debug, Default)]
    struct WrongEvidenceOutputRefRuntimeClient;

    #[derive(Debug, Default)]
    struct FailingProfileRuntimeClient;

    #[derive(Debug)]
    struct SlowDraftRuntimeClient {
        active: Arc<std::sync::atomic::AtomicUsize>,
        max_active: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl SlowDraftRuntimeClient {
        fn new(
            active: Arc<std::sync::atomic::AtomicUsize>,
            max_active: Arc<std::sync::atomic::AtomicUsize>,
        ) -> Self {
            Self { active, max_active }
        }
    }

    #[async_trait]
    impl RuntimeClient for DraftRuntimeClient {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "draft runtime only supports profile steps",
            ))
        }

        async fn send_session_message(
            &self,
            _input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "draft runtime only supports profile steps",
            ))
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            let operation = input
                .runtime_step
                .as_ref()
                .and_then(|step| step.metadata.get("operation"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let message = input
                .messages
                .first()
                .map(|message| message.content.clone())
                .unwrap_or_default();
            let tool_rounds = if input.requested_tools.is_empty() {
                0
            } else {
                1
            };
            let package_id = package_id_from_step_message(&message);
            let tool_results = if input.requested_tools.is_empty() {
                json!([])
            } else {
                Value::Array(
                    input
                        .requested_tools
                        .iter()
                        .enumerate()
                        .map(|(index, tool_name)| {
                            let output_ref = if matches!(
                                tool_name.as_str(),
                                "tonglingyu.evidence.package.create"
                                    | "tonglingyu.evidence.package.read"
                                    | "tonglingyu.evidence.package.replay"
                            ) {
                                package_id
                                    .as_ref()
                                    .map(|package_id| {
                                        format!(
                                            "runtime://tonglingyu/{}/packages/{package_id}",
                                            input.trace_id
                                        )
                                    })
                                    .unwrap_or_else(|| {
                                        format!(
                                            "runtime://tonglingyu/{}/tools/{operation}/{index}",
                                            input.trace_id
                                        )
                                    })
                            } else if matches!(
                                tool_name.as_str(),
                                "tonglingyu.text.search" | "tonglingyu.commentary.search"
                            ) {
                                evidence_set_output_ref(
                                    &input.trace_id,
                                    &evidence_ids_from_step_message(&message),
                                )
                            } else {
                                format!(
                                    "runtime://tonglingyu/{}/tools/{operation}/{index}",
                                    input.trace_id
                                )
                            };
                            json!({
                                "call_id": format!("call-runtime-{operation}-{index}"),
                                "profile_id": input.profile_id,
                                "tool_name": tool_name,
                                "output_ref": output_ref,
                            })
                        })
                        .collect(),
                )
            };
            let tool_audit_events = if input.requested_tools.is_empty() {
                json!([])
            } else {
                Value::Array(
                    input
                        .requested_tools
                        .iter()
                        .map(|tool_name| {
                            json!({
                                "event": "runtime_tool_result",
                                "tool_name": tool_name,
                                "trace_id": input.trace_id,
                            })
                        })
                        .collect(),
                )
            };
            Ok(RuntimeOutput {
                result_summary: match operation {
                    "text_evidence_search" => serde_json::to_string(&json!({
                        "evidence_observation": {
                            "evidence_refs": evidence_ids_from_step_message(&message),
                            "evidence_analysis": "Hermes observed text evidence refs",
                            "unsupported_scope": "observation only; local runtime evidence is enforced",
                        }
                    }))
                    .expect("text evidence output serializes"),
                    "commentary_evidence_search" => serde_json::to_string(&json!({
                        "evidence_observation": {
                            "commentary_refs": evidence_ids_from_step_message(&message),
                            "commentary_analysis": "Hermes observed commentary evidence refs",
                            "base_text_limits": "commentary cannot prove base-text facts alone",
                        }
                    }))
                    .expect("commentary evidence output serializes"),
                    "draft_answer" => serde_json::to_string(&json!({
                        "draft_candidate": {
                            "draft_answer": format!("Hermes full workflow draft from {operation}. context={message}"),
                            "package_id": package_id_from_step_message(&message)
                                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                            "claim_statements": ["Hermes full workflow draft claim"],
                        }
                    }))
                    .expect("draft output serializes"),
                    "evidence_package_create" => serde_json::to_string(&json!({
                        "package_observation": {
                            "package_id": package_id_from_step_message(&message)
                                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                            "summary": "Hermes observed runtime package ref",
                        }
                    }))
                    .expect("package output serializes"),
                    "review_answer" => serde_json::to_string(&json!({
                        "review_observation": {
                            "review_status": "passed",
                            "severity": "none",
                            "issues": [],
                            "required_revisions": [],
                        }
                    }))
                    .expect("review output serializes"),
                    _ => format!("Hermes full workflow step {operation}. context={message}"),
                },
                result_ref: Some(format!(
                    "result://draft-runtime/{}/{}",
                    input.profile_id, operation
                )),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                    "operation": operation,
                    "tool_rounds": tool_rounds,
                    "tool_results": tool_results,
                    "tool_audit_events": tool_audit_events,
                }),
            })
        }
    }

    #[async_trait]
    impl RuntimeClient for NoToolRuntimeClient {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "no-tool runtime only supports profile steps",
            ))
        }

        async fn send_session_message(
            &self,
            _input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "no-tool runtime only supports profile steps",
            ))
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            let operation = input
                .runtime_step
                .as_ref()
                .and_then(|step| step.metadata.get("operation"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let message = input
                .messages
                .first()
                .map(|message| message.content.clone())
                .unwrap_or_default();
            Ok(RuntimeOutput {
                result_summary: match operation {
                    "text_evidence_search" => serde_json::to_string(&json!({
                        "evidence_observation": {
                            "evidence_refs": evidence_ids_from_step_message(&message),
                            "evidence_analysis": "Hermes observed text evidence refs without model tool calls",
                            "unsupported_scope": "observation only; local runtime evidence is enforced",
                        }
                    }))
                    .expect("text evidence output serializes"),
                    "commentary_evidence_search" => serde_json::to_string(&json!({
                        "evidence_observation": {
                            "commentary_refs": evidence_ids_from_step_message(&message),
                            "commentary_analysis": "Hermes observed commentary evidence refs without model tool calls",
                            "base_text_limits": "commentary cannot prove base-text facts alone",
                        }
                    }))
                    .expect("commentary evidence output serializes"),
                    "draft_answer" => serde_json::to_string(&json!({
                        "draft_candidate": {
                            "draft_answer": format!("Hermes full workflow draft from {operation}. context={message}"),
                            "package_id": package_id_from_step_message(&message)
                                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                            "claim_statements": ["Hermes full workflow draft claim"],
                        }
                    }))
                    .expect("draft output serializes"),
                    "evidence_package_create" => serde_json::to_string(&json!({
                        "package_observation": {
                            "package_id": package_id_from_step_message(&message)
                                .unwrap_or_else(|| "pkg-missing-from-step-output".to_string()),
                            "summary": "Hermes observed runtime package ref without model tool calls",
                        }
                    }))
                    .expect("package output serializes"),
                    "review_answer" => serde_json::to_string(&json!({
                        "review_observation": {
                            "review_status": "passed",
                            "severity": "none",
                            "issues": [],
                            "required_revisions": [],
                        }
                    }))
                    .expect("review output serializes"),
                    _ => format!("Hermes full workflow step {operation}. context={message}"),
                },
                result_ref: Some(format!("result://no-tool-runtime/{}", input.profile_id)),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                    "tool_rounds": 0,
                    "tool_results": [],
                    "tool_audit_events": [],
                }),
            })
        }
    }

    #[async_trait]
    impl RuntimeClient for BadOutputRefRuntimeClient {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "bad-output-ref runtime only supports profile steps",
            ))
        }

        async fn send_session_message(
            &self,
            _input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "bad-output-ref runtime only supports profile steps",
            ))
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            let tool_results = Value::Array(
                input
                    .requested_tools
                    .iter()
                    .enumerate()
                    .map(|(index, tool_name)| {
                        json!({
                            "call_id": format!("call-bad-output-ref-{index}"),
                            "profile_id": input.profile_id,
                            "tool_name": tool_name,
                            "output_ref": format!("runtime://tool-results/{index}"),
                        })
                    })
                    .collect(),
            );
            Ok(RuntimeOutput {
                result_summary: "{}".to_string(),
                result_ref: Some(format!(
                    "result://bad-output-ref-runtime/{}",
                    input.profile_id
                )),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                    "tool_rounds": 1,
                    "tool_results": tool_results,
                    "tool_audit_events": [],
                }),
            })
        }
    }

    #[async_trait]
    impl RuntimeClient for IncompleteHermesContentRuntimeClient {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "incomplete-hermes-content runtime only supports profile steps",
            ))
        }

        async fn send_session_message(
            &self,
            _input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "incomplete-hermes-content runtime only supports profile steps",
            ))
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            let operation = input
                .runtime_step
                .as_ref()
                .and_then(|step| step.metadata.get("operation"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let message = input
                .messages
                .first()
                .map(|message| message.content.clone())
                .unwrap_or_default();
            let package_id = package_id_from_step_message(&message);
            let tool_results = Value::Array(
                input
                    .requested_tools
                    .iter()
                    .enumerate()
                    .map(|(index, tool_name)| {
                        let output_ref = if matches!(
                            tool_name.as_str(),
                            "tonglingyu.text.search" | "tonglingyu.commentary.search"
                        ) {
                            evidence_set_output_ref(
                                &input.trace_id,
                                &evidence_ids_from_step_message(&message),
                            )
                        } else if matches!(
                            tool_name.as_str(),
                            "tonglingyu.evidence.package.create"
                                | "tonglingyu.evidence.package.read"
                                | "tonglingyu.evidence.package.replay"
                        ) {
                            package_id
                                .as_ref()
                                .map(|package_id| {
                                    format!(
                                        "runtime://tonglingyu/{}/packages/{package_id}",
                                        input.trace_id
                                    )
                                })
                                .unwrap_or_else(|| {
                                    format!(
                                        "runtime://tonglingyu/{}/tools/{operation}/{index}",
                                        input.trace_id
                                    )
                                })
                        } else {
                            format!(
                                "runtime://tonglingyu/{}/tools/{operation}/{index}",
                                input.trace_id
                            )
                        };
                        json!({
                            "call_id": format!("call-incomplete-hermes-{operation}-{index}"),
                            "profile_id": input.profile_id,
                            "tool_name": tool_name,
                            "output_ref": output_ref,
                        })
                    })
                    .collect(),
            );
            Ok(RuntimeOutput {
                result_summary: "{}".to_string(),
                result_ref: Some(format!(
                    "result://incomplete-hermes-content/{}",
                    input.profile_id
                )),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                    "operation": operation,
                    "tool_rounds": if input.requested_tools.is_empty() { 0 } else { 1 },
                    "tool_results": tool_results,
                    "tool_audit_events": [],
                }),
            })
        }
    }

    #[async_trait]
    impl RuntimeClient for MissingToolAuditRuntimeClient {
        async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            DraftRuntimeClient.execute_run(input).await
        }

        async fn send_session_message(
            &self,
            input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            DraftRuntimeClient.send_session_message(input).await
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            let mut output = DraftRuntimeClient.execute_profile_step(input).await?;
            output.metadata["tool_audit_events"] = json!([]);
            Ok(output)
        }
    }

    #[async_trait]
    impl RuntimeClient for WrongEvidenceOutputRefRuntimeClient {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "wrong-evidence-output-ref runtime only supports profile steps",
            ))
        }

        async fn send_session_message(
            &self,
            _input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "wrong-evidence-output-ref runtime only supports profile steps",
            ))
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            let tool_results = Value::Array(
                input
                    .requested_tools
                    .iter()
                    .enumerate()
                    .map(|(index, tool_name)| {
                        let output_ref = if matches!(
                            tool_name.as_str(),
                            "tonglingyu.text.search" | "tonglingyu.commentary.search"
                        ) {
                            format!("runtime://tonglingyu/{}/evidence/wrong-set", input.trace_id)
                        } else {
                            format!("runtime://tonglingyu/{}/tools/{index}", input.trace_id)
                        };
                        json!({
                            "call_id": format!("call-wrong-evidence-output-ref-{index}"),
                            "profile_id": input.profile_id,
                            "tool_name": tool_name,
                            "output_ref": output_ref,
                        })
                    })
                    .collect(),
            );
            Ok(RuntimeOutput {
                result_summary: "{}".to_string(),
                result_ref: Some(format!(
                    "result://wrong-evidence-output-ref-runtime/{}",
                    input.profile_id
                )),
                messages: Vec::new(),
                metadata: json!({
                    "runtime_profile": input.profile_id,
                    "trace_id": input.trace_id,
                    "tool_rounds": 1,
                    "tool_results": tool_results,
                    "tool_audit_events": [],
                }),
            })
        }
    }

    #[async_trait]
    impl RuntimeClient for FailingProfileRuntimeClient {
        async fn execute_run(&self, _input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "failing-profile runtime only supports profile steps",
            ))
        }

        async fn send_session_message(
            &self,
            _input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::Conflict,
                "failing-profile runtime only supports profile steps",
            ))
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            Err(AgentCoreError::coded(
                ErrorCode::InternalError,
                format!("profile {} backend unavailable", input.profile_id),
            ))
        }
    }

    #[async_trait]
    impl RuntimeClient for SlowDraftRuntimeClient {
        async fn execute_run(&self, input: RuntimeRunInput) -> CoreResult<RuntimeOutput> {
            DraftRuntimeClient.execute_run(input).await
        }

        async fn send_session_message(
            &self,
            input: RuntimeSessionInput,
        ) -> CoreResult<RuntimeOutput> {
            DraftRuntimeClient.send_session_message(input).await
        }

        async fn execute_profile_step(
            &self,
            input: RuntimeProfileInput,
        ) -> CoreResult<RuntimeOutput> {
            use std::sync::atomic::Ordering;

            let runtime_step = input.runtime_step.clone();
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            let mut output = DraftRuntimeClient.execute_profile_step(input).await;
            self.active.fetch_sub(1, Ordering::SeqCst);
            if let (Ok(output), Some(runtime_step)) = (&mut output, runtime_step) {
                output.metadata["runtime_step"] = json!(runtime_step);
                output.metadata["runtime_step"]["status"] = json!("completed");
            }
            output
        }
    }

    fn step_output_from_message(message: &str) -> Option<Value> {
        message.lines().find_map(|line| {
            let value = line.strip_prefix("step_output_json: ")?;
            serde_json::from_str::<Value>(value).ok()
        })
    }

    fn package_id_from_step_message(message: &str) -> Option<String> {
        step_output_from_message(message)?
            .get("package_id")?
            .as_str()
            .map(ToOwned::to_owned)
    }

    fn evidence_ids_from_step_message(message: &str) -> Vec<String> {
        step_output_from_message(message)
            .and_then(|value| {
                value
                    .get("evidence_ids")
                    .and_then(Value::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect::<Vec<_>>()
                    })
            })
            .unwrap_or_default()
    }

    fn sample_card(evidence_type: &str) -> EvidenceCard {
        EvidenceCard {
            evidence_id: format!("ev-test-{evidence_type}"),
            evidence_type: evidence_type.to_string(),
            source_id: "test-source".to_string(),
            source_title: "test-title".to_string(),
            source_url: "https://example.test/source".to_string(),
            revision_id: Some(1),
            block_id: format!("block-test-{evidence_type}"),
            text: "脂批：测试证据".to_string(),
            support_scope: "测试支持范围".to_string(),
            unsupported_scope: "测试不支持范围".to_string(),
            evidence_level: "测试层级".to_string(),
            confidence: "medium".to_string(),
            verification_status: "test".to_string(),
        }
    }

    fn seed_retrieval_quality_source(conn: &Connection, snapshot_contract: Value) {
        let license = snapshot_text_field(
            &snapshot_contract,
            &["license", "license_id", "license_note", "licence", "rights"],
        );
        let license_url = snapshot_text_field(
            &snapshot_contract,
            &["license_url", "license_uri", "rights_url"],
        );
        let license_source_url = snapshot_text_field(
            &snapshot_contract,
            &[
                "license_source_url",
                "rights_source_url",
                "copyright_policy_url",
            ],
        );
        let attribution = snapshot_text_field(
            &snapshot_contract,
            &["attribution", "attribution_note", "citation"],
        );
        let usage_boundary = snapshot_text_field(
            &snapshot_contract,
            &["usage_boundary", "usage_limit", "source_usage_boundary"],
        );
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
                "quality-source",
                "base_material",
                "mediawiki",
                "质量测试红楼梦 source",
                "红楼梦",
                "测试底本；仅用于 RQA 单元测试",
                "zh",
                "https://example.test/source",
                "https://example.test/api",
                "2026-05-15T00:00:00Z",
                license,
                license_url,
                license_source_url,
                attribution,
                usage_boundary,
                "测试 source snapshot",
                serde_json::to_string(&snapshot_contract).expect("snapshot serializes"),
                "hash-quality-source",
            ],
        )
        .expect("insert source");
        conn.execute(
            "INSERT INTO version_notes (version_note_id, source_id, note, source_status, usage_limit) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "version-note:quality-source",
                "quality-source",
                "测试 source snapshot",
                "source_snapshot_ready",
                "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            ],
        )
        .expect("insert version note");
        conn.execute(
            r#"
            INSERT INTO blocks (
                block_id, source_id, section_id, source_title, source_url, revision_id,
                block_index, kind, tag, text, normalized_text, evidence_type, chapter_no
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
            params![
                "quality-block-001",
                "quality-source",
                "quality-section-001",
                "质量测试红楼梦/第一回",
                "https://example.test/source/1",
                1_i64,
                1_i64,
                "paragraph",
                Option::<String>::None,
                "通靈玉上写着莫失莫忘，仙壽恒昌。",
                normalize_text("通靈玉上写着莫失莫忘，仙壽恒昌。"),
                "base_text",
                1_i64,
            ],
        )
        .expect("insert block");
    }

    #[test]
    fn kb_schema_adds_source_usage_metadata_columns_to_existing_sources_table() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE sources (
                source_id TEXT PRIMARY KEY,
                source_category TEXT NOT NULL,
                format TEXT,
                title TEXT,
                work TEXT,
                edition TEXT,
                language TEXT,
                api_url TEXT,
                fetched_at TEXT,
                notes TEXT,
                snapshot_contract_json TEXT NOT NULL,
                source_hash TEXT NOT NULL
            );
            "#,
        )
        .expect("old sources table");

        init_knowledge_base_schema(&conn).expect("kb schema upgrades source metadata");

        let columns = conn
            .prepare("PRAGMA table_info(sources)")
            .expect("table info")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query columns")
            .collect::<std::result::Result<BTreeSet<_>, _>>()
            .expect("collect columns");
        for column in [
            "source_url",
            "license",
            "license_url",
            "license_source_url",
            "attribution",
            "usage_boundary",
        ] {
            assert!(columns.contains(column), "missing column {column}");
        }
    }

    #[test]
    fn kb_source_metadata_backfill_updates_legacy_sources_without_rebuild() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE sources (
                source_id TEXT PRIMARY KEY,
                source_category TEXT NOT NULL,
                format TEXT,
                title TEXT,
                work TEXT,
                edition TEXT,
                language TEXT,
                api_url TEXT,
                fetched_at TEXT,
                notes TEXT,
                snapshot_contract_json TEXT NOT NULL,
                source_hash TEXT NOT NULL
            );
            INSERT INTO sources (
                source_id, source_category, format, title, work, edition,
                language, api_url, fetched_at, notes, snapshot_contract_json,
                source_hash
            ) VALUES (
                'legacy-source', 'base_material', 'mediawiki', 'Legacy',
                'Work', 'Edition', 'zh', 'https://example.test/api',
                '2026-05-16T00:00:00Z', 'legacy row', '{}', 'hash-before'
            );
            CREATE TABLE evidence_packages (package_id TEXT PRIMARY KEY);
            INSERT INTO evidence_packages (package_id) VALUES ('pkg-before');
            "#,
        )
        .expect("legacy source row");
        let source_root = std::env::temp_dir().join(format!(
            "tonglingyu-source-backfill-{}",
            uuid::Uuid::now_v7().simple()
        ));
        let metadata_dir = source_root.join("legacy-source/metadata");
        fs::create_dir_all(&metadata_dir).expect("metadata dir");
        fs::write(
            metadata_dir.join("source.json"),
            serde_json::to_string(&json!({
                "source_id": "legacy-source",
                "source_category": "base_material",
                "format": "mediawiki",
                "title": "Legacy",
                "work": "Work",
                "edition": "Edition",
                "language": "zh",
                "source_url": "https://example.test/source",
                "api_url": "https://example.test/api",
                "fetched_at": "2026-05-16T00:00:00Z",
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://example.test/license",
                "attribution": "Example contributors",
                "usage_boundary": "test usage boundary",
                "notes": "metadata row",
                "snapshot_contract": {},
            }))
            .expect("source json"),
        )
        .expect("write source json");

        let report = backfill_source_metadata_from_snapshots(&conn, &source_root, true)
            .expect("backfill source metadata");

        assert_eq!(report["status"], "ok");
        assert_eq!(report["applied"], true);
        assert_eq!(report["updated_source_count"], 1);
        assert!(
            report["missing_columns_before"]
                .as_array()
                .expect("missing columns")
                .iter()
                .any(|value| value == "source_url")
        );
        let row = conn
            .query_row(
                "SELECT source_url, license, attribution, usage_boundary FROM sources WHERE source_id = 'legacy-source'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .expect("updated source metadata");
        assert_eq!(row.0, "https://example.test/source");
        assert_eq!(row.1, "CC-BY-SA-4.0");
        assert_eq!(row.2, "Example contributors");
        assert_eq!(row.3, "test usage boundary");
        let package_count = conn
            .query_row("SELECT count(*) FROM evidence_packages", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("package count");
        assert_eq!(package_count, 1);
        fs::remove_dir_all(source_root).ok();
    }

    #[test]
    fn text_search_required_types_respect_explicit_version_boundary_without_default_base() {
        let required = vec!["version_note".to_string()];

        let text_required = text_search_required_evidence_types(&required);

        assert_eq!(text_required, vec!["version_note".to_string()]);
        assert!(!text_required.contains(&"base_text".to_string()));
    }

    #[test]
    fn text_search_returns_production_ready_retrieval_quality_report() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        conn.execute(
            "INSERT INTO people (person_id, canonical_name, description) VALUES (?1, ?2, ?3)",
            params!["quality-person", "通灵玉", "RQA alias test"],
        )
        .expect("insert person");
        conn.execute(
            "INSERT INTO aliases (alias, person_id, scope) VALUES (?1, ?2, ?3)",
            params!["灵玉", "quality-person", "test"],
        )
        .expect("insert alias");
        let question = "灵玉 password=SECRET_RUNTIME_TOKEN_01234567890123456789";

        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: question.to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
            },
        )
        .expect("search executes");

        let TonglingyuToolOutput::EvidenceCards {
            cards,
            quality_report,
            ..
        } = output
        else {
            panic!("expected evidence cards");
        };
        assert_eq!(cards.len(), 1);
        assert_eq!(
            quality_report.schema_version,
            RETRIEVAL_QUALITY_REPORT_SCHEMA_VERSION
        );
        assert_eq!(quality_report.tool_name, "tonglingyu.text.search");
        assert_eq!(quality_report.candidate_count, 1);
        assert_eq!(quality_report.selected_count, 1);
        assert_eq!(quality_report.quality_status, "passed");
        assert!(quality_report.production_ready);
        assert!(!quality_report.truncated);
        assert_eq!(
            quality_report.channel_distribution.get("base_text"),
            Some(&1_usize)
        );
        assert_eq!(quality_report.expanded_aliases, vec!["灵玉".to_string()]);
        assert_eq!(quality_report.expected_evidence_hit, None);
        assert_eq!(
            quality_report.expected_evidence_status,
            "not_applicable_runtime_search"
        );
        assert_eq!(
            quality_report.evidence_type_coverage.selected,
            vec!["base_text".to_string()]
        );
        assert!(quality_report.evidence_type_coverage.missing.is_empty());
        assert!(!quality_report.query_summary.raw_question_included);
        assert_eq!(
            quality_report.source_usage_refs[0].metadata_status,
            "complete"
        );
        assert_eq!(
            quality_report.source_usage_refs[0].license.as_deref(),
            Some("CC-BY-SA-4.0")
        );
        assert_eq!(
            quality_report.source_usage_refs[0].license_url.as_deref(),
            Some("https://creativecommons.org/licenses/by-sa/4.0/")
        );
        let report_json = serde_json::to_string(&quality_report).expect("report serializes");
        assert!(!report_json.contains(question));
        assert!(!report_json.contains("SECRET_RUNTIME_TOKEN"));
    }

    #[test]
    fn redacted_query_terms_hash_sensitive_patterns() {
        for sensitive in [
            "password=SECRET_RUNTIME_TOKEN",
            "token=SECRET_RUNTIME_TOKEN",
            "api_key=SECRET_RUNTIME_TOKEN",
            "https://example.invalid/path?token=SECRET_RUNTIME_TOKEN",
            "reader@example.invalid",
            "+8613800138000",
            "ABCD1234EFGH5678IJKL9012",
        ] {
            let redacted = redacted_query_term(sensitive);
            assert!(redacted.starts_with("sha256:"), "{sensitive} -> {redacted}");
            assert!(!redacted.contains("SECRET_RUNTIME_TOKEN"));
            assert!(!redacted.contains("example.invalid"));
            assert!(!redacted.contains("13800138000"));
        }

        let question = concat!(
            "通灵玉 token=SECRET_RUNTIME_TOKEN ",
            "https://example.invalid/a?secret=SECRET_RUNTIME_TOKEN ",
            "reader@example.invalid +8613800138000"
        );
        let terms = redacted_terms_from_question(question);
        let rendered = terms.join(" ");
        assert!(terms.iter().any(|term| term.starts_with("sha256:")));
        assert!(rendered.contains("通灵玉"));
        for leaked in [
            "SECRET_RUNTIME_TOKEN",
            "example.invalid",
            "reader@example.invalid",
            "13800138000",
        ] {
            assert!(!rendered.contains(leaked));
        }
    }

    #[test]
    fn required_exact_terms_protect_core_eval_targets() {
        assert_eq!(
            required_exact_terms("通灵玉上的字是什么？"),
            vec!["莫失莫忘", "一除邪祟"]
        );
        assert_eq!(
            required_exact_terms("青埂峰和顽石在哪里出现？"),
            vec!["青埂"]
        );
    }

    #[test]
    fn exact_text_lookup_prefers_primary_source_snapshot() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_knowledge_base_schema(&conn).expect("kb schema");
        for source_id in [
            "hongloumeng-wikisource-chengjia",
            "hongloumeng-wikisource-120",
            "hongloumeng-wikisource-chengyi",
        ] {
            conn.execute(
                r#"
                INSERT INTO sources (
                    source_id, source_category, format, title, work, edition,
                    language, source_url, api_url, fetched_at,
                    snapshot_contract_json, source_hash
                ) VALUES (?1, 'base_material', 'mediawiki', ?1, '红楼梦',
                    'test', 'zh', 'https://example.test/source',
                    'https://example.test/api', '2026-05-15T00:00:00Z',
                    '{}', ?1)
                "#,
                params![source_id],
            )
            .expect("insert source");
        }
        for (block_id, source_id, text) in [
            (
                "chengjia-short",
                "hongloumeng-wikisource-chengjia",
                "青埂短文",
            ),
            (
                "primary-long",
                "hongloumeng-wikisource-120",
                "青埂峰下主要 source snapshot 证据，文字更长。",
            ),
            (
                "chengyi-short",
                "hongloumeng-wikisource-chengyi",
                "青埂短文",
            ),
        ] {
            conn.execute(
                r#"
                INSERT INTO blocks (
                    block_id, source_id, section_id, source_title, source_url,
                    revision_id, block_index, kind, tag, text, normalized_text,
                    evidence_type, chapter_no
                ) VALUES (?1, ?2, 'section', 'source title', 'https://example.test',
                    1, 1, 'paragraph', NULL, ?3, ?4, 'base_text', 1)
                "#,
                params![block_id, source_id, text, normalize_text(text)],
            )
            .expect("insert block");
        }

        let rows = query_blocks_exact_text(&conn, "青埂", 3).expect("query blocks");
        assert_eq!(rows[0].block_id, "primary-long");
    }

    #[test]
    fn retrieval_quality_report_blocks_production_without_source_usage_metadata() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "source_of_record": "raw MediaWiki wikitext plus revision metadata",
            }),
        );

        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: "通灵玉是什么？".to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
            },
        )
        .expect("search executes");

        let TonglingyuToolOutput::EvidenceCards { quality_report, .. } = output else {
            panic!("expected evidence cards");
        };
        assert_eq!(quality_report.quality_status, "needs_attention");
        assert!(!quality_report.production_ready);
        assert!(quality_report.issues.iter().any(|issue| {
            issue
                == "source_usage_metadata_incomplete:quality-source:missing_license_and_license_url_and_attribution_and_usage_boundary_metadata"
        }));
        assert!(quality_report.recommended_follow_up.iter().any(|item| {
            item == "add_machine_readable_source_license_usage_attribution_metadata"
        }));
    }

    #[test]
    fn retrieval_quality_report_fails_when_required_type_is_missing() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );

        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: "通灵玉是什么？".to_string(),
                limit: 2,
                required_evidence_types: vec!["commentary".to_string()],
            },
        )
        .expect("search executes");

        let TonglingyuToolOutput::EvidenceCards { quality_report, .. } = output else {
            panic!("expected evidence cards");
        };
        assert_eq!(quality_report.quality_status, "failed");
        assert!(!quality_report.production_ready);
        assert!(
            quality_report
                .issues
                .iter()
                .any(|issue| { issue == "missing_required_evidence_type:commentary" })
        );
    }

    #[test]
    fn retrieval_quality_report_fails_when_no_evidence_is_selected() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");

        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: "不存在的检索目标".to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
            },
        )
        .expect("search executes");

        let TonglingyuToolOutput::EvidenceCards {
            cards,
            quality_report,
            ..
        } = output
        else {
            panic!("expected evidence cards");
        };
        assert!(cards.is_empty());
        assert_eq!(quality_report.quality_status, "failed");
        assert!(!quality_report.production_ready);
        assert!(
            quality_report
                .issues
                .iter()
                .any(|issue| { issue == "no_evidence_selected" })
        );
        assert!(
            quality_report
                .issues
                .iter()
                .any(|issue| { issue == "missing_required_evidence_type:base_text" })
        );
    }

    #[test]
    fn retrieval_quality_report_fails_when_required_exact_term_is_missing() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );

        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: "寳玉和通灵玉是什么关系？".to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
            },
        )
        .expect("search executes");

        let TonglingyuToolOutput::EvidenceCards { quality_report, .. } = output else {
            panic!("expected evidence cards");
        };
        assert_eq!(quality_report.quality_status, "failed");
        assert!(!quality_report.production_ready);
        assert!(
            quality_report
                .exact_match_coverage
                .iter()
                .any(|coverage| { coverage.term == "寳玉" && !coverage.matched })
        );
        assert_eq!(quality_report.protected_terms, vec!["寳玉".to_string()]);
        assert!(
            quality_report
                .issues
                .iter()
                .any(|issue| { issue == "required_exact_term_not_selected:寳玉" })
        );
    }

    #[test]
    fn retrieval_failure_schema_migration_is_idempotent() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        let before = runtime_schema_migration_preflight(&conn).expect("preflight before schema");
        assert!(
            before["pending_migrations"]
                .as_array()
                .is_some_and(|items| items
                    .iter()
                    .any(|item| item.as_str() == Some(RETRIEVAL_FAILURE_SCHEMA_VERSION)))
        );
        assert!(
            before["pending_migrations"]
                .as_array()
                .is_some_and(|items| items
                    .iter()
                    .any(|item| item.as_str() == Some(RETRIEVAL_FAILURE_PRIVACY_MIGRATION)))
        );
        assert_eq!(before["contains_secret_values"], json!(false));
        assert_eq!(before["will_delete_runtime_data"], json!(false));

        init_runtime_schema(&conn).expect("runtime schema");
        init_runtime_schema(&conn).expect("runtime schema idempotent");

        let after = runtime_schema_migration_preflight(&conn).expect("preflight after schema");
        assert_eq!(after["pending_migrations"], json!([]));
        let migration_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE migration_id = ?1",
                params![RETRIEVAL_FAILURE_SCHEMA_VERSION],
                |row| row.get(0),
            )
            .expect("migration count");
        assert_eq!(migration_count, 1);
        assert!(sqlite_table_exists(&conn, "retrieval_failures").expect("table check"));
        let retrieval_failure_columns =
            sqlite_table_columns(&conn, "retrieval_failures").expect("retrieval failure columns");
        assert!(retrieval_failure_columns.contains("question_sha256"));
        assert!(retrieval_failure_columns.contains("question_summary"));
        assert!(retrieval_failure_columns.contains("redacted_question_excerpt"));
        assert!(retrieval_failure_columns.contains("redacted_query_terms_json"));
        assert!(!retrieval_failure_columns.contains("question"));
        assert!(sqlite_table_exists(&conn, "knowledge_governance_tasks").expect("table check"));
        assert!(sqlite_table_exists(&conn, "knowledge_patch_proposals").expect("table check"));
    }

    #[test]
    fn governance_task_schema_migrates_legacy_failure_only_tasks() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            r#"
            CREATE TABLE retrieval_failures (
                failure_id TEXT PRIMARY KEY,
                trace_id TEXT NOT NULL,
                package_id TEXT,
                question_sha256 TEXT NOT NULL,
                question_char_count INTEGER NOT NULL,
                question_summary TEXT NOT NULL,
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
            INSERT INTO retrieval_failures (
                failure_id, trace_id, package_id, question_sha256,
                question_char_count, question_summary, kb_schema_version,
                kb_version_id, failure_type, redacted_query_terms_json,
                required_evidence_types_json, actual_evidence_types_json,
                expected_evidence_ids_json, selected_evidence_ids_json,
                missing_evidence_types_json, quality_issues_json,
                agent_diagnosis, proposed_fix, human_review_status, reviewer,
                review_note, created_at, updated_at, resolved_at
            ) VALUES (
                'rf-legacy', 'trace-legacy', 'pkg-legacy',
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                12, 'legacy question', 'tonglingyu-kb-v1', NULL,
                'expected_evidence_missing', '[]', '[]', '[]', '[]', '[]',
                '[]', '[]', NULL, 'review legacy failure', 'open', NULL,
                NULL, '2026-05-15T00:00:00Z',
                '2026-05-15T00:00:00Z', NULL
            );
            CREATE TABLE knowledge_governance_tasks (
                task_id TEXT PRIMARY KEY,
                source_failure_id TEXT NOT NULL,
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
            INSERT INTO knowledge_governance_tasks (
                task_id, source_failure_id, trace_id, package_id, task_type,
                status, priority, agent_cluster_key, proposed_fix, reviewer,
                review_note, evidence_ref, created_at, updated_at, accepted_at,
                closed_at
            ) VALUES (
                'kgt-legacy', 'rf-legacy', 'trace-legacy', 'pkg-legacy',
                'expected_evidence_fix', 'open', 'p0', 'rf:legacy',
                'review legacy failure', NULL, NULL, NULL,
                '2026-05-15T00:00:00Z', '2026-05-15T00:00:00Z', NULL, NULL
            );
            "#,
        )
        .expect("legacy governance task schema");

        init_runtime_schema(&conn).expect("runtime schema migrates legacy governance tasks");
        let failure_columns =
            sqlite_table_columns(&conn, "retrieval_failures").expect("failure table columns");
        assert!(failure_columns.contains("redacted_question_excerpt"));
        assert!(!failure_columns.contains("question"));
        let migrated_excerpt: String = conn
            .query_row(
                "SELECT redacted_question_excerpt FROM retrieval_failures WHERE failure_id = 'rf-legacy'",
                [],
                |row| row.get(0),
            )
            .expect("migrated excerpt");
        assert_eq!(migrated_excerpt, "legacy question");
        let columns =
            sqlite_table_columns(&conn, "knowledge_governance_tasks").expect("table columns");
        assert!(columns.contains("source_entity_type"));
        assert!(columns.contains("source_entity_id"));
        let task = load_governance_task(&conn, "kgt-legacy")
            .expect("load migrated task")
            .expect("migrated task exists");
        assert_eq!(task.source_failure_id.as_deref(), Some("rf-legacy"));
        assert_eq!(task.source_entity_type, "retrieval_failure");
        assert_eq!(task.source_entity_id, "rf-legacy");

        let trace_task = create_governance_task(
            &conn,
            KnowledgeGovernanceTaskCreateInput {
                source_entity_type: "trace".to_string(),
                source_entity_id: "trace-after-legacy-migration".to_string(),
                trace_id: "trace-after-legacy-migration".to_string(),
                package_id: None,
                source_failure_id: None,
                task_type: "expert_review".to_string(),
                priority: Some("p0".to_string()),
                proposed_fix: Some("request expert review".to_string()),
                agent_cluster_key: None,
            },
        )
        .expect("create trace task after migration");
        assert_eq!(trace_task.source_entity_type, "trace");
    }

    #[test]
    fn runtime_schema_rolls_back_failed_migration_batch() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute(
            "CREATE TABLE retrieval_failures (failure_id TEXT PRIMARY KEY)",
            [],
        )
        .expect("create incompatible table");

        let error = init_runtime_schema(&conn).expect_err("incompatible schema should fail");
        assert!(error.to_string().contains("retrieval_failures"));
        assert!(!sqlite_table_exists(&conn, "schema_migrations").expect("table check"));
        assert!(!sqlite_table_exists(&conn, "audit_events").expect("table check"));
    }

    #[test]
    fn workflow_records_retrieval_failure_with_admin_and_safe_views() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "source_of_record": "raw MediaWiki wikitext plus revision metadata",
            }),
        );
        let question = "通灵玉 password=SECRET_RUNTIME_TOKEN_01234567890123456789";

        let workflow = execute_runtime_workflow(
            &conn,
            RuntimeWorkflowInput {
                trace_id: "trace-retrieval-failure-test".to_string(),
                question: question.to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
                profiles: RuntimeWorkflowProfiles::default(),
            },
        )
        .expect("workflow executes");

        let list = list_retrieval_failures(
            &conn,
            RetrievalFailureListInput {
                human_review_status: Some("open".to_string()),
                failure_type: None,
                limit: 10,
                offset: 0,
                view: RetrievalFailureView::AdminDetail,
            },
        )
        .expect("list failures");
        assert_eq!(list.items.len(), 1);
        let item = &list.items[0];
        assert_eq!(
            item["failure_type"],
            json!("source_usage_metadata_incomplete")
        );
        assert_eq!(item["trace_id"], json!("trace-retrieval-failure-test"));
        assert_eq!(item["package_id"], json!(workflow.package.package_id));
        assert_eq!(item["human_review_status"], json!("open"));
        assert!(item["question_sha256"].as_str().is_some());
        assert!(item["question_summary"].as_str().is_some());
        assert!(item["redacted_question_excerpt"].as_str().is_some());
        assert!(
            item["redacted_query_terms"]
                .as_array()
                .is_some_and(|terms| {
                    terms.iter().any(|term| {
                        term.as_str()
                            .is_some_and(|term| term.starts_with("sha256:"))
                    })
                })
        );
        assert!(item["selected_evidence_ids"].as_array().is_some_and(|ids| {
            ids.iter()
                .any(|id| id.as_str().is_some_and(|id| id.starts_with("ev-")))
        }));
        let admin_json = serde_json::to_string(item).expect("admin serializes");
        assert!(!admin_json.contains(question));
        assert!(!admin_json.contains("SECRET_RUNTIME_TOKEN"));
        assert!(!admin_json.contains("password="));

        let failure_id = item["failure_id"].as_str().expect("failure id");
        let updated = update_retrieval_failure_status(
            &conn,
            failure_id,
            "resolved",
            Some("rqa-reviewer"),
            Some("source metadata follow-up recorded"),
        )
        .expect("update failure")
        .expect("failure exists");
        assert_eq!(updated.human_review_status, "resolved");
        assert!(updated.resolved_at.is_some());

        let safe = read_retrieval_failure(&conn, failure_id, RetrievalFailureView::SafeSummary)
            .expect("read safe failure")
            .expect("failure exists");
        assert_eq!(safe["view"], json!("safe_summary"));
        assert!(safe.get("trace_id").is_none());
        assert!(safe.get("package_id").is_none());
        assert!(safe.get("selected_evidence_ids").is_none());
        assert!(safe["redacted_question_excerpt"].as_str().is_some());
        assert_eq!(safe["quality_issue_count"], json!(1));

        let stats = runtime_store_stats(&conn).expect("stats");
        assert_eq!(stats.retrieval_failures, 1);
        assert_eq!(stats.governance_tasks, 1);
        assert_eq!(stats.retrieval_failure_status.get("resolved"), Some(&1_i64));
        assert_eq!(stats.governance_task_status.get("open"), Some(&1_i64));
        let events = runtime_audit_events_for_trace(&conn, "trace-retrieval-failure-test")
            .expect("audit events");
        assert!(events.iter().any(|event| {
            event["event_type"] == "retrieval_failure_recorded"
                && event["payload"]["failure_type"] == json!("source_usage_metadata_incomplete")
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "retrieval_failure_status_updated"
                && event["payload"]["review_note_sha256"].as_str().is_some()
        }));
        assert!(
            events
                .iter()
                .any(|event| event["event_type"] == "governance_task_created")
        );
        let governance_tasks = list_governance_tasks(
            &conn,
            KnowledgeGovernanceTaskListInput {
                status: Some("open".to_string()),
                task_type: Some("source_metadata_fix".to_string()),
                priority: Some("p0".to_string()),
                source_failure_id: Some(failure_id.to_string()),
                source_entity_type: None,
                source_entity_id: None,
                limit: 10,
                offset: 0,
            },
        )
        .expect("list governance tasks");
        assert_eq!(governance_tasks.items.len(), 1);
        assert_eq!(
            governance_tasks.items[0]["source_failure_id"],
            json!(failure_id)
        );
    }

    #[test]
    fn retrieval_failure_records_expected_evidence_miss_and_dedupes() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        let question = "通灵玉是什么？";
        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: question.to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
            },
        )
        .expect("search executes");
        let TonglingyuToolOutput::EvidenceCards {
            cards,
            quality_report,
            ..
        } = output
        else {
            panic!("expected evidence cards");
        };
        assert!(quality_report.production_ready);
        let selected_evidence_ids = evidence_ids(&cards);

        let first = create_retrieval_failure(
            &conn,
            RetrievalFailureCreateInput {
                trace_id: "trace-expected-evidence-test".to_string(),
                package_id: Some("pkg-expected-evidence-test".to_string()),
                question: question.to_string(),
                quality_report: (*quality_report).clone(),
                selected_evidence_ids: selected_evidence_ids.clone(),
                expected_evidence_ids: vec!["ev-expected-missing".to_string()],
                agent_diagnosis: None,
                proposed_fix: None,
            },
        )
        .expect("expected evidence failure records");
        let second = create_retrieval_failure(
            &conn,
            RetrievalFailureCreateInput {
                trace_id: "trace-expected-evidence-test".to_string(),
                package_id: Some("pkg-expected-evidence-test".to_string()),
                question: question.to_string(),
                quality_report: (*quality_report).clone(),
                selected_evidence_ids,
                expected_evidence_ids: vec!["ev-expected-missing".to_string()],
                agent_diagnosis: None,
                proposed_fix: None,
            },
        )
        .expect("deduped expected evidence failure returns existing record");

        assert_eq!(first.failure_id, second.failure_id);
        assert_eq!(first.failure_type, "expected_evidence_missing");
        assert!(
            first
                .quality_issues
                .iter()
                .any(|issue| { issue == "expected_evidence_missing:ev-expected-missing" })
        );
        let list = list_retrieval_failures(
            &conn,
            RetrievalFailureListInput {
                human_review_status: None,
                failure_type: Some("expected_evidence_missing".to_string()),
                limit: 10,
                offset: 0,
                view: RetrievalFailureView::AdminDetail,
            },
        )
        .expect("list expected evidence failures");
        assert_eq!(list.items.len(), 1);
        let events = runtime_audit_events_for_trace(&conn, "trace-expected-evidence-test")
            .expect("audit events");
        assert_eq!(
            events
                .iter()
                .filter(|event| event["event_type"] == "retrieval_failure_recorded")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["event_type"] == "governance_task_created")
                .count(),
            1
        );
        let governance_tasks = list_governance_tasks(
            &conn,
            KnowledgeGovernanceTaskListInput {
                status: Some("open".to_string()),
                task_type: Some("expected_evidence_fix".to_string()),
                priority: Some("p0".to_string()),
                source_failure_id: Some(first.failure_id.clone()),
                source_entity_type: None,
                source_entity_id: None,
                limit: 10,
                offset: 0,
            },
        )
        .expect("list governance tasks");
        assert_eq!(governance_tasks.items.len(), 1);
        assert_eq!(
            governance_tasks.items[0]["task_type"],
            json!("expected_evidence_fix")
        );
    }

    #[test]
    fn retrieval_failure_cluster_creates_proposed_fix_task_without_fact_mutation() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        let question = "通灵玉是什么？";
        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: question.to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
            },
        )
        .expect("search executes");
        let TonglingyuToolOutput::EvidenceCards {
            cards,
            quality_report,
            ..
        } = output
        else {
            panic!("expected evidence cards");
        };
        let selected_evidence_ids = evidence_ids(&cards);
        for index in 1..=2 {
            create_retrieval_failure(
                &conn,
                RetrievalFailureCreateInput {
                    trace_id: format!("trace-cluster-{index}"),
                    package_id: Some(format!("pkg-cluster-{index}")),
                    question: question.to_string(),
                    quality_report: (*quality_report).clone(),
                    selected_evidence_ids: selected_evidence_ids.clone(),
                    expected_evidence_ids: vec![format!("ev-expected-missing-{index}")],
                    agent_diagnosis: None,
                    proposed_fix: None,
                },
            )
            .expect("expected evidence failure records");
        }

        let result = cluster_retrieval_failures(
            &conn,
            RetrievalFailureClusterInput {
                human_review_status: Some("open".to_string()),
                failure_type: Some("expected_evidence_missing".to_string()),
                min_cluster_size: 2,
                limit: 20,
                create_tasks: true,
            },
        )
        .expect("failures cluster");

        assert_eq!(result.scanned_failure_count, 2);
        assert_eq!(result.cluster_count, 1);
        assert_eq!(result.task_count, 1);
        assert_eq!(result.clusters[0]["direct_fact_mutation"], json!(false));
        assert!(
            result.clusters[0]["proposed_fix"]
                .as_str()
                .is_some_and(|value| value.contains("no_direct_fact_mutation=true"))
        );
        assert_eq!(
            result.clusters[0]["task"]["source_entity_type"],
            json!("retrieval_failure_cluster")
        );
        assert_eq!(
            result.clusters[0]["task"]["task_type"],
            json!("expected_evidence_fix")
        );
        let cluster_key = result.clusters[0]["cluster_key"]
            .as_str()
            .expect("cluster key");
        let tasks = list_governance_tasks(
            &conn,
            KnowledgeGovernanceTaskListInput {
                status: Some("open".to_string()),
                task_type: Some("expected_evidence_fix".to_string()),
                priority: Some("p0".to_string()),
                source_failure_id: None,
                source_entity_type: Some("retrieval_failure_cluster".to_string()),
                source_entity_id: Some(cluster_key.to_string()),
                limit: 10,
                offset: 0,
            },
        )
        .expect("list cluster governance task");
        assert_eq!(tasks.items.len(), 1);
        assert!(
            tasks.items[0]["proposed_fix"]
                .as_str()
                .is_some_and(|value| value.contains("agent_cluster_proposed_fix"))
        );
        let open_failures = list_retrieval_failures(
            &conn,
            RetrievalFailureListInput {
                human_review_status: Some("open".to_string()),
                failure_type: Some("expected_evidence_missing".to_string()),
                limit: 10,
                offset: 0,
                view: RetrievalFailureView::AdminDetail,
            },
        )
        .expect("list open failures");
        assert_eq!(open_failures.items.len(), 2);
        let cluster_audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE event_type = 'retrieval_failures_clustered'",
                [],
                |row| row.get(0),
            )
            .expect("cluster audit count");
        assert_eq!(cluster_audit_count, 1);
    }

    #[test]
    fn knowledge_patch_proposal_creates_human_review_task_without_fact_mutation() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        let alias_count_before =
            table_count(&conn, "aliases").expect("alias count before proposal");
        let version_note_count_before =
            table_count(&conn, "version_notes").expect("version note count before proposal");

        let result = create_knowledge_patch_proposal(
            &conn,
            KnowledgePatchProposalCreateInput {
                proposal_type: "alias".to_string(),
                trace_id: "trace-knowledge-patch-proposal".to_string(),
                package_id: None,
                source_ref: Some("trace:trace-knowledge-patch-proposal".to_string()),
                payload: json!({
                    "alias": "灵玉",
                    "target_ref": "person:baoyu",
                    "rationale": "专家建议进入人工复核，不直接写入别名表。",
                }),
                created_by: Some("agent-rqa".to_string()),
                priority: Some("p1".to_string()),
            },
        )
        .expect("proposal creates");

        assert_eq!(
            result["object"],
            json!("tonglingyu.knowledge_patch_proposal_create")
        );
        assert_eq!(
            result["schema_version"],
            json!(KNOWLEDGE_PATCH_PROPOSAL_SCHEMA_VERSION)
        );
        assert_eq!(result["direct_fact_mutation"], json!(false));
        assert_eq!(result["proposal"]["proposal_type"], json!("alias"));
        assert_eq!(
            result["task"]["source_entity_type"],
            json!("knowledge_patch_proposal")
        );
        assert_eq!(result["task"]["task_type"], json!("alias_term_review"));
        assert_eq!(result["task"]["status"], json!("open"));
        assert!(
            result["task"]["proposed_fix"]
                .as_str()
                .is_some_and(|value| value.contains("no_direct_fact_mutation=true"))
        );

        let duplicate = create_knowledge_patch_proposal(
            &conn,
            KnowledgePatchProposalCreateInput {
                proposal_type: "alias".to_string(),
                trace_id: "trace-knowledge-patch-proposal".to_string(),
                package_id: None,
                source_ref: Some("trace:trace-knowledge-patch-proposal".to_string()),
                payload: json!({
                    "target_ref": "person:baoyu",
                    "rationale": "专家建议进入人工复核，不直接写入别名表。",
                    "alias": "灵玉",
                }),
                created_by: Some("agent-rqa".to_string()),
                priority: Some("p1".to_string()),
            },
        )
        .expect("duplicate proposal returns existing");
        assert_eq!(
            duplicate["proposal"]["proposal_id"],
            result["proposal"]["proposal_id"]
        );
        assert_eq!(duplicate["task"]["task_id"], result["task"]["task_id"]);

        let task_id = result["task"]["task_id"].as_str().expect("task id");
        update_governance_task(
            &conn,
            task_id,
            KnowledgeGovernanceTaskUpdateInput {
                status: "accepted".to_string(),
                reviewer: Some("expert-reviewer".to_string()),
                review_note: Some("accept proposal for later KB rebuild input".to_string()),
                evidence_ref: Some("source://expert-review/alias/001".to_string()),
                expected_updated_at: Some(
                    result["task"]["updated_at"]
                        .as_str()
                        .expect("task updated_at")
                        .to_string(),
                ),
            },
        )
        .expect("proposal task accepts")
        .expect("proposal task exists");

        assert_eq!(
            table_count(&conn, "aliases").expect("alias count after proposal"),
            alias_count_before
        );
        assert_eq!(
            table_count(&conn, "version_notes").expect("version note count after proposal"),
            version_note_count_before
        );
        let proposal_task_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM knowledge_governance_tasks WHERE source_entity_type = 'knowledge_patch_proposal'",
                [],
                |row| row.get(0),
            )
            .expect("proposal task count");
        assert_eq!(proposal_task_count, 1);
        let events = runtime_audit_events_for_trace(&conn, "trace-knowledge-patch-proposal")
            .expect("proposal audit events");
        assert!(events.iter().any(|event| {
            event["event_type"] == "knowledge_patch_proposal_created"
                && event["payload"]["payload_sha256"].as_str().is_some()
                && event["payload"].get("payload").is_none()
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "governance_task_status_updated"
                && event["payload"]["evidence_ref_sha256"].as_str().is_some()
        }));
    }

    #[test]
    fn kb_version_diff_report_records_eval_before_after_summary() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        write_kb_version(&conn, Path::new("resources/sources/wiki"))
            .expect("before kb version writes");
        let before_summary = knowledge_base_summary(&conn)
            .expect("before summary loads")
            .expect("before summary exists");
        conn.execute(
            "UPDATE sources SET source_hash = ?1 WHERE source_id = ?2",
            params!["hash-quality-source-updated", "quality-source"],
        )
        .expect("source hash updates");
        let mut build_report = write_kb_version(&conn, Path::new("resources/sources/wiki"))
            .expect("after kb version writes");
        let after_summary = knowledge_base_summary(&conn)
            .expect("after summary loads")
            .expect("after summary exists");
        build_report.diff_report = write_kb_version_diff_report(
            &conn,
            Some(before_summary),
            after_summary,
            json!({
                "object": "tonglingyu.knowledge_patch_application_report",
                "accepted_proposal_count": 0,
                "applied_count": 0,
                "by_type": {},
                "applications": [],
            }),
        )
        .expect("diff report writes");

        assert_eq!(
            build_report.diff_report["schema_version"],
            json!(KB_VERSION_DIFF_REPORT_SCHEMA_VERSION)
        );
        assert_eq!(
            build_report.diff_report["diff"]["sources"]["changed"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        let report_id = build_report.diff_report["report_id"]
            .as_str()
            .expect("report id");
        let updated = record_kb_version_diff_eval_summaries(
            &conn,
            report_id,
            Some(json!({
                "status": "passed",
                "expected_evidence_hit_at_8": {"ratio": 1.0},
                "blockers": [],
            })),
            json!({
                "status": "passed",
                "expected_evidence_hit_at_8": {"ratio": 1.0},
                "blockers": [],
            }),
        )
        .expect("eval summaries record")
        .expect("diff report still exists");
        assert_eq!(updated["eval_after_summary"]["status"], json!("passed"));
        assert_eq!(updated["eval_diff"]["after_status"], json!("passed"));
    }

    #[test]
    fn accepted_knowledge_patch_proposal_applies_during_kb_rebuild_stage() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_aliases(&conn).expect("seed aliases");
        let result = create_knowledge_patch_proposal(
            &conn,
            KnowledgePatchProposalCreateInput {
                proposal_type: "alias".to_string(),
                trace_id: "trace-accepted-patch".to_string(),
                package_id: None,
                source_ref: Some("trace:trace-accepted-patch".to_string()),
                payload: json!({
                    "alias": "玉兄",
                    "target_ref": "person:baoyu",
                    "scope": "expert accepted alias test",
                }),
                created_by: Some("agent-rqa".to_string()),
                priority: Some("p1".to_string()),
            },
        )
        .expect("proposal creates");
        update_governance_task(
            &conn,
            result["task"]["task_id"].as_str().expect("task id"),
            KnowledgeGovernanceTaskUpdateInput {
                status: "accepted".to_string(),
                reviewer: Some("expert-reviewer".to_string()),
                review_note: Some("accepted alias patch for rebuild".to_string()),
                evidence_ref: Some("source://expert-review/alias/002".to_string()),
                expected_updated_at: Some(
                    result["task"]["updated_at"]
                        .as_str()
                        .expect("task updated_at")
                        .to_string(),
                ),
            },
        )
        .expect("proposal task accepts")
        .expect("task exists");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM aliases WHERE alias = '玉兄'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("alias count before apply"),
            0
        );

        let application_report =
            apply_accepted_knowledge_patch_proposals(&conn).expect("accepted proposal applies");

        assert_eq!(application_report["accepted_proposal_count"], json!(1));
        assert_eq!(application_report["applied_count"], json!(1));
        let person_id: String = conn
            .query_row(
                "SELECT person_id FROM aliases WHERE alias = '玉兄'",
                [],
                |row| row.get(0),
            )
            .expect("alias applied");
        assert_eq!(person_id, "person:baoyu");
        assert_eq!(
            table_count(&conn, "knowledge_patch_applications").expect("application count"),
            1
        );
        let events = runtime_audit_events_for_trace(&conn, "kb-rebuild").expect("audit events");
        assert!(events.iter().any(|event| {
            event["event_type"] == "knowledge_patch_proposals_applied"
                && event["payload"]["direct_agent_fact_mutation"] == json!(false)
        }));
    }

    #[test]
    fn prune_runtime_data_preserves_active_rqa_refs_and_writes_tombstones() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        let old = "2020-01-01T00:00:00Z";
        let active_failure_package = create_evidence_package(
            &conn,
            "trace-active-failure-retention",
            "active failure question",
            vec![retention_test_card("active-failure")],
        )
        .expect("active failure package");
        let active_task_package = create_evidence_package(
            &conn,
            "trace-active-task-retention",
            "active task question",
            vec![retention_test_card("active-task")],
        )
        .expect("active task package");
        let expired_package = create_evidence_package(
            &conn,
            "trace-expired-retention",
            "expired question",
            vec![retention_test_card("expired")],
        )
        .expect("expired package");
        conn.execute(
            "UPDATE evidence_packages SET created_at = ?1 WHERE package_id IN (?2, ?3, ?4)",
            params![
                old,
                &active_failure_package.package_id,
                &active_task_package.package_id,
                &expired_package.package_id
            ],
        )
        .expect("packages old");
        conn.execute(
            "UPDATE evidence_cards SET created_at = ?1 WHERE package_id IN (?2, ?3, ?4)",
            params![
                old,
                &active_failure_package.package_id,
                &active_task_package.package_id,
                &expired_package.package_id
            ],
        )
        .expect("cards old");
        conn.execute(
            "UPDATE review_records SET created_at = ?1 WHERE package_id IN (?2, ?3, ?4)",
            params![
                old,
                &active_failure_package.package_id,
                &active_task_package.package_id,
                &expired_package.package_id
            ],
        )
        .expect("reviews old");
        conn.execute("UPDATE audit_events SET created_at = ?1", params![old])
            .expect("audit old");
        conn.execute(
            r#"
            INSERT INTO retrieval_failures (
                failure_id, trace_id, package_id, question_sha256,
                question_char_count, question_summary, kb_schema_version,
                kb_version_id, failure_type, redacted_query_terms_json,
                required_evidence_types_json, actual_evidence_types_json,
                expected_evidence_ids_json, selected_evidence_ids_json,
                missing_evidence_types_json, quality_issues_json,
                agent_diagnosis, proposed_fix, human_review_status, reviewer,
                review_note, created_at, updated_at, resolved_at
            ) VALUES (
                'rf-active-retention', ?1, ?2, ?3, 23, 'sha256:active',
                ?4, NULL, 'expected_evidence_missing', '[]', '["base_text"]',
                '[]', '["ev-missing"]', '[]', '["base_text"]',
                '["expected_evidence_missing"]', NULL,
                'review expected evidence', 'open', NULL, NULL, ?5, ?5, NULL
            )
            "#,
            params![
                &active_failure_package.trace_id,
                &active_failure_package.package_id,
                hash_text("active failure question"),
                KNOWLEDGE_BASE_SCHEMA_VERSION,
                old,
            ],
        )
        .expect("active failure inserts");
        conn.execute(
            r#"
            INSERT INTO knowledge_governance_tasks (
                task_id, source_failure_id, source_entity_type, source_entity_id,
                trace_id, package_id, task_type, status, priority,
                agent_cluster_key, proposed_fix, reviewer, review_note,
                evidence_ref, created_at, updated_at, accepted_at, closed_at
            ) VALUES (
                'kgt-active-retention', NULL, 'lifecycle_test',
                'active-retention-task', ?1, ?2, 'expected_evidence_fix',
                'accepted', 'p1', 'lifecycle:active-retention-task',
                'rebuild after accepted governance task', 'reviewer',
                'accepted for lifecycle protection', 'source://review/retention',
                ?3, ?3, ?3, NULL
            )
            "#,
            params![
                &active_task_package.trace_id,
                &active_task_package.package_id,
                old
            ],
        )
        .expect("active task inserts");
        conn.execute(
            "INSERT INTO audit_events (event_id, trace_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                "audit-old-unrelated-retention",
                "trace-unrelated-retention",
                "old_unrelated",
                "{}",
                old,
            ],
        )
        .expect("old unrelated audit inserts");

        let dry_run = prune_runtime_data(&conn, 1, true).expect("dry run prune");
        assert_eq!(
            dry_run["lifecycle_policy_version"],
            json!(RQA_LIFECYCLE_POLICY_VERSION)
        );
        assert_eq!(dry_run["counts"]["package_candidates"], json!(3));
        assert_eq!(dry_run["counts"]["packages"], json!(1));
        assert_eq!(dry_run["counts"]["protected_packages"], json!(2));
        assert!(
            dry_run["counts"]["protected_audit_events"]
                .as_i64()
                .is_some_and(|count| count >= 4)
        );

        let report = prune_runtime_data(&conn, 1, false).expect("runtime prune");
        assert_eq!(report["status"], json!("pruned"));
        assert_eq!(report["counts"]["packages"], json!(1));
        assert_eq!(report["counts"]["protected_packages"], json!(2));
        assert!(
            report["counts"]["tombstones"]
                .as_i64()
                .is_some_and(|count| count >= 2)
        );
        assert_eq!(
            table_count_where_package(
                &conn,
                "evidence_packages",
                &active_failure_package.package_id
            ),
            1
        );
        assert_eq!(
            table_count_where_package(&conn, "evidence_packages", &active_task_package.package_id),
            1
        );
        assert_eq!(
            table_count_where_package(&conn, "evidence_packages", &expired_package.package_id),
            0
        );
        let active_audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE trace_id = ?1",
                params![&active_failure_package.trace_id],
                |row| row.get(0),
            )
            .expect("active audit count");
        assert!(active_audit_count > 0);
        let expired_audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE trace_id = ?1",
                params![&expired_package.trace_id],
                |row| row.get(0),
            )
            .expect("expired audit count");
        assert_eq!(expired_audit_count, 0);
        let prune_audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE event_type = 'rqa_retention_pruned'",
                [],
                |row| row.get(0),
            )
            .expect("retention audit count");
        assert_eq!(prune_audit_count, 1);
        let tombstone_payloads = tombstone_payloads(&conn);
        assert!(tombstone_payloads.iter().all(|payload| {
            !payload.contains("active failure question") && !payload.contains("expired question")
        }));
    }

    fn retention_test_card(suffix: &str) -> EvidenceCard {
        let mut card = sample_card("base_text");
        card.evidence_id = format!("ev-retention-{suffix}");
        card.block_id = format!("block-retention-{suffix}");
        card
    }

    fn table_count_where_package(conn: &Connection, table: &str, package_id: &str) -> i64 {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE package_id = ?1"),
            params![package_id],
            |row| row.get(0),
        )
        .expect("package count")
    }

    fn tombstone_payloads(conn: &Connection) -> Vec<String> {
        conn.prepare("SELECT payload_json FROM rqa_lifecycle_tombstones ORDER BY created_at")
            .expect("prepare tombstones")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query tombstones")
            .collect::<std::result::Result<Vec<_>, _>>()
            .expect("collect tombstones")
    }

    #[test]
    fn governance_task_status_flow_requires_human_acceptance_metadata() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        let question = "通灵玉是什么？";
        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: question.to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
            },
        )
        .expect("search executes");
        let TonglingyuToolOutput::EvidenceCards {
            cards,
            quality_report,
            ..
        } = output
        else {
            panic!("expected evidence cards");
        };
        let failure = create_retrieval_failure(
            &conn,
            RetrievalFailureCreateInput {
                trace_id: "trace-governance-task-test".to_string(),
                package_id: Some("pkg-governance-task-test".to_string()),
                question: question.to_string(),
                quality_report: (*quality_report).clone(),
                selected_evidence_ids: evidence_ids(&cards),
                expected_evidence_ids: vec!["ev-expected-missing".to_string()],
                agent_diagnosis: Some("expected evidence absent".to_string()),
                proposed_fix: Some("review_expected_evidence_fixture".to_string()),
            },
        )
        .expect("failure creates governance task");
        let task = create_governance_task_from_failure(
            &conn,
            KnowledgeGovernanceTaskCreateFromFailureInput {
                source_failure_id: failure.failure_id.clone(),
                task_type: None,
                priority: None,
                proposed_fix: None,
                agent_cluster_key: None,
            },
        )
        .expect("governance task loads")
        .expect("governance task exists");

        let rejected = update_governance_task(
            &conn,
            &task.task_id,
            KnowledgeGovernanceTaskUpdateInput {
                status: "accepted".to_string(),
                reviewer: Some("rqa-reviewer".to_string()),
                review_note: Some("accepted".to_string()),
                evidence_ref: None,
                expected_updated_at: None,
            },
        )
        .expect_err("accepted task requires evidence ref");
        assert!(rejected.to_string().contains("requires reviewer"));

        let accepted = update_governance_task(
            &conn,
            &task.task_id,
            KnowledgeGovernanceTaskUpdateInput {
                status: "accepted".to_string(),
                reviewer: Some("rqa-reviewer".to_string()),
                review_note: Some("accepted with source patch".to_string()),
                evidence_ref: Some("source://review-note/001".to_string()),
                expected_updated_at: Some(task.updated_at.clone()),
            },
        )
        .expect("governance task updates")
        .expect("governance task exists");
        assert_eq!(accepted.status, "accepted");
        assert!(accepted.accepted_at.is_some());
        assert_eq!(
            accepted.evidence_ref.as_deref(),
            Some("source://review-note/001")
        );
        let events =
            runtime_audit_events_for_trace(&conn, "trace-governance-task-test").expect("events");
        assert!(events.iter().any(|event| {
            event["event_type"] == "governance_task_status_updated"
                && event["payload"]["evidence_ref_sha256"].as_str().is_some()
        }));
    }

    #[test]
    fn governance_task_can_target_trace_without_source_failure() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");

        let task = create_governance_task(
            &conn,
            KnowledgeGovernanceTaskCreateInput {
                source_entity_type: "trace".to_string(),
                source_entity_id: "trace-expert-review-test".to_string(),
                trace_id: "trace-expert-review-test".to_string(),
                package_id: None,
                source_failure_id: None,
                task_type: "expert_review".to_string(),
                priority: Some("p0".to_string()),
                proposed_fix: Some("request_expert_review_without_fact_mutation".to_string()),
                agent_cluster_key: None,
            },
        )
        .expect("trace governance task creates");

        assert_eq!(task.source_failure_id, None);
        assert_eq!(task.source_entity_type, "trace");
        let listed = list_governance_tasks(
            &conn,
            KnowledgeGovernanceTaskListInput {
                status: Some("open".to_string()),
                task_type: Some("expert_review".to_string()),
                priority: Some("p0".to_string()),
                source_failure_id: None,
                source_entity_type: Some("trace".to_string()),
                source_entity_id: Some("trace-expert-review-test".to_string()),
                limit: 10,
                offset: 0,
            },
        )
        .expect("list trace governance task");
        assert_eq!(listed.items.len(), 1);
        assert_eq!(
            listed.items[0]["source_entity_id"],
            json!("trace-expert-review-test")
        );
    }

    #[test]
    fn workflow_records_reviewer_failure_when_local_review_downgrades() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");

        let workflow = execute_runtime_workflow(
            &conn,
            RuntimeWorkflowInput {
                trace_id: "trace-reviewer-failure-test".to_string(),
                question: "不存在的检索目标".to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
                profiles: RuntimeWorkflowProfiles::default(),
            },
        )
        .expect("workflow executes");

        assert_eq!(workflow.package.review.status, "needs_revision");
        let list = list_retrieval_failures(
            &conn,
            RetrievalFailureListInput {
                human_review_status: Some("open".to_string()),
                failure_type: None,
                limit: 10,
                offset: 0,
                view: RetrievalFailureView::AdminDetail,
            },
        )
        .expect("list failures");
        let failure_types = list
            .items
            .iter()
            .filter_map(|item| item["failure_type"].as_str())
            .collect::<BTreeSet<_>>();
        assert!(failure_types.contains("no_evidence_selected"));
        assert!(failure_types.contains("reviewer_evidence_insufficient"));
    }

    #[test]
    fn retrieval_failure_rolls_back_when_audit_append_fails() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        seed_retrieval_quality_source(
            &conn,
            json!({
                "license": "CC-BY-SA-4.0",
                "license_url": "https://creativecommons.org/licenses/by-sa/4.0/",
                "license_source_url": "https://wikisource.org/wiki/Wikisource:Copyright_policy",
                "attribution": "Wikisource contributors",
                "usage_boundary": "可作为正文或版本对照证据候选；不声明完成学术校勘。",
            }),
        );
        let question = "通灵玉是什么？";
        let output = execute_tool(
            &conn,
            TonglingyuToolCall::TextSearch {
                question: question.to_string(),
                limit: 2,
                required_evidence_types: vec!["base_text".to_string()],
            },
        )
        .expect("search executes");
        let TonglingyuToolOutput::EvidenceCards {
            cards,
            quality_report,
            ..
        } = output
        else {
            panic!("expected evidence cards");
        };
        conn.execute_batch(
            r#"
            DROP TABLE audit_events;
            CREATE TABLE audit_events (event_id TEXT PRIMARY KEY);
            "#,
        )
        .expect("break audit table");

        let error = create_retrieval_failure(
            &conn,
            RetrievalFailureCreateInput {
                trace_id: "trace-audit-failure-test".to_string(),
                package_id: Some("pkg-audit-failure-test".to_string()),
                question: question.to_string(),
                quality_report: (*quality_report).clone(),
                selected_evidence_ids: evidence_ids(&cards),
                expected_evidence_ids: vec!["ev-expected-missing".to_string()],
                agent_diagnosis: None,
                proposed_fix: None,
            },
        )
        .expect_err("audit append failure should fail closed");
        assert!(error.to_string().contains("audit_events"));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM retrieval_failures", [], |row| {
                row.get(0)
            })
            .expect("failure count");
        assert_eq!(count, 0);
    }

    #[test]
    fn parses_chapter_numbers() {
        assert_eq!(extract_chapter_no("紅樓夢/第015回"), Some(15));
        assert_eq!(extract_chapter_no("脂硯齋重評石頭記/第一回"), Some(1));
        assert_eq!(
            extract_chapter_no("紅樓夢_程乙本_第一百十一回_至第一百二十回"),
            Some(111)
        );
    }

    #[test]
    fn reviewer_blocks_no_evidence() {
        let review = review("黛玉结局是什么", &[], &[]);
        assert_eq!(review.status, "needs_revision");
        assert_eq!(review.severity, "high");
    }

    #[test]
    fn reviewer_blocks_commentary_only_body_claim() {
        let cards = vec![sample_card("commentary")];
        let question = "只根据脂批原文说明正文事实可以吗？";
        let claims = claims_from_cards(question, &cards);
        let review = review(question, &cards, &claims);
        assert_eq!(review.status, "needs_revision");
        assert_eq!(review.severity, "medium");
        assert!(
            review
                .issues
                .iter()
                .any(|issue| issue.contains("当前证据全为脂批"))
        );
    }

    #[test]
    fn reviewer_allows_commentary_original_text_question() {
        let cards = vec![sample_card("commentary")];
        let question = "脂批原文如何评价石头？";
        let claims = claims_from_cards(question, &cards);
        let review = review(question, &cards, &claims);

        assert_eq!(review.status, "passed");
        assert!(review.issues.is_empty());
    }

    #[test]
    fn reviewer_downgrades_facsimile_authoritative_collation_claim() {
        let cards = vec![sample_card("base_text")];
        let question = "请确认通灵玉铭文在影印件、权威校注本和专家校勘中完全一致吗？";
        let claims = claims_from_cards(question, &cards);
        let review = review(question, &cards, &claims);

        assert_eq!(review.status, "needs_revision");
        assert_eq!(review.severity, "medium");
        assert!(
            review
                .issues
                .iter()
                .any(|issue| issue.contains("缺少影印件、权威校注本或专家校勘复核"))
        );
    }

    #[test]
    fn replay_keeps_package_id_and_review_downgrade() {
        let package = EvidencePackage {
            package_id: "pkg-test".to_string(),
            trace_id: "trace-test".to_string(),
            question: "量子计算机是什么？".to_string(),
            cards: vec![],
            claims: vec!["当前知识库未找到可追溯证据，不能给出确定结论。".to_string()],
            claim_evidence_map: vec![],
            review: review("量子计算机是什么？", &[], &[]),
        };
        let answer = replay_answer(&package);
        assert!(answer.contains("pkg-test"));
        assert!(answer.contains("证据不足"));
    }

    #[test]
    fn runtime_workflow_emits_profile_step_refs_and_review() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        init_runtime_schema(&conn).expect("runtime schema");
        init_knowledge_base_schema(&conn).expect("kb schema");
        let workflow = execute_runtime_workflow(
            &conn,
            RuntimeWorkflowInput {
                trace_id: "trace-workflow-test".to_string(),
                question: "量子红学理论如何解释通灵玉？".to_string(),
                limit: 3,
                required_evidence_types: vec!["base_text".to_string()],
                profiles: RuntimeWorkflowProfiles::default(),
            },
        )
        .expect("workflow executes");

        assert_eq!(workflow.steps.len(), 4);
        assert_eq!(workflow.package.review.status, "needs_revision");
        assert!(workflow.final_answer.contains(&workflow.package.package_id));
        assert_eq!(
            workflow.agent_runtime_summary["profile_execution_status"],
            "deterministic_workflow_only"
        );
        assert_eq!(
            workflow.agent_runtime_summary["profile_step_count"],
            json!(workflow.steps.len())
        );
        let plan = runtime_workflow_plan(RuntimeWorkflowPlanInput {
            question_type: "runtime_workflow".to_string(),
            required_evidence_types: vec!["base_text".to_string()],
            blocked_controls: Vec::new(),
            profiles: RuntimeWorkflowProfiles::default(),
        });
        let planned_steps = plan
            .steps
            .iter()
            .map(|step| {
                (
                    step.step_id.clone(),
                    step.operation.clone(),
                    step.allowed_tools.clone(),
                )
            })
            .collect::<Vec<_>>();
        let actual_steps = workflow
            .steps
            .iter()
            .map(|step| {
                (
                    step.step_id.clone(),
                    step.operation.clone(),
                    step.allowed_tools.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual_steps, planned_steps);
        assert!(
            workflow
                .steps
                .iter()
                .all(|step| step.output_ref.starts_with("runtime://tonglingyu/"))
        );
        assert!(
            workflow
                .steps
                .iter()
                .any(|step| step.operation == "review_answer"
                    && step.output["draft_consumed"] == true)
        );
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "content_delta"
                && event
                    .content_delta
                    .as_deref()
                    .is_some_and(|chunk| !chunk.is_empty())
        }));
        let profile_step_events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE event_type = 'runtime_profile_step_completed'",
                [],
                |row| row.get(0),
            )
            .expect("audit count");
        assert_eq!(profile_step_events, workflow.steps.len() as i64);
    }

    #[test]
    fn hermes_mode_applies_runtime_draft_when_local_review_passes() {
        let mut workflow = runtime_draft_workflow(
            vec![sample_card("base_text")],
            ReviewRecord {
                status: "passed".to_string(),
                severity: "none".to_string(),
                issues: vec![],
                summary: "reviewer passed".to_string(),
            },
        );

        let application =
            apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
                .expect("runtime draft consumed");
        assert!(application.draft_consumed);
        assert!(application.content_used_for_final_answer);
        assert!(workflow.draft_answer.contains("Hermes profile 草稿"));
        assert_eq!(workflow.final_answer, workflow.draft_answer);
        assert_eq!(
            workflow.answer_source,
            "agent_runtime_hermes_profile_with_local_review"
        );
        assert_eq!(
            workflow.steps[0].agent_runtime.as_ref().unwrap()["content_used_for_final_answer"],
            json!(true)
        );
        assert_eq!(
            workflow.steps[1].output["draft_source"],
            "agent_runtime_hermes_profile"
        );
    }

    #[test]
    fn hermes_mode_rejects_runtime_draft_when_local_review_downgrades() {
        let mut workflow = runtime_draft_workflow(
            Vec::new(),
            ReviewRecord {
                status: "needs_revision".to_string(),
                severity: "high".to_string(),
                issues: vec!["当前没有可追溯证据。".to_string()],
                summary: "reviewer requires downgrade".to_string(),
            },
        );

        let application =
            apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
                .expect("runtime draft consumed");
        assert!(application.draft_consumed);
        assert!(!application.content_used_for_final_answer);
        assert!(workflow.draft_answer.contains("Hermes profile 草稿"));
        assert!(!workflow.final_answer.contains("Hermes profile 草稿"));
        assert_eq!(
            workflow.answer_source,
            "agent_runtime_hermes_profile_rejected_by_local_review"
        );
        assert_eq!(
            workflow.steps[0].agent_runtime.as_ref().unwrap()["content_used_for_final_answer"],
            json!(false)
        );
        assert_eq!(
            workflow.steps[1].output["final_answer_source"],
            "agent_runtime_hermes_profile_rejected_by_local_review"
        );
    }

    #[test]
    fn hermes_mode_accepts_structured_draft_with_matching_package() {
        let mut workflow = runtime_draft_workflow(
            vec![sample_card("base_text")],
            ReviewRecord {
                status: "passed".to_string(),
                severity: "none".to_string(),
                issues: vec![],
                summary: "reviewer passed".to_string(),
            },
        );
        let package_id = workflow.package.package_id.clone();
        workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
            serde_json::to_string(&json!({
                "draft_answer": "结构化 Hermes 草稿：必须引用证据包 pkg-runtime-draft-test。",
                "package_id": package_id,
                "claim_statements": ["结构化 claim"],
            }))
            .expect("structured draft serializes")
        );

        let application =
            apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
                .expect("structured runtime draft consumed");

        assert!(application.draft_consumed);
        assert_eq!(application.result_format, "json");
        assert!(application.rejected_reason.is_none());
        assert_eq!(
            workflow.draft_answer,
            "结构化 Hermes 草稿：必须引用证据包 pkg-runtime-draft-test。"
        );
        assert_eq!(
            workflow.steps[0].output["agent_runtime_result_format"],
            "json"
        );
        assert_eq!(
            workflow.steps[0].output["agent_runtime_claim_statement_count"],
            json!(1)
        );
    }

    #[test]
    fn hermes_mode_accepts_nested_result_summary_draft_with_matching_package() {
        let mut workflow = runtime_draft_workflow(
            vec![sample_card("base_text")],
            ReviewRecord {
                status: "passed".to_string(),
                severity: "none".to_string(),
                issues: vec![],
                summary: "reviewer passed".to_string(),
            },
        );
        let package_id = workflow.package.package_id.clone();
        workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
            serde_json::to_string(&json!({
                "result_summary": serde_json::to_string(&json!({
                    "draft_candidate": {
                        "draft_answer": "嵌套 Hermes 草稿：必须引用本地证据包。",
                        "package_id": package_id,
                        "claim_statements": ["nested claim"],
                    }
                }))
                .expect("inner draft serializes")
            }))
            .expect("outer draft serializes")
        );

        let application =
            apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
                .expect("nested structured runtime draft consumed");

        assert!(application.draft_consumed);
        assert_eq!(application.result_format, "json");
        assert_eq!(
            workflow.draft_answer,
            "嵌套 Hermes 草稿：必须引用本地证据包。"
        );
        assert_eq!(
            workflow.steps[0].output["agent_runtime_claim_statement_count"],
            json!(1)
        );
    }

    #[test]
    fn hermes_mode_rejects_structured_draft_with_wrong_package() {
        let mut workflow = runtime_draft_workflow(
            vec![sample_card("base_text")],
            ReviewRecord {
                status: "passed".to_string(),
                severity: "none".to_string(),
                issues: vec![],
                summary: "reviewer passed".to_string(),
            },
        );
        let original_draft = workflow.draft_answer.clone();
        let original_final = workflow.final_answer.clone();
        workflow.steps[0].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
            serde_json::to_string(&json!({
                "draft_answer": "错误 package 的 Hermes 草稿不应被消费。",
                "package_id": "pkg-other",
                "claim_statements": ["wrong package claim"],
            }))
            .expect("structured draft serializes")
        );

        let application =
            apply_agent_runtime_content_outputs(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
                .expect("structured runtime draft rejected");

        assert!(!application.draft_consumed);
        assert!(!application.content_used_for_final_answer);
        assert_eq!(application.result_format, "json");
        assert_eq!(application.rejected_reason, Some("package_id_mismatch"));
        assert_eq!(workflow.draft_answer, original_draft);
        assert_eq!(workflow.final_answer, original_final);
        assert_eq!(workflow.answer_source, "runtime_local_profile");
        assert_eq!(
            workflow.steps[0].output["agent_runtime_draft_rejected_reason"],
            "package_id_mismatch"
        );
        assert_eq!(
            workflow.steps[0].agent_runtime.as_ref().unwrap()["content_application"]["draft_consumed"],
            json!(false)
        );
    }

    #[test]
    fn hermes_mode_observes_structured_reviewer_agreement() {
        let mut workflow = runtime_draft_workflow(
            vec![sample_card("base_text")],
            ReviewRecord {
                status: "passed".to_string(),
                severity: "none".to_string(),
                issues: vec![],
                summary: "reviewer passed".to_string(),
            },
        );
        workflow.steps[1].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
            serde_json::to_string(&json!({
                "review_status": "passed",
                "severity": "none",
                "issues": [],
                "required_revisions": [],
            }))
            .expect("structured review serializes")
        );

        let observation =
            apply_agent_runtime_reviewer_output(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
                .expect("reviewer observation is recorded");

        assert_eq!(observation.result_format, "json");
        assert_eq!(observation.review_status.as_deref(), Some("passed"));
        assert!(observation.agrees_with_local_reviewer);
        assert!(!observation.local_reviewer_override);
        assert_eq!(
            workflow.steps[1].output["agent_runtime_review_agrees_with_local"],
            json!(true)
        );
        assert_eq!(
            workflow.steps[1].agent_runtime.as_ref().unwrap()["review_observation"]["local_reviewer_enforced"],
            json!(true)
        );
    }

    #[test]
    fn hermes_mode_observes_nested_result_summary_reviewer_agreement() {
        let mut workflow = runtime_draft_workflow(
            vec![sample_card("base_text")],
            ReviewRecord {
                status: "passed".to_string(),
                severity: "none".to_string(),
                issues: vec![],
                summary: "reviewer passed".to_string(),
            },
        );
        workflow.steps[1].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
            serde_json::to_string(&json!({
                "result_summary": serde_json::to_string(&json!({
                    "review_observation": {
                        "review_status": "passed",
                        "severity": "none",
                        "issues": [],
                        "required_revisions": [],
                    }
                }))
                .expect("inner review serializes")
            }))
            .expect("outer review serializes")
        );

        let observation =
            apply_agent_runtime_reviewer_output(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
                .expect("nested reviewer observation is recorded");

        assert_eq!(observation.result_format, "json");
        assert_eq!(observation.review_status.as_deref(), Some("passed"));
        assert!(observation.agrees_with_local_reviewer);
        assert!(!observation.local_reviewer_override);
    }

    #[test]
    fn hermes_mode_marks_reviewer_disagreement_as_local_override() {
        let mut workflow = runtime_draft_workflow(
            Vec::new(),
            ReviewRecord {
                status: "needs_revision".to_string(),
                severity: "high".to_string(),
                issues: vec!["当前没有可追溯证据。".to_string()],
                summary: "reviewer requires downgrade".to_string(),
            },
        );
        workflow.steps[1].agent_runtime.as_mut().unwrap()["result_summary"] = json!(
            serde_json::to_string(&json!({
                "review_status": "passed",
                "severity": "none",
                "issues": [],
                "required_revisions": [],
            }))
            .expect("structured review serializes")
        );

        let observation =
            apply_agent_runtime_reviewer_output(&mut workflow, TonglingyuAgentRuntimeMode::Hermes)
                .expect("reviewer observation is recorded");

        assert_eq!(observation.review_status.as_deref(), Some("passed"));
        assert!(!observation.agrees_with_local_reviewer);
        assert!(observation.local_reviewer_override);
        assert_eq!(workflow.package.review.status, "needs_revision");
        assert_eq!(
            workflow.steps[1].output["agent_runtime_local_reviewer_override"],
            json!(true)
        );
        assert_eq!(
            workflow.steps[1].agent_runtime.as_ref().unwrap()["review_observation"]["local_review_status"],
            "needs_revision"
        );
    }

    #[test]
    fn package_observation_rejects_wrong_runtime_package_id() {
        let observation = extract_agent_runtime_package_observation(
            r#"{"package_id":"pkg-other","summary":"wrong package"}"#,
            "pkg-runtime-draft-test",
        );

        assert_eq!(observation.result_format, "json");
        assert_eq!(observation.package_id.as_deref(), Some("pkg-other"));
        assert!(!observation.matches_runtime_package);
        assert_eq!(observation.rejected_reason, Some("package_id_mismatch"));
    }

    #[test]
    fn package_observation_accepts_text_containing_expected_package_id() {
        let observation = extract_agent_runtime_package_observation(
            "result_summary: 已创建证据包 pkg-runtime-draft-test，包含 8 张卡片。",
            "pkg-runtime-draft-test",
        );

        assert_eq!(observation.result_format, "text");
        assert_eq!(
            observation.package_id.as_deref(),
            Some("pkg-runtime-draft-test")
        );
        assert!(observation.matches_runtime_package);
        assert!(observation.rejected_reason.is_none());
    }

    #[test]
    fn package_observation_accepts_named_package_observation() {
        let observation = extract_agent_runtime_package_observation(
            r#"{"package_observation":{"package_id":"pkg-runtime-draft-test","summary":"package observed"}}"#,
            "pkg-runtime-draft-test",
        );

        assert_eq!(observation.result_format, "json");
        assert_eq!(
            observation.package_id.as_deref(),
            Some("pkg-runtime-draft-test")
        );
        assert!(observation.matches_runtime_package);
        assert!(observation.rejected_reason.is_none());
    }

    #[test]
    fn evidence_observation_rejects_unknown_refs() {
        let observation = extract_agent_runtime_evidence_observation(
            r#"{"evidence_refs":["ev-known","ev-unknown"],"evidence_analysis":"test","unsupported_scope":"test"}"#,
            "text_evidence_search",
            "honglou-text",
            &["ev-known".to_string()],
        );

        assert_eq!(observation.result_format, "json");
        assert_eq!(observation.evidence_ref_count, 2);
        assert_eq!(
            observation.unknown_evidence_refs,
            vec!["ev-unknown".to_string()]
        );
        assert!(!observation.matches_runtime_evidence);
        assert_eq!(observation.rejected_reason, Some("unknown_evidence_ref"));
    }

    #[test]
    fn evidence_observation_accepts_nested_result_summary_refs() {
        let summary = serde_json::to_string(&json!({
            "result_summary": serde_json::to_string(&json!({
                "evidence_observation": {
                    "evidence_refs": ["ev-known"],
                    "evidence_analysis": "test",
                    "unsupported_scope": "test",
                }
            }))
            .expect("inner evidence serializes")
        }))
        .expect("outer evidence serializes");

        let observation = extract_agent_runtime_evidence_observation(
            &summary,
            "text_evidence_search",
            "honglou-text",
            &["ev-known".to_string()],
        );

        assert_eq!(observation.result_format, "json");
        assert_eq!(observation.evidence_ref_count, 1);
        assert!(observation.matches_runtime_evidence);
        assert!(observation.rejected_reason.is_none());
    }

    fn runtime_draft_workflow(
        cards: Vec<EvidenceCard>,
        review: ReviewRecord,
    ) -> RuntimeWorkflowOutput {
        let package = EvidencePackage {
            package_id: "pkg-runtime-draft-test".to_string(),
            trace_id: "trace-runtime-draft-test".to_string(),
            question: "通灵玉是什么？".to_string(),
            cards,
            claims: vec!["Hermes 草稿候选需要保留证据边界。".to_string()],
            claim_evidence_map: vec![],
            review,
        };
        RuntimeWorkflowOutput {
            trace_id: package.trace_id.clone(),
            question: package.question.clone(),
            package,
            draft_answer: "本地草稿".to_string(),
            final_answer: "本地最终回答".to_string(),
            answer_source: "runtime_local_profile".to_string(),
            agent_runtime_summary: default_agent_runtime_summary(),
            steps: vec![
                RuntimeWorkflowStepReport {
                    step_id: "step-01-draft-answer".to_string(),
                    profile: "honglou-main".to_string(),
                    profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
                    operation: "draft_answer".to_string(),
                    status: "completed".to_string(),
                    required: true,
                    allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
                    tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
                    input_ref: None,
                    output_ref:
                        "runtime://tonglingyu/trace-runtime-draft-test/step-01-draft-answer"
                            .to_string(),
                    duration_ms: 1,
                    trace_id: "trace-runtime-draft-test".to_string(),
                    output: json!({"object": "tonglingyu.draft_answer"}),
                    agent_runtime: Some(json!({
                        "client": "hermes",
                        "status": "executed",
                        "content_used_for_final_answer": false,
                        "result_summary": "Hermes profile 草稿：必须引用证据包 pkg-runtime-draft-test。",
                    })),
                },
                RuntimeWorkflowStepReport {
                    step_id: "step-02-review-answer".to_string(),
                    profile: "honglou-reviewer".to_string(),
                    profile_contract_version: PROFILE_CONTRACT_VERSION.to_string(),
                    operation: "review_answer".to_string(),
                    status: "completed".to_string(),
                    required: true,
                    allowed_tools: vec!["tonglingyu.evidence.package.read".to_string()],
                    tool_calls: vec!["tonglingyu.evidence.package.read".to_string()],
                    input_ref: Some(
                        "runtime://tonglingyu/trace-runtime-draft-test/step-01-draft-answer"
                            .to_string(),
                    ),
                    output_ref:
                        "runtime://tonglingyu/trace-runtime-draft-test/step-02-review-answer"
                            .to_string(),
                    duration_ms: 1,
                    trace_id: "trace-runtime-draft-test".to_string(),
                    output: json!({"object": "tonglingyu.review_result"}),
                    agent_runtime: Some(json!({
                        "client": "hermes",
                        "status": "executed",
                        "content_used_for_final_answer": false,
                        "result_summary": "Hermes reviewer envelope",
                    })),
                },
            ],
            stream_events: Vec::new(),
        }
    }

    #[test]
    fn tool_catalog_defines_expected_readonly_contracts() {
        let catalog = tool_catalog();
        let names = catalog
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<BTreeSet<_>>();
        for expected in [
            "tonglingyu.text.search",
            "tonglingyu.commentary.search",
            "tonglingyu.evidence.package.create",
            "tonglingyu.evidence.package.read",
            "tonglingyu.evidence.package.replay",
        ] {
            assert!(names.contains(expected), "missing tool contract {expected}");
        }
        assert!(
            catalog
                .iter()
                .all(|tool| tool.version == TOOL_CATALOG_VERSION)
        );
        assert!(
            catalog
                .iter()
                .filter(|tool| tool.name.ends_with(".search"))
                .all(|tool| tool.effect_scope == "read_only_kb")
        );
    }

    #[test]
    fn profile_catalog_defines_four_runtime_profiles() {
        let catalog = profile_catalog();
        let profiles = catalog
            .iter()
            .map(|profile| profile.profile.as_str())
            .collect::<BTreeSet<_>>();
        for expected in [
            "honglou-text",
            "honglou-commentary",
            "honglou-main",
            "honglou-reviewer",
        ] {
            assert!(profiles.contains(expected), "missing profile {expected}");
        }
        assert!(
            catalog
                .iter()
                .all(|profile| profile.version == PROFILE_CONTRACT_VERSION)
        );
        let reviewer = catalog
            .iter()
            .find(|profile| profile.profile == "honglou-reviewer")
            .expect("reviewer profile exists");
        assert!(
            reviewer
                .allowed_tools
                .contains(&"tonglingyu.evidence.package.read".to_string())
        );
        assert!(reviewer.safety_contract["cannot_be_disabled_by_user"] == true);
    }

    #[tokio::test]
    async fn runtime_store_executes_workflow_step_envelopes_through_agent_runtime() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-agent-step-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let workflow = store
            .execute_workflow_with_agent_runtime_mode(
                RuntimeWorkflowInput {
                    trace_id: "trace-agent-runtime-step-test".to_string(),
                    question: "量子红学理论如何解释通灵玉？".to_string(),
                    limit: 3,
                    required_evidence_types: vec!["base_text".to_string()],
                    profiles: RuntimeWorkflowProfiles::default(),
                },
                TonglingyuAgentRuntimeMode::Minimal,
            )
            .await
            .expect("workflow executes");

        assert!(
            workflow
                .steps
                .iter()
                .all(|step| step.agent_runtime.is_some())
        );
        assert!(workflow.steps.iter().any(|step| {
            step.agent_runtime.as_ref().is_some_and(|value| {
                value["client"] == "minimal"
                    && value["content_source"] == "tonglingyu-deterministic-workflow"
                    && value["content_used_for_final_answer"] == json!(false)
            })
        }));
        assert!(workflow.steps.iter().any(|step| {
            step.operation == "draft_answer"
                && step.agent_runtime.as_ref().is_some_and(|value| {
                    value["result_summary_contract"]
                        .as_str()
                        .is_some_and(|contract| {
                            contract.contains("draft_answer") && contract.contains("package_id")
                        })
                        && value["result_summary"].as_str().is_some_and(|summary| {
                            summary.contains("operation: draft_answer")
                                && summary.contains(&workflow.package.package_id)
                        })
                })
        }));
        assert!(workflow.steps.iter().any(|step| {
            step.operation == "review_answer"
                && step.agent_runtime.as_ref().is_some_and(|value| {
                    value["result_summary_contract"]
                        .as_str()
                        .is_some_and(|contract| {
                            contract.contains("review_status")
                                && contract.contains("local reviewer")
                        })
                })
        }));
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "step_completed"
                && event.metadata["agent_runtime"]["status"] == "executed"
        }));
        assert_eq!(
            workflow.agent_runtime_summary["profile_execution_status"],
            "minimal_envelope_only"
        );
        assert_eq!(
            workflow.agent_runtime_summary["executed_profile_step_count"],
            json!(workflow.steps.len())
        );
        assert_eq!(
            workflow.agent_runtime_summary["hermes_content_execution_complete"],
            json!(false)
        );
        let conn = store.open_connection().expect("runtime conn");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE event_type = 'agent_runtime_profile_step_executed'",
                [],
                |row| row.get(0),
            )
            .expect("audit count");
        assert_eq!(count, workflow.steps.len() as i64);
        let summary_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE event_type = 'agent_runtime_profile_execution_summarized'",
                [],
                |row| row.get(0),
            )
            .expect("summary audit count");
        assert_eq!(summary_count, 1);
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn runtime_store_consumes_hermes_draft_candidate_through_full_workflow() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-hermes-draft-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let workflow = store
            .execute_workflow_with_agent_runtime_client(
                RuntimeWorkflowInput {
                    trace_id: "trace-hermes-draft-workflow-test".to_string(),
                    question: "通灵玉是什么？".to_string(),
                    limit: 2,
                    required_evidence_types: vec!["base_text".to_string()],
                    profiles: RuntimeWorkflowProfiles::default(),
                },
                TonglingyuAgentRuntimeMode::Hermes,
                Arc::new(DraftRuntimeClient),
            )
            .await
            .expect("workflow executes");

        assert_eq!(workflow.package.review.status, "needs_revision");
        assert!(workflow.draft_answer.contains("Hermes full workflow draft"));
        assert!(workflow.draft_answer.contains(&workflow.package.package_id));
        assert!(!workflow.final_answer.contains("Hermes full workflow draft"));
        assert_eq!(
            workflow.answer_source,
            "agent_runtime_hermes_profile_rejected_by_local_review"
        );
        let draft_step = workflow
            .steps
            .iter()
            .find(|step| step.operation == "draft_answer")
            .expect("draft step");
        assert_eq!(
            draft_step.agent_runtime.as_ref().unwrap()["content_used_for_final_answer"],
            json!(false)
        );
        let draft_agent_runtime = draft_step.agent_runtime.as_ref().unwrap();
        assert_eq!(
            draft_agent_runtime["content_source"],
            json!("agent-runtime-hermes-profile")
        );
        assert_eq!(draft_agent_runtime["tool_rounds"], json!(1));
        assert_eq!(draft_agent_runtime["tool_result_count"], json!(1));
        assert_eq!(draft_agent_runtime["tool_audit_event_count"], json!(1));
        assert_eq!(
            draft_agent_runtime["tool_results"][0]["tool_name"],
            "tonglingyu.evidence.package.read"
        );
        let text_step = workflow
            .steps
            .iter()
            .find(|step| step.operation == "text_evidence_search")
            .expect("text evidence step");
        assert_eq!(
            text_step.agent_runtime.as_ref().unwrap()["content_source"],
            json!("agent-runtime-hermes-evidence-observation")
        );
        assert_eq!(
            text_step.agent_runtime.as_ref().unwrap()["evidence_observation"]["matches_runtime_evidence"],
            json!(true)
        );
        let package_step = workflow
            .steps
            .iter()
            .find(|step| step.operation == "evidence_package_create")
            .expect("package step");
        assert_eq!(
            package_step.agent_runtime.as_ref().unwrap()["content_source"],
            json!("agent-runtime-hermes-package-observation")
        );
        assert_eq!(
            package_step.agent_runtime.as_ref().unwrap()["package_observation"]["matches_runtime_package"],
            json!(true)
        );
        let review_step = workflow
            .steps
            .iter()
            .find(|step| step.operation == "review_answer")
            .expect("review step");
        assert_eq!(
            review_step.agent_runtime.as_ref().unwrap()["content_source"],
            json!("agent-runtime-hermes-review-observation")
        );
        assert_eq!(
            review_step.agent_runtime.as_ref().unwrap()["review_observation"]["local_reviewer_override"],
            json!(true)
        );
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "step_completed"
                && event.metadata["operation"] == json!("draft_answer")
                && event.metadata["agent_runtime"]["content_source"]
                    == json!("agent-runtime-hermes-profile")
        }));
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "step_completed"
                && event.metadata["agent_runtime"]["tool_result_count"] == json!(1)
        }));
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "step_completed"
                && event.metadata["agent_runtime"]["evidence_observation"]
                    ["matches_runtime_evidence"]
                    == json!(true)
        }));
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "step_completed"
                && event.metadata["agent_runtime"]["package_observation"]["matches_runtime_package"]
                    == json!(true)
        }));
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "step_completed"
                && event.metadata["agent_runtime"]["review_observation"]["local_reviewer_override"]
                    == json!(true)
        }));
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "content_delta"
                && event
                    .content_delta
                    .as_deref()
                    .is_some_and(|chunk| chunk.contains("证据不足"))
        }));
        assert_eq!(
            workflow.agent_runtime_summary["profile_execution_status"],
            "hermes_profile_observed_with_local_governance"
        );
        assert_eq!(
            workflow.agent_runtime_summary["hermes_content_execution_complete"],
            json!(true)
        );
        assert_eq!(
            workflow.agent_runtime_summary["draft_consumed"],
            json!(true)
        );
        assert_eq!(
            workflow.agent_runtime_summary["content_used_for_final_answer"],
            json!(false)
        );
        assert_eq!(
            workflow.agent_runtime_summary["tool_result_count"],
            json!(4)
        );
        assert_eq!(
            workflow.agent_runtime_summary["tool_audit_event_count"],
            json!(4)
        );
        let events = store
            .audit_events_for_trace(&workflow.trace_id)
            .expect("audit events");
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_draft_consumed"
                && event["payload"]["content_used_for_final_answer"] == json!(false)
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_step_executed"
                && event["payload"]["agent_runtime"]["tool_result_count"] == json!(1)
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_evidence_observed"
                && event["payload"]["matches_runtime_evidence"] == json!(true)
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_package_observed"
                && event["payload"]["matches_runtime_package"] == json!(true)
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_review_observed"
                && event["payload"]["local_reviewer_override"] == json!(true)
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_execution_summarized"
                && event["payload"]["profile_execution_status"]
                    == json!("hermes_profile_observed_with_local_governance")
                && event["payload"]["hermes_content_execution_complete"] == json!(true)
                && event["payload"]["tool_result_count"] == json!(4)
                && event["payload"]["tool_audit_event_count"] == json!(4)
        }));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn hermes_workflow_host_enforces_missing_runtime_tool_results() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-hermes-required-tools-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let workflow = store
            .execute_workflow_with_agent_runtime_client(
                RuntimeWorkflowInput {
                    trace_id: "trace-hermes-required-tools-test".to_string(),
                    question: "通灵玉是什么？".to_string(),
                    limit: 2,
                    required_evidence_types: vec!["base_text".to_string()],
                    profiles: RuntimeWorkflowProfiles::default(),
                },
                TonglingyuAgentRuntimeMode::Hermes,
                Arc::new(NoToolRuntimeClient),
            )
            .await
            .expect("Hermes profile steps should host-enforce required tool observations");

        assert_eq!(
            workflow.agent_runtime_summary["profile_execution_status"],
            json!("hermes_profile_observed_with_local_governance")
        );
        assert_eq!(
            workflow.agent_runtime_summary["tool_result_count"],
            json!(4)
        );
        assert_eq!(
            workflow.agent_runtime_summary["tool_audit_event_count"],
            json!(8)
        );
        assert!(workflow.steps.iter().all(|step| {
            step.agent_runtime
                .as_ref()
                .and_then(|value| value.get("tool_results"))
                .and_then(Value::as_array)
                .is_some_and(|items| {
                    items.iter().any(|item| {
                        item.get("host_enforced") == Some(&json!(true))
                            && item.get("tool_name").and_then(Value::as_str).is_some()
                    })
                })
        }));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn hermes_workflow_rejects_unbound_runtime_tool_output_refs() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-hermes-output-ref-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let error = store
            .execute_workflow_with_agent_runtime_client(
                RuntimeWorkflowInput {
                    trace_id: "trace-hermes-output-ref-test".to_string(),
                    question: "通灵玉是什么？".to_string(),
                    limit: 2,
                    required_evidence_types: vec!["base_text".to_string()],
                    profiles: RuntimeWorkflowProfiles::default(),
                },
                TonglingyuAgentRuntimeMode::Hermes,
                Arc::new(BadOutputRefRuntimeClient),
            )
            .await
            .expect_err("Hermes runtime tool output refs must bind to Tonglingyu runtime refs");

        let message = error.to_string();
        assert!(message.contains("invalid output_ref"));
        assert!(message.contains("text_evidence_search"));
        assert!(message.contains("tonglingyu.text.search"));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn hermes_workflow_rejects_mismatched_evidence_tool_output_refs() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-hermes-evidence-output-ref-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let error = store
            .execute_workflow_with_agent_runtime_client(
                RuntimeWorkflowInput {
                    trace_id: "trace-hermes-evidence-output-ref-test".to_string(),
                    question: "通灵玉是什么？".to_string(),
                    limit: 2,
                    required_evidence_types: vec!["base_text".to_string()],
                    profiles: RuntimeWorkflowProfiles::default(),
                },
                TonglingyuAgentRuntimeMode::Hermes,
                Arc::new(WrongEvidenceOutputRefRuntimeClient),
            )
            .await
            .expect_err("Hermes evidence tool output refs must bind to exact runtime evidence set");

        let message = error.to_string();
        assert!(message.contains("evidence tool"));
        assert!(message.contains("mismatched output_ref"));
        assert!(message.contains("text_evidence_search"));
        assert!(message.contains("tonglingyu.text.search"));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn hermes_workflow_rejects_incomplete_profile_content_execution() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-hermes-incomplete-content-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let trace_id = "trace-hermes-incomplete-content-test";
        let error = store
            .execute_workflow_with_agent_runtime_client(
                RuntimeWorkflowInput {
                    trace_id: trace_id.to_string(),
                    question: "通灵玉是什么？".to_string(),
                    limit: 2,
                    required_evidence_types: vec!["base_text".to_string()],
                    profiles: RuntimeWorkflowProfiles::default(),
                },
                TonglingyuAgentRuntimeMode::Hermes,
                Arc::new(IncompleteHermesContentRuntimeClient),
            )
            .await
            .expect_err(
                "Hermes mode must fail closed when profile content execution is incomplete",
            );

        let message = error.to_string();
        assert!(message.contains("Hermes runtime profile execution incomplete"));
        assert!(message.contains("hermes_profile_incomplete_local_governance"));
        let events = store
            .audit_events_for_trace(trace_id)
            .expect("audit events");
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_execution_summarized"
                && event["payload"]["profile_execution_status"]
                    == json!("hermes_profile_incomplete_local_governance")
                && event["payload"]["hermes_content_execution_complete"] == json!(false)
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_execution_rejected"
                && event["payload"]["summary"]["profile_execution_status"]
                    == json!("hermes_profile_incomplete_local_governance")
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_draft_rejected"
                && event["payload"]["draft_consumed"] == json!(false)
        }));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn hermes_workflow_audits_profile_backend_failure() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-hermes-profile-failure-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let trace_id = "trace-hermes-profile-failure-test";
        let error = store
            .execute_workflow_with_agent_runtime_client(
                RuntimeWorkflowInput {
                    trace_id: trace_id.to_string(),
                    question: "通灵玉是什么？".to_string(),
                    limit: 2,
                    required_evidence_types: vec!["base_text".to_string()],
                    profiles: RuntimeWorkflowProfiles::default(),
                },
                TonglingyuAgentRuntimeMode::Hermes,
                Arc::new(FailingProfileRuntimeClient),
            )
            .await
            .expect_err("Hermes mode must fail closed when a profile backend fails");

        assert!(error.to_string().contains("backend unavailable"));
        let events = store
            .audit_events_for_trace(trace_id)
            .expect("audit events");
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_execution_rejected"
                && event["payload"]["failure_stage"] == json!("agent_runtime_step_execution")
                && event["payload"]["runtime_mode"] == json!("hermes")
                && event["payload"]["profile_step_count"].as_u64().unwrap_or(0) > 0
                && event["payload"]["executed_profile_step_count"] == json!(0)
        }));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn hermes_workflow_rejects_missing_tool_audit_events() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-hermes-missing-tool-audit-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let trace_id = "trace-hermes-missing-tool-audit-test";
        let error = store
            .execute_workflow_with_agent_runtime_client(
                RuntimeWorkflowInput {
                    trace_id: trace_id.to_string(),
                    question: "通灵玉是什么？".to_string(),
                    limit: 2,
                    required_evidence_types: vec!["base_text".to_string()],
                    profiles: RuntimeWorkflowProfiles::default(),
                },
                TonglingyuAgentRuntimeMode::Hermes,
                Arc::new(MissingToolAuditRuntimeClient),
            )
            .await
            .expect_err("Hermes mode must fail closed when tool results are not audited");

        let message = error.to_string();
        assert!(message.contains("missing tool audit events"));
        assert!(message.contains("0/4"));
        let events = store
            .audit_events_for_trace(trace_id)
            .expect("audit events");
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_execution_summarized"
                && event["payload"]["profile_execution_status"]
                    == json!("hermes_profile_observed_with_local_governance")
                && event["payload"]["hermes_content_execution_complete"] == json!(true)
                && event["payload"]["tool_result_count"] == json!(4)
                && event["payload"]["tool_audit_event_count"] == json!(0)
        }));
        assert!(events.iter().any(|event| {
            event["event_type"] == "agent_runtime_profile_execution_rejected"
                && event["payload"]["error"]
                    == json!("Hermes runtime profile execution missing tool audit events: 0/4")
        }));
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn agent_runtime_profile_steps_execute_concurrently_and_preserve_step_order() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-agent-step-concurrency-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let mut workflow = store
            .execute_workflow(RuntimeWorkflowInput {
                trace_id: "trace-agent-runtime-step-concurrency-test".to_string(),
                question: "脂批如何评价通灵玉？".to_string(),
                limit: 3,
                required_evidence_types: vec!["base_text".to_string()],
                profiles: RuntimeWorkflowProfiles::default(),
            })
            .expect("workflow executes");
        let expected_step_ids = workflow
            .steps
            .iter()
            .map(|step| step.step_id.clone())
            .collect::<Vec<_>>();
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_active = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        attach_agent_runtime_step_execution(
            &mut workflow,
            &RuntimeWorkflowProfiles::default(),
            TonglingyuAgentRuntimeMode::Hermes,
            Arc::new(SlowDraftRuntimeClient::new(
                Arc::clone(&active),
                Arc::clone(&max_active),
            )),
        )
        .await
        .expect("profile steps execute");

        assert!(
            max_active.load(std::sync::atomic::Ordering::SeqCst) > 1,
            "profile steps should overlap instead of serializing"
        );
        assert_eq!(
            workflow
                .steps
                .iter()
                .map(|step| step.step_id.clone())
                .collect::<Vec<_>>(),
            expected_step_ids
        );
        assert!(
            workflow
                .steps
                .iter()
                .all(|step| step.agent_runtime.is_some())
        );
        for step in &workflow.steps {
            let agent_runtime = step.agent_runtime.as_ref().expect("agent runtime attached");
            let expected_runtime_step_id = format!("agent-runtime-{}", step.step_id);
            assert_eq!(
                agent_runtime["runtime_step"]["step_id"].as_str(),
                Some(expected_runtime_step_id.as_str())
            );
            assert_eq!(
                agent_runtime["runtime_step"]["metadata"]["workflow_step_id"].as_str(),
                Some(step.step_id.as_str())
            );
            assert_eq!(agent_runtime["client"].as_str(), Some("hermes"));
        }

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn runtime_tool_executor_runs_store_backed_tools() {
        let db_path = std::env::temp_dir().join(format!(
            "tonglingyu-runtime-tool-executor-{}.db",
            uuid::Uuid::now_v7().simple()
        ));
        let store = TonglingyuRuntimeStore::new(db_path.clone());
        {
            let conn = store.open_connection().expect("runtime conn");
            init_knowledge_base_schema(&conn).expect("kb schema");
        }
        let executor = TonglingyuRuntimeToolExecutor::new(store);
        let search = executor
            .execute_tool(
                RuntimeToolCall::new(
                    "honglou-text",
                    "tonglingyu.text.search",
                    json!({
                        "question": "通灵玉是什么？",
                        "limit": 2,
                        "required_evidence_types": ["base_text"],
                    }),
                    "trace-runtime-tool-executor-test",
                ),
                RuntimeToolSpec::read_only("tonglingyu.text.search"),
            )
            .await
            .expect("text search tool executes");
        assert_eq!(search.tool_name, "tonglingyu.text.search");
        assert!(search.output_ref.as_deref().is_some_and(|value| {
            value.starts_with("runtime://tonglingyu/trace-runtime-tool-executor-test/evidence/")
        }));
        assert_eq!(search.output["object"], "evidence_cards");
        assert_eq!(
            search.metadata["runtime_tool_executor"],
            "tonglingyu-runtime-store"
        );

        let package = executor
            .execute_tool(
                RuntimeToolCall::new(
                    "honglou-main",
                    "tonglingyu.evidence.package.create",
                    json!({
                        "trace_id": "trace-runtime-tool-executor-test",
                        "question": "脂批如何评价通灵玉？",
                        "cards": [sample_card("base_text")],
                    }),
                    "trace-runtime-tool-executor-test",
                ),
                RuntimeToolSpec::read_only("tonglingyu.evidence.package.create"),
            )
            .await
            .expect("package create tool executes");
        let package_id = package.output["package"]["package_id"]
            .as_str()
            .expect("package id")
            .to_string();
        assert!(package.output_ref.as_deref().is_some_and(|value| {
            value
                == format!(
                    "runtime://tonglingyu/trace-runtime-tool-executor-test/packages/{package_id}"
                )
        }));

        let read = executor
            .execute_tool(
                RuntimeToolCall::new(
                    "honglou-main",
                    "tonglingyu.evidence.package.read",
                    json!({"package_id": package_id}),
                    "trace-runtime-tool-executor-test",
                ),
                RuntimeToolSpec::read_only("tonglingyu.evidence.package.read"),
            )
            .await
            .expect("package read tool executes");
        assert_eq!(
            read.output["package"]["package_id"],
            package.output["package"]["package_id"]
        );
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    }

    #[tokio::test]
    async fn agent_runtime_plan_gate_executes_profile_contracts() {
        let report = execute_agent_runtime_plan_gate(AgentRuntimePlanGateInput {
            trace_id: "trace-agent-runtime-gate-test".to_string(),
            question: "脂批如何评价通灵玉？".to_string(),
            required_evidence_types: vec!["base_text".to_string(), "commentary".to_string()],
            profiles: RuntimeWorkflowProfiles::default(),
        })
        .await
        .expect("agent-runtime plan gate executes");

        assert_eq!(report.status, "passed");
        assert_eq!(report.agent_runtime_client, "minimal");
        assert_eq!(report.profile_contract_count, 4);
        assert_eq!(report.runtime_step_count, 5);
        assert_eq!(
            report.runtime_step_plan["owner"].as_str(),
            Some("domain_gateway")
        );
        assert!(
            report
                .runtime_step_outputs
                .as_array()
                .is_some_and(|outputs| {
                    outputs
                        .iter()
                        .any(|output| output["profile_id"] == "honglou-reviewer")
                })
        );
        assert!(
            report.requested_tools_by_profile["honglou-main"]
                .contains(&"tonglingyu.evidence.package.create".to_string())
        );
        assert!(
            report.requested_tools_by_profile["honglou-reviewer"]
                .contains(&"tonglingyu.evidence.package.read".to_string())
        );
    }
}
