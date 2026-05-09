use anyhow::{Context, Result, anyhow};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use reqwest::header;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use time::OffsetDateTime;
use tower_http::trace::TraceLayer;

const DEFAULT_MODEL_ID: &str = "tonglingyu";
const DEFAULT_MODEL_NAME: &str = "通灵玉";

#[derive(Debug, Parser)]
#[command(name = "tonglingyu-gateway")]
#[command(about = "Tonglingyu source snapshot loader and OpenAI-compatible gateway")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    BuildKb(BuildKbArgs),
    Query(QueryArgs),
    Serve(ServeArgs),
}

#[derive(Debug, Parser, Clone)]
struct BuildKbArgs {
    #[arg(
        long,
        env = "TONGLINGYU_SOURCE_ROOT",
        default_value = "resources/sources/wiki"
    )]
    source_root: PathBuf,
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(long, default_value_t = false)]
    rebuild: bool,
}

#[derive(Debug, Parser, Clone)]
struct QueryArgs {
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    question: String,
    #[arg(long, default_value_t = 8)]
    limit: usize,
}

#[derive(Debug, Parser, Clone)]
struct ServeArgs {
    #[arg(long, env = "TONGLINGYU_BIND", default_value = "127.0.0.1:8090")]
    bind: SocketAddr,
    #[arg(
        long,
        env = "TONGLINGYU_DB_PATH",
        default_value = "data/tonglingyu/tonglingyu.db"
    )]
    db: PathBuf,
    #[arg(
        long,
        env = "TONGLINGYU_SOURCE_ROOT",
        default_value = "resources/sources/wiki"
    )]
    source_root: PathBuf,
    #[arg(long, env = "TONGLINGYU_AUTO_BUILD_KB", default_value_t = false)]
    auto_build_kb: bool,
    #[arg(long, env = "TONGLINGYU_MODEL_ID", default_value = DEFAULT_MODEL_ID)]
    model_id: String,
    #[arg(long, env = "TONGLINGYU_MODEL_NAME", default_value = DEFAULT_MODEL_NAME)]
    model_name: String,
    #[arg(long, env = "TONGLINGYU_UPSTREAM_BASE_URL")]
    upstream_base_url: Option<String>,
    #[arg(long, env = "TONGLINGYU_UPSTREAM_API_KEY")]
    upstream_api_key: Option<String>,
    #[arg(
        long,
        env = "TONGLINGYU_UPSTREAM_MODEL",
        default_value = "hermes-agent"
    )]
    upstream_model: String,
    #[arg(long, env = "TONGLINGYU_MAX_EVIDENCE", default_value_t = 8)]
    max_evidence: usize,
}

#[derive(Clone)]
struct AppState {
    db: PathBuf,
    model_id: String,
    model_name: String,
    upstream_base_url: Option<String>,
    upstream_api_key: Option<String>,
    upstream_model: String,
    max_evidence: usize,
    client: reqwest::Client,
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

#[derive(Debug, Clone, Serialize)]
struct EvidenceCard {
    evidence_id: String,
    evidence_type: String,
    source_id: String,
    source_title: String,
    source_url: String,
    revision_id: Option<i64>,
    block_id: String,
    text: String,
    support_scope: String,
    unsupported_scope: String,
    evidence_level: String,
    confidence: String,
    verification_status: String,
}

#[derive(Debug, Clone, Serialize)]
struct ReviewRecord {
    status: String,
    severity: String,
    issues: Vec<String>,
    summary: String,
}

#[derive(Debug, Clone, Serialize)]
struct EvidencePackage {
    package_id: String,
    trace_id: String,
    question: String,
    cards: Vec<EvidenceCard>,
    claims: Vec<String>,
    review: ReviewRecord,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: Option<String>,
    messages: Vec<ChatMessage>,
    stream: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    role: String,
    content: MessageContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Parts(Vec<MessagePart>),
    Other(Value),
}

#[derive(Debug, Deserialize)]
struct MessagePart {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchParams {
    q: String,
    limit: Option<usize>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    match Args::parse().command {
        Command::BuildKb(args) => {
            build_kb(&args)?;
            Ok(())
        }
        Command::Query(args) => {
            let conn = open_db(&args.db)?;
            let cards = search_evidence(&conn, &args.question, args.limit)?;
            let trace_id = new_trace_id();
            let package = create_evidence_package(&conn, &trace_id, &args.question, cards)?;
            println!("{}", serde_json::to_string_pretty(&package)?);
            Ok(())
        }
        Command::Serve(args) => serve(args).await,
    }
}

async fn serve(args: ServeArgs) -> Result<()> {
    if args.auto_build_kb && !has_kb(&args.db)? {
        let build = BuildKbArgs {
            source_root: args.source_root.clone(),
            db: args.db.clone(),
            rebuild: false,
        };
        build_kb(&build)?;
    }
    let state = Arc::new(AppState {
        db: args.db.clone(),
        model_id: args.model_id,
        model_name: args.model_name,
        upstream_base_url: args
            .upstream_base_url
            .map(|value| value.trim_end_matches('/').to_string()),
        upstream_api_key: args.upstream_api_key.filter(|value| !value.is_empty()),
        upstream_model: args.upstream_model,
        max_evidence: args.max_evidence,
        client: reqwest::Client::new(),
    });
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/evidence/search", get(search_endpoint))
        .route("/v1/evidence/packages/{package_id}", get(package_endpoint))
        .with_state(state)
        .layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    tracing::info!(bind = %args.bind, "tonglingyu gateway listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_kb(args: &BuildKbArgs) -> Result<()> {
    if args.rebuild && args.db.exists() {
        fs::remove_file(&args.db)
            .with_context(|| format!("remove existing db {}", args.db.display()))?;
    }
    if let Some(parent) = args.db.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }

    let mut conn = open_db(&args.db)?;
    init_schema(&conn)?;
    let source_dirs = list_source_dirs(&args.source_root)?;
    if source_dirs.is_empty() {
        return Err(anyhow!(
            "no source snapshots found under {}",
            args.source_root.display()
        ));
    }

    let tx = conn.transaction()?;
    clear_generated_rows(&tx)?;
    seed_aliases(&tx)?;
    for source_dir in source_dirs {
        load_source_snapshot(&tx, &source_dir)?;
    }
    write_kb_version(&tx, &args.source_root)?;
    tx.commit()?;
    println!(
        "OK build_kb db={} source_root={}",
        args.db.display(),
        args.source_root.display()
    );
    Ok(())
}

fn open_db(path: &Path) -> Result<Connection> {
    let conn =
        Connection::open(path).with_context(|| format!("open sqlite db {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

fn has_kb(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let conn = open_db(path)?;
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

fn init_schema(conn: &Connection) -> Result<()> {
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
        CREATE INDEX IF NOT EXISTS idx_evidence_cards_package ON evidence_cards(package_id);
        "#,
    )?;
    Ok(())
}

fn clear_generated_rows(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
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

fn list_source_dirs(root: &Path) -> Result<Vec<PathBuf>> {
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

fn write_kb_version(conn: &Connection, source_root: &Path) -> Result<()> {
    let source_count: i64 = conn.query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))?;
    let block_count: i64 = conn.query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))?;
    conn.execute(
        "INSERT INTO kb_version (version_id, source_root, source_count, block_count, schema_version, built_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            format!("kb-{}", uuid::Uuid::now_v7().simple()),
            source_root.display().to_string(),
            source_count,
            block_count,
            "tonglingyu-v1-sqlite-fts",
            now_rfc3339(),
        ],
    )?;
    Ok(())
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

fn hash_files<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> Result<String> {
    let mut hasher = Sha256::new();
    for path in paths {
        hasher.update(path.display().to_string().as_bytes());
        hasher.update(fs::read(path).with_context(|| format!("hash {}", path.display()))?);
    }
    Ok(format!("{:x}", hasher.finalize()))
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

fn search_evidence(conn: &Connection, question: &str, limit: usize) -> Result<Vec<EvidenceCard>> {
    let terms = extract_terms(conn, question)?;
    let mut scored: BTreeMap<String, (i64, EvidenceCard)> = BTreeMap::new();
    for term in &terms {
        for block in query_blocks_like(conn, term, limit * 4)? {
            let score = score_block(question, term, &block);
            let card = evidence_card_from_block(block);
            scored
                .entry(card.block_id.clone())
                .and_modify(|(existing, _)| *existing += score)
                .or_insert((score, card));
        }
    }
    if scored.is_empty() {
        for block in query_blocks_like(conn, question, limit * 2)? {
            let card = evidence_card_from_block(block);
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
    Ok(ranked.into_iter().map(|(_, card)| card).collect())
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
        ("宝玉", "寶玉"),
        ("黛玉", "黛玉"),
        ("宝钗", "寶釵"),
        ("凤姐", "鳳姐"),
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

fn query_blocks_like(conn: &Connection, term: &str, limit: usize) -> Result<Vec<BlockRecord>> {
    let like = format!("%{}%", term.replace('%', "\\%").replace('_', "\\_"));
    let normalized_like = format!(
        "%{}%",
        normalize_text(term).replace('%', "\\%").replace('_', "\\_")
    );
    let mut stmt = conn.prepare(
        r#"
        SELECT block_id, block_index, kind, revision_id, section_id,
               source_id, source_title, source_url, tag, text
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
        Ok(BlockRecord {
            block_id: row.get(0)?,
            block_index: row.get(1)?,
            kind: row.get(2)?,
            revision_id: row.get(3)?,
            section_id: row.get(4)?,
            source_id: row.get(5)?,
            source_title: row.get(6)?,
            source_url: row.get(7)?,
            tag: row.get(8)?,
            text: row.get(9)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn evidence_card_from_block(block: BlockRecord) -> EvidenceCard {
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
        text: trim_text(&block.text, 520),
        support_scope,
        unsupported_scope,
        evidence_level,
        confidence,
        verification_status: "source_snapshot_ready_not_scholarly_collated".to_string(),
    }
}

fn create_evidence_package(
    conn: &Connection,
    trace_id: &str,
    question: &str,
    cards: Vec<EvidenceCard>,
) -> Result<EvidencePackage> {
    let claims = claims_from_cards(question, &cards);
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
    Ok(EvidencePackage {
        package_id,
        trace_id: trace_id.to_string(),
        question: question.to_string(),
        cards,
        claims,
        review,
    })
}

fn claims_from_cards(question: &str, cards: &[EvidenceCard]) -> Vec<String> {
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

fn review(question: &str, cards: &[EvidenceCard], claims: &[String]) -> ReviewRecord {
    let mut issues = Vec::new();
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

async fn healthz(State(state): State<Arc<AppState>>) -> Response {
    match open_db(&state.db).and_then(|conn| {
        let source_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM sources", [], |row| row.get(0))?;
        let block_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM blocks", [], |row| row.get(0))?;
        Ok((source_count, block_count))
    }) {
        Ok((source_count, block_count)) => Json(json!({
            "status": "ok",
            "model": state.model_id,
            "sources": source_count,
            "blocks": block_count
        }))
        .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "degraded", "error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn models(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [{
            "id": state.model_id,
            "object": "model",
            "owned_by": "tonglingyu",
            "name": state.model_name,
            "description": "红楼文本证据与脂批问答系统"
        }]
    }))
}

async fn search_endpoint(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Response {
    match open_db(&state.db)
        .and_then(|conn| search_evidence(&conn, &params.q, params.limit.unwrap_or(8)))
    {
        Ok(cards) => Json(json!({"object": "list", "data": cards})).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "search_failed", "message": error.to_string()})),
        )
            .into_response(),
    }
}

async fn package_endpoint(
    State(state): State<Arc<AppState>>,
    AxumPath(package_id): AxumPath<String>,
) -> Response {
    match load_package(&state.db, &package_id) {
        Ok(Some(package)) => Json(package).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "package_load_failed", "message": error.to_string()})),
        )
            .into_response(),
    }
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Response {
    let trace_id = new_trace_id();
    let request = match serde_json::from_value::<ChatCompletionRequest>(payload.clone()) {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid_request", "message": error.to_string(), "trace_id": trace_id})),
            )
                .into_response();
        }
    };
    let question = last_user_message(&request.messages);
    if question.trim().is_empty() {
        return completion_response(
            request.model.as_deref().unwrap_or(&state.model_id),
            "请提出一个《红楼梦》相关问题。".to_string(),
            None,
        );
    }

    let package = match open_db(&state.db).and_then(|conn| {
        let cards = search_evidence(&conn, &question, state.max_evidence)?;
        create_evidence_package(&conn, &trace_id, &question, cards)
    }) {
        Ok(package) => package,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "evidence_pipeline_failed", "message": error.to_string(), "trace_id": trace_id})),
            )
            .into_response();
        }
    };

    let draft = match answer_with_optional_upstream(&state, &question, &package).await {
        Ok(answer) => answer,
        Err(error) => {
            tracing::warn!(%trace_id, error = %error, "upstream answer failed; using local fallback");
            local_answer(&question, &package)
        }
    };
    let final_answer = enforce_review(draft, &package);
    if request.stream.unwrap_or(false) {
        streaming_response(
            request.model.as_deref().unwrap_or(&state.model_id),
            final_answer,
        )
    } else {
        completion_response(
            request.model.as_deref().unwrap_or(&state.model_id),
            final_answer,
            Some(&package),
        )
    }
}

async fn answer_with_optional_upstream(
    state: &AppState,
    question: &str,
    package: &EvidencePackage,
) -> Result<String> {
    let Some(base_url) = &state.upstream_base_url else {
        return Ok(local_answer(question, package));
    };
    let prompt = upstream_prompt(question, package);
    let mut request = state
        .client
        .post(format!("{base_url}/chat/completions"))
        .json(&json!({
            "model": state.upstream_model,
            "stream": false,
            "messages": [
                {
                    "role": "system",
                    "content": "你是通灵玉的回答生成层。只能依据给定证据包回答；必须保留版本边界、支持范围和不支持范围；证据不足时直说证据不足。"
                },
                {"role": "user", "content": prompt}
            ]
        }));
    if let Some(key) = &state.upstream_api_key {
        request = request.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    let response = request.send().await?.error_for_status()?;
    let value: Value = response.json().await?;
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("upstream response missing choices[0].message.content"))?;
    Ok(format!(
        "{}\n\n证据包：{}\nreviewer：{}",
        content.trim(),
        package.package_id,
        package.review.summary
    ))
}

fn upstream_prompt(question: &str, package: &EvidencePackage) -> String {
    let evidence = package
        .cards
        .iter()
        .enumerate()
        .map(|(index, card)| {
            format!(
                "[{}] {} {} {} rev={:?}\n证据：{}\n支持：{}\n不支持：{}",
                index + 1,
                card.evidence_type,
                card.source_id,
                card.source_title,
                card.revision_id,
                card.text,
                card.support_scope,
                card.unsupported_scope
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "问题：{}\n\n证据包编号：{}\n审校预判：{}\n\n证据：\n{}\n\n请给出简洁中文回答。",
        question, package.package_id, package.review.summary, evidence
    )
}

fn local_answer(question: &str, package: &EvidencePackage) -> String {
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

fn enforce_review(draft: String, package: &EvidencePackage) -> String {
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

fn completion_response(
    model: &str,
    content: String,
    package: Option<&EvidencePackage>,
) -> Response {
    let mut value = json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::now_v7().simple()),
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }]
    });
    if let Some(package) = package {
        value["trace_id"] = json!(package.trace_id);
        value["evidence_package_id"] = json!(package.package_id);
        value["review"] = json!(package.review);
    }
    Json(value).into_response()
}

fn streaming_response(model: &str, content: String) -> Response {
    let body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({
            "id": format!("chatcmpl-{}", uuid::Uuid::now_v7().simple()),
            "object": "chat.completion.chunk",
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": content},
                "finish_reason": "stop"
            }]
        })
    );
    (
        [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
        body,
    )
        .into_response()
}

fn load_package(db: &Path, package_id: &str) -> Result<Option<Value>> {
    let conn = open_db(db)?;
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
    let mut stmt = conn.prepare(
        "SELECT evidence_json FROM evidence_cards WHERE package_id = ?1 ORDER BY evidence_id",
    )?;
    let cards = stmt
        .query_map(params![package_id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|item| serde_json::from_str::<Value>(&item))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Some(json!({
        "package_id": package_id,
        "trace_id": trace_id,
        "question": question,
        "claims": serde_json::from_str::<Value>(&claims_json)?,
        "evidence_ids": serde_json::from_str::<Value>(&evidence_ids_json)?,
        "cards": cards,
        "review": serde_json::from_str::<Value>(&review_json)?,
    })))
}

fn last_user_message(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| match &message.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Parts(parts) => parts
                .iter()
                .filter(|part| part.kind.as_deref().unwrap_or("text") == "text")
                .filter_map(|part| part.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n"),
            MessageContent::Other(value) => value.to_string(),
        })
        .unwrap_or_default()
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

fn score_block(question: &str, term: &str, block: &BlockRecord) -> i64 {
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
        score += 8;
    }
    if question.contains("程乙") && block.source_id.contains("chengyi") {
        score += 8;
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
    } else if term.contains("通灵") || term.contains("通靈") {
        if looks_like_inscription {
            score += 20;
        }
    }
    score
}

fn normalize_text(input: &str) -> String {
    let replacements = [
        ("紅", "红"),
        ("樓", "楼"),
        ("夢", "梦"),
        ("寶", "宝"),
        ("寳", "宝"),
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

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn new_trace_id() -> String {
    format!("tly-{}", uuid::Uuid::now_v7().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
