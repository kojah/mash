"""Reproduce pre-exec rusage retention without scientific imports or private data."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys

from memory_audit import kib_fields


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if platform.system() != 'Linux':
        raise ValueError('control requires Linux')
    if args.output.exists() or args.output.resolve().is_relative_to(Path(__file__).resolve().parents[2]):
        raise ValueError('use fresh private output')
    code = ('import resource,json; from pathlib import Path; '
            'print(json.dumps({"rusage_kib": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss, '
            '"status":Path("/proc/self/status").read_text()}))')
    seed = 'import os,sys; allocation=bytearray(64*1024*1024); os.execv(sys.executable,[sys.executable,"-c",sys.argv[1]])'
    reports = []
    for mode in ('clean', 'pre-exec-64MiB'):
        command = [sys.executable, '-c', code] if mode == 'clean' else [sys.executable, '-c', seed, code]
        # This driver is single-threaded. Force fork rather than vfork/posix_spawn
        # to isolate the child from the external launcher's pre-exec HWM.
        result = subprocess.run(command, preexec_fn=lambda: None, capture_output=True,
                                text=True, check=True, timeout=10)
        value = json.loads(result.stdout)
        reports.append({'mode': mode, 'rusage_kib': value['rusage_kib'],
                        **kib_fields(value['status'], {'VmRSS', 'VmHWM'})})
    with args.output.open('x') as output:
        json.dump({'schema_version': 1, 'scope': 'linux-pre-exec-rusage-control',
                   'python': platform.python_version(), 'platform': platform.platform(),
                   'source_sha256': {n: hashlib.sha256(Path(__file__).with_name(n).read_bytes()).hexdigest()
                                     for n in ('memory_exec_control.py', 'memory_audit.py')},
                   'captures': reports}, output, indent=2)
    print(json.dumps(reports))


if __name__ == '__main__':
    main()
