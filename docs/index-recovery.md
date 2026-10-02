# Recovering incomplete indexing

An `incomplete` indexing result means the attempt did not establish a complete
snapshot. The CLI exits with status 9; MCP returns `isError: true` and structured
partial counts. A partial first index remains unavailable for search until a
clean explicit `mash index` finishes. Previously completed, unchanged local data
can remain searchable. Shared publication failures can instead report
`shared-pending`; check that state separately from malformed input.

For malformed Claude Code or Codex JSONL, inspect `record_diagnostics` in the
final result. Each sample identifies a provider, source path, one-based physical
line, byte column and category. Blank lines count toward the physical line.
Columns count UTF-8 bytes, so an editor's character column may differ. Samples
cover the first rejected record in each of at most 32 sources; later failures
still contribute to `malformed_records`, and additional sources contribute to
`omitted_sources`. A sample is a starting point, not a list of every damaged line.

1. Preserve the original source and a private backup before attempting recovery.
   Stop the provider writing to that source before restoring or repairing it.
   Keep histories out of public bug reports; share the bounded diagnostics
   instead of record text or decoder messages.
2. Check the category. `syntax` and `unexpected-eof` can indicate interrupted or
   damaged writes. `data` can indicate an unsupported provider format or a
   mistyped field; preserve a private reproduction and investigate the adapter
   before changing valid upstream data. A complete line terminator alone does
   not establish valid JSON.
3. Prefer an intact provider backup or another provably identical native copy.
   A matching message body, attachment, or suffix does not prove record identity,
   ancestry or timestamps. Do not guess missing fields or delete a rejected
   record merely to make indexing pass. Mash does not repair provider histories.
4. Run `mash index` explicitly after restoring a complete source. Inspect the
   terminal state and diagnostics again: fixing the first sample may reveal
   another failure. Do not delete the index or its checkpoints as a recovery
   shortcut. Healthy sources can advance during an incomplete attempt; malformed
   JSONL sources retain their last complete indexed contribution and checkpoint
   until a clean retry succeeds.

Watch mode can observe later source changes, but it does not turn a corrupt
source into a complete snapshot. Do not interpret partial counts, progress, or
an old searchable contribution as proof that the latest source was admitted.

## Real-corpus diagnostic control, 2026-10-03

The open recovery issue `mash-san` concerns one Claude JSONL source with a
1,125-byte rejected physical line. Its final 944 bytes exactly match a later
valid attachment record; there are 181 bytes before that suffix. A read-only
scan of the current local Claude projects covered 533 JSONL files, 194,349 lines
and 525,996,067 bytes. It found one other line starting with that complete
181-byte prefix: a valid 600-byte attachment record in the same source.

The prefix contains complete `parentUuid`, `isSidechain` and `attachment` fields,
all equal to the valid candidate. It then ends inside the next field name,
before the original UUID and timestamp. This supports a partially interrupted
record followed by another record, but does not prove the missing identity or
metadata. Neither replacement with the candidate nor deletion of the prefix is
established as lossless. The native source remains unchanged and the recovery
issue remains open.

The private, content-free audit script and JSON evidence are stored outside Git
as `claude-corrupt-prefix-audit-20261003.{py,json}` under the project artifact
directory. The audit checks each scanned file's identity, size and modification
metadata before and after its read, checks the inventory, and confirms the
diagnosed source bytes remain identical after the scan. This is evidence for
the current local project files only; it does not inspect remote nodes,
provider backups or deleted records. It is not a successful indexing or
real-corpus latency result.
