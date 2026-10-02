"""Development-only stage comparator; no topic algorithm or inference selected."""
import argparse
from dataclasses import dataclass
from enum import StrEnum
import hashlib
import json
import math
from pathlib import Path
import re


class Stage(StrEnum):
    UNITS = 'units'
    CONTEXT = 'context'
    EMBEDDINGS = 'embeddings'
    CLUSTERING = 'clustering'
    LABELS = 'labels'
    TAGS = 'tags'
    LIFECYCLE = 'lifecycle'


class ParityError(ValueError):
    def __init__(self, reason, stage=None, path=()):
        self.reason,self.stage,self.path=reason,stage,path
        super().__init__(reason)

    def report(self):
        # Never include source text, vectors or expected/actual payload values.
        return {'status':'failed','reason':self.reason,'stage':self.stage,'path':list(self.path)}


@dataclass(frozen=True)
class Tolerance:
    absolute: float
    relative: float


@dataclass(frozen=True)
class StageContract:
    name: Stage
    diagnostics: dict[str,Tolerance]


@dataclass(frozen=True)
class Contract:
    sha256: str
    stages: tuple[StageContract,...]


def fields(value,expected,stage=None,path=()):
    if type(value) is not dict or set(value)!=set(expected):
        raise ParityError('schema fields differ',stage,path)


def decode(raw):
    def object_pairs(pairs):
        result={}
        for key,value in pairs:
            if key in result:raise ParityError('duplicate JSON key')
            result[key]=value
        return result
    def invalid(_):raise ParityError('nonfinite JSON constant')
    def integer(raw):
        if len(raw)>21:raise ParityError('integer outside JSON u64/i64 range')
        value=int(raw)
        if not -(2**63)<=value<2**64:raise ParityError('integer outside JSON u64/i64 range')
        return value
    def floating(raw):
        value=float(raw)
        if not math.isfinite(value):raise ParityError('nonfinite JSON number')
        return value
    try:return json.loads(raw,object_pairs_hook=object_pairs,parse_constant=invalid,parse_int=integer,parse_float=floating)
    except (json.JSONDecodeError,UnicodeDecodeError,RecursionError) as error:
        raise ParityError('invalid JSON') from error


def finite_number(value):
    try:return type(value) in (int,float) and math.isfinite(value)
    except OverflowError:return False


def name(value):
    return type(value) is str and re.fullmatch(r'[a-z][a-z0-9_]{0,63}',value) is not None


def contract(raw):
    value=decode(raw);fields(value,['schema_version','stages'])
    if type(value['schema_version']) is not int or value['schema_version']!=1:
        raise ParityError('unsupported schema version')
    if type(value['stages']) is not list or not value['stages']:raise ParityError('empty or invalid stage contract')
    stages=[]
    for item in value['stages']:
        fields(item,['name','diagnostics'])
        try:stage=Stage(item['name'])
        except (ValueError,TypeError) as error:raise ParityError('unknown stage') from error
        if type(item['diagnostics']) is not dict:raise ParityError('invalid diagnostic contract',stage)
        diagnostics={}
        for key,tolerance in item['diagnostics'].items():
            if not name(key):raise ParityError('invalid diagnostic name',stage)
            fields(tolerance,['absolute','relative'],stage,(key,))
            if any(not finite_number(v) or v<0 for v in tolerance.values()):
                raise ParityError('invalid numeric tolerance',stage,(key,))
            diagnostics[key]=Tolerance(float(tolerance['absolute']),float(tolerance['relative']))
        stages.append(StageContract(stage,diagnostics))
    names=[s.name for s in stages];ordered=[s for s in Stage if s in names]
    if names!=ordered:raise ParityError('duplicate or reordered contract stages')
    return Contract(hashlib.sha256(raw).hexdigest(),tuple(stages))


def exact_value(value,stage,path=()):
    if len(path)>128:raise ParityError('decision nesting limit exceeded',stage)
    if type(value) is dict:
        for key,item in value.items():
            if not name(key):raise ParityError('invalid decision field name',stage,path)
            exact_value(item,stage,(*path,key))
    elif type(value) is list:
        for i,item in enumerate(value):exact_value(item,stage,(*path,i))
    elif type(value) not in (str,int,bool,type(None)):
        raise ParityError('invalid decision type',stage,path)
    elif type(value) is int and not -(2**63)<=value<2**64:
        raise ParityError('decision integer outside JSON u64/i64 range',stage,path)


def validate_header(value,policy,fixture_sha256):
    fields(value,['schema_version','contract_sha256','fixture_sha256','stages'])
    if type(value['schema_version']) is not int or value['schema_version']!=1:
        raise ParityError('unsupported schema version')
    if value['contract_sha256']!=policy.sha256:raise ParityError('contract provenance differs')
    if value['fixture_sha256']!=fixture_sha256:raise ParityError('fixture provenance differs')
    if type(value['stages']) is not list or len(value['stages'])!=len(policy.stages):
        raise ParityError('incomplete or extra stages')


def validate_stage(item,spec):
    fields(item,['name','decisions','diagnostics'],spec.name)
    if item['name']!=spec.name:raise ParityError('stage order differs',spec.name)
    if type(item['decisions']) is not dict:raise ParityError('decision object required',spec.name)
    exact_value(item['decisions'],spec.name)
    fields(item['diagnostics'],spec.diagnostics,spec.name,('diagnostics',))
    for key,values in item['diagnostics'].items():
        if type(values) is not list or any(type(v) is not float or not math.isfinite(v) for v in values):
            raise ParityError('finite float diagnostic array required',spec.name,('diagnostics',key))


def first_difference(left,right,path=()):
    if type(left) is not type(right):return path
    if type(left) is dict:
        if set(left)!=set(right):return path
        for key in sorted(left):
            different=first_difference(left[key],right[key],(*path,key))
            if different is not None:return different
    elif type(left) is list:
        if len(left)!=len(right):return path
        for i,(a,b) in enumerate(zip(left,right)):
            different=first_difference(a,b,(*path,i))
            if different is not None:return different
    elif left!=right:return path
    return None


def compare(reference,candidate,policy,fixture_sha256):
    if not re.fullmatch('[0-9a-f]{64}',fixture_sha256):raise ParityError('invalid fixture pin')
    validate_header(reference,policy,fixture_sha256);validate_header(candidate,policy,fixture_sha256)
    maximum=0.;components=0
    for left,right,spec in zip(reference['stages'],candidate['stages'],policy.stages):
        validate_stage(left,spec);validate_stage(right,spec)
        different=first_difference(left['decisions'],right['decisions'])
        if different is not None:raise ParityError('discrete decision differs',spec.name,('decisions',*different))
        for key,tolerance in spec.diagnostics.items():
            a,b=left['diagnostics'][key],right['diagnostics'][key]
            if len(a)!=len(b):raise ParityError('diagnostic shape differs',spec.name,('diagnostics',key))
            for i,(expected,actual) in enumerate(zip(a,b)):
                difference=abs(expected-actual);bound=tolerance.absolute+tolerance.relative*abs(expected)
                if not math.isfinite(difference) or not math.isfinite(bound):
                    raise ParityError('numeric comparison overflow',spec.name,('diagnostics',key,i))
                maximum=max(maximum,difference);components+=1
                if difference>bound:
                    raise ParityError('numeric tolerance exceeded',spec.name,('diagnostics',key,i))
    return {'status':'matched','stages':len(policy.stages),'numeric_components':components,
            'maximum_absolute_difference':maximum,'contract_sha256':policy.sha256,'fixture_sha256':fixture_sha256}


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for key in ['contract','fixture','reference','candidate']:p.add_argument('--'+key,type=Path,required=True)
    args=p.parse_args()
    try:
        policy=contract(args.contract.read_bytes());pin=hashlib.sha256(args.fixture.read_bytes()).hexdigest()
        result=compare(decode(args.reference.read_bytes()),decode(args.candidate.read_bytes()),policy,pin)
    except ParityError as error:
        print(json.dumps(error.report(),allow_nan=False));raise SystemExit(1) from None
    print(json.dumps(result,allow_nan=False))


if __name__=='__main__':main()
