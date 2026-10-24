# dmgbuild settings for the SolidCraft disk image: the window a user sees when they open the .dmg.
# packaging/macos/package.sh passes the paths with -D:
#   dmgbuild -s dmg-settings.py -D app=<SolidCraft.app> -D background=<dmg-background.png> SolidCraft <out.dmg>
# `defines` is provided by dmgbuild. dmgbuild finds dmg-background@2x.png next to the 1x image and
# combines both into one HiDPI background.

app = defines["app"]  # noqa: F821
background = defines["background"]  # noqa: F821

format = "UDZO"
compression_level = 9
filesystem = "HFS+"

files = [app]
symlinks = {"Applications": "/Applications"}
hide_extensions = ["SolidCraft.app"]

# The background is 640x430 points (dmg-background.png, and @2x). The window size includes the
# title bar, so it is a little taller; with Finder's path bar, status bar or tab bar turned on the
# bottom of the background is cut off, but the icons and labels still fit.
window_rect = ((200, 140), (640, 456))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False

# Icon centres, either side of the arrow in the middle of the background. Finder draws the labels
# in black, so the background has a light pill under each label (at y = 277); move them together.
icon_size = 112
text_size = 13
label_pos = "bottom"
icon_locations = {
    "SolidCraft.app": (122, 205),
    "Applications": (522, 205),
}
arrange_by = None
