"""Offline regressions for the CI safeguards: python3 -m unittest discover -s tools."""

import contextlib
import io
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_quiddity_pin as kernel
import check_wasm_api as api
import wasm_size


class WorkspacePinTests(unittest.TestCase):
    def test_release_and_wasm_use_the_reviewed_kernel(self):
        local = kernel.pins(kernel.tomllib.loads((kernel.ROOT / "Cargo.toml").read_text()))
        self.assertEqual(set(local.values()), {("tag", "v0.4.9")})
        self.assertEqual(set(local.values()), {("tag", kernel.REQUIRED_TAG)})
        kernel.check_wasm_pin(local)

    def test_mismatched_wasm_pin_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "Cargo.toml"
            manifest.write_text('[dependencies.haecceity]\ngit = "https://github.com/pzfreo/quiddity"\ntag = "v0.2.8"\n')
            with self.assertRaisesRegex(ValueError, "same source and pin"):
                kernel.check_wasm_pin({"haecceity": ("tag", "v0.4.9")}, manifest)

    def test_only_a_released_sibling_may_use_the_former_kernel_name(self):
        former = kernel.tomllib.loads(
            '[dependencies.haecceity]\ngit = "https://github.com/pzfreo/quiddity-rust"\ntag = "v0.4.9"\n'
            '[dependencies.quiddity]\ngit = "https://github.com/pzfreo/quiddity-rust"\ntag = "v0.4.9"\n')
        with self.assertRaisesRegex(ValueError, "not sourced from pzfreo/quiddity"):
            kernel.pins(former)
        sibling = kernel.pins(former, (kernel.KERNEL, kernel.FORMER_KERNEL))
        self.assertEqual(set(sibling.values()), {("tag", "v0.4.9")})


class ApiTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.generated = self.root / "generated.d.ts"
        self.snapshot = self.root / "api.d.ts"
        self.changelog = self.root / "CHANGELOG.md"
        self.generated.write_text("export function review(input: string): string;\n")
        self.snapshot.write_text(api.normalize(self.generated.read_text()))
        self.changelog.write_text("## Unreleased\n\n### WASM API\n\n- Existing note.\n\n## 1.0\n")
        self.git("init", "--quiet")
        self.git("add", ".")
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "baseline")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True)

    def check(self):
        with contextlib.redirect_stdout(io.StringIO()):
            return api.check(self.root, self.generated, self.snapshot, "HEAD")

    def test_snapshot_and_comment_or_order_noise_pass(self):
        self.assertEqual(self.check(), 0)
        self.generated.write_text("/* version 1.2.3 */\nexport function review(input: string): string; // docs\n")
        self.assertEqual(self.check(), 0)

    def test_actual_change_requires_new_unreleased_api_bullet(self):
        self.generated.write_text("export function review(input: number): string;\n")
        self.assertEqual(self.check(), 1)  # An existing API note does not count.
        original = self.changelog.read_text()
        self.changelog.write_text(original + "\n### WASM API\n- Released note.\n")
        self.assertEqual(self.check(), 1)
        self.changelog.write_text(original.replace("### WASM API", "### Added\n- Unrelated note.\n\n### WASM API"))
        self.assertEqual(self.check(), 1)
        self.changelog.write_text(original.replace("- Existing note.", "- Existing note.\n- review now accepts a number."))
        self.assertEqual(self.check(), 0)

    def test_snapshot_update_alone_does_not_bypass_gate(self):
        self.generated.write_text("export function added(): void;\n")
        self.snapshot.write_text(api.normalize(self.generated.read_text()))
        self.assertEqual(self.check(), 1)

    def test_normalization_sorts_declarations_and_members_preserving_types(self):
        left = '''/* generated v1 */
export interface InitOutput {
    readonly z: (x: { value: number }) => void;
    readonly a: "https://example.com/a  b";
}
export function z(): string;
export function a(): number;
'''
        right = '''export function a(): number;
export function z(): string;
export interface InitOutput {
    readonly a: "https://example.com/a  b";
    readonly z: (x: { value: number }) => void;
}
'''
        self.assertEqual(api.normalize(left), api.normalize(right))
        self.assertIn('"https://example.com/a  b"', api.normalize(left))
        self.assertNotEqual(api.normalize(left), api.normalize(left.replace("number", "boolean")))
        self.assertEqual(api.normalize(left), api.normalize(api.normalize(left)))


class SizeTests(unittest.TestCase):
    def test_ceiling_is_inclusive_and_always_prints_headroom(self):
        for raw, expected in [(100, 0), (101, 1)]:
            with self.subTest(raw=raw), contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(wasm_size.check({"raw_bytes": raw, "gzip_bytes": 80, "brotli_bytes": 60}, {"ceiling_bytes": 100}), expected)
                self.assertIn("headroom", output.getvalue())


class KernelTests(unittest.TestCase):
    def test_latest_includes_prereleases_and_skips_drafts(self):
        releases = [
            {"tag_name": "v1", "published_at": "2026-10-10", "draft": False},
            {"tag_name": "v2-alpha", "published_at": "2026-10-16", "draft": False, "prerelease": True},
            {"tag_name": "v3", "published_at": None, "draft": True},
        ]
        with patch.object(kernel, "api", return_value=releases):
            self.assertEqual(kernel.latest_release(kernel.SIBLING), "v2-alpha")

    def test_tag_and_rev_can_match_and_internal_drift_is_detected(self):
        sha = "a" * 40
        local = dict.fromkeys(("haecceity", "quiddity"), ("rev", sha))
        sibling = dict.fromkeys(("haecceity", "quiddity"), ("tag", "v1"))
        with patch.object(kernel, "api", return_value={"sha": sha}):
            self.assertTrue(kernel.compare(local, sibling)[0])
            local["haecceity"] = ("rev", "b" * 40)
            self.assertFalse(kernel.compare(local, sibling)[0])

    def test_reads_both_pins(self):
        manifest = {"dependencies": {name: {"git": f"https://github.com/{kernel.KERNEL}", "tag": "v1"} for name in ("haecceity", "quiddity")}}
        self.assertEqual(set(kernel.pins(manifest)), {"haecceity", "quiddity"})
        del manifest["dependencies"]["quiddity"]["tag"]
        with self.assertRaises(ValueError):
            kernel.pins(manifest)

    def test_release_note_preserves_body_and_is_idempotent(self):
        note = "Kernel note: current mismatch."
        bodies = []

        def gh(args, **kwargs):
            if args[2] == "view":
                return subprocess.CompletedProcess(args, 0, stdout=json.dumps({"body": "Release details.\n\nKernel note: previous mismatch.\n"}))
            self.assertEqual(args[:3], ["gh", "release", "edit"])
            bodies.append(Path(args[-1]).read_text())
            return subprocess.CompletedProcess(args, 0)

        with patch.object(kernel.subprocess, "run", side_effect=gh):
            kernel.append_note("v1", "owner/repo", note)
        self.assertEqual(bodies, ["Release details.\n\n" + note + "\n"])

    def test_mismatch_warns_and_edits_release_without_failing(self):
        local = dict.fromkeys(("haecceity", "quiddity"), ("rev", "a" * 40))
        sibling = dict.fromkeys(("haecceity", "quiddity"), ("tag", "v2"))
        with patch("sys.argv", ["check_quiddity_pin.py", "--release", "v1"]), \
             patch.object(kernel, "pins", side_effect=[local, sibling]), \
             patch.object(kernel, "check_wasm_pin"), \
             patch.object(kernel, "latest_release", return_value="v2"), \
             patch.object(kernel, "api", return_value={"content": ""}), \
             patch.object(kernel, "compare", return_value=(False, {})), \
             patch.object(kernel, "append_note") as edit, \
             contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(kernel.main(), 0)
            self.assertIn("::warning::Kernel note:", output.getvalue())
            self.assertIn("haecceity", edit.call_args.args[2])
            self.assertIn("quiddity", edit.call_args.args[2])

    def test_network_failure_is_advisory(self):
        with patch("sys.argv", ["check_quiddity_pin.py"]), \
             patch.object(kernel, "latest_release", side_effect=OSError("offline")), \
             contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(kernel.main(), 0)
            self.assertIn("::warning::", output.getvalue())


if __name__ == "__main__":
    unittest.main()
