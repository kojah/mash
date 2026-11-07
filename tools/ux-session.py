"""Disposable clean-room sessions for CLI UX acceptance testing.

    python3 tools/ux-session.py select [--seed N]
    python3 tools/ux-session.py start SESSION_ID [--binary PATH]
    python3 tools/ux-session.py stop SESSION_ID

`start` builds (or copies) an opaque mash binary, packages it as a simulated
release, seeds a fresh home with Claude Code, Codex and OpenCode histories and
fake `claude`/`codex` CLIs, and writes SESSION/enter. Testers run every command
through `SESSION/enter 'command'`, which executes it in a bubblewrap sandbox
with no network, no repository or host home, a private /tmp and time and
memory limits. The home directory persists between commands. `stop` deletes
only a directory this script created. The profile in
.agents/ux-acceptance-testing/PROFILE.md describes how agents use this.
Standard library only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import random
import re
import shutil
import sqlite3
import subprocess
import sys
import tarfile
import time
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parent.parent
ROOT = Path("/home/james/scratch/mash-artifacts/ux-sessions")
JOURNALS = Path("/home/james/scratch/mash-artifacts/ux-journals")
SESSION_ID = re.compile(r"^ux-[a-z0-9-]{3,40}$")
MARKER = ".ux-session.json"
HOME = "/home/user"
LIMITS = {"timeout_seconds": 600, "virtual_memory_kib": 8 * 1024 * 1024}

# Outcomes a user wants, never commands. Variations change the situation.
FAMILIES = {
    "record-and-recall": (
        "Mash is installed. Using its MCP interface, save an evidence-backed preference from one of your chats and retrieve it as context for later work. Inspect its source evidence, and make sure repeating the same write does not create another record.",
        "You can recall the preference, inspect its evidence and verify retry behavior, or reach a bounded impasse after 20 minutes.",
        ["You have not used Mash's record tools before."],
    ),
    "record-correction": (
        "Mash is installed. Using its MCP interface and your available chat history, save a fact and an open question, correct the fact, resolve the question, and inspect what changed. Finally remove the fact and check it no longer appears in recall.",
        "You can inspect the history and resolution and verify removal, or reach a bounded impasse after 20 minutes.",
        ["You want changes to remain understandable to a future agent."],
    ),
    "install-and-connect": (
        "You downloaded Mash's installer to ~/Downloads/install.sh. Install Mash and set it up so Claude Code and Codex on this machine can search your past chats.",
        "Mash is installed, and you believe your agents will be able to use it.",
        [
            "You are in a hurry and want the fastest path.",
            "You want to understand what the installer changed on your machine.",
        ],
    ),
    "first-search": (
        "Mash is installed on this machine (it is on PATH). Find the past chat where you worked out how to rotate application logs.",
        "You found that chat and can read what was said in it.",
        [
            "You have never used Mash before.",
            "You only remember it was about 'logs' and happened a few days ago.",
        ],
    ),
    "semantic-upgrade": (
        "Mash is installed. You heard it can find chats by meaning, not only exact words. Get that working, then find the chat about making database writes safe to retry, without using the word 'retry'.",
        "Meaning-based search works, or you know exactly why it cannot here.",
        [
            "This machine may not have internet access.",
            "You want to know what extra disk space or downloads this needs.",
        ],
    ),
    "agent-wiring": (
        "Mash is installed. Make sure both Claude Code and Codex on this machine are connected to Mash, and find out how you would know if they were not.",
        "You are confident each agent is connected, or you know what is missing.",
        [
            "You are not sure whether setup was ever run on this machine.",
            "You want to check it without opening either agent.",
        ],
    ),
    "keep-fresh": (
        "Mash is installed and set up. You just had a new conversation in Claude Code (use ~/bin/agent-say 'your message' to simulate it). Make sure Mash can find that new conversation.",
        "Mash finds the new message.",
        [
            "You want it to stay up to date without thinking about it.",
            "You want to know how stale Mash's view of your history is.",
        ],
    ),
    "settings-and-scope": (
        "Mash is installed and set up. Make it stop including your OpenCode history, and confirm OpenCode chats no longer appear in results.",
        "OpenCode chats no longer appear, or you know why they still do.",
        [
            "You prefer editing configuration over flags.",
            "You want to undo it again afterwards.",
        ],
    ),
    "remove-and-reinstall": (
        "Mash is installed and set up. Remove Mash completely from this machine, including anything it stored, then install it again from ~/Downloads/install.sh and check your history is searchable.",
        "Mash is reinstalled and works, and you know what was left behind.",
        [
            "You are worried about leftover copies of your chat history.",
            "You want to keep your agents' own history untouched.",
        ],
    ),
}

# Seeded history: (provider, chat id, project, days ago, user, assistant).
CHATS = [
    (
        "claude-code",
        "logs-rotation",
        "/home/user/work/ops",
        3,
        "how should we rotate the application logs on the ops box? they fill the disk every week",
        "Use logrotate with daily rotation, keep 14 compressed files, and send SIGHUP to the app after rotation.",
    ),
    (
        "claude-code",
        "pasted-key",
        "/home/user/work/ops",
        1,
        "deploy fails with 401, my token is sk-test-FAKE0000000000000000 can you check the header",
        "The header needs 'Bearer ' before the token. Rotate that key, since it is now in this chat.",
    ),
    (
        "codex",
        "idempotent-writes",
        "/home/user/work/billing",
        6,
        "payments double-charge when the client times out and sends again. how do we stop that?",
        "Give each charge an idempotency key, store it with a unique constraint, and return the stored result on a repeat.",
    ),
    (
        "codex",
        "flaky-test",
        "/home/user/work/billing",
        9,
        "test_refund_window fails about one run in twenty on CI",
        "It reads the wall clock twice across midnight. Freeze time in the test with a fixed clock.",
    ),
    (
        "opencode",
        "css-grid",
        "/home/user/work/site",
        2,
        "the pricing cards overflow on narrow phones",
        "Switch the container to CSS grid with minmax(16rem, 1fr) and let it wrap.",
    ),
    (
        "opencode",
        "release-notes",
        "/home/user/work/site",
        12,
        "draft release notes for 2.3",
        "2.3 adds dark mode, fixes the pricing overflow, and drops Internet Explorer support.",
    ),
]

OPENCODE = """
PRAGMA journal_mode=WAL;
CREATE TABLE project(id TEXT PRIMARY KEY,worktree TEXT,vcs TEXT);
CREATE TABLE session(id TEXT PRIMARY KEY,project_id TEXT,title TEXT,directory TEXT,time_created INTEGER,time_updated INTEGER);
CREATE TABLE message(id TEXT PRIMARY KEY,session_id TEXT,time_created INTEGER,data TEXT);
CREATE TABLE part(id TEXT PRIMARY KEY,message_id TEXT,session_id TEXT,data TEXT);
CREATE TABLE session_message(id TEXT PRIMARY KEY,session_id TEXT,type TEXT,seq INTEGER,time_created INTEGER,data TEXT);
"""

# A stand-in for an agent CLI. `mcp` edits the real agents' user settings
# (~/.claude.json, ~/.codex/config.toml) the way the real CLIs do, so mash's own
# installation checks see them. Plugins come from a marketplace on GitHub, so
# here they fail the way they would on any offline machine.
FAKE_AGENT = r"""#!/usr/bin/env python3
import json, os, re, sys
name = os.path.basename(sys.argv[0])
args = sys.argv[1:]
home = os.path.expanduser("~")
claude = os.path.join(home, ".claude.json")
codex = os.path.join(home, ".codex", "config.toml")

def servers():
    if name == "claude":
        return json.load(open(claude)).get("mcpServers", {}) if os.path.exists(claude) else {}
    text = open(codex).read() if os.path.exists(codex) else ""
    found = {}
    for server, body in re.findall(r"^\[mcp_servers\.([^\]]+)\]\n((?:[^\[].*\n?)*)", text, re.M):
        command = re.search(r'^command = (".*")$', body, re.M)
        arguments = re.search(r"^args = (\[.*\])$", body, re.M)
        found[server] = {"command": json.loads(command.group(1)) if command else None,
                         "args": json.loads(arguments.group(1)) if arguments else []}
    return found

def write(found):
    if name == "claude":
        data = json.load(open(claude)) if os.path.exists(claude) else {}
        data["mcpServers"] = found
        json.dump(data, open(claude, "w"), indent=2)
        return
    text = open(codex).read() if os.path.exists(codex) else ""
    text = re.sub(r"^\[mcp_servers\.[^\]]+\]\n(?:[^\[].*\n?)*", "", text, flags=re.M)
    for server, entry in found.items():
        text += f"[mcp_servers.{server}]\ncommand = {json.dumps(entry['command'])}\nargs = {json.dumps(entry['args'])}\n"
    os.makedirs(os.path.dirname(codex), exist_ok=True)
    open(codex, "w").write(text)

found = servers()
if args[:2] == ["mcp", "get"]:
    if args[2] in found:
        print(json.dumps(found[args[2]], indent=2)); sys.exit(0)
    print(f"No MCP server found with name: {args[2]}", file=sys.stderr); sys.exit(1)
if args[:2] == ["mcp", "add"]:
    rest = args[2:]
    if rest[:2] == ["--scope", "user"]:
        rest = rest[2:]
    if rest[0] in found:
        print(f"MCP server {rest[0]} already exists", file=sys.stderr); sys.exit(1)
    command = rest[2:]
    found[rest[0]] = {"type": "stdio", "command": command[0], "args": command[1:]} if name == "claude" \
        else {"command": command[0], "args": command[1:]}
    write(found); print(f"Added stdio MCP server {rest[0]}"); sys.exit(0)
if args[:2] == ["mcp", "list"]:
    for key, value in found.items():
        print(f"{key}: {value['command']} {' '.join(value['args'])}")
    sys.exit(0)
if args[:2] == ["mcp", "remove"]:
    if found.pop(args[2], None) is None:
        print(f"No MCP server found with name: {args[2]}", file=sys.stderr); sys.exit(1)
    write(found); print(f"Removed MCP server {args[2]}"); sys.exit(0)
if args[:3] == ["plugin", "marketplace", "add"]:
    print(f"Failed to add marketplace {args[3]}: getaddrinfo ENOTFOUND github.com", file=sys.stderr); sys.exit(1)
if args[:1] == ["plugin"] and args[1:2] in (["install"], ["add"], ["uninstall"], ["remove"]):
    print(f"Plugin {args[2]} not found in any configured marketplace", file=sys.stderr); sys.exit(1)
if args[:2] == ["plugin", "list"]:
    print("No plugins installed."); sys.exit(0)
print(f"{name}: this stand-in only supports mcp and plugin commands", file=sys.stderr)
sys.exit(2)
"""

AGENT_SAY = r"""#!/usr/bin/env python3
import json, sys, time, uuid, os
text = " ".join(sys.argv[1:]) or "hello"
folder = os.path.expanduser("~/.claude/projects/-home-user-work-ops")
os.makedirs(folder, exist_ok=True)
stamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
with open(os.path.join(folder, "new-chat.jsonl"), "a") as file:
    file.write(json.dumps({"type": "user", "sessionId": "new-chat", "uuid": str(uuid.uuid4()),
        "cwd": "/home/user/work/ops", "timestamp": stamp,
        "message": {"role": "user", "content": text}}) + "\n")
print("Claude Code recorded your message.")
"""

# install.sh downloads from GitHub; this curl serves the simulated release.
FAKE_CURL = r"""#!/bin/sh
out=""; url=""
while [ $# -gt 0 ]; do
  case "$1" in -o) out="$2"; shift 2 ;; -*) shift ;; *) url="$1"; shift ;; esac
done
case "$url" in
  https://github.com/kojah/mash/releases/*) file="$HOME/.release-mirror/${url##*/}" ;;
  *) echo "curl: (6) Could not resolve host: ${url#*://}" >&2; exit 6 ;;
esac
[ -f "$file" ] || { echo "curl: (22) The requested URL returned error: 404" >&2; exit 22; }
if [ -n "$out" ]; then cp "$file" "$out"; else cat "$file"; fi
"""


def iso(days: float) -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() - days * 86400))


def seed(home: Path) -> None:
    records: list[dict[str, object]]
    opencode = home / ".local/share/opencode/opencode.db"
    opencode.parent.mkdir(parents=True)
    database = sqlite3.connect(opencode)
    database.executescript(OPENCODE)
    for provider, chat, project, days, user, assistant in CHATS:
        if provider == "claude-code":
            folder = home / ".claude/projects" / project.replace("/", "-")
            folder.mkdir(parents=True, exist_ok=True)
            records = [
                {
                    "type": role,
                    "sessionId": chat,
                    "uuid": f"{chat}-{n}",
                    "cwd": project,
                    "timestamp": iso(days - n / 1440),
                    "message": {"role": role, "content": text},
                }
                for n, (role, text) in enumerate(
                    [("user", user), ("assistant", assistant)]
                )
            ]
            (folder / f"{chat}.jsonl").write_text(
                "".join(json.dumps(r) + "\n" for r in records)
            )
        elif provider == "codex":
            day = time.gmtime(time.time() - days * 86400)
            folder = home / ".codex/sessions" / time.strftime("%Y/%m/%d", day)
            folder.mkdir(parents=True, exist_ok=True)
            records = [
                {
                    "timestamp": iso(days),
                    "type": "session_meta",
                    "payload": {"id": chat, "timestamp": iso(days), "cwd": project},
                }
            ]
            for n, (role, kind, text) in enumerate(
                [("user", "input_text", user), ("assistant", "output_text", assistant)]
            ):
                records.append(
                    {
                        "timestamp": iso(days - (n + 1) / 1440),
                        "type": "response_item",
                        "payload": {
                            "type": "message",
                            "role": role,
                            "content": [{"type": kind, "text": text}],
                        },
                    }
                )
            name = f"rollout-{time.strftime('%Y-%m-%dT%H-%M-%S', day)}-{chat}.jsonl"
            (folder / name).write_text("".join(json.dumps(r) + "\n" for r in records))
        else:
            millis = int((time.time() - days * 86400) * 1000)
            database.execute(
                "INSERT OR IGNORE INTO project VALUES(?,?,'git')", (project, project)
            )
            database.execute(
                "INSERT INTO session VALUES(?,?,?,?,?,?)",
                (chat, project, chat, project, millis, millis),
            )
            for n, (role, text) in enumerate(
                [("user", user), ("assistant", assistant)]
            ):
                database.execute(
                    "INSERT INTO session_message VALUES(?,?,?,?,?,?)",
                    (
                        f"{chat}-{n}",
                        chat,
                        role,
                        n,
                        millis + n,
                        json.dumps(
                            {"text": text}
                            if role == "user"
                            else {"content": [{"type": "text", "text": text}]}
                        ),
                    ),
                )
    database.commit()
    database.close()


def release(home: Path, binary: Path) -> None:
    """A release laid out as install.sh expects, plus the script itself."""
    server = binary.with_name("mash-server")
    if not server.is_file():
        raise SystemExit(f"build mash-server alongside {binary} before starting")
    mirror = home / ".release-mirror"
    mirror.mkdir()
    archive = mirror / "mash-x86_64-unknown-linux-gnu.tar.gz"
    with tarfile.open(archive, "w:gz", compresslevel=1) as tar:
        tar.add(binary, arcname="mash")
        tar.add(server, arcname="mash-server")
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (mirror / "SHA256SUMS").write_text(f"{digest}  {archive.name}\n")
    downloads = home / "Downloads"
    downloads.mkdir()
    shutil.copy(REPOSITORY / "install.sh", downloads / "install.sh")


def tools(home: Path) -> None:
    fakes = home / ".fake-tools"
    fakes.mkdir()
    for name, text in [
        ("claude", FAKE_AGENT),
        ("codex", FAKE_AGENT),
        ("curl", FAKE_CURL),
    ]:
        (fakes / name).write_text(text)
        (fakes / name).chmod(0o755)
    (home / "bin").mkdir()
    (home / "bin/agent-say").write_text(AGENT_SAY)
    (home / "bin/agent-say").chmod(0o755)
    (home / ".profile").write_text(
        'export PATH="$HOME/.local/bin:$HOME/bin:$HOME/.fake-tools:/usr/bin:/bin"\n'
    )


def sandbox(directory: Path) -> list[str]:
    command = [
        "bwrap",
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/etc",
        "/etc",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--tmpfs",
        "/home",
        "--bind",
        str(directory / "home"),
        HOME,
        "--clearenv",
        "--setenv",
        "HOME",
        HOME,
        "--setenv",
        "USER",
        "user",
        "--setenv",
        "LANG",
        "C.UTF-8",
        "--setenv",
        "TERM",
        "dumb",
        "--setenv",
        "PATH",
        f"{HOME}/.local/bin:{HOME}/bin:{HOME}/.fake-tools:/usr/bin:/bin",
        "--chdir",
        HOME,
    ]
    for link in ("bin", "lib", "lib64", "sbin"):
        if Path(f"/{link}").is_symlink():
            command += ["--symlink", os.readlink(f"/{link}"), f"/{link}"]
    return command


def enter_script(directory: Path) -> str:
    sandbox_command = " ".join(f"'{part}'" for part in sandbox(directory))
    return f"""#!/bin/bash
# Run one shell command inside this UX session's sandbox.
[ $# -ge 1 ] || {{ echo "usage: $0 'command'" >&2; exit 2; }}
ulimit -v {LIMITS["virtual_memory_kib"]}
exec timeout {LIMITS["timeout_seconds"]} {sandbox_command} /bin/bash -c "$*"
"""


def probe(enter: Path, command: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(enter), command], capture_output=True, text=True, timeout=60, check=False
    )


def build(binary: str | None) -> tuple[Path, str]:
    revision = subprocess.run(
        ["git", "-C", str(REPOSITORY), "rev-parse", "HEAD"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    dirty = subprocess.run(
        ["git", "-C", str(REPOSITORY), "status", "--porcelain", "--untracked-files=no"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    if dirty:
        revision += "-dirty"
    if binary:
        return Path(binary).resolve(), revision
    target = Path("/home/james/scratch/achi-targets/integrated")
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--locked",
            "--bin",
            "mash",
            "--bin",
            "mash-server",
        ],
        cwd=REPOSITORY,
        check=True,
        env={
            **os.environ,
            "CARGO_TARGET_DIR": str(target),
            "TMPDIR": "/home/james/scratch/achi-tmp",
        },
    )
    return target / "release/mash", revision


def start(session: str, binary: str | None) -> dict:
    if not SESSION_ID.match(session):
        raise SystemExit(f"session id must match {SESSION_ID.pattern}")
    directory = ROOT / session
    if directory.exists():
        raise SystemExit(f"session {session} already exists")
    artifact, revision = build(binary)
    home = directory / "home"
    home.mkdir(parents=True)
    (directory / MARKER).write_text(json.dumps({"session": session}))
    seed(home)
    release(home, artifact)
    tools(home)
    enter = directory / "enter"
    enter.write_text(enter_script(directory))
    enter.chmod(0o755)
    isolation = {
        "network_blocked": probe(
            enter,
            "python3 -c 'import socket; socket.create_connection((\"1.1.1.1\", 53), 3)'",
        ).returncode
        != 0,
        "repository_absent": probe(enter, f"test ! -e {REPOSITORY}").returncode == 0,
        "host_home_absent": probe(
            enter, f'test ! -e {Path.home()} && test -z "$(ls /home | grep -v user)"'
        ).returncode
        == 0,
        "mash_not_installed": probe(enter, "! command -v mash").returncode == 0,
        "home": probe(enter, "echo $HOME").stdout.strip() == HOME,
    }
    report = {
        "session": session,
        "enter": str(enter),
        "binary": "not installed; ~/Downloads/install.sh installs the simulated release",
        "binary_sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
        "revision": revision,
        "roots": {"home": HOME, "host_path": str(home)},
        "isolation": isolation,
        "limits": LIMITS,
    }
    if not all(isolation.values()):
        shutil.rmtree(directory)
        raise SystemExit(f"isolation failed: {json.dumps(isolation)}")
    (directory / "report.json").write_text(json.dumps(report, indent=2))
    return report


def stop(session: str) -> dict:
    directory = ROOT / session
    marker = directory / MARKER
    if (
        not SESSION_ID.match(session)
        or not marker.is_file()
        or json.loads(marker.read_text()).get("session") != session
    ):
        raise SystemExit(f"refusing to remove unknown session {session!r}")
    shutil.rmtree(directory)
    return {"session": session, "removed": str(directory)}


def select(seed: int | None, family: str | None = None) -> dict:
    """Pick a journey; `family` pins one for a targeted batch."""
    seed = seed if seed is not None else random.SystemRandom().randrange(2**32)
    recent = []
    if JOURNALS.is_dir():
        for journal in sorted(
            JOURNALS.glob("**/*.md"), key=lambda p: p.stat().st_mtime, reverse=True
        ):
            found = re.search(
                r"^- Journey family: `([^`]+)`",
                journal.read_text(errors="replace"),
                re.MULTILINE,
            )
            if found and found.group(1) in FAMILIES and found.group(1) not in recent:
                recent.append(found.group(1))
            if len(recent) == 3:
                break
    choices = [f for f in FAMILIES if f not in recent]
    if len(choices) < 3:
        choices = list(FAMILIES)
    generator = random.Random(seed)
    if family is None:
        family = generator.choice(sorted(choices))
    elif family not in FAMILIES:
        raise SystemExit(
            f"unknown family {family!r}; choose from {', '.join(FAMILIES)}"
        )
    goal, stop_condition, variations = FAMILIES[family]
    variation = generator.choice(variations)
    return {
        "seed": seed,
        "family": family,
        "variation": variation,
        "goal": goal,
        "stop": stop_condition,
        "excluded": recent,
        "preinstall": family != "install-and-connect",
        "presetup": family
        in ("keep-fresh", "settings-and-scope", "remove-and-reinstall"),
    }


def prepare(session: str, preinstall: bool, presetup: bool) -> None:
    """Bring a fresh session to the journey's starting point."""
    enter = ROOT / session / "enter"
    steps = []
    if preinstall:
        steps.append("sh ~/Downloads/install.sh")
    if presetup:
        steps.append("~/.local/bin/mash init")
    for step in steps:
        result = probe(enter, step)
        if result.returncode != 0:
            raise SystemExit(
                f"preparing {session}: {step} failed:\n{result.stdout}\n{result.stderr}"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    choose = commands.add_parser("select")
    choose.add_argument("--seed", type=int)
    choose.add_argument("--family")
    begin = commands.add_parser("start")
    begin.add_argument("session")
    begin.add_argument("--binary")
    begin.add_argument("--preinstall", action="store_true")
    begin.add_argument("--presetup", action="store_true")
    end = commands.add_parser("stop")
    end.add_argument("session")
    arguments = parser.parse_args()
    if arguments.command == "select":
        report = select(arguments.seed, arguments.family)
    elif arguments.command == "start":
        report = start(arguments.session, arguments.binary)
        prepare(arguments.session, arguments.preinstall, arguments.presetup)
        report["prepared"] = {
            "installed": arguments.preinstall,
            "set_up": arguments.presetup,
        }
    else:
        report = stop(arguments.session)
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
