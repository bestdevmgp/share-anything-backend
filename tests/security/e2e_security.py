"""End-to-end verification of the security-patch behaviors against a running
server. Stdlib only. Run with: python3 -I e2e_security.py

Env: BASE (default http://127.0.0.1:18080), SESSION_SECRET, ADMIN_PASSWORD,
MYSQL (mysql CLI prefix for direct DB setup/inspection).
"""
import base64
import concurrent.futures as cf
import hashlib
import hmac
import http.client
import itertools
import json
import os
import subprocess
import sys
import time
import urllib.parse
import uuid

BASE = os.environ.get("BASE", "http://127.0.0.1:18080")
HOST = urllib.parse.urlparse(BASE).hostname
PORT = urllib.parse.urlparse(BASE).port
SESSION_SECRET = os.environ["SESSION_SECRET"].encode()
ADMIN_PASSWORD = os.environ["ADMIN_PASSWORD"]
MYSQL = os.environ["MYSQL"].split()
FRONTEND = "https://shareany.test"

results = []
_ip_counter = itertools.count(1)


def fresh_ip():
    """A distinct private client IP per call so per-IP limits don't bleed
    between checks (private range => no external geolocation lookups)."""
    n = next(_ip_counter)
    return f"10.77.{n // 250}.{n % 250 + 1}"


def check(name, cond, detail=""):
    results.append((name, bool(cond), detail))
    print(("PASS " if cond else "FAIL ") + name + (f"  [{detail}]" if detail and not cond else ""))


def request(method, path, headers=None, body=None, ip=None):
    conn = http.client.HTTPConnection(HOST, PORT, timeout=60)
    h = dict(headers or {})
    h.setdefault("CF-Connecting-IP", ip or fresh_ip())
    if isinstance(body, (dict, list)):
        body = json.dumps(body).encode()
        h.setdefault("Content-Type", "application/json")
    conn.request(method, path, body=body, headers=h)
    resp = conn.getresponse()
    data = resp.read()
    out = (resp.status, {k.lower(): v for k, v in resp.getheaders()}, data)
    conn.close()
    return out


def mysql(sql):
    out = subprocess.run(MYSQL + ["-N", "-B", "-e", sql], capture_output=True, text=True, check=True)
    return out.stdout.strip()


def b64url(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def session_token():
    now = int(time.time())
    header = b64url(json.dumps({"alg": "HS256", "typ": "JWT"}).encode())
    payload = b64url(json.dumps({"kind": "session", "iat": now, "exp": now + 1800}).encode())
    sig = hmac.new(SESSION_SECRET, f"{header}.{payload}".encode(), hashlib.sha256).digest()
    return f"{header}.{payload}.{b64url(sig)}"


def query_param(url, key):
    return urllib.parse.parse_qs(urllib.parse.urlparse(url).query).get(key, [None])[0]


# --------------------------------------------------------------------------
# C1: OAuth redirect allowlist
# --------------------------------------------------------------------------
def test_oauth():
    for provider in ("google", "naver", "kakao", "apple"):
        default_cb = f"{BASE}/auth/callback/{provider}"
        s, h, _ = request("GET", f"/auth/{provider}?redirect_uri=" + urllib.parse.quote("https://evil.example/cb", safe=""))
        state = query_param(h.get("location", ""), "state")
        check(f"C1 {provider} login: foreign redirect_uri replaced by default", s == 307 and state == default_cb, f"status={s} state={state}")

        legit = f"{FRONTEND}/auth/callback/{provider}"
        s, h, _ = request("GET", f"/auth/{provider}?redirect_uri=" + urllib.parse.quote(legit, safe=""))
        state = query_param(h.get("location", ""), "state")
        check(f"C1 {provider} login: allowlisted redirect_uri preserved", s == 307 and state == legit, f"status={s} state={state}")

    s, h, _ = request("GET", "/auth/google?redirect_uri=" + urllib.parse.quote("http://localhost:3000/auth/callback/google", safe=""))
    check("C1 CORS-listed origin (localhost:3000) allowed", query_param(h.get("location", ""), "state") == "http://localhost:3000/auth/callback/google")

    s, h, _ = request("GET", "/auth/google")
    check("C1 no redirect_uri -> default callback", query_param(h.get("location", ""), "state") == f"{BASE}/auth/callback/google")

    for provider in ("google", "naver", "kakao"):
        for evil in ("https://evil.example/x", "https://shareany.test.evil.example/x", "javascript:alert(1)", "//evil.example/x"):
            s, h, _ = request("GET", f"/auth/{provider}/callback?code=CODE123&state=" + urllib.parse.quote(evil, safe=""))
            loc = h.get("location", "")
            check(f"C1 {provider} callback: state={evil!r} cannot redirect off-site", s == 307 and loc.startswith(FRONTEND + "?code=CODE123"), f"status={s} loc={loc}")
        legit = f"{FRONTEND}/auth/callback/{provider}"
        s, h, _ = request("GET", f"/auth/{provider}/callback?code=CODE123&state=" + urllib.parse.quote(legit, safe=""))
        loc = h.get("location", "")
        check(f"C1 {provider} callback: legit state keeps code round-trip", s == 307 and loc.startswith(legit + "?code=CODE123"), f"loc={loc}")

    form = urllib.parse.urlencode({"code": "ACODE", "state": "https://evil.example/apple"}).encode()
    s, h, _ = request("POST", "/auth/apple/callback", {"Content-Type": "application/x-www-form-urlencoded"}, form)
    loc = h.get("location", "")
    check("C1 apple form_post callback: foreign state falls back to frontend", s == 303 and loc.startswith(FRONTEND + "?code=ACODE"), f"status={s} loc={loc}")
    form = urllib.parse.urlencode({"code": "ACODE", "state": f"{FRONTEND}/auth/callback/apple"}).encode()
    s, h, _ = request("POST", "/auth/apple/callback", {"Content-Type": "application/x-www-form-urlencoded"}, form)
    check("C1 apple form_post callback: legit state preserved", h.get("location", "").startswith(f"{FRONTEND}/auth/callback/apple?code=ACODE"))


# --------------------------------------------------------------------------
# C2 / M4: email magic-link + code flow
# --------------------------------------------------------------------------
def email_send(email, device_id, ip):
    s, _, d = request("POST", "/auth/email/send", {"X-Session-Token": session_token()}, {"email": email, "device_id": device_id}, ip=ip)
    assert s == 200, (s, d)
    return json.loads(d)["session_id"]


def session_row(session_id):
    token, code, status = mysql(f"SELECT token, verification_code, status FROM email_auth_sessions WHERE id='{session_id}'").split("\t")
    return token, code, status


def verify_code(session_id, code, ip):
    return request("POST", "/auth/email/verify-code", {"X-Session-Token": session_token()}, {"session_id": session_id, "code": code}, ip=ip)


def test_email():
    ip = fresh_ip()
    sid = email_send(f"victim-{uuid.uuid4().hex[:8]}@example.com", "devA", ip)
    token, code, status = session_row(sid)
    check("M4 session starts pending", status == "pending")

    s, h, _ = verify_code(sid, code, ip)
    check("M4 correct code rejected while magic link unopened (pending -> 401)", s == 401 and "set-cookie" not in h, f"status={s}")

    s, _, d = request("POST", "/auth/email/verify", body={"token": token, "device_id": "devB"}, ip=ip)
    body = json.loads(d) if d else {}
    check("C2 cross-device verify reveals code and sets verified", s == 200 and body.get("verification_code") == code and session_row(sid)[2] == "verified", f"status={s}")

    s, _, _ = request("POST", "/auth/email/verify", body={"token": token, "device_id": "devB"}, ip=ip)
    check("C2 replayed magic-link click -> 410", s == 410, f"status={s}")

    s, h, d = verify_code(sid, code, ip)
    check("C2 verify-code after verified -> 200 + cookie", s == 200 and "__Host-auth_token=" in h.get("set-cookie", ""), f"status={s} body={d[:120]!r}")

    s, h, _ = verify_code(sid, code, ip)
    check("C2 same code cannot complete twice -> 410", s == 410 and "set-cookie" not in h, f"status={s}")

    s, h, d = request("GET", f"/auth/email/status/{sid}", ip=ip)
    check("C2 polling within TTL still completes (legit flow kept)", s == 200 and json.loads(d).get("status") == "completed" and "set-cookie" in h, f"status={s}")

    mysql(f"UPDATE email_auth_sessions SET expires_at = DATE_SUB(UTC_TIMESTAMP(), INTERVAL 1 MINUTE) WHERE id='{sid}'")
    s, h, d = request("GET", f"/auth/email/status/{sid}", ip=ip)
    check("C2 expired completed session no longer mints sessions", s == 200 and json.loads(d).get("status") == "expired" and "set-cookie" not in h, f"status={s} body={d[:80]!r}")

    ip2 = fresh_ip()
    sid2 = email_send(f"victim-{uuid.uuid4().hex[:8]}@example.com", "devA", ip2)
    token2, code2, _ = session_row(sid2)
    request("POST", "/auth/email/verify", body={"token": token2, "device_id": "devB"}, ip=ip2)
    wrong = "000000" if code2 != "000000" else "111111"
    statuses = [verify_code(sid2, wrong, ip2)[0] for _ in range(5)]
    check("M4 wrong codes -> 401 x5", statuses == [401] * 5, f"{statuses}")
    s, h, _ = verify_code(sid2, code2, ip2)
    check("M4 after 5 wrong attempts even the right code is refused (410)", s == 410 and "set-cookie" not in h, f"status={s}")


# --------------------------------------------------------------------------
# H3 / M3: rate limits on admin and OG; L2 swagger auth
# --------------------------------------------------------------------------
def test_rate_limits_and_swagger():
    ip = fresh_ip()
    codes = [request("GET", "/admin/api-keys/applications", {"X-Admin-Password": "wrong"}, ip=ip)[0] for _ in range(51)]
    check("H3 admin wrong password -> 401 until limit", all(c == 401 for c in codes[:50]), f"{set(codes[:50])}")
    check("H3 admin 51st request in a minute -> 429", codes[50] == 429, f"{codes[50]}")
    s, _, _ = request("GET", "/admin/api-keys/applications", {"X-Admin-Password": ADMIN_PASSWORD})
    check("H3 admin correct password from fresh IP -> 200", s == 200, f"{s}")

    ip = fresh_ip()
    codes = [request("GET", "/og/000000", ip=ip)[0] for _ in range(51)]
    check("M3 OG page served normally under the limit", all(c == 200 for c in codes[:50]), f"{set(codes[:50])}")
    check("M3 OG 51st request in a minute -> 429", codes[50] == 429, f"{codes[50]}")

    s, _, _ = request("GET", "/swagger-ui/")
    check("L2 swagger without credentials -> 401", s == 401, f"{s}")
    bad = base64.b64encode(b"sw:nope").decode()
    s, _, _ = request("GET", "/swagger-ui/", {"Authorization": f"Basic {bad}"})
    check("L2 swagger wrong password -> 401", s == 401, f"{s}")
    good = base64.b64encode(b"sw:swpw").decode()
    s, _, _ = request("GET", "/swagger-ui/", {"Authorization": f"Basic {good}"})
    check("L2 swagger correct credentials -> 2xx/3xx", 200 <= s < 400, f"{s}")
    s, _, _ = request("GET", "/api-docs/openapi.json", {"Authorization": f"Basic {good}"})
    check("L2 openapi.json with credentials -> 200", s == 200, f"{s}")


# --------------------------------------------------------------------------
# L1: headers
# --------------------------------------------------------------------------
def test_headers():
    for path in ("/health", "/auth/me", "/og/000000", "/does-not-exist"):
        s, h, _ = request("GET", path)
        check(f"L1 nosniff + referrer-policy on {path}", h.get("x-content-type-options") == "nosniff" and h.get("referrer-policy") == "strict-origin-when-cross-origin", f"{s} {h.get('x-content-type-options')}")
    s, h, _ = request("GET", "/health")
    check("L1 no X-Frame-Options (framing intentionally untouched)", "x-frame-options" not in h)


# --------------------------------------------------------------------------
# M1: one-time download races (needs S3 mock)
# --------------------------------------------------------------------------
def make_user_and_token():
    uid = str(uuid.uuid4())
    mysql(
        "INSERT INTO users (id, oauth_provider, oauth_id, email, name, notify_language, created_at, updated_at) "
        f"VALUES ('{uid}', 'email', '{uid}@e2e.test', '{uid}@e2e.test', 'e2e', 'en', UTC_TIMESTAMP(), UTC_TIMESTAMP())"
    )
    raw = "sat_" + uuid.uuid4().hex + uuid.uuid4().hex[:8]
    mysql(
        "INSERT INTO personal_tokens (id, user_id, token_hash, token_prefix, name, created_at) "
        f"VALUES ('{uuid.uuid4()}', '{uid}', '{hashlib.sha256(raw.encode()).hexdigest()}', '{raw[:8]}', 'e2e', UTC_TIMESTAMP())"
    )
    return raw


def cli_upload(token, files, one_time):
    boundary = "----e2e" + uuid.uuid4().hex
    parts = []
    for name, content in files:
        parts.append(
            f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n"
            f"Content-Type: application/octet-stream\r\n\r\n".encode() + content + b"\r\n"
        )
    parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"is_one_time\"\r\n\r\n{'true' if one_time else 'false'}\r\n".encode())
    parts.append(f"--{boundary}--\r\n".encode())
    s, _, d = request("POST", "/cli/uploads", {"X-Personal-Token": token, "Content-Type": f"multipart/form-data; boundary={boundary}"}, b"".join(parts))
    assert s == 200, (s, d)
    return json.loads(d)["share_code"]


def file_ids(code):
    return mysql(f"SELECT id FROM file_shares WHERE share_code='{code}' ORDER BY display_order").split()


def race(n, fn):
    with cf.ThreadPoolExecutor(max_workers=n) as pool:
        return list(pool.map(lambda _: fn(), range(n)))


def test_one_time():
    token = make_user_and_token()
    content = os.urandom(200_000)
    st = session_token()

    code = cli_upload(token, [("secret.bin", content)], one_time=True)
    outs = race(10, lambda: request("GET", f"/cli/shares/{code}/download"))
    ok = [o for o in outs if o[0] == 200]
    check("M1 CLI stream: exactly one of 10 concurrent downloads succeeds", len(ok) == 1 and ok[0][2] == content, f"{[o[0] for o in outs]}")
    check("M1 CLI stream: losers get 404", sorted(o[0] for o in outs).count(404) == 9, f"{[o[0] for o in outs]}")

    code = cli_upload(token, [("secret.bin", content)], one_time=True)
    outs = race(10, lambda: request("GET", f"/download?code={code}", {"X-Session-Token": st}))
    ok = [o for o in outs if o[0] == 200]
    check("M1 web /download: exactly one of 10 concurrent downloads succeeds", len(ok) == 1 and ok[0][2] == content, f"{[o[0] for o in outs]}")

    code = cli_upload(token, [("secret.bin", content)], one_time=True)
    fid = file_ids(code)[0]
    outs = race(10, lambda: request("GET", f"/download/file?code={code}&file_id={fid}", {"X-Session-Token": st}))
    check("M1 web /download/file: exactly one success", sum(o[0] == 200 for o in outs) == 1, f"{[o[0] for o in outs]}")

    code = cli_upload(token, [("secret.bin", content)], one_time=True)
    outs = race(10, lambda: request("POST", f"/cli/shares/{code}/download-url"))
    check("M1 CLI download-url: exactly one URL issued", sum(o[0] == 200 for o in outs) == 1, f"{[o[0] for o in outs]}")

    code = cli_upload(token, [("a.bin", b"A" * 1000), ("b.bin", b"B" * 1000)], one_time=True)
    ids = file_ids(code)
    outs = race(6, lambda: request("POST", "/download/bulk", {"X-Session-Token": st}, {"code": code, "file_ids": ids}))
    check("M1 web bulk zip: exactly one archive delivered", sum(o[0] == 200 for o in outs) == 1, f"{[o[0] for o in outs]}")

    code = cli_upload(token, [("a.bin", b"A" * 1000), ("b.bin", b"B" * 1000)], one_time=True)
    ids = file_ids(code)
    s, _, d = request("GET", f"/download/file?code={code}&file_id={ids[0]}", {"X-Session-Token": st})
    reserved = mysql(f"SELECT COUNT(*) FROM share_codes WHERE code='{code}'")
    s2, _, d2 = request("GET", f"/files/list?code={code}", {"X-Session-Token": st})
    remaining = [f["id"] for f in json.loads(d2).get("files", [])] if s2 == 200 else []
    check("M1 multi-file one-time: first file consumed, sibling still listed", s == 200 and d == b"A" * 1000 and remaining == [ids[1]], f"{s} {s2} {remaining}")
    check("Share code stays reserved while a sibling file remains", reserved == "1", reserved)

    code = cli_upload(token, [("public.bin", content)], one_time=False)
    statuses = [request("GET", f"/cli/shares/{code}/download")[0] for _ in range(3)] + \
               [request("GET", f"/download?code={code}", {"X-Session-Token": st})[0] for _ in range(2)]
    check("Regression: normal (non-one-time) share downloads repeatedly", statuses == [200] * 5, f"{statuses}")


if __name__ == "__main__":
    for t in (test_headers, test_oauth, test_email, test_rate_limits_and_swagger, test_one_time):
        try:
            t()
        except Exception as e:  # report and keep going
            check(f"{t.__name__} crashed", False, repr(e))
    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)}/{len(results)} checks passed")
    sys.exit(1 if failed else 0)
