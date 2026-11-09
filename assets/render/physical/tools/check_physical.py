#!/usr/bin/env python3
"""Run physical-data checks and optional read-only native card admission.

The optional --cli must point to a trusted existing SolidCraft CLI executable.
It only runs material.properties.validate in fresh CLI processes; it never runs
material.create, material.assign, opens a user's document or writes a native design.
Evidence stays under ignored target/. This is not aggregate workspace CI.
"""
from __future__ import annotations
import argparse
import copy
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

import physical


def digest_file(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda:f.read(1024*1024),b''):h.update(block)
    return h.hexdigest()


def snapshot():
    paths=[p for p in physical.PACK.rglob('*') if p.is_file() and '__pycache__' not in p.parts]
    paths += [physical.ROOT/p for p in ('docs/render-materials/materials.json','assets/render/starter/materials/manifest.json',
                                      'crates/doc/src/physical_material.rs','crates/engine/src/cmd/material_properties.rs','ATTRIBUTION.md')]
    return {p.relative_to(physical.ROOT).as_posix():digest_file(p) for p in sorted(paths)}


def invoke(cli,card):
    payload=json.dumps({'card':card},ensure_ascii=True,separators=(',',':'),allow_nan=False)
    physical.require(len(payload)<65536,'native candidate exceeds invocation budget')
    result=subprocess.run([str(cli),'exec','material.properties.validate',payload],cwd=physical.ROOT,
                          capture_output=True,timeout=30)
    physical.require(len(result.stdout)+len(result.stderr)<4*1024*1024,'native output exceeds budget')
    if result.returncode:return result,None
    return result,json.loads(result.stdout,object_pairs_hook=physical.unique,parse_constant=physical.reject_constant)


def native_check(cli,items):
    physical.require(cli.is_file(),'trusted native CLI not found')
    identity={'name':cli.name,'sha256':digest_file(cli),'rebuilt_by_this_gate':False}
    receipts=[]
    for i,item in enumerate(items):
        result,value=invoke(cli,item['card'])
        physical.require(result.returncode==0 and isinstance(value,dict) and value.get('valid') is True,
                         f"native card admission failed: {item['profile_id']}: {result.stderr.decode(errors='replace')[-500:]}")
        report=value.get('validation',{})
        expected=sorted(r['data']['kind'] for r in item['card']['properties'])
        physical.require(sorted(report.get('property_kinds',[]))==expected,'native property kinds changed or were dropped')
        receipts.append({'profile_id':item['profile_id'],'valid':True,'property_kinds':expected,
                         'empty_projection':not expected,'source_review_required':report.get('source_review_required'),
                         'missing_condition_fields':report.get('missing_condition_fields'),
                         'isotropic_moduli':report.get('isotropic_moduli')})
        if (i+1)%12==0:print(json.dumps({'native_cards_checked':i+1,'total':len(items)}),flush=True)
    template=next(x['card'] for x in items if x['profile_id']=='steel-c45')
    bad=[]
    x=copy.deepcopy(template);next(r for r in x['properties'] if r['data']['kind']=='elasticity')['data']['model']['poisson_ratio']=.5;bad.append(('unstable-isotropic',x))
    x=copy.deepcopy(template);x['properties'][0]['source_id']='missing';bad.append(('unknown-source',x))
    x=copy.deepcopy(template);x['properties'].append(copy.deepcopy(x['properties'][0]));bad.append(('duplicate-property',x))
    x=copy.deepcopy(template);next(r for r in x['properties'] if r['data']['kind']=='density')['data']['kg_m3']=-1;bad.append(('negative-density',x))
    refused=[]
    for label,card in bad:
        result,value=invoke(cli,card)
        physical.require(result.returncode!=0 or isinstance(value,dict) and value.get('valid') is False,
                         f'hostile card was accepted: {label}')
        refused.append(label)
    return {'binary':identity,'admitted_card_envelopes':len(receipts),
            'nonempty_cards':sum(not r['empty_projection'] for r in receipts),
            'empty_cards_not_ready':sum(r['empty_projection'] for r in receipts),
            'hostile_cases_refused':refused,'receipts':receipts,'persistence_tested':False,'body_assignment_tested':False}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--cli',type=Path)
    args=parser.parse_args();stamp=datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')
    folder=physical.ROOT/'target/physical-material-review'/stamp;folder.mkdir(parents=True,exist_ok=False)
    start=time.monotonic();before=snapshot();report={'schema':'solidcraft-physical-check/1','status':'fail','source_inputs':before,
      'python':sys.version.split()[0],'full_workspace_ci':'not_run_by_this_gate','runtime_assignment':False}
    exit_code=1
    try:
        result=subprocess.run([sys.executable,'-B','-m','unittest','discover','-s','assets/render/physical/tools','-p','test_*.py'],
                              cwd=physical.ROOT,capture_output=True,timeout=120)
        (folder/'python-tests.stdout.txt').write_bytes(result.stdout);(folder/'python-tests.stderr.txt').write_bytes(result.stderr)
        physical.require(result.returncode==0,'physical Python regressions failed')
        report['python_tests']=result.stderr.decode(errors='replace')[-1000:]
        report['coverage']=physical.validate()
        bundle=physical.read(physical.PACK/'exports/native-cards.json')
        report['native']=native_check(args.cli,bundle['items']) if args.cli else {'status':'not_requested'}
        after=snapshot();physical.require(before==after,'physical/source inputs changed during validation')
        report['source_inputs_unchanged']=True;report['status']='pass';exit_code=0
    except (OSError,ValueError,TypeError,KeyError,subprocess.SubprocessError) as error:
        report['error']=str(error)
    report['seconds']=round(time.monotonic()-start,3)
    (folder/'report.json').write_bytes(physical.encoded(report))
    print(json.dumps({'status':report['status'],'report':str((folder/'report.json').relative_to(physical.ROOT)),
                      'native_requested':bool(args.cli),'runtime_assignment':False,'error':report.get('error')},indent=2))
    return exit_code

if __name__=='__main__':raise SystemExit(main())
