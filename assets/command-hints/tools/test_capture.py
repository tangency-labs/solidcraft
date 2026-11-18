"""Regression checks for the command illustration authoring verifier."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch

import capture


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
        class Window:
            def __init__(self):
                self.active = 0
                self.count = 1
                self.anchor_dirty = False

            def preflight(self):
                if self.anchor_dirty:
                    raise RuntimeError('Anchor was changed')

            def call(self, method, params=None):
                if method == 'ui.inspect':
                    return {'ui': {}, 'visibility': {}, 'home': False}
                if method == 'ui.prefs':
                    return {'reference_geometry_through_bodies': False}
                if method == 'ui.documents':
                    if params:
                        if params['action'] == 'new':
                            self.active, self.count = 1, 2
                        elif params['action'] == 'close':
                            self.active, self.count = 0, 1
                        elif params['action'] == 'switch':
                            self.active = params['index']
                    return {'active': self.active, 'documents': [{}] * self.count}
                if method == 'ui.set' and self.active == 0:
                    self.anchor_dirty = True
                return {}

        window = Window()
        for _ in range(2):
            with capture.scratch_document(window):
                self.assertEqual(window.active, 1)
            self.assertEqual(window.active, 0)
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
