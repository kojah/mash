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

- **Keeping it honest.** `dashboard/src/demo/backend.test.ts` passes every
  demo answer through the dashboard's own response checks. When a response
  shape changes, update `backend.ts` until that test passes.
- **Search.** The demo matches words only and says so on the results page.
- **The tool list.** `dashboard/src/demo/tools.json` is a copy of what a
  running dashboard serves at `/api/console/tools`. Refresh it with
  `curl -s http://127.0.0.1:8790/api/console/tools > dashboard/src/demo/tools.json`.
- **History.** Every project, machine and conversation is invented. Never add
  real history to it.
