# Press-in hardware catalogue

Sheet metal hardware that is pressed into a punched hole (self-clinching nuts, flush nuts,
studs and standoffs) needs a sheet of a minimum thickness, a mounting hole of a set size, and
a minimum distance from the hole's centre to an edge. SolidCraft keeps a finite table of ISO
metric coarse sizes (`crates/doc/src/hardware_catalog.rs`); this page says where its numbers come
from. Internal units are millimetres. The table is SolidCraft's own: its layout, kind names and
part ids are ours, and it names no maker's products or part numbers.

## Kinds and sizes

| Kind | Sizes |
|---|---|
| `nut` | M2.5, M3, M4, M5, M6, M8 |
| `flush_nut` | M2.5, M3, M4, M5, M6 |
| `stud` | M3, M4, M5, M6, M8 |
| `standoff` | M3, M4, M5 (blind or through, same numbers) |

A size is in the table only when two makers print it. Left out: inch sizes, fine pitches, M2 and
M10 and up, flush nuts above M6, standoffs of M2.5 and M6 or more, and floating, broaching,
thin-sheet and other special types.

| Kind | Thread | Hole (+0.08/-0.00) | Thinnest sheet | Centre to edge |
|---|---|---|---|---|
| nut | M2.5 | 4.25 | 0.8 | 4.8 |
| nut | M3 | 4.25 | 0.8 | 4.8 |
| nut | M4 | 5.4 | 0.8 | 6.9 |
| nut | M5 | 6.4 | 0.8 | 7.1 |
| nut | M6 | 8.75 | 1.4 | 8.6 |
| nut | M8 | 10.5 | 1.4 | 9.7 |
| flush_nut | M2.5 | 4.4 | 1.5 | 6.0 |
| flush_nut | M3 | 4.4 | 1.5 | 6.0 |
| flush_nut | M4 | 7.4 | 1.5 | 7.2 |
| flush_nut | M5 | 7.92 | 1.5 | 8.8 |
| flush_nut | M6 | 8.75 | 3.2 | 8.8 |
| stud | M3 | 3.0 | 1.0 | 5.6 |
| stud | M4 | 4.0 | 1.0 | 7.2 |
| stud | M5 | 5.0 | 1.0 | 7.2 |
| stud | M6 | 6.0 | 1.6 | 7.9 |
| stud | M8 | 8.0 | 2.4 | 9.6 |
| standoff | M3 | 4.2 | 1.0 | 6.0 |
| standoff | M4 | 7.2 | 1.3 | 8.0 |
| standoff | M5 | 7.2 | 1.3 | 8.0 |

## Sources

Every row comes from two makers' printed tables, read side by side:

- **PSM International**, catalogue version 0810 (the PDFs at rgafasteners-psm.com): self-clinching
  nuts, printed p.2-13 ("P-S & P-CLS types (metric)"); blind-hole standoffs, printed p.2-42
  ("P-BSO blind hole type (metric)").
- **PSM International**, product guide pages on psmfasteners.com: "P-F type (metric)" for flush
  nuts and "P-FH & P-FHS types (metric)" for studs ("General dimensions").
- **TR Fastenings**, knowledge base, "Hole sizes - metric (mm)" tables of the nut, flush nut, stud
  and blind standoff installation pages (self-clinch fasteners).

Each table gives, per thread, the minimum sheet thickness, the hole in the sheet (+0.08/-0.00 mm)
and the minimum distance from the hole's centre line to the sheet edge. Neither maker's table
has a separate distance for a bend.

Where a maker's table has several shank (length) codes for one size, the table takes the thinnest
sheet that both makers print for that size: the nuts' M2.5 to M5 are 0.8 mm (code 0) in both; M6
and M8 are 1.4 mm, the least TR prints (PSM also prints a 0.92 mm shank for M6, which only one
maker has).

## Where the makers differ

The table takes the stricter value, the larger distance or thickness. Holes are a band (+0.08
mm over the size), so the table takes a size inside both makers' bands.

| Row | Difference | Taken |
|---|---|---|
| nut M6 thinnest sheet | PSM 0.92 (shank 00), TR 1.4 | 1.4 |
| flush nut M5 centre to edge | PSM 8.0, TR 8.8 | 8.8 |
| flush nut M6 thinnest sheet | PSM 3.1, TR 3.2 | 3.2 |
| flush nut holes | PSM 4.37 / 7.37 / 7.92 / 8.74, TR 4.4 / 7.4 / 7.9 / 8.75 | 4.4 / 7.4 / 7.92 / 8.75 (in both bands) |
| stud M6 thinnest sheet | PSM 1.5, TR 1.6 | 1.6 |
| stud M8 centre to edge | PSM 9.6, TR 9.0 | 9.6 |
| standoff holes (M4, M5) | PSM 7.15, TR 7.2 | 7.2 (in both bands) |
| standoff thinnest sheet | PSM ">1.0" and ">1.3", TR 1.0 and 1.3 | 1.0 and 1.3 (a sheet of exactly that thickness is accepted) |

PSM's stud table prints its hole tolerance column as "-0.08 -0.00"; its design guide and the TR
table both say +0.08/-0.00, which the table uses.

## Near a bend

The makers' tables give a distance to an edge only. Handbooks of self-clinching fastener makers
say to use the same centre-to-edge value near a bend, measured to where the bend starts (the
outer end of the flat sheet), not to the bend line, for example NP Fasteners'
"Self Clinching Fasteners - Do's and Don'ts". SolidCraft does the same: the check measures from
the hole's centre to the edge of the flat panel the hole is in, whether that edge is a free edge
or a bend. The sheet metal design rules' hole-to-bend distance, which depends on the press
brake's die opening, is a different check for plain holes and is not part of this table.

## What the table is not

A size's numbers are starting points for a design check, not a substitute for the maker's
datasheet of the part you buy. Materials, sheet hardness and the part's variant change them.
