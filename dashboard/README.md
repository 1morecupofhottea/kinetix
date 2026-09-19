# Kinetix Dashboard

The admin dashboard for Kinetix: a hand-drawn-styled React 19 + Vite + Tailwind 4
single-page app. Every view is backed by the real Kinetix admin API
(`/admin/api/*`) — there is no mock data.

## Development

The dev server proxies API calls to a locally running Kinetix on
`http://127.0.0.1:8080`:

```bash
npm install
npm run dev      # http://localhost:3000
```

Run the Kinetix server separately (`cargo run` in the repo root).

## Production build

The production bundle is written to `dashboard/dist`, which `rust-embed` bakes
into the Kinetix binary at compile time. Build the dashboard **before**
compiling the Rust binary:

```bash
npm install
npm run build
cd .. && cargo build --release
```

The helper script `../scripts/build-dashboard.sh` does both steps.

## Layout

- `src/lib/api.ts` — fetch wrapper (cookie-based admin session).
- `src/lib/resources.ts` — typed calls to the admin API.
- `src/lib/mappers.ts` — admin API JSON → dashboard view model.
- `src/components/` — navbar, login, live tester, hand-drawn primitives.
- `src/components/views/` — one view per dashboard tab.

Authentication uses the Kinetix admin session cookie, obtained from the
`KINETIX_ADMIN_TOKEN` configured on the server. Cloudflare Access (when
configured) is validated server-side before the session cookie is issued.
