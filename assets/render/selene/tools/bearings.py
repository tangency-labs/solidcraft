#!/usr/bin/env python3
"""Native SELENE bearing and arm-clearance authoring commands.

All geometry and assembly edits go through SolidCraft commands. The ten bearings
are intentionally inspectable open bearings, not catalogue-certified parts. Their
grooves have 0.02 mm geometric clearance and the rolling relation is an ideal
zero-contact-angle, fixed-outer-ring relation; it does not simulate contact loads,
lubrication, friction or slip. Distances are millimetres, command angles degrees.

``stages()`` supplies safe native save/reopen boundaries. Each stage activates the
root or its first target component explicitly. Creating all 110 new components
in one ``component.from_bodies`` operation avoids repeated global reevaluations.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path


AXIS = [0, 1, 0]
JOINTS = (
    ("Shoulder", [177, 0, 165], "Arm turret", "Upper arm", "Shoulder articulation", 11.0),
    ("Elbow", [242, 0, 281], "Upper arm", "Forearm", "Elbow articulation", 7.0),
    ("Wrist", [350, 0, 249], "Forearm", "Distal arm", "Wrist articulation", 11.2),
    ("Tool tilt", [410, 0, 267], "Distal arm", "Gripper tool", "Tool tilt articulation", 7.0),
    ("Turret tilt", [-45, 0, 249], "Turret yaw", "Turret sensor head", "Turret elevation articulation", 50.0),
)
HYDRAULIC_SUFFIXES = (
    "gold cylinder", "chrome piston", "barrel eye", "barrel eye pin",
    "barrel eye fork", "rod eye", "rod eye pin", "rod eye fork",
    "flexible hose", "hose union A", "hose union B",
)


def cmd(command: str, **params) -> dict:
    return {"command": command, "params": params}


def activate(name: str = "root") -> dict:
    return cmd("component.activate", component=name)


def cylinder(name, base, radius, height, *, axis=AXIS, operation="new", targets=None):
    p = dict(base=base, radius=radius, height=height, axis=axis,
             operation=operation, name=name)
    if operation == "new":
        p["body_name"] = name
    if targets:
        p["targets"] = targets
    return cmd("solid.cylinder", **p)


def box(name, corner, size, *, operation="new", targets=None):
    p = dict(corner=corner, length=size[0], width=size[1], height=size[2],
             operation=operation, name=name)
    if operation == "new":
        p["body_name"] = name
    if targets:
        p["targets"] = targets
    return cmd("solid.box", **p)


def ring(name, center, y0, width, ro, ri):
    x, _, z = center
    return [cylinder(name, [x, y0, z], ro, width),
            cylinder(name + " bore", [x, y0-1, z], ri, width+2,
                     operation="cut", targets=[name])]


def style(names, material="Steel", color=(185, 193, 201), roughness=.26):
    return [cmd("material.assign", bodies=names, material=material),
            cmd("appearance.assign", bodies=names, color=list(color),
                pbr={"metallic": 1, "roughness": roughness, "dielectric_ior": 1.5})]


def bearing_specs():
    """The actual native names, frames and analytic ratios used by validation."""
    result = []
    for label, center, parent, child, driver, y in JOINTS:
        ro, r = (5.0, .44) if label == "Turret tilt" else (6.0, .70)
        pitch = (ro + 3.05) / 2
        for side, sign in (("port", -1), ("starboard", 1)):
            name = f"Bearing {label} {side}"
            result.append(dict(name=name, center=[center[0], sign*y, center[2]],
                               parent=parent, child=child, driver=driver,
                               outer_radius=ro, bore_radius=3.05, width=4.0,
                               ball_radius=r, pitch_radius=pitch, balls=8,
                               groove_clearance=.02,
                               cage_joint=name + " cage orbit",
                               cage_ratio=.5*(1-r/pitch),
                               ball_relative_spin_ratio=-.5*(pitch/r-r/pitch)))
    return result


def race(name, center, ri, ro, pitch, ball_r, inner):
    """Revolve a fixed closed cross-section, including an exact circular groove."""
    chamfer, half, groove = .06, 2.0, ball_r+.02
    shoulder = .85*ball_r
    h = math.sqrt(groove*groove-shoulder*shoulder)
    land = pitch-shoulder if inner else pitch+shoulder
    if inner:
        first = [[ri+chamfer,-half], [land-chamfer,-half],
                 [land,-half+chamfer], [land,-h]]
        after = ["a1.end", [land,half-chamfer], [land-chamfer,half],
                 [ri+chamfer,half], [ri,half-chamfer], [ri,-half+chamfer],
                 "l1.start"]
        arc = dict(start="l3.end", end=[land,h], through=[pitch-groove,0])
    else:
        first = [[land,-h], [land,-half+chamfer], [land+chamfer,-half],
                 [ro-chamfer,-half], [ro,-half+chamfer], [ro,half-chamfer],
                 [ro-chamfer,half], [land+chamfer,half], [land,half-chamfer],
                 [land,h]]
        after = None
        arc = dict(start="l9.end", end="l1.start", through=[pitch+groove,0])
    out = [cmd("sketch.create", name=name + " section",
               plane={"origin":center, "x_dir":[1,0,0], "y_dir":[0,1,0]}),
           cmd("sketch.line", points=first, infer=False),
           cmd("sketch.arc.three_point", **arc)]
    if after:
        out.append(cmd("sketch.line", points=after, infer=False))
    out += [cmd("sketch.constraint.fix", entities=[f"l{i}" for i in range(1,10)]+["a1"], fixed=True),
            cmd("sketch.finish"),
            cmd("solid.revolve", sketch=name+" section", axis="y", angle=360,
                operation="new", name=name, body_name=name)]
    return out


def bearing_geometry(spec):
    n, c = spec["name"], spec["center"]
    r, pitch, ro = spec["ball_radius"], spec["pitch_radius"], spec["outer_radius"]
    out = race(n+" inner race", c, 3.05, ro, pitch, r, True)
    out += race(n+" outer race", c, 3.05, ro, pitch, r, False)
    rail_inner = r+.10
    out += ring(n+" cage", c, c[1]-rail_inner-.15, .15, pitch+.10, pitch-.10)
    out += ring(n+" cage rail +1", c, c[1]+rail_inner, .15, pitch+.10, pitch-.10)
    cage = [n+" cage", n+" cage rail +1"]
    balls = []
    for i in range(8):
        a = i*math.tau/8
        b = a+math.pi/8
        name = n+f" ball {i+1:02d}"
        balls.append(name)
        out.append(cmd("solid.sphere", name=name, body_name=name, operation="new", radius=r,
                       center=[c[0]+pitch*math.cos(a), c[1], c[2]+pitch*math.sin(a)]))
        bridge = n+f" cage bridge {i+1:02d}"
        cage.append(bridge)
        out.append(cylinder(bridge,
                            [c[0]+pitch*math.cos(b),c[1]-rail_inner,c[2]+pitch*math.sin(b)], .15, 2*rail_inner))
    out += style([n+" inner race", n+" outer race"], roughness=.23)
    out += style(balls, color=(209,215,220), roughness=.14)
    out += style(cage, "Brass", (186,147,67), .35)
    return out


def bearing_assembly(specs):
    out = [activate()]
    anchors = []
    for s in specs:
        n = s["name"]
        anchors += [n+" inner race", n+" outer race", n+" cage"]
        anchors += [n+f" ball {i+1:02d}" for i in range(8)]
    out.append(cmd("component.from_bodies", bodies=anchors))
    for s in specs:
        n, c = s["name"], s["center"]
        cage_parts = [n+" cage rail +1"]+[n+f" cage bridge {i+1:02d}" for i in range(8)]
        out.append(cmd("component.move_bodies", bodies=cage_parts, component=n+" cage"))
        for race_kind, parent in (("outer",s["parent"]),("inner",s["child"])):
            out.append(cmd("joint.as_built", type="rigid", a=parent, b=n+f" {race_kind} race",
                           name=n+f" {race_kind} seat", quiet=True))
        out.append(cmd("joint.as_built", type="revolute", a=n+" outer race", b=n+" cage",
                       at={"occurrence":n+" outer race","point":c,"z":AXIS},
                       name=s["cage_joint"], quiet=True))
        out.append(cmd("joint.motion_link", a=s["driver"], b=s["cage_joint"],
                       ratio=s["cage_ratio"], offset=0, quiet=True))
        for i in range(8):
            name = n+f" ball {i+1:02d}"
            a = i*math.tau/8
            p = [c[0]+s["pitch_radius"]*math.cos(a),c[1],c[2]+s["pitch_radius"]*math.sin(a)]
            out.append(cmd("joint.as_built", type="revolute", a=n+" cage", b=name,
                           at={"occurrence":n+" cage","point":p,"z":AXIS},
                           name=name+" spin", quiet=True))
            out.append(cmd("joint.motion_link", a=s["driver"], b=name+" spin",
                           ratio=s["ball_relative_spin_ratio"], offset=0, quiet=True))
    return out


def clamp_hub(name, center, plate_y, inward):
    """Split adapter for a 12.2 mm link bore and a 6 mm driven shaft."""
    x, _, z = center
    # Revolve the stepped section in one operation. Joining two tangent annuli
    # instead makes a needlessly difficult coincident curved-face boolean.
    profile = ([[3.025,0],[6.075,0],[6.075,4],[7.2,4],[7.2,4.6],[3.025,4.6]]
               if inward == 1 else
               [[3.025,-.6],[7.2,-.6],[7.2,0],[6.075,0],[6.075,4],[3.025,4]])
    out = [cmd("sketch.create", name=name+" section",
               plane={"origin":[x,plate_y,z],"x_dir":[1,0,0],"y_dir":[0,1,0]}),
           cmd("sketch.line", points=profile, closed=True, infer=False),
           cmd("sketch.constraint.fix", entities=[f"l{i}" for i in range(1,7)], fixed=True),
           cmd("sketch.finish"),
           cmd("solid.revolve", sketch=name+" section", axis="y", angle=360,
               operation="new",name=name,body_name=name)]
    out.append(box(name+" split", [x+2.8,plate_y-1,z-.16], [6,6,.32],
                   operation="cut", targets=[name]))
    # Mill the screw seating flat before drilling: avoids a curved counterbore.
    out.append(box(name+" pinch head seat", [x+3.2,plate_y+.65,z-6], [3.3,2.7,2.5],
                   operation="cut", targets=[name]))
    out.append(cylinder(name+" pinch clearance", [x+4.7,plate_y+2,z-5], .65,10,
                        axis=[0,0,1], operation="cut", targets=[name]))
    shank, head = name+" pinch screw", name+" pinch screw head"
    out.append(cylinder(shank, [x+4.7,plate_y+2,z-3.5], .6,7, axis=[0,0,1]))
    out.append(cylinder(head, [x+4.7,plate_y+2,z-4.6], 1.15,1.1, axis=[0,0,1]))
    out.append(cylinder(head+" socket", [x+4.7,plate_y+2,z-4.7], .52,.75,
                        axis=[0,0,1], operation="cut", targets=[head]))
    out += style([name], "Aluminum", (170,181,191), .34)
    out += style([shank,head], color=(64,69,75), roughness=.37)
    return out, [name,shank,head]


def arm_clearance_stages():
    out = [activate()]
    for _,_,_,_,driver,_ in JOINTS:
        out.append(cmd("joint.drive", joint=driver, value=0, quiet=True))
    # Remove decorative cylinders that cannot telescope with the electric joints.
    for label, component in (("Shoulder","Upper arm"),("Elbow","Forearm"),("Wrist","Distal arm")):
        out += [activate(component), cmd("solid.remove", bodies=[label+" hydraulic "+s for s in HYDRAULIC_SUFFIXES])]
    for label, center, parent, child, _, _ in JOINTS:
        out.append(activate(parent))
        old = ([f"Turret tilt bearing {y}" for y in (-52,48)] if label=="Turret tilt"
               else [f"{label} rolling bearing {y}" for y in (-9,5)])
        out.append(cmd("solid.remove", bodies=old))
        shaft = "Turret elevation shaft" if label=="Turret tilt" else label+" joint shaft"
        transfer = [shaft]
        if label != "Turret tilt":
            out.append(cmd("solid.scale", bodies=[shaft], factors=[1,37/34,1], origin=center))
            out.append(cmd("solid.move", bodies=[shaft], translate=[0,.5,0]))
            transfer += [label+" retaining cap -18", label+" retaining cap 17"]
        out.append(cmd("component.move_bodies", bodies=transfer, component=child))
    out += [activate("Forearm"),
            cmd("solid.move", bodies=["Elbow link -9"], translate=[0,-4.2,0]),
            cmd("solid.move", bodies=["Elbow link 5"], translate=[0,4.2,0])]
    for t in (.28,.68):
        out.append(cmd("solid.scale", bodies=[f"Forearm cross brace {t}"], factors=[1,1.84,1], origin=[0,0,0]))
        out.append(cmd("solid.scale", bodies=[f"Forearm brace screw {t} shank"], factors=[1,26.4/18,1], origin=[0,0,0]))
        out.append(cmd("solid.move", bodies=[f"Forearm brace screw {t} head"], translate=[0,-4.2,0]))
        p = [242+108*t, -15, 281-32*t]
        out.append(cylinder(f"Forearm brace {t} drilled clearance", p,1.6,30,
                            operation="cut", targets=["Elbow link -9","Elbow link 5",f"Forearm cross brace {t}"]))
    out.append(activate("Arm turret"))
    out.append(cylinder("Shoulder clevis bearing seats", [177,-15,165],6.05,30,
                        operation="cut", targets=["Shoulder clevis -13","Shoulder clevis 9"]))
    out.append(activate("Turret yaw"))
    out.append(cylinder("Sensor fork bearing seats", [-45,-55,249],5.05,110,
                        operation="cut", targets=["Turret elevation fork -52","Turret elevation fork 48"]))
    # The original actuator ears intrude into adjacent moving plates.
    for label, c, parent, _,_,_ in JOINTS[:3]:
        face = {"Shoulder":-13.0,"Elbow":-13.4,"Wrist":-13.2}[label]
        out.append(activate(parent))
        for suffix, dx in ((-14,-12),(10,12)):
            ear = f"{label} actuator ear {suffix}"
            screw = f"{label} ear screw {suffix}"
            out.append(box(ear+" face relief", [c[0]+dx-3,face,c[2]-4], [6,4,8], operation="cut", targets=[ear]))
            out.append(cmd("solid.move", bodies=[screw], translate=[0,face+12,0]))
            out.append(cylinder(ear+" screw clearance", [c[0]+dx,-26,c[2]],1.6,15,
                                operation="cut", targets=[ear]))
    out.append(activate("Gripper tool"))
    out.append(cmd("solid.remove", bodies=["Tool wrist"]))
    out += [cmd("sketch.create", name="Tool wrist tongue outline", plane={"origin":[0,-4.8,0],"x_dir":[1,0,0],"y_dir":[0,0,-1]}),
            cmd("sketch.slot.center_to_center", p0=[410,-267], p1=[435,-247], width=12),
            cmd("sketch.circle.center", center=[410,-267], radius=3.05),
            cmd("sketch.constraint.fix", entities=["l1","a1","l2","a2","c1"], fixed=True),
            cmd("sketch.finish"),
            cmd("solid.extrude", sketch="Tool wrist tongue outline", distance=9.6,
                operation="new", profiles=[{"point":[420,-260]}],
                name="Tool wrist machined tongue", body_name="Tool wrist machined tongue"),
            box("Tool wrist crosshead mating face", [425,-6,230], [25,12,17],
                operation="cut", targets=["Tool wrist machined tongue"])]
    out += style(["Tool wrist machined tongue"], "Aluminum", (175,184,193), .32)
    out.append(activate())

    # New hubs are made in root and transferred after their geometry is complete.
    hubs = [activate()]
    transfers = []
    for label, c, _, child, _, _ in JOINTS[:3]:
        for side, y0, inward in (("port",-13.2 if label=="Elbow" else -9,1),
                                 ("starboard",9.2 if label=="Elbow" else 5,-1)):
            if label == "Elbow":
                inward = -inward  # Forearm plates lie outside the upper-arm bearings.
            name = f"{label} driven split hub {side}"
            part, bodies = clamp_hub(name,c,y0,inward)
            hubs += part
            transfers.append((child,bodies))
    for component,bodies in transfers:
        hubs.append(cmd("component.move_bodies", bodies=bodies, component=component))
    for label,c,_,child,_,_ in JOINTS[:4]:
        hubs.append(activate(child))
        collars = [label+" retaining cap -18",label+" retaining cap 17"]
        for cap,y in zip(collars,(-18,17)):
            hubs.append(cylinder(cap+" shaft clearance",[c[0],y-.5,c[2]],3.05,3,
                                  operation="cut",targets=[cap]))
            hubs.append(cylinder(cap+" radial locking bore",[c[0],y+1,c[2]+2.5],.65,3.5,
                                  axis=[0,0,1],operation="cut",targets=[cap]))
            screw=cap+" locking screw"
            hubs.append(cylinder(screw,[c[0],y+1,c[2]+3.0],.6,1.85,axis=[0,0,1]))
            hubs.append(cylinder(screw+" socket",[c[0],y+1,c[2]+4.4],.3,.6,
                                  axis=[0,0,1],operation="cut",targets=[screw]))
            hubs += style([screw],color=(66,70,76),roughness=.38)
        # Axial spacers touch only inner-ring/shaft shoulders, never the outer race.
        shoulder_y = {"Shoulder":13,"Elbow":13.8,"Wrist":13.2,"Tool tilt":9}[label]
        names=[]
        for side,ya,yb in (("port",-16,-shoulder_y),("starboard",shoulder_y,17)):
            name=f"{label} shaft spacer {side}"
            hubs += ring(name,c,ya,yb-ya,3.7,3.05)
            names.append(name)
        if label != "Shoulder":
            inner_y = 4.8 if label == "Tool tilt" else 9.0
            for side,ya in (("port",-inner_y-.2),("starboard",inner_y)):
                name=f"{label} inner ring thrust spacer {side}"
                hubs += ring(name,c,ya,.2,3.7,3.05)
                names.append(name)
        hubs += style(names,roughness=.31)
    hubs.append(activate())
    return [("arm-clearances",out),("driven-hubs",hubs)]


def head_retention_commands():
    """Bored sensor-head axle, lockable collars and inner-race-only spacers."""
    out = [activate("Turret sensor head")]
    center = [-45, 0, 249]
    screws, spacers = [], []
    for cap_y in (-56, 54):
        cap = f"Turret pivot cap {cap_y}"
        out += [cylinder(cap+" shaft clearance", [-45,cap_y-.5,249],3.05,3,
                         operation="cut",targets=[cap]),
                cylinder(cap+" radial locking bore", [-45,cap_y+1,251.5],.65,3.5,
                         axis=[0,0,1],operation="cut",targets=[cap])]
        screw = cap+" locking screw"
        out += [cylinder(screw,[-45,cap_y+1,252],.6,1.85,axis=[0,0,1]),
                cylinder(screw+" socket",[-45,cap_y+1,253.4],.3,.6,
                         axis=[0,0,1],operation="cut",targets=[screw])]
        screws.append(screw)
    # The old solid crossbeam enclosed the shaft. A continuous clearance bore
    # and two radial set screws now describe an actual fastening arrangement.
    out.append(cylinder("Sensor crossbeam shaft clearance",[-45,-48,249],3.05,96,
                        operation="cut",targets=["Unified sensor crossbeam"]))
    for y in (-44,44):
        screw = f"Sensor crossbeam shaft set screw {y}"
        out += [cylinder(screw+" clearance",[-45,y,251.95],.8,4.1,
                         axis=[0,0,1],operation="cut",targets=["Unified sensor crossbeam"]),
                cylinder(screw,[-45,y,252],.75,3,axis=[0,0,1]),
                cylinder(screw+" socket",[-45,y,254.4],.35,.7,
                         axis=[0,0,1],operation="cut",targets=[screw])]
        screws.append(screw)
    for side, outer_y, inner_y in (("port",-54,-48),("starboard",52,47)):
        for kind, y, width in (("shaft spacer",outer_y,2),
                               ("inner ring thrust spacer",inner_y,1)):
            name = f"Turret tilt {kind} {side}"
            out += ring(name,center,y,width,3.5,3.05)
            spacers.append(name)
    out += style(screws,color=(66,70,76),roughness=.38)
    out += style(spacers,roughness=.31)
    out.append(activate())
    return out


def stages():
    """Named command lists; each is safe to execute then save/reopen."""
    specs=bearing_specs()
    result=arm_clearance_stages()
    result.append(("head-retention",head_retention_commands()))
    for i in range(0,len(specs),4):
        commands=[activate()]
        for s in specs[i:i+4]:
            commands += bearing_geometry(s)
        commands.append(activate())
        result.append((f"bearing-geometry-{i//4+1}",commands))
    result.append(("bearing-assembly",bearing_assembly(specs)))
    return result


def commands():
    return [c for _,block in stages() for c in block]


def fixture_commands():
    """Single compact bearing fixture for independent geometry and phase checks."""
    s=bearing_specs()[0]
    s=dict(s,name="Bearing test",center=[0,0,0],parent="Fixed support",child="Driven shaft",
           driver="Test shaft angle",cage_joint="Bearing test cage orbit")
    out=[cylinder("Fixed support",[0,-3,0],7,1),
         cylinder("Support clearance",[0,-4,0],3.1,3,operation="cut",targets=["Fixed support"]),
         cylinder("Driven shaft",[0,-4,0],3,8),
         cmd("component.from_bodies",bodies=["Fixed support","Driven shaft"]),
         cmd("occurrence.ground",occurrence="Fixed support:1",grounded=True),
         cmd("joint.as_built",type="revolute",a="Fixed support",b="Driven shaft",
             at={"occurrence":"Fixed support","point":[0,0,0],"z":AXIS},name="Test shaft angle"),
         activate()]
    out+=bearing_geometry(s)+bearing_assembly([s])
    out+=[cmd("joint.list"),cmd("document.inspect",measure=True,sketches=False)]
    return out


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out",type=Path,required=True)
    parser.add_argument("--fixture",action="store_true")
    parser.add_argument("--stages",action="store_true")
    args=parser.parse_args()
    args.out.parent.mkdir(parents=True,exist_ok=True)
    data=({"stages":[{"name":n,"commands":cs} for n,cs in stages()],"bearings":bearing_specs()}
          if args.stages else {"commands":fixture_commands() if args.fixture else commands()})
    args.out.write_text(json.dumps(data,indent=2)+"\n",encoding="utf-8")


if __name__=="__main__":
    main()
