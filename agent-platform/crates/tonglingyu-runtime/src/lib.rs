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
use rusqlite::{Connection, OptionalExtension, params};
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
pub const RUNTIME_WORKFLOW_PLAN_SCHEMA_VERSION: &str = "tonglingyu-runtime-step-plan-v1";
pub const RUNTIME_WORKFLOW_PLAN_POLICY_VERSION: &str = "tonglingyu-plan-policy-v1";

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
    pub source_root: String,
    pub source_count: i64,
    pub block_count: i64,
    pub schema_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStoreStats {
    pub sources: i64,
    pub blocks: i64,
    pub evidence_packages: i64,
    pub evidence_cards: i64,
    pub audit_events: i64,
    pub review_status: BTreeMap<String, i64>,
    pub evidence_types: BTreeMap<String, i64>,
    pub audit_event_types: BTreeMap<String, i64>,
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
        attach_agent_runtime_step_execution(&mut workflow, &input.profiles, mode, runtime).await?;
        let agent_runtime_content_application =
            apply_agent_runtime_content_outputs(&mut workflow, mode);
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
        if let Some(application) = agent_runtime_content_application {
            append_runtime_audit_event(
                &conn,
                &workflow.trace_id,
                "agent_runtime_profile_draft_consumed",
                &json!({
                    "answer_source": &workflow.answer_source,
                    "package_id": &workflow.package.package_id,
                    "review_status": &workflow.package.review.status,
                    "draft_profile": &input.profiles.main,
                    "runtime_mode": mode.as_str(),
                    "local_reviewer_enforced": true,
                    "content_used_for_final_answer": application.content_used_for_final_answer,
                }),
            )?;
        }
        Ok(workflow)
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

    pub fn package_ids_for_trace(&self, trace_id: &str) -> Result<Vec<String>> {
        let conn = self.open_connection()?;
        runtime_package_ids_for_trace(&conn, trace_id)
    }

    pub fn audit_events_for_trace(&self, trace_id: &str) -> Result<Vec<Value>> {
        let conn = self.open_connection()?;
        runtime_audit_events_for_trace(&conn, trace_id)
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
    api_url: Option<String>,
    fetched_at: Option<String>,
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
                "required_evidence_type": "commentary"
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
                "required": ["evidence_refs", "evidence_analysis", "unsupported_scope"],
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
                "required": ["commentary_refs", "commentary_analysis", "base_text_limits"],
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
                "required": ["draft_answer", "package_id", "claim_statements"],
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
                "required": ["review_status", "issues", "severity", "required_revisions"],
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
    runtime_profile_descriptors(profiles)
        .into_iter()
        .map(|descriptor| {
            let mut contract =
                AgentProfileContract::new(descriptor.profile.clone(), descriptor.version.clone());
            contract.input_schema = agent_runtime_profile_input_schema();
            contract.output_schema = agent_runtime_output_schema();
            contract.tool_policy = agent_runtime_tool_policy(descriptor.allowed_tools.clone());
            contract.max_context_messages = Some(16);
            contract.max_runtime_seconds = Some(5);
            contract.safety_policy = json!({
                "deny_message_roles": ["tool"],
                "max_message_bytes": 8192
            });
            contract
        })
        .collect()
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
        } => Ok(TonglingyuToolOutput::EvidenceCards {
            cards: search_evidence(conn, &question, limit, &required_evidence_types)?,
            tool_version: TOOL_CATALOG_VERSION.to_string(),
        }),
        TonglingyuToolCall::CommentarySearch { question, limit } => {
            Ok(TonglingyuToolOutput::EvidenceCards {
                cards: search_evidence(conn, &question, limit, &["commentary".to_string()])?,
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
    let mut text_required_types = input
        .required_evidence_types
        .iter()
        .filter(|item| item.as_str() != "commentary")
        .cloned()
        .collect::<Vec<_>>();
    if !text_required_types.iter().any(|item| item == "base_text") {
        text_required_types.push("base_text".to_string());
    }
    let text_started = Instant::now();
    let text_cards = match execute_tool(
        conn,
        TonglingyuToolCall::TextSearch {
            question: input.question.clone(),
            limit: input.limit,
            required_evidence_types: text_required_types,
        },
    )? {
        TonglingyuToolOutput::EvidenceCards { cards, .. } => cards,
        other => return Err(anyhow!("unexpected runtime tool output: {:?}", other)),
    };
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
            }),
        },
    )?);

    if input
        .required_evidence_types
        .iter()
        .any(|item| item == "commentary")
    {
        let commentary_started = Instant::now();
        let commentary_cards = match execute_tool(
            conn,
            TonglingyuToolCall::CommentarySearch {
                question: input.question.clone(),
                limit: input.limit,
            },
        )? {
            TonglingyuToolOutput::EvidenceCards { cards, .. } => cards,
            other => return Err(anyhow!("unexpected runtime tool output: {:?}", other)),
        };
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
    for step in &mut workflow.steps {
        let contract = contracts
            .get(&step.profile)
            .cloned()
            .ok_or_else(|| anyhow!("runtime profile contract missing for {}", step.profile))?;
        let runtime_step = agent_runtime_step_from_workflow_step(step);
        let output = runtime
            .execute_profile_step(RuntimeProfileInput {
                profile_id: step.profile.clone(),
                messages: vec![agent_runtime_profile_step_message(
                    &trace_id, &question, step,
                )],
                metadata: json!({
                    "runtime": "tonglingyu",
                    "workflow_step_id": &step.step_id,
                    "operation": &step.operation,
                    "input_ref": &step.input_ref,
                    "output_ref": &step.output_ref,
                    "step_output": &step.output,
                    "question_chars": question.chars().count(),
                    "question_sha256": hash_text(&question),
                    "content_source": "tonglingyu-deterministic-workflow",
                }),
                profile_contract: Some(contract),
                runtime_step: Some(runtime_step),
                requested_tools: step.allowed_tools.clone(),
                trace_id: trace_id.clone(),
            })
            .await?;
        let tool_results = output
            .metadata
            .get("tool_results")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let tool_audit_events = output
            .metadata
            .get("tool_audit_events")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let tool_result_count = tool_results.as_array().map_or(0, Vec::len);
        let tool_audit_event_count = tool_audit_events.as_array().map_or(0, Vec::len);
        step.agent_runtime = Some(json!({
            "client": mode.as_str(),
            "status": "executed",
            "content_source": "tonglingyu-deterministic-workflow",
            "executor_output_source": format!("agent-runtime-{}", mode.as_str()),
            "content_used_for_final_answer": false,
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
        }));
    }
    Ok(())
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
                "step_output_json: {step_output}\n"
            ),
            trace_id = trace_id,
            profile = &step.profile,
            operation = &step.operation,
            question = question,
            input_ref = step.input_ref.as_deref().unwrap_or("none"),
            output_ref = &step.output_ref,
            allowed_tools = step.allowed_tools.join(","),
            step_output = serde_json::to_string(&step.output).unwrap_or_else(|_| "{}".to_string()),
        ),
    )
}

#[derive(Debug, Clone, Copy)]
struct AgentRuntimeContentApplication {
    content_used_for_final_answer: bool,
}

fn apply_agent_runtime_content_outputs(
    workflow: &mut RuntimeWorkflowOutput,
    mode: TonglingyuAgentRuntimeMode,
) -> Option<AgentRuntimeContentApplication> {
    if mode != TonglingyuAgentRuntimeMode::Hermes {
        return None;
    }
    let (draft_step_index, draft) =
        workflow
            .steps
            .iter()
            .enumerate()
            .find_map(|(index, step)| {
                if step.operation != "draft_answer" {
                    return None;
                }
                let draft = step
                    .agent_runtime
                    .as_ref()
                    .and_then(|value| value.get("result_summary"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())?;
                Some((index, draft.to_string()))
            })?;

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
        if let Some(agent_runtime) = step.agent_runtime.as_mut().and_then(Value::as_object_mut) {
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
        content_used_for_final_answer,
    })
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

        CREATE INDEX IF NOT EXISTS idx_evidence_cards_package ON evidence_cards(package_id);
        CREATE INDEX IF NOT EXISTS idx_audit_events_trace ON audit_events(trace_id);
        "#,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (migration_id, applied_at) VALUES (?1, ?2)",
        params!["tonglingyu-runtime-schema-v1", now_rfc3339()],
    )?;
    Ok(())
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
        audit_events: table_count(conn, "audit_events")?,
        review_status: grouped_count_map(
            conn,
            "SELECT review_status, COUNT(*) FROM evidence_packages GROUP BY review_status",
        )?,
        evidence_types: grouped_count_map(
            conn,
            "SELECT evidence_type, COUNT(*) FROM evidence_cards GROUP BY evidence_type",
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

pub fn prune_runtime_data(conn: &Connection, retention_days: u32, dry_run: bool) -> Result<Value> {
    if retention_days == 0 {
        return Ok(json!({
            "object": "tonglingyu.runtime_prune_report",
            "status": "disabled",
            "retention_days": retention_days,
            "dry_run": dry_run,
        }));
    }
    let cutoff = (OffsetDateTime::now_utc() - time::Duration::days(retention_days as i64))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
    let old_packages = collect_string_column(
        conn,
        "SELECT package_id FROM evidence_packages WHERE created_at < ?1",
        &cutoff,
    )?;
    let counts = json!({
        "packages": old_packages.len(),
        "audit_events": count_where(conn, "audit_events", "created_at < ?1", &cutoff)?,
    });
    if dry_run {
        return Ok(json!({
            "object": "tonglingyu.runtime_prune_report",
            "status": "dry_run",
            "retention_days": retention_days,
            "cutoff": cutoff,
            "counts": counts,
        }));
    }
    for package_id in &old_packages {
        conn.execute(
            "DELETE FROM evidence_claim_links WHERE package_id = ?1",
            params![package_id],
        )?;
        conn.execute(
            "DELETE FROM review_records WHERE package_id = ?1",
            params![package_id],
        )?;
        conn.execute(
            "DELETE FROM evidence_cards WHERE package_id = ?1",
            params![package_id],
        )?;
        conn.execute(
            "DELETE FROM evidence_packages WHERE package_id = ?1",
            params![package_id],
        )?;
    }
    conn.execute(
        "DELETE FROM audit_events WHERE created_at < ?1",
        params![&cutoff],
    )?;
    Ok(json!({
        "object": "tonglingyu.runtime_prune_report",
        "status": "pruned",
        "retention_days": retention_days,
        "cutoff": cutoff,
        "counts": counts,
    }))
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
            api_url TEXT,
            fetched_at TEXT,
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

        CREATE INDEX IF NOT EXISTS idx_blocks_source ON blocks(source_id);
        CREATE INDEX IF NOT EXISTS idx_blocks_chapter ON blocks(chapter_no);
        CREATE INDEX IF NOT EXISTS idx_blocks_type ON blocks(evidence_type);
        CREATE INDEX IF NOT EXISTS idx_commentaries_source ON commentaries(source_id);
        "#,
    )?;
    Ok(())
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
    clear_knowledge_base_rows(conn)?;
    seed_aliases(conn)?;
    for source_dir in source_dirs {
        load_source_snapshot(conn, &source_dir)?;
    }
    write_kb_version(conn, source_root)
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
    conn.execute(
        r#"
        INSERT INTO sources (
            source_id, source_category, format, title, work, edition, language,
            api_url, fetched_at, notes, snapshot_contract_json, source_hash
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        "#,
        params![
            source.source_id,
            source.source_category,
            source.format,
            source.title,
            source.work,
            source.edition,
            source.language,
            source.api_url,
            source.fetched_at,
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
            usage_limit(&source.source_category),
        ],
    )?;
    conn.execute(
        "INSERT INTO version_notes (version_note_id, source_id, note, source_status, usage_limit) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            format!("version-note:{}", source.source_id),
            source.source_id,
            source.notes.unwrap_or_else(|| "第一批 Wikisource source snapshot".to_string()),
            "source_snapshot_ready",
            usage_limit(&source.source_category),
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

fn write_kb_version(conn: &Connection, source_root: &Path) -> Result<KnowledgeBaseBuildReport> {
    let source_count: i64 = conn.query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))?;
    let block_count: i64 = conn.query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))?;
    conn.execute(
        "INSERT INTO kb_version (version_id, source_root, source_count, block_count, schema_version, built_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            format!("kb-{}", uuid::Uuid::now_v7().simple()),
            source_root.display().to_string(),
            source_count,
            block_count,
            KNOWLEDGE_BASE_SCHEMA_VERSION,
            now_rfc3339(),
        ],
    )?;
    Ok(KnowledgeBaseBuildReport {
        source_root: source_root.display().to_string(),
        source_count,
        block_count,
        schema_version: KNOWLEDGE_BASE_SCHEMA_VERSION.to_string(),
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

fn count_where(conn: &Connection, table: &str, predicate: &str, value: &str) -> Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {predicate}");
    conn.query_row(&sql, params![value], |row| row.get(0))
        .map_err(Into::into)
}

fn collect_string_column(conn: &Connection, sql: &str, value: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    stmt.query_map(params![value], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()
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

pub fn search_evidence(
    conn: &Connection,
    question: &str,
    limit: usize,
    required_evidence_types: &[String],
) -> Result<Vec<EvidenceCard>> {
    let terms = extract_terms(conn, question)?;
    let mut scored: BTreeMap<String, (i64, EvidenceCard)> = BTreeMap::new();
    for term in &terms {
        for block in query_blocks_like(conn, term, limit * 4)? {
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
    for exact_term in required_exact_terms(question) {
        for block in query_blocks_exact_text(conn, exact_term, limit * 8)? {
            if !block.text.contains(exact_term) {
                continue;
            }
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
    Ok(cards)
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
    if cards.iter().all(|card| card.evidence_type == "commentary")
        && (question.contains("原文") || question.contains("正文"))
    {
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
    if (question.contains("脂批") || question.contains("脂評") || question.contains("甲戌"))
        && !cards.iter().any(|card| card.evidence_type == "commentary")
    {
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

fn required_exact_terms(question: &str) -> Vec<&'static str> {
    let mut terms = Vec::new();
    if question.contains("寳玉") {
        terms.push("寳玉");
    }
    if question.contains("寳釵") {
        terms.push("寳釵");
    }
    terms
}

fn extract_terms(conn: &Connection, question: &str) -> Result<Vec<String>> {
    let mut terms = Vec::new();
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
    let aliases = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for alias in aliases {
        let alias = alias?;
        if question.contains(&alias) || normalized.contains(&normalize_text(&alias)) {
            push_term(&mut terms, &alias);
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
    Ok(terms)
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
            WHEN source_id LIKE '%chengjia%' THEN 1
            WHEN source_id LIKE '%chengyi%' THEN 2
            ELSE 3
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
            let tool_rounds = if operation == "draft_answer" { 1 } else { 0 };
            let tool_results = if operation == "draft_answer" {
                json!([{
                    "call_id": "call-runtime-draft-package-read",
                    "profile_id": input.profile_id,
                    "tool_name": "tonglingyu.evidence.package.read",
                    "output_ref": format!("runtime://tool-results/{operation}"),
                }])
            } else {
                json!([])
            };
            let tool_audit_events = if operation == "draft_answer" {
                json!([{
                    "event": "runtime_tool_result",
                    "tool_name": "tonglingyu.evidence.package.read",
                    "trace_id": input.trace_id,
                }])
            } else {
                json!([])
            };
            Ok(RuntimeOutput {
                result_summary: if operation == "draft_answer" {
                    format!("Hermes full workflow draft from {operation}. context={message}")
                } else {
                    format!("Hermes full workflow step {operation}. context={message}")
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
        let claims = claims_from_cards("脂批原文如何评价石头？", &cards);
        let review = review("脂批原文如何评价石头？", &cards, &claims);
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
                    value["result_summary"].as_str().is_some_and(|summary| {
                        summary.contains("operation: draft_answer")
                            && summary.contains(&workflow.package.package_id)
                    })
                })
        }));
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "step_completed"
                && event.metadata["agent_runtime"]["status"] == "executed"
        }));
        let conn = store.open_connection().expect("runtime conn");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_events WHERE event_type = 'agent_runtime_profile_step_executed'",
                [],
                |row| row.get(0),
            )
            .expect("audit count");
        assert_eq!(count, workflow.steps.len() as i64);
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
        assert_eq!(draft_agent_runtime["tool_rounds"], json!(1));
        assert_eq!(draft_agent_runtime["tool_result_count"], json!(1));
        assert_eq!(draft_agent_runtime["tool_audit_event_count"], json!(1));
        assert_eq!(
            draft_agent_runtime["tool_results"][0]["tool_name"],
            "tonglingyu.evidence.package.read"
        );
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "step_completed"
                && event.metadata["agent_runtime"]["tool_result_count"] == json!(1)
        }));
        assert!(workflow.stream_events.iter().any(|event| {
            event.event_type == "content_delta"
                && event
                    .content_delta
                    .as_deref()
                    .is_some_and(|chunk| chunk.contains("证据不足"))
        }));
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
            value.starts_with("runtime://tonglingyu/trace-runtime-tool-executor-test/tools/")
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
