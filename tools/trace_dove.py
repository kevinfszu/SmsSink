"""从参考图里把白色鸽子字形矢量化成多边形，输出 tools/dove_glyph.py。

算法：阈值取字形 → 连通域标记 → 逐块 Moore 边界跟踪 → Douglas-Peucker 简化
→ 归一化到 1024 画布。纯 PIL + 标准库，不依赖 numpy / opencv。

字形是多个分离部件（三片翅膀与身体之间有蓝色缝隙），所以输出是「多边形列表」。

用法：
    python tools/trace_dove.py "C:/path/to/reference.jpg"
    python tools/trace_dove.py "C:/path/to/reference.jpg" --ratio 0.66 --eps 1.0
"""

from __future__ import annotations

import argparse
from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "tools" / "dove_glyph.py"
CANVAS = 1024

DIRS = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]


def load_mask(path: Path, pad: int = 6):
    im = Image.open(path).convert("RGB")
    px = im.load()
    w, h = im.size

    blues = [
        (x, y)
        for y in range(0, int(h * 0.8), 2)
        for x in range(0, w, 2)
        if px[x, y][2] > 150 and px[x, y][2] - px[x, y][0] > 60
    ]
    bx0, bx1 = min(x for x, _ in blues), max(x for x, _ in blues)
    by0, by1 = min(y for _, y in blues), max(y for _, y in blues)

    # 方块四角的浅灰背景同为高红通道，先把搜索区向内收 10%，把圆角排除在外
    ix0 = bx0 + round((bx1 - bx0) * 0.10); ix1 = bx1 - round((bx1 - bx0) * 0.10)
    iy0 = by0 + round((by1 - by0) * 0.10); iy1 = by1 - round((by1 - by0) * 0.10)

    xs, ys = [], []
    for y in range(iy0, iy1 + 1):
        for x in range(ix0, ix1 + 1):
            if px[x, y][0] > 160:
                xs.append(x); ys.append(y)
    gx0, gy0 = min(xs) - pad, min(ys) - pad
    gx1, gy1 = max(xs) + pad, max(ys) + pad

    mw, mh = gx1 - gx0 + 1, gy1 - gy0 + 1
    mask = [[False] * mw for _ in range(mh)]
    for y in range(mh):
        for x in range(mw):
            mask[y][x] = px[gx0 + x, gy0 + y][0] > 160
    return mask, (gx0, gy0, gx1, gy1)


def components(mask, min_area: int = 40):
    """8 连通域标记，返回每个连通块的像素集合。"""
    h, w = len(mask), len(mask[0])
    seen = [[False] * w for _ in range(h)]
    out = []
    for y0 in range(h):
        for x0 in range(w):
            if not mask[y0][x0] or seen[y0][x0]:
                continue
            q = deque([(x0, y0)])
            seen[y0][x0] = True
            blob = []
            while q:
                x, y = q.popleft()
                blob.append((x, y))
                for dx, dy in DIRS:
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < w and 0 <= ny < h and mask[ny][nx] and not seen[ny][nx]:
                        seen[ny][nx] = True
                        q.append((nx, ny))
            if len(blob) >= min_area:
                out.append(blob)
    out.sort(key=len, reverse=True)
    return out


def trace(blob, mask) -> list[tuple[int, int]]:
    """对单个连通块做 Moore 边界跟踪。"""
    h, w = len(mask), len(mask[0])
    members = set(blob)
    start = min(blob, key=lambda p: (p[1], p[0]))

    b = start
    c = (start[0] - 1, start[1])
    contour = [start]

    for _ in range(w * h * 2):
        idx = DIRS.index((c[0] - b[0], c[1] - b[1]))
        nxt = None
        for k in range(1, 9):
            j = (idx + k) % 8
            nx, ny = b[0] + DIRS[j][0], b[1] + DIRS[j][1]
            if (nx, ny) in members:
                nxt = (nx, ny)
                c = (b[0] + DIRS[(j - 1) % 8][0], b[1] + DIRS[(j - 1) % 8][1])
                break
        if nxt is None:
            break
        b = nxt
        contour.append(b)
        if b == start and len(contour) > 8:
            break
    return contour


def dp(points, eps: float):
    if len(points) < 3:
        return points

    def rdp(pts):
        if len(pts) < 3:
            return pts
        x0, y0 = pts[0]; x1, y1 = pts[-1]
        dx, dy = x1 - x0, y1 - y0
        norm = (dx * dx + dy * dy) ** 0.5
        best, best_i = -1.0, 0
        for i in range(1, len(pts) - 1):
            x, y = pts[i]
            d = (((x - x0) ** 2 + (y - y0) ** 2) ** 0.5 if norm == 0
                 else abs(dy * x - dx * y + x1 * y0 - y1 * x0) / norm)
            if d > best:
                best, best_i = d, i
        if best > eps:
            return rdp(pts[: best_i + 1])[:-1] + rdp(pts[best_i:])
        return [pts[0], pts[-1]]

    far = max(range(len(points)),
              key=lambda i: (points[i][0] - points[0][0]) ** 2 + (points[i][1] - points[0][1]) ** 2)
    a = rdp(points[: far + 1])
    b = rdp(points[far:] + [points[0]])
    return a[:-1] + b[:-1]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("image")
    ap.add_argument("--ratio", type=float, default=0.66)
    ap.add_argument("--eps", type=float, default=1.0)
    ap.add_argument("--min-area", type=int, default=40)
    args = ap.parse_args()

    mask, (gx0, gy0, gx1, gy1) = load_mask(Path(args.image))
    blobs = components(mask, args.min_area)
    print(f"连通块 {len(blobs)} 个，面积 {[len(b) for b in blobs[:8]]}")

    gw, gh = gx1 - gx0 + 1, gy1 - gy0 + 1
    scale = CANVAS * args.ratio / gw
    polys = []
    for blob in blobs:
        cont = dp([(float(x), float(y)) for x, y in trace(blob, mask)], args.eps)
        if len(cont) < 3:
            continue
        polys.append([
            (round((x - (gw - 1) / 2) * scale + CANVAS / 2, 1),
             round((y - (gh - 1) / 2) * scale + CANVAS / 2, 1))
            for x, y in cont
        ])
    print(f"多边形 {len(polys)} 个，点数 {[len(p) for p in polys]}")

    lines = [
        '"""自动生成，勿手改：白色鸽子字形轮廓（1024 画布坐标系）。',
        "",
        f"由 tools/trace_dove.py 从参考图矢量化得到：{len(polys)} 个分离部件，",
        f"字形宽度占 1024 的 {args.ratio:.2f}，简化阈值 {args.eps}。",
        '"""',
        "",
        "POLYGONS = [",
    ]
    for poly in polys:
        lines.append("    [")
        for i in range(0, len(poly), 4):
            chunk = ", ".join(f"({x}, {y})" for x, y in poly[i : i + 4])
            lines.append(f"        {chunk},")
        lines.append("    ],")
    lines.append("]")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"已写出 {OUT}")


if __name__ == "__main__":
    main()
