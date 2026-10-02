"""Explicit pinned local author reference; no implicit dependency installation."""
import importlib
import json
from pathlib import Path
import sys

from cqadup_semantic import digest


AUTHOR='33faf171cf145a1e0a8b35d75730db5aca22baf1'
MANIFEST='c30686369754137063394c219edfdec904b1610b34655c55a5bf3b6ba4b0ac05'


def load(author):
    path=author/'source-manifest.json'
    if digest(path)!=MANIFEST:raise ValueError('pinned author manifest required')
    m=json.loads(path.read_text())
    if m['commit']!=AUTHOR or m['repository']!='https://github.com/wq2012/LinksCluster':raise ValueError('author source identity drift')
    for name,sha in m['files'].items():
        if digest(author/name)!=sha:raise ValueError('author source bytes changed')
    sys.path.insert(0,str(author.resolve()));reference=importlib.import_module('linkscluster')
    if Path(reference.__file__).resolve()!=author.resolve()/'linkscluster/__init__.py':raise ValueError('wrong imported reference')
    return reference
