// Chamfer holes by size
// @param min_dia Minimum diameter length | 5 mm
// @param max_dia Maximum diameter length | 20 mm
// @param chamfer Chamfer distance length | 1 mm
let holes = run("inspect.find_holes", #{ min_diameter: min_dia, max_diameter: max_dia });
run("solid.chamfer", #{ edges: holes.edges, distance: chamfer });
