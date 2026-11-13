# Mash dashboard

`mash dash` serves this Svelte application and a read-only API on loopback.
Build with `pnpm install --frozen-lockfile && pnpm run build`, then rebuild Mash
to embed the bundle. For frontend development, start `mash dash` and run
`pnpm dev`; Vite forwards `/api` to port 8790 with the correct Host header.

The dashboard reads real local and shared silver through the typed application
core. Search uses the existing combined search operation. It never indexes,
downloads models, or writes history. Shared catalogue and transcript pages need
the shared service built from this version; an unavailable service produces a
visible partial-history warning rather than hiding local chats.

## Refresh and reading behavior

- TanStack Query shares the catalogue between screens. One Server-Sent Events
  stream per tab (`/api/events`, `src/lib/live.ts`) says when the local index
  changed; the catalogue then rereads itself, at most once every five seconds. Counts describe
  indexed chats, not machine connectivity. The newest 10,000 chats are retained;
  a warning identifies incomplete counts and filter choices at that bound.
- Search runs on submission. Editing filters does not change existing results.
  Empty searches filter a captured catalogue snapshot, including project and
  date. Nonempty searches use the backend's bounded result window in 20-chat
  pages. Each new page is a fresh backend read; it is not a database snapshot
  across concurrent indexing.
- Routes retain machine identity and use the search result's canonical lookup
  anchor. Search matches open a bounded context view, respecting branch ancestry.
  Reading from the beginning uses 100-position pages in recorded display order.
- The displayed transcript page rereads itself when its chat changes. New
  messages are appended only while the reader is at the bottom of the chat;
  elsewhere a button offers them, and another applies changes to text already
  shown. Nothing on screen changes or moves by itself.
- Markdown passes through DOMPurify. Raw scripts, styles and remote images are
  excluded. Messages over 16,384 characters initially show a preview. Tool inputs
  remain inert text. Markdown export explicitly exports only the displayed page.

## Verification

Run `pnpm run lint`, `pnpm run check`, `pnpm run test`, and `pnpm run build`.
`pnpm run test:browser` additionally exercises the UI against intercepted API
responses in Chromium: refresh, stable reading, explicit search, source routing,
sanitization and error recovery. Install the matching Playwright Chromium binary
explicitly if it is unavailable. On Xenia run browser tests through `agent-test`.
Browser artifacts go to the sibling `mash-artifacts/dashboard-browser` directory.

Rust regressions cover local HTTP guards, catalogue merge/fallback, and parity
between local and shared silver paging. The ignored
`qdrant_dashboard_catalog_and_pages_preserve_group_scope` test verifies the
authenticated shared endpoints against a disposable Qdrant-backed service.
These are read-only additions: publication, writer admission, acknowledgement,
crash recovery and certification ordering are unchanged, so no protocol model
change is needed.

Remaining dashboard work is tracked in `mash-twg6`: revision/ETag-based refresh
instead of polling bodies, whole-chat change detection, native branch/subagent
navigation, richer tool/code presentation, and complete-chat exports.
