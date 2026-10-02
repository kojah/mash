import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from stage_parity import ParityError,compare,contract,decode


class StageParityTests(unittest.TestCase):
    def fixture(self):
        raw=json.dumps({'schema_version':1,'stages':[
            {'name':'units','diagnostics':{}},
            {'name':'clustering','diagnostics':{'scores':{'absolute':1e-9,'relative':0.0}}},
            {'name':'tags','diagnostics':{}}]}).encode()
        policy=contract(raw);pin=hashlib.sha256(b'synthetic fixture').hexdigest()
        result={'schema_version':1,'contract_sha256':policy.sha256,'fixture_sha256':pin,'stages':[
            {'name':'units','decisions':{'sources':[{'id':'a','start':0,'end':2,'included':True}]},'diagnostics':{}},
            {'name':'clustering','decisions':{'memberships':[['a','b'],['c']]},'diagnostics':{'scores':[0.5,-0.25]}},
            {'name':'tags','decisions':{'parents':[{'child':'leaf','parent':'root'}]},'diagnostics':{}}]}
        return raw,policy,pin,result

    def test_roundtrip_and_tolerated_diagnostics_preserve_exact_decisions(self):
        _,p,pin,a=self.fixture();b=decode(json.dumps(a));b['stages'][1]['diagnostics']['scores'][0]+=5e-10
        result=compare(a,b,p,pin);self.assertEqual(result['status'],'matched');self.assertEqual(result['numeric_components'],2)

    def test_near_threshold_membership_change_is_not_excused_by_numeric_tolerance(self):
        _,p,pin,a=self.fixture();b=copy.deepcopy(a)
        b['stages'][1]['diagnostics']['scores'][0]+=1e-10;b['stages'][1]['decisions']['memberships']=[['a'],['b','c']]
        with self.assertRaises(ParityError) as error:compare(a,b,p,pin)
        self.assertEqual(error.exception.stage,'clustering');self.assertEqual(error.exception.reason,'discrete decision differs')

    def test_reports_first_stage_and_exact_type_or_order_drift(self):
        _,p,pin,a=self.fixture()
        for value in [1,'true',None]:
            b=copy.deepcopy(a);b['stages'][0]['decisions']['sources'][0]['included']=value
            b['stages'][2]['diagnostics']['unexpected']=[0.0]
            with self.assertRaises(ParityError) as error:compare(a,b,p,pin)
            self.assertEqual(error.exception.stage,'units')
            self.assertEqual(error.exception.path,('decisions','sources',0,'included'))
        b=copy.deepcopy(a);b['stages'][1]['decisions']['memberships'][0].reverse()
        with self.assertRaises(ParityError):compare(a,b,p,pin)

    def test_ancestry_difference_fails_even_with_same_leaf_memberships(self):
        _,p,pin,a=self.fixture();b=copy.deepcopy(a);b['stages'][2]['decisions']['parents'][0]['parent']='other'
        with self.assertRaises(ParityError) as error:compare(a,b,p,pin)
        self.assertEqual(error.exception.stage,'tags')

    def test_provenance_and_complete_ordered_stage_coverage(self):
        _,p,pin,a=self.fixture()
        mutations=[lambda b:b.update(contract_sha256='0'*64),lambda b:b.update(fixture_sha256='0'*64),
                   lambda b:b.update(schema_version=True),lambda b:b['stages'].pop(),
                   lambda b:b['stages'].reverse(),lambda b:b.update(extra='field')]
        for mutation in mutations:
            b=copy.deepcopy(a);mutation(b)
            with self.assertRaises(ParityError):compare(a,b,p,pin)

    def test_numeric_diagnostic_shape_types_and_boundaries(self):
        _,p,pin,a=self.fixture()
        for values in [[0.5], [0.5,-0.25,0.0], [0.5,True], [0.5,0], [float('nan'),0.0], [float('inf'),0.0], [0.6,-0.25]]:
            b=copy.deepcopy(a);b['stages'][1]['diagnostics']['scores']=values
            with self.assertRaises(ParityError):compare(a,b,p,pin)
        b=copy.deepcopy(a);b['stages'][1]['diagnostics']['unexpected']=[0.0]
        with self.assertRaises(ParityError):compare(a,b,p,pin)
        b=copy.deepcopy(a);b['stages'][0]['decisions']['sources'][0]['start']=0.0
        with self.assertRaises(ParityError):compare(a,b,p,pin)

    def test_contract_rejects_reordering_duplicates_and_invalid_tolerances(self):
        raw,_,_,_=self.fixture();base=json.loads(raw)
        for v in [True,-1.0,float('inf'),'0',None]:
            data=copy.deepcopy(base);data['stages'][1]['diagnostics']['scores']['absolute']=v
            with self.assertRaises(ParityError):contract(json.dumps(data).encode())
        for stages in [[],list(reversed(base['stages'])),[base['stages'][0]]*2]:
            data=copy.deepcopy(base);data['stages']=stages
            with self.assertRaises(ParityError):contract(json.dumps(data).encode())

    def test_relative_bound_is_reference_scaled_and_overflow_fails_closed(self):
        raw,_,pin,a=self.fixture();v=json.loads(raw)
        v['stages'][1]['diagnostics']['scores']={'absolute':0.0,'relative':0.1};p=contract(json.dumps(v).encode())
        a['contract_sha256']=p.sha256;a['stages'][1]['diagnostics']['scores']=[1.0]
        b=copy.deepcopy(a);b['stages'][1]['diagnostics']['scores']=[1.11]
        with self.assertRaises(ParityError):compare(a,b,p,pin)
        a['stages'][1]['diagnostics']['scores']=[1e308];b['stages'][1]['diagnostics']['scores']=[-1e308]
        with self.assertRaises(ParityError) as error:compare(a,b,p,pin)
        self.assertEqual(error.exception.reason,'numeric comparison overflow')

    def test_json_duplicate_keys_nonfinite_and_integer_overflow_rejected(self):
        for raw in [b'{"a":1,"a":2}',b'{"x":NaN}',b'{"x":Infinity}',b'{"x":1e400}',b'not json',b'\xff',b'1'+b'0'*5000]:
            with self.assertRaises(ParityError):decode(raw)
        self.assertEqual(decode(b'18446744073709551615'),2**64-1)

    def test_cli_pins_fixture_and_returns_failure_without_payload_values(self):
        raw,p,pin,a=self.fixture()
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'contract.json').write_bytes(raw);(root/'fixture.json').write_bytes(b'synthetic fixture')
            (root/'reference.json').write_text(json.dumps(a));(root/'candidate.json').write_text(json.dumps(a))
            cmd=[sys.executable,str(Path(__file__).with_name('stage_parity.py'))]
            for key in ['contract','fixture','reference','candidate']:cmd.extend(['--'+key,str(root/(key+'.json'))])
            result=subprocess.run(cmd,text=True,capture_output=True,check=True);self.assertEqual(json.loads(result.stdout)['status'],'matched')
            a['stages'][0]['decisions']['sources'][0]['id']='DO_NOT_PRINT_SOURCE_PAYLOAD'
            (root/'candidate.json').write_text(json.dumps(a));result=subprocess.run(cmd,text=True,capture_output=True)
            self.assertEqual(result.returncode,1);self.assertNotIn('DO_NOT_PRINT_SOURCE_PAYLOAD',result.stdout+result.stderr)
            self.assertEqual(json.loads(result.stdout)['stage'],'units')


if __name__=='__main__':unittest.main()
