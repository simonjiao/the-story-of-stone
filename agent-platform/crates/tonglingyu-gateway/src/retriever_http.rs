use anyhow::{Context, Result, anyhow};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tonglingyu_runtime::EvidenceCard;

pub(crate) const SEARCH_PLAN_SCHEMA_VERSION: &str = "tonglingyu.agent_retriever.search_plan.v1";
pub(crate) const RETRIEVE_OPTIONS_SCHEMA_VERSION: &str =
    "tonglingyu.agent_retriever.retrieve_options.v1";
pub(crate) const SERVICE_SCHEMA_VERSION: &str = "tonglingyu.agent_retriever.service.v1";
pub(crate) const RETRIEVE_RESPONSE_SCHEMA_VERSION: &str =
    "tonglingyu.agent_retriever.retrieve_response.v1";
pub(crate) const EVIDENCE_PACK_SCHEMA_VERSION: &str = "tonglingyu.agent_retriever.evidence_pack.v1";
pub(crate) const EVIDENCE_DOC_SCHEMA_VERSION: &str = "tonglingyu.agent_retriever.evidence_doc.v1";
pub(crate) const HEALTH_SCHEMA_VERSION: &str = "tonglingyu.agent_retriever.service_health.v1";
pub(crate) const METADATA_SCHEMA_VERSION: &str = "tonglingyu.agent_retriever.service_metadata.v1";
pub(crate) const ERROR_RESPONSE_SCHEMA_VERSION: &str =
    "tonglingyu.agent_retriever.error_response.v1";
pub(crate) const WORKFLOW_RETRIEVAL_INPUT_SCHEMA_VERSION: &str =
    "tonglingyu.gateway.workflow_retrieval_input.v1";
pub(crate) const RETRIEVER_TOOL_NAME: &str = "tonglingyu.agent_retriever.retrieve_http";

const DEFAULT_ROUTES: &[&str] = &["bm25", "vector", "entity", "event", "poem", "commentary"];
const MAX_ROUTE_QUERY_REWRITES: usize = 8;
const MAX_PLAN_TERMS: usize = 16;
const RETRIEVE_RESPONSE_RECORD_TYPE: &str = "agent_retriever_retrieve_response";
const HEALTH_RECORD_TYPE: &str = "agent_retriever_service_health";
const METADATA_RECORD_TYPE: &str = "agent_retriever_service_metadata";
const ERROR_RECORD_TYPE: &str = "agent_retriever_error_response";

#[derive(Clone)]
pub(crate) struct RetrieverHttpClient {
    base_url: String,
    client: reqwest::Client,
}

impl RetrieverHttpClient {
    pub(crate) fn new(base_url: &str, timeout_secs: u64) -> Result<Self> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return Err(anyhow!("retriever base URL is empty"));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs.max(1)))
            .build()
            .context("build retriever HTTP client")?;
        Ok(Self { base_url, client })
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) async fn health(&self) -> Result<RetrieverHealthResponse> {
        let payload = self.get_json("/health").await?;
        validate_health_response_json(&payload)?;
        let response: RetrieverHealthResponse = parse_ok_payload(payload, HEALTH_SCHEMA_VERSION)?;
        response.validate()?;
        Ok(response)
    }

    pub(crate) async fn metadata(&self) -> Result<RetrieverMetadataResponse> {
        let payload = self.get_json("/metadata").await?;
        validate_metadata_response_json(&payload)?;
        let response: RetrieverMetadataResponse =
            parse_ok_payload(payload, METADATA_SCHEMA_VERSION)?;
        response.validate()?;
        Ok(response)
    }

    pub(crate) async fn retrieve(
        &self,
        request: &RetrieverRetrieveRequest,
    ) -> Result<RetrieverRetrieveResponse> {
        let payload = self.post_json("/retrieve", request).await?;
        parse_retrieve_response_payload(payload)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let response = self
            .client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .with_context(|| format!("retriever GET {path} failed"))?;
        response_json_or_error(response, path).await
    }

    async fn post_json<T: Serialize + ?Sized>(&self, path: &str, body: &T) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}{}", self.base_url, path))
            .json(body)
            .send()
            .await
            .with_context(|| format!("retriever POST {path} failed"))?;
        response_json_or_error(response, path).await
    }
}

async fn response_json_or_error(response: reqwest::Response, path: &str) -> Result<Value> {
    let status = response.status();
    let payload = response
        .json::<Value>()
        .await
        .with_context(|| format!("retriever {path} response was not JSON"))?;
    if !status.is_success() {
        return Err(retriever_error_from_payload(status, payload));
    }
    if payload.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(retriever_error_from_payload(status, payload));
    }
    Ok(payload)
}

fn retriever_error_from_payload(status: StatusCode, payload: Value) -> anyhow::Error {
    if let Ok(error) = serde_json::from_value::<RetrieverErrorResponse>(payload.clone()) {
        if error.schema_version != ERROR_RESPONSE_SCHEMA_VERSION {
            return anyhow!(
                "retriever error used unsupported schema {}: {}",
                error.schema_version,
                error.error.message
            );
        }
        if error.record_type != ERROR_RECORD_TYPE {
            return anyhow!(
                "retriever error used unsupported record_type {}: {}",
                error.record_type,
                error.error.message
            );
        }
        return anyhow!(
            "retriever {} failed: {}: {}",
            error.operation.unwrap_or_else(|| "request".to_string()),
            error.error.code,
            error.error.message
        );
    }
    anyhow!("retriever HTTP {status} failed: {payload}")
}

fn parse_ok_payload<T: for<'de> Deserialize<'de>>(payload: Value, schema: &str) -> Result<T> {
    match payload.get("ok").and_then(Value::as_bool) {
        Some(true) => {}
        Some(false) => return Err(retriever_error_from_payload(StatusCode::OK, payload)),
        None => return Err(anyhow!("retriever response missing ok=true")),
    }
    let payload_schema = payload
        .get("schema_version")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("retriever response missing schema_version"))?;
    if payload_schema != schema {
        return Err(anyhow!(
            "unsupported retriever response schema: {payload_schema}"
        ));
    }
    serde_json::from_value(payload).context("parse retriever response")
}

fn parse_retrieve_response_payload(payload: Value) -> Result<RetrieverRetrieveResponse> {
    validate_retrieve_response_json(&payload)?;
    let mut response: RetrieverRetrieveResponse =
        parse_ok_payload(payload.clone(), RETRIEVE_RESPONSE_SCHEMA_VERSION)?;
    response.raw_payload = payload;
    response.validate()?;
    Ok(response)
}

fn validate_health_response_json(payload: &Value) -> Result<()> {
    for field in [
        "ok",
        "schema_version",
        "record_type",
        "service",
        "status",
        "ready",
        "required_failed",
        "components",
    ] {
        require_json_field(payload, field, "health response")?;
    }
    Ok(())
}

fn validate_metadata_response_json(payload: &Value) -> Result<()> {
    for field in [
        "ok",
        "schema_version",
        "record_type",
        "service",
        "contracts",
        "capabilities",
        "adapter_guidance",
    ] {
        require_json_field(payload, field, "metadata response")?;
    }
    Ok(())
}

fn validate_retrieve_response_json(payload: &Value) -> Result<()> {
    for field in [
        "ok",
        "schema_version",
        "record_type",
        "service",
        "evidence_pack",
        "diagnostics",
    ] {
        require_json_field(payload, field, "retrieve response")?;
    }
    let pack = require_json_field(payload, "evidence_pack", "retrieve response")?;
    for field in [
        "schema_version",
        "query",
        "search_plan",
        "docs",
        "diagnostics",
        "sufficiency",
    ] {
        require_json_field(pack, field, "evidence_pack")?;
    }
    let diagnostics = require_json_field(pack, "diagnostics", "evidence_pack")?;
    for field in ["index_path", "route_errors", "routes", "fusion"] {
        require_json_field(diagnostics, field, "evidence_pack.diagnostics")?;
    }
    let fusion = require_json_field(diagnostics, "fusion", "evidence_pack.diagnostics")?;
    for field in ["route_counts", "fused_count", "rerank"] {
        require_json_field(fusion, field, "evidence_pack.diagnostics.fusion")?;
    }
    let rerank = require_json_field(fusion, "rerank", "evidence_pack.diagnostics.fusion")?;
    for field in ["enabled", "applied"] {
        require_json_field(rerank, field, "evidence_pack.diagnostics.fusion.rerank")?;
    }
    let sufficiency = require_json_field(pack, "sufficiency", "evidence_pack")?;
    for field in [
        "sufficient",
        "top_score",
        "doc_count",
        "direct_evidence_doc_count",
        "route_coverage",
        "reasons",
    ] {
        require_json_field(sufficiency, field, "evidence_pack.sufficiency")?;
    }
    let docs = require_json_field(pack, "docs", "evidence_pack")?
        .as_array()
        .ok_or_else(|| anyhow!("retriever response field evidence_pack.docs must be an array"))?;
    for (index, doc) in docs.iter().enumerate() {
        let context = format!("evidence_pack.docs[{index}]");
        for field in [
            "schema_version",
            "doc_id",
            "route",
            "content",
            "score",
            "source",
            "metadata",
            "refs",
            "routes",
            "display",
            "source_scope",
            "usage_policy",
            "evidence_card",
        ] {
            require_json_field(doc, field, &context)?;
        }
        let refs = require_json_field(doc, "refs", &context)?;
        for field in [
            "chunk_ids",
            "entity_ids",
            "event_ids",
            "theme_ids",
            "text_entity_ids",
            "segment_ids",
            "commentary_ids",
        ] {
            require_json_field(refs, field, &format!("{context}.refs"))?;
        }
    }
    Ok(())
}

fn require_json_field<'a>(value: &'a Value, field: &str, context: &str) -> Result<&'a Value> {
    value
        .get(field)
        .ok_or_else(|| anyhow!("retriever {context} missing required field {field}"))
}

fn validate_record_type(actual: &str, expected: &str, context: &str) -> Result<()> {
    if actual != expected {
        return Err(anyhow!(
            "unsupported retriever {context} record_type: {actual}"
        ));
    }
    Ok(())
}

fn validate_non_empty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("retriever response field {field} is empty"));
    }
    Ok(())
}

fn validate_canonical_route(route: &str, field: &str) -> Result<()> {
    if !DEFAULT_ROUTES.contains(&route) {
        return Err(anyhow!("unsupported retriever route in {field}: {route}"));
    }
    Ok(())
}

fn direct_evidence_doc_count(docs: &[RetrieverEvidenceDoc]) -> usize {
    docs.iter()
        .filter(|doc| !doc.refs.segment_ids.is_empty() || !doc.refs.commentary_ids.is_empty())
        .count()
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RetrieverRetrieveRequest {
    pub(crate) request_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) caller: String,
    pub(crate) graph_node: String,
    pub(crate) search_plan: RetrieverSearchPlan,
    pub(crate) retrieve_options: RetrieverRetrieveOptions,
    pub(crate) include_raw: bool,
    pub(crate) metadata: Value,
}

impl RetrieverRetrieveRequest {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn common_recall(
        request_id: String,
        session_id: Option<String>,
        kind: RetrieverCommonRecallKind,
        query: &str,
        top_k: usize,
        rerank: bool,
        include_raw: bool,
        metadata: Value,
        trace_level: &str,
        trace_doc_limit: usize,
    ) -> Self {
        Self {
            request_id,
            session_id,
            caller: "tonglingyu-gateway".to_string(),
            graph_node: kind.graph_node().to_string(),
            search_plan: RetrieverSearchPlan::for_common_recall(kind, query, top_k, rerank),
            retrieve_options: RetrieverRetrieveOptions::common_recall(trace_level, trace_doc_limit),
            include_raw,
            metadata,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetrieverCommonRecallKind {
    Workflow,
    Person,
    Event,
    Poem,
    JudgementPoem,
    Commentary,
}

impl RetrieverCommonRecallKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "workflow" | "all" => Some(Self::Workflow),
            "person" | "people" | "character" | "characters" | "entity" | "entity_lookup" => {
                Some(Self::Person)
            }
            "event" | "story" | "plot" | "event_lookup" => Some(Self::Event),
            "poem" | "poetry" | "poem_lookup" | "text" | "text_work" => Some(Self::Poem),
            "judgement" | "judgment" | "judgement_poem" | "judgment_poem" | "panci" => {
                Some(Self::JudgementPoem)
            }
            "commentary" | "commentary_lookup" | "zhiyanzhai" => Some(Self::Commentary),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Workflow => "workflow",
            Self::Person => "person",
            Self::Event => "event",
            Self::Poem => "poem",
            Self::JudgementPoem => "judgement_poem",
            Self::Commentary => "commentary",
        }
    }

    fn graph_node(self) -> &'static str {
        match self {
            Self::Workflow => "chat_workflow",
            Self::Person => "common_recall_person",
            Self::Event => "common_recall_event",
            Self::Poem => "common_recall_poem",
            Self::JudgementPoem => "common_recall_judgement_poem",
            Self::Commentary => "common_recall_commentary",
        }
    }

    fn route_policy(self) -> &'static str {
        match self {
            Self::Workflow => "all_knownledge_retriever_routes",
            Self::Person => "person_lookup_entity_first_with_event_context",
            Self::Event => "event_lookup_event_first_with_entity_context",
            Self::Poem => "poem_lookup_text_route_first",
            Self::JudgementPoem => "judgement_poem_lookup_text_route_with_commentary_context",
            Self::Commentary => "commentary_lookup_commentary_first",
        }
    }

    fn routes(self) -> &'static [&'static str] {
        match self {
            Self::Workflow => DEFAULT_ROUTES,
            Self::Person => &["entity", "event", "bm25", "vector"],
            Self::Event => &["event", "entity", "bm25", "vector"],
            Self::Poem => &["poem", "entity", "event", "commentary", "bm25", "vector"],
            Self::JudgementPoem => &["poem", "commentary", "event", "entity", "bm25", "vector"],
            Self::Commentary => &["commentary", "poem", "event", "bm25", "vector"],
        }
    }

    fn route_weights(self) -> BTreeMap<String, f64> {
        let entries: &[(&str, f64)] = match self {
            Self::Workflow => &[],
            Self::Person => &[
                ("entity", 1.4),
                ("event", 0.95),
                ("bm25", 1.0),
                ("vector", 0.9),
            ],
            Self::Event => &[
                ("event", 1.45),
                ("entity", 0.95),
                ("bm25", 1.0),
                ("vector", 0.9),
            ],
            Self::Poem => &[
                ("poem", 1.5),
                ("entity", 0.9),
                ("event", 0.85),
                ("commentary", 0.75),
                ("bm25", 1.0),
                ("vector", 0.9),
            ],
            Self::JudgementPoem => &[
                ("poem", 1.6),
                ("commentary", 1.05),
                ("event", 0.9),
                ("entity", 0.9),
                ("bm25", 1.0),
                ("vector", 0.9),
            ],
            Self::Commentary => &[
                ("commentary", 1.5),
                ("poem", 0.85),
                ("event", 0.8),
                ("bm25", 1.0),
                ("vector", 0.85),
            ],
        };
        entries
            .iter()
            .map(|(route, weight)| ((*route).to_string(), *weight))
            .collect()
    }

    fn queries(self, query: &str) -> BTreeMap<String, Vec<String>> {
        let mut queries = BTreeMap::new();
        match self {
            Self::Workflow => {}
            Self::Person => {
                queries.insert(
                    "entity".to_string(),
                    query_rewrites(query, &["人物", "别名 身份"]),
                );
                queries.insert(
                    "event".to_string(),
                    query_rewrites(query, &["相关事件", "经历"]),
                );
                queries.insert("bm25".to_string(), query_rewrites(query, &["人物", "关系"]));
                queries.insert(
                    "vector".to_string(),
                    query_rewrites(query, &["人物关系 经历"]),
                );
            }
            Self::Event => {
                queries.insert(
                    "event".to_string(),
                    query_rewrites(query, &["事件", "情节"]),
                );
                queries.insert("entity".to_string(), query_rewrites(query, &["相关人物"]));
                queries.insert("bm25".to_string(), query_rewrites(query, &["情节", "经过"]));
                queries.insert(
                    "vector".to_string(),
                    query_rewrites(query, &["事件经过 因果"]),
                );
            }
            Self::Poem => {
                queries.insert(
                    "poem".to_string(),
                    query_rewrites(query, &["诗词", "曲词 文本"]),
                );
                queries.insert("entity".to_string(), query_rewrites(query, &["诗词题名"]));
                queries.insert(
                    "event".to_string(),
                    query_rewrites(query, &["诗词相关情节"]),
                );
                queries.insert(
                    "commentary".to_string(),
                    query_rewrites(query, &["诗词 脂批"]),
                );
                queries.insert("bm25".to_string(), query_rewrites(query, &["诗词", "题名"]));
                queries.insert(
                    "vector".to_string(),
                    query_rewrites(query, &["诗词文本 含义"]),
                );
            }
            Self::JudgementPoem => {
                queries.insert(
                    "poem".to_string(),
                    query_rewrites(query, &["判词", "册页判语", "金陵十二钗 判词"]),
                );
                queries.insert(
                    "commentary".to_string(),
                    query_rewrites(query, &["判词 脂批", "册页判语 脂批"]),
                );
                queries.insert(
                    "event".to_string(),
                    query_rewrites(query, &["判词相关情节"]),
                );
                queries.insert("entity".to_string(), query_rewrites(query, &["判词人物"]));
                queries.insert("bm25".to_string(), query_rewrites(query, &["判词", "册页"]));
                queries.insert(
                    "vector".to_string(),
                    query_rewrites(query, &["判词 命运 伏笔"]),
                );
            }
            Self::Commentary => {
                queries.insert(
                    "commentary".to_string(),
                    query_rewrites(query, &["脂批", "批语"]),
                );
                queries.insert("poem".to_string(), query_rewrites(query, &["脂批所评诗文"]));
                queries.insert(
                    "event".to_string(),
                    query_rewrites(query, &["脂批所评情节"]),
                );
                queries.insert("bm25".to_string(), query_rewrites(query, &["脂批", "批语"]));
                queries.insert("vector".to_string(), query_rewrites(query, &["脂批 评论"]));
            }
        }
        queries
    }

    fn keyword_queries(self, query: &str) -> Vec<String> {
        match self {
            Self::Workflow => vec![query.to_string()],
            Self::Person => query_rewrites(query, &["人物", "身份", "关系"]),
            Self::Event => query_rewrites(query, &["事件", "情节", "经过"]),
            Self::Poem => query_rewrites(query, &["诗词", "题名", "曲词"]),
            Self::JudgementPoem => query_rewrites(query, &["判词", "册页判语", "金陵十二钗"]),
            Self::Commentary => query_rewrites(query, &["脂批", "批语", "评点"]),
        }
    }

    fn semantic_queries(self, query: &str) -> Vec<String> {
        match self {
            Self::Workflow => vec![query.to_string()],
            Self::Person => query_rewrites(query, &["人物身份经历关系"]),
            Self::Event => query_rewrites(query, &["情节事件经过因果"]),
            Self::Poem => query_rewrites(query, &["诗词曲文文本含义"]),
            Self::JudgementPoem => query_rewrites(query, &["判词册页人物命运伏笔"]),
            Self::Commentary => query_rewrites(query, &["脂批评点解释"]),
        }
    }

    fn structured_terms(self) -> Vec<String> {
        match self {
            Self::Workflow => Vec::new(),
            Self::Person => plan_terms(&["entity_type:person", "entity_class:person"]),
            Self::Event => plan_terms(&["route_view:event_route_view", "chunk_kind:event_scene"]),
            Self::Poem => plan_terms(&["entity_type:text", "entity_subtype:poem", "route:poem"]),
            Self::JudgementPoem => plan_terms(&[
                "entity_type:text",
                "entity_subtype:judgement",
                "entity_facets:judgment",
                "route:poem",
            ]),
            Self::Commentary => plan_terms(&[
                "route:commentary",
                "chunk_kind:commentary",
                "source:zhiyanzhai",
            ]),
        }
    }

    fn expansion_terms(self) -> Vec<String> {
        match self {
            Self::Workflow => Vec::new(),
            Self::Person => plan_terms(&["人物", "别名", "身份", "关系", "经历"]),
            Self::Event => plan_terms(&["事件", "情节", "经过", "因果", "场景"]),
            Self::Poem => plan_terms(&["诗词", "曲词", "题名", "文本实体"]),
            Self::JudgementPoem => plan_terms(&["判词", "册页判语", "金陵十二钗", "正册", "副册"]),
            Self::Commentary => plan_terms(&["脂批", "批语", "评点", "眉批", "夹批"]),
        }
    }

    fn filters(self) -> Option<Value> {
        match self {
            Self::Workflow => None,
            Self::Person => Some(json!({
                "entity_types": ["person"],
            })),
            Self::Event => Some(json!({
                "chunk_kinds": ["event_scene", "primary_text_segment", "primary_text_window", "theme_anchor"],
            })),
            Self::Poem => Some(json!({
                "chunk_kinds": ["text_work", "text_work_body", "text_work_body_line", "primary_text_segment", "primary_text_window"],
                "entity_types": ["text"],
                "entity_subtypes": ["poem", "song"],
            })),
            Self::JudgementPoem => Some(json!({
                "chunk_kinds": ["text_work", "text_work_body", "text_work_body_line", "primary_text_segment", "primary_text_window", "commentary"],
                "entity_types": ["text"],
                "entity_subtypes": ["judgement"],
                "entity_facets": ["judgment"],
            })),
            Self::Commentary => Some(json!({
                "chunk_kinds": ["commentary", "commentary_anchor", "commentary_target_span"],
            })),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RetrieverSearchPlan {
    pub(crate) schema_version: String,
    pub(crate) query: String,
    pub(crate) routes: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub(crate) route_weights: BTreeMap<String, f64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub(crate) queries: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) keyword_queries: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) semantic_queries: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) structured_terms: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) expansion_terms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) explicit_scope_allowed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) filters: Option<Value>,
    pub(crate) top_k: usize,
    pub(crate) candidate_limit: usize,
    pub(crate) route_record_limit: usize,
    pub(crate) route_doc_limit: usize,
    pub(crate) vector_top_k: usize,
    pub(crate) include_cards: bool,
    pub(crate) rerank: bool,
    pub(crate) rerank_top_k: usize,
    pub(crate) fail_on_route_error: bool,
    pub(crate) fail_on_rerank_error: bool,
    #[serde(default)]
    pub(crate) raw_plan: Value,
}

impl RetrieverSearchPlan {
    pub(crate) fn for_workflow(query: &str, top_k: usize, rerank: bool) -> Self {
        Self::for_common_recall(RetrieverCommonRecallKind::Workflow, query, top_k, rerank)
    }

    pub(crate) fn for_common_recall(
        kind: RetrieverCommonRecallKind,
        query: &str,
        top_k: usize,
        rerank: bool,
    ) -> Self {
        let top_k = top_k.max(1);
        Self {
            schema_version: SEARCH_PLAN_SCHEMA_VERSION.to_string(),
            query: query.to_string(),
            routes: kind
                .routes()
                .iter()
                .map(|route| (*route).to_string())
                .collect(),
            route_weights: kind.route_weights(),
            queries: kind.queries(query),
            keyword_queries: kind.keyword_queries(query),
            semantic_queries: kind.semantic_queries(query),
            structured_terms: kind.structured_terms(),
            expansion_terms: kind.expansion_terms(),
            explicit_scope_allowed: None,
            filters: kind.filters(),
            top_k,
            candidate_limit: (top_k * 20).clamp(20, 160),
            route_record_limit: 10,
            route_doc_limit: 4,
            vector_top_k: (top_k * 10).clamp(10, 80),
            include_cards: true,
            rerank,
            rerank_top_k: top_k.max(8),
            fail_on_route_error: true,
            fail_on_rerank_error: true,
            raw_plan: json!({
                "planner": "tonglingyu-gateway",
                "common_recall_kind": kind.as_str(),
                "route_policy": kind.route_policy(),
                "vector_required": true,
            }),
        }
    }
}

fn query_rewrites(query: &str, suffixes: &[&str]) -> Vec<String> {
    let values = std::iter::once(query.to_string())
        .chain(suffixes.iter().map(|suffix| format!("{query} {suffix}")));
    dedupe_limited(values, MAX_ROUTE_QUERY_REWRITES)
}

fn plan_terms(terms: &[&str]) -> Vec<String> {
    dedupe_limited(terms.iter().map(|term| (*term).to_string()), MAX_PLAN_TERMS)
}

fn dedupe_limited(values: impl IntoIterator<Item = String>, max_items: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in values {
        let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        out.push(normalized);
        if out.len() >= max_items {
            break;
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RetrieverRetrieveOptions {
    pub(crate) schema_version: String,
    pub(crate) trace_level: String,
    pub(crate) trace_doc_limit: usize,
    pub(crate) include_ref_audit: bool,
    #[serde(default)]
    pub(crate) expected_refs: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub(crate) forbidden_refs: BTreeMap<String, Vec<String>>,
    pub(crate) audit_k_values: Vec<usize>,
}

impl RetrieverRetrieveOptions {
    pub(crate) fn workflow(trace_doc_limit: usize) -> Self {
        Self::common_recall("route", trace_doc_limit)
    }

    pub(crate) fn common_recall(trace_level: &str, trace_doc_limit: usize) -> Self {
        Self {
            schema_version: RETRIEVE_OPTIONS_SCHEMA_VERSION.to_string(),
            trace_level: trace_level.to_string(),
            trace_doc_limit: trace_doc_limit.max(1),
            include_ref_audit: false,
            expected_refs: BTreeMap::new(),
            forbidden_refs: BTreeMap::new(),
            audit_k_values: vec![5, 10, 20],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverRetrieveResponse {
    pub(crate) ok: bool,
    pub(crate) schema_version: String,
    pub(crate) record_type: String,
    pub(crate) request_id: Option<String>,
    #[serde(default)]
    pub(crate) request_context: Value,
    pub(crate) service: RetrieverServiceDescriptor,
    pub(crate) evidence_pack: RetrieverEvidencePack,
    #[serde(default)]
    pub(crate) diagnostics: Value,
    #[serde(default, skip_serializing)]
    pub(crate) raw_payload: Value,
}

impl RetrieverRetrieveResponse {
    fn validate(&self) -> Result<()> {
        if !self.ok {
            return Err(anyhow!("retriever retrieve response was not ok"));
        }
        validate_record_type(
            &self.record_type,
            RETRIEVE_RESPONSE_RECORD_TYPE,
            "retrieve response",
        )?;
        if let Some(request_id) = self.request_id.as_deref() {
            validate_non_empty(request_id, "request_id")?;
        }
        self.service.validate()?;
        if !self.diagnostics.is_object() {
            return Err(anyhow!(
                "retriever retrieve response diagnostics must be an object"
            ));
        }
        self.evidence_pack.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverHealthResponse {
    pub(crate) ok: bool,
    pub(crate) schema_version: String,
    pub(crate) record_type: String,
    pub(crate) service: RetrieverServiceDescriptor,
    pub(crate) status: String,
    pub(crate) ready: bool,
    #[serde(default)]
    pub(crate) required_failed: Vec<String>,
    #[serde(default)]
    pub(crate) components: Value,
}

impl RetrieverHealthResponse {
    fn validate(&self) -> Result<()> {
        if !self.ok {
            return Err(anyhow!("retriever health response was not ok"));
        }
        validate_record_type(&self.record_type, HEALTH_RECORD_TYPE, "health response")?;
        self.service.validate()?;
        validate_non_empty(&self.status, "health.status")?;
        if !self.components.is_object() {
            return Err(anyhow!("retriever health components must be an object"));
        }
        validate_unique_strings(&self.required_failed, "health.required_failed")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverMetadataResponse {
    pub(crate) ok: bool,
    pub(crate) schema_version: String,
    pub(crate) record_type: String,
    pub(crate) service: RetrieverServiceDescriptor,
    #[serde(default)]
    pub(crate) contracts: Value,
    #[serde(default)]
    pub(crate) capabilities: Value,
    #[serde(default)]
    pub(crate) adapter_guidance: Value,
}

impl RetrieverMetadataResponse {
    fn validate(&self) -> Result<()> {
        if !self.ok {
            return Err(anyhow!("retriever metadata response was not ok"));
        }
        validate_record_type(&self.record_type, METADATA_RECORD_TYPE, "metadata response")?;
        self.service.validate()?;
        validate_contract_value(
            &self.contracts,
            "search_plan_schema",
            SEARCH_PLAN_SCHEMA_VERSION,
        )?;
        validate_contract_value(
            &self.contracts,
            "retrieve_options_schema",
            RETRIEVE_OPTIONS_SCHEMA_VERSION,
        )?;
        validate_contract_value(
            &self.contracts,
            "evidence_pack_schema",
            EVIDENCE_PACK_SCHEMA_VERSION,
        )?;
        validate_contract_value(
            &self.contracts,
            "retrieve_response_schema",
            RETRIEVE_RESPONSE_SCHEMA_VERSION,
        )?;
        validate_contract_value(
            &self.contracts,
            "error_response_schema",
            ERROR_RESPONSE_SCHEMA_VERSION,
        )?;
        let routes = string_array_from_value(&self.capabilities, "routes")?;
        for route in DEFAULT_ROUTES {
            if !routes.iter().any(|item| item == route) {
                return Err(anyhow!(
                    "retriever metadata capabilities.routes missing required route {route}"
                ));
            }
        }
        let required_routes = string_array_from_value(&self.capabilities, "required_routes")?;
        if !required_routes.iter().any(|route| route == "vector") {
            return Err(anyhow!(
                "retriever metadata capabilities.required_routes must include vector"
            ));
        }
        let stable_input = self
            .adapter_guidance
            .get("stable_input")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !stable_input.contains("SearchPlan") || !stable_input.contains("RetrieveOptions") {
            return Err(anyhow!(
                "retriever metadata adapter_guidance.stable_input must name SearchPlan + RetrieveOptions"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverErrorResponse {
    pub(crate) ok: bool,
    pub(crate) schema_version: String,
    pub(crate) record_type: String,
    pub(crate) request_id: Option<String>,
    pub(crate) operation: Option<String>,
    #[serde(default)]
    pub(crate) service: Option<RetrieverServiceDescriptor>,
    pub(crate) error: RetrieverErrorPayload,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverErrorPayload {
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) r#type: Option<String>,
    #[serde(default)]
    pub(crate) retryable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverServiceDescriptor {
    pub(crate) schema_version: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) repo_root: String,
    pub(crate) index_path: String,
    pub(crate) env_file: String,
}

impl RetrieverServiceDescriptor {
    fn validate(&self) -> Result<()> {
        if self.schema_version != SERVICE_SCHEMA_VERSION {
            return Err(anyhow!(
                "unsupported retriever service schema: {}",
                self.schema_version
            ));
        }
        validate_non_empty(&self.name, "service.name")?;
        validate_non_empty(&self.version, "service.version")?;
        validate_non_empty(&self.repo_root, "service.repo_root")?;
        validate_non_empty(&self.index_path, "service.index_path")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverEvidencePack {
    pub(crate) schema_version: String,
    pub(crate) query: String,
    pub(crate) search_plan: RetrieverNormalizedSearchPlan,
    #[serde(default)]
    pub(crate) docs: Vec<RetrieverEvidenceDoc>,
    pub(crate) diagnostics: RetrieverPackDiagnostics,
    pub(crate) sufficiency: RetrieverSufficiency,
}

impl RetrieverEvidencePack {
    fn validate(&self) -> Result<()> {
        if self.schema_version != EVIDENCE_PACK_SCHEMA_VERSION {
            return Err(anyhow!(
                "unsupported retriever evidence pack schema: {}",
                self.schema_version
            ));
        }
        validate_non_empty(&self.query, "evidence_pack.query")?;
        self.search_plan.validate()?;
        self.diagnostics.validate(self.docs.len())?;
        for (index, doc) in self.docs.iter().enumerate() {
            doc.validate(index)?;
        }
        self.sufficiency.validate(&self.docs)?;
        if self.docs.len() > self.search_plan.top_k {
            return Err(anyhow!(
                "retriever evidence_pack.docs length {} exceeds normalized search_plan.top_k {}",
                self.docs.len(),
                self.search_plan.top_k
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverNormalizedSearchPlan {
    pub(crate) schema_version: String,
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) routes: Vec<String>,
    #[serde(default)]
    pub(crate) route_weights: BTreeMap<String, f64>,
    #[serde(default)]
    pub(crate) queries: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub(crate) keyword_queries: Vec<String>,
    #[serde(default)]
    pub(crate) semantic_queries: Vec<String>,
    #[serde(default)]
    pub(crate) structured_terms: Vec<String>,
    #[serde(default)]
    pub(crate) expansion_terms: Vec<String>,
    #[serde(default)]
    pub(crate) filters: Option<Value>,
    pub(crate) explicit_scope_allowed: bool,
    pub(crate) top_k: usize,
    pub(crate) candidate_limit: usize,
    pub(crate) route_record_limit: usize,
    pub(crate) route_doc_limit: usize,
    pub(crate) vector_top_k: usize,
    pub(crate) include_cards: bool,
    pub(crate) rerank: bool,
    pub(crate) rerank_top_k: usize,
    #[serde(default)]
    pub(crate) raw_plan: Value,
}

impl RetrieverNormalizedSearchPlan {
    fn validate(&self) -> Result<()> {
        if self.schema_version != SEARCH_PLAN_SCHEMA_VERSION {
            return Err(anyhow!(
                "unsupported retriever normalized search_plan schema: {}",
                self.schema_version
            ));
        }
        validate_non_empty(&self.query, "search_plan.query")?;
        if self.routes.is_empty() {
            return Err(anyhow!("retriever normalized search_plan.routes is empty"));
        }
        validate_unique_strings(&self.routes, "search_plan.routes")?;
        let mut has_vector = false;
        for route in &self.routes {
            validate_canonical_route(route, "search_plan.routes")?;
            has_vector |= route == "vector";
        }
        if !has_vector {
            return Err(anyhow!(
                "retriever normalized search_plan.routes must include vector"
            ));
        }
        validate_unique_strings(&self.keyword_queries, "search_plan.keyword_queries")?;
        validate_unique_strings(&self.semantic_queries, "search_plan.semantic_queries")?;
        validate_unique_strings(&self.structured_terms, "search_plan.structured_terms")?;
        validate_unique_strings(&self.expansion_terms, "search_plan.expansion_terms")?;
        if let Some(filters) = &self.filters {
            if !filters.is_object() {
                return Err(anyhow!(
                    "retriever normalized search_plan.filters must be an object"
                ));
            }
        }
        if self.keyword_queries.is_empty() || self.semantic_queries.is_empty() {
            return Err(anyhow!(
                "retriever normalized search_plan must include keyword_queries and semantic_queries"
            ));
        }
        if self.top_k == 0
            || self.candidate_limit == 0
            || self.route_record_limit == 0
            || self.route_doc_limit == 0
            || self.vector_top_k == 0
            || self.rerank_top_k == 0
        {
            return Err(anyhow!(
                "retriever normalized search_plan limit fields must be positive"
            ));
        }
        if !self.include_cards {
            return Err(anyhow!(
                "retriever normalized search_plan.include_cards must be true for gateway workflow"
            ));
        }
        if !self.raw_plan.is_object() {
            return Err(anyhow!(
                "retriever normalized search_plan.raw_plan must be an object"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverEvidenceDoc {
    pub(crate) schema_version: String,
    pub(crate) doc_id: String,
    pub(crate) route: String,
    pub(crate) content: String,
    pub(crate) score: f64,
    #[serde(default)]
    pub(crate) source: RetrieverEvidenceSource,
    #[serde(default)]
    pub(crate) metadata: Value,
    #[serde(default)]
    pub(crate) refs: RetrieverEvidenceRefs,
    #[serde(default)]
    pub(crate) routes: Vec<RetrieverRouteHit>,
    #[serde(default)]
    pub(crate) display: RetrieverEvidenceDisplay,
    #[serde(default)]
    pub(crate) source_scope: Value,
    #[serde(default)]
    pub(crate) usage_policy: Value,
    #[serde(default)]
    pub(crate) evidence_card: Option<Value>,
    #[serde(default)]
    pub(crate) raw_candidate: Option<Value>,
}

impl RetrieverEvidenceDoc {
    fn validate(&self, index: usize) -> Result<()> {
        let context = format!("evidence_pack.docs[{index}]");
        if self.schema_version != EVIDENCE_DOC_SCHEMA_VERSION {
            return Err(anyhow!(
                "unsupported retriever evidence doc schema at {context}: {}",
                self.schema_version
            ));
        }
        validate_non_empty(&self.doc_id, &format!("{context}.doc_id"))?;
        validate_canonical_route(&self.route, &format!("{context}.route"))?;
        if !self.score.is_finite() {
            return Err(anyhow!("retriever {context}.score must be finite"));
        }
        if !self.metadata.is_object() {
            return Err(anyhow!("retriever {context}.metadata must be an object"));
        }
        if !self.source_scope.is_object() {
            return Err(anyhow!(
                "retriever {context}.source_scope must be an object"
            ));
        }
        if !self.usage_policy.is_object() {
            return Err(anyhow!(
                "retriever {context}.usage_policy must be an object"
            ));
        }
        self.source.validate(&format!("{context}.source"))?;
        self.refs.validate(&format!("{context}.refs"))?;
        for (route_index, route) in self.routes.iter().enumerate() {
            route.validate(&format!("{context}.routes[{route_index}]"))?;
        }
        if evidence_text_for_doc(self).trim().is_empty() {
            return Err(anyhow!(
                "retriever {context} has no usable evidence text in content/display"
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct RetrieverEvidenceSource {
    #[serde(default)]
    pub(crate) work: Option<String>,
    #[serde(default)]
    pub(crate) chapter_no: Option<i64>,
    #[serde(default)]
    pub(crate) chapter_scope: Option<String>,
    #[serde(default)]
    pub(crate) version_scope: Option<String>,
    #[serde(default)]
    pub(crate) source_label: Option<String>,
    #[serde(default)]
    pub(crate) citation_hint: Option<String>,
    #[serde(default)]
    pub(crate) source_ids: Vec<String>,
    #[serde(default)]
    pub(crate) edition_ids: Vec<String>,
    #[serde(default)]
    pub(crate) route_view: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

impl RetrieverEvidenceSource {
    fn validate(&self, context: &str) -> Result<()> {
        if let Some(chapter_no) = self.chapter_no {
            if chapter_no < 1 {
                return Err(anyhow!("retriever {context}.chapter_no must be >= 1"));
            }
        }
        validate_unique_strings(&self.source_ids, &format!("{context}.source_ids"))?;
        validate_unique_strings(&self.edition_ids, &format!("{context}.edition_ids"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct RetrieverEvidenceRefs {
    #[serde(default)]
    pub(crate) chunk_ids: Vec<String>,
    #[serde(default)]
    pub(crate) entity_ids: Vec<String>,
    #[serde(default)]
    pub(crate) event_ids: Vec<String>,
    #[serde(default)]
    pub(crate) theme_ids: Vec<String>,
    #[serde(default)]
    pub(crate) text_entity_ids: Vec<String>,
    #[serde(default)]
    pub(crate) segment_ids: Vec<String>,
    #[serde(default)]
    pub(crate) commentary_ids: Vec<String>,
}

impl RetrieverEvidenceRefs {
    fn validate(&self, context: &str) -> Result<()> {
        validate_unique_strings(&self.chunk_ids, &format!("{context}.chunk_ids"))?;
        validate_unique_strings(&self.entity_ids, &format!("{context}.entity_ids"))?;
        validate_unique_strings(&self.event_ids, &format!("{context}.event_ids"))?;
        validate_unique_strings(&self.theme_ids, &format!("{context}.theme_ids"))?;
        validate_unique_strings(&self.text_entity_ids, &format!("{context}.text_entity_ids"))?;
        validate_unique_strings(&self.segment_ids, &format!("{context}.segment_ids"))?;
        validate_unique_strings(&self.commentary_ids, &format!("{context}.commentary_ids"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub(crate) struct RetrieverEvidenceDisplay {
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) summary_text: Option<String>,
    #[serde(default)]
    pub(crate) quote_text: Option<String>,
    #[serde(default)]
    pub(crate) context_text: Option<String>,
    #[serde(default)]
    pub(crate) source_label: Option<String>,
    #[serde(default)]
    pub(crate) citation_hint: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverRouteHit {
    pub(crate) route: String,
    #[serde(default)]
    pub(crate) record_id: Option<String>,
    #[serde(default)]
    pub(crate) record_title: Option<String>,
    #[serde(default)]
    pub(crate) record_rank: Option<usize>,
    #[serde(default)]
    pub(crate) chunk_rank: Option<usize>,
    #[serde(default)]
    pub(crate) surface_id: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

impl RetrieverRouteHit {
    fn validate(&self, context: &str) -> Result<()> {
        validate_non_empty(&self.route, &format!("{context}.route"))?;
        if self.record_rank == Some(0) {
            return Err(anyhow!("retriever {context}.record_rank must be >= 1"));
        }
        if self.chunk_rank == Some(0) {
            return Err(anyhow!("retriever {context}.chunk_rank must be >= 1"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverPackDiagnostics {
    pub(crate) index_path: String,
    #[serde(default)]
    pub(crate) route_errors: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) routes: BTreeMap<String, Value>,
    pub(crate) fusion: RetrieverFusionDiagnostics,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

impl RetrieverPackDiagnostics {
    fn validate(&self, returned_doc_count: usize) -> Result<()> {
        validate_non_empty(&self.index_path, "evidence_pack.diagnostics.index_path")?;
        for route in self.route_errors.keys() {
            validate_non_empty(route, "evidence_pack.diagnostics.route_errors key")?;
        }
        for route in self.routes.keys() {
            validate_non_empty(route, "evidence_pack.diagnostics.routes key")?;
        }
        self.fusion.validate(returned_doc_count)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverFusionDiagnostics {
    #[serde(default)]
    pub(crate) route_counts: BTreeMap<String, usize>,
    pub(crate) fused_count: usize,
    pub(crate) rerank: RetrieverRerankDiagnostics,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

impl RetrieverFusionDiagnostics {
    fn validate(&self, returned_doc_count: usize) -> Result<()> {
        for route in self.route_counts.keys() {
            validate_non_empty(route, "evidence_pack.diagnostics.fusion.route_counts key")?;
        }
        if self.fused_count < returned_doc_count {
            return Err(anyhow!(
                "retriever fusion.fused_count {} is smaller than returned doc count {}",
                self.fused_count,
                returned_doc_count
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverRerankDiagnostics {
    pub(crate) enabled: bool,
    pub(crate) applied: bool,
    #[serde(default)]
    pub(crate) error: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct RetrieverSufficiency {
    pub(crate) sufficient: bool,
    pub(crate) top_score: f64,
    pub(crate) doc_count: usize,
    pub(crate) direct_evidence_doc_count: usize,
    #[serde(default)]
    pub(crate) route_coverage: Vec<String>,
    #[serde(default)]
    pub(crate) reasons: Vec<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

impl RetrieverSufficiency {
    fn validate(&self, docs: &[RetrieverEvidenceDoc]) -> Result<()> {
        if !self.top_score.is_finite() {
            return Err(anyhow!("retriever sufficiency.top_score must be finite"));
        }
        if self.doc_count != docs.len() {
            return Err(anyhow!(
                "retriever sufficiency.doc_count {} does not match docs length {}",
                self.doc_count,
                docs.len()
            ));
        }
        let actual_direct = direct_evidence_doc_count(docs);
        if self.direct_evidence_doc_count != actual_direct {
            return Err(anyhow!(
                "retriever sufficiency.direct_evidence_doc_count {} does not match refs-derived count {}",
                self.direct_evidence_doc_count,
                actual_direct
            ));
        }
        validate_unique_strings(&self.route_coverage, "sufficiency.route_coverage")?;
        validate_unique_strings(&self.reasons, "sufficiency.reasons")?;
        Ok(())
    }
}

fn validate_contract_value(contracts: &Value, field: &str, expected: &str) -> Result<()> {
    let actual = contracts
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("retriever metadata contracts.{field} must be a string"))?;
    if actual != expected {
        return Err(anyhow!(
            "retriever metadata contracts.{field} mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

fn string_array_from_value(value: &Value, field: &str) -> Result<Vec<String>> {
    let items = value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("retriever metadata capabilities.{field} must be an array"))?;
    let mut strings = Vec::with_capacity(items.len());
    for item in items {
        let text = item
            .as_str()
            .ok_or_else(|| {
                anyhow!("retriever metadata capabilities.{field} items must be strings")
            })?
            .trim();
        if text.is_empty() {
            return Err(anyhow!(
                "retriever metadata capabilities.{field} items must be non-empty"
            ));
        }
        strings.push(text.to_string());
    }
    validate_unique_strings(&strings, &format!("metadata.capabilities.{field}"))?;
    Ok(strings)
}

fn validate_unique_strings(values: &[String], field: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(anyhow!(
                "retriever response field {field} contains empty string"
            ));
        }
        if !seen.insert(trimmed.to_string()) {
            return Err(anyhow!(
                "retriever response field {field} contains duplicate string {trimmed}"
            ));
        }
    }
    Ok(())
}

pub(crate) fn workflow_retrieval_input(
    response: &RetrieverRetrieveResponse,
    retriever_base_url: &str,
    request: &RetrieverRetrieveRequest,
    cards: &[EvidenceCard],
) -> Value {
    json!({
        "schema_version": WORKFLOW_RETRIEVAL_INPUT_SCHEMA_VERSION,
        "retriever_base_url": retriever_base_url,
        "request": request,
        "raw_request": request,
        "retrieve_response": response,
        "raw_retrieve_response": &response.raw_payload,
        "request_response_trace": {
            "transport": "http",
            "tool_name": RETRIEVER_TOOL_NAME,
            "method": "POST",
            "path": "/retrieve",
            "request": request,
            "response": &response.raw_payload,
        },
        "evidence_pack": &response.evidence_pack,
        "cards": cards,
        "diagnostics": {
            "service": &response.service,
            "retriever_diagnostics": &response.diagnostics,
            "pack_diagnostics": &response.evidence_pack.diagnostics,
            "sufficiency": &response.evidence_pack.sufficiency,
        },
    })
}

pub(crate) fn evidence_cards_from_pack(pack: &RetrieverEvidencePack) -> Vec<EvidenceCard> {
    pack.docs
        .iter()
        .enumerate()
        .map(|(index, doc)| evidence_card_from_doc(pack, doc, index))
        .collect()
}

fn evidence_card_from_doc(
    pack: &RetrieverEvidencePack,
    doc: &RetrieverEvidenceDoc,
    index: usize,
) -> EvidenceCard {
    let evidence_type = evidence_type_for_doc(doc);
    let source_id = first_non_empty(doc.source.source_ids.iter().map(String::as_str))
        .or_else(|| doc.source.route_view.as_deref())
        .or_else(|| first_non_empty(doc.refs.chunk_ids.iter().map(String::as_str)))
        .unwrap_or(&doc.doc_id)
        .to_string();
    let block_id = first_non_empty(doc.refs.chunk_ids.iter().map(String::as_str))
        .unwrap_or(&doc.doc_id)
        .to_string();
    let source_title = first_non_empty(
        [
            doc.display.source_label.as_deref(),
            doc.source.source_label.as_deref(),
            doc.display.title.as_deref(),
            doc.source.citation_hint.as_deref(),
            doc.source.work.as_deref(),
        ]
        .into_iter()
        .flatten(),
    )
    .unwrap_or("knownledge retriever evidence")
    .to_string();
    EvidenceCard {
        evidence_id: format!(
            "ev-ret-{}",
            &hash_text(&format!("{}:{index}", doc.doc_id))[..32]
        ),
        evidence_type,
        source_id,
        source_title,
        source_url: String::new(),
        revision_id: None,
        block_id,
        text: evidence_text_for_doc(doc),
        support_scope: support_scope_for_doc(pack, doc),
        unsupported_scope: unsupported_scope_for_doc(doc),
        evidence_level: evidence_level_for_doc(doc),
        confidence: confidence_for_score(doc.score),
        verification_status: "knownledge_retriever_source_backed".to_string(),
    }
}

fn evidence_type_for_doc(doc: &RetrieverEvidenceDoc) -> String {
    let kind = doc_chunk_kind(doc);
    if doc.route == "commentary"
        || !doc.refs.commentary_ids.is_empty()
        || kind.contains("commentary")
    {
        return "commentary".to_string();
    }
    if kind.contains("version")
        || doc
            .source
            .version_scope
            .as_deref()
            .is_some_and(|v| v != "default")
    {
        return "version_note".to_string();
    }
    "base_text".to_string()
}

fn doc_chunk_kind(doc: &RetrieverEvidenceDoc) -> String {
    doc.metadata
        .get("chunk_kind")
        .and_then(Value::as_str)
        .or_else(|| {
            doc.evidence_card
                .as_ref()
                .and_then(|card| card.get("chunk_kind"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default()
        .to_string()
}

fn evidence_text_for_doc(doc: &RetrieverEvidenceDoc) -> String {
    let mut parts = Vec::new();
    for value in [
        doc.display.title.as_deref(),
        doc.display.quote_text.as_deref(),
        doc.display.summary_text.as_deref(),
        doc.display.context_text.as_deref(),
        Some(doc.content.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() && !parts.iter().any(|part| part == trimmed) {
            parts.push(trimmed.to_string());
        }
    }
    trim_chars(&parts.join("\n"), 1_200)
}

fn support_scope_for_doc(pack: &RetrieverEvidencePack, doc: &RetrieverEvidenceDoc) -> String {
    let answer_policy = doc
        .usage_policy
        .get("answer_policy")
        .and_then(Value::as_str)
        .unwrap_or("answer_with_retrieved_evidence");
    let route_summary = if doc.routes.is_empty() {
        doc.route.clone()
    } else {
        doc.routes
            .iter()
            .map(|route| route.route.as_str())
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "knownledge retriever EvidencePack 命中；answer_policy={answer_policy}; route={route_summary}; score={:.4}; pack_sufficient={}",
        doc.score, pack.sufficiency.sufficient
    )
}

fn unsupported_scope_for_doc(doc: &RetrieverEvidenceDoc) -> String {
    let mut limits = Vec::new();
    if let Some(scope) = doc.source.version_scope.as_deref() {
        if !scope.trim().is_empty() {
            limits.push(format!("version_scope={scope}"));
        }
    }
    if let Some(chapter_scope) = doc.source.chapter_scope.as_deref() {
        if !chapter_scope.trim().is_empty() {
            limits.push(format!("chapter_scope={chapter_scope}"));
        }
    }
    if let Some(answer_policy) = doc
        .usage_policy
        .get("answer_policy")
        .and_then(Value::as_str)
    {
        if answer_policy == "supporting_evidence_only" {
            limits.push("仅可作为辅助证据，不能单独推出结论".to_string());
        }
    }
    if limits.is_empty() {
        "不得超出该 EvidenceDoc 的 refs/source_scope/usage_policy；未命中的人物、章节、版本关系不能据此外推。".to_string()
    } else {
        format!(
            "{}；不得超出该 EvidenceDoc 的 refs/source_scope/usage_policy。",
            limits.join("；")
        )
    }
}

fn evidence_level_for_doc(doc: &RetrieverEvidenceDoc) -> String {
    if doc.route == "vector" {
        "向量召回证据".to_string()
    } else if doc.route == "bm25" {
        "词面召回证据".to_string()
    } else {
        "结构化召回证据".to_string()
    }
}

fn confidence_for_score(score: f64) -> String {
    if score >= 1.0 {
        "high".to_string()
    } else if score >= 0.35 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn first_non_empty<'a>(values: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    values.map(str::trim).find(|value| !value.is_empty())
}

fn trim_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>()
        + "..."
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
