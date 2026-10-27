# Tapped-hole catalogue

The Hole dialog offers ISO metric coarse sizes, 15 additional fine diameter/pitch
pairs, a finite set of 14 UNC and 14 UNF sizes, six sizes each for internal
G/BSPP and Rp parallel pipe threads, and the ISO 7-1 Rc and ASME B1.20.1 NPT
internal taper threads (every size of their tables, `docs/threads.md`). This is a verified subset,
not complete coverage of these standards. Unified sizes range from #4 through
#12 and 1/4 through 1 inch. Include the family in a designation, for example
`1/4-20 UNC` or `#10-32 UNF`.

Internal units are millimetres. Unified screw diameters convert using exactly
25.4 mm/in; pitch is 25.4 divided by threads per inch. Numbered screw sizes are
nominal screw diameters, not numbered drill diameters. Pipe fractions are nominal
labels: G 1/4 has a basic major diameter of 13.157 mm, not 6.35 mm.

For UNC/UNF, the proposed pilot diameter comes from Guhring's **CUT TAPS,
approximately 75% theoretical thread** column. Fractional, letter and numbered
drills are converted to millimetres without rounding away the recommendation.
These are starting recommendations for cutting taps, not form taps. Material,
depth, tolerances, drilling accuracy and the tap manufacturer's process guidance
still determine the appropriate drill; the command's explicit `diameter` can
override the recommendation.

Metric holes retain the existing major-minus-pitch estimate, rounded to 0.001 mm.
This preserves existing command behavior, including M8's 6.75 mm default. It is
not presented as a tabulated 75% cut-tap recommendation. Existing files retain
their saved diameter expressions. Custom metric dimensions remain editable, and
Custom stays selected even when its current dimensions match a catalogue entry.

Tapped holes create the physical pilot bore and the thread, listed (cosmetic) or,
with `modeled`, cut into the wall as the Thread command cuts a modelled thread:
the entry is where it starts, and towards the bottom it stops square at the
thread depth, clear of a blind hole's bottom, or fades out (`ease_out`, ISO 3508).
A Unified thread is cut with the ISO 68-1 basic profile both share. External R,
thread classes and sealing behaviour are outside this catalogue.

## Taper Rc and NPT threads

An Rc or NPT hole is drilled with the largest stocked drill not larger than the
internal thread's minor diameter where the thread ends: the reference plane's
minor (d - 2h for ISO 7-1, E1 - h for NPT) at the face, less the 1 in 16 taper
over the thread's length (ISO 7-1's useful thread, NPT's effective thread L2), so
that the tap cuts full form over the whole length. Rc takes a metric drill of the
DIN 345 series (to 100 mm), NPT a letter or fractional inch drill (by 64ths to
1 1/2 in, ASME B94.11M); past the series the hole is bored to the exact value. The
Hole dialog shows the drill and, as its tooltip, the exact value: Rc 1/2 is
17.807 mm, drilled 17.75 mm; 1/2 NPT is 17.473 mm (0.6879 in), drilled 11/16 in
(17.4625 mm). This is derived from the standards, not a tap maker's
recommendation (which often allows for reaming); an explicit `diameter`
overrides it. Listed, the hole
is that cylindrical drill; modelled, the thread's taper is cut into it (the tap's
minor cone and the helical groove).

## Parallel G/BSPP and Rp threads

Both families use a 55-degree profile and a cylindrical pilot bore. Keep G and Rp
separate: their process-drill recommendations differ. The UI shows the nominal
pipe designation, actual basic major diameter, pitch/TPI and pilot drill. The
command accepts canonical labels such as `G 1/4` and `Rp 1/4`, compact forms
`G1/4` and `Rp1/4`, and `BSPP 1/4` as an alias for G. The Thread command cuts
them too (forms `bspp` and `rp`).

| Nominal | TPI | Major mm | Pitch diameter mm | Minor mm | G drill mm | Rp drill mm |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1/8 | 28 | 9.728 | 9.147 | 8.566 | 8.75 | 8.5 |
| 1/4 | 19 | 13.157 | 12.301 | 11.445 | 11.75 | 11.4 |
| 3/8 | 19 | 16.662 | 15.806 | 14.950 | 15.25 | 14.8 |
| 1/2 | 14 | 20.955 | 19.793 | 18.631 | 19.0 | 18.5 |
| 3/4 | 14 | 26.441 | 25.279 | 24.117 | 24.5 | 24.0 |
| 1 | 11 | 33.249 | 31.770 | 30.291 | 30.75 | 30.2 |

Pitch is exactly 25.4/TPI. These basic thread dimensions are separate from process
bores. The catalogue uses Yamawa's drill column as its default pilot. Its separate
recommended process-bore references, for the six rows above, are G: 8.78, 11.78,
15.28, 19.04, 24.52, 30.77 mm and Rp: 8.55, 11.5, 14.9, 18.55, 24.1, 30.25 mm.
A saved explicit diameter remains authoritative. Listed, the geometry is the
cylindrical pilot plus the designation; modelled, the helical groove is cut too.

## Primary sources

- DIN 345:2006 (taper shank twist drills), the metric drill diameters to 100 mm as
  reproduced at <https://www.fasten.it/en/Norms/Norm/DIN%20345>: the stocked drills
  of Rc holes. Three digits that reproduction drops (15.25, 34.5, 54) are restored
  from their neighbours.
- ASME B94.11M (twist drills): the letter drill sizes A to Z and fractional sizes
  by 64ths of an inch, the stocked drills of NPT holes.

- [Yamawa, YEU23 European catalogue](https://www.yamawa.eu/files/Yamawa_European_Catalogue_YEU23.pdf),
  printed page 668: basic G/Rp dimensions; page 707: separate G and Rp drill and
  recommended bore columns. These supply the finite parallel-pipe entries above.

- [Guhring, Tap Drill Sizes](https://guhring.com/media/catalogs/lgbkp3ieki4.pdf),
  PDF page 2: UNC/UNF pitch and practical CUT TAPS approximately75% drills;
  page 1: decimal equivalents for named drills. The separate FORM TAPS columns
  are not used. Also verifies 14 of the additional metric fine pairs.
- [PEM, Thread Code Comparison Chart](https://www.pemnet.com/resources/download-center/thread-code-comparison-chart/):
  nominal diameters of #4, #6, #8, #10 and #12 screws.
- [Yamawa, European catalogue, printed page159](https://www.yamawa.eu/wp-content/uploads/Catalogo_sfogliabile_YEU21/files/basic-html/page161.html):
  M12x1.25 diameter/pitch pair. Its product-specific pilot recommendation is not
  used for the retained metric estimate.
- [OSG, Tap Drill Sizes and Pitch Diameter Limits, Vol3 (2024)](https://osgtool.com/800274ca-v3/):
  independent cross-check of UNC/UNF pitches. OSG's calculated percentages and
  Guhring's practical named drills are not interchangeable columns.

## Trial

On a small plate, open Hole, choose Tapped, then switch Profile between ISO metric,
UNC, UNF, G / BSPP, Rp, Rc and NPT. Try M8, M8x1, 1/4-20 UNC, #10-32 UNF, G 1/4,
Rp 1/4, Rc 1/4 and 1/4-18 NPT, listed and Modeled, with the End Square or Fade. Check the displayed drill,
create the hole, save, reopen and check the designation and bore. In ISO metric,
choose Custom, edit the nominal diameter and pitch, and verify that Custom stays
selected and an invalid draft cannot apply an earlier valid value.
