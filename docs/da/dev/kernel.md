# The Kernel Boundary and Truck (kernel-grænsefladen og truck)

`crates/kernel` er SolidCrafts **B-rep-kernel**. Den indkapsler [truck](https://github.com/ricosjp/truck) (Apache-2.0), som håndterer topologi, NURBS-geometri, booleske operationer, tessellering og parsing af STEP. Det er den eneste crate, der må bruge truck direkte. Kaldende kode får SolidCrafts egne typer (`Body`, flade- og kantindekser, mesh og målinger) samt `KernelError` i stedet for truck-typer og panic.

## `guard`

truck kan udløse panic ved geometri, det ikke understøtter. Alle kald til truck går derfor gennem `solidcraft_kernel::guard`, som opfanger fejlen og omdanner den til `KernelError::Internal`, så modellen forbliver uændret:

```rust
pub fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T>
```

En fejlet Feature vises med en fejl på tidslinjen i stedet for at lukke programmet.

## Hvad SolidCraft selv implementerer

truck har ikke Fillet, Chamfer, Shell eller redigering af flader. Derfor implementerer SolidCraft selv afrundinger med konstant radius, chord- og variabel radius og hjørneovergange, Chamfer, Shell, Offset, Delete Face, Split Face, Replace Face, sammensyning af flader, læsning og skrivning af STEP og IGES ud over trucks parser samt oprindelsesdata for flader (hvilken inputflade en outputflade stammer fra; bruges til [Persistent Naming](naming.md)).

## Indbyggede kopier af truck-crates

Rettede kopier af `truck-shapeops`, `truck-stepio` og `truck-meshalgo` findes i `vendor/` og forbindes via `[patch.crates-io]`. Rettelserne løser robusthedsproblemer fundet i tests og STEP-testfiler. Hver rettelse bør sendes upstream, når det er muligt. `vendor/` er undtaget fra projektets fmt- og clippy-regler.

To ens cylindre, der krydser hinanden (`tangent_cylinders.rs`), berører hinanden i to punkter, hvor trucks mesh-baserede snitberegning ikke kan følge skæringen. Hvis den booleske operation fejler, deler SolidCrafts kernel cylinderfladerne langs linjen mellem berøringspunkterne og udfører den booleske operation på de opdelte emner. En shapeops-rettelse, som vælger det korrekte fladehjørne ved et punkt, hvor flere kurver ender, sørger for, at de krydsende armer forbindes.

Nogle af trucks assertions køres kun i Debug-builds og kan fejle ved gyldig geometri. Derfor kører tests og CI med `--release`.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| Kernel Boundary | Kernel-grænseflade |
| B-rep Kernel | Kerne til grænsefladebaseret CAD-geometri |
| Topology | Topologi |
| NURBS | Ikke-uniform rationel B-spline |
| Boolean Operations | Booleske operationer |
| Tessellation | Opdeling i trekantmesh |
| STEP Parsing | Læsning og fortolkning af STEP |
| KernelError | Fejl fra geometrikernen |
| Guard | Beskyttende fejlhåndtering |
| Panic | Uventet programfejl |
| Fillet | Afrunding |
| Chamfer | Affasning |
| Shell | Udhuling |
| Offset | Forskydning |
| Face Provenance | Oprindelse for en flade |
| Persistent Naming | Vedvarende navngivning |
| Vendored | Medtaget lokalt i repositoryet |
| Upstream | Det oprindelige projekt |
| Assertions | Kontrolbetingelser |
| Release Build | Optimeret build |
