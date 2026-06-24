#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
BASE_REF="HEAD"

usage() {
  cat <<'EOF'
Usage: scripts/check-tonglingyu-no-question-hardcode.sh [--base REF]

Scans added production Rust lines for question-specific or eval-specific
hardcoding. Test fixtures, docs, resources, and this script are out of scope.
EOF
}

while (($#)); do
  case "$1" in
    --base)
      BASE_REF="${2:?--base requires a ref}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

cd "${REPO_DIR}"

diff_output="$(
  git diff --unified=0 --diff-filter=AM "${BASE_REF}" -- \
    'agent-platform/crates/tonglingyu-runtime/src/*.rs' \
    'agent-platform/crates/tonglingyu-gateway/src/*.rs'
)"

if [[ -z "${diff_output}" ]]; then
  printf 'NO_HARDCODE_DIFF base=%s status=skipped_no_changes\n' "${BASE_REF}"
  exit 0
fi

awk '
  function is_production_file(path) {
    return path !~ /\/tests?\.rs$/ && path !~ /\/tests\//
  }
  /^\+\+\+ b\// {
    file = substr($0, 7)
    production = is_production_file(file)
    next
  }
  production && /^\+/ && $0 !~ /^\+\+\+/ {
    line = substr($0, 2)
    if (line ~ /(通灵宝玉|通靈寶玉|史湘云|史湘雲|紫鹃|紫鵑|甄宝玉|甄寶玉|扫雪拾玉|掃雪拾玉|良儿|良兒)/) {
      print file ": question_or_entity_specific_text: " line
      found = 1
    }
    if (line ~ /(small[0-9]+|conversation-eval|eval_toolkit|tly-[0-9a-f]{12,})/) {
      print file ": eval_or_trace_specific_text: " line
      found = 1
    }
    if (line ~ /第[0-9一二三四五六七八九十百]+回/ && line !~ /source[_ -]?scope|later[_ -]?forty|后四十|後四十/) {
      print file ": chapter_specific_text: " line
      found = 1
    }
  }
  END {
    if (found) {
      exit 1
    }
  }
' <<<"${diff_output}"

printf 'NO_HARDCODE_DIFF base=%s status=passed\n' "${BASE_REF}"

