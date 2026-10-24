#!/usr/bin/env python3
"""Package the already reviewed native showcase without a build or renderer run.

Only the committed PNG, public provenance and a short README are distributed.
No fonts, executable, credentials, CAD internals or provisional review files are
included. A passing package check is not new renderer execution evidence.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
from publish_selene import EXPORT, MANIFEST, MODEL, MODEL_SHA, digest, load, require, resource_receipts, verify_png


def package(output: Path) -> dict:
    output = output.resolve()
    require(not output.exists(), 'Use a new package directory; existing artifacts are never overwritten')
    manifest = load(MANIFEST)
    require(digest(MODEL) == MODEL_SHA == manifest['source']['sha256'], 'The native editable authority changed')
    require(manifest['visual_review']['status'] == 'ACCEPTED', 'Only the chosen reviewed export can be packaged')
    width, height = manifest['output']['width'], manifest['output']['height']
    require(type(width) is int and type(height) is int and 1 <= width <= 8192 and 1 <= height <= 8192
            and width*height <= 16_777_216, 'Unbounded recorded dimensions')
    require(EXPORT.stat().st_size <= width*height*8+65_536, 'Unexpected encoded PNG size')
    verified = verify_png(EXPORT.read_bytes(), width, height)
    require(verified['sha256'] == manifest['output']['sha256'], 'Committed PNG differs from the accepted native frame')
    require(resource_receipts(manifest['request']) == manifest['authored_resource_receipts'], 'Original authored resource identity changed')
    output.mkdir(parents=True, exist_ok=False)
    files = [(EXPORT, 'SELENE-studio-4k.png'), (MANIFEST, 'SELENE-studio-4k-manifest.json')]
    sums = []
    for source, name in files:
        destination = output/name
        with destination.open('xb') as target, source.open('rb') as stream:
            shutil.copyfileobj(stream, target, length=1024*1024)
        require(digest(destination) == digest(source), 'Copied artifact changed')
        sums.append(digest(destination)+'  '+name)
    readme = '''# SELENE native studio render

This package contains the exact completed SolidCraft GPU-path PNG and its source,
renderer, resource and request manifest. It is a 3840 x 2400 native render with
1,024 samples per pixel, maximum path depth 16, ACES-fitted tone mapping and
edge-aware a-trous denoising. The functional master is unchanged: 2,116 native
bodies, 2,094 visible bodies and 2,144,099 visible subject triangles. One passive
ground plane is added only to the captured render scene.

The original orange/charcoal identity, geometry, pose and saved visibility are
preserved. Glass and authored RGB materials are not measured optical datasets.
Some residual Monte Carlo grain remains in glass/vial regions at native pixel
scale. No image generation, painting, resampling, geometry simplification or
engineering-property reassignment was used. This is not a dynamics certificate
or complete GUI/CI release approval.

The manifest's repository-relative paths refer to 0xlillith/solidcraft, branch
lab/render-scale-resources. Reproduction tools are in tools/render-library and
the native master/provenance in assets/render/selene. The manifest pins the exact
renderer executable source and bytes used for this image; packaging does not
rerender it or imply validation of later code changes.

Credits: 0xlillith (original rover), SychPL / Mateusz Sych (showcase assembly),
SolidCraft contributors (native enhancements, rendering and studio authoring),
and Canonical Ltd. (modeled lettering). New contributions use MIT OR Apache-2.0;
underlying source and Ubuntu Font Licence provenance remains in
assets/render/selene/SOURCE.md. No font files are included in this package.
'''
    (output/'README.md').write_text(readme, encoding='utf-8', newline='\n')
    sums.append(digest(output/'README.md')+'  README.md')
    (output/'SHA256SUMS.txt').write_text('\n'.join(sums)+'\n', encoding='utf-8', newline='\n')
    return {'status': 'PASS', 'scope': 'package byte/source/resource verification only; no new renderer execution',
            'output': str(output), 'png_sha256': verified['sha256'], 'files': [name for _, name in files]+['README.md', 'SHA256SUMS.txt']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(package(args.out), indent=2))
