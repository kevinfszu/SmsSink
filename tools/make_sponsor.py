#!/usr/bin/env python3
"""把赞助页模板里的占位符换成内嵌的 base64 图片，生成 docs/sponsor.html。

用法：
    python tools/make_sponsor.py

输入：
    tools/sponsor-template.html   版式与文案（含 @@ICON@@ / @@WECHAT@@ / @@ALIPAY@@ 占位符）
    assets/app.png                页首图标，缩到 128px 内嵌
    docs/img/sponsor-wechat.png   微信收款码
    docs/img/sponsor-alipay.png   支付宝收款码

为什么内嵌：页面要跟着 exe 一起发（include_str!），单文件才能在离线、无外网的环境打开。
换收款码 = 覆盖两张 png 后重跑本脚本，不需要动 HTML。
"""

import base64
import io
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / "tools" / "sponsor-template.html"
OUTPUT = ROOT / "docs" / "sponsor.html"
ICON = ROOT / "assets" / "app.png"
QRS = {
    "@@WECHAT@@": ROOT / "docs" / "img" / "sponsor-wechat.png",
    "@@ALIPAY@@": ROOT / "docs" / "img" / "sponsor-alipay.png",
}


def data_uri(path: Path, size: int | None = None) -> str:
    if not path.exists():
        raise SystemExit(f"缺少图片: {path}")
    image = Image.open(path)
    if size:
        image = image.resize((size, size), Image.LANCZOS)
    buffer = io.BytesIO()
    image.save(buffer, "PNG", optimize=True)
    return "data:image/png;base64," + base64.b64encode(buffer.getvalue()).decode()


def main() -> None:
    html = TEMPLATE.read_text(encoding="utf-8")
    html = html.replace("@@ICON@@", data_uri(ICON, 128))
    for token, path in QRS.items():
        html = html.replace(token, data_uri(path))

    leftover = [t for t in ("@@ICON@@", *QRS) if t in html]
    if leftover:
        raise SystemExit(f"模板里有未替换的占位符: {leftover}")

    OUTPUT.write_text(html, encoding="utf-8")
    print(f"已生成 {OUTPUT.relative_to(ROOT)}  {len(html.encode()) / 1024:.0f} KB")


if __name__ == "__main__":
    main()
