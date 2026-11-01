{
  "format": "solidcraft/15",
  "name": "pi-case",
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
      "name": "d6",
      "expr": "board_w / 2 - 3.5 mm",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d7",
      "expr": "board_d / 2 - 3.5 mm",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d8",
      "expr": "6 mm",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d9",
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
          9.822363153944025e-7,
          -4.162040681876533e-6,
          23.999999999999943
        ]
      ],
      "thickness": "wall",
      "tangent_chain": true
    },
    {
      "id": 5,
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
          -5.000003765004408,
          18.00000437526785,
          2.0
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
            "param": "d6"
          },
          {
            "id": "k2",
            "type": "distance_y",
            "p": 0,
            "q": 13,
            "value": 24.5,
            "param": "d7"
          },
          {
            "id": "k3",
            "type": "diameter",
            "c": 8,
            "value": 6.0,
            "param": "d8"
          },
          {
            "id": "k4",
            "type": "diameter",
            "c": 9,
            "value": 2.5,
            "param": "d9"
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
      "id": 6,
      "name": "Extrude2",
      "suppressed": false,
      "param_names": [
        "d10"
      ],
      "type": "extrude",
      "sketch": 5,
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
      "id": 7,
      "name": "RectangularPattern1",
      "suppressed": false,
      "param_names": [
        "d11",
        "d12",
        "d13",
        "d14"
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
      "id": 8,
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
          15.999996572036139
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
              24.0
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
              24.0
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
                16.0
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
      "id": 9,
      "name": "Extrude3",
      "suppressed": false,
      "param_names": [
        "d15"
      ],
      "type": "extrude",
      "sketch": 8,
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
      "id": 10,
      "name": "Sketch4",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "face",
        "plane": {
          "origin": [
            47.49999999999911,
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
          47.49999999999966,
          -0.000010517254452224734,
          4.000003857313914
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
              -27.99999999999999,
              1.1077630395475666e-11
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              -27.999999999998284,
              23.999999999994547
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              27.999999999999996,
              7.101605498946572e-12
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p4",
            "pos": [
              28.000000000001705,
              23.99999999999057
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
            "a": 3,
            "b": 4,
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
                47.49999999999992,
                9.333333333333382,
                0.6666666666727387
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
      "id": 11,
      "name": "Extrude4",
      "suppressed": false,
      "param_names": [
        "d16"
      ],
      "type": "extrude",
      "sketch": 10,
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
      "id": 12,
      "name": "Sketch5",
      "suppressed": false,
      "type": "sketch",
      "plane": {
        "type": "face",
        "plane": {
          "origin": [
            0.0,
            0.0,
            1.999999999998668
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
          -5.000003765081658,
          18.00000437554013,
          1.9999999999969873
        ],
        "name": "F4:inner:F2:start#[F4:inner:F2:side:l1,F4:inner:F2:side:l2,F4:inner:F2:side:l3,F4:inner:F2:side:l4,F4:inner:F3:blend:F2:side:l1|F2:side:l2,F4:inner:F3:blend:F2:side:l1|F2:side:l4,F4:inner:F3:blend:F2:side:l2|F2:side:l3,F4:inner:F3:blend:F2:side:l3|F2:side:l4,F6:side:c1,F6:side:c1@F7.1,F6:side:c1@F7.2,F6:side:c1@F7.3]",
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
              -42.50000000000019,
              -28.000000000000266
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p2",
            "pos": [
              -45.4999999999999,
              -27.999999999999993
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p3",
            "pos": [
              -42.500000000000014,
              -30.999999999999975
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p4",
            "pos": [
              -45.5,
              28.0
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p5",
            "pos": [
              -42.50000000000011,
              28.00000000000006
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p6",
            "pos": [
              -42.50000000000001,
              30.999999999999954
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p7",
            "pos": [
              42.5,
              -31.000000000000014
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p8",
            "pos": [
              42.5,
              30.999999999999986
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p9",
            "pos": [
              42.500000000000064,
              -28.000000000000075
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p10",
            "pos": [
              45.50000000000002,
              -27.999999999999986
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p11",
            "pos": [
              42.49999999999982,
              27.999999999999932
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p12",
            "pos": [
              45.49999999999987,
              28.00000000000001
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p13",
            "pos": [
              -39.00000000000002,
              -24.5
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p14",
            "pos": [
              -42.0,
              -24.499999966525618
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p15",
            "pos": [
              -36.00000000000004,
              -24.49999999011337
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p16",
            "pos": [
              -39.00000000000003,
              24.500000000000018
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p17",
            "pos": [
              -42.0,
              24.500000033474386
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p18",
            "pos": [
              -36.00000000000006,
              24.500000009886637
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p19",
            "pos": [
              18.999999999999996,
              -24.50000000000003
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p20",
            "pos": [
              16.0,
              -24.499999966525625
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p21",
            "pos": [
              21.999999999999993,
              -24.499999990113363
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p22",
            "pos": [
              19.00000000000001,
              24.499999999999986
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p23",
            "pos": [
              16.00000000000001,
              24.500000033474382
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p24",
            "pos": [
              22.00000000000001,
              24.500000009886644
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p25",
            "pos": [
              -40.24999999999997,
              -24.50000001394769
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p26",
            "pos": [
              -37.75,
              -24.500000004119443
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p27",
            "pos": [
              -40.249999999999744,
              24.499999986052313
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p28",
            "pos": [
              -37.75,
              24.49999999588055
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p29",
            "pos": [
              17.750000000000092,
              -24.500000013947687
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p30",
            "pos": [
              20.249999999999993,
              -24.500000004119435
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p31",
            "pos": [
              17.750000000000025,
              24.499999986052313
            ],
            "fixed": true,
            "link": "j1"
          },
          {
            "id": "p32",
            "pos": [
              20.249999999999982,
              24.49999999588056
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
                -38.58333333529897,
                -24.000000005227474,
                1.9999999999971347
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
      "id": 13,
      "name": "Extrude5",
      "suppressed": false,
      "param_names": [
        "d17"
      ],
      "type": "extrude",
      "sketch": 12,
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
      "id": 14,
      "name": "RectangularPattern2",
      "suppressed": false,
      "param_names": [
        "d18",
        "d19"
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
    }
  ],
  "next_id": 15,
  "appearances": {
    "bodies": {
      "Body1": {
        "name": "ABS - Red",
        "color": [
          196,
          40,
          40
        ],
        "opacity": 1.0
      }
    }
  }
}