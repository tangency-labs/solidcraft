#!/usr/bin/env python3
"""Capture native sketch tools at one input per second for illustrated hints."""
import argparse
import importlib.util
import json
import math
from pathlib import Path
import subprocess
import sys
import time

spec = importlib.util.spec_from_file_location('hint_capture', Path(__file__).with_name('capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


def pointer(control, method, point, **params):
    projected = control.call('ui.sketchToScreen', {'points': [point]})[0]
    if projected is None:
        raise RuntimeError('Construction point is outside the native camera')
    return control.call(method, dict(params, x=projected[0], y=projected[1]))


def path_point(start, route, fraction):
    points = [start] + route
    scaled = fraction * (len(points) - 1)
    index = min(int(scaled), len(points) - 2)
    local = scaled - index
    return [points[index][axis] * (1 - local) + points[index + 1][axis] * local for axis in range(2)]


def same_point(a, b):
    return len(a) == len(b) == 2 and all(math.isclose(x, y, abs_tol=0.05) for x, y in zip(a, b))


def verify(control, sample, before):
    report = capture.verify(control, sample, 'after')
    sketch = control.execute('sketch.inspect')
    created = [curve for curve in sketch['curves'] if curve['id'] not in before and not curve['construction']]
    if sample.get('preserve_reference_curves'):
        current = {curve['id']: curve for curve in sketch['curves']}
        for curve_id, original in before.items():
            actual = current.get(curve_id, {})
            if actual.get('type') != original['type'] or actual.get('construction') != original['construction']:
                raise RuntimeError(f'{sample["command"]}: reference geometry changed')
            for field in ('start_at', 'end_at', 'center_at'):
                if field in original and (field not in actual or not same_point(original[field], actual[field])):
                    raise RuntimeError(f'{sample["command"]}: reference geometry moved')
    if sketch['failing'] or not created:
        raise RuntimeError(f'{sample["command"]}: no healthy native result: {sketch["failing"]}')
    family = sample['command'].split('.')[1]
    expected = {'line': 'line', 'circle': 'circle', 'arc': 'arc', 'spline': 'spline'}.get(family)
    if expected and (len(created) != 1 or created[0]['type'] != expected):
        raise RuntimeError(f'{sample["command"]}: unexpected native curves: {created}')
    if 'circle_geometry' in sample:
        expected_circle = sample['circle_geometry']
        actual = created[0]
        if not math.isclose(actual['radius'], expected_circle['radius'], abs_tol=0.05) or any(
            not math.isclose(value, expected, abs_tol=0.05)
            for value, expected in zip(actual['center_at'], expected_circle['center'])
        ):
            raise RuntimeError(f'{sample["command"]}: native circle differs from its analytic construction')
    if 'line_geometry' in sample:
        expected_line = sample['line_geometry']
        actual = created[0]
        a, b = actual['start_at'], actual['end_at']
        if not ((same_point(a, expected_line['start']) and same_point(b, expected_line['end'])) or
                (same_point(b, expected_line['start']) and same_point(a, expected_line['end']))):
            raise RuntimeError(f'{sample["command"]}: native line endpoints differ from the construction')
        if 'midpoint' in expected_line and not same_point([(x + y) / 2 for x, y in zip(a, b)], expected_line['midpoint']):
            raise RuntimeError(f'{sample["command"]}: the chosen point is not the native line midpoint')
    if family in ('rectangle', 'slot') and (len(created) < 4 or not sketch['profiles']):
        raise RuntimeError(f'{sample["command"]}: native tool did not complete a closed profile')
    report['native_result'] = created
    return report, [curve['id'] for curve in created], sketch['id']


def animate(control, sample, theme, scratch, output, delay, final_delay):
    with capture.scratch_document(control):
        control.call('ui.home', {'open': False})
        control.call('ui.agent_cursor', {'show': False, 'follow_camera': False})
        control.call('ui.set', {
            'showGrid': False, 'showOrigin': False, 'groundShadow': False, 'perspective': False,
            'showBrowser': False, 'showTimeline': False, 'showNavigationBar': False, 'dark': theme == 'dark',
        })
        control.batch(sample['setup'])
        before = {curve['id']: curve for curve in control.execute('sketch.inspect')['curves']}
        control.call('ui.hint_capture', {'emphasize_sketches': True})
        control.call('ui.camera', sample['camera'])
        control.call('ui.start', {'command': sample['command']})
        control.call('ui.agent_cursor', {'show': True, 'speed': 'instant', 'follow_camera': False})
        frames = []
        work = scratch / f'{sample["slug"]}-{theme}'
        work.mkdir(parents=True, exist_ok=True)

        def frame():
            cropped = capture.shot(control, work / f'{len(frames):02}.png', sample['crop_scale'])
            small = cropped.with_name(cropped.stem + '-480.png')
            subprocess.run(['magick', str(cropped), '-resize', '480x300', str(small)], check=True)
            frames.append(small)
            return cropped

        first = next(event['sketch'] for event in sample['events'] if 'sketch' in event)
        previous = [first[0] - 6, first[1] + 6]
        pointer(control, 'ui.move', previous)
        frame()
        for event in sample['events']:
            if 'sketch' in event:
                route = event.get('via', [event['sketch']])
                for fraction in (1 / 3, 2 / 3, 1):
                    pointer(control, 'ui.move', path_point(previous, route, fraction))
                    frame()
                pointer(control, 'ui.click', event['sketch'], button=event.get('button', 'left'))
                previous = event['sketch']
            elif 'handle' in event:
                target = control.call('ui.at', {'handle': event['handle']})
                start = control.call('ui.sketchToScreen', {'points': [previous]})[0]
                if start is None:
                    raise RuntimeError('The previous construction pick is outside the camera')
                for fraction in (1 / 3, 2 / 3, 1):
                    at = path_point(start, [target], fraction)
                    control.call('ui.move', {'x': at[0], 'y': at[1]})
                    frame()
                control.call('ui.click', {'x': target[0], 'y': target[1]})
            else:
                control.call('ui.text', {'text': event['text']})
                for _ in range(3):
                    frame()
                if 'commit' in event:
                    control.call('ui.key', {'key': 'Escape'})
                    time.sleep(0.2)
                    control.call('engine.execute', dict(event['commit'], animate=False))
                else:
                    control.call('ui.key', {'key': event['key']})
            frame()
        report, curves, sketch_id = verify(control, sample, before)
        control.call('ui.key', {'key': 'Escape'})
        control.call('ui.agent_cursor', {'show': False})
        control.call('ui.move', {'x': 20, 'y': 950})
        time.sleep(0.2)
        if sample.get('result_commands'):
            control.batch(sample['result_commands'])
            report, curves, sketch_id = verify(control, sample, before)
        control.execute('select.set', {'items': [
            {'type': 'external_sketch_curve', 'sketch': sketch_id, 'id': curve} for curve in curves
        ]})
        poster = frame()
        if len(frames) > 32 or (len(frames) - 1) * delay + final_delay > 1000:
            raise RuntimeError('Construction animation exceeds the bounded player')
        destination = output / f'{sample["slug"]}-{theme}.gif'
        arguments = ['magick', '-dispose', 'none']
        for index, path in enumerate(frames):
            arguments.extend(['-delay', str(final_delay if index == len(frames) - 1 else delay), str(path)])
        arguments.extend(['-dither', 'None', '-colors', '128', '-loop', '0', '-layers', 'Optimize', str(destination)])
        subprocess.run(arguments, check=True)
        subprocess.run(['magick', str(poster), str(output / f'{sample["slug"]}-{theme}.png')], check=True)
        report['frames_before_optimization'] = len(frames)
        report['encoded_duration_cs'] = (len(frames) - 1) * delay + final_delay
        (work / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
        print(f'{sample["command"]} {theme}: {len(frames)} native frames', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', required=True, type=int)
    parser.add_argument('--token-dir', required=True, type=Path)
    parser.add_argument('--anchor', required=True, type=Path, help='Expected saved, clean capture design; the window must contain only this document')
    parser.add_argument('--source', type=Path, default=capture.REPO / 'assets/command-hints/source/sketch-construction.json')
    parser.add_argument('--only', help='Comma-separated slugs')
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error('port must be in 1..65535')
    source = json.loads(args.source.read_text())
    wanted = set(args.only.split(',')) if args.only else None
    samples = [sample for sample in source['samples'] if wanted is None or sample['slug'] in wanted]
    if wanted and wanted != {sample['slug'] for sample in samples}:
        parser.error('unknown requested slug')
    for sample in samples:
        if not 0.25 <= sample['crop_scale'] <= 1 or len(sample['events']) > 7:
            parser.error('invalid capture crop or event count')
        for event in sample['events']:
            points = [event['sketch']] + event.get('via', []) if 'sketch' in event else []
            if any(len(point) != 2 or not all(math.isfinite(value) for value in point) for point in points):
                parser.error('invalid native construction point')
    control = capture.Control(args.port, args.token_dir.resolve(), args.anchor)
    control.preflight()
    control.call('ui.resize', {'width': 1728, 'height': 998})
    cursor = control.call('ui.agent_cursor')
    scratch = capture.REPO / 'target/command-illustrations/sketch-construction-captures'
    output = capture.REPO / 'assets/command-hints/exports'
    try:
        for sample in samples:
            for theme in ('light', 'dark'):
                animate(control, sample, theme, scratch, output, source['frame_delay_cs'], source['final_source_delay_cs'])
    finally:
        control.call('ui.agent_cursor', cursor)


if __name__ == '__main__':
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f'construction capture failed: {error}', file=sys.stderr)
        sys.exit(1)
