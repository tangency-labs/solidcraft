-- Explicit test fixtures for the development service. They are not company defaults.
insert into material_versions (material_version_id, name, density_g_cm3, price_per_kg, currency)
values ('00000000-0000-4000-8000-000000000001', 'PA12 test fixture', 2.7, 20, 'CNY')
on conflict (material_version_id) do nothing;

insert into material_versions (material_version_id, name, density_g_cm3, price_per_kg, currency)
values ('00000000-0000-4000-8000-000000000002', 'Aluminium test fixture', 2.7, 20, 'CNY')
on conflict (material_version_id) do nothing;

insert into rate_versions (rate_version_id, name, rate_per_hour, currency)
values ('00000000-0000-4000-8000-000000000101', 'Machining test fixture', 120, 'CNY')
on conflict (rate_version_id) do nothing;

insert into rate_versions (rate_version_id, name, rate_per_hour, currency)
values ('00000000-0000-4000-8000-000000000102', 'Setup test fixture', 120, 'CNY')
on conflict (rate_version_id) do nothing;
