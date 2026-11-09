#!/usr/bin/env python3
"""Export chosen reference materials through a trusted card-capable SolidCraft CLI.

A fresh command-line session creates no geometry or body assignments. The native
application owns library-v2 serialization and content revisions. All output stays
in a new review directory under target/. Older scalar-only executables refuse
before any material is created. Public reference data is not design certification.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

import physical as p

MAX_OUTPUT = 16 * 1024 * 1024
PACK_ID = "tangency.physical-references"
SOURCE_FILES = ("profiles.json", "sources.json", "bindings.json", "quantities.json",
                "exports/native-cards.json", "tools/physical.py", "tools/export_library.py")


def input_pins() -> dict[str, str]:
    return {name: p.sha(p.PACK/name) for name in SOURCE_FILES}


def selection(ids: list[str] | None, acknowledge: bool) -> dict:
    p.require(acknowledge, "Explicit --acknowledge-reference is required")
    before = input_pins()
    p.validate()
    profiles, sources, _ = p.load()
    selected, omitted = [], []
    names = sorted(profiles) if ids is None else ids
    p.require(isinstance(names, list) and 0 < len(names) <= 256, "Select 1..256 profiles")
    p.require(all(isinstance(name, str) for name in names) and len(names) == len(set(names)), "Duplicate or invalid selected profile")
    for id in names:
        p.require(id in profiles, f"Unknown profile: {id}")
        native = p.native_card(profiles[id], sources)
        density = next((row for row in native['card']['properties'] if row['data']['kind'] == 'density'), None)
        if density is None:
            omitted.append({'profile_id': id, 'reason': 'No supported point density; no nominal midpoint or unrelated substrate is substituted.'})
            continue
        selected.append({'profile_id': id, 'material_form': profiles[id]['material_form'],
                         'command': p.prepare(id, True)['commands'][0],
                         'extended_property_ids': [row['id'] for row in native['not_projected']]})
    p.require(selected, "No selected reference has a supported point density")
    p.require(before == input_pins(), "Source revision changed during plan preparation")
    return {'schema': 'solidcraft-physical-library-plan/1', 'executed': False,
            'reference_selection_required': True, 'assigns_bodies': False,
            'id': PACK_ID, 'input_sha256': before, 'materials': selected, 'omitted': omitted,
            'preserve_extended_profiles': True,
            'required_contract': 'material.create physical_card, material.inspect, solidcraft-library/2'}


def registry_check(registry: object) -> None:
    p.require(isinstance(registry, list) and len(registry) <= 4096, "Invalid native command registry")
    p.require(all(isinstance(row, dict) and isinstance(row.get('id'), str) for row in registry), "Malformed native command record")
    commands = {row['id']: row for row in registry}
    p.require(len(commands) == len(registry), "Duplicate command ID")
    for name in ('file.new', 'material.create', 'material.inspect', 'library.export', 'library.inspect'):
        p.require(name in commands, f"Native command unavailable: {name}")
    p.require(all(isinstance(commands[name].get('params'), str) for name in ('material.create', 'library.export')), 'Malformed native parameter documentation')
    p.require('physical_card' in commands['material.create'].get('params', ''),
              "This executable cannot preserve physical cards; scalar material creation is refused")
    p.require('solidcraft-library/2' in commands['library.export'].get('params', ''),
              "Native library-v2 export is required; scalar export would lose engineering data")


def invoke(cli: Path, arguments: list[str]) -> object:
    # Capture into temporary files, then enforce the JSON byte budget before reading.
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        result = subprocess.run([str(cli), *arguments], cwd=p.ROOT, stdout=out, stderr=err, timeout=180)
        p.require(out.tell() <= MAX_OUTPUT and err.tell() <= MAX_OUTPUT, "CLI output exceeds the diagnostic budget")
        err.seek(0)
        p.require(result.returncode == 0, 'Native command failed: ' + err.read(2048).decode(errors='replace'))
        out.seek(0)
        return json.load(out, object_pairs_hook=p.unique, parse_constant=p.reject_constant)


def verify_catalog(catalog: object, plan: dict) -> None:
    p.require(isinstance(catalog, dict) and catalog.get('format') == 'solidcraft-library/2', "Native export did not produce library v2")
    p.require(catalog.get('id') == PACK_ID, "Wrong native library identity")
    entries = catalog.get('entries')
    p.require(isinstance(entries, list) and len(entries) == len(plan['materials']), "Missing or extra native materials")
    expected = {row['command']['params']['name']: row['command']['params'] for row in plan['materials']}
    found = set()
    for entry in entries:
        p.require(isinstance(entry, dict), "Invalid native entry")
        definition = entry.get('definition', {})
        p.require(isinstance(definition, dict) and definition.get('kind') == 'material', "Unexpected appearance entry")
        value = definition.get('value', {})
        p.require(isinstance(value, dict), "Invalid material definition")
        name = value.get('name')
        p.require(name in expected and name not in found, "Unknown or duplicate exported material")
        found.add(name)
        reference = expected[name]
        p.require(value.get('density_g_cm3') == reference['density_g_cm3'], "Assigned reference density changed")
        p.require(value.get('physical_card') == reference['physical_card'], "A physical card, source or condition was changed or dropped")
        p.require(value.get('appearance') is None, "Physical export must not assign a visual appearance")
        revision = entry.get('revision', '')
        p.require(isinstance(revision, str) and len(revision) == 64 and all(c in '0123456789abcdef' for c in revision), "Invalid native revision")
    # Revisions are subsequently verified by the real library.inspect command,
    # not a parallel Python approximation of serde's ordering/float formatting.


def library_command(path: Path) -> dict:
    return {'command': 'library.export', 'params': {
        'path': str(path), 'id': PACK_ID, 'name': 'Physical references - select grade and conditions',
        'format': 'solidcraft-library/2', 'overwrite': False,
        'provenance': {'license': 'MIT OR Apache-2.0', 'author': 'SolidCraft contributors',
                       'source_url': 'https://github.com/0xlillith/solidcraft/tree/assets/material-physical-properties/assets/render/physical'}}}


def export(cli: Path, plan: dict) -> dict:
    cli = cli.resolve(strict=True)
    p.require(cli.is_file(), "Trusted CLI executable not found")
    p.require(plan.get("input_sha256") == input_pins(), "Stale physical reference plan")
    registry_check(invoke(cli, ['commands']))
    # No live app, existing document, source profile, output file or library is touched.
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')
    folder = p.ROOT / 'target/physical-library-exports' / stamp
    p.require(not folder.exists(), "Review output already exists")
    folder.mkdir(parents=True, exist_ok=False)
    output, cold = folder/'physical-reference-library.json', folder/'cold-reopened-library.json'
    design = folder/'physical-reference-library-design.json'
    script = folder/'create.json'
    commands = [{'command': 'file.new', 'params': {'name': 'Physical reference library'}}]
    commands += [row['command'] for row in plan['materials']]
    commands.append(library_command(output))
    script.write_bytes(p.encoded({'commands': commands}))
    before = input_pins()
    p.require(before == plan['input_sha256'], 'Source changed after native preflight')
    invoke(cli, ['run', str(script), '--results', '--save', str(design)])
    catalog = p.read(output)
    verify_catalog(catalog, plan)
    inspected = invoke(cli, ['exec', 'library.inspect', json.dumps({'path': str(output)})])
    p.require(isinstance(inspected, dict) and inspected.get('catalog') == catalog, "Native library inspection disagrees with its export")
    # New process, cold document load and native re-export must preserve every card.
    reopen = folder/'reopen.json'
    reopen.write_bytes(p.encoded({'commands': [library_command(cold)]}))
    invoke(cli, ['run', str(reopen), '--in', str(design), '--results'])
    restored = p.read(cold)
    verify_catalog(restored, plan)
    p.require(restored == catalog, "Cold reopen changed the native material library")
    p.require(before == input_pins(), "Source revision changed during library export")
    receipt = {'schema': 'solidcraft-physical-library-export-receipt/1', 'status': 'pass',
               'cli_sha256': p.sha(cli), 'input_sha256': before, 'library_sha256': p.sha(output),
               'material_count': len(plan['materials']), 'omitted': plan['omitted'],
               'native_inspection': True, 'cold_reopen': True, 'body_assignment': False,
               'engineering_certification': False, 'extended_data_embedded': False,
               'library': str(output.relative_to(p.ROOT))}
    (folder/'receipt.json').write_bytes(p.encoded(receipt))
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', action='append', help='Exact reference ID; default selects all point-density references')
    parser.add_argument('--acknowledge-reference', action='store_true')
    parser.add_argument('--cli', type=Path, help='Trusted card-capable executable; absent means a non-executing JSON plan only')
    args = parser.parse_args()
    try:
        plan = selection(args.profile, args.acknowledge_reference)
        result = export(args.cli, plan) if args.cli else plan
        print(json.dumps(result, indent=2, ensure_ascii=True, allow_nan=False))
        return 0
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(json.dumps({'status': 'fail', 'error': str(error)}), file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
