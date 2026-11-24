// Every moved entry of `ROWS` with its class against the certified enclosure.
static EXPECTED: [(&str, usize, Side); 21] = [
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", 0, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", 3, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", 4, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", 9, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", 10, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", 11, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 0, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 3, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 10, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 11, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", 0, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", 3, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", 4, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", 9, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", 10, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", 11, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", 0, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", 3, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", 10, Side::Equal),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", 11, Side::Equal),
    ("source-file-corner_box-edge9", 0, Side::Equal),
];

// The entries whose enclosure is too wide for EQUAL to bound the new value's error
// within the rows' accuracy.
static WIDE: [(&str, usize); 0] = [];
