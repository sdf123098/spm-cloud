#!/usr/bin/env python3
"""End-to-end proof of the thumbnail contract against a real server process.

Starts a throwaway instance (temp dir, loopback port 8799, no touching the live
database), uploads a model, renders it with tools/render-thumbnails.mjs through
headless chromium, then asserts the whole published contract:

  - a fresh revision lists as `pending` with no URL, and its thumbnail 404s
  - after rendering it lists as `ready` and the anonymous URL serves image/png
  - Cache-Control is the immutable year, so a CDN can pin it
  - ETag equals the sha256 of the bytes, and If-None-Match answers 304
  - the queue drains to zero

Usage: python3 tools/thumbnail_e2e.py [--binary target/release/spm-cloud]
Exit 0 = all assertions held. Needs node + a Playwright chromium.
"""
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent.as_posix()
BINARY = sys.argv[sys.argv.index("--binary") + 1] if "--binary" in sys.argv else f"{ROOT}/target/release/spm-cloud"
work = tempfile.mkdtemp(prefix="thumb-e2e-")
token = "thumb-e2e-owner-token"
pathlib.Path(work, "token.txt").write_text(token)
cfg = {
  "instance": {"instance_id": "thumb-e2e", "origin": "https://thumbnail.invalid", "bind": "127.0.0.1:8799"},
  "storage": {"data_dir": f"{work}/data", "database_path": f"{work}/data/spm-cloud.db", "object_dir": f"{work}/data/objects"},
  "auth": {"bootstrap_account_id": "account_local", "bootstrap_access_token_file": f"{work}/token.txt", "allow_self_registration": True},
  "web_ui": {"enabled": True},
}
cfg_path = pathlib.Path(work, "config.json"); cfg_path.write_text(json.dumps(cfg))
log = open(f"{work}/service.log", "wb")
proc = subprocess.Popen([BINARY, "--config", str(cfg_path)], stdout=log, stderr=log)
base = "http://127.0.0.1:8799"
def req(method, path, data=None, headers=None, ctype=None, auth=True):
    h = {"Authorization": f"Bearer {token}"} if auth else {}
    h.update(headers or {})
    if ctype: h["Content-Type"] = ctype
    r = urllib.request.Request(base + path, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(r, timeout=30) as resp:
            return resp.status, {k.lower(): v for k, v in resp.headers.items()}, resp.read()
    except urllib.error.HTTPError as e:
        return e.code, {k.lower(): v for k, v in e.headers.items()}, e.read()

try:
    for _ in range(100):
        try:
            if req("GET", "/health")[0] == 200: break
        except Exception: pass
        time.sleep(0.2)
    else:
        print("SERVICE DID NOT START"); print(open(f"{work}/service.log").read()[-2000:]); sys.exit(1)

    model = json.dumps({"minecraft:geometry": [{"description": {"texture_width": 16, "texture_height": 16},
        "bones": [{"name": "body", "pivot": [0,0,0], "cubes": [{"origin": [-4,0,-4], "size": [8,24,8], "uv": [0,0]}]},
                  {"name": "head", "pivot": [0,24,0], "cubes": [{"origin": [-4,24,-4], "size": [8,8,8], "uv": [0,0]}]}]}]}).encode()
    st, _, body = req("POST", "/admin/api/assets", model, {
        "Idempotency-Key": str(uuid.uuid4()), "X-Asset-Id": "e2e-model", "X-Asset-Name": "e2e model",
        "X-Asset-Format": "bbmodel", "X-Asset-Visibility": "PUBLIC"})
    print("upload:", st, body[:160])
    assert st == 201, body

    st, _, body = req("GET", "/v1/assets?scope=accessible&limit=5")
    entry = [e for e in json.loads(body)["entries"] if e["asset_id"] == "e2e-model"][0]
    print("list before render:", entry["thumbnail_status"], entry["thumbnail"])
    assert entry["thumbnail_status"] == "pending" and entry["thumbnail"] is None

    st, _, body = req("GET", "/v1/public/assets/e2e-model/revisions/1/thumbnail")
    print("public thumbnail before render:", st); assert st == 404

    r = subprocess.run(["node", "tools/render-thumbnails.mjs", "--site", base, "--token", token,
                        "--limit", "5", "--concurrency", "2"], cwd=ROOT, capture_output=True, text=True, timeout=240)
    print(r.stdout.strip()); print(r.stderr.strip()[:800])
    assert r.returncode == 0, f"renderer exit {r.returncode}"

    st, hdrs, body = req("GET", "/v1/public/assets/e2e-model/revisions/1/thumbnail")
    print("public thumbnail after render:", st, hdrs.get("content-type"), hdrs.get("cache-control"), len(body), "bytes")
    assert st == 200 and body[:8] == b"\x89PNG\r\n\x1a\n"
    assert hdrs.get("cache-control") == "public, max-age=31536000, immutable"
    assert hdrs.get("content-type") == "image/png"
    etag = hdrs.get("etag"); print("etag:", etag, "sha256(body):", hashlib.sha256(body).hexdigest())
    assert etag.strip('"') == hashlib.sha256(body).hexdigest()
    st2, _, _ = req("GET", "/v1/public/assets/e2e-model/revisions/1/thumbnail", headers={"If-None-Match": etag})
    print("conditional GET:", st2); assert st2 == 304

    st, _, body = req("GET", "/v1/assets?scope=accessible&limit=5")
    entry = [e for e in json.loads(body)["entries"] if e["asset_id"] == "e2e-model"][0]
    print("list after render:", entry["thumbnail_status"], entry["thumbnail"])
    assert entry["thumbnail_status"] == "ready"
    assert entry["thumbnail"] == "/v1/public/assets/e2e-model/revisions/1/thumbnail"

    st, _, body = req("GET", "/admin/api/thumbnails/pending?limit=5")
    print("queue remaining:", json.loads(body)["remaining"]); assert json.loads(body)["remaining"] == 0

    print("E2E OK")
finally:
    proc.terminate()
    try: proc.wait(timeout=10)
    except Exception: proc.kill()
    if proc.returncode not in (0, None, -15):
        print("service log tail:")
        print(open(f"{work}/service.log").read()[-4000:])
    shutil.rmtree(work, ignore_errors=True)
