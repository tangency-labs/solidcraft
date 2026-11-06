create table sources (
    source_id uuid primary key,
    sha256 text not null unique check (sha256 ~ '^[0-9a-f]{64}$'),
    byte_len bigint not null check (byte_len > 0),
    original_name text not null check (length(original_name) > 0),
    bytes bytea not null,
    created_at timestamptz not null default now()
);

create table projects (
    project_id uuid primary key,
    product_revision_id uuid not null,
    current_version bigint not null default 0 check (current_version >= 0),
    source_id uuid not null references sources(source_id),
    currency text not null check (currency = 'CNY'),
    created_at timestamptz not null default now()
);

-- Append-only: the service inserts, and never updates or deletes a version.
create table project_versions (
    project_id uuid not null references projects(project_id),
    version bigint not null check (version >= 1),
    draft jsonb not null,
    source_id uuid not null references sources(source_id),
    created_at timestamptz not null default now(),
    primary key (project_id, version)
);

create table material_versions (
    material_version_id uuid primary key,
    name text not null,
    density_g_cm3 numeric not null check (density_g_cm3 > 0),
    price_per_kg numeric not null check (price_per_kg >= 0),
    currency text not null check (currency = 'CNY'),
    created_at timestamptz not null default now()
);

create table rate_versions (
    rate_version_id uuid primary key,
    name text not null,
    rate_per_hour numeric not null check (rate_per_hour > 0),
    currency text not null check (currency = 'CNY'),
    created_at timestamptz not null default now()
);

-- Append-only calculation snapshots: a later edit never rewrites an earlier result.
create table calculations (
    calculation_id uuid primary key,
    project_id uuid not null references projects(project_id),
    saved_version bigint not null check (saved_version >= 1),
    input_hash text not null,
    calculator_version text not null,
    inputs jsonb not null,
    outputs jsonb not null,
    issues jsonb not null,
    created_at timestamptz not null default now()
);

create index calculations_project on calculations (project_id, created_at);

-- One row per accepted request: a lost reply is answered from here on an explicit retry.
create table requests (
    request_id text primary key check (length(request_id) > 0),
    operation text not null,
    payload_hash text not null,
    response jsonb not null,
    created_at timestamptz not null default now()
);
