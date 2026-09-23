"""生成应用图标（无第三方依赖，纯标准库）。

产出：
  assets/tray.png   32x32   托盘图标
  assets/icon.png   256x256 预览 / 通用
  assets/icon.ico   多尺寸 Windows 图标（64/48/32/16，PNG 压缩条目）
  src/main/icon-data.ts  托盘图标的 base64 常量（打包后无需依赖资源路径）

用法： python tools/gen-icons.py
"""
import base64
import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
ASSETS = os.path.join(ROOT, "assets")

SS = 4  # 超采样倍数，用于抗锯齿

BG = (47, 111, 235, 255)     # 品牌蓝
BAR = (255, 255, 255, 255)   # 白色列表条
DOT = (229, 72, 77, 255)     # 红点（通知）
RING = (255, 255, 255, 255)  # 红点外圈


def in_rrect(x, y, x0, y0, x1, y1, r):
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    dx = x - cx
    dy = y - cy
    return dx * dx + dy * dy <= r * r


def fill_circle(buf, cx, cy, radius, color, S):
    x0 = max(0, int(cx - radius - 1))
    x1 = min(S - 1, int(cx + radius + 1))
    y0 = max(0, int(cy - radius - 1))
    y1 = min(S - 1, int(cy + radius + 1))
    rr = radius * radius
    for y in range(y0, y1 + 1):
        dy = y - cy
        row = buf[y]
        for x in range(x0, x1 + 1):
            dx = x - cx
            if dx * dx + dy * dy <= rr:
                row[x] = color


def fill_rect(buf, x0, y0, x1, y1, color, S):
    xa = max(0, int(x0))
    xb = min(S - 1, int(x1))
    ya = max(0, int(y0))
    yb = min(S - 1, int(y1))
    for y in range(ya, yb + 1):
        row = buf[y]
        for x in range(xa, xb + 1):
            row[x] = color


def render(size):
    """渲染一张 size x size 的 RGBA 图标，返回每行 bytes 的列表。"""
    S = size * SS
    buf = [[(0, 0, 0, 0)] * S for _ in range(S)]

    # 圆角方形底
    r = S * 0.22
    for y in range(S):
        row = buf[y]
        for x in range(S):
            if in_rrect(x, y, 0, 0, S - 1, S - 1, r):
                row[x] = BG

    # 三条白色列表条
    bar_h = S * 0.085
    bx0 = S * 0.20
    widths = (0.48, 0.48, 0.34)
    for i, w in enumerate(widths):
        by = S * (0.30 + i * 0.16)
        fill_rect(buf, bx0, by, bx0 + S * w, by + bar_h, BAR, S)

    # 右上角通知红点（带白圈，压在底色之上）
    cx, cy = S * 0.755, S * 0.255
    fill_circle(buf, cx, cy, S * 0.205, RING, S)
    fill_circle(buf, cx, cy, S * 0.155, DOT, S)

    # 下采样
    out = []
    n = SS * SS
    for y in range(size):
        row = bytearray()
        for x in range(size):
            tr = tg = tb = ta = 0
            for dy in range(SS):
                sy = y * SS + dy
                for dx in range(SS):
                    R, G, B, A = buf[sy][x * SS + dx]
                    tr += R * A
                    tg += G * A
                    tb += B * A
                    ta += A
            if ta == 0:
                row += bytes((0, 0, 0, 0))
            else:
                row += bytes((tr // ta, tg // ta, tb // ta, ta // n))
        out.append(bytes(row))
    return out


def png_bytes(size, rows):
    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + row for row in rows)
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico_bytes(entries):
    """entries: [(size, png_bytes)] ；Vista+ 支持 PNG 压缩的 ICO 条目。"""
    count = len(entries)
    header = struct.pack("<HHH", 0, 1, count)
    offset = 6 + 16 * count
    directory = b""
    data = b""
    for size, png in entries:
        dim = 0 if size >= 256 else size
        directory += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), offset)
        data += png
        offset += len(png)
    return header + directory + data


def main():
    os.makedirs(ASSETS, exist_ok=True)

    tray = png_bytes(32, render(32))
    with open(os.path.join(ASSETS, "tray.png"), "wb") as f:
        f.write(tray)

    icon = png_bytes(256, render(256))
    with open(os.path.join(ASSETS, "icon.png"), "wb") as f:
        f.write(icon)

    # electron-builder 要求 ico 内必须含 256x256 条目，否则打包直接报错
    entries = [
        (size, png_bytes(size, render(size))) for size in (256, 128, 64, 48, 32, 16)
    ]
    with open(os.path.join(ASSETS, "icon.ico"), "wb") as f:
        f.write(ico_bytes(entries))

    # 托盘图标内联为 base64，打包后不依赖资源路径
    b64 = base64.b64encode(tray).decode("ascii")
    ts = os.path.join(ROOT, "src", "main", "icon-data.ts")
    with open(ts, "w", encoding="utf-8") as f:
        f.write(
            "/** 自动生成：由 tools/gen-icons.py 产出，请勿手工编辑。 */\n"
            "export const TRAY_ICON_PNG_BASE64 =\n"
            f"  '{b64}';\n"
        )

    print("tray.png  ", len(tray), "bytes")
    print("icon.png  ", len(icon), "bytes")
    print("icon.ico  ", os.path.getsize(os.path.join(ASSETS, "icon.ico")), "bytes")
    print("icon-data.ts written,", len(b64), "chars")


if __name__ == "__main__":
    main()
