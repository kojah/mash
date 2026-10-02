"""Create or verify the exact historical scientific environment, outside Git."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess


PYTHON='3.11.15'
ROOT=Path(__file__).resolve().parent
PIN=re.compile(r'([A-Za-z0-9][A-Za-z0-9._-]*)==([0-9][A-Za-z0-9.+-]*)')
PROBE='''import importlib.metadata,json,platform
print(json.dumps({'python':platform.python_version(),'implementation':platform.python_implementation(),
 'platform':platform.platform(),'packages':[{'name':d.metadata['Name'],'version':d.version}
 for d in importlib.metadata.distributions()]}))'''


def digest(path):
    with path.open('rb') as file:return hashlib.file_digest(file,'sha256').hexdigest()


def canonical(name):return re.sub(r'[-_.]+','-',name).lower()


def read_pins(path):
    result={}
    for line in path.read_text().splitlines():
        line=line.strip()
        if not line or line.startswith('#'):continue
        match=PIN.fullmatch(line)
        if match is None:raise ValueError('exact package/version pins required')
        name=canonical(match[1])
        if name in result:raise ValueError('duplicate canonical package pin')
        result[name]=match[2]
    if not result:raise ValueError('nonempty scientific pins required')
    return result


def verify_manifest(actual,pins):
    if actual['python']!=PYTHON or actual['implementation']!='CPython':raise ValueError('exact CPython 3.11.15 required')
    packages={}
    for row in actual['packages']:
        name=canonical(row['name'])
        if name in packages:raise ValueError('duplicate installed distribution')
        packages[name]=row['version']
    if packages!=pins:raise ValueError('installed package set/versions differ from scientific pins')
    return {'schema_version':1,**actual,'packages':[{'name':name,'version':version} for name,version in sorted(packages.items())]}


def verify(python):
    actual=json.loads(subprocess.check_output([str(python),'-I','-c',PROBE],text=True))
    result=verify_manifest(actual,read_pins(ROOT/'science-requirements.in'))
    result.update({'pins_sha256':digest(ROOT/'science-requirements.in'),
        'hash_lock_sha256':digest(ROOT/'science-requirements.txt'),'initializer_sha256':digest(Path(__file__))})
    return result


def validate_destination(destination):
    destination=destination.resolve()
    if destination.is_relative_to(ROOT.parents[1]):raise ValueError('environment/artifacts must remain outside Git')
    if destination.exists():raise ValueError('fresh destination required; existing environment preserved')
    return destination


def create(destination):
    destination=validate_destination(destination);uv=shutil.which('uv')
    if uv is None:raise RuntimeError('uv required')
    before={name:digest(ROOT/name) for name in ['science-requirements.in','science-requirements.txt']}
    destination.mkdir(parents=True,mode=0o700);environment=destination/'venv'
    commands=[[uv,'venv','--no-config','--no-project','--no-python-downloads','--python',PYTHON,'--link-mode','copy',str(environment)],
        [uv,'pip','install','--no-config','--default-index','https://pypi.org/simple','--python',str(environment/'bin/python'),
         '--require-hashes','--link-mode','copy','-r',str(ROOT/'science-requirements.txt')]]
    with (destination/'setup.log').open('x') as log:
        for command in commands:subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,check=True)
    result=verify(environment/'bin/python')
    if before!={name:digest(ROOT/name) for name in before}:raise ValueError('scientific pins drifted during setup')
    with (destination/'manifest.json').open('x') as file:json.dump(result,file,sort_keys=True,allow_nan=False)
    (destination/'manifest.json').chmod(0o600)
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);mode=parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--create',type=Path);mode.add_argument('--verify',type=Path)
    args=parser.parse_args();result=create(args.create) if args.create else verify(args.verify)
    print(json.dumps({'verified':True,'python':result['python'],'package_count':len(result['packages']),
        'pins_sha256':result['pins_sha256'],'hash_lock_sha256':result['hash_lock_sha256']}))
