# Wi-Fi coverage

Wi-Fi coverage predicts the strength of a Wi-Fi signal across a floor plan, using the walls in the
design. It shows the result as a heatmap in the 3D view and reports how much of the floor gets a
usable signal. Use it to decide where a router or access point should go before you mount it
(#20161). The command is `inspect.rf_coverage`, and the ribbon button is **Wi-Fi Coverage** on the
INSPECT tab.

It changes nothing in the design and is not an undo step.

## How the signal is worked out

The model is a multi-wall indoor model:

    RSSI = EIRP − FSPL(1 m, f) − 10 · n · log10(d) − Σ wall losses

- **EIRP** is what the emitter sends, in dBm. By default it is capped at indoor limits: 30 dBm at
  2.4 and 5 GHz, and 27 dBm at 6 GHz (low-power indoor).
- **FSPL(1 m, f) = 20 · log10(f in MHz) − 27.55** is the free-space loss over the first metre. The
  frequencies are 2437, 5500 and 6525 MHz.
- **n** is the path-loss exponent. The default 2.2 allows for furniture; 2 is free space.
- **d** is the 3D distance from the emitter to the receiver, in metres. It counts the difference
  in height, and is at least 0.3 m.
- **The wall losses**: each wall the straight path crosses adds its material's loss once. The path
  is seen from above, so a wall counts the same at every height.

With several emitters, each point takes the strongest signal.

## Where the walls come from

The command cuts the bodies horizontally at the **slice height** (default 1 m). Each piece the
cut makes is a wall region. The cut is exact:

- A body that is proved to be a prism along Z uses its section.
- Any other solid uses the same half-space cut as a section view, and reads its cut faces.
- A mesh body (an imported STL, OBJ or 3MF) is cut by its own triangles.

Curved outlines are followed to within 1 mm. If the kernel refuses a cut at the exact height, the
body is cut again up to 1.3 mm higher or lower, which gives the same walls.

Which bodies count:

- By default, every body shown in the view.
- `bodies` or `components` keeps only those. With `rf_materials: true`, bodies whose material has
  an RF loss count as well.
- `exclude` leaves bodies or components out, such as furniture and plants.
- A hidden body counts only when its material has an RF loss, or with `include_hidden: true`. This
  lets hidden window glass still count.

## RF losses on materials

A design material can store a loss in dB for each crossing at 2.4, 5 and 6 GHz. Set it with `rf` on
`material.create` or `material.edit`, as a preset name or as `{"2.4": dB, "5": dB, "6": dB}`. The
value `null` clears it. `material.inspect` shows it. The presets are typical published averages:

| Preset | 2.4 GHz | 5 GHz | 6 GHz |
|---|---|---|---|
| `concrete` | 12 | 20 | 24 |
| `drywall` (steel stud) | 3 | 4 | 5 |
| `interior-glass` | 2 | 3 | 3 |
| `low-e-glass` | 10 | 15 | 18 |
| `metal` | 15 | 20 | 22 |
| `none` | 0 | 0 | 0 |
| `wood-door` | 3 | 4 | 5 |

Some built-in materials have losses too:
- The metals (Steel, Stainless Steel, Aluminum, Brass, Copper, Titanium, Cast Iron) count as `metal`.
- Oak counts as `wood-door`.
- Glass counts as `interior-glass`.

**A wall without an RF loss is guessed from its thickness** where the path crosses it. A wall
thinner than 140 mm is drywall, and a thicker one is concrete. The thickness is twice the distance
from the middle of the crossing to the wall's nearest edge, which is the wall's width for a
straight wall at any angle. One body may hold all the walls of a floor, thin and thick, and each
crossing is guessed on its own.

Counting crossings:
- Boxes of one material that meet along a path count as one wall.
- Where two materials overlap, such as a door inside a wall's opening, the larger loss counts.
- An emitter inside a wall isn't charged for leaving it.

## The floor

The statistics count the floor, not the walls. By default the floor is the part of the plan the
walls enclose. Openings narrower than `close_gaps` (default 1 m) count as closed, so an open
doorway doesn't let the outside in. When the walls enclose nothing, the floor is every cell
outside the walls, and the reply's `region` says `open`. `region: {polygon: [[x, y], …]}` or
`region: {body: "Floor Slab"}` gives the floor instead. A body's outline is read from a cut halfway
up the body.

The grid's cells are `grid_step` wide (default 75 mm). Each cell takes the signal at its centre.

## The reply

The reply is kept small for agents:
- `stats` for each band: the share of the floor at −60 dBm or better (`pct_at_least_60`, fast) and
  at −67 dBm or better (`pct_at_least_67`, video calls), and the 5th percentile, median, weakest and
  strongest signal.
- `points`: the signal at each named point, the emitter that gives it, and the wall loss on the way.
- `grid`: the grid's size and the floor's area.
- `walls`: how many bodies and regions the cut found, and the count for each material.

`grid: true` adds every cell's value. `png: "path.png"` writes the heatmap seen from above, with
north up, each wall's outline in its material's colour, and the emitters marked.

A call with no emitters returns the device library (`devices`). The library lists a few Wi-Fi 7
routers and access points. Their EIRP is the transmit power plus the antenna gain from the makers'
public spec sheets. An emitter can give `eirp_dbm` instead: one number, or one for each band.

## In the 3D view

- **Wi-Fi Coverage** on the ribbon turns the heatmap on with one router in the middle of the model,
  or turns it off.
- The heatmap lies on the slice plane. While it is on and no Section Analysis is, the view is cut
  just above the slice, so ceilings don't hide it.
- Drag a router's marker to move it. While it moves, the map is worked out on a coarser grid. When
  you release it, the map is worked out on the full grid. The cut walls are kept between calls, so
  only the paths are traced again.
- The card at the bottom right sets the device, the band, the router's height and the slice height.
  It also chooses which component makes the walls (bodies with an RF material always count), adds a
  router, and clears the map. It shows the coverage figures and the colour key.

## Example

```json
{"command": "inspect.rf_coverage", "params": {
  "emitters": [{"name": "Hall", "at": [4064, 4064], "height": 2550, "device": "u7-pro"}],
  "components": ["Walls"], "rf_materials": true,
  "points": [{"name": "Bedroom", "at": [1650, 2290]}],
  "png": "coverage.png"}}
```

## Limits

- The losses are typical averages. Real walls vary by several dB, and so do people, furniture and
  neighbours' networks. Treat the map as a way to rank spots and devices, not as a guarantee.
- The antennas radiate the same in every direction, and there are no reflections.
- Only the downlink is predicted. Phones send 6 to 10 dB weaker, so far rooms lose the uplink first.
- There is one slice: floors and ceilings between storeys don't count.
- The 3D view's screenshot through MCP (`screenshot`) doesn't draw the heatmap yet. Use `png`.
