"""Regression tests for reference data, physical scope and loss-aware projection."""
import copy
import json
import math
from pathlib import Path
import tempfile
import unittest

import physical as p

class PhysicalTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data,cls.sources,cls.bindings,cls.registry=[p.read(p.PACK/name) for name in ('profiles.json','sources.json','bindings.json','quantities.json')]
        cls.profiles,cls.source_map=p.validate_data(cls.data,cls.sources,cls.bindings,cls.registry)

    def mutated(self,which,change):
        args=copy.deepcopy([self.data,self.sources,self.bindings,self.registry]);change(args[which])
        with self.assertRaises((p.Invalid,KeyError,TypeError)):p.validate_data(*args)

    def row(self,id,quantity):
        return next(x for x in self.profiles[id]['properties'] if x['quantity']==quantity)

    def test_all_profiles_and_associations_validate(self):
        self.assertGreaterEqual(len(self.profiles),51)
        self.assertEqual(len(self.bindings['catalog']),600)
        self.assertEqual(len(self.bindings['surfaces']),48)

    def test_duplicate_json_fields_and_nonfinite_literals_refuse(self):
        with self.assertRaises(p.Invalid):json.loads('{"density":1,"density":2}',object_pairs_hook=p.unique)
        for value in ('NaN','Infinity','-Infinity'):
            with self.assertRaises(p.Invalid):json.loads('{"value":'+value+'}',parse_constant=p.reject_constant)

    def test_numeric_boolean_text_infinity_and_huge_integers_refuse(self):
        for v in (True,False,'1',float('nan'),float('inf'),10**400,-10**400):
            with self.subTest(v=repr(v)[:20]),self.assertRaises(p.Invalid):p.number(v)

    def test_units_and_source_arithmetic_are_checked(self):
        row=copy.deepcopy(self.row('aluminium-6061-t6','young_modulus'))
        for change in (lambda x:x.update(unit='MPa'),lambda x:x.update(value=x['value']/1000),
                       lambda x:x['original'].update(scale_to_si=1),lambda x:x['original'].update(unit='degC')):
            bad=copy.deepcopy(row);change(bad)
            with self.assertRaises(p.Invalid):p.check_property(bad,self.source_map)

    def test_temperature_conversions_are_affine_not_scalar(self):
        r=self.row('epoxy-690','heat_deflection')
        self.assertAlmostEqual(r['value'],(115-32)*5/9+273.15,places=9)
        self.assertIsNone(r['conditions']['temperature_k'])
        self.assertAlmostEqual(self.row('pom-c','glass_transition')['value'],213.15)

    def test_electrical_resistivity_and_breakdown_have_distinct_units(self):
        self.assertAlmostEqual(self.row('steel-304-plate','volume_resistivity')['value'],74e-8)
        self.assertAlmostEqual(self.row('ptfe','dielectric_strength')['value'],80e6)
        self.assertAlmostEqual(self.row('silicone-ds20','dielectric_strength')['value'],350/.0000254,delta=1e-4)

    def test_ranges_never_become_a_midpoint(self):
        result=p.native_card(self.profiles['tpu95-feedstock'],self.source_map)
        self.assertFalse(any(r['data']['kind']=='density' for r in result['card']['properties']))
        with self.assertRaises(p.Invalid):p.prepare('tpu95-feedstock',True)

    def test_reversed_range_and_wrong_range_qualifier_refuse(self):
        r=copy.deepcopy(self.row('tpu95-feedstock','density'));r['value'].reverse()
        with self.assertRaises(p.Invalid):p.check_property(r,self.source_map)
        r=copy.deepcopy(self.row('tpu95-feedstock','density'));r['qualifier']='typical'
        with self.assertRaises(p.Invalid):p.check_property(r,self.source_map)

    def test_limits_are_not_native_nominal_strengths(self):
        result=p.native_card(self.profiles['titanium-grade5'],self.source_map)
        kinds={r['data']['kind'] for r in result['card']['properties']}
        self.assertNotIn('yield_strength',kinds);self.assertNotIn('tensile_strength',kinds)

    def test_missing_poisson_ratio_does_not_invent_elasticity(self):
        for id in ('steel-304-plate','peek','pa6-stock','epoxy-690'):
            result=p.native_card(self.profiles[id],self.source_map)
            self.assertNotIn('elasticity',{r['data']['kind'] for r in result['card']['properties']})

    def test_native_steel_c45_analytic_elastic_constants(self):
        result=p.native_card(self.profiles['steel-c45'],self.source_map)
        model=next(r['data']['model'] for r in result['card']['properties'] if r['data']['kind']=='elasticity')
        self.assertEqual(model['young_modulus_pa'],210e9)
        self.assertAlmostEqual(model['young_modulus_pa']/(2*(1+model['poisson_ratio'])),80.76923076923077e9)

    def test_derived_poisson_is_not_presented_as_measured(self):
        result=p.native_card(self.profiles['c11000'],self.source_map)
        elastic=next(r for r in result['card']['properties'] if r['data']['kind']=='elasticity')
        self.assertIn('derived',elastic['conditions']['state'])
        self.assertAlmostEqual(elastic['data']['model']['poisson_ratio'],17000/(2*6400)-1)

    def test_derived_value_without_receipt_refuses(self):
        r=copy.deepcopy(self.row('c11000','poisson_ratio'));del r['derivation']
        with self.assertRaises(p.Invalid):p.check_property(r,self.source_map)

    def test_wood_moisture_density_not_dry_mass_specific_gravity(self):
        r=self.row('wood-white-oak','density')
        self.assertAlmostEqual(r['value'],761.6)
        self.assertNotEqual(r['value'],680)
        self.assertEqual(r['conditions']['moisture_dry_basis_kg_kg'],.12)

    def test_wood_flexure_not_isotropic_young_modulus(self):
        result=p.native_card(self.profiles['wood-white-oak'],self.source_map)
        self.assertNotIn('elasticity',{r['data']['kind'] for r in result['card']['properties']})
        self.assertNotIn('compressive_strength',{r['data']['kind'] for r in result['card']['properties']})
        self.assertIn('flexural_modulus',{r['quantity'] for r in result['not_projected']})

    def test_print_directions_and_uncertainty_are_preserved(self):
        profile=self.profiles['pla-printed']
        strengths=[x for x in profile['properties'] if x['quantity']=='tensile_strength']
        self.assertEqual({x['conditions']['direction'] for x in strengths},{'XY','Z'})
        self.assertTrue(all(x['uncertainty'] is not None for x in strengths))
        self.assertNotIn('tensile_strength',{r['data']['kind'] for r in p.native_card(profile,self.source_map)['card']['properties']})

    def test_finite_strain_stress_never_becomes_elastic_modulus(self):
        profile=self.profiles['silicone-ds20']
        self.assertEqual(self.row('silicone-ds20','stress_at_strain')['conditions']['engineering_strain'],1)
        self.assertNotIn('young_modulus',{r['quantity'] for r in profile['properties']})

    def test_source_conflicts_stay_quarantined(self):
        profile=self.profiles['abs-printed']
        self.assertTrue(profile['quarantined_source_ids'])
        self.assertNotIn('density',{r['quantity'] for r in profile['properties']})
        with self.assertRaises(p.Invalid):p.prepare('abs-printed',True)

    def test_fr4_frequency_and_cte_regimes_stay_distinct(self):
        profile=self.profiles['fr4-370hr']
        eps=[r for r in profile['properties'] if r['quantity']=='relative_permittivity']
        self.assertEqual(len({r['conditions']['frequency_hz'] for r in eps}),5)
        z=[r for r in profile['properties'] if r['quantity']=='thermal_expansion' and r['conditions']['direction']=='Z']
        self.assertEqual({r['conditions']['thermal_regime'] for r in z},{'pre-Tg','post-Tg'})
        self.assertNotIn('elasticity',{r['data']['kind'] for r in p.native_card(profile,self.source_map)['card']['properties']})

    def test_silica_and_sapphire_are_not_the_same_material_model(self):
        self.assertTrue(self.profiles['fused-silica']['isotropic_reference_model'])
        self.assertFalse(self.profiles['sapphire']['isotropic_reference_model'])
        self.assertNotIn('thermal_expansion',{r['data']['kind'] for r in p.native_card(self.profiles['sapphire'],self.source_map)['card']['properties']})

    def test_references_do_not_relicense_supplier_publications(self):
        self.assertIsNone(self.source_map['isola-370hr']['license'])
        self.assertIsNone(self.source_map['schott-borofloat']['license'])

    def test_simulant_bulk_and_grain_density_remain_distinct(self):
        self.assertEqual(self.row('lms-1','density')['value'],1560)
        self.assertEqual(self.row('lms-1','grain_density')['value'],2920)
        self.assertIn('NOT measured lunar soil',self.profiles['lms-1']['reference_state'])

    def test_coating_preserves_substrate_and_has_no_invented_density(self):
        result=p.resolve('SURF-045')
        self.assertEqual(result['binding']['physical_role'],'layer')
        self.assertTrue(result['binding']['preserve_existing_material'])
        self.assertFalse(result['binding']['auto_assign_body'])
        with self.assertRaises(p.Invalid):p.prepare('az93-coating',True)

    def test_no_preparation_without_explicit_reference_acknowledgement(self):
        with self.assertRaises(p.Invalid):p.prepare('steel-c45',False)
        result=p.prepare('steel-c45',True)
        self.assertFalse(result['executed']);self.assertFalse(result['assigns_bodies'])
        self.assertEqual(result['commands'][0]['params']['density_g_cm3'],7.8)
        self.assertEqual(result['commands'][0]['command'],'material.create')
        self.assertNotIn('appearance',result['commands'][0]['params'])

    def test_unresolved_ids_fail_not_fallback_to_generic_material(self):
        for id in ('MAT-999','SURF-999','__proto__',''):
            with self.assertRaises(p.Invalid):p.resolve(id)

    def test_false_ready_certification_and_zero_unknown_claims_refuse(self):
        self.mutated(0,lambda d:d.update(runtime_assignment=True))
        self.mutated(0,lambda d:d['profiles'][0].update(certified_design_allowables=True))
        self.mutated(2,lambda d:d['catalog'][0].update(unknown_is_zero=True))
        self.mutated(2,lambda d:d['catalog'][0].update(auto_assign_body=True))

    def test_duplicate_profiles_sources_properties_refuse(self):
        self.mutated(0,lambda d:d['profiles'].append(d['profiles'][0]))
        self.mutated(1,lambda d:d['sources'].append(d['sources'][0]))
        self.mutated(0,lambda d:d['profiles'][0]['properties'].append(d['profiles'][0]['properties'][0]))

    def test_catalog_surface_drift_and_missing_records_refuse(self):
        self.mutated(2,lambda d:d['catalog'].pop())
        self.mutated(2,lambda d:d['surfaces'].pop())
        self.mutated(2,lambda d:d['surfaces'][0].update(physical_binding_ids=['MAT-003']))
        self.mutated(2,lambda d:d['inputs']['catalog'].update(sha256='0'*64))
        self.mutated(2,lambda d:d['inputs']['catalog'].update(path='../../secret'))

    def test_undefined_property_source_and_profile_refuse(self):
        self.mutated(0,lambda d:d['profiles'][0]['properties'][0].update(source_id='absent'))
        self.mutated(2,lambda d:d['catalog'][0]['references'][0].update(profile_id='absent'))

    def test_nonfinite_frequency_and_unknown_conditions_refuse(self):
        r=copy.deepcopy(self.row('fr4-370hr','relative_permittivity'))
        r['conditions']['frequency_hz']=float('nan')
        with self.assertRaises(p.Invalid):p.check_property(r,self.source_map)
        r=copy.deepcopy(self.row('fr4-370hr','relative_permittivity'));r['conditions']['temperature']=20
        with self.assertRaises(p.Invalid):p.check_property(r,self.source_map)

    def test_negative_and_impossible_fraction_refuse(self):
        for id,q,value in [('steel-c45','density',-1),('lms-1','porosity',2)]:
            r=copy.deepcopy(self.row(id,q));r['value']=value
            r['original']={'value':value,'unit':r['unit'],'scale_to_si':1.,'offset_to_si':0.}
            with self.assertRaises(p.Invalid):p.check_property(r,self.source_map)

    def test_native_projection_has_bounded_unique_kinds(self):
        for profile in self.profiles.values():
            card=p.native_card(profile,self.source_map)['card']
            self.assertLessEqual(len(card['properties']),16)
            kinds=[r['data']['kind'] for r in card['properties']]
            self.assertEqual(len(kinds),len(set(kinds)))
            self.assertTrue(all(len(r['conditions']['state'].encode())<=1024 for r in card['properties']))

    def test_authority_bytes_and_exports_are_deterministic(self):
        first=p.make_exports();second=p.make_exports()
        self.assertEqual(p.encoded(first),p.encoded(second))
        self.assertFalse(first['coverage.json']['numerically_complete_catalog_claim'])

    def test_bounded_input_and_array_top_level_refuse(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'input.json';path.write_text('[]')
            with self.assertRaises(p.Invalid):p.read(path)
            with path.open('wb') as stream:stream.truncate(p.MAX_BYTES+1)
            with self.assertRaises(p.Invalid):p.read(path)

    def test_empty_native_projection_is_explicit_not_reported_ready(self):
        for id in ('fr4-370hr','az93-coating','mdf-premier-18'):
            result=p.native_card(self.profiles[id],self.source_map)
            self.assertEqual(result['native_projection_status'],'extended_data_only')
            self.assertFalse(result['card']['properties'])
            self.assertTrue(result['not_projected'])

    def test_wrong_chemistry_is_not_a_convenient_density_fallback(self):
        by_id={row['catalog_id']:row for row in self.bindings['catalog']}
        for id,wrong in [('MAT-145','hdpe'),('MAT-248','borofloat-33'),('MAT-439','mdf-premier-18')]:
            self.assertNotIn(wrong,{r['profile_id'] for r in by_id[id]['references']})
        self.assertNotIn('c11000',{r['profile_id'] for r in by_id['MAT-398']['references']})

    def test_no_reference_profile_is_duplicated_with_two_roles(self):
        for row in self.bindings['catalog']:
            ids=[r['profile_id'] for r in row['references']]
            self.assertEqual(len(ids),len(set(ids)),row['catalog_id'])

    def test_incomplete_coverage_is_not_reported_numerically_complete(self):
        c=p.make_exports()['coverage.json']
        self.assertEqual(c['nonempty_native_cards']+c['extended_only_profiles'],c['profile_count'])
        self.assertEqual(c['concepts_with_reference_candidates']+c['concepts_needing_specific_recipe'],600)
        self.assertFalse(c['numerically_complete_catalog_claim'])
        self.assertEqual(c['surfaces_with_reference_candidates'],48)

    def test_cured_resin_state_survives_native_projection(self):
        result=p.native_card(self.profiles['sikafloor161-cured'],self.source_map)
        row=next(r for r in result['card']['properties'] if r['data']['kind']=='compressive_strength')
        self.assertIn('28 days',row['conditions']['state'])
        self.assertEqual(row['conditions']['temperature_k'],296.15)
        self.assertNotIn('density',{r['data']['kind'] for r in result['card']['properties']})

    def test_distinct_elastic_conditions_cannot_be_mixed(self):
        for field,value in [('state','other temper'),('moisture_dry_basis_kg_kg',.1),('strain_rate_per_s',1.0)]:
            profile=copy.deepcopy(self.profiles['steel-c45'])
            nu=next(r for r in profile['properties'] if r['quantity']=='poisson_ratio')
            nu['conditions'][field]=value
            with self.subTest(field=field),self.assertRaises(p.Invalid):
                p.native_card(profile,self.source_map)

    def test_new_reference_ranges_and_fibre_axes_are_not_flattened(self):
        pmma=p.native_card(self.profiles['pmma-goodfellow-sheet'],self.source_map)
        self.assertNotIn('elasticity',{r['data']['kind'] for r in pmma['card']['properties']})
        fibre=p.native_card(self.profiles['kevlar49-fibre'],self.source_map)
        self.assertNotIn('elasticity',{r['data']['kind'] for r in fibre['card']['properties']})
        self.assertNotIn('tensile_strength',{r['data']['kind'] for r in fibre['card']['properties']})
        self.assertNotIn('thermal_expansion',{r['data']['kind'] for r in fibre['card']['properties']})

    def test_named_coating_candidates_do_not_invent_bulk_density(self):
        for id in ('c71-enamel','hmg-stoving-hammer','hmg-sc601-clear','uv22-coating'):
            profile=self.profiles[id]
            self.assertNotIn('density',{r['quantity'] for r in profile['properties']})
            with self.assertRaises(p.Invalid):p.prepare(id,True)

    def test_para_aramid_not_assigned_as_meta_aramid_honeycomb(self):
        row=next(r for r in self.bindings['catalog'] if r['catalog_id']=='MAT-482')
        self.assertNotIn('kevlar49-fibre',{r['profile_id'] for r in row['references']})

if __name__=='__main__':unittest.main()
