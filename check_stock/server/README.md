# check_stock_server

Web backend of the [MTG Stock Checker](../README.md), for use on the home LAN.
All logic and data access come from [`check_stock_core`](../core/); this crate
adds the HTTP layer, logins and permissions, the server's own database tables
and admin commands.

Status: foundation done up to step 1b: server, login, users with
fine-grained permissions, audit log. The web UI follows in step 1c; until then
the API can be tried with `curl` (see below).

## Try it locally

```bash
cd check_stock
./dev.sh              # http://127.0.0.1:8080, on a copy of the desktop DB
./dev.sh reset-data   # fresh copy of the desktop data; your login stays
./dev.sh help
```

The first start copies the desktop app's `inventory.db` into
`check_stock/dev-data/` (gitignored) and asks for a password for your local
login (user name: your Linux user, or `DEV_ADMIN`). The desktop file is only
read, never changed, and the desktop app keeps working alongside.

Logging in with `curl`:

```bash
curl -i -X POST http://127.0.0.1:8080/api/auth/login \
  -H 'Content-Type: application/json' -H 'X-Check-Stock-CSRF: 1' \
  -d '{"username":"aron","password":"…"}'
# then pass the session cookie from the response:
curl http://127.0.0.1:8080/api/auth/me -H 'Cookie: __Host-check_stock_session=…'
```

## Commands

```bash
check_stock_server serve --data-dir <DIR> [--bind 127.0.0.1:8080] \
    [--allowed-host stock.example.com] [--trusted-proxy 172.18.0.0/16] \
    [--read-connections 4] [--create-empty]
check_stock_server import-desktop --data-dir <DIR> --db <desktop inventory.db> [--refresh | --replace]
check_stock_server create-admin --data-dir <DIR> --username <NAME> [--display-name <NAME>] [--if-none]
check_stock_server reset-password --data-dir <DIR> --username <NAME>
```

| Option | Environment variable | Meaning |
|---|---|---|
| `--data-dir` | `CHECK_STOCK_DATA_DIR` | Database, backups, caches |
| `--bind` | `CHECK_STOCK_BIND` | Listen address; default `127.0.0.1:8080`. Any other address makes the server reachable from other machines. |
| `--allowed-host` | `CHECK_STOCK_ALLOWED_HOSTS` (comma-separated) | Extra host names to answer; `localhost`, `127.0.0.1`, `[::1]` always work |
| `--trusted-proxy` | `CHECK_STOCK_TRUSTED_PROXIES` (comma-separated) | Proxies (Caddy) whose `X-Forwarded-For` names the real client, as IPs or CIDR ranges. Without it the header is ignored. |
| `--create-empty` | | Start with an empty database if none exists (otherwise the server refuses, so a forgotten import is noticed) |
| `--refresh` | | `import-desktop`: swap only the desktop data of an existing database; users, sessions and the audit log stay |
| `--replace` | | `import-desktop`: replace the whole database file |
| `--if-none` | | `create-admin`: do nothing if any user exists |

`--refresh` and `--replace` keep the previous database as
`inventory.db.replaced-<time>.bak` (the newest 3 are kept).

`create-admin` and `reset-password` need the server to be stopped. They ask
for the password on the terminal (twice, without echo) or read one line from
standard input; passwords are never command-line arguments.

## API

| Route | Access | |
|---|---|---|
| `GET /api/health` | public | `{"status":"ok"}` if the database answers |
| `POST /api/auth/login` | public | `{username, password}` → session cookie + current user |
| `POST /api/auth/logout` | logged in | ends this session |
| `GET /api/auth/me` | logged in | user name, display name, permissions |
| `POST /api/auth/password` | logged in | `{current_password, new_password}`; ends all other sessions |
| `GET /api/permissions` | logged in | every permission with label, description, group and what it requires (for the user administration) |
| `GET /api/users`, `POST /api/users` | Manage users | list / create |
| `PATCH /api/users/{id}`, `DELETE /api/users/{id}` | Manage users | change display name, permissions, disabled, unlock / delete |
| `POST /api/users/{id}/password` | Manage users | set a password (unlocks, ends the user's sessions) |
| `GET /api/audit?before_id=&limit=` | View audit log | newest first |

State-changing requests (anything but `GET`) need the header
`X-Check-Stock-CSRF: 1`. Errors are always
`{"error": {"code": "...", "message": "..."}}`.

Every route is registered in `src/api/mod.rs` together with its access rule;
there is no other way to add one. A test walks all routes and checks that
each one refuses anonymous callers and users without the permission.

### Permissions

20 permissions in groups (Administration, Inventory, Daily work, Reports and
money, Pricing, Bulk lots, Tools), defined with label and description in
[`src/permissions.rs`](src/permissions.rs). Some need others (e.g. *Export new
prices* needs *View repricing*); the server refuses inconsistent sets. A
change applies to the user's next request, without a new login.

The server keeps at least one enabled user with *Manage users*: removing that
permission from the last admin, or disabling or deleting them, is refused.
Nobody can disable or delete their own account.

## Safety

- **Loopback by default.** Other addresses must be configured explicitly and
  log a warning.
- **Host allowlist.** A request for any host name not on the list gets a 400,
  so a web page that points its own domain at `127.0.0.1` (DNS rebinding)
  cannot use the server through your browser.
- **Passwords:** Argon2id (19 MiB, 2 passes, OWASP), random salt, at least 12
  characters, at most 1024 bytes, not the user name.
- **Login:** the same error for unknown users, wrong passwords and locked or
  disabled accounts, and the same time for unknown users (a dummy hash is
  checked). 5 wrong passwords lock the account for 15 minutes; at most 10
  login attempts per client address per 5 minutes. The audit log records the
  real reason.
- **Sessions:** 32 random bytes in a `__Host-…; HttpOnly; Secure;
  SameSite=Strict` cookie; the database stores only their SHA-256. Ended after
  8 hours without requests and at the latest 7 days after login; also on
  logout, password change, disabling the account and a password reset.
- **CSRF:** the custom header (pages of other sites cannot set it) plus checks
  of `Origin` and `Sec-Fetch-Site`, on top of the `SameSite=Strict` cookie.
- **Security headers** on every response, including errors: strict CSP (no
  inline code, no framing), `nosniff`, `no-referrer`, same-origin isolation,
  `Cache-Control: no-store`. HSTS comes from Caddy, which terminates TLS.
- **No internal details for clients.** Database errors, paths and panic
  messages go only to the log; malformed request bodies get a generic message;
  unknown JSON fields are refused; bodies are limited to 64 KiB.
- **Audit log:** logins (also failed ones, with address and reason), logouts,
  password changes, every user change with its before/after, imports. Written
  in the same transaction as the change.
- **One process per data folder.** `serve`, `import-desktop`, `create-admin`
  and `reset-password` hold an OS file lock on `<data-dir>/.lock`.
- **Database:** one writer connection (foreign keys on) plus read-only
  readers (SQLite read-only flag and `query_only`), WAL mode, 5 s busy
  timeout. All database work and password hashing run on blocking threads.
  Ctrl-C / SIGTERM finish the requests in flight before stopping.
- **Import:** the desktop file is opened read-only; the copy is taken with
  SQLite's backup API, integrity-checked, migrated, and only then put in
  place. A damaged or foreign file never becomes the server database.

## Database

The server uses the same SQLite file as the inventory data (`inventory.db`),
so a change and its audit entry commit together. Core's tables are migrated
by core exactly as in the desktop app. The server's own tables all start with
`srv_` and have their own version (`srv_migrations`); a database written by a
newer server version is refused.

| Table | Content |
|---|---|
| `srv_migrations` | Applied server schema versions |
| `srv_meta` | Server settings, e.g. where and when the desktop copy was taken |
| `srv_users` | Accounts: login name, display name, password hash, disabled, lockout |
| `srv_user_permissions` | Granted permissions per user |
| `srv_sessions` | Sessions: SHA-256 of the token, user, last activity, end |
| `srv_audit_log` | Who did what and when |

## Development

```bash
cd check_stock
cargo test -p check_stock_server
./run_quality_checks.sh    # whole workspace: desktop app, core, server
```
