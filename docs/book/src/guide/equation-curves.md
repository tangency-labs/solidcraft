# Equation curves

An equation curve is a curve you **describe with a rule** instead of drawing it with clicks.
You type the rule, and SolidCraft draws the curve that follows it, exactly, every time.

It is the way to get shapes that are hard to draw by hand: a smooth wave, a parabola, a spiral,
or the curved side of a gear tooth.

## Your first curve: a straight ramp

1. Start a sketch: **SOLID › CREATE › Create Sketch**, then click a plane.
2. Click **SKETCH › CREATE › Equation Curve**. A window opens with a wave already filled in.
3. Set **Form** to **Explicit**. Now there is one rule to fill in, **y(x)**: *how high is the curve
   at each x?*
4. Type `x / 2` in **y(x)**, `0` in **Start** and `40` in **End**.
5. Click **OK**.

![The curve y = x / 2 from x = 0 to x = 40: a straight ramp](../generated/equation-line.png)

The rule says: *the height is half of how far along you are*. At x = 10 the curve is 5 high, at
x = 40 it is 20 high. **Start** and **End** say where along x the curve begins and stops.

## Explicit: one rule for the height

With **Explicit**, you write the height `y` using `x`. Try these (same steps, other rules):

| Rule in **y(x)** | Start | End | What you get |
|---|---|---|---|
| `x / 2` | `0` | `40` | a straight ramp (above) |
| `x * x / 20` | `-30` | `30` | a parabola, the shape of a thrown ball's path |
| `8 * sin(x / 5)` | `0` | `20 * pi` | a smooth wave |

![The parabola y = x * x / 20 from x = -30 to x = 30](../generated/equation-parabola.png)

![The wave y = 8 * sin(x / 5): two smooth waves along x](../generated/equation-wave.png)

In the wave, `8` is how tall each bump is and `5` stretches it sideways: try `x / 10` and the
waves get twice as long.

## Parametric: a rule for x and a rule for y

Some curves go back on themselves, like a circle: for one x there are two heights. For those,
set **Form** to **Parametric** and give **two** rules, **x(t)** and **y(t)**.

Think of `t` as **a clock**. It starts at **Start** and ticks up to **End**. At every moment the
two rules say where the pen is: across (`x`) and up (`y`). The curve is the path the pen draws
while the clock runs.

| Rule in **x(t)** | Rule in **y(t)** | Start | End | What you get |
|---|---|---|---|---|
| `20 * cos(t)` | `20 * sin(t)` | `0` | `2 * pi` | a circle of radius 20 |
| `2 * t * cos(t)` | `2 * t * sin(t)` | `0` | `4 * pi` | a spiral, two turns |

![A circle of radius 20 drawn from x = 20 cos(t), y = 20 sin(t)](../generated/equation-circle.png)

For the circle, `cos` and `sin` walk the pen around, and `20` is how far from the middle it
stays. `2 * pi` is one full turn, so the circle closes. In the spiral, the distance from the
middle (`2 * t`) grows while the clock runs, so the pen moves outwards as it goes round.

![A spiral of two turns drawn from x = 2 t cos(t), y = 2 t sin(t)](../generated/equation-spiral.png)

## Angles are in radians

`sin`, `cos` and `tan` count angles in **radians**, not degrees: a full turn is `2 * pi`
(about 6.28), half a turn is `pi`. You can also write an angle with its unit for **Start** and
**End**, such as `360 deg`.

A number without a unit is in your design's unit (millimetres unless you changed it).

## Other functions

Besides `sin`, `cos` and `tan`, a rule can use `sqrt`, `exp`, `ln`, `pow`, `abs`, `min`, `max`
and the hyperbolic functions `sinh`, `cosh` and `tanh` (and their inverses `asinh`, `acosh`,
`atanh`), which take a plain number. A hanging chain is `20 * cosh(x / 20)` in **y(x)**, and the
flare of a horn is often written with `cosh` and `sinh`.

## Using parameters: a gear tooth

Rules can use the design's [parameters](parameters.md). Here a parameter `r` (20 mm) is the size
of a base circle, and the curve is the *involute* of that circle: the path the end of a string
makes as you unwind it from the circle. The sides of most gear teeth have this shape.

| Rule in **x(t)** | Rule in **y(t)** | Start | End |
|---|---|---|---|
| `r * (cos(t) + t * sin(t))` | `r * (sin(t) - t * cos(t))` | `0` | `1.5` |

![The involute of a 20 mm base circle, starting on the circle and curving away from it](../generated/equation-involute.png)

Change `r` in the Parameters dialog and the curve redraws itself to match: the rule is kept, not
just the shape.

## From curve to part

An equation curve is an ordinary sketch curve, so you can build with it:

- A curve that **ends where it starts** closes by itself, smoothly. Extrude it and you get a
  solid. This flower uses a radius that goes in and out six times as the clock goes round once:

  | Rule in **x(t)** | Rule in **y(t)** | Start | End |
  |---|---|---|---|
  | `(20 + 4 * cos(6 * t)) * cos(t)` | `(20 + 4 * cos(6 * t)) * sin(t)` | `0` | `2 * pi` |

  ![The flower curve extruded 6 mm into a solid](../generated/equation-flower.png)

- An **open** curve can be closed with lines: draw lines from its two ends, then extrude the
  shape. For example, close a wave with three lines down to the x axis to get a wavy strip.
- It can be the **path** of a sweep.

## Changing a curve

Right-click the curve and choose **Edit Equation Curve**. The same window opens with the rules
in it; change them and click **OK**.

## When something goes wrong

- **The curve is tiny or huge.** Check the numbers in the rule and the Start/End range: `sin`
  never goes past 1, so `sin(x)` alone is at most 1 mm high. Multiply it (`8 * sin(x)`).
- **The wave has too few bumps, or far too many.** Remember radians: `sin(x)` repeats every
  `2 * pi` (6.28) along x, not every 360.
- **The circle has a gap.** End must be a whole turn: `2 * pi`, or `360 deg`.
- **A rule is rejected.** Use `*` for multiply (`2 * t`, not `2t`), and close every bracket.

## For scripts and agents

The commands are `sketch.equation_curve` and `sketch.equation_curve.edit` (see the
[command reference](../reference/commands.md)). The curve is a fit point spline through points
worked out from the rules, as many as keep it within the **Tolerance** (0.01 mm unless you set
another), up to 500. Its points are fixed, so other curves can start at its ends. When a parameter
a rule uses changes, the points follow. The pictures on this page come from the scripts in
`docs/book/scripts/equation-*.json`.
