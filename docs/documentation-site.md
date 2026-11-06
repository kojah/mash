# Documentation website

The documentation at [mashapp.dev](https://mashapp.dev) uses Astro and Starlight,
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
automatically. Re-run the development command after editing source guides.

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

GitHub Pages must use GitHub Actions as its publishing source and `mashapp.dev`
as its custom domain. DNS points the domain at GitHub Pages; no Cloudflare token
is required in GitHub Actions. Enable HTTPS enforcement once GitHub has issued
the domain certificate.
