#!/usr/bin/env python3
"""Validate local source snapshots against a small acceptance registry."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


REQUIRED_BLOCK_KEYS = {
    "block_id",
    "block_index",
    "kind",
    "revision_id",
    "section_id",
    "source_id",
    "source_title",
    "source_url",
    "text",
}


class ValidationError(RuntimeError):
    pass


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def iter_jsonl(path: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    with path.open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            try:
                records.append(json.loads(line))
            except json.JSONDecodeError as exc:
                raise ValidationError(f"{path}:{line_number}: invalid JSONL: {exc}") from exc
    return records


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValidationError(message)


def non_empty_text(value: Any) -> bool:
    return isinstance(value, str) and bool(value.strip())


def resolve_repo_path(repo_root: Path, value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else repo_root / path


def validate_source(
    root: Path,
    required_files: list[str],
    raw_html_contract: dict[str, Any],
    spec: dict[str, Any],
) -> None:
    source_id = spec["source_id"]
    source_root = root / source_id
    require(source_root.is_dir(), f"{source_id}: source directory not found")

    for required_file in required_files:
        require((source_root / required_file).is_file(), f"{source_id}: missing {required_file}")

    source = read_json(source_root / "metadata/source.json")
    report = read_json(source_root / "metadata/extraction_report.json")
    documents = iter_jsonl(source_root / "documents/documents.jsonl")
    blocks = iter_jsonl(source_root / "documents/blocks.jsonl")

    require(source.get("source_id") == source_id, f"{source_id}: source_id mismatch")
    require(
        source.get("source_category") == spec["source_category"],
        f"{source_id}: source_category mismatch",
    )
    require(
        source.get("snapshot_contract", {}).get("source_of_record")
        == "raw MediaWiki wikitext plus revision metadata",
        f"{source_id}: source_of_record mismatch",
    )
    require(
        source.get("snapshot_contract", {}).get("raw_html_saved")
        == raw_html_contract["saved"],
        f"{source_id}: raw_html_saved mismatch",
    )
    validate_source_usage_metadata(source_id, source, spec)
    require(report.get("missing") == spec["expected_missing"], f"{source_id}: missing != 0")
    require(
        report.get("raw_html_files") == raw_html_contract["expected_files"],
        f"{source_id}: raw_html_files mismatch",
    )
    require(report.get("documents", 0) >= spec["min_documents"], f"{source_id}: too few documents")
    require(report.get("blocks", 0) >= spec["min_blocks"], f"{source_id}: too few blocks")
    require(report.get("documents") == len(documents), f"{source_id}: document count mismatch")
    require(report.get("blocks") == len(blocks), f"{source_id}: block count mismatch")

    seen: set[str] = set()
    for index, block in enumerate(blocks, start=1):
        missing_keys = REQUIRED_BLOCK_KEYS - set(block)
        require(not missing_keys, f"{source_id}: block {index} missing keys {sorted(missing_keys)}")
        block_id = block["block_id"]
        require(block_id not in seen, f"{source_id}: duplicate block_id {block_id}")
        seen.add(block_id)

    print(f"OK source {source_id}: documents={len(documents)} blocks={len(blocks)}")


def validate_source_usage_metadata(
    source_id: str,
    source: dict[str, Any],
    spec: dict[str, Any],
) -> None:
    required_fields = (
        "source_url",
        "license",
        "license_url",
        "license_source_url",
        "attribution",
        "usage_boundary",
    )
    for field in required_fields:
        require(non_empty_text(source.get(field)), f"{source_id}: missing {field}")

    expected_license = spec.get("license")
    if expected_license:
        require(
            source.get("license") == expected_license,
            f"{source_id}: license mismatch",
        )
    expected_license_url = spec.get("license_url")
    if expected_license_url:
        require(
            source.get("license_url") == expected_license_url,
            f"{source_id}: license_url mismatch",
        )

    require(
        source["source_url"].startswith(("https://", "http://")),
        f"{source_id}: source_url must be absolute URL",
    )
    require(
        source["license_url"].startswith(("https://", "http://")),
        f"{source_id}: license_url must be absolute URL",
    )
    require(
        source["license_source_url"].startswith(("https://", "http://")),
        f"{source_id}: license_source_url must be absolute URL",
    )


def find_block(root: Path, source_id: str, block_id: str) -> dict[str, Any] | None:
    blocks_path = root / source_id / "documents/blocks.jsonl"
    with blocks_path.open(encoding="utf-8") as handle:
        for line in handle:
            block = json.loads(line)
            if block.get("block_id") == block_id:
                return block
    return None


def validate_sample(root: Path, sample: dict[str, Any]) -> None:
    sample_id = sample["sample_id"]
    source_id = sample["source_id"]
    block_id = sample["block_id"]
    block = find_block(root, source_id, block_id)
    require(block is not None, f"{sample_id}: block not found: {block_id}")

    haystack = f"{block.get('source_title', '')}\n{block.get('text', '')}"
    for term in sample.get("must_contain", []):
        require(term in haystack, f"{sample_id}: expected term not found: {term}")

    title_term = sample.get("source_title_contains")
    if title_term:
        require(title_term in block.get("source_title", ""), f"{sample_id}: source title mismatch")

    print(f"OK sample {sample_id}: {block_id}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--registry",
        default="resources/sources/wiki/_registry/first_batch_acceptance.json",
        help="Acceptance registry JSON path.",
    )
    parser.add_argument("--root", help="Override source snapshot root.")
    parser.add_argument("--samples", help="Override sample manifest JSONL path.")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    registry_path = resolve_repo_path(repo_root, args.registry)
    registry = read_json(registry_path)

    source_root = resolve_repo_path(repo_root, args.root or registry["source_root"])
    samples_path = resolve_repo_path(repo_root, args.samples or registry["sample_manifest"])

    required_files = registry["required_files"]
    raw_html_contract = registry["raw_html_contract"]
    for spec in registry["expected_sources"]:
        validate_source(source_root, required_files, raw_html_contract, spec)

    samples = iter_jsonl(samples_path)
    for sample in samples:
        validate_sample(source_root, sample)

    print(f"OK acceptance: sources={len(registry['expected_sources'])} samples={len(samples)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
