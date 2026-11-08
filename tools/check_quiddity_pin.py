#!/usr/bin/env python3
"""Warn when Specify and Draftwright's latest release use different quiddity kernels."""

import argparse
import base64
import json
import os
import re
import subprocess
import tempfile
import tomllib
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SIBLING = "pzfreo/draftwright-rust"
KERNEL = "pzfreo/quiddity"
# Its name before the 2026-11-03 rename; GitHub redirects it, and released siblings still use it.
FORMER_KERNEL = "pzfreo/quiddity-rust"
REQUIRED_TAG = "v0.4.9"


def api(path):
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "specify-core-quiddity-pin-check"}
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(f"https://api.github.com/repos/{path}", headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def latest_release(repo):
    # /releases/latest excludes prereleases; these projects currently publish alphas.
    releases, page = [], 1
    while True:
        batch = api(f"{repo}/releases?per_page=100&page={page}")
        releases.extend(release for release in batch if not release["draft"] and release["published_at"])
        if len(batch) < 100:
            break
        page += 1
    if not releases:
        raise ValueError(f"{repo} has no published releases")
    return max(releases, key=lambda release: release["published_at"])["tag_name"]


def pins(manifest, sources=(KERNEL,)):
    result = {}
    workspace = manifest.get("workspace", {}).get("dependencies", {})
    for name in ("haecceity", "quiddity"):
        dependency = manifest["dependencies"][name]
        if not isinstance(dependency, dict):
            raise ValueError(f"{name} must be pinned to a pzfreo/quiddity tag or rev")
        if dependency.get("workspace"):
            dependency = workspace[name]
        if dependency.get("git", "").removesuffix(".git").rstrip("/") not in {f"https://github.com/{s}" for s in sources}:
            raise ValueError(f"{name} is not sourced from {KERNEL}")
        fields = [key for key in ("tag", "rev") if key in dependency]
        if len(fields) != 1:
            raise ValueError(f"{name} must have exactly one tag or rev pin")
        result[name] = (fields[0], dependency[fields[0]])
    return result


def check_wasm_pin(local, manifest=ROOT / "crates/specify-core-wasm/Cargo.toml"):
    """The WASM kernel must follow the root pin (v0.4.9 for this release)."""
    dependency = tomllib.loads(manifest.read_text())["dependencies"]["haecceity"]
    kind, value = local["haecceity"]
    if (dependency.get("git", "").removesuffix(".git").rstrip("/") != f"https://github.com/{KERNEL}"
            or dependency.get(kind) != value
            or any(key in dependency for key in ("tag", "rev") if key != kind)):
        raise ValueError("WASM haecceity must use the same source and pin as the root crate")


def resolve(pin):
    kind, value = pin
    if kind == "rev" and re.fullmatch(r"[0-9a-fA-F]{40}", value):
        return value.lower()
    return api(f"{KERNEL}/commits/{urllib.parse.quote(value, safe='')}")["sha"]


def describe(package_pins):
    return ", ".join(f"{name} {kind} {value}" for name, (kind, value) in package_pins.items())


def compare(local, sibling):
    resolved = {pin: resolve(pin) for pin in set(local.values()) | set(sibling.values())}
    # Internal disagreement between the two kernel crates is also a mismatch.
    return len(set(resolved.values())) == 1, resolved


def warning(message):
    escaped = message.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
    print(f"::warning::{escaped}")


def append_note(tag, repo, note):
    environment = os.environ.copy()
    if environment.get("GITHUB_TOKEN"):
        environment["GH_TOKEN"] = environment["GITHUB_TOKEN"]
    result = subprocess.run(["gh", "release", "view", tag, "--repo", repo, "--json", "body"], env=environment, check=True, capture_output=True, text=True)
    body = json.loads(result.stdout)["body"] or ""
    # Rerunning the release job replaces its own note without duplicating it.
    body = re.sub(r"(?m)^Kernel note:.*(?:\n|$)", "", body).rstrip()
    with tempfile.NamedTemporaryFile(mode="w", suffix=".md") as source:
        source.write((body + "\n\n" if body else "") + note + "\n")
        source.flush()
        subprocess.run(["gh", "release", "edit", tag, "--repo", repo, "--notes-file", source.name], env=environment, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=ROOT / "Cargo.toml")
    parser.add_argument("--release", help="on mismatch append a Kernel note to this Specify release with gh release edit")
    parser.add_argument("--repository", default=os.environ.get("GH_REPO", "pzfreo/specify-core"))
    args = parser.parse_args()
    try:
        local = pins(tomllib.loads(args.manifest.read_text()))
        if any(pin != ("tag", REQUIRED_TAG) for pin in local.values()):
            warning(f"Expected both kernel crates pinned to {REQUIRED_TAG}: {describe(local)}")
        if args.manifest.resolve() == (ROOT / "Cargo.toml").resolve():
            check_wasm_pin(local)
        tag = latest_release(SIBLING)
        contents = api(f"{SIBLING}/contents/Cargo.toml?ref={urllib.parse.quote(tag, safe='')}")
        sibling = pins(tomllib.loads(base64.b64decode(contents["content"]).decode()), (KERNEL, FORMER_KERNEL))
        print(f"specify-core-rust: {describe(local)}")
        print(f"{SIBLING}@{tag}: {describe(sibling)}")
        matches, resolved = compare(local, sibling)
        if matches:
            print(f"Quiddity pins match: {next(iter(resolved.values()))} (haecceity and quiddity in both packages).")
        else:
            note = f"Kernel note: specify-core-rust uses {describe(local)}; {SIBLING}@{tag} uses {describe(sibling)}. Different kernels may number STEP faces differently."
            warning(note)
            if args.release:
                append_note(args.release, args.repository, note)
                print(f"Updated release {args.release} with the Kernel note.")
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        warning(f"Quiddity pin check could not complete: {error}. Release remains allowed; check the kernel pins manually.")
    # One repository must release first after a kernel bump; this is always advisory.
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
