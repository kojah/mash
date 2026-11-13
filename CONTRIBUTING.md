# Contributing

Bug reports, fixes and documentation improvements are welcome.

## Report a problem

Open an [issue](https://github.com/kojah/mash/issues/new/choose) with the Mash
version (`mash --version`), your platform, the command you ran and what
happened. `mash status` output helps for setup and indexing problems.

Your chats are private. Do not paste real conversation text, tokens or
exported history into an issue; describe the shape of the data or use a
made-up example. Report security problems privately, as
[SECURITY.md](SECURITY.md) describes.

## Propose a change

For anything larger than a small fix, open an issue first so the approach can
be agreed before you write it. Mash has a deliberately narrow scope: local
agent chat history, searched through one CLI, one MCP server and one
dashboard. Changes that widen that scope are unlikely to be accepted.

## Build and test

You need the Rust nightly toolchain that `rust-toolchain.toml` selects,
[cargo-nextest](https://nexte.st), and Node.js 24 with pnpm for the dashboard.

```sh
git clone https://github.com/kojah/mash.git
cd mash
cargo build
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo nextest run
```

Dashboard changes:

```sh
cd dashboard
pnpm install --frozen-lockfile
pnpm run lint && pnpm run check && pnpm run test && pnpm run build
```

Documentation lives in `docs/` and is published to
[getmash.io](https://getmash.io); the site is generated from those files, so
edit them rather than anything under `site/src/content`.

## Pull requests

- Keep each pull request to one change, with tests for behaviour it changes.
- Formatting, Clippy and the test suite must pass.
- Update the guide in `docs/` when you change something a user sees.
- Contributions are accepted under the project's [MIT license](LICENSE).
