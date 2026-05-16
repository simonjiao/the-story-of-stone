# Tonglingyu RQA Release Runbook

This runbook is the operator handoff for Tonglingyu RQA production release
readiness. It is intentionally evidence-driven: a release is not
production-ready until the live gates, rollback path, alert policy, and
post-release monitor evidence are bound into the saved release report.

<!-- tonglingyu:release-runbook:release_flow -->

## Release Flow

1. Confirm the target deploy checkout, image tags, and `.env` source belong to
   the same intended release.
2. Run the migration preflight and backup steps below before changing the live
   service.
3. Deploy with the current compose entrypoint:

   ```bash
   cd deploy
   docker compose build tonglingyu-gateway
   docker compose up -d tonglingyu-gateway open-webui
   docker compose ps
   ```

4. Run the live release gate with the report path set:

   ```bash
   TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
   TONGLINGYU_RELEASE_REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:?}" \
   ./scripts/verify-tonglingyu-release-readiness.sh
   ```

5. Validate the saved report:

   ```bash
   ./scripts/verify-tonglingyu-release-readiness-report.sh \
     "${TONGLINGYU_RELEASE_REPORT_PATH:?}"
   ```

<!-- tonglingyu:release-runbook:migration_preflight -->

## Migration Preflight

Run the RQA restore drill against existing references before deployment. The
gate must use live references for production:

```bash
TONGLINGYU_RQA_RESTORE_DRILL_REQUIRE_LIVE=true \
./scripts/verify-tonglingyu-rqa-backup-restore-drill.sh
```

The release cannot proceed if this drill uses fixture-only references, misses
post-restore checks, or fails to rerun the RQA quality gate and saved report
validator.

<!-- tonglingyu:release-runbook:backup -->

## Backup

Back up deploy configuration before touching `.env`:

```bash
./scripts/env-backup.sh backup
```

Record the backup artifact path or digest in release evidence. RQA DB backup
and restore evidence is recorded by
`verify-tonglingyu-rqa-backup-restore-drill.sh`.

<!-- tonglingyu:release-runbook:deploy -->

## Deploy

Use the compose project in `deploy/` as the deployment boundary. Do not expose
Hermes `8642` or `9119` publicly; the public path remains Cloudflare Tunnel to
Open WebUI.

```bash
docker compose build tonglingyu-gateway
docker compose up -d tonglingyu-gateway open-webui
docker compose ps
```

<!-- tonglingyu:release-runbook:live_gate -->

## Live Gate

Production release readiness requires `TONGLINGYU_RELEASE_REQUIRE_LIVE=true`.
The live report must include passed gates for strict Gateway, model upstream
network, Open WebUI Function, Open WebUI Admin Action, and browser review
evidence.

```bash
TONGLINGYU_RELEASE_REQUIRE_LIVE=true \
TONGLINGYU_RELEASE_REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:?}" \
./scripts/verify-tonglingyu-release-readiness.sh
```

<!-- tonglingyu:release-runbook:saved_report_validation -->

## Saved Report Validation

The saved report is the production artifact. Validate it after the release gate
and after any rollback:

```bash
./scripts/verify-tonglingyu-release-readiness-report.sh \
  "${TONGLINGYU_RELEASE_REPORT_PATH:?}"
```

<!-- tonglingyu:release-runbook:rollback_image_config -->

## Rollback To Previous Image Or Config

Rollback must restore the previous image tag and configuration snapshot, then
rerun release readiness or explicitly mark the environment non-production.

1. Restore the previous `.env` from the backup made before deployment.
2. Restore the previous `TONGLINGYU_GATEWAY_IMAGE_TAG` or build context.
3. Restart the affected services:

   ```bash
   docker compose up -d tonglingyu-gateway open-webui
   docker compose ps
   ```

4. Rerun the live release readiness gate. If the live gate cannot pass, record
   the environment as non-production and keep the saved report failed.

<!-- tonglingyu:release-runbook:db_restore_or_additive_downgrade -->

## DB Restore Or Additive Schema Downgrade

RQA migrations are expected to be additive. If rollback keeps the additive
schema, the operator must verify older code ignores the additive columns. If a
DB restore is required, use the restore drill artifact, then rerun integrity,
RQA quality, and saved report validation checks.

<!-- tonglingyu:release-runbook:rto_rpo_restore -->

## RTO And RPO

Default targets:

- RTO: restore service and pass release validation within 900 seconds.
- RPO: lose no more than 3600 seconds of RQA state.

The release report must cite the latest restore evidence ref. RTO/RPO breach is
a production blocker until the incident owner accepts a non-production state or
reruns a passing restore drill.

<!-- tonglingyu:release-runbook:alert_policy -->

## Alert Policy

Alert labels are fixed and low-cardinality. They must not include query text,
question text, trace IDs, package IDs, user IDs, session IDs, or message IDs.

<!-- tonglingyu:alert:rqa_write_failure_rate -->

- `rqa_write_failure_rate`: severity `page`, owner `rqa-oncall`, metric
  `tonglingyu_rqa_write_failures_total`, threshold `> 0 for 5m`, labels
  `status`, `failure_type`.

<!-- tonglingyu:alert:admin_api_5xx_rate -->

- `admin_api_5xx_rate`: severity `page`, owner `rqa-oncall`, metric
  `tonglingyu_admin_api_5xx_total`, threshold `> 0 for 5m`, labels `status`.

<!-- tonglingyu:alert:admin_api_latency_p95 -->

- `admin_api_latency_p95`: severity `ticket`, owner `rqa-oncall`, metric
  `tonglingyu_admin_api_latency_ms`, threshold `p95 > 2000ms for 10m`, labels
  `status`.

<!-- tonglingyu:alert:open_p0_retrieval_failure -->

- `open_p0_retrieval_failure`: severity `page`, owner `rqa-oncall`, metric
  `tonglingyu_retrieval_failures_total`, threshold `open P0 > 0`, labels
  `status`, `failure_type`.

<!-- tonglingyu:alert:open_p0_governance_task -->

- `open_p0_governance_task`: severity `page`, owner `rqa-oncall`, metric
  `tonglingyu_governance_tasks_total`, threshold `open P0 > 0`, labels
  `status`, `task_type`, `priority`.

<!-- tonglingyu:alert:rqa_quality_gate_failure -->

- `rqa_quality_gate_failure`: severity `page`, owner `rqa-oncall`, metric
  `tonglingyu_rqa_quality_gate_failures_total`, threshold `> 0`, labels
  `status`.

<!-- tonglingyu:alert:release_gate_failure -->

- `release_gate_failure`: severity `page`, owner `release-oncall`, metric
  `tonglingyu_release_gate_failures_total`, threshold `> 0`, labels `status`.

<!-- tonglingyu:alert:openwebui_admin_action_failure -->

- `openwebui_admin_action_failure`: severity `ticket`, owner `rqa-oncall`,
  metric `tonglingyu_openwebui_admin_action_failures_total`, threshold `> 0`,
  labels `status`.

<!-- tonglingyu:release-runbook:incident_response -->

## Incident Response

Severity, owner, first response, mitigation, rollback, and post-incident review
must be recorded for every production release incident. RTO/RPO breach escalates
to release owner and keeps the release non-production until a passing live gate
and saved report validation are recorded. If RQA write paths are disabled during
mitigation, preserve existing evidence, audit tombstones, and release report
artifacts before rollback or deletion.

## Incident And Audit Evidence

Incident drill and audit-history evidence must be recorded as a JSON artifact.
The artifact binds status-history counts, actor coverage, tombstone evidence,
incident severity, owner, first response, mitigation, rollback, recovery
validation, and RTO/RPO breach escalation.

<!-- markdownlint-disable MD013 -->
```bash
TONGLINGYU_RQA_INCIDENT_AUDIT_REPORT_PATH="${INCIDENT_AUDIT_REPORT_PATH:?}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_OPERATOR="${TONGLINGYU_RELEASE_OPERATOR:?}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_ENVIRONMENT="${TONGLINGYU_RELEASE_ENVIRONMENT:?}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_STARTED_AT="${INCIDENT_AUDIT_STARTED_AT:?}" \
TONGLINGYU_RQA_INCIDENT_AUDIT_FINISHED_AT="${INCIDENT_AUDIT_FINISHED_AT:?}" \
TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF="${TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF:?}" \
TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF="${TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF:?}" \
TONGLINGYU_RQA_AUDIT_STATUS_HISTORY_EVENT_COUNT="${TONGLINGYU_RQA_AUDIT_STATUS_HISTORY_EVENT_COUNT:?}" \
TONGLINGYU_RQA_AUDIT_STATUS_HISTORY_ACTOR_COUNT="${TONGLINGYU_RQA_AUDIT_STATUS_HISTORY_ACTOR_COUNT:?}" \
TONGLINGYU_RQA_INCIDENT_SEVERITY="${TONGLINGYU_RQA_INCIDENT_SEVERITY:?}" \
TONGLINGYU_RQA_INCIDENT_OWNER="${TONGLINGYU_RQA_INCIDENT_OWNER:?}" \
TONGLINGYU_RQA_INCIDENT_FIRST_RESPONSE_REF="${TONGLINGYU_RQA_INCIDENT_FIRST_RESPONSE_REF:?}" \
TONGLINGYU_RQA_INCIDENT_MITIGATION_REF="${TONGLINGYU_RQA_INCIDENT_MITIGATION_REF:?}" \
TONGLINGYU_RQA_INCIDENT_ROLLBACK_REF="${TONGLINGYU_RQA_INCIDENT_ROLLBACK_REF:?}" \
TONGLINGYU_RQA_INCIDENT_RECOVERY_VALIDATION_REF="${TONGLINGYU_RQA_INCIDENT_RECOVERY_VALIDATION_REF:?}" \
TONGLINGYU_RQA_INCIDENT_RTO_RPO_BREACH_ESCALATION_REF="${TONGLINGYU_RQA_INCIDENT_RTO_RPO_BREACH_ESCALATION_REF:?}" \
TONGLINGYU_RQA_INCIDENT_CONCLUSION=passed \
./scripts/verify-tonglingyu-rqa-incident-audit-evidence.sh
```
<!-- markdownlint-enable MD013 -->

## Capacity And Load Evidence

Capacity/load smoke must produce a JSON artifact before the live release report
can be considered production-ready. The artifact binds representative eval
report count, failure count, admin list pagination, RQA write p95, admin query
p95, metrics query p95, release gate runtime, audit-history evidence, and
incident evidence.

<!-- markdownlint-disable MD013 -->
```bash
TONGLINGYU_RQA_CAPACITY_LOAD_REPORT_PATH="${CAPACITY_LOAD_REPORT_PATH:?}" \
TONGLINGYU_RQA_CAPACITY_LOAD_OPERATOR="${TONGLINGYU_RELEASE_OPERATOR:?}" \
TONGLINGYU_RQA_CAPACITY_LOAD_ENVIRONMENT="${TONGLINGYU_RELEASE_ENVIRONMENT:?}" \
TONGLINGYU_RQA_CAPACITY_LOAD_STARTED_AT="${CAPACITY_LOAD_STARTED_AT:?}" \
TONGLINGYU_RQA_CAPACITY_LOAD_FINISHED_AT="${CAPACITY_LOAD_FINISHED_AT:?}" \
TONGLINGYU_RQA_CAPACITY_EVIDENCE_REF="${TONGLINGYU_RQA_CAPACITY_EVIDENCE_REF:?}" \
TONGLINGYU_RQA_LOAD_EVIDENCE_REF="${TONGLINGYU_RQA_LOAD_EVIDENCE_REF:?}" \
TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF="${TONGLINGYU_RQA_AUDIT_HISTORY_EVIDENCE_REF:?}" \
TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF="${TONGLINGYU_RQA_INCIDENT_EVIDENCE_REF:?}" \
TONGLINGYU_RQA_CAPACITY_EVAL_REPORT_COUNT="${TONGLINGYU_RQA_CAPACITY_EVAL_REPORT_COUNT:?}" \
TONGLINGYU_RQA_CAPACITY_FAILURE_COUNT="${TONGLINGYU_RQA_CAPACITY_FAILURE_COUNT:?}" \
TONGLINGYU_RQA_CAPACITY_ADMIN_LIST_PAGE_COUNT="${TONGLINGYU_RQA_CAPACITY_ADMIN_LIST_PAGE_COUNT:?}" \
TONGLINGYU_RQA_LOAD_RQA_WRITE_P95_MS="${TONGLINGYU_RQA_LOAD_RQA_WRITE_P95_MS:?}" \
TONGLINGYU_RQA_LOAD_ADMIN_READ_P95_MS="${TONGLINGYU_RQA_LOAD_ADMIN_READ_P95_MS:?}" \
TONGLINGYU_RQA_LOAD_METRICS_READ_P95_MS="${TONGLINGYU_RQA_LOAD_METRICS_READ_P95_MS:?}" \
TONGLINGYU_RQA_LOAD_RELEASE_GATE_MS="${TONGLINGYU_RQA_LOAD_RELEASE_GATE_MS:?}" \
./scripts/verify-tonglingyu-rqa-capacity-load-evidence.sh
```
<!-- markdownlint-enable MD013 -->

<!-- tonglingyu:release-runbook:post_release_monitor -->

## Post-Release Monitor

The post-release window is at least 60 minutes. Evidence must record:

- operator;
- environment;
- saved release report path;
- one successful live release gate or live gate artifact;
- one successful Open WebUI Admin Action or Gateway admin API query;
- conclusion.

The release report cannot be marked production-ready if this monitor evidence
is missing or if the conclusion is not `passed`.

Generate the monitor evidence as a JSON artifact, then bind it into the live
ops gate with `TONGLINGYU_RELEASE_POST_RELEASE_MONITOR_EVIDENCE`:

<!-- markdownlint-disable MD013 -->
```bash
TONGLINGYU_POST_RELEASE_MONITOR_REPORT_PATH="${POST_RELEASE_MONITOR_REPORT_PATH:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_OPERATOR="${TONGLINGYU_RELEASE_OPERATOR:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_ENVIRONMENT="${TONGLINGYU_RELEASE_ENVIRONMENT:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_RELEASE_REPORT_PATH="${TONGLINGYU_RELEASE_REPORT_PATH:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_REF="file:${POST_RELEASE_MONITOR_REPORT_PATH:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_LIVE_GATE_REF="${TONGLINGYU_RELEASE_POST_RELEASE_LIVE_GATE_REF:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_ADMIN_ACTION_REF="${TONGLINGYU_RELEASE_POST_RELEASE_ADMIN_ACTION_REF:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_STARTED_AT="${POST_RELEASE_MONITOR_STARTED_AT:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_FINISHED_AT="${POST_RELEASE_MONITOR_FINISHED_AT:?}" \
TONGLINGYU_POST_RELEASE_MONITOR_CONCLUSION=passed \
./scripts/verify-tonglingyu-post-release-monitor.sh
```
<!-- markdownlint-enable MD013 -->

<!-- tonglingyu:release-runbook:release_report_reproduction -->

## Release Report Reproduction

The saved release report must be enough to reproduce the release by commit,
image digest, config digest, source snapshot digest, KB build hash, security
scan summary, runtime profile digest, prompt digest, and tool policy digest.
If any of these inputs are missing, the report is not production-ready.
