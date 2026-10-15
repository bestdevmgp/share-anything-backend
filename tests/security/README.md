# Security-patch verification harness

Reproducible local verification for the `security-patch` branch. Three layers:

1. **Unit tests** — no services required.
2. **DB integration tests** — real MySQL 8.0; verify the SQL-level claims
   (atomic one-time claim, conditional email-session transitions, cleanup).
3. **HTTP E2E** (`e2e_security.py`) — drives a running server and asserts the
   fixed behaviors. Run it against the **patched** binary (all pass) and the
   **pre-patch** binary from `main` (many fail) to see the before/after.

> Everything here uses throwaway databases/credentials. Never point it at a
> real database or a production server.

---

## 1. Unit tests (no DB)

```bash
cargo test
```
Includes the OAuth redirect-allowlist tests (`redirect_allowlist_tests`) and the
existing zip/filename/scope tests.

## 2. DB integration tests (MySQL 8.0)

These are `#[ignore]` by default so they never run without a database. Each test
creates and drops its own `sa_sec_test_<uuid>` database, so nothing else is
touched.

```bash
# start a throwaway MySQL (example: Docker)
docker run --rm -d --name sa-mysql -e MYSQL_ALLOW_EMPTY_PASSWORD=1 -p 3306:3306 mysql:8.0

# a user that can create/drop sa_sec_test_* databases is enough:
#   CREATE USER 'sa_test'@'%' IDENTIFIED BY 'sa_test_pw';
#   GRANT ALL PRIVILEGES ON `sa_sec_test_%`.* TO 'sa_test'@'%';
#   GRANT CREATE, DROP ON *.* TO 'sa_test'@'%';

TEST_DATABASE_URL=mysql://sa_test:sa_test_pw@127.0.0.1:3306 cargo test -- --ignored
```
`TEST_DATABASE_URL` is a **server URL with no database path**. Covered:
`delete_file_share` exactly-once claim, 16-thread concurrent claim single-winner,
share-code released only after the last file, conditional email transitions,
concurrent email completion single-winner, expired-session purge.

## 3. HTTP E2E (`e2e_security.py`, stdlib only)

Brings the whole stack up and asserts the C1/C2/M1/H3/M3/L1/L2 behaviors
end-to-end. Requires a MySQL with the app schema and an S3-compatible endpoint
(the mock [`moto`](https://github.com/getmoto/moto) works well).

### 3a. App database (fresh)
```bash
# apply migrations in filename order on a fresh DB
mysql -uroot share_anything_e2e < <(cat $(ls migrations/*.sql | sort))
# NOTE: email_auth_sessions has no migration file in this repo (created out of
# band in production). Create it for the E2E:
mysql -uroot share_anything_e2e -e "
CREATE TABLE IF NOT EXISTS email_auth_sessions (
  id VARCHAR(36) PRIMARY KEY, email VARCHAR(255) NOT NULL, token VARCHAR(64) NOT NULL,
  verification_code VARCHAR(6) NOT NULL, status VARCHAR(20) NOT NULL,
  request_ip VARCHAR(64) NOT NULL, device_id VARCHAR(64) NOT NULL DEFAULT '',
  expires_at DATETIME NOT NULL, created_at DATETIME NOT NULL) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;"
# the users.oauth_provider enum must include 'email' for the E2E user inserts:
mysql -uroot share_anything_e2e -e "ALTER TABLE users MODIFY oauth_provider ENUM('google','naver','apple','kakao','email') NOT NULL;"
```

### 3b. S3 mock
```bash
pip install 'moto[server]'
moto_server -H 127.0.0.1 -p 5001 &
python - <<'PY'
import boto3
boto3.client('s3', endpoint_url='http://localhost:5001',
             aws_access_key_id='test', aws_secret_access_key='test',
             region_name='us-east-1').create_bucket(Bucket='sa-e2e')
PY
```

### 3c. Run the patched server
Set a complete environment (see `src/config.rs` for the full list). The security-
relevant ones for the E2E:
```bash
export SERVER_HOST=127.0.0.1 SERVER_PORT=18080
export BASE_URL=http://127.0.0.1:18080 FRONTEND_URL=https://shareany.test
export CORS_ALLOWED_ORIGINS=https://shareany.test,http://localhost:3000
export DATABASE_URL=mysql://<user>:<pass>@127.0.0.1:3306/share_anything_e2e
export S3_ENDPOINT=http://localhost:5001 S3_REGION=us-east-1 S3_BUCKET_NAME=sa-e2e \
       S3_ACCESS_KEY_ID=test S3_SECRET_ACCESS_KEY=test S3_PREFIX=
export JWT_SECRET=e2e-jwt-secret SESSION_TOKEN_JWT_SECRET=e2e-session-secret
export ADMIN_PASSWORD=e2e-admin-password SWAGGER_USERNAME=sw SWAGGER_PASSWORD=swpw
# OAuth + TURN vars can be dummy values (no real calls in the E2E):
export GOOGLE_CLIENT_ID=x GOOGLE_CLIENT_SECRET=x GOOGLE_REDIRECT_URI=http://127.0.0.1:18080/auth/google/callback
export NAVER_CLIENT_ID=x NAVER_CLIENT_SECRET=x NAVER_REDIRECT_URI=http://127.0.0.1:18080/auth/naver/callback
export KAKAO_CLIENT_ID=x KAKAO_CLIENT_SECRET=x KAKAO_REDIRECT_URI=http://127.0.0.1:18080/auth/kakao/callback
export APPLE_CLIENT_ID=x APPLE_TEAM_ID=x APPLE_KEY_ID=x APPLE_PRIVATE_KEY=x APPLE_REDIRECT_URI=http://127.0.0.1:18080/auth/apple/callback
export CLOUDFLARE_TURN_KEY_ID=x CLOUDFLARE_TURN_API_TOKEN=x TURNSTILE_SECRET_KEY=x

cargo build && ./target/debug/share-anything &
```

### 3d. Run the checks
```bash
SESSION_SECRET="$SESSION_TOKEN_JWT_SECRET" \
ADMIN_PASSWORD="$ADMIN_PASSWORD" \
MYSQL="mysql -uroot share_anything_e2e" \
BASE=http://127.0.0.1:18080 \
python3 -I tests/security/e2e_security.py
```
Expected on `security-patch`: **all checks pass** (exit 0).

### 3e. Before/after
Build the binary from `main` into a separate target dir and point the same
running environment at it:
```bash
git worktree add /tmp/sa-main origin/main
( cd /tmp/sa-main && CARGO_TARGET_DIR=/tmp/sa-main-target cargo build )
# stop the patched server, start /tmp/sa-main-target/debug/share-anything with
# the same env, then re-run 3d.
```
On `main` a large subset of the checks **fail** — each failure corresponds to a
vulnerability this PR closes (the script prints `PASS`/`FAIL` per check).

The session-token the script mints itself is signed with `SESSION_SECRET` and is
only accepted because the E2E uses that same `SESSION_TOKEN_JWT_SECRET`.
