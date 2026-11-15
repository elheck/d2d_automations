# check_stock_server

Web backend of the [MTG Stock Checker](../README.md), for use on the home LAN.
All logic and data access come from [`check_stock_core`](../core/); this crate
adds the HTTP layer, logins and permissions, the server's own database tables
and admin commands.

Status: foundation done (steps 1a–1c: server, login, users with
fine-grained permissions, audit log, and the [web UI](../web/) built into the
binary); ported screens: the singles listing (2a), the buy helper (2b),
precon EV (2c) and the reports restock (3a), price movers (3b),
mispricing (3c) and stock analysis (3d).

## Try it locally

```bash
cd check_stock
./dev.sh              # web UI on http://localhost:5173 (hot reload), server on 127.0.0.1:8080
./dev.sh --release    # UI built into a release server: http://127.0.0.1:8080
./dev.sh reset-data   # fresh copy of the desktop data; your login stays
./dev.sh help
```

The first start copies the desktop app's `inventory.db` into
`check_stock/dev-data/` (gitignored) and asks for a password for your local
login (user name: your Linux user, or `DEV_ADMIN`). The desktop file is only
read, never changed, and the desktop app keeps working alongside.

The API can also be used with `curl`:

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
    [--read-connections 4] [--create-empty] \
    [--inventory-sync-url http://cardscanner.local:3000]
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
| `--inventory-sync-url` | `CHECK_STOCK_INVENTORY_SYNC_URL` | The inventory_sync price tracker (Cardmarket prices). `http`/`https` with a host only: no user name, path, query or fragment. Without it pages show no Cardmarket prices. `dev.sh` passes `http://cardscanner.local:3000` (`DEV_INVENTORY_SYNC_URL`, empty = none). |
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
| `GET /api/singles/lookup?input=&default_set=` | Singles listing | the card for `hou120` (or `120` + default set): Scryfall data, Cardmarket link, latest inventory_sync prices (`prices.status`: `found`, `no_cardmarket_id`, `not_tracked`, `not_configured`, `unavailable`) |
| `GET /api/singles/image/{set}/{number}` | Singles listing | the card image (JPEG/PNG), browser-cacheable for 7 days (`private`) |
| `POST /api/buy-helper/offer` | Buy Helper | `{csv, params}` → summary (cards, value, offer for singles and bulk) and the 200 most valuable singles. The card list is parsed in memory and never stored |
| `POST /api/buy-helper/export` | Buy Helper | same body → the offer as `buy_offer.csv` (download) |
| `GET` / `PUT /api/buy-helper/settings` | Buy Helper | the user's own split rules and rates (`params`) plus the `defaults`; `PUT` stores them (audit entry only when they changed) |
| `GET /api/precon-ev/status` | Precon EV | whether a tracker is configured, the `load` and `sales` jobs (`idle`/`running` with stage and counts/`finished`/`failed`/`cancelled`), and what is loaded (`version` changes with every load and when the sales history arrives) |
| `POST /api/precon-ev/load` | Precon EV | `{refresh_index}` → starts a load for everyone (202 with the status; also while one runs). 429 `too_soon` with `Retry-After` within 30 s after the last load ended, 409 `not_configured` without `--inventory-sync-url` |
| `GET /api/precon-ev/ranking?field=&sealed_field=&cutoff=&foil_needs_sales=&sort=&desc=&filter=&hide_no_decklist=` | Precon EV | one row per precon (EV, ratio, Liquid EV, Top 5, coverage, notes), sorted and filtered by core; 409 `not_loaded` before the first load |
| `GET /api/precon-ev/products/{id}?field=&sealed_field=&cutoff=&foil_needs_sales=` | Precon EV | the product's figures and every card line (rank, Top 5, unit and line price, sales days, note) per deck |
| `GET /api/precon-ev/products/{id}/history?…` | Precon EV | weekly EV, sealed price and Top 5 since the release (at most 52 weeks) |
| `GET /api/data-info` | logged in | how current the inventory data is: `copy_taken_at` (when the desktop copy was taken) and `last_import` (day of the last inventory import) |
| `GET /api/restock?min_copies=&sort=&desc=` | View reports | sold-out variants ranked by sell-through speed (first 300; `total`, `copies`); `realized_revenue` / `revenue` only with "View revenue and costs", and `sort=revenue` is refused (403) without it |
| `GET /api/restock/export?…` | View reports | the same list, all rows, as `restock_buy_list.csv` (the `realizedRevenue` column only with "View revenue and costs"; spreadsheet-safe text) |
| `GET /api/movers/status` | View reports | the shared market data job and what is loaded (`day`, `cards`, `products`, `version`) |
| `POST /api/movers/load` | View reports | `{refresh}` → loads today's market data for everyone (202 with the status; does nothing when today's data is there unless `refresh`); 429 `too_soon` within 30 s after the last load ended |
| `GET /api/movers?field=&min_price=&min_age=&direction=&sort=&desc=` | View reports | cards in stock whose market price moved over 7/30 days (first 300 of `total`); 409 `not_loaded` before the first load |
| `GET /api/mispricing/status`, `POST /api/mispricing/load` | Mispricing report | the shared market data, as `/api/movers/status` and `/load` |
| `GET /api/mispricing?field=&threshold=&filter=&sort=&desc=` | Mispricing report | every listing against the market (verdict, flags, momentum, action, 7/30-day change; first 300 of `total`), counts over all listings, the internal consistency conflicts (first 100 of `conflicts_total`); the money totals (`upside`, `excess`, `listed_value`, `market_value`) only with "View revenue and costs" |
| `GET /api/stock-analysis` | View reports | stock overview, sales velocity, dead-stock aging, top lists, longest unsold, breakdowns and the purchase-lot table; `money` says whether the money figures (stock value, revenue, value per age bucket, lot value/revenue/cost/margin/payback, bulk-lot revenue) are in the answer: only with "View revenue and costs". Listing prices of single cards are always shown |
| `GET` / `PUT /api/precon-ev/settings` | Precon EV | the user's price columns, cutoff, foil option, sorting and "hide without decklist" plus the `defaults` |

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

## Web UI

`build.rs` embeds `check_stock/web/dist` (from `npm run build`) into the
binary; every path outside `/api` is the UI (`src/web.rs`): embedded files are
served as they are (`assets/…` with content hashes cached for a year,
everything else revalidated), any other path gets `index.html` for the
browser-side router. Lookups are exact matches in the embedded list, never
file system paths. Without a build (desktop release builds, CI without Node)
the server shows a short hint page instead.

The UI's TypeScript types for the API are generated from the Rust structs
(`#[derive(TS)]`, ts-rs) into `web/src/api/generated/` by `cargo test`
(configured in `check_stock/.cargo/config.toml`); CI fails if the committed
files are out of date.

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
  unknown JSON fields are refused; bodies are limited to 64 KiB, except the
  buy helper's card list upload (offer and export routes only): at most 6 MB
  of CSV, 50,000 rows, quantities and prices up to 1,000,000; checked only
  after the session, so anonymous callers cannot make the server read a large
  body. Errors about a file name the line, never the file's content.
- **Money figures** (revenue, margins, lot costs) are only sent to users
  with "View revenue and costs": without it the server leaves them out of
  answers and CSV files and refuses sorting by them (the order would give
  them away). The pages only hide what the server already left out.
- **Spreadsheet-safe reports:** text from an uploaded file that could start a
  spreadsheet formula (`=`, `+`, `-`, `@`, also full-width or after spaces) is
  exported with a leading `'` (`check_stock_core::spreadsheet`). Only for
  reports people open; CSVs that are imported somewhere again keep their
  values exactly.
- **Audit log:** logins (also failed ones, with address and reason), logouts,
  password changes, every user change with its before/after, imports. Written
  in the same transaction as the change.
- **One process per data folder.** `serve`, `import-desktop`, `create-admin`
  and `reset-password` hold an OS file lock on `<data-dir>/.lock`.
- **Database:** one writer connection (foreign keys on) plus read-only
  readers (SQLite read-only flag and `query_only`), WAL mode, 5 s busy
  timeout. All database work and password hashing run on blocking threads.
  Ctrl-C / SIGTERM finish the requests in flight before stopping.
- **Background jobs** (`src/jobs.rs`): long work (the precon load and its
  sales history) runs on the server once for all users, one run per kind at a
  time; pages poll its status. A replaced run is cancelled and can no longer
  change the status. On Ctrl-C / SIGTERM running jobs are cancelled and the
  process waits at most 3 s for them.
- **Market data** (`src/market.rs`): latest prices and the prices 0/7/14/30/60/90
  days back for every card in stock, loaded from inventory_sync once a day
  for all users (price movers, mispricing). The history is fetched in
  portions of 1,000 products (the tracker holds one database lock per
  request; one request for the whole stock took over 30 s and was sometimes
  cut off), each retried once, with progress; errors are shortened like the
  precon ones.
- **Precon EV** (`src/precon.rs`): MTGJSON only at its fixed address (deck
  file names checked by core), inventory_sync only at the configured one. A
  new load is refused within 30 s after the last load ended; weekly histories are
  fetched one at a time and the last 20 products are kept (until the next
  load). Load errors shown to users are cut to their first part ("Catalog
  fetch failed."); the full text with addresses only goes to the log, and a
  failed decklist download shows no details. Decklists and the sealed index
  are cached in `<data-dir>/cache/mtgjson_v1/`, the daily sales stats in
  `<data-dir>/cache/precon_sales/` (the desktop app's layout).
- **Scryfall** (`src/cards.rs`): the browser never contacts Scryfall; the
  server looks a printing up once and keeps it in `<data-dir>/cache/`
  (`scryfall_cache.json`, `images/`, the desktop app's cache layout). Set
  codes and collector numbers are only letters, digits and `-` (they end up
  in a URL and a file name). Images only from `https://cards.scryfall.io`,
  without following redirects, at most 4 MiB, and only JPEG/PNG data is kept
  and served. API requests to Scryfall are spaced 100 ms apart for the whole
  server. Unknown printings are not cached, so made-up input cannot fill the
  disk; a Scryfall or inventory_sync failure never shows their answer to the
  client.
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
| `srv_user_settings` | Per-user settings of the screens (`src/user_settings.rs`): one JSON value (≤ 16 KiB) per user and setting, deleted with the user; read back through each screen's validation, defaults if it no longer fits |

## Development

```bash
cd check_stock
cargo test -p check_stock_server
./run_quality_checks.sh    # whole workspace: desktop app, core, server
```
