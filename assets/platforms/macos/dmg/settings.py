"""dmgbuild settings for the SolidCraft disk image (packaging/macos/package.sh).

Passed with -D here=<this folder> -D app=<SolidCraft.app> -D readme=<README.txt>
-D icns=<icon.icns>. dmgbuild writes the Finder layout (window, background, icon positions) straight into the image, so the build
needs no Finder session and no AppleScript.
"""
import json
import os

here = defines["here"]  # noqa: F821  (this folder; settings files have no __file__)
layout = json.load(open(os.path.join(here, "layout.json")))
width, height = layout["window"]

format = "UDZO"
filesystem = "HFS+"
# `dmgbuild -D` values arrive in `defines`.
files = [defines["app"], defines["readme"]]  # noqa: F821
symlinks = {"Applications": "/Applications"}
# The read-me stays on the disk for people who look for it, out of the window's calm layout.
hide = ["README.txt"]
icon = defines["icns"]  # noqa: F821

background = os.path.join(here, "background.tiff")
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
default_view = "icon-view"
arrange_by = None
# The rectangle is the whole window: the title bar (32 pt on macOS 27) sits above the content area,
# which must be exactly the size of the background.
window_rect = ((260, 260), (width, height + layout["title_bar"]))
icon_size = layout["icon_size"]
text_size = layout["text_size"]
label_pos = "bottom"
icon_locations = {
    os.path.basename(defines["app"]): tuple(layout["app"]),  # noqa: F821
    "Applications": tuple(layout["applications"]),
    "README.txt": (width // 2, height + 200),
}
