use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LLM_EVAL_REPORT_SCHEMA_VERSION: &str = "v1";
pub const LLM_EVAL_SUITE_VERSION: &str = "tonglingyu-llm-eval-v1";
pub const USER_RESPONSE_SAFETY_SCHEMA_VERSION: &str = "tonglingyu-user-response-safety-v1";

pub const REQUEST_SAFETY_DATASET: &str = "request_safety";
pub const STREAMING_DEDUPE_DATASET: &str = "streaming_dedupe";
pub const S1_STAGE: &str = "S1";
pub const REQUEST_SAFETY_MIN_CASES: usize = 20;
pub const STREAMING_DEDUPE_MIN_CASES: usize = 16;
pub const DEFAULT_MAX_MESSAGES: usize = 20;
pub const DEFAULT_MAX_BODY_CHARS: usize = 20_000;
pub const DEFAULT_MAX_QUESTION_CHARS: usize = 2_000;

pub const PUBLIC_OUTPUT_FORBIDDEN_KEYS: &[&str] = &[
    "trace_id",
    "evidence_package_id",
    "review",
    "session_id",
    "user_session_id",
    "interaction_context_id",
    "context_pack_id",
    "context_pack_ref",
    "context_projection_id",
    "context_projection_ref",
    "memory_read_refs",
    "memory_read_ref_digest",
    "memory_policy",
    "memory_candidate",
    "memory_card",
    "llm_extraction",
    "llm_filter",
    "rule_filter",
    "_runtime_stream_events",
];

#[derive(Debug, Clone, Copy)]
pub struct InternalRefPattern {
    pub category: &'static str,
    pub tokens: &'static [&'static str],
}

pub const PUBLIC_OUTPUT_FORBIDDEN_PATTERNS: &[InternalRefPattern] = &[
    InternalRefPattern {
        category: "trace_package",
        tokens: &["trace-", "package:", "pkg-"],
    },
    InternalRefPattern {
        category: "context_projection",
        tokens: &["context-pack://", "context-projection://"],
    },
    InternalRefPattern {
        category: "memory",
        tokens: &[
            "memory-card-",
            "memory-candidate-",
            "memory_policy_decision",
        ],
    },
    InternalRefPattern {
        category: "runtime_internals",
        tokens: &["runtime://", "_runtime_stream_events"],
    },
    InternalRefPattern {
        category: "tool_payload",
        tokens: &["tool_call_id", "tool_result_ref"],
    },
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEvalFixture {
    pub case_id: String,
    pub dataset: String,
    pub stage: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub expected: Value,
    #[serde(default)]
    pub hard_gates: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}
