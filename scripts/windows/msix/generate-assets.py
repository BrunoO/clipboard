#!/usr/bin/env python3
"""Generates standard MSIX visual assets from assets/icon.png."""

import os
from PIL import Image

def generate_assets():
    script_dir = os.path.dirname(os.path.abspath(__file__))
    repo_root = os.path.abspath(os.path.join(script_dir, "..", "..", ".."))
    src_icon = os.path.join(repo_root, "assets", "icon.png")
    out_dir = os.path.join(script_dir, "Assets")
    os.makedirs(out_dir, exist_ok=True)

    if not os.path.exists(src_icon):
        raise FileNotFoundError(f"Source icon not found at {src_icon}")

    img = Image.open(src_icon).convert("RGBA")

    # 1. StoreLogo: 50x50
    store_logo = img.resize((50, 50), Image.Resampling.LANCZOS)
    store_logo.save(os.path.join(out_dir, "StoreLogo.png"))

    # 2. Square44x44Logo: 44x44
    sq44 = img.resize((44, 44), Image.Resampling.LANCZOS)
    sq44.save(os.path.join(out_dir, "Square44x44Logo.png"))

    # 3. Square150x150Logo: 150x150
    sq150 = img.resize((150, 150), Image.Resampling.LANCZOS)
    sq150.save(os.path.join(out_dir, "Square150x150Logo.png"))

    # 4. Wide310x150Logo: 310x150 (transparent canvas, centered 130x130 icon)
    wide = Image.new("RGBA", (310, 150), (0, 0, 0, 0))
    icon_wide = img.resize((130, 130), Image.Resampling.LANCZOS)
    wide.paste(icon_wide, ((310 - 130) // 2, (150 - 130) // 2), icon_wide)
    wide.save(os.path.join(out_dir, "Wide310x150Logo.png"))

    # 5. SplashScreen: 620x300 (transparent canvas, centered 200x200 icon)
    splash = Image.new("RGBA", (620, 300), (0, 0, 0, 0))
    icon_splash = img.resize((200, 200), Image.Resampling.LANCZOS)
    splash.paste(icon_splash, ((620 - 200) // 2, (300 - 200) // 2), icon_splash)
    splash.save(os.path.join(out_dir, "SplashScreen.png"))

    print(f"Generated MSIX assets in {out_dir}")

if __name__ == "__main__":
    generate_assets()
