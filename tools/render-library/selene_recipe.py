"""Original reproducible studio recipe for the pinned full-detail functional SELENE.

Import alongside selene_studio.Studio. Preparation uses public command adapters,
never native JSON edits, hidden geometry reduction or measured-property guesses.
The final request and recipe receipts describe authored appearance choices.
"""
from __future__ import annotations
import copy,json,math


def build(studio, width=800, height=500, samples=128):
    if not (isinstance(width,int) and isinstance(height,int) and 1<=width<=8192 and 1<=height<=8192 and width*height<=16_777_216):
        raise ValueError('Image dimensions exceed the native GPU envelope')
    if not isinstance(samples,int) or not 1<=samples<=4096:
        raise ValueError('Samples must be an integer in 1..4096')
    scene=studio.scene
    bodies=scene['bodies']
    lo=scene['bounds']['min'];hi=scene['bounds']['max']
    if isinstance(lo,dict):lo=[lo[k] for k in ('x','y','z')];hi=[hi[k] for k in ('x','y','z')]
    center=[(a+b)/2 for a,b in zip(lo,hi)]
    extent=max(b-a for a,b in zip(lo,hi))
    ground={'id':'studio-ground','center_mm':[center[0],center[1],lo[2]-0.1],
            'half_u_mm':[extent*2.5,0,0],'half_v_mm':[0,extent*2.5,0],
            'reflectance_linear':[0.18,0.19,0.21]}
    glass=[]
    maps=[]
    bindings=[]
    for b in bodies:
        if b['opacity'] < 1:
            name=b['name'];kind='diffuser' if 'diffuser' in name.lower() else 'lens' if 'lens' in name.lower() else 'vial' if 'vial' in name.lower() else 'window'
            # Full closed solid validation remains in the actual native executor.
            # These RGB absorption choices are authored, not measured glass spectra.
            sigma={'vial':[0.002,0.0006,0.0004],'lens':[0.008,0.003,0.001],
                   'window':[0.0002,0.0002,0.0002],'diffuser':[0.015,0.003,0.001]}[kind]
            glass.append({'body_index':b['ordinal'],'body_name':name,'ior':1.49 if kind=='diffuser' else 1.5,
                          'absorption_per_mm':sigma,'boundary':'closed_mesh','thickness_mm':0.0,'shadow_transmission':True})
        name=b['name'].lower()
        surface=None;preserve_palette=False
        metal_names=('Structural electronics tub','Arm structural nose','Rear science outrigger','Sample compartment','Unified sensor crossbeam')
        paint_prefix=('Shoulder link ','Elbow link ','Distal wrist link ','Orange solar frame ','Payload rail ','Orange side service strip ','18650 removable battery tray')
        if b['opacity'] == 1:
            if b['name'] in metal_names or b['name'].startswith(('Solar outrigger beam ','Suspension chassis shoe ')):
                surface='SURF-007'
            elif b['name'] in ('Raspberry Pi 5 PCB','Battery management board'):
                surface='SURF-036';preserve_palette=True
            elif b['name'].startswith(paint_prefix) and b['color']==[233,137,53]:
                surface='SURF-015';preserve_palette=True
            elif b['name'].startswith(('Forearm cross brace ','Sensor rear cover')):
                surface='SURF-014';preserve_palette=True
            elif b['name'].startswith('TPU wheel tyre '):
                surface='SURF-024';preserve_palette=True
            elif b['name']=='Basalt pebble':
                surface='SURF-047'
        if surface:
            bounds=b['bounds'];mn=bounds['min'];mx=bounds['max']
            if isinstance(mn,dict):mn=[mn[k] for k in ('x','y','z')];mx=[mx[k] for k in ('x','y','z')]
            axis=min(range(3),key=lambda i:mx[i]-mn[i]);plane=['yz','zx','xy'][axis]
            prepared=studio.call('render.library.surface.prepare',{'id':surface,'body_index':b['ordinal'],'body_name':b['name'],'plane':plane})
            texture=prepared['texture']
            if preserve_palette:texture.pop('base_color_srgb',None)
            maps.append(texture)
            bindings.append({'body':b['name'],'surface':surface,'plane':plane,'preserve_native_palette':preserve_palette,
                             'projection':'static-world planar appearance mapping; no geometric displacement'})
    if len(maps)>128:raise ValueError('Explicit reviewed material targets exceed 128 bindings')
    if len(glass)>32:raise ValueError('Native transparent body count exceeds the explicitly admitted contract')
    # The original orthographic hero camera is kept initially for full-master comparison.
    camera={'target':[87.0,0.0,145.0],'yaw':-0.66,'pitch':0.53,'roll':0.0,'distance':630.0,'fov':0.0}
    # HDR alone owns illumination for the initial beauty-light assessment.
    params={'progressive':True,'samples':samples,'max_depth':12,'seed':21026,'tone_map':'aces_fitted','denoise':True,
            'settings':{'width':width,'height':height,'camera':camera,'background':[232,236,242],
                        'exposure_ev':-2.0,'stage':[ground]},
            'environment':{'kind':'builtin_hdr','id':'HDR-002','rotation_rad':0.35,'intensity':1.0},
            'lighting':{'environment':[0.0,0.0,0.0],'area_lights':[],'sun':None},
            'raytrace':{'dielectrics':glass,'clear_coats':[],'textures':maps}}
    recipe={'scope':'Native full visible functional SELENE; authored optical appearance only',
            'master_visibility_preserved':True,'model_geometry_or_engineering_properties_changed':False,
            'glass_body_count':len(glass),'mapped_bodies':bindings,
            'limitations':['RGB glass absorption is an authored illustrative value, not a measured optical dataset.',
                'The LED diffusers are transmission surfaces; no mesh emission or volumetric scattering is claimed.',
                'Planar normal maps change shading only, not silhouette or CAD geometry.'],
            'request':params}
    studio.write('hero-recipe.json',recipe)
    print(json.dumps({'glass_bodies':len(glass),'mapped_body_count':len(bindings),'surface_ids':sorted({x['surface'] for x in bindings}),'bounds':scene['bounds']}),flush=True)
    return params


def fit_camera(scene, aspect, yaw=-0.66, pitch=0.53, fov=0.46, margin=0.08):
    """Original analytic fit of captured body AABB corners, excluding stage/lights.
    Derives the smallest distance at a fixed target/rotation satisfying a frame
    margin for every captured corner. No renderer projection code is invoked.
    """
    import itertools
    if not all(math.isfinite(x) for x in (aspect,yaw,pitch,fov,margin)) or aspect<=0 or not 0<fov<math.pi or not 0<margin<0.4:
        raise ValueError('Finite positive aspect/FOV and a margin inside 0..0.4 required')
    if not scene.get('bodies'):
        raise ValueError('Camera fit requires captured visible bodies')
    lo,hi=scene['bounds']['min'],scene['bounds']['max']
    if len(lo)!=3 or len(hi)!=3 or not all(math.isfinite(a) and math.isfinite(b) and a<=b for a,b in zip(lo,hi)):
        raise ValueError('Invalid captured scene bounds')
    target=[(a+b)/2 for a,b in zip(lo,hi)]
    right=[math.cos(yaw),-math.sin(yaw),0.0]
    back=[-math.sin(yaw)*math.cos(pitch),-math.cos(yaw)*math.cos(pitch),math.sin(pitch)]
    up=[back[1]*right[2]-back[2]*right[1],back[2]*right[0]-back[0]*right[2],back[0]*right[1]-back[1]*right[0]]
    tan=math.tan(fov/2);extent=1-2*margin
    distance=0.0;corners=[]
    for body in scene['bodies']:
        bounds=body['bounds']
        if not all(math.isfinite(a) and math.isfinite(b) and a<=b for a,b in zip(bounds['min'],bounds['max'])):
            raise ValueError('Invalid captured body bounds')
        for point in itertools.product(*zip(bounds['min'],bounds['max'])):
            d=[p-t for p,t in zip(point,target)]
            x=sum(a*b for a,b in zip(d,right));y=sum(a*b for a,b in zip(d,up));z=sum(a*b for a,b in zip(d,back))
            distance=max(distance,z+max(abs(x)/(tan*aspect*extent),abs(y)/(tan*extent)))
            corners.append((x,y,z))
    distance*=1.002
    maximum=max(max(abs(x)/((distance-z)*tan*aspect),abs(y)/((distance-z)*tan)) for x,y,z in corners)
    assert maximum<=extent and math.isfinite(distance)
    print('CAMERA_FIT',json.dumps({'body_bounds':len(scene['bodies']),'margin':margin,'distance_mm':distance,'maximum_ndc':maximum}),flush=True)
    return {'target':target,'yaw':yaw,'pitch':pitch,'roll':0.0,'fov':fov,'distance':distance}


def hero_request(studio, width=3840, height=2400, samples=1024):
    """The reviewed perspective three-area-light/HDR-fill variant, authored RGB.

    The 1/16 rig radiance gain is an explicit creative adjustment, not a claim
    that the original sphere-calibration exposure is preserved. All geometry,
    alpha-free optical body identity, original pose and visibility are retained.
    """
    params=build(studio,width,height,samples)
    params['settings']['camera']=fit_camera(studio.scene,width/height)
    lo,hi=studio.scene['bounds']['min'],studio.scene['bounds']['max']
    center=[(a+b)/2 for a,b in zip(lo,hi)]
    extent=max(b-a for a,b in zip(lo,hi))
    rig=studio.call('render.library.studio.prepare',{
        'id':'RIG-002','variant':'complete','center_mm':center,'subject_extent_mm':extent})
    if rig.get('stage'):
        raise ValueError('This reviewed rig is emitter-only by construction; do not silently omit passive cards')
    lighting=copy.deepcopy(rig['lighting'])
    clearance=[]
    for light in lighting['area_lights']:
        low=[c-abs(u)-abs(v) for c,u,v in zip(light['center'],light['u'],light['v'])]
        high=[c+abs(u)+abs(v) for c,u,v in zip(light['center'],light['u'],light['v'])]
        separate=any(a>h or b<l for a,b,l,h in zip(low,high,lo,hi))
        if not separate:
            raise ValueError('The analytic emitter has no conservative clearance from the subject')
        clearance.append({'emitter_min':low,'emitter_max':high,'disjoint_subject_aabb':True})
        light['radiance']=[value/16.0 for value in light['radiance']]
    params.update(lighting=lighting,max_depth=16)
    params['environment'].update(id='HDR-002',intensity=0.12)
    params['settings']['exposure_ev']=-1.1
    params['settings']['stage'][0]['reflectance_linear']=[0.32,0.34,0.38]
    studio.write('hero-lighting-recipe.json',{
        'rig':'RIG-002','variant':'complete','source_projection':rig['projection'],
        'relative_radiance_gain':1/16,'hdr':'HDR-002','hdr_intensity':0.12,
        'emitter_clearance':clearance,'camera_fit':'all captured body AABB corners; 8% frame margin',
        'manual_exposure_ev':-1.1,'measured_photometry':False,'request':params})
    return params
