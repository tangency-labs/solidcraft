{
  "format": "solidcraft/16",
  "name": "enclosure-pi",
  "units": "mm",
  "params": [
    {
      "name": "board_w",
      "expr": "85 mm",
      "unit": "mm",
      "model": false
    },
    {
      "name": "board_d",
      "expr": "56 mm",
      "unit": "mm",
      "model": false
    },
    {
      "name": "wall",
      "expr": "2 mm",
      "unit": "mm",
      "model": false
    },
    {
      "name": "height",
      "expr": "32 mm",
      "unit": "mm",
      "model": false
    },
    {
      "name": "d1",
      "expr": "board_w + 10 mm",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d2",
      "expr": "board_d + 10 mm",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d7",
      "expr": "board_w / 2 - 3.5 mm",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d8",
      "expr": "board_d / 2 - 3.5 mm",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d9",
      "expr": "6 mm",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d10",
      "expr": "2.5 mm",
      "unit": "mm",
      "model": true
    }
  ],
  "features": [
    {
      "id": 1,
      "name": "Sketch1",
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
            "id": "p1",
            "pos": [
              -47.5,
              -33.0
            ],
            "fixed": false
          },
          {
            "id": "p2",
            "pos": [
              47.5,
              -33.0
            ],
            "fixed": false
          },
          {
            "id": "p3",
            "pos": [
              47.5,
              33.0
            ],
            "fixed": false
          },
          {
            "id": "p4",
            "pos": [
              -47.5,
              33.0
            ],
            "fixed": false
          },
          {
            "id": "p5",
            "pos": [
              0.0,
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
            "b": 1,
            "construction": false
          },
          {
            "id": "l5",
            "type": "line",
            "a": 1,
            "b": 3,
            "construction": true
          },
          {
            "id": "l6",
            "type": "line",
            "a": 2,
            "b": 4,
            "construction": true
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
          },
          {
            "id": "k5",
            "type": "midpoint",
            "p": 5,
            "l": 4
          },
          {
            "id": "k6",
            "type": "coincident",
            "p": 0,
            "q": 5
          },
          {
            "id": "k7",
            "type": "length",
            "l": 2,
            "value": 95.0,
            "param": "d1",
            "text": [
              0.0,
              -8.0
            ]
          },
          {
            "id": "k8",
            "type": "length",
            "l": 3,
            "value": 66.0,
            "param": "d2",
            "text": [
              -0.0,
              -8.0
            ]
          }
        ],
        "counters": {
          "k": 8,
          "l": 6,
          "p": 5
        }
      }
    },
    {
      "id": 2,
      "name": "Extrude1",
      "suppressed": false,
      "param_names": [
        "d3"
      ],
      "type": "extrude",
      "sketch": 1,
      "profiles": {
        "type": "curves",
        "loops": [
          [
            "l1",
            "l2",
            "l3",
            "l4"
          ]
        ]
      },
      "extent": {
        "distance": "height",
        "direction": "positive"
      },
      "operation": "new_body"
    },
    {
      "id": 3,
      "name": "Fillet1",
      "suppressed": false,
      "param_names": [
        "d4"
      ],
      "edge_refs": [
        {
          "dir": [
            0.0,
            0.0,
            1.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.5,
            33.0,
            24.0
          ]
        },
        {
          "dir": [
            0.0,
            0.0,
            -1.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.5,
            33.0,
            24.0
          ]
        },
        {
          "dir": [
            0.0,
            0.0,
            1.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.5,
            33.0,
            24.0
          ]
        },
        {
          "dir": [
            0.0,
            0.0,
            1.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.5,
            33.0,
            24.0
          ]
        }
      ],
      "edge_names": [
        "F2:side:l1|F2:side:l2",
        "F2:side:l1|F2:side:l4",
        "F2:side:l2|F2:side:l3",
        "F2:side:l3|F2:side:l4"
      ],
      "type": "fillet",
      "edges": [
        [
          47.5,
          -33.0,
          12.0
        ],
        [
          -47.5,
          -33.0,
          12.0
        ],
        [
          47.5,
          33.0,
          12.0
        ],
        [
          -47.5,
          33.0,
          12.0
        ]
      ],
      "radius": "5"
    },
    {
      "id": 4,
      "name": "Shell1",
      "suppressed": false,
      "param_names": [
        "d5"
      ],
      "face_names": [
        "F2:top"
      ],
      "type": "shell",
      "faces": [
        [
          9.814026498133899e-7,
          -4.161589913564967e-6,
          23.999999999999943
        ]
      ],
      "thickness": "wall",
      "tangent_chain": true
    },
    {
      "id": 5,
      "name": "Fillet2",
      "suppressed": false,
      "param_names": [
        "d6"
      ],
      "edge_refs": [
        {
          "dir": [
            1.0,
            3.343730521224001e-16,
            0.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.500000000000014,
            33.00000000000001,
            24.00000000000001
          ]
        },
        {
          "dir": [
            0.0,
            0.0,
            0.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.500000000000014,
            33.00000000000001,
            24.00000000000001
          ]
        },
        {
          "dir": [
            0.0,
            0.0,
            0.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.500000000000014,
            33.00000000000001,
            24.00000000000001
          ]
        },
        {
          "dir": [
            5.075305255429287e-16,
            -1.0,
            0.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.500000000000014,
            33.00000000000001,
            24.00000000000001
          ]
        },
        {
          "dir": [
            0.0,
            0.0,
            0.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.500000000000014,
            33.00000000000001,
            24.00000000000001
          ]
        },
        {
          "dir": [
            -1.0,
            -3.343730521224001e-16,
            0.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.500000000000014,
            33.00000000000001,
            24.00000000000001
          ]
        },
        {
          "dir": [
            0.0,
            0.0,
            0.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.500000000000014,
            33.00000000000001,
            24.00000000000001
          ]
        },
        {
          "dir": [
            -1.2688263138573217e-16,
            1.0,
            0.0
          ],
          "min": [
            -47.5,
            -33.0,
            0.0
          ],
          "max": [
            47.500000000000014,
            33.00000000000001,
            24.00000000000001
          ]
        }
      ],
      "edge_names": [
        "F2:side:l3|F2:top",
        "F2:top|F3:blend:F2:side:l3|F2:side:l4",
        "F2:top|F3:blend:F2:side:l2|F2:side:l3",
        "F2:side:l2|F2:top",
        "F2:top|F3:blend:F2:side:l1|F2:side:l2",
        "F2:side:l1|F2:top",
        "F2:top|F3:blend:F2:side:l1|F2:side:l4",
        "F2:side:l4|F2:top"
      ],
      "type": "fillet",
      "edges": [
        [
          0.0,
          32.999999999999986,
          24.0
        ],
        [
          -46.03553390593275,
          31.535533905932738,
          24.0
        ],
        [
          46.03553390593274,
          31.535533905932745,
          24.000000000000004
        ],
        [
          47.499999999999986,
          0.0,
          24.0
        ],
        [
          46.03553390593275,
          -31.535533905932738,
          24.0
        ],
        [
          0.0,
          -32.999999999999986,
          24.0
        ],
        [
          -46.03553390593273,
          -31.53553390593274,
          23.999999999999996
        ],
        [
          -47.5,
          -3.552713678800501e-15,
          24.0
        ]
      ],
      "radius": "1"
    },
    {
      "id": 6,
      "name": "Sketch2",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "face",
        "plane": {
          "origin": [
            0.0,
            0.0,
            1.999999999999998
          ],
          "x_dir": [
            1.0,
            0.0,
            0.0
          ],
          "y_dir": [
            0.0,
            1.0,
            0.0
          ]
        },
        "at": [
          -5.000003765078077,
          18.000004375539675,
          2.000000000000057
        ],
        "name": "F4:inner:F2:start",
        "anchor": {
          "target": {
            "body": "Body1",
            "origin_feature": 4,
            "name": "F4:shell:inner:cap:low:ed6121d9bed14d3fd103f347231990d103349f33b7874ad292d3c4d3539266c3",
            "point": [
              0.0,
              0.0,
              2.0
            ],
            "certificate": {
              "kind": "shell_planar",
              "source_producer": 2,
              "source": {
                "kind": "extrude_cap",
                "sketch": 1,
                "high": false,
                "outer": [
                  "l1",
                  "l2",
                  "l3",
                  "l4"
                ],
                "holes": []
              },
              "inner": true
            }
          },
          "component": 0,
          "path": [],
          "author_path": [],
          "coordinates": [
            0.0,
            0.0
          ],
          "x_axis": [
            1.0,
            0.0
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
            "id": "p1",
            "pos": [
              -42.49999999999991,
              -27.999999999999872
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              -45.500000000000036,
              -27.99999999999999
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              -42.5,
              -31.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p4",
            "pos": [
              -45.5,
              27.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p5",
            "pos": [
              -42.4999999999997,
              27.999999999999833
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p6",
            "pos": [
              -42.50000000000001,
              31.00000000000013
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p7",
            "pos": [
              42.5,
              -31.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p8",
            "pos": [
              42.5,
              30.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p9",
            "pos": [
              42.50000000000015,
              -28.000000000000092
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p10",
            "pos": [
              45.5,
              -27.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p11",
            "pos": [
              42.499999999999886,
              27.999999999999773
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p12",
            "pos": [
              45.50000000000011,
              28.000000000000004
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "c1.center",
            "pos": [
              -39.0,
              -24.5
            ],
            "fixed": false
          }
        ],
        "curves": [
          {
            "id": "a1",
            "type": "arc",
            "c": 1,
            "a": 2,
            "b": 3,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l1",
            "type": "line",
            "a": 2,
            "b": 4,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a2",
            "type": "arc",
            "c": 5,
            "a": 6,
            "b": 4,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l2",
            "type": "line",
            "a": 7,
            "b": 3,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l3",
            "type": "line",
            "a": 6,
            "b": 8,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a3",
            "type": "arc",
            "c": 9,
            "a": 7,
            "b": 10,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a4",
            "type": "arc",
            "c": 11,
            "a": 12,
            "b": 8,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l4",
            "type": "line",
            "a": 12,
            "b": 10,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "c1",
            "type": "circle",
            "c": 13,
            "r": 3.0,
            "construction": false
          },
          {
            "id": "c2",
            "type": "circle",
            "c": 13,
            "r": 1.25,
            "construction": false
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "distance_x",
            "p": 0,
            "q": 13,
            "value": 39.0,
            "param": "d7"
          },
          {
            "id": "k2",
            "type": "distance_y",
            "p": 0,
            "q": 13,
            "value": 24.5,
            "param": "d8"
          },
          {
            "id": "k3",
            "type": "diameter",
            "c": 8,
            "value": 6.0,
            "param": "d9"
          },
          {
            "id": "k4",
            "type": "diameter",
            "c": 9,
            "value": 2.5,
            "param": "d10"
          }
        ],
        "links": [
          {
            "id": "j1",
            "kind": "project",
            "source": {
              "type": "face",
              "body": "Body1",
              "at": [
                -15.166666666666663,
                -29.99999999999999,
                1.9999999999999976
              ]
            },
            "face_name": "F4:shell:inner:cap:low:ed6121d9bed14d3fd103f347231990d103349f33b7874ad292d3c4d3539266c3",
            "auto_face": true
          }
        ],
        "counters": {
          "a": 4,
          "c": 2,
          "j": 1,
          "k": 4,
          "l": 4,
          "p": 12
        }
      }
    },
    {
      "id": 7,
      "name": "Extrude2",
      "suppressed": false,
      "param_names": [
        "d11"
      ],
      "type": "extrude",
      "sketch": 6,
      "profiles": {
        "type": "curves",
        "loops": [
          [
            "c1"
          ]
        ]
      },
      "extent": {
        "distance": "4 mm",
        "direction": "positive"
      },
      "operation": "join"
    },
    {
      "id": 8,
      "name": "RectangularPattern1",
      "suppressed": false,
      "param_names": [
        "d12",
        "d13",
        "d14",
        "d15"
      ],
      "type": "pattern",
      "features": [
        "Extrude2"
      ],
      "pattern": {
        "type": "rectangular",
        "dir1": [
          1.0,
          0.0,
          0.0
        ],
        "count1": "2",
        "spacing1": "58 mm",
        "dir2": [
          0.0,
          1.0,
          0.0
        ],
        "count2": "2",
        "spacing2": "49 mm"
      }
    },
    {
      "id": 9,
      "name": "Sketch3",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "face",
        "plane": {
          "origin": [
            0.0,
            -32.99999999999997,
            0.0
          ],
          "x_dir": [
            1.0,
            0.0,
            -0.0
          ],
          "y_dir": [
            0.0,
            0.0,
            1.0
          ]
        },
        "at": [
          10.000004238290387,
          -33.0,
          15.999996572036082
        ],
        "name": "F2:side:l1",
        "anchor": {
          "target": {
            "body": "Body1",
            "origin_feature": 4,
            "name": "F4:shell:outer:side:l1",
            "point": [
              0.0,
              -33.0,
              0.0
            ],
            "certificate": {
              "kind": "shell_planar",
              "source_producer": 2,
              "source": {
                "kind": "extrude_side",
                "sketch": 1,
                "curve": "l1"
              },
              "inner": false
            }
          },
          "component": 0,
          "path": [],
          "author_path": [],
          "coordinates": [
            0.0,
            0.0
          ],
          "x_axis": [
            1.0,
            0.0
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
            "id": "p1",
            "pos": [
              -42.5,
              0.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              -42.5,
              22.999999999999968
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              42.5,
              0.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p4",
            "pos": [
              42.5,
              22.999999999999968
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p5",
            "pos": [
              -36.5,
              7.5
            ],
            "fixed": false
          },
          {
            "id": "p6",
            "pos": [
              -26.5,
              7.5
            ],
            "fixed": false
          },
          {
            "id": "p7",
            "pos": [
              -26.5,
              11.5
            ],
            "fixed": false
          },
          {
            "id": "p8",
            "pos": [
              -36.5,
              11.5
            ],
            "fixed": false
          },
          {
            "id": "p9",
            "pos": [
              -31.5,
              9.5
            ],
            "fixed": false
          },
          {
            "id": "p10",
            "pos": [
              -20.500001157466368,
              7.5
            ],
            "fixed": false
          },
          {
            "id": "p11",
            "pos": [
              -12.499998842533632,
              7.5
            ],
            "fixed": false
          },
          {
            "id": "p12",
            "pos": [
              -12.499998842533632,
              11.5
            ],
            "fixed": false
          },
          {
            "id": "p13",
            "pos": [
              -20.500001157466368,
              11.5
            ],
            "fixed": false
          },
          {
            "id": "p14",
            "pos": [
              -16.5,
              9.5
            ],
            "fixed": false
          },
          {
            "id": "p15",
            "pos": [
              -7.000003005960782,
              7.5
            ],
            "fixed": false
          },
          {
            "id": "p16",
            "pos": [
              1.0000030059607816,
              7.5
            ],
            "fixed": false
          },
          {
            "id": "p17",
            "pos": [
              1.0000030059607816,
              11.5
            ],
            "fixed": false
          },
          {
            "id": "p18",
            "pos": [
              -7.000003005960782,
              11.5
            ],
            "fixed": false
          },
          {
            "id": "p19",
            "pos": [
              -3.0,
              9.5
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
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l2",
            "type": "line",
            "a": 1,
            "b": 3,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l3",
            "type": "line",
            "a": 4,
            "b": 2,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l4",
            "type": "line",
            "a": 4,
            "b": 3,
            "construction": false,
            "link": "j1"
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
            "b": 5,
            "construction": false
          },
          {
            "id": "l9",
            "type": "line",
            "a": 5,
            "b": 7,
            "construction": true
          },
          {
            "id": "l10",
            "type": "line",
            "a": 6,
            "b": 8,
            "construction": true
          },
          {
            "id": "l11",
            "type": "line",
            "a": 10,
            "b": 11,
            "construction": false
          },
          {
            "id": "l12",
            "type": "line",
            "a": 11,
            "b": 12,
            "construction": false
          },
          {
            "id": "l13",
            "type": "line",
            "a": 12,
            "b": 13,
            "construction": false
          },
          {
            "id": "l14",
            "type": "line",
            "a": 13,
            "b": 10,
            "construction": false
          },
          {
            "id": "l15",
            "type": "line",
            "a": 10,
            "b": 12,
            "construction": true
          },
          {
            "id": "l16",
            "type": "line",
            "a": 11,
            "b": 13,
            "construction": true
          },
          {
            "id": "l17",
            "type": "line",
            "a": 15,
            "b": 16,
            "construction": false
          },
          {
            "id": "l18",
            "type": "line",
            "a": 16,
            "b": 17,
            "construction": false
          },
          {
            "id": "l19",
            "type": "line",
            "a": 17,
            "b": 18,
            "construction": false
          },
          {
            "id": "l20",
            "type": "line",
            "a": 18,
            "b": 15,
            "construction": false
          },
          {
            "id": "l21",
            "type": "line",
            "a": 15,
            "b": 17,
            "construction": true
          },
          {
            "id": "l22",
            "type": "line",
            "a": 16,
            "b": 18,
            "construction": true
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "horizontal",
            "l": 4
          },
          {
            "id": "k2",
            "type": "horizontal",
            "l": 6
          },
          {
            "id": "k3",
            "type": "vertical",
            "l": 5
          },
          {
            "id": "k4",
            "type": "vertical",
            "l": 7
          },
          {
            "id": "k5",
            "type": "midpoint",
            "p": 9,
            "l": 8
          },
          {
            "id": "k6",
            "type": "horizontal",
            "l": 10
          },
          {
            "id": "k7",
            "type": "horizontal",
            "l": 12
          },
          {
            "id": "k8",
            "type": "vertical",
            "l": 11
          },
          {
            "id": "k9",
            "type": "vertical",
            "l": 13
          },
          {
            "id": "k10",
            "type": "midpoint",
            "p": 14,
            "l": 14
          },
          {
            "id": "k11",
            "type": "horizontal",
            "l": 16
          },
          {
            "id": "k12",
            "type": "horizontal",
            "l": 18
          },
          {
            "id": "k13",
            "type": "vertical",
            "l": 17
          },
          {
            "id": "k14",
            "type": "vertical",
            "l": 19
          },
          {
            "id": "k15",
            "type": "midpoint",
            "p": 19,
            "l": 20
          }
        ],
        "links": [
          {
            "id": "j1",
            "kind": "project",
            "source": {
              "type": "face",
              "body": "Body1",
              "at": [
                -14.166666666666666,
                -32.99999999999997,
                15.333333333333309
              ]
            },
            "face_name": "F4:shell:outer:side:l1",
            "auto_face": true
          }
        ],
        "counters": {
          "j": 1,
          "k": 15,
          "l": 22,
          "p": 19
        }
      }
    },
    {
      "id": 10,
      "name": "Extrude3",
      "suppressed": false,
      "param_names": [
        "d16"
      ],
      "type": "extrude",
      "sketch": 9,
      "profiles": {
        "type": "curves",
        "loops": [
          [
            "l5",
            "l6",
            "l7",
            "l8"
          ],
          [
            "l11",
            "l12",
            "l13",
            "l14"
          ],
          [
            "l17",
            "l18",
            "l19",
            "l20"
          ]
        ]
      },
      "extent": {
        "distance": "-wall",
        "direction": "positive"
      },
      "operation": "cut"
    },
    {
      "id": 11,
      "name": "Sketch4",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "face",
        "plane": {
          "origin": [
            47.49999999999997,
            0.0,
            0.0
          ],
          "x_dir": [
            0.0,
            1.0,
            0.0
          ],
          "y_dir": [
            0.0,
            0.0,
            1.0
          ]
        },
        "at": [
          47.49999999999983,
          -0.00001051725462275499,
          4.000003857314084
        ],
        "name": "F2:side:l2",
        "anchor": {
          "target": {
            "body": "Body1",
            "origin_feature": 4,
            "name": "F4:shell:outer:side:l2",
            "point": [
              47.5,
              0.0,
              0.0
            ],
            "certificate": {
              "kind": "shell_planar",
              "source_producer": 2,
              "source": {
                "kind": "extrude_side",
                "sketch": 1,
                "curve": "l2"
              },
              "inner": false
            }
          },
          "component": 0,
          "path": [],
          "author_path": [],
          "coordinates": [
            0.0,
            0.0
          ],
          "x_axis": [
            1.0,
            0.0
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
            "id": "p1",
            "pos": [
              -28.0,
              22.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              -28.0,
              0.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              28.0,
              0.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p4",
            "pos": [
              28.0,
              22.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p5",
            "pos": [
              -25.0,
              8.0
            ],
            "fixed": false
          },
          {
            "id": "p6",
            "pos": [
              25.0,
              8.0
            ],
            "fixed": false
          },
          {
            "id": "p7",
            "pos": [
              25.0,
              22.0
            ],
            "fixed": false
          },
          {
            "id": "p8",
            "pos": [
              -25.0,
              22.0
            ],
            "fixed": false
          },
          {
            "id": "p9",
            "pos": [
              -0.0,
              15.0
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
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l2",
            "type": "line",
            "a": 2,
            "b": 3,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l3",
            "type": "line",
            "a": 4,
            "b": 1,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l4",
            "type": "line",
            "a": 4,
            "b": 3,
            "construction": false,
            "link": "j1"
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
            "b": 5,
            "construction": false
          },
          {
            "id": "l9",
            "type": "line",
            "a": 5,
            "b": 7,
            "construction": true
          },
          {
            "id": "l10",
            "type": "line",
            "a": 6,
            "b": 8,
            "construction": true
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "horizontal",
            "l": 4
          },
          {
            "id": "k2",
            "type": "horizontal",
            "l": 6
          },
          {
            "id": "k3",
            "type": "vertical",
            "l": 5
          },
          {
            "id": "k4",
            "type": "vertical",
            "l": 7
          },
          {
            "id": "k5",
            "type": "midpoint",
            "p": 9,
            "l": 8
          }
        ],
        "links": [
          {
            "id": "j1",
            "kind": "project",
            "source": {
              "type": "face",
              "body": "Body1",
              "at": [
                47.49999999999997,
                -9.333333333333334,
                15.333333333333329
              ]
            },
            "face_name": "F4:shell:outer:side:l2",
            "auto_face": true
          }
        ],
        "counters": {
          "j": 1,
          "k": 5,
          "l": 10,
          "p": 9
        }
      }
    },
    {
      "id": 12,
      "name": "Extrude4",
      "suppressed": false,
      "param_names": [
        "d17"
      ],
      "type": "extrude",
      "sketch": 11,
      "profiles": {
        "type": "curves",
        "loops": [
          [
            "l5",
            "l6",
            "l7",
            "l8"
          ]
        ]
      },
      "extent": {
        "distance": "-wall",
        "direction": "positive"
      },
      "operation": "cut"
    },
    {
      "id": 13,
      "name": "Sketch5",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "face",
        "plane": {
          "origin": [
            0.0,
            0.0,
            1.9999999999999984
          ],
          "x_dir": [
            1.0,
            0.0,
            0.0
          ],
          "y_dir": [
            0.0,
            1.0,
            0.0
          ]
        },
        "at": [
          -5.00000376507802,
          18.000004375539675,
          2.000000000000057
        ],
        "name": "F4:inner:F2:start#[F4:inner:F2:side:l1,F4:inner:F2:side:l2,F4:inner:F2:side:l3,F4:inner:F2:side:l4,F4:inner:F3:blend:F2:side:l1|F2:side:l2,F4:inner:F3:blend:F2:side:l1|F2:side:l4,F4:inner:F3:blend:F2:side:l2|F2:side:l3,F4:inner:F3:blend:F2:side:l3|F2:side:l4,F7:side:c1]#[F4:inner:F2:side:l1,F4:inner:F2:side:l2,F4:inner:F2:side:l3,F4:inner:F2:side:l4,F4:inner:F3:blend:F2:side:l1|F2:side:l2,F4:inner:F3:blend:F2:side:l1|F2:side:l4,F4:inner:F3:blend:F2:side:l2|F2:side:l3,F4:inner:F3:blend:F2:side:l3|F2:side:l4,F7:side:c1,F7:side:c1@F8.1,F7:side:c1@F8.2,F7:side:c1@F8.3]",
        "anchor": {
          "target": {
            "body": "Body1",
            "origin_feature": 4,
            "name": "F4:shell:inner:cap:low:ed6121d9bed14d3fd103f347231990d103349f33b7874ad292d3c4d3539266c3",
            "point": [
              0.0,
              0.0,
              2.0
            ],
            "certificate": {
              "kind": "shell_planar",
              "source_producer": 2,
              "source": {
                "kind": "extrude_cap",
                "sketch": 1,
                "high": false,
                "outer": [
                  "l1",
                  "l2",
                  "l3",
                  "l4"
                ],
                "holes": []
              },
              "inner": true
            }
          },
          "component": 0,
          "path": [],
          "author_path": [],
          "coordinates": [
            0.0,
            0.0
          ],
          "x_axis": [
            1.0,
            0.0
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
            "id": "p1",
            "pos": [
              -42.49999999999991,
              -27.999999999999872
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              -45.500000000000036,
              -27.99999999999999
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              -42.5,
              -31.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p4",
            "pos": [
              -45.5,
              27.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p5",
            "pos": [
              -42.4999999999997,
              27.999999999999833
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p6",
            "pos": [
              -42.50000000000001,
              31.00000000000013
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p7",
            "pos": [
              42.5,
              -31.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p8",
            "pos": [
              42.5,
              30.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p9",
            "pos": [
              42.50000000000015,
              -28.000000000000092
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p10",
            "pos": [
              45.5,
              -27.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p11",
            "pos": [
              42.499999999999886,
              27.999999999999773
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p12",
            "pos": [
              45.50000000000011,
              28.000000000000004
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p13",
            "pos": [
              -38.999999999999986,
              -24.49999999999994
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p14",
            "pos": [
              -41.99999999999998,
              -24.499999965311186
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p15",
            "pos": [
              -35.99999999999999,
              -24.499999991327797
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p16",
            "pos": [
              -39.00000000000001,
              24.499999999999964
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p17",
            "pos": [
              -42.000000000000014,
              24.500000034688817
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p18",
            "pos": [
              -36.0,
              24.500000008672213
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p19",
            "pos": [
              19.000000000000007,
              -24.500000000000014
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p20",
            "pos": [
              16.00000000000001,
              -24.499999965311183
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p21",
            "pos": [
              22.000000000000004,
              -24.49999999132779
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p22",
            "pos": [
              19.000000000000007,
              24.499999999999986
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p23",
            "pos": [
              16.000000000000018,
              24.500000034688817
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p24",
            "pos": [
              21.999999999999996,
              24.500000008672206
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p25",
            "pos": [
              -40.25,
              -24.500000014453715
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p26",
            "pos": [
              -37.750000000000014,
              -24.50000000361343
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p27",
            "pos": [
              -40.25,
              24.499999985546296
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p28",
            "pos": [
              -37.749999999999915,
              24.49999999638657
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p29",
            "pos": [
              17.750000000000004,
              -24.500000014453708
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p30",
            "pos": [
              20.249999999999947,
              -24.500000003613422
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p31",
            "pos": [
              17.75,
              24.499999985546296
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p32",
            "pos": [
              20.249999999999964,
              24.49999999638658
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p33",
            "pos": [
              -22.0,
              -15.5
            ],
            "fixed": false
          },
          {
            "id": "p34",
            "pos": [
              6.0,
              -15.5
            ],
            "fixed": false
          },
          {
            "id": "p35",
            "pos": [
              6.0,
              -12.5
            ],
            "fixed": false
          },
          {
            "id": "p36",
            "pos": [
              -22.0,
              -12.5
            ],
            "fixed": false
          },
          {
            "id": "p37",
            "pos": [
              -8.0,
              -14.0
            ],
            "fixed": false
          }
        ],
        "curves": [
          {
            "id": "a1",
            "type": "arc",
            "c": 1,
            "a": 2,
            "b": 3,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l1",
            "type": "line",
            "a": 2,
            "b": 4,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a2",
            "type": "arc",
            "c": 5,
            "a": 6,
            "b": 4,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l2",
            "type": "line",
            "a": 7,
            "b": 3,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l3",
            "type": "line",
            "a": 6,
            "b": 8,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a3",
            "type": "arc",
            "c": 9,
            "a": 7,
            "b": 10,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a4",
            "type": "arc",
            "c": 11,
            "a": 12,
            "b": 8,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l4",
            "type": "line",
            "a": 12,
            "b": 10,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a5",
            "type": "arc",
            "c": 13,
            "a": 14,
            "b": 15,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a6",
            "type": "arc",
            "c": 13,
            "a": 15,
            "b": 14,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a7",
            "type": "arc",
            "c": 16,
            "a": 17,
            "b": 18,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a8",
            "type": "arc",
            "c": 16,
            "a": 18,
            "b": 17,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a9",
            "type": "arc",
            "c": 19,
            "a": 20,
            "b": 21,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a10",
            "type": "arc",
            "c": 19,
            "a": 21,
            "b": 20,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a11",
            "type": "arc",
            "c": 22,
            "a": 23,
            "b": 24,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a12",
            "type": "arc",
            "c": 22,
            "a": 24,
            "b": 23,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a13",
            "type": "arc",
            "c": 13,
            "a": 25,
            "b": 26,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a14",
            "type": "arc",
            "c": 13,
            "a": 26,
            "b": 25,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a15",
            "type": "arc",
            "c": 16,
            "a": 27,
            "b": 28,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a16",
            "type": "arc",
            "c": 16,
            "a": 28,
            "b": 27,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a17",
            "type": "arc",
            "c": 19,
            "a": 29,
            "b": 30,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a18",
            "type": "arc",
            "c": 19,
            "a": 30,
            "b": 29,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a19",
            "type": "arc",
            "c": 22,
            "a": 31,
            "b": 32,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a20",
            "type": "arc",
            "c": 22,
            "a": 32,
            "b": 31,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l5",
            "type": "line",
            "a": 33,
            "b": 34,
            "construction": false
          },
          {
            "id": "l6",
            "type": "line",
            "a": 34,
            "b": 35,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 35,
            "b": 36,
            "construction": false
          },
          {
            "id": "l8",
            "type": "line",
            "a": 36,
            "b": 33,
            "construction": false
          },
          {
            "id": "l9",
            "type": "line",
            "a": 33,
            "b": 35,
            "construction": true
          },
          {
            "id": "l10",
            "type": "line",
            "a": 34,
            "b": 36,
            "construction": true
          }
        ],
        "constraints": [
          {
            "id": "k1",
            "type": "horizontal",
            "l": 24
          },
          {
            "id": "k2",
            "type": "horizontal",
            "l": 26
          },
          {
            "id": "k3",
            "type": "vertical",
            "l": 25
          },
          {
            "id": "k4",
            "type": "vertical",
            "l": 27
          },
          {
            "id": "k5",
            "type": "midpoint",
            "p": 37,
            "l": 28
          }
        ],
        "links": [
          {
            "id": "j1",
            "kind": "project",
            "source": {
              "type": "face",
              "body": "Body1",
              "at": [
                43.05005776197402,
                -30.913093669479007,
                2.0
              ]
            },
            "face_name": "F4:shell:inner:cap:low:ed6121d9bed14d3fd103f347231990d103349f33b7874ad292d3c4d3539266c3",
            "auto_face": true
          }
        ],
        "counters": {
          "a": 20,
          "j": 1,
          "k": 5,
          "l": 10,
          "p": 37
        }
      }
    },
    {
      "id": 14,
      "name": "Extrude5",
      "suppressed": false,
      "param_names": [
        "d18"
      ],
      "type": "extrude",
      "sketch": 13,
      "profiles": {
        "type": "curves",
        "loops": [
          [
            "l5",
            "l6",
            "l7",
            "l8"
          ]
        ]
      },
      "extent": {
        "distance": "-wall",
        "direction": "positive"
      },
      "operation": "cut"
    },
    {
      "id": 15,
      "name": "RectangularPattern2",
      "suppressed": false,
      "param_names": [
        "d19",
        "d20"
      ],
      "type": "pattern",
      "features": [
        "Extrude5"
      ],
      "pattern": {
        "type": "rectangular",
        "dir1": [
          0.0,
          1.0,
          0.0
        ],
        "count1": "5",
        "spacing1": "7 mm"
      }
    },
    {
      "id": 16,
      "name": "Sketch6",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "face",
        "plane": {
          "origin": [
            0.0,
            0.0,
            1.9999999999999976
          ],
          "x_dir": [
            1.0,
            0.0,
            0.0
          ],
          "y_dir": [
            0.0,
            1.0,
            0.0
          ]
        },
        "at": [
          -5.000003765077963,
          18.000004375539675,
          2.000000000000057
        ],
        "name": "F4:inner:F2:start#[F4:inner:F2:side:l1,F4:inner:F2:side:l2,F4:inner:F2:side:l3,F4:inner:F2:side:l4,F4:inner:F3:blend:F2:side:l1|F2:side:l2,F4:inner:F3:blend:F2:side:l1|F2:side:l4,F4:inner:F3:blend:F2:side:l2|F2:side:l3,F4:inner:F3:blend:F2:side:l3|F2:side:l4,F7:side:c1]#[F4:inner:F2:side:l1,F4:inner:F2:side:l2,F4:inner:F2:side:l3,F4:inner:F2:side:l4,F4:inner:F3:blend:F2:side:l1|F2:side:l2,F4:inner:F3:blend:F2:side:l1|F2:side:l4,F4:inner:F3:blend:F2:side:l2|F2:side:l3,F4:inner:F3:blend:F2:side:l3|F2:side:l4,F7:side:c1,F7:side:c1@F8.1,F7:side:c1@F8.2,F7:side:c1@F8.3]",
        "anchor": {
          "target": {
            "body": "Body1",
            "origin_feature": 4,
            "name": "F4:shell:inner:cap:low:ed6121d9bed14d3fd103f347231990d103349f33b7874ad292d3c4d3539266c3",
            "point": [
              0.0,
              0.0,
              2.0
            ],
            "certificate": {
              "kind": "shell_planar",
              "source_producer": 2,
              "source": {
                "kind": "extrude_cap",
                "sketch": 1,
                "high": false,
                "outer": [
                  "l1",
                  "l2",
                  "l3",
                  "l4"
                ],
                "holes": []
              },
              "inner": true
            }
          },
          "component": 0,
          "path": [],
          "author_path": [],
          "coordinates": [
            0.0,
            0.0
          ],
          "x_axis": [
            1.0,
            0.0
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
            "id": "p1",
            "pos": [
              -42.49999999999991,
              -27.999999999999872
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              -45.500000000000036,
              -27.99999999999999
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              -42.5,
              -31.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p4",
            "pos": [
              -45.5,
              27.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p5",
            "pos": [
              -42.4999999999997,
              27.999999999999833
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p6",
            "pos": [
              -42.50000000000001,
              31.00000000000013
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p7",
            "pos": [
              42.5,
              -31.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p8",
            "pos": [
              42.5,
              30.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p9",
            "pos": [
              42.50000000000015,
              -28.000000000000092
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p10",
            "pos": [
              45.5,
              -27.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p11",
            "pos": [
              42.499999999999886,
              27.999999999999773
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p12",
            "pos": [
              45.50000000000011,
              28.000000000000004
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p13",
            "pos": [
              -22.0,
              -15.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p14",
            "pos": [
              -22.0,
              -12.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p15",
            "pos": [
              6.0,
              -15.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p16",
            "pos": [
              6.0,
              -12.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p17",
            "pos": [
              -22.0,
              -8.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p18",
            "pos": [
              -22.0,
              -5.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p19",
            "pos": [
              6.0,
              -8.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p20",
            "pos": [
              6.0,
              -5.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p21",
            "pos": [
              -22.0,
              -1.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p22",
            "pos": [
              -22.0,
              1.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p23",
            "pos": [
              6.0,
              -1.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p24",
            "pos": [
              6.0,
              1.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p25",
            "pos": [
              -22.0,
              5.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p26",
            "pos": [
              -22.0,
              8.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p27",
            "pos": [
              6.0,
              5.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p28",
            "pos": [
              6.0,
              8.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p29",
            "pos": [
              -22.0,
              12.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p30",
            "pos": [
              -22.0,
              15.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p31",
            "pos": [
              6.0,
              12.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p32",
            "pos": [
              6.0,
              15.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p33",
            "pos": [
              -38.999999999999986,
              -24.49999999999994
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p34",
            "pos": [
              -41.99999999999998,
              -24.499999965311186
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p35",
            "pos": [
              -35.99999999999999,
              -24.499999991327797
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p36",
            "pos": [
              -39.00000000000001,
              24.499999999999964
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p37",
            "pos": [
              -42.000000000000014,
              24.500000034688817
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p38",
            "pos": [
              -36.0,
              24.500000008672213
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p39",
            "pos": [
              19.000000000000007,
              -24.500000000000014
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p40",
            "pos": [
              16.00000000000001,
              -24.499999965311183
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p41",
            "pos": [
              22.000000000000004,
              -24.49999999132779
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p42",
            "pos": [
              19.000000000000007,
              24.499999999999986
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p43",
            "pos": [
              16.000000000000018,
              24.500000034688817
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p44",
            "pos": [
              21.999999999999996,
              24.500000008672206
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p45",
            "pos": [
              -40.25,
              -24.500000014453715
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p46",
            "pos": [
              -37.750000000000014,
              -24.50000000361343
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p47",
            "pos": [
              -40.25,
              24.499999985546296
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p48",
            "pos": [
              -37.749999999999915,
              24.49999999638657
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p49",
            "pos": [
              17.750000000000004,
              -24.500000014453708
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p50",
            "pos": [
              20.249999999999947,
              -24.500000003613422
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p51",
            "pos": [
              17.75,
              24.499999985546296
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p52",
            "pos": [
              20.249999999999964,
              24.49999999638658
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p101",
            "pos": [
              -8.154474212993971,
              25.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p102",
            "pos": [
              -5.586490288010717,
              25.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p103",
            "pos": [
              -3.828285331547221,
              24.861627595445412
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p104",
            "pos": [
              -4.4411453449430685,
              25.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p105",
            "pos": [
              -3.2154253181513734,
              23.41286671131949
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p106",
            "pos": [
              -3.2154253181513734,
              24.353255190890824
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p107",
            "pos": [
              -3.828285331547221,
              21.96008707300737
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p108",
            "pos": [
              -3.2154253181513734,
              22.468459477561957
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p109",
            "pos": [
              -5.586490288010717,
              21.45171466845278
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p110",
            "pos": [
              -4.4411453449430685,
              21.45171466845278
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p111",
            "pos": [
              -6.607253851306096,
              21.45171466845278
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p112",
            "pos": [
              -6.607253851306096,
              19.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p113",
            "pos": [
              -8.154474212993971,
              19.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p114",
            "pos": [
              -6.607253851306096,
              24.248767582049567
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p115",
            "pos": [
              -6.607253851306096,
              22.572947086403214
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p116",
            "pos": [
              -5.75125920964501,
              22.572947086403214
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p117",
            "pos": [
              -5.056014735432017,
              22.79196918955124
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p118",
            "pos": [
              -5.301158740790355,
              22.572947086403214
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p119",
            "pos": [
              -4.8108707300736775,
              23.41286671131949
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p120",
            "pos": [
              -4.8108707300736775,
              23.010991292699263
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p121",
            "pos": [
              -5.056014735432017,
              24.03175485599464
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p122",
            "pos": [
              -4.8108707300736775,
              23.81474212993972
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p123",
            "pos": [
              -5.75125920964501,
              24.248767582049567
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p124",
            "pos": [
              -5.301158740790355,
              24.248767582049567
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p125",
            "pos": [
              -2.186624246483591,
              23.871004688546552
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p126",
            "pos": [
              -0.7479102478231745,
              23.871004688546552
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p127",
            "pos": [
              -0.7479102478231745,
              19.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p128",
            "pos": [
              -2.186624246483591,
              19.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p129",
            "pos": [
              -2.186624246483591,
              25.623181513730742
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p130",
            "pos": [
              -0.7479102478231745,
              25.623181513730742
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p131",
            "pos": [
              -0.7479102478231745,
              24.449705291359678
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p132",
            "pos": [
              -2.186624246483591,
              24.449705291359678
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p133",
            "pos": [
              5.838827863362358,
              24.096054922973877
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p134",
            "pos": [
              4.142913596784997,
              21.584333556597457
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p135",
            "pos": [
              5.838827863362358,
              21.584333556597457
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p136",
            "pos": [
              5.581627595445411,
              25.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p137",
            "pos": [
              7.301654387139987,
              25.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p138",
            "pos": [
              7.301654387139987,
              21.584333556597457
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p139",
            "pos": [
              8.157649028801071,
              21.584333556597457
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p140",
            "pos": [
              8.157649028801071,
              20.46310113864702
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p141",
            "pos": [
              7.301654387139987,
              20.46310113864702
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p142",
            "pos": [
              7.301654387139987,
              19.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p143",
            "pos": [
              5.838827863362358,
              19.37
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p144",
            "pos": [
              5.838827863362358,
              20.46310113864702
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p145",
            "pos": [
              3.17841259209645,
              20.46310113864702
            ],
            "fixed": true,
            "link": "j2"
          },
          {
            "id": "p146",
            "pos": [
              3.17841259209645,
              21.789290020093773
            ],
            "fixed": true,
            "link": "j2"
          }
        ],
        "curves": [
          {
            "id": "a1",
            "type": "arc",
            "c": 1,
            "a": 2,
            "b": 3,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l1",
            "type": "line",
            "a": 2,
            "b": 4,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a2",
            "type": "arc",
            "c": 5,
            "a": 6,
            "b": 4,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l2",
            "type": "line",
            "a": 7,
            "b": 3,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l3",
            "type": "line",
            "a": 6,
            "b": 8,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a3",
            "type": "arc",
            "c": 9,
            "a": 7,
            "b": 10,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a4",
            "type": "arc",
            "c": 11,
            "a": 12,
            "b": 8,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l4",
            "type": "line",
            "a": 12,
            "b": 10,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l5",
            "type": "line",
            "a": 13,
            "b": 14,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l6",
            "type": "line",
            "a": 15,
            "b": 13,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l7",
            "type": "line",
            "a": 14,
            "b": 16,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l8",
            "type": "line",
            "a": 16,
            "b": 15,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l9",
            "type": "line",
            "a": 17,
            "b": 18,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l10",
            "type": "line",
            "a": 19,
            "b": 17,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l11",
            "type": "line",
            "a": 18,
            "b": 20,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l12",
            "type": "line",
            "a": 20,
            "b": 19,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l13",
            "type": "line",
            "a": 21,
            "b": 22,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l14",
            "type": "line",
            "a": 23,
            "b": 21,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l15",
            "type": "line",
            "a": 22,
            "b": 24,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l16",
            "type": "line",
            "a": 24,
            "b": 23,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l17",
            "type": "line",
            "a": 25,
            "b": 26,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l18",
            "type": "line",
            "a": 27,
            "b": 25,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l19",
            "type": "line",
            "a": 26,
            "b": 28,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l20",
            "type": "line",
            "a": 28,
            "b": 27,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l21",
            "type": "line",
            "a": 29,
            "b": 30,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l22",
            "type": "line",
            "a": 31,
            "b": 29,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l23",
            "type": "line",
            "a": 30,
            "b": 32,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l24",
            "type": "line",
            "a": 32,
            "b": 31,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a5",
            "type": "arc",
            "c": 33,
            "a": 34,
            "b": 35,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a6",
            "type": "arc",
            "c": 33,
            "a": 35,
            "b": 34,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a7",
            "type": "arc",
            "c": 36,
            "a": 37,
            "b": 38,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a8",
            "type": "arc",
            "c": 36,
            "a": 38,
            "b": 37,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a9",
            "type": "arc",
            "c": 39,
            "a": 40,
            "b": 41,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a10",
            "type": "arc",
            "c": 39,
            "a": 41,
            "b": 40,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a11",
            "type": "arc",
            "c": 42,
            "a": 43,
            "b": 44,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a12",
            "type": "arc",
            "c": 42,
            "a": 44,
            "b": 43,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a13",
            "type": "arc",
            "c": 33,
            "a": 45,
            "b": 46,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a14",
            "type": "arc",
            "c": 33,
            "a": 46,
            "b": 45,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a15",
            "type": "arc",
            "c": 36,
            "a": 47,
            "b": 48,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a16",
            "type": "arc",
            "c": 36,
            "a": 48,
            "b": 47,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a17",
            "type": "arc",
            "c": 39,
            "a": 49,
            "b": 50,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a18",
            "type": "arc",
            "c": 39,
            "a": 50,
            "b": 49,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a19",
            "type": "arc",
            "c": 42,
            "a": 51,
            "b": 52,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "a20",
            "type": "arc",
            "c": 42,
            "a": 52,
            "b": 51,
            "construction": false,
            "link": "j1"
          },
          {
            "id": "l57",
            "type": "line",
            "a": 53,
            "b": 54,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "k9",
            "type": "conic",
            "a": 54,
            "b": 55,
            "apex": 56,
            "rho": 0.5,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "k10",
            "type": "conic",
            "a": 55,
            "b": 57,
            "apex": 58,
            "rho": 0.5,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "k11",
            "type": "conic",
            "a": 57,
            "b": 59,
            "apex": 60,
            "rho": 0.5,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "k12",
            "type": "conic",
            "a": 59,
            "b": 61,
            "apex": 62,
            "rho": 0.5,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l58",
            "type": "line",
            "a": 61,
            "b": 63,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l59",
            "type": "line",
            "a": 63,
            "b": 64,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l60",
            "type": "line",
            "a": 64,
            "b": 65,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l61",
            "type": "line",
            "a": 65,
            "b": 53,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l62",
            "type": "line",
            "a": 66,
            "b": 67,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l63",
            "type": "line",
            "a": 67,
            "b": 68,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "k13",
            "type": "conic",
            "a": 68,
            "b": 69,
            "apex": 70,
            "rho": 0.5,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "k14",
            "type": "conic",
            "a": 69,
            "b": 71,
            "apex": 72,
            "rho": 0.5,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "k15",
            "type": "conic",
            "a": 71,
            "b": 73,
            "apex": 74,
            "rho": 0.5,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "k16",
            "type": "conic",
            "a": 73,
            "b": 75,
            "apex": 76,
            "rho": 0.5,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l64",
            "type": "line",
            "a": 75,
            "b": 66,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l65",
            "type": "line",
            "a": 77,
            "b": 78,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l66",
            "type": "line",
            "a": 78,
            "b": 79,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l67",
            "type": "line",
            "a": 79,
            "b": 80,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l68",
            "type": "line",
            "a": 80,
            "b": 77,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l69",
            "type": "line",
            "a": 81,
            "b": 82,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l70",
            "type": "line",
            "a": 82,
            "b": 83,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l71",
            "type": "line",
            "a": 83,
            "b": 84,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l72",
            "type": "line",
            "a": 84,
            "b": 81,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l73",
            "type": "line",
            "a": 85,
            "b": 86,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l74",
            "type": "line",
            "a": 86,
            "b": 87,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l75",
            "type": "line",
            "a": 87,
            "b": 85,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l76",
            "type": "line",
            "a": 88,
            "b": 89,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l77",
            "type": "line",
            "a": 89,
            "b": 90,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l78",
            "type": "line",
            "a": 90,
            "b": 91,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l79",
            "type": "line",
            "a": 91,
            "b": 92,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l80",
            "type": "line",
            "a": 92,
            "b": 93,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l81",
            "type": "line",
            "a": 93,
            "b": 94,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l82",
            "type": "line",
            "a": 94,
            "b": 95,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l83",
            "type": "line",
            "a": 95,
            "b": 96,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l84",
            "type": "line",
            "a": 96,
            "b": 97,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l85",
            "type": "line",
            "a": 97,
            "b": 98,
            "construction": false,
            "link": "j2"
          },
          {
            "id": "l86",
            "type": "line",
            "a": 98,
            "b": 88,
            "construction": false,
            "link": "j2"
          }
        ],
        "constraints": [],
        "links": [
          {
            "id": "j1",
            "kind": "project",
            "source": {
              "type": "face",
              "body": "Body1",
              "at": [
                43.05005776197402,
                -30.913093669479007,
                2.0
              ]
            },
            "face_name": "F4:shell:inner:cap:low:ed6121d9bed14d3fd103f347231990d103349f33b7874ad292d3c4d3539266c3",
            "auto_face": true
          },
          {
            "id": "j2",
            "kind": "text",
            "source": {
              "type": "text",
              "text": "Pi 4",
              "at": [
                -8.91,
                19.37
              ],
              "height": 6.0,
              "angle": 0.0,
              "font": "DejaVu Sans Bold"
            }
          }
        ],
        "counters": {
          "a": 20,
          "j": 2,
          "k": 16,
          "l": 86,
          "p": 146
        }
      }
    },
    {
      "id": 17,
      "name": "Extrude6",
      "suppressed": false,
      "param_names": [
        "d21"
      ],
      "type": "extrude",
      "sketch": 16,
      "profiles": {
        "type": "curves",
        "loops": [
          [
            "l57",
            "l61",
            "l60",
            "l59",
            "l58",
            "k12",
            "k11",
            "k10",
            "k9"
          ],
          [
            "l65",
            "l68",
            "l67",
            "l66"
          ],
          [
            "l69",
            "l72",
            "l71",
            "l70"
          ],
          [
            "l76",
            "l86",
            "l85",
            "l84",
            "l83",
            "l82",
            "l81",
            "l80",
            "l79",
            "l78",
            "l77"
          ]
        ]
      },
      "extent": {
        "distance": "-0.6 mm",
        "direction": "positive"
      },
      "operation": "cut"
    }
  ],
  "next_id": 18,
  "materials": {
    "Body1": "PETG"
  },
  "user_materials": [
    {
      "name": "PETG",
      "density": 1.27,
      "appearance": {
        "name": "PETG",
        "color": [
          42,
          134,
          150
        ],
        "opacity": 1.0
      }
    }
  ]
}