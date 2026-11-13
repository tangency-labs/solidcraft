{
  "format": "solidcraft/21",
  "name": "Battery TPU Case v2",
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
      "comment": "Connector tab width",
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
      "expr": "2.5 mm",
      "unit": "mm",
      "comment": "TPU wall thickness",
      "model": false
    },
    {
      "name": "r_big",
      "expr": "5 mm",
      "unit": "mm",
      "comment": "Fillet on the 4 long edges",
      "model": false
    },
    {
      "name": "r_small",
      "expr": "1 mm",
      "unit": "mm",
      "comment": "Fillet on all other edges",
      "model": false
    },
    {
      "name": "clear",
      "expr": "1 mm",
      "unit": "mm",
      "comment": "Tool overshoot",
      "model": false
    },
    {
      "name": "pleat_top",
      "expr": "batt_h - 2 * r_small",
      "unit": "mm",
      "comment": "Height where the pleat fades out",
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
        "convention": "total_distance",
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
        "convention": "total_distance",
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
      "name": "PleatCutSketch",
      "suppressed": false,
      "param_names": [
        "d23"
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
            "id": "p1",
            "pos": [
              -24.0,
              -0.5
            ],
            "fixed": false
          },
          {
            "id": "p2",
            "pos": [
              24.0,
              -0.5
            ],
            "fixed": false
          },
          {
            "id": "p3",
            "pos": [
              24.0,
              3.5
            ],
            "fixed": false
          },
          {
            "id": "p4",
            "pos": [
              -24.0,
              3.5
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
            "b": 4,
            "construction": false
          },
          {
            "id": "l4",
            "type": "line",
            "a": 4,
            "b": 1,
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
            "type": "horizontal",
            "l": 2
          },
          {
            "id": "k3",
            "type": "vertical",
            "l": 1
          },
          {
            "id": "k4",
            "type": "vertical",
            "l": 3
          }
        ],
        "counters": {
          "k": 4,
          "l": 4,
          "p": 4
        }
      }
    },
    {
      "id": 8,
      "name": "PleatCut",
      "suppressed": false,
      "param_names": [
        "d24"
      ],
      "type": "extrude",
      "sketch": 7,
      "profiles": {
        "type": "all"
      },
      "extent": {
        "distance": "pleat_top + clear",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "cut",
      "targets": [
        "Case"
      ]
    },
    {
      "id": 9,
      "name": "PleatBottom",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "origin",
        "name": "XY"
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
              -26.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l1.end",
            "pos": [
              -24.76384,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l2.end",
            "pos": [
              -16.0568,
              14.97217
            ],
            "fixed": false
          },
          {
            "id": "l3.end",
            "pos": [
              -8.0,
              4.90117
            ],
            "fixed": false
          },
          {
            "id": "l4.end",
            "pos": [
              0.0,
              14.90117
            ],
            "fixed": false
          },
          {
            "id": "l5.end",
            "pos": [
              8.0,
              4.90117
            ],
            "fixed": false
          },
          {
            "id": "l6.end",
            "pos": [
              16.0568,
              14.97217
            ],
            "fixed": false
          },
          {
            "id": "l7.end",
            "pos": [
              24.76384,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l8.end",
            "pos": [
              26.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l9.end",
            "pos": [
              26.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l10.end",
            "pos": [
              23.23616,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l11.end",
            "pos": [
              15.9432,
              10.02783
            ],
            "fixed": false
          },
          {
            "id": "l12.end",
            "pos": [
              8.0,
              0.09883
            ],
            "fixed": false
          },
          {
            "id": "l13.end",
            "pos": [
              0.0,
              10.09883
            ],
            "fixed": false
          },
          {
            "id": "l14.end",
            "pos": [
              -8.0,
              0.09883
            ],
            "fixed": false
          },
          {
            "id": "l15.end",
            "pos": [
              -15.9432,
              10.02783
            ],
            "fixed": false
          },
          {
            "id": "l16.end",
            "pos": [
              -23.23616,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l17.end",
            "pos": [
              -26.0,
              0.0
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
            "b": 7,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 7,
            "b": 8,
            "construction": false
          },
          {
            "id": "l8",
            "type": "line",
            "a": 8,
            "b": 9,
            "construction": false
          },
          {
            "id": "l9",
            "type": "line",
            "a": 9,
            "b": 10,
            "construction": false
          },
          {
            "id": "l10",
            "type": "line",
            "a": 10,
            "b": 11,
            "construction": false
          },
          {
            "id": "l11",
            "type": "line",
            "a": 11,
            "b": 12,
            "construction": false
          },
          {
            "id": "l12",
            "type": "line",
            "a": 12,
            "b": 13,
            "construction": false
          },
          {
            "id": "l13",
            "type": "line",
            "a": 13,
            "b": 14,
            "construction": false
          },
          {
            "id": "l14",
            "type": "line",
            "a": 14,
            "b": 15,
            "construction": false
          },
          {
            "id": "l15",
            "type": "line",
            "a": 15,
            "b": 16,
            "construction": false
          },
          {
            "id": "l16",
            "type": "line",
            "a": 16,
            "b": 17,
            "construction": false
          },
          {
            "id": "l17",
            "type": "line",
            "a": 17,
            "b": 18,
            "construction": false
          },
          {
            "id": "l18",
            "type": "line",
            "a": 18,
            "b": 1,
            "construction": false
          }
        ],
        "constraints": [],
        "counters": {
          "l": 18
        }
      }
    },
    {
      "id": 10,
      "name": "PleatTop",
      "suppressed": false,
      "param_names": [
        "d25"
      ],
      "type": "sketch",
      "plane": {
        "type": "offset",
        "base": {
          "type": "origin",
          "name": "XY"
        },
        "distance": "pleat_top + 0.5 mm"
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
              -26.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l1.end",
            "pos": [
              -24.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l2.end",
            "pos": [
              -16.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l3.end",
            "pos": [
              -8.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l4.end",
            "pos": [
              0.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l5.end",
            "pos": [
              8.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l6.end",
            "pos": [
              16.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l7.end",
            "pos": [
              24.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l8.end",
            "pos": [
              26.0,
              3.0
            ],
            "fixed": false
          },
          {
            "id": "l9.end",
            "pos": [
              26.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l10.end",
            "pos": [
              24.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l11.end",
            "pos": [
              16.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l12.end",
            "pos": [
              8.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l13.end",
            "pos": [
              0.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l14.end",
            "pos": [
              -8.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l15.end",
            "pos": [
              -16.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l16.end",
            "pos": [
              -24.0,
              0.0
            ],
            "fixed": false
          },
          {
            "id": "l17.end",
            "pos": [
              -26.0,
              0.0
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
            "b": 7,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 7,
            "b": 8,
            "construction": false
          },
          {
            "id": "l8",
            "type": "line",
            "a": 8,
            "b": 9,
            "construction": false
          },
          {
            "id": "l9",
            "type": "line",
            "a": 9,
            "b": 10,
            "construction": false
          },
          {
            "id": "l10",
            "type": "line",
            "a": 10,
            "b": 11,
            "construction": false
          },
          {
            "id": "l11",
            "type": "line",
            "a": 11,
            "b": 12,
            "construction": false
          },
          {
            "id": "l12",
            "type": "line",
            "a": 12,
            "b": 13,
            "construction": false
          },
          {
            "id": "l13",
            "type": "line",
            "a": 13,
            "b": 14,
            "construction": false
          },
          {
            "id": "l14",
            "type": "line",
            "a": 14,
            "b": 15,
            "construction": false
          },
          {
            "id": "l15",
            "type": "line",
            "a": 15,
            "b": 16,
            "construction": false
          },
          {
            "id": "l16",
            "type": "line",
            "a": 16,
            "b": 17,
            "construction": false
          },
          {
            "id": "l17",
            "type": "line",
            "a": 17,
            "b": 18,
            "construction": false
          },
          {
            "id": "l18",
            "type": "line",
            "a": 18,
            "b": 1,
            "construction": false
          }
        ],
        "constraints": [],
        "counters": {
          "l": 18
        }
      }
    },
    {
      "id": 11,
      "name": "PleatNear",
      "suppressed": false,
      "type": "loft",
      "sections": [
        {
          "sketch": 9,
          "profiles": {
            "type": "all"
          }
        },
        {
          "sketch": 10,
          "profiles": {
            "type": "all"
          }
        }
      ],
      "operation": "join",
      "targets": [
        "Case"
      ]
    },
    {
      "id": 12,
      "name": "MidLength",
      "suppressed": false,
      "param_names": [
        "d26"
      ],
      "type": "construction_plane",
      "plane": {
        "type": "offset",
        "base": {
          "type": "origin",
          "name": "XZ"
        },
        "distance": "-batt_len / 2"
      }
    },
    {
      "id": 13,
      "name": "PleatFar",
      "suppressed": false,
      "type": "mirror",
      "features": [
        "PleatCut",
        "PleatNear"
      ],
      "plane": {
        "type": "construction",
        "name": "MidLength"
      }
    },
    {
      "id": 14,
      "name": "FilletBig",
      "suppressed": false,
      "param_names": [
        "d27"
      ],
      "edge_refs": [
        {
          "dir": [
            0.0,
            -1.0,
            0.0
          ],
          "min": [
            -158.0833834441611,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            158.08338344416106,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.0,
            -1.0,
            0.0
          ],
          "min": [
            -158.0833834441611,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            158.08338344416106,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.0,
            1.0,
            0.0
          ],
          "min": [
            -158.0833834441611,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            158.08338344416106,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.0,
            -1.0,
            0.0
          ],
          "min": [
            -158.0833834441611,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            158.08338344416106,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.0,
            1.0,
            0.0
          ],
          "min": [
            -158.0833834441611,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            158.08338344416106,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.0,
            1.0,
            0.0
          ],
          "min": [
            -158.0833834441611,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            158.08338344416106,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.0,
            -1.0,
            0.0
          ],
          "min": [
            -158.0833834441611,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            158.08338344416106,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.0,
            1.0,
            0.0
          ],
          "min": [
            -158.0833834441611,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            158.08338344416106,
            14.97245064486031,
            95.00000000000003
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
      "id": 15,
      "name": "FilletFrontOuter",
      "suppressed": false,
      "param_names": [
        "d28"
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
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.9189160095873,
            14.97245064486031,
            95.00000000000003
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
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.9189160095873,
            14.97245064486031,
            95.00000000000003
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
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.9189160095873,
            14.97245064486031,
            95.00000000000003
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
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.9189160095873,
            14.97245064486031,
            95.00000000000003
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
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.9189160095873,
            14.97245064486031,
            95.00000000000003
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
          -80.0,
          3.0,
          95.0
        ]
      ],
      "radius": "r_small",
      "body": "Case"
    },
    {
      "id": 16,
      "name": "FilletRearOuter",
      "suppressed": false,
      "param_names": [
        "d29"
      ],
      "edge_refs": [
        {
          "dir": [
            -0.1520571842539413,
            0.0,
            -0.9883716976506173
          ],
          "min": [
            -157.92605659069406,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659069403,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.34731435582359377,
            0.0,
            -0.9377487607237037
          ],
          "min": [
            -157.92605659069406,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659069403,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            -0.3473143558235947,
            0.0,
            0.9377487607237034
          ],
          "min": [
            -157.92605659069406,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659069403,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            0.15205718425394107,
            0.0,
            0.9883716976506172
          ],
          "min": [
            -157.92605659069406,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659069403,
            14.97245064486031,
            95.00000000000003
          ]
        },
        {
          "dir": [
            1.0,
            2.4463450711209976e-16,
            0.0
          ],
          "min": [
            -157.92605659069406,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659069403,
            14.97245064486031,
            95.00000000000003
          ]
        }
      ],
      "edge_names": [
        "F2:side:l3@F5.1|F4:side:l3@F5.1",
        "F2:side:l2@F5.1|F4:side:l3@F5.1",
        "F2:side:l5@F5.1|F4:side:l5@F5.1",
        "F2:side:l6@F5.1|F4:side:l5@F5.1",
        "F2:side:l1@F5.1|F4:side:l3@F5.1"
      ],
      "type": "fillet",
      "edges": [
        [
          153.0593,
          -201.0,
          32.6563
        ],
        [
          152.5857,
          -201.0,
          80.1563
        ],
        [
          -153.1413,
          -206.5,
          13.3437
        ],
        [
          -152.8286,
          -206.5,
          60.8437
        ],
        [
          50.0,
          -201.0,
          95.0
        ]
      ],
      "radius": "r_small",
      "body": "Case"
    },
    {
      "id": 17,
      "name": "FilletFrontInner",
      "suppressed": false,
      "param_names": [
        "d30"
      ],
      "edge_refs": [
        {
          "dir": [
            0.1520571842539411,
            0.0,
            0.988371697650617
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        },
        {
          "dir": [
            -0.34731435582359377,
            0.0,
            0.9377487607237037
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        },
        {
          "dir": [
            -1.0,
            0.0,
            0.0
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        },
        {
          "dir": [
            -0.15205718425394102,
            0.0,
            -0.9883716976506172
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        },
        {
          "dir": [
            0.3473143558235949,
            0.0,
            -0.9377487607237033
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
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
          80.0,
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
    },
    {
      "id": 18,
      "name": "FilletRearInner",
      "suppressed": false,
      "param_names": [
        "d31"
      ],
      "edge_refs": [
        {
          "dir": [
            0.1520571842539411,
            0.0,
            0.988371697650617
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        },
        {
          "dir": [
            -0.34731435582359377,
            0.0,
            0.9377487607237037
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        },
        {
          "dir": [
            -1.0,
            -2.4685277687458984e-16,
            0.0
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        },
        {
          "dir": [
            -0.15205718425394102,
            0.0,
            -0.9883716976506172
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        },
        {
          "dir": [
            0.347314355823594,
            0.0,
            -0.9377487607237036
          ],
          "min": [
            -157.9260565909712,
            -212.97245548710006,
            -0.0005980913533997968
          ],
          "max": [
            157.92605659097126,
            14.97245064486031,
            95.00000000000001
          ]
        }
      ],
      "edge_names": [
        "F6:inner:F2:side:l3@F5.1|F6:inner:F4:side:l3@F5.1",
        "F6:inner:F2:side:l2@F5.1|F6:inner:F4:side:l3@F5.1",
        "F6:inner:F2:side:l1@F5.1|F6:inner:F4:side:l3@F5.1",
        "F6:inner:F2:side:l6@F5.1|F6:inner:F4:side:l5@F5.1",
        "F6:inner:F2:side:l5@F5.1|F6:inner:F4:side:l5@F5.1"
      ],
      "type": "fillet",
      "edges": [
        [
          149.9014,
          -198.0,
          31.8594
        ],
        [
          150.3792,
          -198.0,
          77.4761
        ],
        [
          80.0,
          -198.0,
          92.0
        ],
        [
          -150.2276,
          -203.5,
          58.0208
        ],
        [
          -149.7749,
          -203.5,
          12.8922
        ]
      ],
      "radius": "r_small",
      "body": "Case"
    }
  ],
  "next_id": 19,
  "visibility": {
    "hidden_bodies": [
      "Battery"
    ],
    "hidden_occurrences": [],
    "hidden_origin": [],
    "hidden_sketches": [],
    "shown_sketches": []
  }
}