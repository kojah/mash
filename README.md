# mash

mash (Multi-Agent Search History) is a small, JSON-only search CLI for Claude Code and Codex JSONL
histories. SQLite is the canonical store; SQLite FTS5, local FastEmbed
embeddings, reciprocal-rank fusion, and bounded cross-encoder reranking provide
hybrid retrieval. mash can also federate searches and views across configured
machines over SSH.

## Commands

```text
mash index [--full] [--provider claude-code|codex]... [--since-days N|--all-history]
mash search <query> [--node NAME]... [--limit N] [--provider PROVIDER] [--days N]
mash view <message-id> [--node NAME] [--context N]
mash status
mash forget <conversation-id>
mash models install
```

All operational commands emit one JSON value. Bare `mash` prints help. Global
options are `--config PATH`, `--local-node NAME`, `--db PATH`, and
`--models-dir PATH`.

## Models and indexing

Search is semantic by default and has no production lexical-only mode. Install
models explicitly, then build the local index:

```bash
mash models install
mash index
```

No other command downloads models. Indexing fingerprints sources and messages,
skips unchanged input, updates FTS incrementally for small changes, embeds only
new or changed searchable messages, and excludes raw tool output from FTS and
embeddings while preserving it for `view`. `--full` rebuilds derived search
state. The default source horizon is 90 days; `--since-days N` overrides it and
`--all-history` removes it for one run.

On Apple Silicon, indexing runs the FP32 embedding model through Core ML and
quantizes only the resulting vectors for storage. Other targets use the
quantized ONNX CPU model. After upgrading an existing Mac installation, rerun
`mash models install` once to install the FP32 asset and compile its Core ML
cache.

Without a configuration file, mash indexes these built-in roots when present:

- Claude Code: `~/.claude/projects`, `~/.config/claude/projects`
- Codex: `~/.codex/sessions`, `~/.local/share/codex/sessions`

## Configuration

The rename changes the default configuration and data directories to `mash`,
and the database filename to `mash.sqlite3`. To reuse an existing achi installation,
pass its paths with `--config`, `--db`, and `--models-dir`, or move its configuration,
database, and installed models into the new locations reported by `mash status`.
Federated nodes must install `mash` at `~/.local/bin/mash`.

mash reads `config.json` from the platform mash configuration directory. Use
`mash status` to see the resolved path, or pass `--config PATH` explicitly.
The file is strict, versioned JSON. Provider presence enables that provider for
the node; roots must be absolute. `since_days` defaults to 90 and may be `null`
for all history.

```json
{
  "version": 1,
  "local_node": "xenia",
  "nodes": [
    {
      "name": "xenia",
      "ssh": "xenia",
      "search": true,
      "providers": {
        "claude-code": {"roots": ["/home/james/.claude/projects"]},
        "codex": {"roots": ["/home/james/.codex/sessions"]}
      },
      "index": {"since_days": 90}
    },
    {
      "name": "dev-macbook",
      "ssh": "dev-macbook",
      "search": true,
      "providers": {
        "claude-code": {"roots": ["/Users/jko/.claude/projects"]},
        "codex": {"roots": ["/Users/jko/.codex/sessions"]}
      }
    }
  ]
}
```

The configured `local_node` selects this machine's roots and indexing horizon;
`--local-node` overrides that identity. CLI indexing flags override the local
node settings. A search without `--node` contacts every other node with
`"search": true`; repeated `--node NAME` values replace that default set and
may explicitly select a node whose `search` value is false. Logical node names
are reported in results while `ssh` supplies the transport destination.

There are no provider or federation environment-variable overrides. There is
also no TUI, export, daemon, remote sync, watch mode, analytics platform,
provider registry, compatibility layer, or alternate output encoding.

## Verification

```bash
cargo fmt --check
cargo nextest run
cargo clippy --all-targets -- -D warnings
cargo test --doc
```

The real-model integration test is ignored by default and requires
`MASH_TEST_MODELS_DIR` pointing at assets created by `mash models install`.

The Veritas test annotations come from the private `jamesqo/veritas`
repository at the exact revision recorded in `Cargo.toml`. Contributors running
tests need repository access and a GitHub SSH identity that can clone Veritas.

## Automatic deployment

Every push to `main` builds the Linux x86-64 and Apple Silicon binaries in
GitHub Actions, then atomically deploys `mash` to `~/.local/bin` on Xenia,
dev-macbook, and personal-macbook over Tailscale. The macOS binary is
cross-compiled on the Linux runner with `cargo-zigbuild`; destination machines
need neither this repository nor a Rust toolchain.

Deployment requires `TS_OAUTH_CLIENT_ID`, `TS_OAUTH_SECRET`,
`DEPLOY_SSH_PRIVATE_KEY`, and `DEPLOY_SSH_KNOWN_HOSTS` repository secrets.

## License

See [LICENSE](LICENSE).
