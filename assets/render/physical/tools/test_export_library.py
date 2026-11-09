"""Regression checks for explicit, no-loss physical-library export planning."""
import copy
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import export_library as e
import physical as p


def modern_registry():
    rows = [{'id': name, 'params': ''} for name in ('file.new', 'material.create', 'material.inspect', 'library.export', 'library.inspect')]
    rows[1]['params'] = 'physical_card: schema 1'
    rows[3]['params'] = 'format solidcraft-library/2'
    return rows


def projected_catalog(plan):
    entries = []
    for i, row in enumerate(plan['materials']):
        params = row['command']['params']
        entries.append({'id': e.PACK_ID + '/material-' + str(i), 'revision': 'a'*64,
                        'definition': {'kind': 'material', 'value': copy.deepcopy(params)}})
    return {'format': 'solidcraft-library/2', 'id': e.PACK_ID, 'entries': entries}


class ExportLibraryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.plan = e.selection(None, True)

    def test_plan_covers_every_point_density_without_part_assignment(self):
        self.assertEqual(len(self.plan['materials']), 46)
        self.assertEqual(len(self.plan['omitted']), 13)
        self.assertFalse(self.plan['assigns_bodies'])
        self.assertFalse(self.plan['executed'])
        self.assertTrue(self.plan['preserve_extended_profiles'])
        ids = {x['profile_id'] for x in self.plan['materials']}
        omitted = {x['profile_id'] for x in self.plan['omitted']}
        self.assertFalse(ids & omitted)
        self.assertEqual(len(ids | omitted), 59)
        for row in self.plan['materials']:
            self.assertEqual(row['command']['command'], 'material.create')
            self.assertEqual(set(row['command']['params']), {'name', 'density_g_cm3', 'physical_card'})

    def test_acknowledgement_unknown_duplicate_and_empty_selections_refuse(self):
        for ids, acknowledge in [(None, False), ([], True), (['unknown'], True), (['steel-c45']*2, True)]:
            with self.subTest(ids=ids), self.assertRaises(p.Invalid):
                e.selection(ids, acknowledge)

    def test_ranges_and_coatings_without_density_are_not_assigned(self):
        omitted = {x['profile_id'] for x in self.plan['omitted']}
        self.assertTrue({'c71-enamel', 'hmg-stoving-hammer', 'hmg-sc601-clear', 'uv22-coating'} <= omitted)
        with self.assertRaises(p.Invalid):
            e.selection(['c71-enamel'], True)
        plan = e.selection(['pmma-goodfellow-sheet'], True)
        params = plan['materials'][0]['command']['params']
        self.assertAlmostEqual(params['density_g_cm3'], 1.19)
        self.assertNotIn('elasticity', {r['data']['kind'] for r in params['physical_card']['properties']})

    def test_old_scalar_registry_refuses_and_modern_contract_is_recognized(self):
        e.registry_check(modern_registry())
        for field, replacement in [(1, 'density and colour only'), (3, 'scalar library/1')]:
            rows = modern_registry()
            rows[field]['params'] = replacement
            with self.assertRaises(p.Invalid):
                e.registry_check(rows)
        for value in [None, {}, [None], modern_registry()+[modern_registry()[0]]]:
            with self.subTest(value=value), self.assertRaises(p.Invalid):
                e.registry_check(value)

    def test_catalog_retains_every_native_value_source_and_condition(self):
        catalog = projected_catalog(self.plan)
        e.verify_catalog(catalog, self.plan)
        for mutation in ('drop_card', 'density', 'appearance', 'source', 'state', 'revision', 'duplicate', 'missing'):
            value = copy.deepcopy(catalog)
            material = value['entries'][0]['definition']['value']
            if mutation == 'drop_card':material.pop('physical_card')
            elif mutation == 'density':material['density_g_cm3'] *= 2
            elif mutation == 'appearance':material['appearance'] = {'name': 'unrequested optical change'}
            elif mutation == 'source':material['physical_card']['sources'][0]['reference'] = 'changed'
            elif mutation == 'state':material['physical_card']['properties'][0]['conditions']['state'] = 'wrong condition'
            elif mutation == 'revision':value['entries'][0]['revision'] = 'not a revision'
            elif mutation == 'duplicate':value['entries'][1] = copy.deepcopy(value['entries'][0])
            elif mutation == 'missing':value['entries'].pop()
            with self.subTest(mutation=mutation), self.assertRaises(p.Invalid):
                e.verify_catalog(value, self.plan)

    def test_legacy_refusal_does_not_create_output_or_run_mutations(self):
        rows = modern_registry()
        rows[1]['params'] = 'no card persistence'
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cli = root/'trusted-cli-fixture'
            cli.write_bytes(b'unit test marker; not executable')
            with patch.object(p, 'ROOT', root), patch.object(e, 'invoke', return_value=rows) as invoke:
                with self.assertRaises(p.Invalid):e.export(cli, self.plan)
                invoke.assert_called_once_with(cli.resolve(), ['commands'])
                self.assertFalse((root/'target').exists())

    def test_stale_plan_refuses_before_executable_invocation(self):
        plan = copy.deepcopy(self.plan)
        plan['input_sha256']['profiles.json'] = '0'*64
        with tempfile.TemporaryDirectory() as temporary:
            cli = Path(temporary)/'fixture'
            cli.write_bytes(b'fixture')
            with patch.object(e, 'invoke') as invoke:
                with self.assertRaises(p.Invalid):e.export(cli, plan)
                invoke.assert_not_called()

    def test_library_command_has_no_overwrite_and_no_body_targets(self):
        command = e.library_command(Path('review.json'))
        self.assertEqual(command['command'], 'library.export')
        self.assertEqual(command['params']['format'], 'solidcraft-library/2')
        self.assertIs(command['params']['overwrite'], False)
        self.assertNotIn('targets', command['params'])


if __name__ == '__main__':
    unittest.main()
