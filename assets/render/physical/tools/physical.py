#!/usr/bin/env python3
"""Validate physical references and prepare non-executing schema-1 material cards.

JSON masters retain provenance, directional data, limits and unsupported quantities.
No CAD document is opened, no body is assigned and no missing constant is invented.
"""
from __future__ import annotations
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[4]
PACK = ROOT / 'assets/render/physical'
MAX_BYTES = 8 * 1024 * 1024
ID = re.compile(r'[a-z][a-z0-9-]{0,63}\Z')
CORE = {'density','elasticity','tensile_strength','yield_strength','compressive_strength','shear_strength','thermal_conductivity','thermal_expansion','specific_heat'}
PRESSURE = {'young_modulus','shear_modulus','tensile_strength','yield_strength','compressive_strength','shear_strength','flexural_strength','flexural_modulus','compressive_modulus','stress_at_strain','internal_bond_strength'}
TEMPERATURE = {'glass_transition','melting_temperature','heat_deflection','service_temperature','decomposition_temperature'}
DIMENSIONLESS = {'poisson_ratio','elongation_break','relative_permittivity','loss_tangent','water_absorption','hardness_shore_a','hardness_shore_d','hardness_brinell','hardness_knoop','specific_gravity','electrical_conductivity_iacs','refractive_index','porosity','void_ratio','thermal_emittance','solar_absorptance','thickness_swelling'}
CONVERSIONS = {
 'kg/m^3':('density',1.,0.), 'g/cm^3':('density',1000.,0.), 'lb/in^3':('density',27679.904710203,0.),
 'Pa':('pressure',1.,0.), 'kPa':('pressure',1e3,0.), 'MPa':('pressure',1e6,0.), 'GPa':('pressure',1e9,0.),
 'psi':('pressure',6894.757293168,0.), 'ksi':('pressure',6894757.293168,0.),
 '1':('ratio',1.,0.), '%':('ratio',.01,0.), 'W/(m K)':('conductivity',1.,0.),
 'Btu/(h ft degF)':('conductivity',1.730734666371,0.), '1/K':('expansion',1.,0.),
 'ppm/K':('expansion',1e-6,0.), '1/degF':('expansion',1.8,0.),
 'J/(kg K)':('heat',1.,0.), 'J/(g K)':('heat',1000.,0.), 'Btu/(lb degF)':('heat',4186.8,0.),
 'K':('temperature',1.,0.), 'degC':('temperature',1.,273.15), 'degF':('temperature',5/9,255.3722222222222),
 'ohm m':('resistivity',1.,0.), 'ohm cm':('resistivity',.01,0.), 'microohm cm':('resistivity',1e-8,0.), 'ohm mm^2/m':('resistivity',1e-6,0.),
 'V/m':('field',1.,0.), 'kV/mm':('field',1e6,0.), 'V/mil':('field',1/.0000254,0.),
 'N':('force',1.,0.), 'N/m':('tear',1.,0.), 'pli':('tear',175.1268352465,0.),
 'Pa sqrt(m)':('toughness',1.,0.), 'MPa sqrt(m)':('toughness',1e6,0.),
 'm':('length',1.,0.), 'micrometre':('length',1e-6,0.), 'mil':('length',.0000254,0.),
 'rad':('angle',1.,0.), 'degree':('angle',math.pi/180,0.)}
QUANTITY = {q:('pressure','Pa') for q in PRESSURE}
QUANTITY.update({q:('temperature','K') for q in TEMPERATURE})
QUANTITY.update({q:('ratio','1') for q in DIMENSIONLESS})
QUANTITY.update({'density':('density','kg/m^3'),'grain_density':('density','kg/m^3'),
 'thermal_conductivity':('conductivity','W/(m K)'), 'thermal_expansion':('expansion','1/K'),
 'specific_heat':('heat','J/(kg K)'), 'volume_resistivity':('resistivity','ohm m'),
 'dielectric_strength':('field','V/m'), 'hardness_janka':('force','N'), 'tear_strength':('tear','N/m'),
 'fracture_toughness':('toughness','Pa sqrt(m)'), 'mean_particle_size':('length','m'),
 'dry_film_thickness':('length','m'), 'angle_of_repose':('angle','rad')})

class Invalid(ValueError):
    """A physical reference violates the reviewed contract."""

def require(ok, message):
    if not ok: raise Invalid(message)

def unique(pairs):
    out = {}
    for key, value in pairs:
        require(key not in out, f'duplicate JSON field: {key}')
        out[key] = value
    return out

def reject_constant(value):
    raise Invalid(f'non-finite JSON constant: {value}')

def read(path):
    require(path.is_file() and not path.is_symlink(), f'expected regular input: {path.name}')
    require(path.stat().st_size <= MAX_BYTES, f'oversized input: {path.name}')
    raw = path.read_bytes();require(len(raw) <= MAX_BYTES, 'input grew beyond byte budget')
    data = json.loads(raw, object_pairs_hook=unique, parse_constant=reject_constant)
    require(isinstance(data, dict), 'expected JSON object')
    return data

def text(value, label, limit=2048):
    require(isinstance(value,str) and 0 < len(value.encode('utf-8')) <= limit and not any(ord(c)<32 for c in value), f'invalid {label}')

def number(value, low=-1e30, high=1e30):
    require(type(value) in (int,float) and low <= value <= high and math.isfinite(value), 'invalid bounded number')
    return float(value)

def close(a,b):
    return math.isclose(a,b,rel_tol=2e-11,abs_tol=1e-14*max(abs(b),1e-20))

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def encoded(value):
    return (json.dumps(value,ensure_ascii=True,indent=2,allow_nan=False)+'\n').encode()

def check_conditions(c):
    base={'temperature_k','temperature_interval_k','moisture_dry_basis_kg_kg','strain_rate_per_s','state','direction','test_method'}
    numeric={'crosshead_speed_mm_min','frequency_hz','specimen_thickness_mm','engineering_strain','heating_rate_k_min',
             'test_stress_pa','wavelength_nm','minimum_film_thickness_m','immersion_hours','specimen_mass_kg'}
    strings={'environment','thermal_regime','density_basis','excitation'}
    require(isinstance(c,dict) and base<=set(c)<=base|numeric|strings|{'lower_thickness_exclusive','thickness_range_mm'},'unknown or missing condition field')
    for key in numeric & set(c): number(c[key],0,1e18)
    for key in strings & set(c): text(c[key],key,256)
    if 'lower_thickness_exclusive' in c: require(type(c['lower_thickness_exclusive']) is bool,'thickness endpoint flag must be Boolean')
    if 'thickness_range_mm' in c:
        bounds=c['thickness_range_mm']
        require(isinstance(bounds,list) and len(bounds)==2 and 0<number(bounds[0])<number(bounds[1]),'invalid specimen thickness range')
    if c['temperature_k'] is not None: number(c['temperature_k'],1e-20,20000)

def check_property(p, sources):
    require(isinstance(p,dict),'property must be an object')
    required={'id','quantity','value','unit','qualifier','source_id','source_locator','original','conditions','note','uncertainty'}
    require(required <= set(p) <= required|{'derivation'},'unknown or missing property fields')
    text(p['id'],'property ID',128)
    q=p['quantity'];require(q in QUANTITY and p['unit']==QUANTITY[q][1],'unknown quantity or wrong SI unit')
    require(p['source_id'] in sources,'unresolved property source');text(p['source_locator'],'source locator')
    require(p['qualifier'] in {'typical','reported_range','lower_bound','upper_bound','derived'},'invalid qualifier')
    orig=p['original'];require(isinstance(orig,dict) and set(orig)=={'value','unit','scale_to_si','offset_to_si'},'invalid original quantity')
    require(orig['unit'] in CONVERSIONS,'unknown source unit')
    dim,scale,offset=CONVERSIONS[orig['unit']]
    require(dim==QUANTITY[q][0],'source quantity has incompatible dimensions')
    require(number(orig['scale_to_si'])==scale and number(orig['offset_to_si'])==offset,'conversion recipe altered')
    value,raw=p['value'],orig['value'];is_range=isinstance(value,list)
    require(is_range==isinstance(raw,list),'range/scalar mismatch')
    if is_range:
        require(len(value)==len(raw)==2 and p['qualifier']=='reported_range','range needs two endpoints and range qualifier')
        require(number(value[0])<=number(value[1]),'reversed range')
    else: require(p['qualifier']!='reported_range','scalar cannot claim range')
    for v,r in zip(value if is_range else [value],raw if is_range else [raw]):
        v,r=number(v),number(r);require(close(v,r*scale+offset),'SI value does not reproduce cited original')
        if q not in {'thermal_expansion','poisson_ratio'}: require(v>=0,'negative physical value')
        if q in TEMPERATURE|{'density','grain_density','young_modulus','shear_modulus','specific_heat','thermal_conductivity','relative_permittivity','refractive_index'}:require(v>0,'positive physical quantity required')
        if q in {'porosity','thermal_emittance','solar_absorptance'}:require(v<=1,'fraction above one')
        if q.startswith('hardness_shore_'):require(v<=100,'Shore hardness above 100')
    c=p['conditions'];check_conditions(c)
    for key in ['temperature_k','moisture_dry_basis_kg_kg','strain_rate_per_s']:
        require(key in c,'condition field must be explicit even when unknown')
        if c[key] is not None:number(c[key],0,1e12)
    if c.get('temperature_interval_k') is not None:
        interval=c['temperature_interval_k'];require(isinstance(interval,list) and len(interval)==2,'temperature interval shape')
        require(0<number(interval[0])<=number(interval[1])<=20000,'temperature interval bounds')
    for key in ('state','direction','test_method'):text(c.get(key),key,1024)
    if p['uncertainty'] is not None:
        u=p['uncertainty'];require(isinstance(u,dict) and set(u)=={'reported_plus_minus','interpretation'},'invalid uncertainty')
        number(u['reported_plus_minus'],0);text(u['interpretation'],'uncertainty interpretation')
    require((p['qualifier']=='derived')==('derivation' in p),'derived value needs a calculation receipt')

def check_derivation(record, properties):
    d=record['derivation'];require(isinstance(d,dict),'invalid derivation')
    require(all(key in properties for key in d.get('input_property_ids',[])),'missing derivation input')
    if d.get('formula')=='young_modulus/(2*shear_modulus)-1':
        require(record['quantity']=='poisson_ratio','wrong modulus-derived quantity')
        e,g=properties['young_modulus'],properties['shear_modulus']
        require(e['source_id']==g['source_id']==record['source_id'],'mixed-source elastic derivation')
        expected=number(e['value'])/(2*number(g['value'],1e-30))-1
        require(-1<expected<.5 and close(record['value'],expected),'incorrect derived Poisson ratio')
    elif d.get('formula')=='specific_gravity*1000*(1+moisture_dry_basis_kg_kg)':
        require(record['quantity']=='density','wrong moisture-derived quantity')
        moisture=number(d.get('moisture_dry_basis_kg_kg'),0,100)
        require(d.get('water_reference_density_kg_m3')==1000 and record['conditions']['moisture_dry_basis_kg_kg']==moisture,'moisture-density conditions mismatch')
        expected=number(properties['specific_gravity']['value'])*1000*(1+moisture)
        require(close(record['value'],expected),'incorrect moisture-derived density')
    else:raise Invalid('unreviewed derivation formula')

def validate_data(data, source_data, binding, registry, root=ROOT):
    require(data.get('schema')=='solidcraft-physical-reference-profiles/1' and data.get('runtime_assignment') is False,'unknown profile schema or false runtime claim')
    require(source_data.get('schema')=='solidcraft-physical-sources/1','unknown source schema')
    require(binding.get('schema')=='solidcraft-physical-bindings/1','unknown binding schema')
    require(registry.get('units')=={q:u for q,(_,u) in QUANTITY.items()},'quantity registry changed without validator review')
    require(registry.get('conversions')=={k:{'scale':v[1],'offset':v[2]} for k,v in CONVERSIONS.items()},'unit conversion registry drift')
    require(isinstance(source_data.get('sources'),list) and 1<=len(source_data['sources'])<=256,'source count')
    sources={}
    for s in source_data['sources']:
        require(isinstance(s,dict) and isinstance(s.get('id'),str) and ID.fullmatch(s['id']) is not None and s['id'] not in sources,'invalid/duplicate source')
        text(s.get('title'),'source title',256);text(s.get('url'),'source URL')
        require(s['url'].startswith('https://') and s.get('kind')=='public_reference','source must identify public HTTPS reference')
        require(s.get('license') is None or isinstance(s['license'],str),'invalid source rights metadata')
        sources[s['id']]=s
    profiles={};require(isinstance(data.get('profiles'),list) and 1<=len(data['profiles'])<=256,'profile count')
    for p in data['profiles']:
        require(isinstance(p,dict) and isinstance(p.get('id'),str) and ID.fullmatch(p['id']) is not None and p['id'] not in profiles,'invalid/duplicate profile')
        require(p.get('family') in {'wood','polymer','glass','metal','other'},'unknown native material family')
        require(p.get('source_id') in sources and p.get('certified_design_allowables') is False,'invalid provenance or false certification')
        require(type(p.get('isotropic_reference_model')) is bool,'isotropic policy must be Boolean')
        text(p.get('designation'),'designation',256);text(p.get('reference_state'),'state',800)
        allowed={'id','designation','family','material_form','reference_state','identity_status','isotropic_reference_model','source_id','properties','limitations','certified_design_allowables','quarantined_source_ids'}
        require(set(p)<=allowed,'unknown profile field')
        require(all(x in sources for x in p.get('quarantined_source_ids',[])),'unresolved quarantined source')
        require(isinstance(p.get('properties'),list) and len(p['properties'])<=128,'property list bounds')
        seen={}
        for r in p['properties']:
            check_property(r,sources);require(r['id'] not in seen,'duplicate property ID');seen[r['id']]=r
        for r in seen.values():
            if r['qualifier']=='derived':check_derivation(r,seen)
        profiles[p['id']]=p
    expected={'catalog':('docs/render-materials/materials.json',600),'surfaces':('assets/render/starter/materials/manifest.json',48)}
    authorities={}
    for key,(path,count) in expected.items():
        spec=binding.get('inputs',{}).get(key,{})
        require(spec.get('path')==path and type(spec.get('count')) is int and spec['count']==count,'authority path/count mismatch')
        require(spec.get('sha256')==sha(root/path),'catalog changed: explicit binding review required')
        authorities[key]=read(root/path)['items'];require(len(authorities[key])==count,'authority count mismatch')
    concepts={c['id']:c for c in authorities['catalog']}
    require(len(binding.get('catalog',[]))==600 and len(binding.get('surfaces',[]))==48,'all concepts/surfaces need physical specifications')
    checked={}
    for b in binding['catalog']:
        id=b.get('catalog_id');require(id in concepts and id not in checked and b.get('name')==concepts[id]['name'],'invalid/duplicate concept binding')
        require(b.get('auto_assign_body') is False and b.get('preserve_existing_material') is True and b.get('unknown_is_zero') is False,'appearance must not overwrite host physics')
        require(b.get('physical_role') in binding.get('role_definitions',{}),'unknown physical role')
        require(isinstance(b.get('required_definition_fields'),list) and len(b['required_definition_fields'])>0,'physical requirements cannot be empty')
        refs=b.get('references');require(isinstance(refs,list) and len(refs)<=16,'reference list bounds');pairs=set()
        for ref in refs:
            require(ref.get('profile_id') in profiles,'unresolved physical reference');text(ref.get('relation'),'reference relationship',256)
            pair=(ref['profile_id'],ref['relation']);require(pair not in pairs,'duplicate reference');pairs.add(pair)
        require(b.get('status')==('reference_selection_required' if refs else 'specific_recipe_required'),'status conflicts with references')
        checked[id]=b
    surface_ids=set();originals={s['id']:s for s in authorities['surfaces']}
    for s in binding['surfaces']:
        id=s.get('surface_id');require(id in originals and id not in surface_ids,'invalid surface ID');original=originals[id]
        require(s.get('catalog_ids')==original['catalog_ids']==s.get('physical_binding_ids'),'surface physical association drift')
        require(s.get('name')==original['name'] and s.get('auto_assign_body') is False and s.get('preserve_existing_material') is True,'surface scope/name mismatch')
        first=checked[original['catalog_ids'][0]]
        require(s.get('status')==first['status'] and s.get('physical_role')==first['physical_role'],'surface status/role drift');surface_ids.add(id)
    return profiles,sources

def load():
    data,sources,binding,registry=[read(PACK/name) for name in ('profiles.json','sources.json','bindings.json','quantities.json')]
    profiles,source_map=validate_data(data,sources,binding,registry)
    return profiles,source_map,binding

def native_condition(p,r):
    c=r['conditions'];extra={k:v for k,v in c.items() if k not in {'state','temperature_k','moisture_dry_basis_kg_kg','strain_rate_per_s'} and v is not None}
    state=c['state']+' | '+r['qualifier']+' reference; source review required; not design allowables.'
    if extra:state+=' | '+json.dumps(extra,sort_keys=True,separators=(',',':'))
    if r['uncertainty'] is not None:state+=' | Reported uncertainty retained in extended profile.'
    if r['qualifier']=='derived':state+=' | Derived value; formula retained in extended profile.'
    require(len(state.encode())<=1024,'native condition overflow')
    return {k:c[k] for k in ('temperature_k','moisture_dry_basis_kg_kg','strain_rate_per_s')}|{'state':state}

def native_card(p,sources):
    selected=[];used=set();properties=p['properties'];by_q={}
    for r in properties:by_q.setdefault(r['quantity'],[]).append(r)
    def point(q):
        rows=by_q.get(q,[])
        if len(rows)!=1:return None
        r=rows[0]
        return r if type(r['value']) in (int,float) and r['qualifier'] in {'typical','derived'} and r['conditions']['direction'] in {'not_reported','isotropic'} else None
    def add(r,data,extra=()):
        selected.append({'data':data,'source_id':r['source_id'],'conditions':native_condition(p,r)})
        used.add(r['id']);used.update(extra)
    rho=point('density')
    if rho:add(rho,{'kind':'density','kg_m3':rho['value']})
    cp=point('specific_heat')
    if cp:add(cp,{'kind':'specific_heat','j_kg_k':cp['value']})
    for q in ('tensile_strength','yield_strength','compressive_strength','shear_strength'):
        r=point(q)
        if r:add(r,{'kind':q,'pa':r['value']})
    if p['isotropic_reference_model']:
        e,nu=point('young_modulus'),point('poisson_ratio')
        if e and nu:
            keys=('temperature_k','temperature_interval_k','moisture_dry_basis_kg_kg','strain_rate_per_s','state','direction','environment','thermal_regime','specimen_thickness_mm','heating_rate_k_min')
            require(e['source_id']==nu['source_id'] and all(e['conditions'].get(k)==nu['conditions'].get(k) for k in keys),'mixed elastic source/conditions')
            require(-1<number(nu['value'])<.5,'unstable isotropic Poisson ratio')
            add(e,{'kind':'elasticity','model':{'symmetry':'isotropic','young_modulus_pa':e['value'],'poisson_ratio':nu['value']}},[nu['id']])
            selected[-1]['conditions']['state']+=' | Isotropic reference model; not validated part behavior.'
            if nu['qualifier']=='derived':selected[-1]['conditions']['state']+=' Poisson ratio derived from same-source E and G, not independently measured.'
        for q,field in [('thermal_conductivity','w_m_k'),('thermal_expansion','per_k')]:
            r=point(q)
            if r:
                add(r,{'kind':q,field:[r['value']]*3,'axes':'material123'})
                selected[-1]['conditions']['state']+=' | Isotropic reference approximation, not measured directional tensor.'
    selected.sort(key=lambda r:r['data']['kind']);source_ids=sorted({r['source_id'] for r in selected} or {p['source_id']})
    card={'schema_version':1,'designation':p['designation'],'family':p['family'],
          'sources':[{'id':id,'title':sources[id]['title'],'kind':'public_reference','license':sources[id]['license'],'reference':sources[id]['url']} for id in source_ids],
          'properties':selected}
    return {'profile_id':p['id'],'card':card,
            'native_projection_status':'partial_native_card' if selected else 'extended_data_only',
            'exported_property_ids':sorted(used),
            'not_projected':[{'id':r['id'],'quantity':r['quantity'],'reason':'Retained in extended profile: unsupported native quantity, range/bound, directional measurement or incomplete constitutive model.'} for r in properties if r['id'] not in used],
            'missing_native_kinds':sorted(CORE-{r['data']['kind'] for r in selected}),
            'reference_selection_required':True,'runtime_assignment':False}

def make_exports():
    profiles,sources,bindings=load();cards=[native_card(p,sources) for p in profiles.values()]
    pin={name:sha(PACK/name) for name in ('sources.json','profiles.json','quantities.json','bindings.json')}
    data={'schema':'solidcraft-physical-card-bundle/1','native_card_schema':1,'native_library_format':False,
          'source_sha256':pin,'generator_sha256':sha(Path(__file__)),'runtime_assignment':False,'items':cards}
    coverage={'schema':'solidcraft-physical-coverage/1','profile_count':len(profiles),'source_count':len(sources),
              'property_records':sum(len(p['properties']) for p in profiles.values()),'catalog_specifications':len(bindings['catalog']),
              'concepts_with_reference_candidates':sum(bool(c['references']) for c in bindings['catalog']),
              'concepts_needing_specific_recipe':sum(not c['references'] for c in bindings['catalog']),
              'surface_specifications':len(bindings['surfaces']),
              'surfaces_with_reference_candidates':sum(s['status']=='reference_selection_required' for s in bindings['surfaces']),
              'native_property_blocks':sum(len(c['card']['properties']) for c in cards),
              'nonempty_native_cards':sum(bool(c['card']['properties']) for c in cards),
              'extended_only_profiles':sum(not c['card']['properties'] for c in cards),
              'profiles_with_point_density':sum(any(r['data']['kind']=='density' for r in c['card']['properties']) for c in cards),
              'quantities':dict(sorted(Counter(r['quantity'] for p in profiles.values() for r in p['properties']).items())),
              'runtime_assignment':False,'numerically_complete_catalog_claim':False}
    return {'native-cards.json':data,'coverage.json':coverage}

def build():
    outputs=make_exports();folder=PACK/'exports';folder.mkdir(exist_ok=True)
    for name,data in outputs.items():
        path=folder/name;require(not path.is_symlink(),'refusing symlink export');path.write_bytes(encoded(data))
    return outputs['coverage.json']

def validate():
    outputs=make_exports()
    for name,data in outputs.items():
        path=PACK/'exports'/name
        require(path.is_file() and not path.is_symlink() and path.read_bytes()==encoded(data),f'stale export: {name}; review source changes before rebuilding')
    return {'status':'pass',**outputs['coverage.json']}

def resolve(id):
    profiles,_,binding=load()
    if id.startswith('SURF-'):
        row=next((s for s in binding['surfaces'] if s['surface_id']==id),None)
        require(row is not None,'unknown surface ID');id=row['catalog_ids'][0]
    row=next((c for c in binding['catalog'] if c['catalog_id']==id),None)
    require(row is not None,'unknown catalog ID')
    return {'binding':row,'profiles':[profiles[r['profile_id']] for r in row['references']],'runtime_assignment':False}

def prepare(id,acknowledge):
    require(acknowledge,'--acknowledge-reference is required; these are not certified design allowables')
    profiles,sources,_=load();require(id in profiles,'unknown reference profile')
    p=profiles[id];result=native_card(p,sources)
    densities=[v for v in result['card']['properties'] if v['data']['kind']=='density']
    require(len(densities)==1,'no single supported density; select a reviewed point value/grade before material creation')
    name='REF '+p['designation'];require(len(name.encode())<=128,'material name too long')
    return {'schema':'solidcraft-physical-command-plan/1',
            'requires':['material.create must explicitly support physical_card; refuse older command contracts',
                        'review grade, form, temperature, moisture, process and density basis before execution',
                        'body assignment is a separate explicit operation; preserve existing appearances'],
            'executed':False,'assigns_bodies':False,'profile_id':id,
            'commands':[{'command':'material.create','params':{'name':name,'density_g_cm3':densities[0]['data']['kg_m3']/1000,'physical_card':result['card']}}],
            'extended_profile':'assets/render/physical/profiles.json#'+id,'limitations':p['limitations'],
            'omitted_native_properties':result['not_projected']}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command',choices=['build','validate','resolve','prepare']);parser.add_argument('id',nargs='?')
    parser.add_argument('--acknowledge-reference',action='store_true');args=parser.parse_args()
    try:
        if args.command=='build':result=build()
        elif args.command=='validate':result=validate()
        elif args.command=='resolve':result=resolve(args.id or '')
        else:result=prepare(args.id or '',args.acknowledge_reference)
        print(json.dumps(result,ensure_ascii=True,indent=2,allow_nan=False));return 0
    except (ValueError,OSError,TypeError,KeyError,AttributeError,RecursionError) as error:
        print(json.dumps({'status':'fail','error':str(error)}),file=sys.stderr);return 1

if __name__=='__main__':
    raise SystemExit(main())
