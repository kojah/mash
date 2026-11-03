# CLI UX acceptance profile

## Product

- Name: `<product>`
- Binary: `<session-local absolute path or supplied artifact>`
- Repository revision command: `<command>`

## Journey selection

- Selector: `<command returning seed, family, and variation>`
- Recent-family source: `<journal metadata location>`
- Families describe outcomes, not commands.

## Session adapter

- Start: `<command receiving unique session ID and repository root>`
- Stop: `<command receiving validated session ID>`
- Required reported fields: `<session, binary, roots, isolation, limits>`
- Allowed session ID pattern: `<pattern>`

The adapter must build or install an opaque product artifact, exclude repository
source and host/user state, disable networking, use disposable state, apply
resource limits, validate every claim, and refuse cleanup of unknown sessions.

## Disposable product state

- Working roots: `<paths>`
- Configuration/state roots: `<paths>`
- Test fixtures: `<paths and semantics>`
- Forbidden host or production state: `<paths/classes>`

## Journals and discovery

- Journal directory: `<repository-relative path>`
- After exploration: `<semantic/source discovery workflow>`
- Performance add-on: `<owning workflow>`
- External issue updates: `<owning workflow and authorization rule>`

## Handoff

- Formal acceptance tests: `<owner>`
- Unit/integration tests: `<owner>`
- Product changes: `<owner>`
