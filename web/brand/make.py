#!/usr/bin/env python3
"""Rebuild the share card, GitHub preview and site icons from web/brand/.

    python3 web/brand/make.py

Needs Pillow. The banner is cropped, not redrawn: 1200x630 for og.jpg, 1280x640
for github.jpg. Icons are the three-pixel-bots avatar at the sizes the site serves.
"""

from __future__ import annotations

import base64
import io
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
WEB = HERE.parent
AVATAR = HERE / "avatar.png"
BANNER = HERE / "banner.jpg"


def cover_crop(im: Image.Image, width: int, height: int) -> Image.Image:
    scale = max(width / im.width, height / im.height)
    sized = im.resize(
        (round(im.width * scale), round(im.height * scale)), Image.Resampling.LANCZOS
    )
    left = (sized.width - width) // 2
    top = (sized.height - height) // 2
    return sized.crop((left, top, left + width, top + height))


def save_png(im: Image.Image, path: Path, size: tuple[int, int]) -> None:
    im.resize(size, Image.Resampling.LANCZOS).save(
        path, format="PNG", optimize=True, compress_level=9
    )
    print(f"{path.relative_to(WEB.parent)}: {size[0]}x{size[1]}, {path.stat().st_size} bytes")


def save_jpeg(im: Image.Image, path: Path, quality: int) -> None:
    im.save(
        path,
        format="JPEG",
        quality=quality,
        optimize=True,
        progressive=True,
        subsampling=0,
    )
    print(
        f"{path.relative_to(WEB.parent)}: {im.width}x{im.height}, "
        f"{path.stat().st_size} bytes"
    )


def write_favicon_svg(avatar: Image.Image) -> None:
    buf = io.BytesIO()
    avatar.resize((192, 192), Image.Resampling.LANCZOS).save(
        buf, format="PNG", optimize=True, compress_level=9
    )
    svg = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 192 192">\n'
        f'  <image href="data:image/png;base64,{base64.b64encode(buf.getvalue()).decode("ascii")}" '
        'width="192" height="192"/>\n'
        "</svg>\n"
    )
    path = WEB / "favicon.svg"
    path.write_text(svg)
    print(f"{path.relative_to(WEB.parent)}: {path.stat().st_size} bytes")


def main() -> None:
    avatar = Image.open(AVATAR).convert("RGB")
    banner = Image.open(BANNER).convert("RGB")

    save_png(avatar, WEB / "icon-512.png", (512, 512))
    save_png(avatar, WEB / "icon-192.png", (192, 192))
    save_png(avatar, WEB / "apple-touch-icon.png", (180, 180))
    save_png(avatar, WEB / "favicon-32.png", (32, 32))
    ico32 = avatar.resize((32, 32), Image.Resampling.LANCZOS)
    ico32.save(WEB / "favicon.ico", format="ICO", sizes=[(32, 32), (16, 16)])
    print(f"web/favicon.ico: {(WEB / 'favicon.ico').stat().st_size} bytes")
    write_favicon_svg(avatar)

    save_jpeg(cover_crop(banner, 1200, 630), WEB / "og.jpg", 90)
    save_jpeg(cover_crop(banner, 1280, 640), HERE / "github.jpg", 88)


if __name__ == "__main__":
    main()
