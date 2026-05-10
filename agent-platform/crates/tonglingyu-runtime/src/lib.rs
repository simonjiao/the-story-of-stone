use anyhow::{Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
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

fn append_runtime_audit_event(
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
}
