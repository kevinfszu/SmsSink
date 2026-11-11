"""生成 SmsSink 应用图标（assets/app.ico + assets/app.png）。

设计：Win11 风格圆角方块底 + 竖向蓝紫渐变，主体是一枚「向下箭头进入托盘」的白色字形，
对应本工具「被动接收 → 归入收件盘」的语义。多尺寸分别按视觉重量微调笔画粗细，
避免小尺寸下笔画过细糊成一团。全部为程序化绘制，随时可重跑。

用法：
    python tools/make_icon.py             # 生成 assets/app.ico 与 assets/app.png
    python tools/make_icon.py --preview   # 额外输出多尺寸预览图
    python tools/make_icon.py --embed     # 同步更新管理台页面里内嵌的图标（base64）

改了设计之后跑一次 `--embed`，`assets/index.html` 的 logo 与 favicon 会一起更新。
"""

from __future__ import annotations

import math
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"

# 与界面主色一致（assets/index.html 的 --accent）
TOP = (91, 155, 255)   # #5B9BFF
BOTTOM = (29, 78, 216)  # #1D4ED8
SS = 4                  # 超采样倍数
CANVAS = 1024           # 字形坐标系
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]

# 各尺寸的笔画加权：小图标需要相对更粗的笔画才清晰
STROKE_BOOST = {16: 1.45, 20: 1.32, 24: 1.16, 32: 1.06, 40: 1.02, 48: 1.0}
# 小尺寸整体缩放：留出更多呼吸空间，避免图形顶到方块边缘
FIT = {16: 0.90, 20: 0.94, 24: 0.97}


def gradient(size: int) -> Image.Image:
    """竖向渐变，逐行插值。"""
    column = Image.new("RGB", (1, size))
    px = column.load()
    for y in range(size):
        t = y / max(1, size - 1)
        # 稍微做一点 gamma 让中间偏亮，避免上亮下暗断裂
        tt = t ** 0.92
        px[0, y] = (
            round(TOP[0] + (BOTTOM[0] - TOP[0]) * tt),
            round(TOP[1] + (BOTTOM[1] - TOP[1]) * tt),
            round(TOP[2] + (BOTTOM[2] - TOP[2]) * tt),
        )
    return column.resize((size, size), Image.BICUBIC)


def rounded_mask(size: int, radius_ratio: float = 0.219) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    d = ImageDraw.Draw(mask)
    r = round(size * radius_ratio)
    d.rounded_rectangle([0, 0, size - 1, size - 1], radius=r, fill=255)
    return mask


def sheen(size: int, layer: Image.Image) -> None:
    """左上角柔光 + 顶部内高光，制造轻微立体感（仅大尺寸）。"""
    if size < 48:
        return
    glow = Image.new("L", (size, size), 0)
    g = ImageDraw.Draw(glow)
    r = int(size * 0.62)
    g.ellipse([-r // 3, -r // 2, r, r // 2], fill=52)
    glow = glow.filter(ImageFilter.GaussianBlur(size * 0.10))
    white = Image.new("RGBA", (size, size), (255, 255, 255, 255))
    layer.paste(white, (0, 0), glow)


def glyph_layers(size: int, boost: float, ss: int = SS) -> Image.Image:
    """在 ss 倍画布上绘制白色字形，返回同尺寸 RGBA（含柔和投影）。

    托盘侧壁刻意做矮，小尺寸下自然退化成圆头横线，避免与箭头粘连。
    """
    s = size * ss
    k = s / CANVAS * FIT.get(size, 1.0)
    layer = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    simple = size <= 20

    def pts(*xy: tuple[float, float]) -> list[tuple[float, float]]:
        return [(x * k, y * k) for x, y in xy]

    def stroke(width: float) -> int:
        return max(1, round(width * k * boost))

    def poly(points, width):
        d.line(points, fill=(255, 255, 255, 255), width=width, joint="curve")
        r = width / 2
        for x, y in points:
            d.ellipse([x - r, y - r, x + r, y + r], fill=(255, 255, 255, 255))

    # 箭头：竖笔 + 人字形箭头
    arrow_w = stroke(90)
    if simple:
        # 16/20px：人字形箭头在这个尺寸下会和竖笔连成一个「十」字，
        # 改用「竖笔 + 实心三角箭头 + 底线」，下采样后仍能看出是一支向下的箭头。
        stem = stroke(86)
        d.rectangle(
            [512 * k - stem / 2, 176 * k, 512 * k + stem / 2, 366 * k],
            fill=(255, 255, 255, 255),
        )
        d.polygon(
            pts((382, 346), (642, 346), (512, 520)),
            fill=(255, 255, 255, 255),
        )
        poly(pts((360, 722), (664, 722)), stroke(78))
    else:
        poly(pts((512, 198), (512, 436)), arrow_w)
        poly(pts((414, 352), (512, 456), (610, 352)), arrow_w)
        # 浅口托盘：侧壁很矮，32px 起才画得开
        poly(pts((330, 700), (330, 792), (694, 792), (694, 700)), stroke(76))

    # 以实际墨迹包围盒做光学居中（含笔画外扩），避免肉眼可见的偏上/偏下
    bbox = layer.split()[3].getbbox()
    if bbox:
        x0, y0, x1, y1 = bbox
        dx = (s - (x1 - x0)) / 2 - x0
        dy = (s - (y1 - y0)) / 2 - y0
        layer = layer.transform(
            (s, s), Image.AFFINE, (1, 0, -dx, 0, 1, -dy), resample=Image.BICUBIC
        )

    # 投影：让白色字形从渐变面上浮起来
    if size >= 32:
        alpha = layer.split()[3].filter(ImageFilter.GaussianBlur(max(1, s * 0.016)))
        shadow = Image.new("RGBA", (s, s), (8, 22, 58, 0))
        shadow.putalpha(alpha.point(lambda v: int(v * 0.28)))
        shadow = shadow.transform(
            (s, s), Image.AFFINE, (1, 0, 0, 0, 1, -s * 0.010), resample=Image.BICUBIC
        )
        layer = Image.alpha_composite(shadow, layer)
    return layer


def render(size: int) -> Image.Image:
    boost = STROKE_BOOST.get(size, 1.0)
    big = size * SS
    base = gradient(big).convert("RGBA")
    sheen(big, base)
    glyph = glyph_layers(size, boost, SS).resize((big, big), Image.LANCZOS)
    base = Image.alpha_composite(base, glyph)
    base.putalpha(rounded_mask(big))
    return base.resize((size, size), Image.LANCZOS)


# ---------------------------------------------------------------- ICO 写出
def dib_bytes(img: Image.Image) -> bytes:
    """32 位 BMP（ICO 内嵌格式）：BITMAPINFOHEADER + 自下而上 BGRA + AND 掩码。"""
    w, h = img.size
    rgba = img.load()
    header = struct.pack(
        "<IiiHHIIiiII",
        40, w, h * 2, 1, 32, 0, 0, 0, 0, 0, 0,
    )
    pixels = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = rgba[x, y]
            pixels += bytes((b, g, r, a))
    # AND 掩码：1 = 透明；每行按 4 字节对齐
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
                0,
                0,
                1,
                32,
                len(blob),
                offset,
            )
        )
        payloads.append(blob)
        offset += len(blob)
    path.write_bytes(
        struct.pack("<HHH", 0, 1, len(images)) + b"".join(entries) + b"".join(payloads)
    )


def preview(images: dict[int, Image.Image]) -> Image.Image:
    """深浅两种底色上的多尺寸效果图，图标底对齐便于比较大小时的可读性。"""
    shown = [16, 20, 24, 32, 40, 48, 64, 128, 256]
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
