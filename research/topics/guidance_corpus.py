"""Frozen research source/group inventory, shared by downstream guidance controls."""
from dataclasses import dataclass
import json
from pathlib import Path
from types import SimpleNamespace
import numpy as np
from discovery_inputs import Inputs,read,digest


REFERENCE='1a4c6c992a2bd18213ae5a53bf20235d934db2e16eea4de7c167359f7b1ea0dc'


@dataclass(frozen=True)
class Corpus:
    data:Inputs
    reference:dict
    topic_owner:np.ndarray
    source_meeting:np.ndarray
    topic_meeting:np.ndarray


def load(root):
    path=root/'topic-qmsum-soft-guidance-first-20261002.json'
    if digest(path)!=REFERENCE:raise ValueError('frozen guidance reference required')
    reference=json.loads(path.read_text())
    for name,pin in reference['code_pins'].items():
        if digest(Path(__file__).with_name(name))!=pin:raise ValueError('reference implementation drift')
    cp=root/'topic-qmsum-query-navigation-first-20261002.json'
    if digest(cp)!=reference['control_sha256']:raise ValueError('query/group control drift')
    control=json.loads(cp.read_text());sp=root/'topic-qmsum-source-support-first-20261002.json'
    if digest(sp)!=control['control_sha256']:raise ValueError('source ownership control drift')
    source_control=json.loads(sp.read_text())
    def folder(n):return root/f'topic-qmsum-{n}-20261002'
    gp=folder('block-geometry-first')/'results.json'
    if digest(gp)!=source_control['geometry_sha256']:raise ValueError('geometry control drift')
    geometry=json.loads(gp.read_text())
    data=read(SimpleNamespace(source=folder('author-training'),vectors=folder('semantic-vectors-first'),units=folder('semantic-units-final-first'),
        labels=folder('label-cleanup-first'),baseline=folder('lexical-context-final-first'),reference_manifest=root/'grinch-reference-20261001/manifest.json'))
    if data.source!=reference['source_sha256']:raise ValueError('original source drift')
    owner=np.full(len(data.ready),-1,dtype=np.int64);meetings=np.full(len(owner),-1,dtype=np.int64);groups=control['groups']
    for meeting,g in zip(data.meetings,geometry['records']):
        offset=g['source_offset'];meetings[offset:offset+len(meeting.turns)]=meeting.ordinal
        assignment={b:v['id'] for v in groups if v['meeting']==meeting.ordinal for b in v['blocks']}
        for b in g['blocks']:owner[offset+b['start']:offset+b['end']+1]=assignment[b['ordinal']]
    if len(groups)!=1296 or not np.array_equal(owner>=0,data.ready) or (meetings<0).any():raise ValueError('complete source/topic inventory required')
    return Corpus(data,reference,owner,meetings,np.array([g['meeting'] for g in groups]))
