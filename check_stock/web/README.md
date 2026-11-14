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

| Path                 | Content                                                                                                                                                                                 |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/api/client.ts`  | `fetch` wrapper: same origin, CSRF header on changing requests, `ApiError` with the server's code and message, 401 → back to the login                                                  |
| `src/api/queries.ts` | TanStack Query hooks per endpoint                                                                                                                                                       |
| `src/api/generated/` | TypeScript types generated from the Rust structs by `cargo test` (ts-rs). **Never edit**: change the Rust struct, run `cargo test`, commit the result. CI fails if they are out of date |
| `src/router.tsx`     | Pages (TanStack Router); everything but `/login` is inside the app layout, which sends visitors without a session to the login                                                          |
| `src/components/`    | Layout, permission picker, dialog, error message                                                                                                                                        |
| `src/pages/`         | Login, start page, users, audit log, account                                                                                                                                            |
| `src/test/`          | Fake server for `fetch`, fixtures, `renderApp` (whole app at a path)                                                                                                                    |

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
