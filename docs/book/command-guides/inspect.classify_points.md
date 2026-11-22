Check whether points lie in a body's material, in empty space, or on its boundary.
Boundary points also give a persistent face name and the outward unit normal there.
This read command is available to plugins, scripts and MCP's `execute` tool.

#### Before you start

Use an exact body and its world-space name, as listed by {{#command model.faces}}.
Coordinates are in millimetres in the world frame, including for moved components
and placed copies. Mesh bodies are refused.

#### Parameters

| Parameter | Meaning |
| --- | --- |
| `body` | The body's name. |
| `points` | 1–256 `[x,y,z]` points; coordinates must be finite and below 1e9 in magnitude. |
| `face` | Optional face name from `model.faces`, or a point on the face. Chooses the normal's face for all boundary points. |

#### Example

For a 10 mm cube starting at the origin, call:

```json
{"body":"Block","points":[[5,5,5],[20,5,5],[0,5,5]]}
```

The `results` array contains `inside`, `outside`, and `on` classes in that order.
The `on` result includes the face's persistent name and normal `[-1,0,0]`.
Points within the body's modelling tolerance of its trimmed boundary count as `on`.
A hole or cavity is outside material. Its wall's normal points into the empty space;
a cylindrical boss's normal points away from its axis.

#### Tips

- At an edge or vertex, select `face` to choose one incident face's normal. Without
  it, the first containing face in `model.faces` order supplies the normal.
- A selected face must contain every `on` point. A point on the face's extended
  surface, beyond its trim or inside one of its holes, does not lie on that face.
- Split requests when boundary points need normals from different selected faces.
- Invalid inputs, unsupported mesh bodies and undefined normals return an error.
  The command leaves the design and undo history unchanged.

#### See also

{{#command model.faces}} lists names and sample points for faces.
{{#command inspect.measure}} reads sizes and distances.
