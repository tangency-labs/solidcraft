#!/usr/bin/env python3
"""Author bundled command pictures using an isolated running SolidCraft window.

Python and ImageMagick are authoring tools only, never application dependencies.
"""
import argparse
from collections import Counter
from contextlib import contextmanager
import json
import math
import os
from pathlib import Path
import socket
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

REPO = Path(__file__).resolve().parents[3]
SIZE = (960, 600)
# Keep the connection alive past the control server's 3600-second batch wait.
BATCH_TIMEOUT = 3720
PENDING_TIMEOUT = 3720
CAPTURE_METADATA = 'io.tangencylabs.command-hints'


class Control:
    def __init__(self, port, token_dir, anchor):
        self.port = port
        self.token = (token_dir / f'{port}.token').read_text().strip()
        self.anchor = anchor.resolve(strict=True)
        self.pending_work = False

    def preflight(self):
        ensure_idle(self)
        documents = self.call('ui.documents')
        document = self.call('document.inspect')
        ui = self.call('ui.inspect')
        tabs = documents.get('documents', [])
        saved = document.get('path')
        if (len(tabs) != 1 or documents.get('active') != 0
                or tabs[0].get('dirty') is not False or document.get('dirty') is not False
                or not saved or Path(saved).resolve() != self.anchor
                or not document.get('timeline') or document.get('active_sketch') is not None):
            raise RuntimeError('Capture requires only the expected saved, clean, nonempty anchor document')
        if (documents.get('closing') is not None or documents.get('quitting')
                or documents.get('batch') or documents.get('read_only')
                or documents.get('open_elsewhere') or documents.get('changed_on_disk')
                or ui.get('tool') is not None or ui.get('dialog') is not None or ui.get('windows')
                or ui.get('file_busy') or ui.get('file_picker_busy')
                or ui.get('quit_requested') or ui.get('quitting')
                or ui.get('preview', {}).get('active') or ui.get('preview', {}).get('busy')):
            raise RuntimeError('Capture window has pending work; refusing to change it')

    def call(self, method, params=None):
        timeout = BATCH_TIMEOUT if method == 'engine.batch' else 120
        with socket.create_connection(('127.0.0.1', self.port), timeout=timeout) as sock:
            with sock.makefile() as stream:
                def request(name, values, identifier):
                    message = {'id': identifier, 'token': self.token, 'method': name, 'params': values}
                    sock.sendall((json.dumps(message) + '\n').encode())
                    line = stream.readline()
                    if not line:
                        raise RuntimeError(f'{method}: connection closed before the final reply')
                    return json.loads(line)

                self.pending_work = method == 'engine.batch'
                reply = request(method, params or {}, 1)
                if reply.get('running'):
                    self.pending_work = True
                    deadline = time.monotonic() + PENDING_TIMEOUT
                    identifier = 2
                    while True:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0:
                            raise RuntimeError(f'{method}: still running; capture cleanup is deferred')
                        sock.settimeout(min(120, remaining))
                        pending = request('control.pending', {}, identifier)
                        identifier += 1
                        result = pending.get('result', {})
                        if not pending.get('ok') or result.get('pending') is None:
                            raise RuntimeError(f'{method}: pending reply unavailable; capture cleanup is deferred')
                        if result.get('done'):
                            reply = result['reply']
                            break
                        time.sleep(0.2)
                self.pending_work = False
        if not reply.get('ok'):
            raise RuntimeError(f'{method}: {reply.get("error")}')
        result = reply['result']
        if method == 'engine.batch' and result.get('failed'):
            raise RuntimeError(f'batch: {result["failed"]}')
        return result

    def execute(self, command, params=None):
        return self.call('engine.execute', {'command': command, 'params': params or {}})

    def batch(self, commands):
        if commands:
            self.call('engine.batch', {'commands': commands})


def ensure_idle(control):
    running = control.call('control.running').get('running')
    if not isinstance(running, list) or running:
        raise RuntimeError('The capture window may still be processing a request; leaving documents untouched')


def scratch_status(control, documents, token=None):
    current = control.call('ui.documents')
    count = len(documents['documents'])
    if (len(current['documents']) != count + 1 or current.get('active') != count
            or current['documents'][:count] != documents['documents']):
        return None
    document = control.call('document.inspect')
    if document.get('path', 'unknown') is not None:
        return None
    if token is not None:
        if document.get('metadata', {}).get(CAPTURE_METADATA) != {'token': token}:
            return None
    elif (document.get('dirty') is not False or document.get('metadata') != {}
          or any(document.get(key) != [] for key in ('timeline', 'sketches', 'bodies', 'params'))):
        return None
    return current


@contextmanager
def scratch_document(control):
    control.preflight()
    original = control.call('ui.inspect')
    original['camera'] = control.call('ui.camera')
    hint_style = control.call('ui.hint_capture')
    through_bodies = control.call('ui.prefs')['reference_geometry_through_bodies']
    origin_hidden = original['visibility'].get('origin_hidden')
    documents = control.call('ui.documents')
    count = len(documents['documents'])
    if count >= 32:
        raise RuntimeError('No free document slot; nothing changed')
    token = uuid.uuid4().hex
    try:
        created = control.call('ui.documents', {'action': 'new'})
        if (created.get('active') != count or len(created.get('documents', [])) != count + 1
                or created['documents'][:count] != documents['documents']):
            raise RuntimeError('Scratch creation was not confirmed; refusing to edit')
        if scratch_status(control, documents) is None:
            raise RuntimeError('A separate empty scratch document was not identified; refusing to edit')
        control.execute('document.metadata.set', {'key': CAPTURE_METADATA, 'value': {'token': token}})
        if scratch_status(control, documents, token) is None:
            raise RuntimeError('The capture scratch document could not be identified')
        yield
    finally:
        if getattr(control, 'pending_work', False):
            raise RuntimeError('Capture work is still pending; leaving documents untouched')
        ensure_idle(control)
        owned = scratch_status(control, documents, token)
        if owned is None:
            if control.call('ui.documents') != documents:
                raise RuntimeError('Scratch identity is uncertain; leaving all documents untouched. Inspect the capture window before retrying.')
        else:
            errors = []
            for method, params in [
                ('ui.set', original['ui']),
                ('ui.set', {'showOrigin': None if origin_hidden is None else not origin_hidden}),
                ('ui.hint_capture', hint_style),
                ('ui.prefs', {'reference_geometry_through_bodies': through_bodies}),
                ('ui.camera', original['camera']),
            ]:
                try:
                    control.call(method, params)
                except (OSError, RuntimeError, ValueError) as error:
                    if getattr(control, 'pending_work', False):
                        raise RuntimeError('Restoration is still pending; leaving documents untouched') from error
                    ensure_idle(control)
                    errors.append(str(error))
            if scratch_status(control, documents, token) is None:
                raise RuntimeError('Scratch identity changed during restoration; leaving all documents untouched')
            control.call('ui.documents', {'action': 'close', 'index': count, 'force': True})
            control.call('ui.documents', {'action': 'switch', 'index': documents['active']})
            control.call('ui.home', {'open': original['home']})
            if errors:
                raise RuntimeError('Could not restore capture window: ' + '; '.join(errors))


def apply_ui(control, values):
    values = dict(values)
    selected = values.pop('select', [])
    preferences = values.pop('prefs', {})
    hint_style = values.pop('hint_capture', {})
    if hint_style:
        control.call('ui.hint_capture', hint_style)
    if set(preferences) - {'reference_geometry_through_bodies'}:
        raise RuntimeError('Unsupported authoring preference')
    if preferences:
        control.call('ui.prefs', preferences)
    control.execute('select.set', {'items': selected})
    if values:
        control.call('ui.set', values)


def shot(control, path, crop_scale=1.0):
    # Control replies precede the window's next paint; allow its display plan to settle.
    time.sleep(0.08)
    for attempt in range(3):
        try:
            info = control.call('ui.screenshot', {'path': str(path)})
            break
        except RuntimeError as error:
            if 'no frame was presented' not in str(error) or attempt == 2:
                raise
            time.sleep(0.2)
    scale = info['width'] / 1728.0
    if abs(info['height'] / scale - 998.0) > 2:
        raise RuntimeError('Capture window size differs from the 1728 x 998 specification')
    width, height = 960 * crop_scale, 600 * crop_scale
    if round(width * scale) < SIZE[0] or round(height * scale) < SIZE[1]:
        raise RuntimeError('Screenshot density is too low for this crop; capture at a higher display scale')
    left, top = 384 + (960 - width) / 2, 216 + (600 - height) / 2
    geometry = f'{round(width * scale)}x{round(height * scale)}+{round(left * scale)}+{round(top * scale)}'
    cropped = path.with_name(path.stem + '-crop.png')
    subprocess.run(['magick', str(path), '-crop', geometry, '+repage', '-resize', '960x600', str(cropped)], check=True)
    return cropped


def capture_state(control, sample, state, scratch):
    apply_ui(control, sample.get(f'{state}_ui', {}))
    control.call('ui.set', {'showGrid': False, 'showOrigin': False, 'groundShadow': False})
    camera = dict(sample['camera'])
    crop_scale = sample.get('crop_scale', 1.0)
    if not isinstance(crop_scale, (int, float)) or not 0.25 <= crop_scale <= 1.0:
        raise RuntimeError('crop_scale must be between 0.25 and 1')
    # Match the model-render camera to the fixed viewport crop. Pair panes are half as wide.
    camera['distance'] *= 1.25 * (1.5 if sample['presentation'] == 'pair' else 1.0) / crop_scale
    control.call('ui.camera', camera)
    control.call('ui.move', {'x': 20, 'y': 950})
    for action in sample.get(f'{state}_actions', []):
        params = dict(action.get('params', {}))
        if 'handle_suffix' in action:
            handles = control.call('ui.handles')
            matches = [handle for handle in handles if handle['name'].endswith(action['handle_suffix'])]
            positions = {(handle['x'], handle['y']) for handle in matches}
            if len(positions) != 1:
                raise RuntimeError(f'{sample["slug"]}: action handle is missing or ambiguous: {matches}')
            x, y = positions.pop()
            params.update(x=x, y=y)
        if 'world' in action:
            time.sleep(0.1)
            point = control.call('ui.worldToScreen', {'points': [action['world']]})[0]
            if point is None:
                raise RuntimeError(f'{sample["slug"]}: action point is not visible')
            params.update(x=point[0], y=point[1])
        control.call(action['method'], params)
        time.sleep(0.1)
    images = {}
    for theme in ('light', 'dark'):
        control.call('ui.set', {'dark': theme == 'dark'})
        path = scratch / f'{sample["slug"]}-{state}-{theme}.png'
        images[theme] = shot(control, path, crop_scale)
        if state == 'after' and sample.get('focus_point'):
            point = control.call('ui.worldToScreen', {'points': [sample['focus_point']]})[0]
            if point is None:
                raise RuntimeError(f'{sample["slug"]}: focus point is not visible')
            left, top = 384 + 480 * (1 - crop_scale), 216 + 300 * (1 - crop_scale)
            x, y = (point[0] - left) / crop_scale, (point[1] - top) / crop_scale
            marker_clearance = 11  # Radius 10 plus half the two-point outline.
            left_edge, right_edge = ((240 + marker_clearance, 719 - marker_clearance)
                                     if sample['presentation'] == 'pair' else (10, SIZE[0] - 10))
            if not left_edge <= x <= right_edge or not 10 <= y <= SIZE[1] - 10:
                raise RuntimeError(f'{sample["slug"]}: focus point leaves the illustration')
            subprocess.run([
                'magick', str(images[theme]), '-fill', 'none', '-stroke', '#e79d00',
                '-strokewidth', '2', '-draw', f'circle {x},{y} {x + 10},{y}', str(images[theme]),
            ], check=True)
    return images


@contextmanager
def staged_exports(output, names):
    staging = Path(tempfile.mkdtemp(prefix='.command-hints-', dir=output))
    clean = True
    try:
        yield staging
        for name in names:
            if not (staging / name).is_file():
                raise RuntimeError(f'Missing staged export: {name}')
        backups = staging / 'previous'
        backups.mkdir()
        for name in names:
            if (output / name).exists():
                shutil.copy2(output / name, backups / name)
        try:
            for name in names:
                os.replace(staging / name, output / name)
        except (OSError, KeyboardInterrupt) as error:
            failures = []
            for name in reversed(names):
                try:
                    if (backups / name).exists():
                        os.replace(backups / name, output / name)
                    elif (output / name).exists():
                        (output / name).unlink()
                except (OSError, KeyboardInterrupt) as restore_error:
                    failures.append(str(restore_error))
            if failures:
                clean = False
                raise RuntimeError(f'Could not restore exports; backups remain in {backups}: {failures}') from error
            raise
    finally:
        if clean:
            shutil.rmtree(staging)


def compose(sample, before, after, output):
    validate_slug(sample['slug'])
    names = [f'{sample["slug"]}-{theme}.png' for theme in ('light', 'dark')]
    with staged_exports(output, names) as staging:
        for theme in ('light', 'dark'):
            destination = staging / f'{sample["slug"]}-{theme}.png'
            if sample['presentation'] == 'single':
                subprocess.run(['magick', str(after[theme]), str(destination)], check=True)
            else:
                arrow = '#b8c0cd' if theme == 'dark' else '#5e6a7b'
                subprocess.run([
                    'magick', '(', str(before[theme]), '-crop', '480x600+240+0', '+repage', ')',
                    '(', str(after[theme]), '-crop', '480x600+240+0', '+repage', ')', '+append',
                    '-stroke', arrow, '-strokewidth', '2', '-draw',
                    'line 460,300 500,300 line 493,294 500,300 line 500,300 493,306', str(destination),
                ], check=True)



def vector(value, label):
    if (not isinstance(value, (list, tuple)) or len(value) != 3
            or any(not isinstance(part, (int, float)) or not math.isfinite(part) for part in value)):
        raise RuntimeError(f'{label}: expected a finite three-dimensional vector, got {value}')
    return value


def unit(value, label):
    value = vector(value, label)
    length = math.hypot(*value)
    if length == 0:
        raise RuntimeError(f'{label}: direction must not be zero')
    return [part / length for part in value]


def check_vector(actual, expected, tolerance, label):
    if math.dist(vector(actual, label), vector(expected, label)) > tolerance:
        raise RuntimeError(f'{label}: expected {expected}, got {actual}')


def verify_body_geometry(bodies, expected, tolerance, label):
    for key in ('closed', 'all_closed'):
        if key in expected and (not bodies and expected[key] is False
                                or any(body.get('closed') is not expected[key] for body in bodies)):
            raise RuntimeError(f'{label}: {key}: expected {expected[key]} for every body')
    if 'bbox' in expected:
        if not bodies or any('bbox' not in body for body in bodies):
            raise RuntimeError(f'{label}: bbox: body measurements are missing')
        for bound, combine in [('min', min), ('max', max)]:
            bounds = [vector(body['bbox'].get(bound), f'{label} bbox.{bound}') for body in bodies]
            actual = [combine(values) for values in zip(*bounds)]
            check_vector(actual, expected['bbox'][bound], tolerance, f'{label} bbox.{bound}')


def verify_construction(control, expected, tolerance, label):
    kind = expected['type']
    selection = {'type': 'construction_point' if kind == 'point' else kind, 'name': expected['name']}
    items = control.execute('inspect.measure', {'items': [selection]})['items']
    if len(items) != 1 or items[0].get('type') != kind:
        raise RuntimeError(f'{label}: expected construction type {kind}, got {items}')
    actual = items[0]
    for key in ('point', 'origin', 'position'):
        if key in expected:
            check_vector(actual.get(key), expected[key], tolerance, f'{label} {key}')
    for key in ('normal', 'normal_parallel', 'direction_parallel'):
        if key not in expected:
            continue
        field = 'direction' if key == 'direction_parallel' else 'normal'
        got, wanted = unit(actual.get(field), label), unit(expected[key], label)
        distance = math.dist(got, wanted)
        if key.endswith('_parallel'):
            distance = min(distance, math.dist(got, [-part for part in wanted]))
        if distance > 1e-6:
            raise RuntimeError(f'{label} {key}: expected {expected[key]}, got {actual.get(field)}')
    for key in ('point_on_plane', 'other_point_on_plane', 'origin_on_axis'):
        if key not in expected:
            continue
        on_axis = key == 'origin_on_axis'
        origin = vector(actual.get('origin' if on_axis else 'point'), label)
        direction = unit(actual.get('direction' if on_axis else 'normal'), label)
        offset = [a - b for a, b in zip(vector(expected[key], label), origin)]
        along = sum(a * b for a, b in zip(offset, direction))
        distance = math.dist(offset, [along * part for part in direction]) if on_axis else abs(along)
        if distance > tolerance:
            raise RuntimeError(f'{label} {key}: {expected[key]} is {distance} mm from the construction')
    return actual


def verify_volume(actual, expected, root, label, before=None):
    if 'volume_less_than_mm3' in expected and not actual < expected['volume_less_than_mm3']:
        raise RuntimeError(f'{label}: volume must be less than {expected["volume_less_than_mm3"]}, got {actual}')
    if 'volume_mm3' in expected:
        tolerance = expected.get('volume_relative_tolerance', expected.get('relative_volume_tolerance',
                                 root.get('volume_relative_tolerance', root.get('relative_volume_tolerance', 0.01))))
        if actual is None or not math.isclose(actual, expected['volume_mm3'], rel_tol=tolerance, abs_tol=0.01):
            raise RuntimeError(f'{label}: expected volume {expected["volume_mm3"]}, got {actual}')
    if 'material_removed_mm3' in expected:
        if before is None:
            raise RuntimeError(f'{label}: material removal needs the measured before volume')
        removed = before - actual
        tolerance = expected.get('relative_removal_tolerance', root.get('relative_removal_tolerance', 0.01))
        if not math.isclose(removed, expected['material_removed_mm3'], rel_tol=tolerance, abs_tol=0.01):
            raise RuntimeError(f'{label}: expected removal {expected["material_removed_mm3"]}, got {removed}')


def verify(control, sample, state, before=None):
    document = control.execute('document.inspect', {'measure': False})
    errors = [feature for feature in document['timeline'] if feature.get('error')]
    if errors:
        raise RuntimeError(f'{sample["slug"]}: feature errors: {errors}')
    measurement = control.execute('inspect.measure')
    expected = sample.get('expected', {})
    expected = expected.get(state, expected)
    for key, actual in [('body_count', measurement['body_count']),
                        ('faces', measurement['total'].get('faces')),
                        ('sketch_count', len(document['sketches'])),
                        ('sketch_curves', sum(sketch['curves'] for sketch in document['sketches'])),
                        ('sketch_profiles', sum(sketch['profiles'] for sketch in document['sketches']))]:
        if key in expected and expected[key] != actual:
            raise RuntimeError(f'{sample["slug"]} {state}: {key}: expected {expected[key]}, got {actual}')
    label = f'{sample["slug"]} {state}'
    root = sample.get('expected', {})
    tolerance = expected.get('bbox_tolerance_mm', root.get('bbox_tolerance_mm',
                            expected.get('geometry_tolerance_mm', root.get('geometry_tolerance_mm', 0.02))))
    before_volume = None if before is None else before['measurement']['total']['volume_mm3']
    verify_volume(measurement['total']['volume_mm3'], expected, root, label, before_volume)
    verify_body_geometry(measurement['bodies'], expected, tolerance, label)
    for name, wanted in expected.get('bodies', {}).items():
        body = next((body for body in measurement['bodies'] if body['name'] == name), None)
        if body is None:
            raise RuntimeError(f'{label}: missing body {name}')
        verify_volume(body.get('volume_mm3'), wanted, root, f'{label} {name}')
        verify_body_geometry([body], wanted, tolerance, f'{label} {name}')
        if 'center_of_mass' in wanted:
            check_vector(body.get('center_of_mass'), wanted['center_of_mass'], tolerance, f'{label} {name} center_of_mass')
    report = {'measurement': measurement, 'sketches': document['sketches']}
    if 'construction' in expected:
        report['construction'] = verify_construction(control, expected['construction'], tolerance, f'{label} construction')
    if 'joint_values' in expected:
        joints = control.execute('joint.list')['joints']
        report['joints'] = joints
        for name, values in expected['joint_values'].items():
            joint = next((joint for joint in joints if joint['name'] == name), None)
            actual = [] if joint is None else [value['value'] for value in joint['values']]
            if len(actual) != len(values) or any(not math.isclose(a, b, abs_tol=0.001) for a, b in zip(actual, values)):
                raise RuntimeError(f'{sample["slug"]} {state}: {name}: expected {values}, got {actual}')
    return report


def capture(control, sample, scratch, output):
    with scratch_document(control):
        control.call('ui.home', {'open': False})
        control.call('ui.set', {
            'showGrid': False, 'showOrigin': False, 'groundShadow': False, 'perspective': False,
            'showBrowser': False, 'showTimeline': False, 'showNavigationBar': False,
        })
        control.batch(sample['setup'])
        reports = {}
        if sample['presentation'] == 'pair' and 'before' in sample.get('expected', {}):
            reports['before'] = verify(control, sample, 'before')
        elif 'material_removed_mm3' in sample.get('expected', {}).get('after', sample.get('expected', {})):
            reports['before'] = {'measurement': control.execute('inspect.measure')}
        before = capture_state(control, sample, 'before', scratch) if sample['presentation'] == 'pair' else {}
        control.batch(sample['operation'])
        reports['after'] = verify(control, sample, 'after', before=reports.get('before'))
        after = capture_state(control, sample, 'after', scratch)
        compose(sample, before, after, output)
        (scratch / f'{sample["slug"]}-measurement.json').write_text(json.dumps(reports, indent=2) + '\n')
    print(f'{sample["command"]}: captured {sample["presentation"]}', flush=True)


def validate_slug(slug):
    if not isinstance(slug, str) or not slug or any(char not in 'abcdefghijklmnopqrstuvwxyz0123456789-_' for char in slug):
        raise ValueError(f'Invalid recipe slug: {slug!r}')


def select_samples(samples, wanted=None):
    selected = [sample for sample in samples if wanted is None or sample['slug'] in wanted]
    for sample in selected:
        validate_slug(sample['slug'])
    duplicates = sorted(slug for slug, count in Counter(sample['slug'] for sample in selected).items() if count > 1)
    if duplicates:
        raise ValueError('Duplicate recipe slugs: ' + ', '.join(duplicates))
    if wanted and wanted != {sample['slug'] for sample in selected}:
        raise ValueError('Unknown requested slug')
    return selected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', required=True, type=int)
    parser.add_argument('--token-dir', required=True, type=Path)
    parser.add_argument('--anchor', required=True, type=Path, help='Expected saved, clean capture design; the window must contain only this document')
    parser.add_argument('--source', action='append', type=Path)
    parser.add_argument('--only', help='Comma-separated slugs to regenerate')
    parser.add_argument('--output', type=Path, default=REPO / 'assets/command-hints/exports')
    parser.add_argument('--scratch', type=Path, default=REPO / 'target/command-illustrations/model-captures')
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error('port must be in 1..65535')
    args.output = args.output.resolve()
    args.scratch = args.scratch.resolve()
    sources = args.source or sorted((REPO / 'assets/command-hints/source').glob('*-menu.json'))
    wanted = set(args.only.split(',')) if args.only else None
    try:
        samples = select_samples([sample for path in sources for sample in json.loads(path.read_text())['samples']], wanted)
    except ValueError as error:
        parser.error(str(error))
    args.output.mkdir(parents=True, exist_ok=True)
    args.scratch.mkdir(parents=True, exist_ok=True)
    control = Control(args.port, args.token_dir.resolve(), args.anchor)
    control.preflight()
    control.call('ui.resize', {'width': 1728, 'height': 998})
    for sample in samples:
        capture(control, sample, args.scratch, args.output)


if __name__ == '__main__':
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f'capture failed: {error}', file=sys.stderr)
        sys.exit(1)
