#!/usr/bin/env python3
"""Size the existing, attributed SolidCraft app mark for iOS loose PNG icons."""
from pathlib import Path

from PIL import Image

here = Path(__file__).resolve().parent
root = here.parent.parent
source = root / "assets/icons/app/linux/exports/hicolor/512x512/apps/ai.storyteller.solidcraft.png"
destination = root / "target/ios-icons"
destination.mkdir(parents=True, exist_ok=True)
with Image.open(source).convert("RGBA") as mark:
    # iOS app icons must be opaque; retain the original mark without redrawing it.
    image = Image.new("RGB", mark.size, "white")
    image.paste(mark, mask=mark.getchannel("A"))
    for name, size in [
        ("AppIcon60x60@2x.png", 120),
        ("AppIcon60x60@3x.png", 180),
        ("AppIcon76x76@2x.png", 152),
        ("AppIcon83.5x83.5@2x.png", 167),
    ]:
        image.resize((size, size), Image.Resampling.LANCZOS).save(destination / name)
