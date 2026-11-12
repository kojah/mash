# Documentation website

The documentation at [getmash.io](https://getmash.io) uses Astro and Starlight,
published as static files on GitHub Pages. Cloudflare manages the domain's DNS.
Internal links use Astro's client router to change pages without a full reload.
Browser back/forward, direct URLs, and ordinary links without JavaScript still
work. Search stays mounted across navigation, preserving its query and shortcut.

## Editing content

Edit the Markdown guides in `docs/`. The homepage is `docs/home.mdx`, and the
quick start comes from the repository README. There is no second maintained
copy: `site/scripts/content.ts` prepares ignored build inputs and rewrites
relative Markdown links for the website. Links to repository source stay on
GitHub. Each generated guide has an edit link to its original file.

Add a guide to `docs/README.md` and, when appropriate, the sidebar in
`site/astro.config.mjs`. New Markdown guides are included in builds and search
automatically. Only top-level `docs/*.md` files are published. Keep engineering
and maintenance notes in `docs/internal/`, outside the website and its search.
Re-run the development command after editing source guides.

## Local preview and checks

From `site/`, with Node 24 and pnpm installed:

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Before publishing, run `pnpm lint`, `pnpm check`, `pnpm test`, and `pnpm build`.
Then run `pnpm test:browser` (install Chromium with
`pnpm exec playwright install chromium` on first use).
On Xenia, run builds and tests through `agent-test`. The static output goes to
`site/dist/`; Starlight builds the Pagefind search index during the build.

## Publishing

The Documentation workflow validates pull requests and publishes changes on
`main`. It can also be dispatched manually. A completed Release workflow
rebuilds the site so newly published apt packages are included.

The release workflow owns the signed apt repository on the `gh-pages` branch.
Documentation builds copy that branch's `apt/` directory into the Pages artifact
without modifying it. The documentation workflow never rewrites that branch.
If fetching an existing apt branch fails, publishing fails rather than dropping
packages. Before the first apt release, publishing only the documentation is
expected.

GitHub Pages must use GitHub Actions as its publishing source and `getmash.io`
as its custom domain. DNS points the domain at GitHub Pages; no Cloudflare token
is required in GitHub Actions. Enable HTTPS enforcement once GitHub has issued
the domain certificate.

## The dashboard demo

`getmash.io/demo/` is the dashboard itself, built with `pnpm build:demo` in
`dashboard/` and copied into the site by `pnpm add-demo` in `site/`. The
documentation workflow runs both, so the demo is republished whenever the
dashboard changes.

The demo sends no requests. Its entry point (`dashboard/src/demo/main.ts`)
installs a transport that answers in the browser from a made-up history
(`dashboard/src/demo/history.ts`), in the shapes `mash dash` sends. Nothing
outside `dashboard/src/demo/` knows the demo exists, and the real build
carries none of it.

- **Keeping it honest.** Every demo answer is typed against the dashboard's
  API contract (see below), so a route or field the server changes stops
  `backend.ts` compiling until the demo follows. `backend.test.ts` also passes
  each answer through the dashboard's own response checks.
- **Search.** The demo matches words only and says so on the results page.
- **The tool list.** `dashboard/src/demo/tools.json` is a copy of what a
  running dashboard serves at `/api/console/tools`. Refresh it with
  `curl -s http://127.0.0.1:8790/api/console/tools > dashboard/src/demo/tools.json`.
- **History.** Every project, machine and conversation is invented. Never add
  real history to it.

## The dashboard's API contract

`mash dash` describes its own API. Its routes are registered with `aide`
(`app/dash.rs`, `app/dash/services.rs`), which builds the router and an
OpenAPI 3.1 document together from the handlers' extractors and the Rust types
each route answers with. A running dashboard serves the document at
`/api/openapi.json`.

- `dashboard/openapi.json` is the committed copy. The Rust test
  `committed_contract_matches_the_routes` fails when it is out of date;
  rewrite it by running that test with `MASH_WRITE_CONTRACT=1`.
- `dashboard/src/lib/contract.ts` holds the TypeScript types generated from
  it with `pnpm contract` in `dashboard/`. A dashboard test fails when they
  are out of date.
- `read()` in `dashboard/src/lib/api.ts` takes its path and answer type from
  those types. The hand-written checks that follow still validate what they
  use, in case a dashboard and its server are from different builds.
- A type that answers a route needs `schemars::JsonSchema`. Types the MCP
  tools also publish must keep their schema names: renaming one changes the
  MCP contract.
- `aide` is pinned to a pre-release (`0.16.0-alpha.4`), the first line that
  reads `schemars` 1 schemas. Move to the stable release when there is one.

The shared server is described the same way. Its handlers already take and
return typed values, so `aide` reads everything from them. The result is kept
as a reference at `docs/internal/shared-server-openapi.json`; the Rust test
`committed_server_contract_matches_the_routes` fails when it is out of date
and rewrites it with `MASH_WRITE_CONTRACT=1`. The synchronization route
upgrades to a WebSocket, and its frames are outside what the document
describes.

Several types share a short name, such as `Request` and `Scope`, and the MCP
tools publish them under those names. `app/openapi.rs` gives each a distinct
name inside the generated documents without renaming the types.
