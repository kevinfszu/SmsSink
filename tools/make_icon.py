"""生成 SmsSink 应用图标（assets/app.ico + assets/app.png）。

设计：Win11 风格圆角方块底 + 一枚白色起飞的鸽子字形，对应本工具「手机短信直飞电脑」的语义。
字形来自参考图矢量化（tools/trace_dove.py → tools/dove_glyph.py），纯多边形，
不含描边、渐变、投影，因此在任何尺寸下都不会出现糊边。

用法：
    python tools/make_icon.py             # 生成 assets/app.ico 与 assets/app.png
    python tools/make_icon.py --preview   # 额外输出多尺寸预览图
    python tools/make_icon.py --embed     # 同步更新管理台页面里内嵌的图标（base64）

换了参考图之后：先跑 tools/trace_dove.py，再跑本脚本。
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dove_glyph import POLYGONS  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"

# 取自参考图的实测底色（#0A7FEE），整块纯色，不做渐变
BG = (10, 127, 238)
SS = 4                  # 超采样倍数
CANVAS = 1024           # 字形坐标系
RADIUS_RATIO = 0.205    # 圆角半径占边长比例（对齐参考图）
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]

# 小尺寸下把字形略微放大，抵消重采样后的笔画变细
SCALE_BOOST = {16: 1.10, 20: 1.06, 24: 1.03, 32: 1.01, 40: 1.0, 48: 1.0}


def rounded_mask(size: int, radius_ratio: float = RADIUS_RATIO) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    r = round(size * radius_ratio)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=r, fill=255)
    return mask


def glyph_layer(size: int, boost: float, ss: int = SS) -> Image.Image:
    """在 ss 倍画布上把多边形填成白色字形，并做光学居中。"""
    s = size * ss
    k = s / CANVAS * boost
    layer = Image.new("L", (s, s), 0)
    d = ImageDraw.Draw(layer)
    for poly in POLYGONS:
        d.polygon([(x * k, y * k) for x, y in poly], fill=255)

    bbox = layer.getbbox()
    if bbox:
        x0, y0, x1, y1 = bbox
        dx = round((s - (x1 - x0)) / 2 - x0)
        dy = round((s - (y1 - y0)) / 2 - y0)
        if dx or dy:
            layer = layer.transform(
                (s, s), Image.AFFINE, (1, 0, -dx, 0, 1, -dy), resample=Image.BICUBIC
            )
    return layer


def render(size: int) -> Image.Image:
    boost = SCALE_BOOST.get(size, 1.0)
    big = size * SS
    base = Image.new("RGBA", (big, big), BG + (255,))
    white = Image.new("RGBA", (big, big), (255, 255, 255, 255))
    base.paste(white, (0, 0), glyph_layer(size, boost))
    base.putalpha(rounded_mask(big))
    return base.resize((size, size), Image.LANCZOS)


# ---------------------------------------------------------------- ICO 写出
def dib_bytes(img: Image.Image) -> bytes:
    """32 位 BMP（ICO 内嵌格式）：BITMAPINFOHEADER + 自下而上 BGRA + AND 掩码。"""
    w, h = img.size
    rgba = img.load()
    header = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = rgba[x, y]
            pixels += bytes((b, g, r, a))
    row_bytes = ((w + 31) // 32) * 4
    mask = bytearray()
    for y in range(h - 1, -1, -1):
        row = bytearray(row_bytes)
        for x in range(w):
            if rgba[x, y][3] == 0:
                row[x // 8] |= 0x80 >> (x % 8)
        mask += row
    return header + bytes(pixels) + bytes(mask)


def png_bytes(img: Image.Image) -> bytes:
    import io

    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def write_ico(path: Path, images: list[Image.Image]) -> None:
    entries, payloads = [], []
    offset = 6 + 16 * len(images)
    for img in images:
        blob = png_bytes(img) if img.size[0] >= 128 else dib_bytes(img)
        w, h = img.size
        entries.append(
            struct.pack(
                "<BBBBHHII",
                0 if w >= 256 else w,
                0 if h >= 256 else h,
                0, 0, 1, 32, len(blob), offset,
            )
        )
        payloads.append(blob)
        offset += len(blob)
    path.write_bytes(
        struct.pack("<HHH", 0, 1, len(images)) + b"".join(entries) + b"".join(payloads)
    )


def preview(images: dict[int, Image.Image]) -> Image.Image:
    """深浅两种底色上的多尺寸效果图，图标底对齐便于比较可读性。"""
    shown = SIZES
    pad, gap, band = 28, 20, 296
    cell = 256 + gap
    width = pad * 2 + cell * len(shown)
    height = pad * 2 + band * 2
    canvas = Image.new("RGB", (width, height), (247, 248, 250))
    d = ImageDraw.Draw(canvas)
    for row in range(2):
        top = pad + row * band
        if row == 1:
            d.rectangle([0, top, width, top + band], fill=(30, 32, 36))
        color = (120, 126, 136) if row == 0 else (150, 156, 166)
        for i, size in enumerate(shown):
            img = images[size]
            x = pad + i * cell + (256 - size) // 2
            canvas.paste(img, (x, top + 248 - size), img)
            d.text((pad + i * cell + 104, top + 262), f"{size}", fill=color)
    return canvas


def embed_in_page(png: Image.Image) -> None:
    """把 64px 图标以 data URI 形式写进管理台页面（logo 与 favicon）。"""
    import base64
    import io
    import re

    buf = io.BytesIO()
    png.resize((64, 64), Image.LANCZOS).save(buf, format="PNG", optimize=True)
    uri = "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode()

    page = ASSETS / "index.html"
    html = page.read_text(encoding="utf-8")
    html, n = re.subn(r"data:image/png;base64,[A-Za-z0-9+/=]+", uri, html)
    page.write_text(html, encoding="utf-8")
    print(f"已更新管理台内嵌图标 {n} 处（{len(uri)} 字符）")


def main() -> None:
    ASSETS.mkdir(exist_ok=True)
    images = {s: render(s) for s in SIZES}
    write_ico(ASSETS / "app.ico", [images[s] for s in SIZES])
    images[256].save(ASSETS / "app.png")
    print(f"app.ico  {len(SIZES)} 个尺寸: {SIZES}")
    print("app.png  256x256")
    if "--preview" in sys.argv:
        out = ROOT / "tools" / "icon-preview.png"
        preview(images).save(out)
        print(f"预览: {out}")
    if "--embed" in sys.argv:
        embed_in_page(images[256])


if __name__ == "__main__":
    main()
