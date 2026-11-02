// Fixture plate with patterned clearance
// @param nx Count in X int | 1
// @param ny Count in Y int | 1
// @param spacing_x Cavity spacing in X length | 80
// @param spacing_y Cavity spacing in Y length | 120
let cx = (nx - 1) * spacing_x / 2.0;
let cy = (ny - 1) * spacing_y / 2.0;
run("solid.box", #{ length: 500, width: 350, height: 8, center: [cx, cy, 4], name: "Tray" });
let left = cx - 237.5;
let right = cx + 237.5;
let bottom = cy - 162.5;
let top = cy + 162.5;
run("solid.hole", #{ position: [left, bottom, 8], diameter: "8 mm", name: "HoleBL" });
run("solid.hole", #{ position: [cx, bottom, 8], diameter: "8 mm", name: "HoleBC" });
run("solid.hole", #{ position: [right, bottom, 8], diameter: "8 mm", name: "HoleBR" });
run("solid.hole", #{ position: [left, top, 8], diameter: "8 mm", name: "HoleTL" });
run("solid.hole", #{ position: [cx, top, 8], diameter: "8 mm", name: "HoleTC" });
run("solid.hole", #{ position: [right, top, 8], diameter: "8 mm", name: "HoleTR" });
let holes = run("inspect.find_holes", #{ "type": "Hole" });
run("solid.chamfer", #{ edges: holes.edges, distance: "1 mm" });
run("solid.extrude", #{ sketch: "clearance_hole", operation: "cut", through_all: true, name: "Clearance" });
if nx > 1 && ny > 1 {
    run("solid.pattern.rectangular", #{
        features: ["Clearance"],
        dir1: [1, 0, 0], count1: nx, spacing1: spacing_x,
        dir2: [0, 1, 0], count2: ny, spacing2: spacing_y
    });
} else if nx > 1 {
    run("solid.pattern.rectangular", #{ features: ["Clearance"], dir1: [1, 0, 0], count1: nx, spacing1: spacing_x });
} else if ny > 1 {
    run("solid.pattern.rectangular", #{ features: ["Clearance"], dir1: [0, 1, 0], count1: ny, spacing1: spacing_y });
}
