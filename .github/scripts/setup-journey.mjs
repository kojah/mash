import { createHash } from 'node:crypto';
import { cp, mkdir, readFile, readdir, writeFile, appendFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

// Only run on disposable hosted runners. Local validation uses explicit temporary roots.
const local = process.argv.includes('--prepare-only');
const journey = process.env.UX_JOURNEY ?? 'setup';
if (!['setup', 'records'].includes(journey)) throw new Error('Unknown journey');
if (!local && process.env.GITHUB_ACTIONS !== 'true') {
  throw new Error('Live preparation requires a disposable GitHub Actions runner.');
}
const temporary = process.env.RUNNER_TEMP;
if (!temporary) throw new Error('RUNNER_TEMP is required.');
const work = join(temporary, 'setup-user');
const report = join(temporary, 'setup-report');
const home = local ? join(temporary, 'fixture-home') : homedir();
await mkdir(work, { recursive: true });
await mkdir(report, { recursive: true });
await mkdir(join(work, 'docs'), { recursive: true });
await mkdir(join(report, 'user-docs', 'docs'), { recursive: true });
await mkdir(join(work, 'assets'), { recursive: true });
await mkdir(join(report, 'user-docs', 'assets'), { recursive: true });
const files = ['README.md', 'assets/mash-logo.png', ...(await readdir('docs')).filter(n => n.endsWith('.md')).map(n => `docs/${n}`)];
const documents = {};
for (const file of files) {
  const content = await readFile(file);
  documents[file] = createHash('sha256').update(content).digest('hex');
  await cp(file, join(work, file));
  await cp(file, join(report, 'user-docs', file));
}
const history = join(home, '.claude', 'projects', '-work-log-service');
await mkdir(history, { recursive: true });
const timestamp = new Date().toISOString();
const chat = 'setup-journey-synthetic';
const messages = [
  { type: 'user', sessionId: chat, uuid: 'setup-user-message', cwd: join(home, 'work', 'log-service'), timestamp,
    message: { role: 'user', content: 'How should we rotate the application logs for the harbor service?' } },
  { type: 'assistant', sessionId: chat, uuid: 'setup-assistant-message', parentUuid: 'setup-user-message', timestamp,
    message: { role: 'assistant', content: [{ type: 'text', text: 'Rotate the harbor service logs daily and retain seven compressed archives. We chose the harbor-lantern policy to keep disk use predictable.' }] } },
];
if (journey === 'records') {
  messages.push({ type: 'user', sessionId: chat, uuid: 'records-user-message', parentUuid: 'setup-assistant-message', timestamp,
    cwd: join(home, 'work', 'log-service'),
    message: { role: 'user', content: 'I accept the harbor-lantern policy: rotate logs daily and retain seven compressed archives. I prefer concise incident reports with commands I can copy. The harbor staging service currently uses port 8080. We still need to decide who owns the weekend rotation review.' } });
}
await writeFile(join(history, `${chat}.jsonl`), messages.map(m => JSON.stringify(m)).join('\n') + '\n', { flag: 'wx' });
await writeFile(join(report, 'metadata.json'), JSON.stringify({
  revision: process.env.GITHUB_SHA ?? 'local-validation',
  platform: process.env.UX_PLATFORM ?? process.platform,
  architecture: process.arch,
  tester: process.env.UX_TESTER ?? 'local',
  journey,
  candidate: journey === 'records' ? await candidateMetadata() : null,
  run: process.env.GITHUB_RUN_ID ?? null,
  attempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
  started: timestamp,
  documents,
  fixture: 'synthetic Claude history, no real user data',
  isolation: 'disposable hosted VM; source-free starting directory, not a filesystem sandbox',
}, null, 2));
const prompt = await readFile(`.github/ux/${journey === 'records' ? 'records' : 'setup'}-tester.md`, 'utf8');
await writeFile(join(report, 'prompt.md'), `${prompt}\n\nYour platform: ${process.env.UX_PLATFORM ?? process.platform}.\nStarting directory: ${resolve(work)}\nJournal: ${resolve(join(report, 'journal.md'))}\n`);
if (!local) {
  if (!process.env.GITHUB_ENV) throw new Error('GITHUB_ENV is required.');
  await appendFile(process.env.GITHUB_ENV, `UX_WORK=${work}\nUX_REPORT=${report}\n`);
}
console.log(`Prepared user docs and synthetic history; reports: ${report}`);

async function candidateMetadata() {
  const root = join(temporary, 'candidate');
  const revision = (await readFile(join(root, 'revision.txt'), 'utf8')).trim();
  if (process.env.GITHUB_SHA && revision !== process.env.GITHUB_SHA) {
    throw new Error('Candidate revision mismatch');
  }
  const suffix = process.platform === 'win32' ? '.exe' : '';
  const binaries = {};
  for (const name of ['mash', 'mash-server']) {
    binaries[name] = createHash('sha256').update(await readFile(join(root, name + suffix))).digest('hex');
  }
  return { revision, binaries, build: 'development; not release timing evidence' };
}
