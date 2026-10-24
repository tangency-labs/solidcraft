"""Independent projection/identity regressions for the original SELENE recipe."""
from __future__ import annotations
import copy
import math
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from selene_recipe import fit_camera, hero_request


class CameraFitTests(unittest.TestCase):
    def fixture(self):
        return {'bounds': {'min': [-200, -245, 0], 'max': [472, 245, 293]},
                'bodies': [{'name': 'subject', 'bounds': {'min': [-200, -245, 0], 'max': [200, 245, 293]}},
                           {'name': 'extended gripper', 'bounds': {'min': [400, -20, 200], 'max': [472, 20, 255]}}]}

    def test_all_native_box_corners_stay_inside_the_explicit_frame_margin(self):
        import itertools
        source = self.fixture()
        before = copy.deepcopy(source)
        for aspect in (0.6, 1.0, 1.6, 2.0):
            for yaw in (-2.2, -0.66, 0.0, 1.3):
                camera = fit_camera(source, aspect, yaw=yaw)
                # Independently project via an explicit yaw/pitch camera matrix.
                sy, cy = math.sin(yaw), math.cos(yaw)
                sp, cp = math.sin(camera['pitch']), math.cos(camera['pitch'])
                tan = math.tan(camera['fov'] / 2)
                for body in source['bodies']:
                    for point in itertools.product(*zip(body['bounds']['min'], body['bounds']['max'])):
                        x, y, z = [p-t for p, t in zip(point, camera['target'])]
                        rx, ry = cy*x-sy*y, sy*sp*x+cy*sp*y+cp*z
                        depth = camera['distance']-(-sy*cp*x-cy*cp*y+sp*z)
                        self.assertGreater(depth, 0)
                        self.assertLessEqual(abs(rx)/(depth*tan*aspect), 0.84)
                        self.assertLessEqual(abs(ry)/(depth*tan), 0.84)
        self.assertEqual(source, before)

    def test_fit_is_scale_covariant_and_rejects_unbounded_inputs(self):
        source = self.fixture()
        original = fit_camera(source, 1.6)
        for multiplier in (0.01, 10.0):
            scaled = copy.deepcopy(source)
            for bounds in [scaled['bounds'], *(body['bounds'] for body in scaled['bodies'])]:
                for key in ('min', 'max'):
                    bounds[key] = [v*multiplier for v in bounds[key]]
            actual = fit_camera(scaled, 1.6)
            self.assertAlmostEqual(actual['distance'], original['distance']*multiplier, delta=1e-8)
        for kwargs in ({'aspect': 0}, {'aspect': float('nan')}, {'aspect': 1, 'fov': 0},
                       {'aspect': 1, 'margin': 0.5}, {'aspect': 1, 'pitch': float('inf')}):
            with self.assertRaises(ValueError):
                fit_camera(source, **kwargs)
        with self.assertRaises(ValueError):
            fit_camera({'bounds': source['bounds'], 'bodies': []}, 1.0)

    def test_request_validation_happens_before_native_commands(self):
        class NeverCalled:
            def __getattr__(self, name):
                raise AssertionError('Invalid request reached the native studio: '+name)
        for values in ({'width': 0}, {'height': 8193}, {'width': 8192, 'height': 8192},
                       {'samples': 0}, {'samples': 4097}, {'samples': 0.5}):
            with self.assertRaises(ValueError):
                hero_request(NeverCalled(), **values)


if __name__ == '__main__':
    unittest.main()
