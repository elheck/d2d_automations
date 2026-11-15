# check_stock web UI

Browser interface of the [MTG Stock Checker](../README.md), served by
[`check_stock_server`](../server/). React + TypeScript, built with Vite.

## Run it

```bash
cd check_stock
./dev.sh             # http://localhost:5173 with hot reload; server on 127.0.0.1:8080
./dev.sh --release   # the UI built into a release server: http://127.0.0.1:8080
```

Needs Node.js 24 (`.nvmrc`), e.g. `sudo snap install node --classic --channel=24/stable`.
`dev.sh` installs the packages (`npm ci`) when needed. During development Vite
forwards `/api` to the Rust server, so pages and API share one origin, as in
production.

## Checks

```bash
cd check_stock/web
npm run check     # format check, type check, lint, tests, build
npm run format    # format with Prettier
npm run test:watch
```

`check_stock/run_quality_checks.sh` and CI run the same checks plus
`npm audit`.

## Layout

| Path                 | Content                                                                                                                                                                                                                                                                                                                                                                                  |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/api/client.ts`  | `fetch` wrapper: same origin, CSRF header on changing requests, `ApiError` with the server's code and message, 401 → back to the login                                                                                                                                                                                                                                                   |
| `src/api/queries.ts` | TanStack Query hooks per endpoint                                                                                                                                                                                                                                                                                                                                                        |
| `src/api/generated/` | TypeScript types generated from the Rust structs by `cargo test` (ts-rs). **Never edit**: change the Rust struct, run `cargo test`, commit the result. CI fails if they are out of date                                                                                                                                                                                                  |
| `src/navigation.ts`  | The sections in their groups (Tools, Reports, Admin) with permission and start-page text; the navigation menus and the start page are built from it                                                                                                                                                                                                                                      |
| `src/router.tsx`     | Pages (TanStack Router); everything but `/login` is inside the app layout, which sends visitors without a session to the login                                                                                                                                                                                                                                                           |
| `src/components/`    | Layout with the grouped navigation, permission picker, dialog, error message, `LineChart` (small SVG chart, no chart library), `DataAsOf` (how current the data of a report is), `InventoryUpdated` (banner after someone else's import)                                                                                                                                                 |
| `src/pages/`         | Login, start page, singles listing (`singles.ts`: input rules and price texts), buy helper (`buyHelper.ts`: settings form, file checks, saving downloads), precon EV ranking and product (`preconEv.ts`: labels, sorting, texts), restock, price movers, mispricing, stock analysis, inventory import (`inventoryImport.ts`: file checks, typed count, texts), users, audit log, account |
| `src/test/`          | Fake server for `fetch`, fixtures, `renderApp` (whole app at a path)                                                                                                                                                                                                                                                                                                                     |

## Rules

- **No inline styles or inline scripts.** The server's Content Security
  Policy only allows files from its own origin; `style={…}` would silently do
  nothing in production (ESLint refuses it). Use classes in `src/styles.css`.
  Vite never inlines assets as `data:` URIs (`assetsInlineLimit: 0`).
- **The server decides.** Navigation and buttons follow the user's
  permissions, but every rule is enforced by the server; the UI only avoids
  offering what would be refused.
- **Permissions come with their descriptions** from `GET /api/permissions`
  (label, description, group, requirements); the user administration shows
  them. Ticking a permission also ticks what it needs.
- **Packages:** `.npmrc` sets `ignore-scripts=true` (no install scripts) and
  `engine-strict=true` (Node 24 only). `package-lock.json` is committed;
  install with `npm ci`. An `overrides` entry pins `minimatch` 11 for
  typescript-eslint (fixes GHSA-g3hv-75wg-m249 in the lint tooling).
- Tablet first: touch targets at least 44 px, no sideways scrolling.
- **Images and links come from the server.** Card images are loaded from the
  server's own `/api/singles/image/…` (CSP `img-src 'self'`; the browser never
  contacts Scryfall). External links are only rendered for the expected site
  (`isCardmarketUrl`) and open with `rel="noopener noreferrer"`.
- **Browser storage only for conveniences** (e.g. the singles listing's
  default set), always inside `try`/`catch`: work state that must survive
  belongs on the server.
- **Settings saved on the server** (e.g. the buy helper's rules) save
  themselves shortly after the last change, right away when the user moves
  to another page, and with a `keepalive` request when the tab is closed or
  reloaded, so no change is lost (`useAutoSave`).
- **Money figures** come only from the server: pages show revenue, margins
  and costs when the answer contains them (the server leaves them out for
  users without "View revenue and costs") and hide the matching columns.
- **Reports** show `DataAsOf` on top (how current the inventory data is).
- **After an import** every open page polls `/api/data-info` once a minute;
  when the inventory version changed, a banner offers "Show new data" (the
  page keeps its figures until then, so a table does not change while
  someone reads it). The importer's own page marks the new version as seen
  (`inventoryVersion.ts`). The market-data panel loads again by itself when
  the server reports `stock_changed`.
- **Inventory import** sends the file twice (check, then confirm); nothing
  of it stays in the browser after the import. A blocked import's button
  only unlocks when the typed number matches (the server checks again).
- **Background jobs** (precon load): the page polls the status about once a
  second while a job runs, and refetches its data when the loaded `version`
  changes.
- **Charts:** `LineChart` draws SVG with classes only (colours as CSS
  variables, light and dark), so the CSP needs no exception; a hidden table
  carries the values for screen readers.
- **Downloads** come from `download()` in `src/api/client.ts` (POST with the
  CSRF header, file name from `Content-Disposition`, letters/digits/`._-`
  only) and are saved with `saveDownload()`; no inline scripts or `data:`
  URLs.
- **Uploads** are read in the browser (`File.text()`) after a size check and
  sent as JSON; the server checks again.
