#!/usr/bin/env python3
"""Smoke-test the app and engine built into a specify-core binary (ADR P12).

    python3 tools/open_smoke.py BINARY STEP

Runs `BINARY open STEP --no-browser --read-only` with no --app/--wasm overrides, an empty HOME
and every proxy pointed at a closed port (so a download would fail), then fetches the page, its
config and the engine the config names, checks their content types and that the engine is
WebAssembly, and stops the command. Exits non-zero on any failure.
"""

import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.request


def main() -> int:
    binary, step = sys.argv[1:3]
    with tempfile.TemporaryDirectory() as home:
        env = {k: v for k, v in os.environ.items() if k not in ("SPECIFY_APP_DIR", "SPECIFY_WASM_DIR")}
        env.update(HOME=home, USERPROFILE=home, LOCALAPPDATA=home, XDG_CACHE_HOME=os.path.join(home, "cache"))
        for proxy in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
            env[proxy] = "http://127.0.0.1:9"
        env.pop("NO_PROXY", None)
        env.pop("no_proxy", None)
        child = subprocess.Popen(
            [binary, "open", step, "--no-browser", "--read-only"],
            env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            line = child.stdout.readline()
            match = re.search(r"http://127\.0\.0\.1:\d+/[0-9a-f]{32}/", line)
            if not match:
                child.kill()
                print(f"no address printed: {line!r} {child.stderr.read()!r}", file=sys.stderr)
                return 1
            url = match.group(0)
            # The client itself goes straight to 127.0.0.1.
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

            def get(rel: str, content_type: str) -> bytes:
                with opener.open(url + rel, timeout=30) as response:
                    got = response.headers.get("Content-Type")
                    assert response.status == 200, (rel, response.status)
                    assert got == content_type, (rel, got)
                    return response.read()

            page = get("", "text/html; charset=utf-8")
            assert b"<html" in page.lower() or b"<!doctype html" in page.lower(), page[:200]
            config = json.loads(get("config", "application/json"))
            wasm_url = config["wasmUrl"]
            glue = get(wasm_url, "text/javascript; charset=utf-8")
            assert b"specify_core_wasm_bg.wasm" in glue, "the glue names its WASM"
            engine = get(wasm_url.rsplit("/", 1)[0] + "/specify_core_wasm_bg.wasm", "application/wasm")
            assert engine[:4] == b"\0asm", engine[:4]
            manifest = json.loads(get("specify-ui-app.json", "application/json"))
            assert manifest["api"] == config["api"], (manifest, config)
            assert manifest["version"] == config["specifyCore"], (manifest, config)
            leftovers = [os.path.join(d, f) for d, _, fs in os.walk(home) for f in fs]
            assert not leftovers, f"files written under HOME: {leftovers}"
            print(f"ok: page {len(page)} B, engine {len(engine)} B, app {manifest['version']} (api {manifest['api']}) served by {binary}")
            return 0
        finally:
            child.kill()
            child.wait()


if __name__ == "__main__":
    sys.exit(main())
