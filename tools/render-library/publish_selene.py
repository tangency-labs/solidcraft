#!/usr/bin/env python3
"""Publish one explicitly reviewed native SELENE showcase export, never drafts.

Requires an existing completed Studio receipt and an unchanged native authority.
Checks exact PNG bytes, bounded scanlines, CRCs, dimensions, captured geometry,
resource hashes and the actual executable identity. Does not run a renderer,
rewrite a model, resample/repaint pixels, or publish to a remote repository.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct
import zlib

ROOT = Path(__file__).resolve().parents[2]
MODEL = ROOT / 'assets/render/selene/models/SELENE-functional.solidcraft'
MODEL_SHA = '3bd2bfa1c6336394c4a4208d30b52c80d06658a3bd7ea57cc93b774411d1bd68'
EXPORT = ROOT / 'assets/render/selene/exports/SELENE-studio-4k.png'
MANIFEST = ROOT / 'assets/render/selene/scenes/studio-hero.json'


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def digest(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def load(path: Path) -> dict:
    require(path.stat().st_size <= 16*1024*1024, 'Metadata exceeds its size bound')
    value = json.loads(path.read_text(encoding='utf-8'))
    require(isinstance(value, dict), 'Metadata must be a JSON object')
    return value


def verify_png(data: bytes, width: int, height: int) -> dict:
    require(type(width) is int and type(height) is int and 1 <= width <= 8192 and 1 <= height <= 8192,
            'Invalid expected dimensions')
    require(width*height <= 16_777_216 and len(data) <= width*height*8+65_536, 'PNG exceeds its byte/pixel envelope')
    require(data.startswith(b'\x89PNG\r\n\x1a\n'), 'Invalid PNG signature')
    offset, chunks, compressed = 8, [], []
    while offset < len(data):
        require(offset+12 <= len(data), 'Truncated PNG chunk header')
        length = struct.unpack('>I', data[offset:offset+4])[0]
        require(length <= len(data)-offset-12, 'Truncated PNG chunk data')
        kind, content = data[offset+4:offset+8], data[offset+8:offset+8+length]
        actual_crc = struct.unpack('>I', data[offset+8+length:offset+12+length])[0]
        require(zlib.crc32(kind+content) & 0xffffffff == actual_crc, 'PNG chunk CRC mismatch')
        if not chunks:
            require(kind == b'IHDR' and length == 13, 'IHDR must be the first chunk')
            require(struct.unpack('>IIBBBBB', content) == (width, height, 8, 6, 0, 0, 0),
                    'Expected native RGBA8, noninterlaced output at the requested resolution')
        elif kind == b'IHDR':
            raise ValueError('Duplicate IHDR')
        if kind == b'IDAT':
            compressed.append(content)
        if kind == b'acTL':
            raise ValueError('Animated images cannot substitute for a completed still')
        if kind == b'IEND':
            require(length == 0 and offset+12 == len(data), 'Invalid IEND or trailing PNG bytes')
        chunks.append(kind)
        offset += length+12
    require(chunks[-1:] == [b'IEND'] and compressed and b'sRGB' in chunks, 'Incomplete native sRGB PNG')
    expected = height*(1+4*width)
    decoder = zlib.decompressobj()
    count = 0
    for content in compressed:
        decoded = decoder.decompress(content, expected-count+1)
        count += len(decoded)
        require(count <= expected and not decoder.unconsumed_tail, 'PNG expands beyond its declared image')
        require(not decoder.unused_data, 'Trailing compressed image stream')
    count += len(decoder.flush(expected-count+1))
    require(decoder.eof and count == expected, 'Truncated or incorrect PNG scanline payload')
    return {'width': width, 'height': height, 'rgba_bits_per_channel': 8, 'color_encoding': 'sRGB',
            'crc_and_bounded_scanline_verification': 'PASS', 'bytes': len(data),
            'sha256': hashlib.sha256(data).hexdigest()}


def resource_receipts(request: dict) -> list[dict]:
    surfaces = {item['id']: item for item in load(ROOT/'assets/render/starter/materials/manifest.json')['items']}
    environments = {item['id']: item for item in load(ROOT/'assets/render/starter/environments/manifest.json')['items']}
    result = {}
    for binding in request['raytrace']['textures']:
        for field in ('base_color_srgb', 'normal_opengl', 'roughness_linear', 'metallic_linear'):
            uri = binding.get(field)
            if uri is None:
                continue
            match = re.fullmatch(r'builtin://(SURF-[0-9]{3})/(base_color|normal_open_gl|roughness|metallic)', uri)
            require(match is not None, 'Publication requires an explicit original built-in surface source')
            rid, role = match.groups()
            source = surfaces[rid]['maps'][role]
            path = ROOT/source['path']
            require(digest(path) == source['sha256'], 'Authored map no longer matches its manifest')
            result[uri] = {'id': rid, 'role': role, 'path': source['path'], 'sha256': source['sha256']}
    env = request['environment']
    require(env['kind'] == 'builtin_hdr', 'This showcase recipe requires a built-in authored HDR')
    source = environments[env['id']]
    path = ROOT/'assets/render/starter/environments'/source['path']
    require(digest(path) == source['sha256'], 'Authored HDR no longer matches its manifest')
    result['environment'] = {'id': env['id'], 'role': 'environment_radiance', 'path': path.relative_to(ROOT).as_posix(),
                             'sha256': source['sha256']}
    return sorted(result.values(), key=lambda item: (item['id'], item['role']))


def publish(session: Path, name: str, visual_review: bool) -> dict:
    require(visual_review, 'Inspect the full image and native-pixel crops before publication; pass --accept-visual-review explicitly')
    require(re.fullmatch(r'[a-z0-9][a-z0-9-]{0,80}', name) is not None, 'Invalid completed-render name')
    session = session.resolve()
    receipt = load(session/(name+'-receipt.json'))
    provenance = load(session/'session.json')
    scene = load(session/'scene.json')
    health = load(session/'native-model-health.json')
    request = load(session/(name+'-request.json'))
    require(receipt['status'] == 'PASS' and receipt['source_model_unchanged'], 'Only a verified completed native render can be published')
    require(receipt['params'] == request, 'Request changed after rendering')
    require(digest(MODEL) == MODEL_SHA == receipt['source_model_sha256'] == provenance['model_sha256'], 'Native editable authority changed')
    require(health['evaluated_bodies'] == 2116 and not health['timeline_issues'], 'Unexpected native model health')
    require(receipt['cli_sha256'] == provenance['cli_sha256'], 'Executable identity mismatch')
    frame = receipt['export']['frame']
    preflight = receipt['preflight']
    require(frame['executor'] == 'gpu-path' and preflight['resolved']['executor'] == 'gpu-path', 'Unexpected native executor')
    require((frame['width'], frame['height']) == (request['settings']['width'], request['settings']['height']), 'Output was resized')
    require(frame['scene_generation'] == preflight['scene_generation'], 'Frame belongs to a different capture')
    for field in ('samples', 'max_depth', 'seed', 'tone_map', 'denoise'):
        require(preflight['options'][field] == request[field], 'Native execution options differ from the authored request: '+field)
    require(preflight['options']['exposure_ev'] == request['settings']['exposure_ev'], 'Native exposure differs from the authored request')
    require(preflight['transparency'] == 'physical_transmission', 'The selected optical request was not physically dispatched')

    require(frame['job'] == receipt['export']['job'], 'Frame/job identity mismatch')
    triangles = sum(body['triangles'] for body in scene['bodies'])
    require(preflight['admission']['triangles'] == triangles+2*len(request['settings']['stage']), 'Unexpected geometry reduction or replacement')
    image = session/(name+'.png')
    require(not image.is_symlink() and image.resolve().parent == session, 'Image is not the native session export')
    require(image.stat().st_size == frame['png_bytes'] == receipt['export']['bytes'], 'Export byte count mismatch')
    require(type(frame['width']) is int and type(frame['height']) is int
            and 1 <= frame['width'] <= 8192 and 1 <= frame['height'] <= 8192
            and frame['width']*frame['height'] <= 16_777_216, 'Unbounded recorded image dimensions')
    require(image.stat().st_size <= frame['width']*frame['height']*8+65_536, 'Encoded PNG exceeds the native envelope before reading')
    verified = verify_png(image.read_bytes(), frame['width'], frame['height'])
    require(verified['sha256'] == frame['png_sha256'], 'Export hash differs from the completed frame')
    require(not EXPORT.exists() and not MANIFEST.exists(), 'Never overwrite an existing showcase without a separate reviewed replacement')
    recipe = ROOT/'tools/render-library/selene_recipe.py'
    manifest = {
        'schema': 'solidcraft-native-selene-showcase/1', 'title': 'SELENE — full-detail native studio render',
        'source': {'path': MODEL.relative_to(ROOT).as_posix(), 'sha256': MODEL_SHA,
                   'functional_master_commit': 'cf8e651022deee528b26c303fde58bf3eb8a22a5',
                   'original_author': '0xlillith', 'showcase_author': 'SychPL / Mateusz Sych',
                   'provenance': 'assets/render/selene/SOURCE.md', 'native_file_modified': False},
        'renderer': {'binary_source_commit': provenance['source_commit'], 'binary_source_tree': provenance['source_tree'],
                     'executable_sha256': provenance['cli_sha256'], 'executor': 'gpu-path',
                     'device_policy': 'hardware', 'backend_filter': 'Vulkan'},
        'authoring': {'recipe': recipe.relative_to(ROOT).as_posix(),
                      'recipe_sha256_lf_normalized': hashlib.sha256(recipe.read_text(encoding='utf-8').encode('utf-8')).hexdigest(),
                      'rig': 'RIG-002', 'rig_variant': 'complete', 'rig_radiance_gain': 1/16,
                      'units': 'millimeters and radians; authored relative linear-sRGB light radiance',
                      'camera_fit': 'all captured body AABB corners; 8 percent frame margin'},
        'geometry': {'evaluated_native_bodies': health['evaluated_bodies'], 'visible_native_bodies': len(scene['bodies']),
                     'native_triangles': triangles, 'passive_stage_panels': len(request['settings']['stage']),
                     'render_triangles': preflight['admission']['triangles'], 'original_visibility_preserved': True,
                     'simplification': 'none', 'engineering_property_reassignment': 'none'},
        'output': {'path': EXPORT.relative_to(ROOT).as_posix(), **verified},
        'request': request, 'authored_resource_receipts': resource_receipts(request),
        'visual_review': {'status': 'ACCEPTED', 'reviewer': 'agent',
                          'scope': 'whole-frame composition plus native-pixel detail crops; not quantitative convergence certification'},
        'limitations': ['Residual Monte Carlo grain remains visible at native pixel scale in some glass/vial regions; the denoiser does not reconstruct those optical paths.',
                       'Authored RGB material/absorption values are not measured optical or engineering data.',
                       'Normal maps affect shading, not CAD silhouette or geometric displacement.',
                       'LED diffusers are transmission surfaces, not validated volumetric or mesh-emission sources.',
                       'The image is a static pose, not a dynamics or collision-envelope certification.',
                       'No cross-driver bit-identical reproduction or full GUI/CI approval is implied.'],
    }
    # Preserve the exact completed image bytes. This is a chosen shipped export,
    # not a screenshot, a reduced display texture or an image-generator substitute.
    encoded_manifest = json.dumps(manifest, ensure_ascii=False, indent=2, allow_nan=False)+'\n'
    created_image = False
    created_manifest = False
    try:
        with EXPORT.open('xb') as destination:
            created_image = True
            with image.open('rb') as source:
                shutil.copyfileobj(source, destination, length=1024*1024)
        require(digest(EXPORT) == verified['sha256'], 'Copied production export changed')
        with MANIFEST.open('x', encoding='utf-8', newline='\n') as destination:
            created_manifest = True
            destination.write(encoded_manifest)
    except BaseException:
        # Remove only artifacts this call created, never a preexisting export.
        if created_image:
            EXPORT.unlink(missing_ok=True)
        if created_manifest:
            MANIFEST.unlink(missing_ok=True)
        raise
    return {'status': 'PUBLISHED_LOCALLY', 'image': str(EXPORT), 'manifest': str(MANIFEST), **verified}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--session', type=Path, required=True)
    parser.add_argument('--render-name', default='selene-studio-4k')
    parser.add_argument('--accept-visual-review', action='store_true')
    args = parser.parse_args()
    print(json.dumps(publish(args.session, args.render_name, args.accept_visual_review), indent=2))
