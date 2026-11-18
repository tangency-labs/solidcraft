"""Regression checks for the command illustration authoring verifier."""
import copy
import json
import importlib.util
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch

import capture


class Window:
    def __init__(self, creation='normal'):
        self.active = 0
        self.tabs = [{'name': 'Anchor', 'dirty': False, 'drawing': None}]
        self.anchor_dirty = False
        self.creation = creation
        self.metadata = {}
        self.hint_style = {'emphasize_axes': False, 'emphasize_planes': False, 'emphasize_sketches': False}
        self.closed = []

    def preflight(self):
        if self.anchor_dirty or len(self.tabs) != 1:
            raise RuntimeError('Anchor was changed')

    def execute(self, command, params=None):
        if command == 'document.metadata.set':
            self.metadata[params['key']] = params['value']
            self.tabs[self.active]['dirty'] = True
        if command == 'sketch.inspect':
            return {'curves': []}
        return {}

    def batch(self, commands):
        pass

    def call(self, method, params=None):
        if method == 'control.running':
            return {'running': []}
        if method == 'ui.inspect':
            return {'ui': {}, 'visibility': {}, 'home': False}
        if method == 'ui.prefs':
            return {'reference_geometry_through_bodies': False}
        if method == 'ui.hint_capture':
            self.hint_style.update(params or {})
            return dict(self.hint_style)
        if method == 'ui.sketchToScreen':
            return [[100, 100]]
        if method == 'document.inspect':
            return {'path': None if self.active else '/anchor.sc', 'timeline': [], 'sketches': [],
                    'bodies': [], 'params': [], 'metadata': dict(self.metadata),
                    'dirty': self.tabs[self.active]['dirty']}
        if method == 'ui.documents':
            if params:
                if params['action'] == 'new':
                    if self.creation != 'not_created':
                        self.active = 1
                        self.tabs.append({'name': 'Untitled', 'dirty': self.creation == 'ambiguous', 'drawing': None})
                    if self.creation != 'normal':
                        raise OSError('Creation reply lost')
                elif params['action'] == 'close':
                    self.closed.append((params['index'], params.get('force', False)))
                    self.tabs.pop(params['index'])
                    self.active = 0
                    self.metadata = {}
                elif params['action'] == 'switch':
                    self.active = params['index']
            return {'active': self.active, 'documents': copy.deepcopy(self.tabs)}
        if method == 'ui.set' and self.active == 0:
            self.anchor_dirty = True
        return {}


class CaptureTests(unittest.TestCase):
    def setUp(self):
        self.body = {'name': 'Block', 'closed': True, 'volume_mm3': 8,
                     'center_of_mass': [0, 0, 1],
                     'bbox': {'min': [-1, -1, 0], 'max': [1, 1, 2]}}
        self.measurement = {'body_count': 1, 'bodies': [self.body], 'total': {'volume_mm3': 8}}
        self.datum = {'type': 'plane', 'point': [0, 0, 32], 'normal': [0, 0, 1]}
        self.control = MagicMock()
        self.control.execute.side_effect = self.execute

    def execute(self, command, params=None):
        if command == 'document.inspect':
            return {'timeline': [], 'sketches': []}
        if command == 'inspect.measure':
            return {'items': [self.datum]} if params and 'items' in params else self.measurement
        if command == 'joint.list':
            return {'joints': [{'name': 'Thread', 'values': [{'value': 450}, {'value': 15}]}]}
        self.fail(f'Unexpected command: {command}')

    def verify(self, expected):
        return capture.verify(self.control, {'slug': 'example', 'expected': expected}, 'after')

    def test_declared_bounds_reject_translation_with_unchanged_volume(self):
        expected = {'volume_mm3': 8, 'bbox': copy.deepcopy(self.body['bbox'])}
        self.verify(expected)
        self.body['bbox']['min'][0] += 5
        self.body['bbox']['max'][0] += 5
        with self.assertRaisesRegex(RuntimeError, 'bbox'):
            self.verify(expected)

    def test_bounds_combine_bodies_and_honor_tolerance(self):
        second = copy.deepcopy(self.body)
        second['name'] = 'Other'
        second['bbox']['max'][0] = 8
        self.measurement['bodies'].append(second)
        expected = {'bbox': {'min': [-1, -1, 0], 'max': [8.01, 1, 2]}, 'bbox_tolerance_mm': 0.02}
        self.verify(expected)
        expected['bbox']['max'][0] = 8.03
        with self.assertRaisesRegex(RuntimeError, 'bbox'):
            self.verify(expected)

    def test_closed_checks_all_bodies_and_rejects_missing_measurements(self):
        self.verify({'closed': True})
        self.body['closed'] = False
        for key in ['closed', 'all_closed']:
            with self.subTest(key=key), self.assertRaisesRegex(RuntimeError, 'closed'):
                self.verify({key: True})
        del self.body['closed']
        with self.assertRaisesRegex(RuntimeError, 'closed'):
            self.verify({'closed': False})

    def test_empty_design_does_not_prove_a_body_is_open(self):
        self.measurement['bodies'] = []
        self.verify({'closed': True})
        for key in ['closed', 'all_closed']:
            with self.subTest(key=key), self.assertRaisesRegex(RuntimeError, 'closed'):
                self.verify({key: False})

    def test_per_body_position_rejects_move_that_did_not_translate(self):
        expected = {'bodies': {'Block': {'center_of_mass': [14, 0, 6]}}}
        self.body['center_of_mass'] = [14, 0, 6]
        self.verify(expected)
        self.body['center_of_mass'] = [-14, 0, 6]
        with self.assertRaisesRegex(RuntimeError, 'Block.*center_of_mass'):
            self.verify(expected)
        with self.assertRaisesRegex(RuntimeError, 'Missing'):
            self.verify({'bodies': {'Missing': {'center_of_mass': [0, 0, 0]}}})

    def test_plane_type_origin_and_oriented_normal_are_measured(self):
        expected = {'construction': {'type': 'plane', 'name': 'Result',
                                     'point': [0, 0, 32], 'normal': [0, 0, 1]}}
        self.verify(expected)
        self.control.execute.assert_any_call('inspect.measure', {'items': [{'type': 'plane', 'name': 'Result'}]})
        for field, bad in [('type', 'point'), ('point', [0, 0, 31]), ('normal', [0, 0, -1])]:
            with self.subTest(field=field):
                original = self.datum[field]
                self.datum[field] = bad
                with self.assertRaisesRegex(RuntimeError, 'construction'):
                    self.verify(expected)
                self.datum[field] = original

    def test_plane_membership_and_parallel_normals_ignore_origin_and_sign(self):
        self.datum = {'type': 'plane', 'point': [4, 6, 10], 'normal': [0, 0, -1]}
        expected = {'construction': {'type': 'plane', 'name': 'Result',
                                     'point_on_plane': [0, 0, 10], 'other_point_on_plane': [8, -9, 10],
                                     'normal_parallel': [0, 0, 3]}}
        self.verify(expected)
        expected['construction']['other_point_on_plane'][2] = 11
        with self.assertRaisesRegex(RuntimeError, 'construction'):
            self.verify(expected)

    def test_axis_position_and_direction_are_validated(self):
        self.datum = {'type': 'axis', 'origin': [0, 0, 12], 'direction': [0, 0, -1]}
        expected = {'construction': {'type': 'axis', 'name': 'Result',
                                     'origin_on_axis': [0, 0, 0], 'direction_parallel': [0, 0, 3]}}
        self.verify(expected)
        self.datum['origin'][0] = 1
        with self.assertRaisesRegex(RuntimeError, 'construction'):
            self.verify(expected)
        self.datum['origin'][0] = 0
        self.datum['direction'] = [1, 0, 0]
        with self.assertRaisesRegex(RuntimeError, 'construction'):
            self.verify(expected)

    def test_construction_point_uses_existing_selection_kind(self):
        self.datum = {'type': 'point', 'position': [20, -15, 20]}
        expected = {'construction': {'type': 'point', 'name': 'Result', 'position': [20, -15, 20]}}
        self.verify(expected)
        self.control.execute.assert_any_call('inspect.measure', {'items': [{'type': 'construction_point', 'name': 'Result'}]})
        self.datum['position'][2] = 19
        with self.assertRaisesRegex(RuntimeError, 'construction'):
            self.verify(expected)

    def test_invalid_datum_vectors_fail_closed(self):
        for bad in [[0, 0, 0], [0, 0, float('nan')], [0, 1]]:
            with self.subTest(bad=bad):
                self.datum['normal'] = bad
                with self.assertRaises(RuntimeError):
                    self.verify({'construction': {'type': 'plane', 'name': 'Result', 'normal_parallel': [0, 0, 1]}})

    def test_screw_travel_is_checked_in_joint_list_value_order(self):
        self.verify({'joint_values': {'Thread': [450, 15]}})
        with self.assertRaisesRegex(RuntimeError, 'Thread'):
            self.verify({'joint_values': {'Thread': [450, 0]}})

    def test_batch_timeout_exceeds_server_wait_without_changing_other_calls(self):
        control = capture.Control.__new__(capture.Control)
        control.port, control.token = 1234, 'test-token'
        with patch.object(capture.socket, 'create_connection') as connect:
            connect.return_value.__enter__.return_value.makefile.return_value.__enter__.return_value.readline.return_value = json.dumps({'ok': True, 'result': {}})
            control.call('engine.batch')
            self.assertGreater(connect.call_args.kwargs['timeout'], 3600)
            control.call('ui.inspect')
            self.assertEqual(connect.call_args.kwargs['timeout'], 120)

    def test_scratch_cleanup_restores_ui_before_returning_to_the_anchor(self):
        window = Window()
        for _ in range(2):
            with capture.scratch_document(window):
                self.assertEqual(window.active, 1)
            self.assertEqual(window.active, 0)
            self.assertFalse(window.anchor_dirty)

    def test_named_stock_bounds_and_volume_are_both_verified(self):
        expected = {'bodies': {'Block': {'bbox': copy.deepcopy(self.body['bbox']), 'volume_mm3': 8}}}
        self.verify(expected)
        self.body['volume_mm3'] = 7
        with self.assertRaisesRegex(RuntimeError, 'Block.*volume'):
            self.verify(expected)

    def test_face_count_rejects_an_unsplit_body_with_other_figures_unchanged(self):
        self.measurement['total']['faces'] = 7
        self.verify({'faces': 7, 'volume_mm3': 8, 'closed': True})
        self.measurement['total']['faces'] = 6
        with self.assertRaisesRegex(RuntimeError, 'faces'):
            self.verify({'faces': 7, 'volume_mm3': 8, 'closed': True})

    def test_material_removal_uses_measured_before_volume_and_its_own_tolerance(self):
        sample = {'slug': 'thread', 'expected': {'volume_mm3': 8, 'relative_volume_tolerance': 0.02,
                  'volume_less_than_mm3': 10, 'material_removed_mm3': 2, 'relative_removal_tolerance': 0.01}}
        before = {'measurement': {'total': {'volume_mm3': 10}}}
        capture.verify(self.control, sample, 'after', before=before)
        self.measurement['total']['volume_mm3'] = 8.1
        with self.assertRaisesRegex(RuntimeError, 'remov'):
            capture.verify(self.control, sample, 'after', before=before)
        self.measurement['total']['volume_mm3'] = 10
        with self.assertRaisesRegex(RuntimeError, 'ceiling|less than'):
            self.verify({'volume_less_than_mm3': 10})

    def test_pair_focus_marker_must_fit_the_retained_pane(self):
        sample = {'slug': 'focus', 'presentation': 'pair', 'camera': {'distance': 10}, 'focus_point': [0, 0, 0]}
        self.control.execute.side_effect = None
        self.control.call.side_effect = lambda method, params=None: [[584, 516]] if method == 'ui.worldToScreen' else {}
        with tempfile.TemporaryDirectory() as folder, patch.object(capture, 'shot', return_value=Path(folder)/'frame.png'), patch.object(capture.subprocess, 'run'):
            with self.assertRaisesRegex(RuntimeError, 'focus point'):
                capture.capture_state(self.control, sample, 'after', Path(folder))
            sample['presentation'] = 'single'
            capture.capture_state(self.control, sample, 'after', Path(folder))

    def test_failed_dark_conversion_leaves_both_previous_exports_intact(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder)
            for theme in ('light', 'dark'):
                (output/f'box-{theme}.png').write_bytes(theme.encode())
            def convert(command, **kwargs):
                destination = Path(command[-1])
                if 'dark' in destination.name:
                    raise subprocess.CalledProcessError(1, command)
                destination.write_bytes(b'new image')
            with patch.object(capture.subprocess, 'run', side_effect=convert):
                with self.assertRaises(subprocess.CalledProcessError):
                    capture.compose({'slug': 'box', 'presentation': 'single'}, {}, {'light': 'light.png', 'dark': 'dark.png'}, output)
            self.assertEqual((output/'box-light.png').read_bytes(), b'light')
            self.assertEqual((output/'box-dark.png').read_bytes(), b'dark')
            self.assertEqual(sorted(path.name for path in output.iterdir()), ['box-dark.png', 'box-light.png'])

    def test_duplicate_slugs_fail_before_control_or_output_creation(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root/'recipes.json'
            source.write_text(json.dumps({'samples': [{'slug': 'move'}]}))
            for only in [[], ['--only', 'move']]:
                output = root/'exports'
                argv = ['capture.py', '--port', '1234', '--token-dir', str(root), '--anchor', str(root/'anchor.sc'),
                        '--source', str(source), '--source', str(source), '--output', str(output), '--scratch', str(root/'work'), *only]
                with self.subTest(only=only), patch.object(sys, 'argv', argv), patch.object(capture, 'Control') as control, patch.object(capture, 'capture'), patch('sys.stderr'):
                    with self.assertRaises(SystemExit):
                        capture.main()
                    control.assert_not_called()
                    self.assertFalse(output.exists())

    def test_lost_creation_reply_leaves_unmarked_tabs_untouched(self):
        window = Window('lost_reply')
        with self.assertRaisesRegex(RuntimeError, 'identity is uncertain'):
            with capture.scratch_document(window):
                self.fail('A lost creation reply must not start capture')
        self.assertEqual(window.closed, [])
        self.assertEqual(len(window.tabs), 2)
        self.assertFalse(window.anchor_dirty)

    def test_uncertain_creation_never_closes_or_edits_the_anchor_or_other_work(self):
        for creation in ['not_created', 'ambiguous']:
            window = Window(creation)
            with self.subTest(creation=creation), self.assertRaises((OSError, RuntimeError)):
                with capture.scratch_document(window):
                    self.fail('Unconfirmed creation must not start capture')
            self.assertEqual(window.closed, [])
            self.assertFalse(window.anchor_dirty)

    def test_animation_screenshot_failure_already_restores_capture_flag(self):
        spec = importlib.util.spec_from_file_location('animation', Path(capture.__file__).with_name('capture-sketch-animation.py'))
        animation = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(animation)
        window = Window()
        original = dict(window.hint_style)
        sample = {'slug': 'line', 'command': 'sketch.line', 'setup': [], 'camera': {}, 'crop_scale': 1, 'events': [{'sketch': [0, 0]}]}
        with tempfile.TemporaryDirectory() as folder, patch.object(animation.capture, 'shot', side_effect=RuntimeError('Screenshot failed')):
            with self.assertRaisesRegex(RuntimeError, 'Screenshot failed'):
                animation.animate(window, sample, 'light', Path(folder), Path(folder), 25, 75)
        self.assertEqual(window.hint_style, original)
        self.assertFalse(window.anchor_dirty)

    def test_pending_batch_reply_is_polled_on_the_same_connection_without_repeating_work(self):
        control = capture.Control.__new__(capture.Control)
        control.port, control.token = 1234, 'test-token'
        replies = [
            {'ok': False, 'running': True, 'error': 'still running'},
            {'ok': True, 'result': {'pending': 1, 'done': False}},
            {'ok': True, 'result': {'pending': 1, 'done': True, 'reply': {'ok': True, 'result': {'completed': 2}}}},
        ]
        with patch.object(capture.socket, 'create_connection') as connect, patch.object(capture.time, 'sleep'):
            sock = connect.return_value.__enter__.return_value
            sock.makefile.return_value.__enter__.return_value.readline.side_effect = [json.dumps(reply) for reply in replies]
            result = control.call('engine.batch', {'commands': []})
            self.assertEqual(result, {'completed': 2})
            self.assertEqual(connect.call_count, 1)
            methods = [json.loads(call.args[0])['method'] for call in sock.sendall.call_args_list]
            self.assertEqual(methods, ['engine.batch', 'control.pending', 'control.pending'])
            self.assertFalse(control.pending_work)

    def test_pending_wait_is_bounded_and_scratch_cleanup_does_not_close_running_work(self):
        control = capture.Control.__new__(capture.Control)
        control.port, control.token = 1234, 'test-token'
        with patch.object(capture.socket, 'create_connection') as connect, patch.object(capture.time, 'monotonic', side_effect=[0, capture.PENDING_TIMEOUT + 1]):
            connect.return_value.__enter__.return_value.makefile.return_value.__enter__.return_value.readline.return_value = json.dumps({'ok': False, 'running': True})
            with self.assertRaisesRegex(RuntimeError, 'still running'):
                control.call('engine.batch')
            self.assertTrue(control.pending_work)
        window = Window()
        with self.assertRaisesRegex(RuntimeError, 'still pending'):
            with capture.scratch_document(window):
                window.pending_work = True
        self.assertEqual(window.closed, [])
        self.assertFalse(window.anchor_dirty)

    def test_changed_scratch_identity_prevents_destructive_cleanup(self):
        window = Window()
        with self.assertRaisesRegex(RuntimeError, 'identity is uncertain'):
            with capture.scratch_document(window):
                window.metadata = {'other.tool': {'unsaved': 'work'}}
        self.assertEqual(window.closed, [])
        self.assertFalse(window.anchor_dirty)

    def test_successful_compose_finishes_both_conversions_before_publication(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder)
            for theme in ('light', 'dark'):
                (output/f'box-{theme}.png').write_bytes(theme.encode())
            def convert(command, **kwargs):
                for theme in ('light', 'dark'):
                    self.assertEqual((output/f'box-{theme}.png').read_bytes(), theme.encode())
                Path(command[-1]).write_bytes(b'new image')
            with patch.object(capture.subprocess, 'run', side_effect=convert):
                capture.compose({'slug': 'box', 'presentation': 'single'}, {}, {'light': 'light.png', 'dark': 'dark.png'}, output)
            for theme in ('light', 'dark'):
                self.assertEqual((output/f'box-{theme}.png').read_bytes(), b'new image')

    def test_failed_second_publication_restores_the_previous_theme_pair(self):
        with tempfile.TemporaryDirectory() as folder:
            output = Path(folder)
            for theme in ('light', 'dark'):
                (output/f'box-{theme}.png').write_bytes(theme.encode())
            replace = capture.os.replace
            def publish(source, destination):
                if Path(source).name == 'box-dark.png' and Path(source).parent.name != 'previous':
                    raise OSError('Second publication failed')
                replace(source, destination)
            with patch.object(capture.os, 'replace', side_effect=publish):
                with self.assertRaisesRegex(OSError, 'Second publication'):
                    with capture.staged_exports(output, ['box-light.png', 'box-dark.png']) as staging:
                        for theme in ('light', 'dark'):
                            (staging/f'box-{theme}.png').write_bytes(b'new image')
            for theme in ('light', 'dark'):
                self.assertEqual((output/f'box-{theme}.png').read_bytes(), theme.encode())
            self.assertEqual(sorted(path.name for path in output.iterdir()), ['box-dark.png', 'box-light.png'])

    def test_tagged_scratch_is_recovered_if_the_metadata_reply_is_lost(self):
        class LostTag(Window):
            def execute(self, command, params=None):
                result = super().execute(command, params)
                if command == 'document.metadata.set':
                    raise OSError('Metadata reply lost')
                return result
        window = LostTag()
        with self.assertRaisesRegex(OSError, 'Metadata reply lost'):
            with capture.scratch_document(window):
                self.fail('Lost reply must not start capture')
        self.assertEqual(window.closed, [(1, True)])
        self.assertFalse(window.anchor_dirty)

    def test_global_running_work_prevents_recovery_and_preflight(self):
        window = Window()
        call = window.call
        def running(method, params=None):
            return {'running': [{'method': 'ui.documents'}]} if method == 'control.running' else call(method, params)
        window.call = running
        with self.assertRaisesRegex(RuntimeError, 'still be processing'):
            with capture.scratch_document(window):
                pass
        self.assertEqual(window.closed, [])
        control = capture.Control.__new__(capture.Control)
        control.call = MagicMock(return_value={'running': [{'method': 'engine.batch'}]})
        with self.assertRaisesRegex(RuntimeError, 'still be processing'):
            control.preflight()
        control.call.assert_called_once_with('control.running')

    def test_interruption_after_rename_rolls_back_both_themes(self):
        for after_rename in [False, True]:
            with self.subTest(after_rename=after_rename), tempfile.TemporaryDirectory() as folder:
                output = Path(folder)
                for theme in ('light', 'dark'):
                    (output/f'box-{theme}.png').write_bytes(theme.encode())
                replace = capture.os.replace
                def publish(source, destination):
                    if Path(source).name == 'box-dark.png' and Path(source).parent.name != 'previous':
                        if after_rename:
                            replace(source, destination)
                        raise KeyboardInterrupt()
                    replace(source, destination)
                with patch.object(capture.os, 'replace', side_effect=publish):
                    with self.assertRaises(KeyboardInterrupt):
                        with capture.staged_exports(output, ['box-light.png', 'box-dark.png']) as staging:
                            for theme in ('light', 'dark'):
                                (staging/f'box-{theme}.png').write_bytes(b'new image')
                for theme in ('light', 'dark'):
                    self.assertEqual((output/f'box-{theme}.png').read_bytes(), theme.encode())

    def test_custom_source_slugs_cannot_escape_the_export_directory(self):
        for slug in ['../outside', '/absolute', 'folder/name', r'folder\name', '', '..']:
            with self.subTest(slug=slug), self.assertRaisesRegex(ValueError, 'Invalid recipe slug'):
                capture.select_samples([{'slug': slug}])

    def test_lost_restore_reply_does_not_close_while_that_mutation_is_running(self):
        class LostRestore(Window):
            running = False

            def call(self, method, params=None):
                if method == 'control.running':
                    return {'running': [{'method': 'ui.set'}] if self.running else []}
                if method == 'ui.set':
                    self.running = True
                    raise OSError('Restoration reply lost')
                return super().call(method, params)

        window = LostRestore()
        with self.assertRaisesRegex(RuntimeError, 'still be processing'):
            with capture.scratch_document(window):
                pass
        self.assertEqual(window.closed, [])
        self.assertFalse(window.anchor_dirty)

    def test_crop_rejects_upsampling_and_accepts_sufficient_pixel_density(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'capture.png'
            for scale, crop_scale, accepted in [(1, 1, True), (1, 0.5, False), (2, 0.5, True), (2, 0.25, False), (4, 0.25, True)]:
                with self.subTest(scale=scale, crop_scale=crop_scale), patch.object(capture.time, 'sleep'), patch.object(capture.subprocess, 'run') as run:
                    self.control.call.return_value = {'width': 1728 * scale, 'height': 998 * scale}
                    if accepted:
                        capture.shot(self.control, path, crop_scale)
                        run.assert_called_once()
                    else:
                        with self.assertRaisesRegex(RuntimeError, 'density'):
                            capture.shot(self.control, path, crop_scale)
                        run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
