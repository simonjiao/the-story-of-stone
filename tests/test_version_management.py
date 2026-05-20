from __future__ import annotations

import importlib.util
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


EXPECTED_PROJECT_VERSION = "0.1.15"
EXPECTED_VERSION_FALLBACK = "latest"
REPO_DIR = Path(__file__).resolve().parents[1]
VERSION_SCRIPT = REPO_DIR / "scripts/version.py"


def load_version_module():
    spec = importlib.util.spec_from_file_location("project_version", VERSION_SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load scripts/version.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class VersionManagementTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.version_module = load_version_module()

    def test_version_format_and_bumps(self) -> None:
        validate = self.version_module.validate_version
        bump_version = self.version_module.bump_version
        bump_patch = self.version_module.bump_patch
        bump_minor = self.version_module.bump_minor
        self.assertEqual(validate("0.1.0"), "0.1.0")
        self.assertEqual(bump_patch("0.1.0"), "0.1.1")
        self.assertEqual(bump_patch("10.200.999"), "10.200.1000")
        self.assertEqual(bump_minor("0.1.13"), "0.2.0")
        self.assertEqual(bump_version("10.200.999", "minor"), "10.201.0")
        with self.assertRaises(Exception):
            bump_version("0.1.0", "major")
        for invalid in ("", "v0.1.0", "0.1", "0.1.0-rc1", "01.1.0"):
            with self.subTest(invalid=invalid):
                with self.assertRaises(Exception):
                    validate(invalid)

    def test_repository_version_surfaces_are_in_sync(self) -> None:
        errors = self.version_module.check_version(REPO_DIR)
        self.assertEqual(errors, [])
        self.assertEqual(
            self.version_module.PROJECT_VERSION_FALLBACK,
            EXPECTED_VERSION_FALLBACK,
        )
        self.assertEqual(
            self.version_module.read_project_version(REPO_DIR),
            EXPECTED_PROJECT_VERSION,
        )

    def test_set_version_updates_fixture_surfaces(self) -> None:
        with tempfile.TemporaryDirectory() as raw_dir:
            tmp = Path(raw_dir)
            self._copy_version_fixture(tmp)
            self.version_module.set_version(tmp, "1.2.3")
            errors = self.version_module.check_version(tmp)
            self.assertEqual(errors, [])
            self.assertEqual((tmp / "VERSION").read_text(encoding="utf-8"), "1.2.3\n")
            self.assertIn(
                'version = "1.2.3"',
                (tmp / "agent-platform/Cargo.toml").read_text(encoding="utf-8"),
            )
            self.assertIn(
                "version.workspace = true",
                (
                    tmp / "agent-platform/crates/tonglingyu-gateway/Cargo.toml"
                ).read_text(encoding="utf-8"),
            )
            compose_text = (tmp / "deploy/docker-compose.yml").read_text(
                encoding="utf-8"
            )
            self.assertIn(
                (
                    "tonglingyu-gateway:"
                    "${TONGLINGYU_VERSION:-latest}"
                ),
                compose_text,
            )
            self.assertNotIn("tonglingyu-gateway:1.2.3", compose_text)

    def test_versioned_scripts_report_project_version(self) -> None:
        expected = f"{EXPECTED_PROJECT_VERSION}\n"
        commands = (
            ["uv", "run", "--no-sync", "python", str(VERSION_SCRIPT), "--version"],
            ["bash", str(REPO_DIR / "scripts/qa.sh"), "--version"],
            ["bash", str(REPO_DIR / "deploy/scripts/start-local-stack.sh"), "--version"],
        )
        for command in commands:
            with self.subTest(command=command):
                result = subprocess.run(
                    command,
                    cwd=REPO_DIR,
                    check=True,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
                self.assertEqual(result.stdout, expected)

    @staticmethod
    def _copy_version_fixture(target: Path) -> None:
        paths = [
            "VERSION",
            "pyproject.toml",
            "agent-platform/Cargo.toml",
            "agent-platform/crates/agent-core/Cargo.toml",
            "agent-platform/crates/agent-runtime/Cargo.toml",
            "agent-platform/crates/tonglingyu-runtime/Cargo.toml",
            "agent-platform/crates/tonglingyu-gateway/Cargo.toml",
            "agent-platform/crates/tonglingyu-gateway/Dockerfile",
            "deploy/docker-compose.yml",
            "scripts/version.py",
            "scripts/qa.sh",
            "deploy/scripts/start-local-stack.sh",
            "tests/test_version_management.py",
        ]
        for relative in paths:
            source = REPO_DIR / relative
            destination = target / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)


if __name__ == "__main__":
    unittest.main()
