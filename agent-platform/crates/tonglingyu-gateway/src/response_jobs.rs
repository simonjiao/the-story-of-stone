use std::collections::{BTreeMap, VecDeque};

use redis::{
    Commands,
    streams::{StreamAutoClaimOptions, StreamId, StreamReadOptions, StreamReadReply},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::run_manager::RunIdentity;

pub(crate) const RESPONSE_JOB_SCHEMA_VERSION: &str = "tonglingyu.response_job.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ResponseJob {
    pub(crate) schema_version: String,
    pub(crate) job_id: String,
    pub(crate) run_id: String,
    pub(crate) response_id: String,
    pub(crate) session_id: String,
    pub(crate) trace_id: String,
    pub(crate) tenant_id: String,
    pub(crate) subject: String,
    pub(crate) user_id: Option<String>,
    pub(crate) api_type: String,
    pub(crate) model: String,
    pub(crate) request: Value,
    pub(crate) attempt: u32,
    pub(crate) max_attempts: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) updated_at: OffsetDateTime,
}

impl ResponseJob {
    pub(crate) fn new(identity: &RunIdentity, request: Value, max_attempts: u32) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            schema_version: RESPONSE_JOB_SCHEMA_VERSION.to_string(),
            job_id: format!("job_{}", uuid::Uuid::now_v7().simple()),
            run_id: identity.run_id.clone(),
            response_id: identity.response_id.clone(),
            session_id: identity.session_id.clone(),
            trace_id: identity.trace_id.clone(),
            tenant_id: identity.owner_scope.tenant_id.clone(),
            subject: identity.owner_scope.subject.clone(),
            user_id: identity.owner_scope.user_id.clone(),
            api_type: serde_json::to_string(&identity.api_type)
                .unwrap_or_else(|_| "\"unknown\"".to_string())
                .trim_matches('"')
                .to_string(),
            model: identity.model.clone(),
            request,
            attempt: 1,
            max_attempts: max_attempts.max(1),
            created_at: now,
            updated_at: now,
        }
    }

    fn retry_copy(&self) -> Self {
        let mut next = self.clone();
        next.attempt += 1;
        next.updated_at = OffsetDateTime::now_utc();
        next
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResponseJobLease {
    pub(crate) job: ResponseJob,
    stream_id: String,
    worker_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResponseJobQueueError {
    BackendUnavailable(String),
    CorruptJob(String),
    UnknownLease(String),
}

#[derive(Debug, Clone)]
pub(crate) struct ResponseJobQueueConfig {
    pub(crate) redis_url: Option<String>,
    pub(crate) redis_required: bool,
    pub(crate) stream_prefix: String,
    pub(crate) group: String,
    pub(crate) job_maxlen: usize,
    pub(crate) job_ttl_secs: u64,
}

impl Default for ResponseJobQueueConfig {
    fn default() -> Self {
        Self {
            redis_url: None,
            redis_required: false,
            stream_prefix: "tonglingyu".to_string(),
            group: "tonglingyu-gateway-workers".to_string(),
            job_maxlen: 2000,
            job_ttl_secs: 86_400,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ResponseJobQueueHealth {
    pub(crate) mode: &'static str,
    pub(crate) required: bool,
    pub(crate) prefix: String,
    pub(crate) group: String,
    pub(crate) status: &'static str,
    pub(crate) error: Option<String>,
}

#[derive(Debug)]
pub(crate) enum ResponseJobQueueBackend {
    InMemory(InMemoryResponseJobQueue),
    Redis(RedisResponseJobQueue),
}

impl ResponseJobQueueBackend {
    pub(crate) fn from_config(
        config: ResponseJobQueueConfig,
    ) -> Result<Self, ResponseJobQueueError> {
        let redis_url = config.redis_url.clone().unwrap_or_default();
        let redis_url = redis_url.trim().to_string();
        if redis_url.is_empty() {
            if config.redis_required {
                return Err(ResponseJobQueueError::BackendUnavailable(
                    "TONGLINGYU_REDIS_URL is required when TONGLINGYU_REDIS_REQUIRED=true"
                        .to_string(),
                ));
            }
            return Ok(Self::InMemory(InMemoryResponseJobQueue::default()));
        }
        let queue = RedisResponseJobQueue::new(&redis_url, config)?;
        queue.ping()?;
        queue.ensure_group()?;
        Ok(Self::Redis(queue))
    }

    pub(crate) fn health_snapshot(&self) -> ResponseJobQueueHealth {
        match self {
            Self::InMemory(_) => ResponseJobQueueHealth {
                mode: "in_memory",
                required: false,
                prefix: "local".to_string(),
                group: "local".to_string(),
                status: "ok",
                error: None,
            },
            Self::Redis(queue) => match queue.ping().and_then(|_| queue.ensure_group()) {
                Ok(()) => ResponseJobQueueHealth {
                    mode: "redis",
                    required: queue.redis_required,
                    prefix: queue.prefix.clone(),
                    group: queue.group.clone(),
                    status: "ok",
                    error: None,
                },
                Err(error) => ResponseJobQueueHealth {
                    mode: "redis",
                    required: queue.redis_required,
                    prefix: queue.prefix.clone(),
                    group: queue.group.clone(),
                    status: "unavailable",
                    error: Some(format!("{error:?}")),
                },
            },
        }
    }

    pub(crate) fn enqueue(&mut self, job: ResponseJob) -> Result<String, ResponseJobQueueError> {
        match self {
            Self::InMemory(queue) => queue.enqueue(job),
            Self::Redis(queue) => queue.enqueue(job),
        }
    }

    pub(crate) fn claim_next(
        &mut self,
        worker_id: &str,
        block_ms: usize,
    ) -> Result<Option<ResponseJobLease>, ResponseJobQueueError> {
        match self {
            Self::InMemory(queue) => queue.claim_next(worker_id, block_ms),
            Self::Redis(queue) => queue.claim_next(worker_id, block_ms),
        }
    }

    pub(crate) fn complete(
        &mut self,
        lease: ResponseJobLease,
    ) -> Result<(), ResponseJobQueueError> {
        match self {
            Self::InMemory(queue) => queue.complete(lease),
            Self::Redis(queue) => queue.complete(lease),
        }
    }

    pub(crate) fn retry_or_dead_letter(
        &mut self,
        lease: ResponseJobLease,
        error_code: &str,
        error_message: &str,
    ) -> Result<RetryDecision, ResponseJobQueueError> {
        match self {
            Self::InMemory(queue) => queue.retry_or_dead_letter(lease, error_code, error_message),
            Self::Redis(queue) => queue.retry_or_dead_letter(lease, error_code, error_message),
        }
    }

    pub(crate) fn reclaim_stale(
        &mut self,
        worker_id: &str,
        min_idle_ms: usize,
        count: usize,
    ) -> Result<usize, ResponseJobQueueError> {
        match self {
            Self::InMemory(queue) => queue.reclaim_stale(worker_id, min_idle_ms, count),
            Self::Redis(queue) => queue.reclaim_stale(worker_id, min_idle_ms, count),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RetryDecision {
    Requeued,
    DeadLettered,
}

#[derive(Debug, Default)]
pub(crate) struct InMemoryResponseJobQueue {
    jobs: VecDeque<(String, ResponseJob)>,
    leases: BTreeMap<String, ResponseJobLease>,
    dead_letters: Vec<Value>,
}

impl InMemoryResponseJobQueue {
    fn enqueue(&mut self, job: ResponseJob) -> Result<String, ResponseJobQueueError> {
        let stream_id = format!("mem-{}", uuid::Uuid::now_v7().simple());
        self.jobs.push_back((stream_id.clone(), job));
        Ok(stream_id)
    }

    fn claim_next(
        &mut self,
        worker_id: &str,
        _block_ms: usize,
    ) -> Result<Option<ResponseJobLease>, ResponseJobQueueError> {
        let Some((stream_id, job)) = self.jobs.pop_front() else {
            return Ok(None);
        };
        let lease = ResponseJobLease {
            job,
            stream_id,
            worker_id: worker_id.to_string(),
        };
        self.leases.insert(lease.stream_id.clone(), lease.clone());
        Ok(Some(lease))
    }

    fn complete(&mut self, lease: ResponseJobLease) -> Result<(), ResponseJobQueueError> {
        self.leases
            .remove(&lease.stream_id)
            .ok_or(ResponseJobQueueError::UnknownLease(lease.stream_id))?;
        Ok(())
    }

    fn retry_or_dead_letter(
        &mut self,
        lease: ResponseJobLease,
        error_code: &str,
        error_message: &str,
    ) -> Result<RetryDecision, ResponseJobQueueError> {
        self.leases
            .remove(&lease.stream_id)
            .ok_or_else(|| ResponseJobQueueError::UnknownLease(lease.stream_id.clone()))?;
        if lease.job.attempt >= lease.job.max_attempts {
            self.dead_letters.push(json!({
                "job": lease.job,
                "error": {
                    "code": error_code,
                    "message": error_message,
                },
                "worker_id": lease.worker_id,
                "failed_at": OffsetDateTime::now_utc(),
            }));
            Ok(RetryDecision::DeadLettered)
        } else {
            self.enqueue(lease.job.retry_copy())?;
            Ok(RetryDecision::Requeued)
        }
    }

    fn reclaim_stale(
        &mut self,
        _worker_id: &str,
        _min_idle_ms: usize,
        _count: usize,
    ) -> Result<usize, ResponseJobQueueError> {
        Ok(0)
    }
}

#[derive(Debug)]
pub(crate) struct RedisResponseJobQueue {
    client: redis::Client,
    prefix: String,
    group: String,
    redis_required: bool,
    job_maxlen: usize,
    job_ttl_secs: u64,
}

impl RedisResponseJobQueue {
    fn new(redis_url: &str, config: ResponseJobQueueConfig) -> Result<Self, ResponseJobQueueError> {
        let client =
            redis::Client::open(redis_url).map_err(|error| redis_backend_error(error, "open"))?;
        Ok(Self {
            client,
            prefix: sanitize_prefix(&config.stream_prefix),
            group: sanitize_group(&config.group),
            redis_required: config.redis_required,
            job_maxlen: config.job_maxlen.max(1),
            job_ttl_secs: config.job_ttl_secs,
        })
    }

    fn jobs_key(&self) -> String {
        format!("{}:jobs", self.prefix)
    }

    fn dead_jobs_key(&self) -> String {
        format!("{}:jobs:dead", self.prefix)
    }

    fn connection(&self) -> Result<redis::Connection, ResponseJobQueueError> {
        self.client
            .get_connection()
            .map_err(|error| redis_backend_error(error, "connect"))
    }

    fn ping(&self) -> Result<(), ResponseJobQueueError> {
        let mut connection = self.connection()?;
        let pong: String = redis::cmd("PING")
            .query(&mut connection)
            .map_err(|error| redis_backend_error(error, "ping"))?;
        if pong.eq_ignore_ascii_case("PONG") {
            Ok(())
        } else {
            Err(ResponseJobQueueError::BackendUnavailable(format!(
                "redis ping returned {pong}"
            )))
        }
    }

    fn ensure_group(&self) -> Result<(), ResponseJobQueueError> {
        let mut connection = self.connection()?;
        let result: redis::RedisResult<()> =
            connection.xgroup_create_mkstream(self.jobs_key(), &self.group, "0");
        match result {
            Ok(()) => Ok(()),
            Err(error) if error.to_string().contains("BUSYGROUP") => Ok(()),
            Err(error) => Err(redis_backend_error(error, "xgroup-create")),
        }
    }

    fn enqueue(&mut self, job: ResponseJob) -> Result<String, ResponseJobQueueError> {
        let mut connection = self.connection()?;
        let job_json = serde_json::to_string(&job)
            .map_err(|error| ResponseJobQueueError::CorruptJob(error.to_string()))?;
        let stream_id: String = redis::cmd("XADD")
            .arg(self.jobs_key())
            .arg("MAXLEN")
            .arg("~")
            .arg(self.job_maxlen)
            .arg("*")
            .arg("job_json")
            .arg(job_json)
            .arg("response_id")
            .arg(&job.response_id)
            .arg("run_id")
            .arg(&job.run_id)
            .arg("attempt")
            .arg(job.attempt.to_string())
            .query(&mut connection)
            .map_err(|error| redis_backend_error(error, "xadd-job"))?;
        self.expire_key(&mut connection, &self.jobs_key())?;
        Ok(stream_id)
    }

    fn claim_next(
        &mut self,
        worker_id: &str,
        block_ms: usize,
    ) -> Result<Option<ResponseJobLease>, ResponseJobQueueError> {
        self.ensure_group()?;
        let mut connection = self.connection()?;
        let options = StreamReadOptions::default()
            .group(&self.group, worker_id)
            .count(1)
            .block(block_ms);
        let reply: Option<StreamReadReply> = connection
            .xread_options(&[self.jobs_key()], &[">"], &options)
            .map_err(|error| redis_backend_error(error, "xreadgroup-job"))?;
        let Some(reply) = reply else {
            return Ok(None);
        };
        let Some(entry) = reply.keys.first().and_then(|key| key.ids.first()).cloned() else {
            return Ok(None);
        };
        Ok(Some(ResponseJobLease {
            job: response_job_from_stream_id(&entry)?,
            stream_id: entry.id,
            worker_id: worker_id.to_string(),
        }))
    }

    fn complete(&mut self, lease: ResponseJobLease) -> Result<(), ResponseJobQueueError> {
        let mut connection = self.connection()?;
        let _: usize = connection
            .xack(self.jobs_key(), &self.group, &[lease.stream_id])
            .map_err(|error| redis_backend_error(error, "xack-job"))?;
        Ok(())
    }

    fn retry_or_dead_letter(
        &mut self,
        lease: ResponseJobLease,
        error_code: &str,
        error_message: &str,
    ) -> Result<RetryDecision, ResponseJobQueueError> {
        let mut connection = self.connection()?;
        if lease.job.attempt >= lease.job.max_attempts {
            let dead_json = serde_json::to_string(&json!({
                "job": lease.job,
                "error": {
                    "code": error_code,
                    "message": error_message,
                },
                "worker_id": lease.worker_id,
                "failed_at": OffsetDateTime::now_utc(),
            }))
            .map_err(|error| ResponseJobQueueError::CorruptJob(error.to_string()))?;
            let _: String = redis::cmd("XADD")
                .arg(self.dead_jobs_key())
                .arg("MAXLEN")
                .arg("~")
                .arg(self.job_maxlen)
                .arg("*")
                .arg("dead_json")
                .arg(dead_json)
                .arg("response_id")
                .arg(&lease.job.response_id)
                .arg("run_id")
                .arg(&lease.job.run_id)
                .query(&mut connection)
                .map_err(|error| redis_backend_error(error, "xadd-dead-job"))?;
            self.expire_key(&mut connection, &self.dead_jobs_key())?;
            let _: usize = connection
                .xack(self.jobs_key(), &self.group, &[lease.stream_id])
                .map_err(|error| redis_backend_error(error, "xack-dead-job"))?;
            Ok(RetryDecision::DeadLettered)
        } else {
            let retry = lease.job.retry_copy();
            let _: usize = connection
                .xack(self.jobs_key(), &self.group, &[lease.stream_id])
                .map_err(|error| redis_backend_error(error, "xack-retry-job"))?;
            self.enqueue(retry)?;
            Ok(RetryDecision::Requeued)
        }
    }

    fn reclaim_stale(
        &mut self,
        worker_id: &str,
        min_idle_ms: usize,
        count: usize,
    ) -> Result<usize, ResponseJobQueueError> {
        self.ensure_group()?;
        let mut connection = self.connection()?;
        let reply: redis::streams::StreamAutoClaimReply = connection
            .xautoclaim_options(
                self.jobs_key(),
                &self.group,
                worker_id,
                min_idle_ms,
                "0-0",
                StreamAutoClaimOptions::default().count(count.max(1)),
            )
            .map_err(|error| redis_backend_error(error, "xautoclaim-job"))?;
        Ok(reply.claimed.len())
    }

    fn expire_key(
        &self,
        connection: &mut redis::Connection,
        key: &str,
    ) -> Result<(), ResponseJobQueueError> {
        if self.job_ttl_secs == 0 {
            return Ok(());
        }
        let _: () = redis::cmd("EXPIRE")
            .arg(key)
            .arg(self.job_ttl_secs)
            .query(connection)
            .map_err(|error| redis_backend_error(error, "expire"))?;
        Ok(())
    }
}

fn response_job_from_stream_id(entry: &StreamId) -> Result<ResponseJob, ResponseJobQueueError> {
    let job_json: String = entry
        .get("job_json")
        .ok_or_else(|| ResponseJobQueueError::CorruptJob("job_json is missing".to_string()))?;
    let job: ResponseJob = serde_json::from_str(&job_json)
        .map_err(|error| ResponseJobQueueError::CorruptJob(error.to_string()))?;
    if job.schema_version != RESPONSE_JOB_SCHEMA_VERSION {
        return Err(ResponseJobQueueError::CorruptJob(format!(
            "unsupported job schema {}",
            job.schema_version
        )));
    }
    Ok(job)
}

fn redis_backend_error(error: redis::RedisError, operation: &str) -> ResponseJobQueueError {
    ResponseJobQueueError::BackendUnavailable(format!("redis {operation} failed: {error}"))
}

fn sanitize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return "tonglingyu".to_string();
    }
    trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn sanitize_group(group: &str) -> String {
    let sanitized = sanitize_prefix(group);
    if sanitized == "tonglingyu" && group.trim().is_empty() {
        "tonglingyu-gateway-workers".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::run_manager::{RunApiType, RunNormalizationInput, normalize_run};

    fn identity() -> RunIdentity {
        normalize_run(RunNormalizationInput {
            api_type: RunApiType::Responses,
            model: "tonglingyu".to_string(),
            session_id: Some("session-job-test".to_string()),
            auth_subject: "subject-job-test".to_string(),
            tenant_id: "tenant-job-test".to_string(),
            user_id: Some("user-job-test".to_string()),
            idempotency_key: Some("idem-job-test".to_string()),
            metadata: json!({"client": "job-test"}),
            request: json!({"model": "tonglingyu", "input": "问题"}),
            stream: false,
            background: true,
        })
        .expect("identity")
    }

    #[test]
    fn in_memory_queue_claims_and_completes_jobs() {
        let identity = identity();
        let mut queue = InMemoryResponseJobQueue::default();
        let job = ResponseJob::new(&identity, json!({"input": "问题"}), 3);
        let stream_id = queue.enqueue(job).expect("enqueue");

        let lease = queue
            .claim_next("worker-test", 0)
            .expect("claim")
            .expect("lease");

        assert_eq!(lease.stream_id, stream_id);
        assert_eq!(lease.job.response_id, identity.response_id);
        queue.complete(lease).expect("complete");
        assert!(queue.claim_next("worker-test", 0).expect("claim").is_none());
    }

    #[test]
    fn in_memory_retry_then_dead_letter_obeys_max_attempts() {
        let identity = identity();
        let mut queue = InMemoryResponseJobQueue::default();
        queue
            .enqueue(ResponseJob::new(&identity, json!({"input": "问题"}), 2))
            .expect("enqueue");

        let first = queue
            .claim_next("worker-test", 0)
            .expect("claim")
            .expect("first");
        assert_eq!(
            queue
                .retry_or_dead_letter(first, "test_error", "first")
                .expect("retry"),
            RetryDecision::Requeued
        );

        let second = queue
            .claim_next("worker-test", 0)
            .expect("claim")
            .expect("second");
        assert_eq!(second.job.attempt, 2);
        assert_eq!(
            queue
                .retry_or_dead_letter(second, "test_error", "second")
                .expect("dead"),
            RetryDecision::DeadLettered
        );
        assert_eq!(queue.dead_letters.len(), 1);
    }

    #[test]
    fn redis_required_without_url_fails_closed() {
        let error = ResponseJobQueueBackend::from_config(ResponseJobQueueConfig {
            redis_url: None,
            redis_required: true,
            ..ResponseJobQueueConfig::default()
        })
        .expect_err("required redis should fail");

        assert!(matches!(
            error,
            ResponseJobQueueError::BackendUnavailable(_)
        ));
    }

    #[test]
    fn missing_redis_url_uses_memory_only_when_not_required() {
        let queue = ResponseJobQueueBackend::from_config(ResponseJobQueueConfig {
            redis_url: Some(" ".to_string()),
            redis_required: false,
            ..ResponseJobQueueConfig::default()
        })
        .expect("queue");

        let health = queue.health_snapshot();
        assert_eq!(health.mode, "in_memory");
        assert_eq!(health.status, "ok");
    }

    #[test]
    fn job_schema_round_trips() {
        let identity = identity();
        let job = ResponseJob::new(&identity, json!({"input": "分析"}), 3);
        let value = serde_json::to_value(&job).expect("json");
        let roundtrip: ResponseJob = serde_json::from_value(value).expect("job");

        assert_eq!(roundtrip.schema_version, RESPONSE_JOB_SCHEMA_VERSION);
        assert_eq!(roundtrip.response_id, identity.response_id);
        assert_eq!(roundtrip.max_attempts, 3);
        assert_eq!(roundtrip.attempt, 1);
    }
}
