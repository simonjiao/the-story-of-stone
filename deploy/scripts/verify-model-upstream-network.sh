#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=lib/deploy-env.sh
. "${SCRIPT_DIR}/lib/deploy-env.sh"
load_optional_deploy_env_file

python3 - <<'PY'
import ipaddress
import json
import os
import shlex
import subprocess
import sys
from urllib.parse import urlparse


def run(command: list[str], timeout: int = 30) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
    )


def docker_container_exists(name: str) -> bool:
    result = run(
        [
            "docker",
            "ps",
            "--filter",
            f"name=^{name}$",
            "--format",
            "{{.Names}}",
        ],
        timeout=10,
    )
    return result.returncode == 0 and name in result.stdout.splitlines()


def choose_container() -> str:
    configured = os.environ.get("MODEL_UPSTREAM_PROBE_CONTAINER", "").strip()
    if configured:
        return configured
    for candidate in ["sub2api", "hermes-agent", "tonglingyu-gateway"]:
        if docker_container_exists(candidate):
            return candidate
    return ""


def fake_ip_class(ip_text: str) -> str:
    try:
        ip = ipaddress.ip_address(ip_text)
    except ValueError:
        return "invalid"
    if ip in ipaddress.ip_network("198.18.0.0/15"):
        return "benchmark_fake_ip"
    if ip.is_private:
        return "private"
    return "public"


def parse_probe_urls() -> list[str]:
    raw = os.environ.get("MODEL_UPSTREAM_PROBE_URLS", "").strip()
    if not raw:
        raw = (
            "https://chatgpt.com/backend-api/codex/responses "
            "https://api.openai.com/v1/models"
        )
    return [item for item in shlex.split(raw) if item]


def exec_in_container(container: str, script: str, timeout: int = 35) -> subprocess.CompletedProcess[str]:
    return run(["docker", "exec", container, "sh", "-lc", script], timeout=timeout)


container = choose_container()
probe_urls = parse_probe_urls()
errors: list[str] = []
probes = []

if not container:
    errors.append("no_probe_container_found")
else:
    for url in probe_urls:
        host = urlparse(url).hostname or ""
        if not host:
            errors.append(f"invalid_probe_url={url}")
            continue
        dns_result = exec_in_container(
            container,
            f"getent hosts {shlex.quote(host)} | awk '{{print $1}}' | head -5",
            timeout=10,
        )
        ips = [line.strip() for line in dns_result.stdout.splitlines() if line.strip()]
        ip_classes = [fake_ip_class(ip) for ip in ips]
        curl_script = (
            "rm -f /tmp/model-upstream-probe.out; "
            f"curl -sS -o /tmp/model-upstream-probe.out "
            "-w 'http=%{http_code} connect=%{time_connect} "
            f"tls=%{{time_appconnect}} total=%{{time_total}}' {shlex.quote(url)}"
        )
        curl_result = exec_in_container(container, curl_script, timeout=35)
        metrics = {}
        for item in curl_result.stdout.strip().split():
            if "=" in item:
                key, value = item.split("=", 1)
                metrics[key] = value
        http_status = metrics.get("http", "000")
        tls_seconds = float(metrics.get("tls") or 0)
        curl_ok = curl_result.returncode == 0 and http_status != "000" and tls_seconds > 0
        probe = {
            "url_host": host,
            "container": container,
            "dns_ips": ips,
            "dns_ip_classes": ip_classes,
            "curl_exit": curl_result.returncode,
            "http_status": http_status,
            "tls_handshake_observed": tls_seconds > 0,
            "connect_seconds": metrics.get("connect", ""),
            "tls_seconds": metrics.get("tls", ""),
            "total_seconds": metrics.get("total", ""),
            "status": "ok" if curl_ok else "failed",
        }
        if curl_result.stderr.strip():
            probe["curl_error"] = curl_result.stderr.strip().splitlines()[-1][:240]
        if "benchmark_fake_ip" in ip_classes:
            probe["dns_warning"] = "host resolves to 198.18.0.0/15 fake-IP range"
        if not curl_ok:
            errors.append(f"probe_failed={host}")
        probes.append(probe)

report = {
    "status": "ok" if not errors else "failed",
    "object": "tonglingyu.model_upstream_network_gate",
    "probe_container": container,
    "probe_count": len(probes),
    "probes": probes,
    "errors": errors,
    "secret_values_printed": False,
}
print(json.dumps(report, ensure_ascii=True, sort_keys=True))
if errors:
    sys.exit(1)
PY
