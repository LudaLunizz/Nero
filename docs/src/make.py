#!/usr/bin/env python3
"""Make the README pictures in docs/ from the HTML sources in docs/src/.

usage: python3 docs/src/make.py [NAME...]     (no NAME: all of them)

Every picture is one HTML file here plus style.css; change the words in the HTML and run this again.
Needs: firefox (headless, nothing appears on your desktop), Pillow, optipng (optional).
"""
import pathlib, shutil, subprocess, sys, tempfile
from PIL import Image, ImageDraw

SRC = pathlib.Path(__file__).resolve().parent
DOCS = SRC.parent
BORDER = (0x2b, 0x30, 0x38, 255)
# name: (width, height) in pixels; the HTML is laid out at half that (style.css: zoom 2)
PAGES = {
    "banner": (1720, 600),
    "parts": (1720, 836),
    "keep-onscreen": (1720, 956),
    "tiling": (1720, 1180),
    "layout": (1720, 848),
    "workspaces": (1720, 940),
    "taskbar": (1720, 688),
    "architecture": (1720, 880),
    "social-preview": (1280, 640),
}

def rounded(im, radius=28):
    """Round the corners (transparent outside) so a picture sits well on a light or a dark page."""
    im = im.convert("RGBA")
    big = 4
    mask = Image.new("L", (im.width * big, im.height * big), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, mask.width - 1, mask.height - 1), radius * big, fill=255)
    im.putalpha(mask.resize(im.size, Image.LANCZOS))
    edge = Image.new("RGBA", (im.width * big, im.height * big), (0, 0, 0, 0))
    ImageDraw.Draw(edge).rounded_rectangle(
        (big, big, edge.width - 1 - big, edge.height - 1 - big), radius * big, outline=BORDER, width=2 * big)
    im.alpha_composite(edge.resize(im.size, Image.LANCZOS))
    return im

def save(im, name):
    out = DOCS / name
    im.save(out)
    if shutil.which("optipng"):
        subprocess.run(["optipng", "-quiet", "-o2", str(out)], check=False)
    print(f"{name}: {im.width}x{im.height}, {out.stat().st_size // 1024} KB")

def page(html, width, height, tmp):
    """A headless Firefox screenshot of docs/src/HTML at width x height pixels."""
    shot = tmp / (html + ".png")
    subprocess.run(
        ["firefox", "--headless", "--no-remote", "--profile", str(tmp / "ff"), f"--window-size={width},{height}",
         "--screenshot", str(shot), (SRC / html).as_uri()],
        check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)
    return Image.open(shot).crop((0, 0, width, height))

def main():
    names = sys.argv[1:] or list(PAGES)
    # work files inside the repo (build/ is git-ignored): a Firefox from a snap does not see the real /tmp
    work = DOCS.parent / "build"
    work.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="docs-", dir=work) as tmp:
        tmp = pathlib.Path(tmp)
        (tmp / "ff").mkdir()
        for name in names:
            width, height = PAGES[name]
            if name == "social-preview":   # GitHub wants this one square-cornered and without alpha
                save(page("social.html", width, height, tmp).convert("RGB"), name + ".png")
            else:
                save(rounded(page(name + ".html", width, height, tmp)), name + ".png")

if __name__ == "__main__":
    main()
