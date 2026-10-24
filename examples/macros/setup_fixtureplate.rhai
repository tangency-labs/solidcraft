// C-shaped clearance hole
run("sketch.create", #{ plane: "XY", name: "clearance_hole" });
run("sketch.line", #{
    points: [[-30, -50], [30, -50], [30, -25], [0, -25], [0, 25], [30, 25], [30, 50], [-30, 50]],
    closed: true
});
run("sketch.finish");
