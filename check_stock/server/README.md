# check_stock_server

Web backend of the [MTG Stock Checker](../README.md), for use on the home LAN.
All logic and data access come from [`check_stock_core`](../core/); this crate
adds the HTTP layer, the server's own database tables and admin commands.

Status: foundation (step 1a). So far the server answers a health check and a
placeholder page; login, users and the web UI follow in the next steps.

## Try it locally

```bash
cd check_stock
./dev.sh              # http://127.0.0.1:8080, on a copy of the desktop DB
./dev.sh reset-data   # fresh copy of the desktop DB
./dev.sh help
```

`dev.sh` copies the desktop app's `inventory.db` into `check_stock/dev-data/`
(gitignored) on first start. The desktop file is only read, never changed, and
the desktop app keeps working alongside.

## Commands

```bash
check_stock_server serve --data-dir <DIR> [--bind 127.0.0.1:8080] \
    [--allowed-host stock.example.com] [--read-connections 4] [--create-empty]
check_stock_server import-desktop --data-dir <DIR> --db <desktop inventory.db> [--replace]
```

| Option | Environment variable | Meaning |
|---|---|---|
| `--data-dir` | `CHECK_STOCK_DATA_DIR` | Database, backups, caches |
| `--bind` | `CHECK_STOCK_BIND` | Listen address; default `127.0.0.1:8080`. Any other address makes the server reachable from other machines. |
| `--allowed-host` | `CHECK_STOCK_ALLOWED_HOSTS` (comma-separated) | Extra host names to answer; `localhost`, `127.0.0.1`, `[::1]` always work |
| `--create-empty` | | Start with an empty database if none exists (otherwise the server refuses, so a forgotten import is noticed) |
| `--replace` | | `import-desktop`: replace an existing database; it is kept as `inventory.db.replaced-<time>.bak` (newest 3 kept) |

## Safety

- **Loopback by default.** Other addresses must be configured explicitly and
  log a warning.
- **Host allowlist.** A request for any host name not on the list gets a 400,
  so a web page that points its own domain at `127.0.0.1` (DNS rebinding)
  cannot use the server through your browser.
- **Security headers** on every response, including errors: strict CSP (no
  inline code, no framing), `nosniff`, `no-referrer`, same-origin isolation,
  `Cache-Control: no-store`. HSTS comes from Caddy, which terminates TLS.
- **No internal details for clients.** Errors are
  `{"error": {"code", "message"}}`; database errors, paths and panic messages
  go only to the log. A panicking handler gives a plain 500.
- **One process per data folder.** `serve` and `import-desktop` hold an OS
  file lock on `<data-dir>/.lock`; a second server or an import into a folder
  in use is refused. The lock is released when the process ends, even after
  a crash.
- **Database:** one writer connection plus read-only readers (SQLite
  read-only flag and `query_only`), WAL mode, 5 s busy timeout. All database
  work runs on blocking threads. Ctrl-C / SIGTERM finish the requests in
  flight before stopping.
- **Import:** the desktop file is opened read-only; the copy is taken with
  SQLite's backup API, integrity-checked, migrated, and only then moved into
  place. A damaged or foreign file never becomes the server database.

## Database

The server uses the same SQLite file as the inventory data (`inventory.db`),
so a change and its audit entry can commit together. Core's tables are
migrated by core exactly as in the desktop app. The server's own tables all
start with `srv_` and have their own version (`srv_migrations`); a database
written by a newer server version is refused.

| Table | Content |
|---|---|
| `srv_migrations` | Applied server schema versions |
| `srv_meta` | Server settings, e.g. where and when the desktop copy was taken |

## Development

```bash
cd check_stock
cargo test -p check_stock_server
./run_quality_checks.sh    # whole workspace: desktop app, core, server
```
