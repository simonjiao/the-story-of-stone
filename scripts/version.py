#!/usr/bin/env python3
"""Manage the project version across Rust, Python, containers, and scripts."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path


PROJECT_VERSION_FALLBACK = "0.1.3"
VERSION_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
CRATES = (
    "agent-core",
    "agent-runtime",
    "tonglingyu-runtime",
    "tonglingyu-gateway",
)
SCRIPT_FALLBACK_PATHS = (
    "scripts/version.py",
    "scripts/qa.sh",
    "deploy/scripts/bump-deploy-version.sh",
    "deploy/scripts/deploy-versioned-stack.sh",
)
TEST_VERSION_PATHS = ("tests/test_version_management.py",)


class VersionError(RuntimeError):
    pass


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def validate_version(value: str) -> str:
    value = value.strip()
    if not VERSION_RE.fullmatch(value):
        raise VersionError(
            f"invalid version {value!r}; expected numeric MAJOR.MINOR.PATCH"
        )
    return value


def read_project_version(repo_dir: Path) -> str:
    version_path = repo_dir / "VERSION"
    if version_path.is_file():
        return validate_version(version_path.read_text(encoding="utf-8").strip())
    return validate_version(PROJECT_VERSION_FALLBACK)


def bump_patch(version: str) -> str:
    major, minor, patch = (int(part) for part in validate_version(version).split("."))
    return f"{major}.{minor}.{patch + 1}"


def replace_required(text: str, pattern: str, repl: str, label: str) -> str:
    updated, count = re.subn(pattern, repl, text, count=1, flags=re.MULTILINE)
    if count != 1:
        raise VersionError(f"{label}: expected one match for {pattern!r}")
    return updated


def write_if_changed(path: Path, text: str) -> None:
    if path.exists() and path.read_text(encoding="utf-8") == text:
        return
    path.write_text(text, encoding="utf-8")


def set_workspace_cargo_version(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    section_match = re.search(
        r"(?ms)^(\[workspace\.package\]\n)(.*?)(?=^\[|\Z)",
        text,
    )
    if not section_match:
        raise VersionError(f"{path}: missing [workspace.package]")
    body = section_match.group(2)
    if re.search(r"(?m)^version\s*=", body):
        new_body = re.sub(
            r'(?m)^version\s*=\s*"[^"]+"$',
            f'version = "{version}"',
            body,
            count=1,
        )
    else:
        new_body = f'version = "{version}"\n{body}'
    updated = (
        text[: section_match.start(2)]
        + new_body
        + text[section_match.end(2) :]
    )
    write_if_changed(path, updated)


def set_crate_workspace_version(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    package_match = re.search(r"(?ms)^(\[package\]\n)(.*?)(?=^\[|\Z)", text)
    if not package_match:
        raise VersionError(f"{path}: missing [package]")
    body = package_match.group(2)
    if re.search(r"(?m)^version\.workspace\s*=\s*true$", body):
        return
    if re.search(r"(?m)^version\s*=", body):
        new_body = re.sub(
            r'(?m)^version\s*=\s*"[^"]+"$',
            "version.workspace = true",
            body,
            count=1,
        )
    else:
        new_body = re.sub(
            r'(?m)^(name\s*=\s*"[^"]+"\n)',
            r"\1version.workspace = true\n",
            body,
            count=1,
        )
    updated = (
        text[: package_match.start(2)]
        + new_body
        + text[package_match.end(2) :]
    )
    write_if_changed(path, updated)


def set_pyproject_version(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated = replace_required(
        text,
        r'^version\s*=\s*"[^"]+"$',
        f'version = "{version}"',
        str(path),
    )
    write_if_changed(path, updated)


def set_dockerfile_version(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated, count = re.subn(
        r"^ARG APP_VERSION=[0-9]+\.[0-9]+\.[0-9]+$",
        f"ARG APP_VERSION={version}",
        text,
        flags=re.MULTILINE,
    )
    if count < 1:
        raise VersionError(f"{path}: expected at least one ARG APP_VERSION")
    write_if_changed(path, updated)


def set_compose_version(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated, image_count = re.subn(
        r"tonglingyu-gateway:[0-9]+\.[0-9]+\.[0-9]+",
        f"tonglingyu-gateway:{version}",
        text,
    )
    updated, env_count = re.subn(
        r"TONGLINGYU_VERSION:-[0-9]+\.[0-9]+\.[0-9]+",
        f"TONGLINGYU_VERSION:-{version}",
        updated,
    )
    if image_count < 1 or env_count < 1:
        raise VersionError(f"{path}: no compose version defaults were updated")
    write_if_changed(path, updated)


def set_script_fallback(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    if path.suffix == ".py":
        pattern = r'^PROJECT_VERSION_FALLBACK\s*=\s*"[^"]+"$'
        repl = f'PROJECT_VERSION_FALLBACK = "{version}"'
    else:
        pattern = r'^PROJECT_VERSION_FALLBACK="[^"]+"$'
        repl = f'PROJECT_VERSION_FALLBACK="{version}"'
    updated = replace_required(text, pattern, repl, str(path))
    write_if_changed(path, updated)


def set_test_version(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    updated = replace_required(
        text,
        r'^EXPECTED_PROJECT_VERSION\s*=\s*"[^"]+"$',
        f'EXPECTED_PROJECT_VERSION = "{version}"',
        str(path),
    )
    write_if_changed(path, updated)


def set_version(repo_dir: Path, version: str) -> None:
    version = validate_version(version)
    write_if_changed(repo_dir / "VERSION", f"{version}\n")
    set_workspace_cargo_version(repo_dir / "agent-platform/Cargo.toml", version)
    for crate in CRATES:
        set_crate_workspace_version(
            repo_dir / f"agent-platform/crates/{crate}/Cargo.toml"
        )
    set_pyproject_version(repo_dir / "pyproject.toml", version)
    set_dockerfile_version(
        repo_dir / "agent-platform/crates/tonglingyu-gateway/Dockerfile",
        version,
    )
    set_compose_version(repo_dir / "deploy/docker-compose.yml", version)
    for relative in SCRIPT_FALLBACK_PATHS:
        set_script_fallback(repo_dir / relative, version)
    for relative in TEST_VERSION_PATHS:
        set_test_version(repo_dir / relative, version)


def read_regex(path: Path, pattern: str) -> str | None:
    match = re.search(pattern, path.read_text(encoding="utf-8"), flags=re.MULTILINE)
    return match.group(1) if match else None


def check_version(repo_dir: Path) -> list[str]:
    errors: list[str] = []
    try:
        version = read_project_version(repo_dir)
    except VersionError as exc:
        return [str(exc)]

    cargo_version = read_regex(
        repo_dir / "agent-platform/Cargo.toml",
        r'^version\s*=\s*"([^"]+)"$',
    )
    if cargo_version != version:
        errors.append("agent-platform/Cargo.toml workspace version drift")

    for crate in CRATES:
        crate_path = repo_dir / f"agent-platform/crates/{crate}/Cargo.toml"
        crate_text = crate_path.read_text(encoding="utf-8")
        if not re.search(r"(?m)^version\.workspace\s*=\s*true$", crate_text):
            errors.append(f"{crate_path.relative_to(repo_dir)} must use workspace version")

    pyproject_version = read_regex(
        repo_dir / "pyproject.toml",
        r'^version\s*=\s*"([^"]+)"$',
    )
    if pyproject_version != version:
        errors.append("pyproject.toml version drift")

    uv_lock_path = repo_dir / "uv.lock"
    if uv_lock_path.is_file():
        lock_data = tomllib.loads(uv_lock_path.read_text(encoding="utf-8"))
        lock_version = None
        for package in lock_data.get("package", []):
            if package.get("name") == "the-story-of-stone":
                lock_version = package.get("version")
                break
        if lock_version != version:
            errors.append("uv.lock project version drift")

    cargo_lock_path = repo_dir / "agent-platform/Cargo.lock"
    if cargo_lock_path.is_file():
        lock_data = tomllib.loads(cargo_lock_path.read_text(encoding="utf-8"))
        package_versions = {
            package.get("name"): package.get("version")
            for package in lock_data.get("package", [])
            if package.get("name") in CRATES
        }
        for crate in CRATES:
            if package_versions.get(crate) != version:
                errors.append(f"agent-platform/Cargo.lock {crate} version drift")

    dockerfile_versions = set(
        re.findall(
            r"^ARG APP_VERSION=([0-9]+\.[0-9]+\.[0-9]+)$",
            (
                repo_dir / "agent-platform/crates/tonglingyu-gateway/Dockerfile"
            ).read_text(encoding="utf-8"),
            flags=re.MULTILINE,
        )
    )
    if dockerfile_versions != {version}:
        errors.append("tonglingyu-gateway Dockerfile version drift")

    compose_text = (repo_dir / "deploy/docker-compose.yml").read_text(
        encoding="utf-8"
    )
    compose_versions = set(
        re.findall(
            r"(?:tonglingyu-gateway:|TONGLINGYU_VERSION:-)"
            r"([0-9]+\.[0-9]+\.[0-9]+)",
            compose_text,
        )
    )
    if compose_versions != {version}:
        errors.append(
            "deploy/docker-compose.yml version drift: "
            + ",".join(sorted(compose_versions))
        )

    for relative in SCRIPT_FALLBACK_PATHS:
        path = repo_dir / relative
        if path.suffix == ".py":
            script_version = read_regex(
                path,
                r'^PROJECT_VERSION_FALLBACK\s*=\s*"([^"]+)"$',
            )
        else:
            script_version = read_regex(
                path,
                r'^PROJECT_VERSION_FALLBACK="([^"]+)"$',
            )
        if script_version != version:
            errors.append(f"{relative} version fallback drift")

    for relative in TEST_VERSION_PATHS:
        test_version = read_regex(
            repo_dir / relative,
            r'^EXPECTED_PROJECT_VERSION\s*=\s*"([^"]+)"$',
        )
        if test_version != version:
            errors.append(f"{relative} expected version drift")

    return errors


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--version",
        dest="show_version",
        action="store_true",
        help="Print the current project version and exit.",
    )
    subparsers = parser.add_subparsers(dest="command")
    subparsers.add_parser("current", help="Print the current project version.")
    set_parser = subparsers.add_parser("set", help="Set and sync a version.")
    set_parser.add_argument("version")
    bump_parser = subparsers.add_parser("bump", help="Bump and sync a version.")
    bump_parser.add_argument("part", choices=("patch",))
    subparsers.add_parser("check", help="Check all managed version surfaces.")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    repo_dir = repo_root()
    if args.show_version:
        print(read_project_version(repo_dir))
        return 0
    if args.command in (None, "current"):
        print(read_project_version(repo_dir))
        return 0
    if args.command == "set":
        set_version(repo_dir, args.version)
        print(read_project_version(repo_dir))
        return 0
    if args.command == "bump":
        current = read_project_version(repo_dir)
        if args.part != "patch":
            parser.error("only patch deploy bumps are supported")
        next_version = bump_patch(current)
        set_version(repo_dir, next_version)
        print(next_version)
        return 0
    if args.command == "check":
        errors = check_version(repo_dir)
        if errors:
            for error in errors:
                print(f"VERSION_DRIFT {error}", file=sys.stderr)
            return 1
        print(f"VERSION_OK {read_project_version(repo_dir)}")
        return 0
    parser.error(f"unsupported command {args.command!r}")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
