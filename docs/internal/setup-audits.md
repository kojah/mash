# Native first-user setup audits

Run `gh workflow run setup-journeys.yml --ref main` to start one independent
Copilot tester on each of Linux, macOS, and Windows. This is manual-only;
there is no scheduled model spend or automatic issue creation.

The macOS tester uses the macOS 26 Apple Silicon image. A job that never acquires
a runner is missing coverage, not a setup failure; inspect GitHub's allocation
annotations and retry when hosted capacity is available.

Add `COPILOT_GITHUB_TOKEN` to the repository's Actions secrets first. Use a
fine-grained token with Account permissions → Copilot Requests: Read, rather
than a repository write token. Never put it in a workflow, prompt, or artifact.

Each tester receives the same outcome and user docs at the triggering revision,
synthetic recent Claude history, and a separate disposable hosted VM. It must
follow the docs, record commands and friction, and stop at success or a bounded
impasse. It is allowed to discover that an installer or prerequisite is missing.
It must not fix implementation code or silently substitute an undocumented setup.

These are documentation-guided journeys, separate from the offline, source-blind
CLI exploration profile in `.agents/ux-acceptance-testing/PROFILE.md`.
Hosted runners are developer images, not factory-fresh consumer machines. They
may already include compilers or package managers and may lack Docker Desktop.
The starting directory contains only public guides, but the native job is not a
filesystem or network sandbox. No personal histories or production credentials
are available. Copilot's inference credential is supplied only to its execution
step. The job token has read-only contents access and checkout credentials are
not persisted. Test agents can install software on their disposable VMs.

The native jobs use pinned Copilot CLI 1.0.106. Each has a 45-minute job limit,
a 35-minute agent step limit, and a prompt instructing it to finish exploration
within 25 minutes. Three jobs can run at once. Interrupted jobs retain available
evidence via an `always()` upload; cancellation before preparation may leave none.

On completion, `setup-journey-review.md` runs as a GitHub Agentic Workflow on
Linux and reviews the three transcripts. Findings have one observer per OS;
the review must not claim independent reproduction on the same platform.
GitHub Agentic Workflows cannot host
their firewall sandbox on native macOS or Windows, so standard Actions owns
native execution and gh-aw owns synthesis. Missing evidence and authentication
failures are never counted as successful setup or as Mash defects.

Download `setup-PLATFORM-TESTER` artifacts from the native run for metadata,
the journal, CLI session, console output, and agent exit code. The review uploads
`setup-review.md` through its artifact safe output. Retention is 14 days. Reports
contain synthetic history only; never expand uploads to the runner's home or
Copilot configuration directories. A successful workflow is not a usability pass:
read the journey outcomes and ranked findings.

Each saved journal ends with an evidence-linked findings table ranked P0–P3:
critical harm or concealed failure, blocked journey, meaningful friction, then
minor polish. Rankings include an impact rationale; uncertainty and runner
limitations are recorded separately. The reviewer uses the same rubric and
explains any severity changes. Missing coverage cannot establish a clean audit.

The reviewer writes only `setup-review.md` in its workspace. The pinned gh-aw
1.0.1 upload handler requires a workspace source and a relative allowed-path
filter; an absolute path under `/tmp/gh-aw/agent` is rejected by this version.

To repeat synthesis for a completed run:

```sh
gh workflow run setup-journey-review.lock.yml --ref main -f run_id=RUN_ID
```

Edit the Markdown workflow, then compile with `gh aw compile setup-journey-review`
(currently gh-aw 1.0.1). Commit the generated lock file with the source. Never
hand-edit the compiled workflow. Validate native workflow syntax with actionlint
and the preparer with `node --check`; `--prepare-only` with a disposable
`RUNNER_TEMP` validates fixtures without writing the local user's home.
