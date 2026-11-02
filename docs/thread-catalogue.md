# Tapped-hole catalogue

The Hole dialog offers ISO metric coarse sizes, 15 additional fine diameter/pitch
pairs, a finite set of 14 UNC and 14 UNF sizes, and six sizes each for internal
G/BSPP and Rp parallel pipe threads. This is a verified subset,
not complete coverage of these standards. Unified sizes range from #4 through
#12 and 1/4 through 1 inch. Include the family in a designation, for example
`1/4-20 UNC` or `#10-32 UNF`.

Internal units are millimetres. Unified screw diameters convert using exactly
25.4 mm/in; pitch is 25.4 divided by threads per inch. Numbered screw sizes are
nominal screw diameters, not numbered drill diameters. Pipe fractions are nominal
labels: G 1/4 has a basic major diameter of 13.157 mm, not 6.35 mm.

For UNC/UNF, the proposed pilot diameter comes from Guhring's **CUT TAPS,
approximately 75% theoretical thread** column. Fractional, letter and numbered
drills are converted to millimetres without rounding away the recommendation. The
Hole dialog shows a Unified size's drill in inches, for example `0.201 in` for
1/4-20 UNC, and the millimetre bore when the pointer rests on it.
These are starting recommendations for cutting taps, not form taps. Material,
depth, tolerances, drilling accuracy and the tap manufacturer's process guidance
still determine the appropriate drill; the command's explicit `diameter` can
override the recommendation.

Metric holes retain the existing major-minus-pitch estimate, rounded to 0.001 mm.
This preserves existing command behavior, including M8's 6.75 mm default. It is
not presented as a tabulated 75% cut-tap recommendation. Existing files retain
their saved diameter expressions. Custom metric dimensions remain editable, and
Custom stays selected even when its current dimensions match a catalogue entry.

Tapped holes create the physical pilot bore and cosmetic thread metadata. The
separate Thread command and its automatic sizing remain ISO metric only; this
catalogue does not enable modelled Unified or pipe threads. Rc, external R, NPT,
thread classes, handedness and sealing behavior are outside this tranche.
No tapered thread is represented as a cylindrical hole.

## Parallel G/BSPP and Rp threads

Both families use a 55-degree profile and a cylindrical pilot bore. Keep G and Rp
separate: their process-drill recommendations differ. The UI shows the nominal
pipe designation, actual basic major diameter, pitch/TPI and pilot drill. The
command accepts canonical labels such as `G 1/4` and `Rp 1/4`, compact forms
`G1/4` and `Rp1/4`, and `BSPP 1/4` as an alias for G. The separate Thread command
remains metric-only.

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
A saved explicit diameter remains authoritative. The geometry stays a physical
cylindrical pilot plus cosmetic designation, without a helical cut.

## Primary sources

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
UNC, UNF, G / BSPP and Rp. Try M8, M8x1, 1/4-20 UNC, #10-32 UNF, G 1/4
and Rp 1/4. Check the displayed drill,
create the hole, save, reopen and check the designation and bore. In ISO metric,
choose Custom, edit the nominal diameter and pitch, and verify that Custom stays
selected and an invalid draft cannot apply an earlier valid value.
