#!/usr/bin/env python3
"""Author bundled command pictures using an isolated running SolidCraft window.

Python and ImageMagick are authoring tools only, never application dependencies.
"""
import argparse
from contextlib import contextmanager
import json
import math
from pathlib import Path
import socket
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parents[3]
SIZE = (960, 600)


class Control:
    def __init__(self, port, token_dir, anchor):
        self.port = port
        self.token = (token_dir / f'{port}.token').read_text().strip()
        self.anchor = anchor.resolve(strict=True)

    def preflight(self):
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
        with socket.create_connection(('127.0.0.1', self.port), timeout=120) as sock:
            message = {'id': 1, 'token': self.token, 'method': method, 'params': params or {}}
            sock.sendall((json.dumps(message) + '\n').encode())
            with sock.makefile() as stream:
                reply = json.loads(stream.readline())
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
    created = control.call('ui.documents', {'action': 'new'})
    if len(created['documents']) != count + 1 or created['active'] == documents['active']:
        control.call('ui.set', original['ui'])
        control.call('ui.set', {'showOrigin': None if origin_hidden is None else not origin_hidden})
        control.call('ui.camera', original['camera'])
        control.call('ui.home', {'open': original['home']})
        raise RuntimeError('A separate scratch document was not created; refusing to edit')
    try:
        yield
    finally:
        errors = []
        for method, params in [
            ('ui.documents', {'action': 'close', 'index': created['active'], 'force': True}),
            ('ui.documents', {'action': 'switch', 'index': documents['active']}),
            ('ui.set', original['ui']),
            ('ui.set', {'showOrigin': None if origin_hidden is None else not origin_hidden}),
            ('ui.hint_capture', hint_style),
            ('ui.prefs', {'reference_geometry_through_bodies': through_bodies}),
            ('ui.camera', original['camera']),
            ('ui.home', {'open': original['home']}),
        ]:
            try:
                control.call(method, params)
            except (OSError, RuntimeError, ValueError) as error:
                errors.append(str(error))
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
            if not 10 <= x <= SIZE[0] - 10 or not 10 <= y <= SIZE[1] - 10:
                raise RuntimeError(f'{sample["slug"]}: focus point leaves the illustration')
            subprocess.run([
                'magick', str(images[theme]), '-fill', 'none', '-stroke', '#e79d00',
                '-strokewidth', '2', '-draw', f'circle {x},{y} {x + 10},{y}', str(images[theme]),
            ], check=True)
    return images


def compose(sample, before, after, output):
    for theme in ('light', 'dark'):
        destination = output / f'{sample["slug"]}-{theme}.png'
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


def verify(control, sample, state):
    document = control.execute('document.inspect', {'measure': False})
    errors = [feature for feature in document['timeline'] if feature.get('error')]
    if errors:
        raise RuntimeError(f'{sample["slug"]}: feature errors: {errors}')
    measurement = control.execute('inspect.measure', {'summary': True})
    expected = sample.get('expected', {})
    expected = expected.get(state, expected)
    for key, actual in [('body_count', measurement['body_count']),
                        ('sketch_count', len(document['sketches'])),
                        ('sketch_curves', sum(sketch['curves'] for sketch in document['sketches'])),
                        ('sketch_profiles', sum(sketch['profiles'] for sketch in document['sketches']))]:
        if key in expected and expected[key] != actual:
            raise RuntimeError(f'{sample["slug"]} {state}: {key}: expected {expected[key]}, got {actual}')
    if 'volume_mm3' in expected:
        root = sample.get('expected', {})
        tolerance = expected.get('volume_relative_tolerance', expected.get('relative_volume_tolerance',
                                 root.get('volume_relative_tolerance', root.get('relative_volume_tolerance', 0.01))))
        actual = measurement['total']['volume_mm3']
        if not math.isclose(actual, expected['volume_mm3'], rel_tol=tolerance, abs_tol=0.01):
            raise RuntimeError(f'{sample["slug"]} {state}: expected volume {expected["volume_mm3"]}, got {actual}')
    report = {'measurement': measurement, 'sketches': document['sketches']}
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
        before = capture_state(control, sample, 'before', scratch) if sample['presentation'] == 'pair' else {}
        control.batch(sample['operation'])
        reports['after'] = verify(control, sample, 'after')
        after = capture_state(control, sample, 'after', scratch)
        compose(sample, before, after, output)
        (scratch / f'{sample["slug"]}-measurement.json').write_text(json.dumps(reports, indent=2) + '\n')
    print(f'{sample["command"]}: captured {sample["presentation"]}', flush=True)


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
    args.output.mkdir(parents=True, exist_ok=True)
    args.scratch.mkdir(parents=True, exist_ok=True)
    sources = args.source or sorted((REPO / 'assets/command-hints/source').glob('*-menu.json'))
    wanted = set(args.only.split(',')) if args.only else None
    samples = [sample for path in sources for sample in json.loads(path.read_text())['samples'] if wanted is None or sample['slug'] in wanted]
    if wanted and wanted != {sample['slug'] for sample in samples}:
        parser.error('unknown or duplicated requested slug')
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
