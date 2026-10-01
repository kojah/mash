"""Linux-only external research memory sampling; peaks are sampled lower bounds."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys
import time


def kib_fields(text, names):
    values = {}
    for line in text.splitlines():
        key, _, raw = line.partition(':')
        if key not in names:
            continue
        tokens = raw.split()
        if key in values or len(tokens) != 2 or tokens[1] != 'kB' or not tokens[0].isdigit():
            raise ValueError('invalid or duplicate proc memory counter')
        values[key] = int(tokens[0])
    if set(values) != set(names):
        raise ValueError('missing proc memory counter')
    return values


def sample(pid):
    root = Path('/proc') / str(pid)
    text = (root / 'status').read_text()
    if any(line.startswith('State:') and line.split()[1] in {'Z', 'X'} for line in text.splitlines()):
        raise ProcessLookupError('target address space already exited')
    if not any(line.startswith(('VmRSS:', 'VmHWM:')) for line in text.splitlines()):
        # exit_mm can release the address space before State becomes Z or wait
        # observes completion. This is a terminal address-space race, not bad units.
        raise ProcessLookupError('target has no resident address space')
    status = kib_fields(text, {'VmRSS', 'VmHWM'})
    text = (root / 'smaps_rollup').read_text()
    if not text.strip():
        raise ProcessLookupError('target mappings already exited')
    rollup = kib_fields(text, {'Rss', 'Pss'})
    return {**status, **rollup}


def monitor(command, interval=.02):
    if platform.system() != 'Linux':
        raise ValueError('memory audit requires Linux procfs')
    if not command or interval < .01 or interval > 1:
        raise ValueError('invalid command or sampling interval')
    started = time.monotonic()
    samples, vanished = [], 0
    # Never use a shell; child output retains the original runner's behavior.
    with subprocess.Popen(command) as child:
        while child.poll() is None:
            try:
                samples.append({'elapsed_ms': (time.monotonic() - started) * 1000, **sample(child.pid)})
            except (FileNotFoundError, ProcessLookupError):
                vanished += 1
            except ValueError:
                # A target may exit between poll and the multi-file snapshot.
                # Preserve genuine malformed-counter errors while it is live.
                if child.poll() is None:
                    raise
                vanished += 1
            time.sleep(interval)
        exit_code = child.returncode
    if not samples:
        raise ValueError('target exited without a usable memory sample')
    return {'schema_version': 1, 'scope': 'linux-external-topic-memory-audit',
            'python': platform.python_version(), 'platform': platform.platform(),
            'command': command, 'interval_ms': interval * 1000, 'exit_code': exit_code,
            'samples': samples, 'sample_count': len(samples), 'vanished_reads': vanished,
            'maximum_kib': {name: max(s[name] for s in samples) for name in ('VmRSS', 'VmHWM', 'Rss', 'Pss')},
            'sampler_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            'limitations': ['sampled RSS/PSS maxima are lower bounds, not continuous peaks',
                            'VmRSS/VmHWM use kernel approximate counters; smaps_rollup snapshots are precise at observation',
                            'monitoring may perturb runtime; not a timing or full-ingestion performance gate',
                            'one target process only, excludes children and observer memory']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private audit output')
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    result = monitor(command)
    with args.output.open('x') as output:
        json.dump(result, output, indent=2, allow_nan=False)
    print(json.dumps({k: result[k] for k in ('exit_code', 'sample_count', 'maximum_kib')}))
    if result['exit_code'] != 0:
        sys.exit(result['exit_code'])


if __name__ == '__main__':
    main()
