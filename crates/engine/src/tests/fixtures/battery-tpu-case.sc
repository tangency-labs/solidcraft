{
  "format": "solidcraft/18",
  "name": "Battery TPU Case",
  "units": "mm",
  "params": [
    {
      "name": "batt_w",
      "expr": "290 mm",
      "unit": "mm",
      "comment": "Battery flat top/bottom width",
      "model": false
    },
    {
      "name": "batt_bulge",
      "expr": "10 mm",
      "unit": "mm",
      "comment": "Side bulge past the top/bottom corners",
      "model": false
    },
    {
      "name": "batt_h",
      "expr": "92 mm",
      "unit": "mm",
      "comment": "Battery height",
      "model": false
    },
    {
      "name": "peak_left",
      "expr": "27 mm",
      "unit": "mm",
      "comment": "Height of the left side's widest point",
      "model": false
    },
    {
      "name": "peak_right",
      "expr": "batt_h - peak_left",
      "unit": "mm",
      "comment": "Height of the right side's widest point",
      "model": false
    },
    {
      "name": "batt_len",
      "expr": "198 mm",
      "unit": "mm",
      "comment": "Battery length",
      "model": false
    },
    {
      "name": "tab_w",
      "expr": "15 mm",
      "unit": "mm",
      "comment": "Connector tab width (from the left top corner)",
      "model": false
    },
    {
      "name": "tab_depth",
      "expr": "5.5 mm",
      "unit": "mm",
      "comment": "Connector tab protrusion",
      "model": false
    },
    {
      "name": "ramp_len",
      "expr": "35 mm",
      "unit": "mm",
      "comment": "Ramp from tab back to the wall",
      "model": false
    },
    {
      "name": "ramp_blend_wall",
      "expr": "80 mm",
      "unit": "mm",
      "comment": "Blend radius ramp to wall",
      "model": false
    },
    {
      "name": "ramp_blend_tab",
      "expr": "40 mm",
      "unit": "mm",
      "comment": "Blend radius tab to ramp",
      "model": false
    },
    {
      "name": "wall",
      "expr": "3 mm",
      "unit": "mm",
      "comment": "TPU wall thickness",
      "model": false
    },
    {
      "name": "r_big",
      "expr": "5 mm",
      "unit": "mm",
      "comment": "Fillet on the 4 long profile edges (inside and outside)",
      "model": false
    },
    {
      "name": "r_small",
      "expr": "1 mm",
      "unit": "mm",
      "comment": "Fillet on all other edges (inside and outside)",
      "model": false
    },
    {
      "name": "pleat_depth",
      "expr": "10 mm",
      "unit": "mm",
      "comment": "Accordion pleat depth at the open edge",
      "model": false
    },
    {
      "name": "pleat_pitch",
      "expr": "16 mm",
      "unit": "mm",
      "comment": "Accordion fold pitch",
      "model": false
    },
    {
      "name": "clear",
      "expr": "1 mm",
      "unit": "mm",
      "comment": "Overshoot so trimming tools never sit on a face",
      "model": false
    },
    {
      "name": "d1",
      "expr": "batt_w",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d2",
      "expr": "batt_w",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d3",
      "expr": "batt_h",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d4",
      "expr": "batt_bulge",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d5",
      "expr": "peak_right",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d6",
      "expr": "batt_bulge",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d7",
      "expr": "peak_left",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d10",
      "expr": "batt_w + 2 * batt_bulge + 10 * clear",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d11",
      "expr": "clear",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d12",
      "expr": "batt_len",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d13",
      "expr": "batt_len + tab_depth",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d14",
      "expr": "batt_w / 2 - tab_w",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d15",
      "expr": "ramp_len",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d16",
      "expr": "ramp_blend_wall",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d17",
      "expr": "ramp_blend_tab",
      "unit": "mm",
      "model": true
    }
  ],
  "features": [
    {
      "id": 1,
      "name": "BatteryProfile",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "custom",
        "plane": {
          "origin": [
            0.0,
            0.0,
            0.0
          ],
          "x_dir": [
            1.0,
            0.0,
            0.0
          ],
          "y_dir": [
            -0.0,
            0.0,
            1.0
          ]
        }
      },
      "sketch": {
        "points": [
          {
            "id": "origin",
            "pos": [
              0.0,
              0.0
            ],
            "fixed": true
          },
          {
            "id": "l1.start",
            "pos": [
              -145.0,
              92.0
            ],
            "fixed": false
          },
          {
            "id": "l1.end",
            "pos": [
              145.0,
              92.0
            ],
            "fixed": false
          },
          {
            "id": "l2.end",
            "pos": [
              155.0,
              65.0
            ],
            "fixed": false
          },
          {
            "id": "l3.end",
            "pos": [
              145.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l4.end",
            "pos": [
              -145.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l5.end",
            "pos": [
              -155.0,
              27.0
            ],
            "fixed": false
          },
          {
            "id": "p1",
            "pos": [
              -100.0,
              0.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              100.0,
              0.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              0.0,
              -100.0
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p4",
            "pos": [
              0.0,
              100.0
            ],
            "fixed": true,
            "link": "j2"
          }
        ],
        "curves": [
          {
            "id": "l1",
            "type": "line",
            "a": 1,
            "b": 2,
            "construction": false
          },
          {
            "id": "l2",
            "type": "line",
            "a": 2,
            "b": 3,
            "construction": false
          },
          {
            "id": "l3",
            "type": "line",
            "a": 3,
            "b": 4,
            "construction": false
          },
          {
            "id": "l4",
            "type": "line",
            "a": 4,
            "b": 5,
            "construction": false
          },
          {
            "id": "l5",
            "type": "line",
            "a": 5,
            "b": 6,
            "construction": false
          },
          {
            "id": "l6",
            "type": "line",
            "a": 6,
            "b": 1,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 7,
            "b": 8,
            "construction": true,
            "link": "j1"
          },
          {
            "id": "l8",
            "type": "line",
            "a": 9,
            "b": 10,
            "construction": true,
            "link": "j2"
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "horizontal",
            "l": 0
          },
          {
            "id": "k2",
            "type": "horizontal",
            "l": 3
          },
          {
            "id": "k3",
            "type": "collinear",
            "a": 3,
            "b": 6
          },
          {
            "id": "k4",
            "type": "symmetric",
            "p": 1,
            "q": 2,
            "l": 7
          },
          {
            "id": "k5",
            "type": "symmetric",
            "p": 4,
            "q": 5,
            "l": 7
          },
          {
            "id": "k6",
            "type": "length",
            "l": 0,
            "value": 290.0,
            "param": "d1"
          },
          {
            "id": "k7",
            "type": "length",
            "l": 3,
            "value": 290.0,
            "param": "d2"
          },
          {
            "id": "k8",
            "type": "distance_y",
            "p": 1,
            "q": 5,
            "value": 92.0,
            "param": "d3"
          },
          {
            "id": "k9",
            "type": "distance_x",
            "p": 2,
            "q": 3,
            "value": 10.0,
            "param": "d4"
          },
          {
            "id": "k10",
            "type": "distance_y",
            "p": 4,
            "q": 3,
            "value": 65.0,
            "param": "d5"
          },
          {
            "id": "k11",
            "type": "distance_x",
            "p": 5,
            "q": 6,
            "value": 10.0,
            "param": "d6"
          },
          {
            "id": "k12",
            "type": "distance_y",
            "p": 5,
            "q": 6,
            "value": 27.0,
            "param": "d7"
          }
        ],
        "links": [
          {
            "id": "j1",
            "kind": "project",
            "source": {
              "type": "sketch_axis",
              "name": "x"
            }
          },
          {
            "id": "j2",
            "kind": "project",
            "source": {
              "type": "sketch_axis",
              "name": "y"
            }
          }
        ],
        "counters": {
          "j": 2,
          "k": 12,
          "l": 8,
          "p": 4
        }
      }
    },
    {
      "id": 2,
      "name": "BatteryStock",
      "suppressed": false,
      "body_names": [
        "Battery"
      ],
      "param_names": [
        "d8"
      ],
      "type": "extrude",
      "sketch": 1,
      "profiles": {
        "type": "all"
      },
      "extent": {
        "distance": "batt_len + tab_depth + clear",
        "direction": "positive"
      },
      "operation": "new_body"
    },
    {
      "id": 3,
      "name": "PlanOutline",
      "suppressed": false,
      "param_names": [
        "d9"
      ],
      "type": "sketch",
      "plane": {
        "type": "offset",
        "base": {
          "type": "origin",
          "name": "XY"
        },
        "distance": "-clear"
      },
      "sketch": {
        "points": [
          {
            "id": "origin",
            "pos": [
              0.0,
              0.0
            ],
            "fixed": true
          },
          {
            "id": "l1.start",
            "pos": [
              -160.0,
              1.0
            ],
            "fixed": false
          },
          {
            "id": "l1.end",
            "pos": [
              160.0,
              1.0
            ],
            "fixed": false
          },
          {
            "id": "l2.end",
            "pos": [
              160.0,
              -198.0
            ],
            "fixed": false
          },
          {
            "id": "corner1",
            "pos": [
              -95.0,
              -198.0
            ],
            "fixed": false
          },
          {
            "id": "corner2",
            "pos": [
              -130.0,
              -203.5
            ],
            "fixed": false
          },
          {
            "id": "l5.end",
            "pos": [
              -160.0,
              -203.5
            ],
            "fixed": false
          },
          {
            "id": "p1",
            "pos": [
              0.0,
              -100.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              0.0,
              100.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              -88.75261852925456,
              -198.0
            ],
            "fixed": false
          },
          {
            "id": "p4",
            "pos": [
              -101.17164524862525,
              -198.96982996764112
            ],
            "fixed": false
          },
          {
            "id": "a1.center",
            "pos": [
              -88.75261852925458,
              -278.0
            ],
            "fixed": false
          },
          {
            "id": "p5",
            "pos": [
              -126.91417737568737,
              -203.01508501617946
            ],
            "fixed": false
          },
          {
            "id": "p6",
            "pos": [
              -133.12369073537272,
              -203.5
            ],
            "fixed": false
          },
          {
            "id": "a2.center",
            "pos": [
              -133.12369073537272,
              -163.5
            ],
            "fixed": false
          }
        ],
        "curves": [
          {
            "id": "l1",
            "type": "line",
            "a": 1,
            "b": 2,
            "construction": false
          },
          {
            "id": "l2",
            "type": "line",
            "a": 2,
            "b": 3,
            "construction": false
          },
          {
            "id": "l3",
            "type": "line",
            "a": 3,
            "b": 9,
            "construction": false
          },
          {
            "id": "l4",
            "type": "line",
            "a": 10,
            "b": 12,
            "construction": false
          },
          {
            "id": "l5",
            "type": "line",
            "a": 13,
            "b": 6,
            "construction": false
          },
          {
            "id": "l6",
            "type": "line",
            "a": 6,
            "b": 1,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 7,
            "b": 8,
            "construction": true,
            "link": "j1"
          },
          {
            "id": "a1",
            "type": "arc",
            "c": 11,
            "a": 9,
            "b": 10,
            "construction": false
          },
          {
            "id": "a2",
            "type": "arc",
            "c": 14,
            "a": 13,
            "b": 12,
            "construction": false
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "horizontal",
            "l": 0
          },
          {
            "id": "k2",
            "type": "vertical",
            "l": 1
          },
          {
            "id": "k3",
            "type": "horizontal",
            "l": 2
          },
          {
            "id": "k4",
            "type": "horizontal",
            "l": 4
          },
          {
            "id": "k5",
            "type": "vertical",
            "l": 5
          },
          {
            "id": "k6",
            "type": "symmetric",
            "p": 1,
            "q": 2,
            "l": 6
          },
          {
            "id": "k7",
            "type": "length",
            "l": 0,
            "value": 320.0,
            "param": "d10"
          },
          {
            "id": "k8",
            "type": "distance_y",
            "p": 0,
            "q": 1,
            "value": 1.0,
            "param": "d11"
          },
          {
            "id": "k9",
            "type": "distance_y",
            "p": 0,
            "q": 3,
            "value": 198.0,
            "param": "d12"
          },
          {
            "id": "k10",
            "type": "distance_y",
            "p": 0,
            "q": 5,
            "value": 203.5,
            "param": "d13"
          },
          {
            "id": "k11",
            "type": "distance_x",
            "p": 0,
            "q": 5,
            "value": 130.0,
            "param": "d14"
          },
          {
            "id": "k12",
            "type": "distance_x",
            "p": 5,
            "q": 4,
            "value": 35.0,
            "param": "d15"
          },
          {
            "id": "k13",
            "type": "point_on_curve",
            "p": 4,
            "c": 2
          },
          {
            "id": "k14",
            "type": "point_on_curve",
            "p": 4,
            "c": 3
          },
          {
            "id": "k15",
            "type": "tangent",
            "a": 2,
            "b": 7
          },
          {
            "id": "k16",
            "type": "tangent",
            "a": 3,
            "b": 7
          },
          {
            "id": "k17",
            "type": "radius",
            "c": 7,
            "value": 80.0,
            "param": "d16"
          },
          {
            "id": "k18",
            "type": "point_on_curve",
            "p": 5,
            "c": 3
          },
          {
            "id": "k19",
            "type": "point_on_curve",
            "p": 5,
            "c": 4
          },
          {
            "id": "k20",
            "type": "tangent",
            "a": 3,
            "b": 8
          },
          {
            "id": "k21",
            "type": "tangent",
            "a": 4,
            "b": 8
          },
          {
            "id": "k22",
            "type": "radius",
            "c": 8,
            "value": 40.0,
            "param": "d17"
          }
        ],
        "links": [
          {
            "id": "j1",
            "kind": "project",
            "source": {
              "type": "sketch_axis",
              "name": "y"
            }
          }
        ],
        "counters": {
          "a": 2,
          "corner": 2,
          "j": 1,
          "k": 22,
          "l": 7,
          "p": 6
        }
      }
    },
    {
      "id": 4,
      "name": "TabAndRamp",
      "suppressed": false,
      "param_names": [
        "d18"
      ],
      "type": "extrude",
      "sketch": 3,
      "profiles": {
        "type": "all"
      },
      "extent": {
        "distance": "batt_h + 2 * clear",
        "direction": "positive"
      },
      "operation": "intersect",
      "targets": [
        "Battery"
      ]
    },
    {
      "id": 5,
      "name": "CopyForCase",
      "suppressed": false,
      "body_names": [
        "Case"
      ],
      "param_names": [
        "d19",
        "d20",
        "d21"
      ],
      "type": "move",
      "bodies": [
        "Battery"
      ],
      "translate": [
        "0",
        "0",
        "0"
      ],
      "copy": true
    },
    {
      "id": 6,
      "name": "CaseShell",
      "suppressed": false,
      "param_names": [
        "d22"
      ],
      "face_names": [
        "F2:side:l4"
      ],
      "type": "shell",
      "faces": [
        [
          0.0,
          -100.0,
          0.0
        ]
      ],
      "thickness": "wall",
      "body": "Case",
      "direction": "outside"
    },
    {
      "id": 7,
      "name": "Fillet1",
      "suppressed": false,
      "param_names": [
        "d23"
      ],
      "edge_refs": [
        {
          "dir": [
            0.0,
            -1.0,
            0.0
          ],
          "min": [
            -158.08338344416106,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            158.08338344416106,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            0.0,
            -1.0,
            0.0
          ],
          "min": [
            -158.08338344416106,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            158.08338344416106,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            0.0,
            1.0,
            0.0
          ],
          "min": [
            -158.08338344416106,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            158.08338344416106,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            0.0,
            -1.0,
            0.0
          ],
          "min": [
            -158.08338344416106,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            158.08338344416106,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            0.0,
            1.0,
            0.0
          ],
          "min": [
            -158.08338344416106,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            158.08338344416106,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            0.0,
            1.0,
            0.0
          ],
          "min": [
            -158.08338344416106,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            158.08338344416106,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            0.0,
            -1.0,
            0.0
          ],
          "min": [
            -158.08338344416106,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            158.08338344416106,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            0.0,
            1.0,
            0.0
          ],
          "min": [
            -158.08338344416106,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            158.08338344416106,
            3.0000000000000004,
            95.0
          ]
        }
      ],
      "edge_names": [
        "F2:side:l5@F5.1|F2:side:l6@F5.1",
        "F2:side:l1@F5.1|F2:side:l6@F5.1",
        "F2:side:l1@F5.1|F2:side:l2@F5.1",
        "F2:side:l2@F5.1|F2:side:l3@F5.1",
        "F6:inner:F2:side:l5@F5.1|F6:inner:F2:side:l6@F5.1",
        "F6:inner:F2:side:l1@F5.1|F6:inner:F2:side:l6@F5.1",
        "F6:inner:F2:side:l1@F5.1|F6:inner:F2:side:l2@F5.1",
        "F6:inner:F2:side:l2@F5.1|F6:inner:F2:side:l3@F5.1"
      ],
      "type": "fillet",
      "edges": [
        [
          -158.0834,
          -101.75,
          26.6874
        ],
        [
          -147.5738,
          -101.75,
          95.0
        ],
        [
          147.088,
          -99.0,
          95.0
        ],
        [
          158.0834,
          -99.0,
          65.3126
        ],
        [
          -155.0,
          -101.75,
          27.0
        ],
        [
          -145.0,
          -101.75,
          92.0
        ],
        [
          145.0,
          -99.0,
          92.0
        ],
        [
          155.0,
          -99.0,
          65.0
        ]
      ],
      "radius": "r_big",
      "body": "Case"
    },
    {
      "id": 8,
      "name": "Fillet2",
      "suppressed": false,
      "param_names": [
        "d24"
      ],
      "edge_refs": [
        {
          "dir": [
            0.34731435582359377,
            0.0,
            -0.9377487607237037
          ],
          "min": [
            -157.9189160095873,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            157.9189160095873,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            -0.1520571842539413,
            6.854850442197856e-18,
            -0.9883716976506173
          ],
          "min": [
            -157.9189160095873,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            157.9189160095873,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            -0.3473143558235947,
            0.0,
            0.9377487607237034
          ],
          "min": [
            -157.9189160095873,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            157.9189160095873,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            0.15205718425394107,
            6.990187846251234e-18,
            0.9883716976506172
          ],
          "min": [
            -157.9189160095873,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            157.9189160095873,
            3.0000000000000004,
            95.0
          ]
        },
        {
          "dir": [
            1.0,
            -1.5479309295511088e-18,
            0.0
          ],
          "min": [
            -157.9189160095873,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            157.9189160095873,
            3.0000000000000004,
            95.0
          ]
        }
      ],
      "edge_names": [
        "F2:side:l2@F5.1|F2:start@F5.1",
        "F2:side:l3@F5.1|F2:start@F5.1",
        "F2:side:l5@F5.1|F2:start@F5.1",
        "F2:side:l6@F5.1|F2:start@F5.1",
        "F2:side:l1@F5.1|F2:start@F5.1"
      ],
      "type": "fillet",
      "edges": [
        [
          152.5857,
          3.0,
          80.1563
        ],
        [
          153.0593,
          3.0,
          32.6563
        ],
        [
          -153.1413,
          3.0,
          13.3437
        ],
        [
          -152.8286,
          3.0,
          60.8437
        ],
        [
          -0.2429,
          3.0,
          95.0
        ]
      ],
      "radius": "r_small",
      "body": "Case"
    },
    {
      "id": 9,
      "name": "Fillet3",
      "suppressed": false,
      "param_names": [
        "d25"
      ],
      "edge_refs": [
        {
          "dir": [
            -1.0,
            -2.4685277687458984e-16,
            0.0
          ],
          "min": [
            -157.91891600958726,
            -206.5,
            -2.220446049250313e-16
          ],
          "max": [
            157.91891600958732,
            3.000000000000001,
            95.0
          ]
        }
      ],
      "edge_names": [
        "F6:inner:F2:side:l1@F5.1|F6:inner:F4:side:l3@F5.1"
      ],
      "type": "fillet",
      "edges": [
        [
          26.3837,
          -198.0,
          92.0
        ]
      ],
      "radius": "r_small",
      "body": "Case"
    },
    {
      "id": 10,
      "name": "Fillet4",
      "suppressed": false,
      "param_names": [
        "d26"
      ],
      "edge_refs": [
        {
          "dir": [
            0.1520571842539411,
            0.0,
            0.988371697650617
          ],
          "min": [
            -157.91891600958726,
            -206.5,
            -1.6689699017913788e-14
          ],
          "max": [
            157.91891600958732,
            3.000000000000001,
            95.0
          ]
        },
        {
          "dir": [
            -0.34731435582359377,
            0.0,
            0.9377487607237037
          ],
          "min": [
            -157.91891600958726,
            -206.5,
            -1.6689699017913788e-14
          ],
          "max": [
            157.91891600958732,
            3.000000000000001,
            95.0
          ]
        },
        {
          "dir": [
            -1.0,
            0.0,
            0.0
          ],
          "min": [
            -157.91891600958726,
            -206.5,
            -1.6689699017913788e-14
          ],
          "max": [
            157.91891600958732,
            3.000000000000001,
            95.0
          ]
        },
        {
          "dir": [
            -0.15205718425394102,
            0.0,
            -0.9883716976506172
          ],
          "min": [
            -157.91891600958726,
            -206.5,
            -1.6689699017913788e-14
          ],
          "max": [
            157.91891600958732,
            3.000000000000001,
            95.0
          ]
        },
        {
          "dir": [
            0.3473143558235949,
            0.0,
            -0.9377487607237033
          ],
          "min": [
            -157.91891600958726,
            -206.5,
            -1.6689699017913788e-14
          ],
          "max": [
            157.91891600958732,
            3.000000000000001,
            95.0
          ]
        }
      ],
      "edge_names": [
        "F6:inner:F2:side:l3@F5.1|F6:inner:F2:start@F5.1",
        "F6:inner:F2:side:l2@F5.1|F6:inner:F2:start@F5.1",
        "F6:inner:F2:side:l1@F5.1|F6:inner:F2:start@F5.1",
        "F6:inner:F2:side:l6@F5.1|F6:inner:F2:start@F5.1",
        "F6:inner:F2:side:l5@F5.1|F6:inner:F2:start@F5.1"
      ],
      "type": "fillet",
      "edges": [
        [
          149.9014,
          0.0,
          31.8594
        ],
        [
          150.3792,
          0.0,
          77.4761
        ],
        [
          0.4048,
          0.0,
          92.0
        ],
        [
          -150.2276,
          0.0,
          58.0208
        ],
        [
          -149.7749,
          0.0,
          12.8922
        ]
      ],
      "radius": "r_small",
      "body": "Case"
    }
  ],
  "next_id": 30,
  "visibility": {
    "hidden_bodies": [
      "Repro"
    ],
    "hidden_occurrences": [],
    "hidden_origin": [],
    "hidden_sketches": [],
    "shown_sketches": []
  }
}