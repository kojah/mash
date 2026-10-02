// Offline first-party helper capture; does not run a model or a VS Code session.
import { strict as assert } from "node:assert";
import { createHash } from "node:crypto";
import { createReadStream, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const COMMIT = "3435f72fcf4cb843bee946b8f9e981683564c9e3";
const [checkoutArg, dependenciesArg, outputArg] = Bun.argv.slice(2);
assert(checkoutArg && dependenciesArg && outputArg, "provide checkout, isolated dependencies, fresh output directory");
const checkout = resolve(checkoutArg);
const dependencies = resolve(dependenciesArg);
const output = resolve(outputArg);
const sha256 = (bytes: string | Buffer) => createHash("sha256").update(bytes).digest("hex");
const runtimeHash = createHash("sha256");
for await (const chunk of createReadStream(process.execPath)) runtimeHash.update(chunk);
const runtime_sha256 = runtimeHash.digest("hex");
function packageHash(root: string): string {
    const files: [string, string][] = [];
    function visit(directory: string) {
        for (const entry of readdirSync(directory, { withFileTypes: true })) {
            const path = join(directory, entry.name);
            if (entry.isDirectory()) visit(path);
            else if (entry.isFile()) files.push([path.slice(root.length + 1), sha256(readFileSync(path))]);
            else throw new Error("unexpected dependency filesystem entry");
        }
    }
    visit(root);
    files.sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0);
    return sha256(JSON.stringify(files));
}
const dependency_sha256 = Object.fromEntries(["zod", "nanoid"].map(name => [name, packageHash(join(dependencies, "node_modules", name))]));
const git = Bun.spawnSync(["git", "-C", checkout, "rev-parse", "HEAD"]);
assert.equal(git.exitCode, 0);
assert.equal(git.stdout.toString().trim(), COMMIT);
const clean = Bun.spawnSync(["git", "-C", checkout, "status", "--porcelain"]);
assert.equal(clean.exitCode, 0);
assert.equal(clean.stdout.toString(), "", "upstream checkout must be unmodified");
assert.equal(Bun.version, "1.4.2");
for (const [name, version] of [["zod", "4.3.6"], ["nanoid", "5.1.7"]]) {
    const metadata = JSON.parse(readFileSync(join(dependencies, "node_modules", name, "package.json"), "utf8"));
    assert.equal(metadata.version, version);
}
mkdirSync(output, { recursive: false });
process.chdir(output);

// Resolve only the upstream exports required by these helpers, without building
// unrelated provider/model/extension entry points. These are original modules,
// not substitute implementations. The catalog adapter below is a test double.
await Bun.plugin({
    name: "pinned-cline-helper-imports",
    setup(builder) {
        builder.onResolve({ filter: /^@cline\/shared$/ }, () => ({ path: join(checkout, "sdk/packages/shared/src/prompt/format.ts") }));
        builder.onResolve({ filter: /^@cline\/shared\/storage$/ }, () => ({ path: join(checkout, "sdk/packages/shared/src/storage/index.ts") }));
        builder.onResolve({ filter: /^(zod|nanoid)$/ }, ({ path }) => ({ path: Bun.resolveSync(path, dependencies) }));
    },
});
const migration = await import(join(checkout, "apps/vscode/src/sdk/legacy-task-handling.ts"));
const { SessionManifestStore } = await import(join(checkout, "sdk/packages/core/src/session/stores/session-manifest-store.ts"));
const { createSessionCompactionState, projectSessionCompactionState } = await import(join(checkout, "sdk/packages/core/src/session/models/session-compaction.ts"));
const { formatUserInputBlock } = await import(join(checkout, "sdk/packages/shared/src/prompt/format.ts"));

type Message = { role: "user" | "assistant"; content: string; id?: string; ts?: number };
type Ui = { ts: number; type: "say"; say: "task" | "text" | "user_feedback"; text: string };
const originals = [
    "Configure the shared search service.", "Use an authenticated HTTP service.",
    "Preserve archived conversation metadata.", "The catalog keeps project and provider metadata.",
    "Verify node-group isolation.", "Each node group has an independent index.",
];
const legacy: Ui[] = originals.map((text, index) => ({ ts: 1700000000000 + index, type: "say", say: index === 0 ? "task" : index % 2 === 0 ? "user_feedback" : "text", text }));
const oldApi: Message[] = originals.map((content, index) => ({ role: index % 2 === 0 ? "user" : "assistant", content }));
const imported: Message[] = migration.legacyApiHistoryToSdkMessages(oldApi, { id: "task-1", conversationHistoryDeletedRange: [2, 3] });
assert(!JSON.stringify(imported).includes(originals[2]));
const suffix: Message[] = [
    { role: "user", content: formatUserInputBlock("Run the Qdrant recovery checks again.") },
    { role: "assistant", content: "The acknowledged snapshots survived a database process restart." },
    { role: "user", content: formatUserInputBlock("Run the Qdrant recovery checks again.") },
    { role: "assistant", content: "Stale revisions were rejected after recovery." },
];
const canonical: Message[] = [...imported, ...suffix];
// Exercise the actual legacy UI merge with controlled display messages. SDK-to-UI
// translation itself is outside this control's scope and is not substituted here.
const sdkDisplay: Ui[] = canonical.map((message, index) => ({ ts: 1700000010000 + index, type: "say", say: message.role === "user" ? "user_feedback" : "text", text: message.content }));
const merged: Ui[] = migration.mergeLegacyUiMessagesWithResumedSdkMessages(legacy, sdkDisplay);
assert.equal(merged.length, legacy.length + suffix.length);
assert.deepEqual(merged.slice(0, legacy.length), legacy);
const expected = [...originals, "Run the Qdrant recovery checks again.", suffix[1].content, "Run the Qdrant recovery checks again.", suffix[3].content];
const sources = [
    "apps/vscode/src/sdk/legacy-task-handling.ts",
    "apps/vscode/src/sdk/initial-message-sanitizer.ts",
    "sdk/packages/core/src/session/stores/session-manifest-store.ts",
    "sdk/packages/core/src/services/session-data.ts",
    "sdk/packages/core/src/services/session-artifacts.ts",
    "sdk/packages/core/src/session/models/session-compaction.ts",
    "sdk/packages/core/src/session/models/session-manifest.ts",
    "sdk/packages/shared/src/prompt/format.ts",
];
const hashes: Record<string, string> = {};
for (const phase of ["initial", "compacted", "regenerated"] as const) {
    const root = join(output, phase);
    const ui = join(root, "legacy", "tasks", "task-1");
    mkdirSync(ui, { recursive: true });
    writeFileSync(join(ui, "ui_messages.json"), JSON.stringify(legacy, null, 2) + "\n");
    // Native artifact paths can be relative to the caller's working directory.
    // Keep capture bytes independent of this machine's home/output directory.
    const sdk = join(phase, "sdk");
    mkdirSync(sdk, { recursive: true });
    const rows = new Map();
    const store = new SessionManifestStore({
        ensureSessionsDir: () => sdk,
        getSession: async (id: string) => rows.get(id),
        upsertSession: async (row: { sessionId: string }) => { rows.set(row.sessionId, row); },
    });
    const manifestPath = store.artifacts.sessionManifestPath("task-1");
    store.writeSessionManifest(manifestPath, {
        version: 1, session_id: "task-1", source: "vscode", pid: 1,
        started_at: "2023-11-14T22:13:20.000Z", status: "completed", interactive: true,
        provider: "offline-control", model: "not-called", cwd: "/work/cline-control", workspace_root: "/work/cline-control",
        enable_tools: false, enable_spawn: false, enable_teams: false,
    });
    const messages = phase === "regenerated" ? canonical.map((message, index) => ({ ...message, id: `regenerated-${index}`, ts: 1700000090000 + index })) : canonical;
    await store.persistSessionMessages("task-1", messages);
    const persistedPath = store.artifacts.sessionMessagesPath("task-1");
    const before = readFileSync(persistedPath);
    if (phase !== "initial") {
        const state = createSessionCompactionState({ sourceMessages: messages, compactedMessages: [{ role: "user", content: "compaction-only-canary" }], updatedAt: "2026-09-30T10:00:00.000Z" });
        await store.persistSessionCompactionState("task-1", state);
        const restored = await store.readSessionCompactionState("task-1");
        assert.deepEqual(projectSessionCompactionState(restored, messages), state.messages);
        assert.deepEqual(readFileSync(persistedPath), before, "compaction must not rewrite canonical history");
    }
    for (const file of [join(ui, "ui_messages.json"), manifestPath, persistedPath, ...(phase === "initial" ? [] : [store.artifacts.sessionCompactionPath("task-1")])]) {
        hashes[resolve(file).slice(output.length + 1)] = sha256(readFileSync(file));
    }
}
writeFileSync(join(output, "capture.json"), JSON.stringify({
    scope: "cline-first-party-helper-control-v1", commit: COMMIT, bun: Bun.version,
    runtime_sha256, dependency_sha256,
    dependencies: { zod: "4.3.6", nanoid: "5.1.7" },
    catalog_adapter: "in-memory test double; manifest fallback uses upstream code",
    sdk_display: "controlled input; full SDK-to-UI translator not executed",
    compaction_input: "fixed offline projection; no inference executed",
    source_sha256: Object.fromEntries(sources.map(path => [path, sha256(readFileSync(join(checkout, path)))])),
    artifact_sha256: hashes, expected_contents: expected,
}, null, 2) + "\n");
console.log(JSON.stringify({ output, phases: 3, visible_messages: expected.length, artifacts: Object.keys(hashes).length }));
