# check_stock_server

Web backend of the [MTG Stock Checker](../README.md), for use on the home LAN.
All logic and data access come from [`check_stock_core`](../core/); this crate
adds the HTTP layer, logins and permissions, the server's own database tables
and admin commands.

Status: foundation done (steps 1a–1c: server, login, users with
fine-grained permissions, audit log, and the [web UI](../web/) built into the
binary); ported screens: the singles listing (2a), the buy helper (2b),
precon EV (2c), the reports restock (3a), price movers (3b),
mispricing (3c), stock analysis (3d) and bin analysis with move lists (3e),
the inventory import with the start page's digest (4a), and Search with
discards, the Stock Checker and Picking (4b).

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
check_stock_server restore-inventory --data-dir <DIR> --backup <inventory.db.import-….bak>
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

Every inventory import on the server first saves the database as
`inventory.db.import-<time>.bak` (the newest 10 are kept); without that
backup nothing is imported. `restore-inventory` puts the inventory data of
such a backup back (users, settings, the audit log and the desktop-copy
details stay; the database as it was is kept as a `replaced` backup first)
and logs it as `inventory.restored`. Use it after a wrong import: importing
the right file afterwards brings the stock back, but the copies the wrong
file counted as sold would stay in the sales history.

`create-admin`, `reset-password` and `restore-inventory` need the server to
be stopped. They ask
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
| `GET /api/data-info` | logged in | how current the inventory data is: `copy_taken_at` (when the desktop copy was taken), `last_import` (day of the last inventory import), `inventory_version` + `updated_at` (change with every import, restore or desktop copy; the pages poll them every minute and offer the new data), `test_copy` (a desktop copy before cutover) |
| `POST /api/digest` | logged in | records today's visit and returns what changed since the user's previous visit day (as the desktop's welcome line): sales, new listings, bulk lots and restock candidates only with "View reports", revenue only with "View revenue and costs" too; the day of the last import for everyone |
| `POST /api/inventory/preview` | Import inventory | `{file_name, csv}` (at most 16 MB, 200,000 rows) → what the import would change: new / updated / gone listings, copies counted as sold, stock before → after, price changes; `blocked` (more than half the stock counted as sold), `unusual` (more than 3× the usual sales for the days since the last import, at least 10 copies), the usual pace, until when it can be confirmed (30 minutes). Keeps the check (file fingerprint, category, inventory version, numbers), not the rows |
| `POST /api/inventory/confirm` | Import inventory | the same file again (+ `confirm_sold` for a blocked one: the copies counted as sold, typed in) → backup, then sync, version, kept file and audit entry in one transaction. 409 `no_preview` / `preview_expired` / `other_file` / `outdated` (another change of the category since the check) / `changed` (the numbers moved), 422 `confirmation_needed` / `confirmation_wrong`, 503 `backup_failed` |
| `DELETE /api/inventory/pending` | Import inventory | drops the user's check |
| `GET /api/bins?min_free=&sort=` | View reports | every bin of the last import's file with cards, piles and free slots (bins with at least `min_free`, `sort=free` or `location`), rows without a bin location; `source: null` before the first import on the server. `/api/bins/export?…` the same as CSV (spreadsheet-safe) |
| `GET /api/bins/fragmented` | View reports | cards (variant) spread over several bins (first 200 of `total`) and what gathering them takes |
| `GET /api/bins/consolidation?threshold=` | View reports | what emptying the bins with at most `threshold` (1–60) cards takes: moves, cards, bins freed, walking distance; 409 `no_import` |
| `GET` / `POST` / `DELETE /api/bins/list` | Bin moves | the user's move list (`null` when none; `outdated` after a newer import: no moves until it is made again); `POST {kind, threshold?}` makes it (again): ticks of moves that are still in it stay (`carried_ticks` / `dropped_ticks`); 422 `no_moves` / `too_many_moves` (over 5,000) |
| `PUT /api/bins/list/ticks` | Bin moves | `{ids, done}`: marks moves as moved (ids of the current list only; 409 when outdated) |
| `GET /api/bins/list/export` | Bin moves | the ticked piles with their new location as a Cardmarket stock-update CSV (exact values: Cardmarket imports it); 409 when outdated, 422 `nothing_ticked` |
| `GET /api/search?q=&fields=&case_sensitive=&all_languages=&page=&per_page=` | Search cards | piles of the stock (last import minus pending discards) matching the text in the chosen fields (`name,set,condition,language,location,rarity,price,comment`), 25/50/100/200 per page; each pile has an `id` |
| `GET /api/search/history/{product}?foil=` | Search cards | trend-price history (120 days) and 7/30-day change from the tracker, only for products in the stock; 503 when the tracker cannot be reached |
| `POST /api/inventory/discard` | Discard cards | `{items: [{pile, quantity}]}`: writes the copies off without a sale (backup first; refused when a pile or the database has fewer copies, 409 `not_enough` / `stock_changed`); `value` only with "View revenue and costs" |
| `GET /api/inventory/discards`, `GET /api/inventory/discards/{id}/csv` | Discard cards | the last 20 discards (settled by a later import or not) and the stock-update CSV of each of them (negative quantities, exact values) |
| `POST /api/stock-checker/check` | Stock Checker | `{wantslist, language, language_only}` (text, at most 256 KB / 2,000 lines, never stored; deck links refused with 422 `deck_link`): every wanted card with the piles to take, missing copies, unreadable lines |
| `POST /api/stock-checker/output` | Stock Checker | `{format: picking/invoice/summary, discount, groups}`: the text, built from the server's data for the chosen piles |
| `POST /api/stock-checker/update-csv` | Stock Checker | `{items}`: stock-update CSV with negative quantities from the current stock (refused when a pile is gone or too small) |
| `GET` / `POST` / `DELETE /api/picking`, `PUT /api/picking/picked`, `GET /api/picking/image/{set}/{number}` | Stock Checker | the user's picking list (shelf order, `outdated` after an import or discard), ticks, card images |
| `GET /api/bins/image/{set}/{number}` | Bin moves | card images for the move list (same rules and cache as the singles listing's) |
| `GET /api/inventory/imports` | Import inventory | the last 20 changes of the inventory data (import, forced import, restore, desktop copy) with who, when and the numbers |
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
  of CSV, 50,000 rows, quantities and prices up to 1,000,000; and the
  inventory import (preview and confirm only): at most 16 MB of CSV, 200,000
  rows, quantities 1–1,000,000, prices 0–1,000,000, a category name of
  letters and digits. Checked only after the session, so anonymous callers
  cannot make the server read a large body. Errors about a file name the
  line or the card, never other content of the file.
- **Inventory import** (`src/inventory_import.rs`), stricter than the desktop
  app: every import is checked first and needs a confirmation; the
  confirmation uploads the same file again (same SHA-256 and category), is
  refused after 30 minutes, after any other change of that category, or when
  the numbers moved. An import the safety check blocks (more than half the
  stock counted as sold) needs the number of those copies typed in and is
  logged as `inventory.import_forced`; more than 3× the usual sales is a
  warning only. A backup before every import (no backup, no import); sync,
  version row, kept file and audit entry in one transaction. Only the
  confirmed file of the last import per category is kept (for bin analysis,
  3e); a pending check keeps no rows. An import marks the shared market data
  as outdated, so the next report load reads the new stock.
- **The stock of the screens that need every pile** (`src/stock.rs`: search,
  Stock Checker, picking, bin analysis) is the file of the last import on the
  server minus pending discards; each pile has an id (SHA-256 of variant and
  location). The browser only ever sends pile ids and quantities; every value
  of a CSV or text output comes from the server's data.
- **Discards** (`src/discards.rs`, owner-confirmed): a backup before every
  discard (`inventory.db.discard-<time>.bak`, newest 10; no backup, no
  discard); refused when the pile or the database has fewer copies than asked
  for (never quietly fewer); discard, record and audit entry (every pile) in
  one transaction. Until the next import, restore or desktop copy settles
  them, discards are taken off the stock of every screen, and an export that
  still holds them is blocked by the import check (`returning_discards`,
  typed number to override): it was downloaded before the CSV went into
  Cardmarket. The CSV of the newest 20 can be downloaded again.
- **Stock Checker**: no deck links (owner's decision, 2026-11-15: the server
  never fetches a page a user names); wantslists at most 256 KB / 2,000
  lines, quantities up to 10,000, never stored.
- **Bin analysis and move lists** (`src/bins.rs`): only on the file of the
  last import on the server (the inventory tables merge a card's bins). A
  move list stores its settings and the ticked move ids, never card rows:
  the planner is deterministic, so the server plans it again from the same
  file (parsed file and the last 8 plans cached in memory). After a newer
  import the list is outdated: no ticks, no export until it is made again on
  the new file (ticks of the same pile, from and to carry over). So the
  "moved" CSV, which Cardmarket imports, never carries quantities or prices
  of an older export; it keeps its values exactly (no spreadsheet escaping).
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
  password changes, every user change with its before/after, desktop copies,
  inventory imports (file name, fingerprint, numbers, backup), forced
  imports and restores, discards with every pile, move lists made and
  discarded, picking lists made (ticks are work state and not logged). Written in the same transaction as the change.
- **One process per data folder.** `serve`, `import-desktop`, `create-admin`,
  `reset-password` and `restore-inventory` hold an OS file lock on
  `<data-dir>/.lock`.
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
| `srv_inventory_versions` | One row per change of the inventory data and category: import, forced import, restore, desktop copy; who (name kept when the user is deleted), file fingerprint and numbers |
| `srv_pending_imports` | The check of each user's import (one per user, 30 minutes): file SHA-256, category, inventory version, preview numbers; no rows of the file |
| `srv_inventory_files` | The confirmed file of the last import per category (every pile with its own location, which the inventory tables merge), for bin analysis (3e) |
| `srv_move_lists` | One bin move list per user: kind, threshold, the import version it was planned on; deleted with the user |
| `srv_move_list_ticks` | The ticked move ids of each list (16 hex characters of a SHA-256 over card variant, from and to location) |
| `srv_discards` | Discards: who, when, the piles (id, card, location, copies), the stock-update CSV (newest 20), settled by a later import or not |
| `srv_picking_sessions` | One picking list per user: card, location, copies, picked, the stock it was made on; deleted with the user |
| `srv_user_visits` | Last and previous visit day per user, the baseline of the start page's digest; deleted with the user |
| `srv_user_settings` | Per-user settings of the screens (`src/user_settings.rs`): one JSON value (≤ 16 KiB) per user and setting, deleted with the user; read back through each screen's validation, defaults if it no longer fits |

## Development

```bash
cd check_stock
cargo test -p check_stock_server
./run_quality_checks.sh    # whole workspace: desktop app, core, server
```
