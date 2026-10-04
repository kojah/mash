# Credential redaction evaluation

Decision: use a small, offline Mash policy built with the Rust `regex` crate.
Neither evaluated scanner is a production dependency. Provider patterns follow
the recognizable formats documented in [Gitleaks](https://github.com/gitleaks/gitleaks);
Mash adds credential-context and whole-private-key handling. Patterns are locally
authored, not a vendored copy of the Gitleaks rule catalogue.

## Comparison setup

Evaluated `secrets_scanner` **0.2.3**, native/default features disabled, bundled
rules and hardened `ScanConfig::proxy()`; Rusty Hog's scanner crate from commit
**9da595ad204f017cff576ffac5bce7f2bed1be2f**, default builder and
`matches_entropy()` (rule-specific entropy filtering; optional generic entropy
discovery disabled). Rusty Hog has no text replacement API; the harness replaces
its matching ranges. Both engines are initialized once and scan identical input.
No real credentials, private histories, or live credential validation are used.
Recall requires detection ranges to cover every byte of every occurrence of a
fixture's secret. Removing only a prefix does not count as catching a credential.

`fixtures.json`: 20 secret-bearing cases and 12 benign technical cases. Cases
include provider tokens, Unicode prefixes, allow-marker bypasses, NUL bytes,
opaque auth headers, URLs, short passwords, and multiline private-key bodies.
`additional.json`: 12 further secret cases and 8 benign cases, including escaped
quotes, Unicode passwords, JSON headers, JWTs, and code/template expressions.
These are synthetic regression fixtures, not a blind or representative quality
evaluation. Rules were refined during development using the fixtures.

Release measurements on Xenia, one initialization, followed by 100 corpus passes:

| Policy | Initial secrets caught | Benign cases changed | Warm MiB/s | Initialization ms |
| --- | ---: | ---: | ---: | ---: |
| secrets_scanner | 10/20 | 0/12 | 10.18 | 3150.33 |
| Rusty Hog | 1/20 | 0/12 | 39.54 | 93.31 |
| Mash | 20/20 | 0/12 | 105.11 | 3.24 |

Additional cases: Mash 12/12 caught, 0/8 benign cases changed. Timings include
redaction and string allocations, exclude initialization, and are a small
synthetic microbenchmark, not complete indexing throughput or fleet latency.
The long benign code fixture disproportionately contributes bytes; numbers are
not cross-machine performance guarantees. Scanner defaults can be improved with
custom rules; this comparison does not measure their maximum possible recall.

Observed bundled-policy gaps: both scanners leave private-key bodies intact;
Rusty Hog misses current GitHub token fixtures. The bundled secrets_scanner rules
also miss several short credentials and opaque Authorization values. Neither is
sufficient as a default chat-output sanitizer without additional rules.

## Production boundary

- `view`, message search, thread excerpts and thread metadata are masked through
  the shared application core used by CLI, MCP and the local search worker.
- Excerpts detect on the complete source before clipping. They use masking
  characters of equal UTF-8 width to preserve byte and character offsets.
- Shared NDJSON publication masks canonical content, projection and metadata.
  Recipe-specific preparation/fingerprints use the same redacted dependencies.
  Metadata enrichment evidence is omitted when its visible values change.
- Remote embedding request text is masked, including queries. Numeric vectors
  already produced from local canonical history are not retroactively rebuilt.
- Shared HTTP search/view/catalogue responses mask retained legacy content too.
- Local canonical SQLite rows and original provider histories are preserved.
  This is publication/output redaction, not encryption or deletion of local data.
- A publication-policy version changes the destination key. Explicit indexing
  advances a previously acknowledged snapshot's revision before republishing;
  failed uploads retry that revision. Independently routed indexing already
  advances its revision and forces a full replay on a publication-policy change.
- No deployment or historical shared-storage purge happens just by rebuilding.
  Re-index upgraded nodes to
  replace old plaintext publications. Old immutable Qdrant snapshot storage may
  require separate retention/cleanup; read masking is not a deletion guarantee.

Inputs over 2 MiB, more than 4096 detected matches, or an invalid compiled policy
produce a whole-text mask. Inline allow comments cannot disable the policy.
Adjacent/overlapping matches merge; repeated application is stable. No entropy
classifier runs over arbitrary technical identifiers.

Detection is best effort: arbitrary passwords in prose, unknown token formats,
encoded/obfuscated secrets, query-string credentials and unsupported layouts can
be missed. A literal credential-looking example may be masked. This policy does
not detect general PII and does not promise that output contains no secrets.

## Reproduction

Keep downloads/build output outside Git. Clone Rusty Hog at the pinned commit
into an artifact directory, then create a standalone Cargo manifest there with
an empty `[workspace]`, `evaluate.rs` as the absolute `[[bin]].path`, and:

```toml
[dependencies]
secrets_scanner = { version = "=0.2.3", default-features = false }
rusty_hog_scanner = { path = "rusty-hog/crates/rusty-hog-scanner" }
regex = "1.12"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

Run `cargo run --release --manifest-path <artifact>/Cargo.toml --
research/redaction/fixtures.json`, then repeat with `additional.json`.
The harness prints case names, miss counts and aggregate timing, never matched
values. CI regression tests use the same fixtures against the production policy.

Implementation validation: the complete standard suite passed (644 tests);
36 affected tests passed on the final source with isolated RAM-backed temporary
fixtures after disk contention timed out an earlier watcher check. The real local
model MCP contract and a disposable Qdrant 1.19.1 HTTP contract passed separately.
Qdrant's functional fixture used temporary RAM-backed storage after disk-backed
setup exceeded transport timeouts. This is not a storage durability/performance
test. Durable logs and comparison builds remain outside Git in mash-artifacts.

Primary references:
[secrets_scanner API](https://docs.rs/secrets_scanner/0.2.3/secrets_scanner/),
[Rusty Hog scanner](https://github.com/newrelic/rusty-hog/tree/9da595ad204f017cff576ffac5bce7f2bed1be2f/crates/rusty-hog-scanner).
