{
  "format": "solidcraft/64",
  "name": "sketch-offsets",
  "units": "mm",
  "params": [
    {
      "name": "gap",
      "expr": "1 mm",
      "unit": "mm",
      "model": false
    },
    {
      "name": "d1",
      "expr": "gap",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d2",
      "expr": "gap",
      "unit": "mm",
      "model": true
    },
    {
      "name": "d3",
      "expr": "gap",
      "unit": "mm",
      "model": true
    }
  ],
  "features": [
    {
      "id": 1,
      "name": "Arch",
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
            "pos": [0.0, 0.0],
            "fixed": true
          },
          {
            "id": "p1",
            "pos": [-10.0, -20.0],
            "fixed": false
          },
          {
            "id": "p2",
            "pos": [10.0, -20.0],
            "fixed": false
          },
          {
            "id": "p3",
            "pos": [10.0, 0.0],
            "fixed": false
          },
          {
            "id": "p4",
            "pos": [-10.0, 0.0],
            "fixed": false
          },
          {
            "id": "a1.center",
            "pos": [0.0, -4.9645161290322575],
            "fixed": false
          },
          {
            "id": "a1.start",
            "pos": [10.0, 0.0],
            "fixed": false
          },
          {
            "id": "a1.end",
            "pos": [-10.0, 0.0],
            "fixed": false
          },
          {
            "id": "p5",
            "pos": [9.104305114128865, -0.4446691707598962],
            "fixed": true
          },
          {
            "id": "p6",
            "pos": [-9.104305114128865, -0.4446691707598962],
            "fixed": true
          },
          {
            "id": "p7",
            "pos": [-9.0, 0.0],
            "fixed": true
          },
          {
            "id": "p8",
            "pos": [-9.0, -19.0],
            "fixed": true
          },
          {
            "id": "p9",
            "pos": [9.0, -19.0],
            "fixed": true
          },
          {
            "id": "p10",
            "pos": [9.0, 0.0],
            "fixed": true
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
            "id": "a1",
            "type": "arc",
            "c": 5,
            "a": 6,
            "b": 7,
            "construction": false
          },
          {
            "id": "a2",
            "type": "arc",
            "c": 5,
            "a": 8,
            "b": 9,
            "construction": false
          },
          {
            "id": "l5",
            "type": "line",
            "a": 10,
            "b": 11,
            "construction": false
          },
          {
            "id": "l6",
            "type": "line",
            "a": 11,
            "b": 12,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 12,
            "b": 13,
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
          },
          {
            "id": "k5",
            "type": "coincident",
            "p": 3,
            "q": 6
          },
          {
            "id": "k6",
            "type": "coincident",
            "p": 4,
            "q": 7
          },
          {
            "id": "k7",
            "type": "offset_distance",
            "output": "a2",
            "value": 1.0,
            "reverse": false,
            "param": "d1"
          },
          {
            "id": "k8",
            "type": "parallel",
            "a": 3,
            "b": 6
          },
          {
            "id": "k9",
            "type": "parallel",
            "a": 0,
            "b": 7
          },
          {
            "id": "k10",
            "type": "parallel",
            "a": 1,
            "b": 8
          },
          {
            "id": "k11",
            "type": "offset_distance",
            "output": "l5",
            "value": 1.0,
            "reverse": false,
            "param": "d1"
          }
        ],
        "offsets": [
          {
            "sources": [
              [
                "a1",
                true
              ]
            ],
            "outputs": [
              "a2"
            ],
            "distance": 1.0,
            "closed": false
          },
          {
            "sources": [
              [
                "l4",
                true
              ],
              [
                "l1",
                true
              ],
              [
                "l2",
                true
              ]
            ],
            "outputs": [
              "l5",
              "l6",
              "l7"
            ],
            "distance": 1.0,
            "closed": false
          }
        ],
        "counters": {
          "a": 2,
          "k": 11,
          "l": 7,
          "p": 10
        }
      }
    },
    {
      "id": 2,
      "name": "Sides",
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
            "pos": [0.0, 0.0],
            "fixed": true
          },
          {
            "id": "p1",
            "pos": [30.0, -20.0],
            "fixed": false
          },
          {
            "id": "p2",
            "pos": [50.0, -20.0],
            "fixed": false
          },
          {
            "id": "p3",
            "pos": [50.0, 0.0],
            "fixed": false
          },
          {
            "id": "p4",
            "pos": [30.0, 0.0],
            "fixed": false
          },
          {
            "id": "a1.center",
            "pos": [40.0, -4.964516129032246],
            "fixed": false
          },
          {
            "id": "a1.start",
            "pos": [50.0, 0.0],
            "fixed": false
          },
          {
            "id": "a1.end",
            "pos": [30.0, 0.0],
            "fixed": false
          },
          {
            "id": "p5",
            "pos": [31.0, 0.0],
            "fixed": true
          },
          {
            "id": "p6",
            "pos": [31.0, -19.0],
            "fixed": true
          },
          {
            "id": "p7",
            "pos": [49.0, -19.0],
            "fixed": true
          },
          {
            "id": "p8",
            "pos": [49.0, 0.0],
            "fixed": true
          },
          {
            "id": "p9",
            "pos": [50.0, -1.0],
            "fixed": true
          },
          {
            "id": "p10",
            "pos": [30.0, -1.0],
            "fixed": true
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
            "id": "a1",
            "type": "arc",
            "c": 5,
            "a": 6,
            "b": 7,
            "construction": false
          },
          {
            "id": "l5",
            "type": "line",
            "a": 8,
            "b": 9,
            "construction": false
          },
          {
            "id": "l6",
            "type": "line",
            "a": 9,
            "b": 10,
            "construction": false
          },
          {
            "id": "l7",
            "type": "line",
            "a": 10,
            "b": 11,
            "construction": false
          },
          {
            "id": "l8",
            "type": "line",
            "a": 12,
            "b": 13,
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
          },
          {
            "id": "k5",
            "type": "coincident",
            "p": 3,
            "q": 6
          },
          {
            "id": "k6",
            "type": "coincident",
            "p": 4,
            "q": 7
          },
          {
            "id": "k7",
            "type": "parallel",
            "a": 3,
            "b": 5
          },
          {
            "id": "k8",
            "type": "parallel",
            "a": 0,
            "b": 6
          },
          {
            "id": "k9",
            "type": "parallel",
            "a": 1,
            "b": 7
          },
          {
            "id": "k10",
            "type": "offset_distance",
            "output": "l5",
            "value": 1.0,
            "reverse": false,
            "param": "d2"
          },
          {
            "id": "k11",
            "type": "parallel",
            "a": 2,
            "b": 8
          },
          {
            "id": "k12",
            "type": "offset_distance",
            "output": "l8",
            "value": 1.0,
            "reverse": false,
            "param": "d2"
          }
        ],
        "offsets": [
          {
            "sources": [
              [
                "l4",
                true
              ],
              [
                "l1",
                true
              ],
              [
                "l2",
                true
              ]
            ],
            "outputs": [
              "l5",
              "l6",
              "l7"
            ],
            "distance": 1.0,
            "closed": false
          },
          {
            "sources": [
              [
                "l3",
                true
              ]
            ],
            "outputs": [
              "l8"
            ],
            "distance": 1.0,
            "closed": false
          }
        ],
        "counters": {
          "a": 1,
          "k": 12,
          "l": 8,
          "p": 10
        }
      }
    },
    {
      "id": 3,
      "name": "Box",
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
            "pos": [0.0, 0.0],
            "fixed": true
          },
          {
            "id": "p1",
            "pos": [70.0, -20.0],
            "fixed": false
          },
          {
            "id": "p2",
            "pos": [90.0, -20.0],
            "fixed": false
          },
          {
            "id": "p3",
            "pos": [90.0, 0.0],
            "fixed": false
          },
          {
            "id": "p4",
            "pos": [70.0, 0.0],
            "fixed": false
          },
          {
            "id": "p5",
            "pos": [71.0, -19.0],
            "fixed": true
          },
          {
            "id": "p6",
            "pos": [89.0, -19.0],
            "fixed": true
          },
          {
            "id": "p7",
            "pos": [89.0, -1.0],
            "fixed": true
          },
          {
            "id": "p8",
            "pos": [71.0, -1.0],
            "fixed": true
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
            "type": "parallel",
            "a": 0,
            "b": 4
          },
          {
            "id": "k6",
            "type": "parallel",
            "a": 1,
            "b": 5
          },
          {
            "id": "k7",
            "type": "parallel",
            "a": 2,
            "b": 6
          },
          {
            "id": "k8",
            "type": "parallel",
            "a": 3,
            "b": 7
          },
          {
            "id": "k9",
            "type": "offset_distance",
            "output": "l5",
            "value": 1.0,
            "reverse": false,
            "param": "d3"
          }
        ],
        "offsets": [
          {
            "sources": [
              [
                "l1",
                true
              ],
              [
                "l2",
                true
              ],
              [
                "l3",
                true
              ],
              [
                "l4",
                true
              ]
            ],
            "outputs": [
              "l5",
              "l6",
              "l7",
              "l8"
            ],
            "distance": 1.0,
            "closed": true
          }
        ],
        "counters": {
          "k": 9,
          "l": 8,
          "p": 8
        }
      }
    },
    {
      "id": 4,
      "name": "Frame",
      "suppressed": false,
      "body_names": [
        "Frame"
      ],
      "param_names": [
        "d4"
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
            "l7",
            "l7",
            "l6",
            "l5",
            "l5",
            "l3",
            "l4"
          ]
        ]
      },
      "extent": {
        "distance": "10",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "new_body"
    },
    {
      "id": 5,
      "name": "Cap",
      "suppressed": false,
      "body_names": [
        "Cap"
      ],
      "param_names": [
        "d5"
      ],
      "type": "extrude",
      "sketch": 1,
      "profiles": {
        "type": "curves",
        "loops": [
          [
            "l3",
            "a2"
          ]
        ]
      },
      "extent": {
        "distance": "10",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "new_body"
    },
    {
      "id": 6,
      "name": "Ring",
      "suppressed": false,
      "body_names": [
        "Ring"
      ],
      "param_names": [
        "d6"
      ],
      "type": "extrude",
      "sketch": 3,
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
        "distance": "10",
        "convention": "total_distance",
        "direction": "positive"
      },
      "operation": "new_body"
    },
    {
      "id": 7,
      "name": "Round",
      "suppressed": false,
      "param_names": [
        "d7"
      ],
      "edge_refs": [
        {
          "dir": [0.0, 0.0, 1.0],
          "min": [-10.0, -20.0, 0.0],
          "max": [10.0, 0.0, 10.0]
        }
      ],
      "edge_names": [
        "F4:side:l5|F4:side:l6"
      ],
      "type": "fillet",
      "edges": [
        [-9.0, -19.0, 5.0]
      ],
      "radius": "0.5",
      "body": "Frame"
    }
  ],
  "next_id": 8,
  "next_component_color_index": 0
}