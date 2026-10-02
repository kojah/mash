"""Fixed exact-atom general-English weighting; no source rewriting or model."""
from dataclasses import dataclass
import hashlib
import importlib.metadata
import math
from pathlib import Path
from types import MappingProxyType
from typing import Mapping


@dataclass(frozen=True)
class EnglishBackground:
    frequencies: Mapping[str,float]
    asset_sha256: str

    def factors(self,vocabulary):
        atoms={atom for term in vocabulary for atom in term.split()};information={}
        for atom in atoms:
            probability=self.frequencies.get(atom,0.)
            if type(probability) not in {int,float} or not math.isfinite(probability) or not 0<=probability<=1:
                raise ValueError('finite background probability required')
            information[atom]=max(1.,min(9.,-math.log10(max(probability,1e-9))))/9
        return {term:sum(information[atom] for atom in term.split())/len(term.split()) for term in vocabulary}


def load():
    import wordfreq
    if importlib.metadata.version('wordfreq')!='3.1.1':raise ValueError('pinned wordfreq required')
    path=Path(wordfreq.available_languages('large')['en'])
    with path.open('rb') as file:identity=hashlib.file_digest(file,'sha256').hexdigest()
    frame=EnglishBackground(MappingProxyType(wordfreq.get_frequency_dict('en','large')),identity)
    code=Path(wordfreq.__file__)
    with code.open('rb') as file:code_sha=hashlib.file_digest(file,'sha256').hexdigest()
    return frame,{'library':'wordfreq','version':'3.1.1','language':'en','wordlist':'large',
        'lookup':'exact scanner atom; no library tokenization','asset_sha256':identity,
        'api_source_sha256':code_sha,'dictionary_entries':len(frame.frequencies)}
