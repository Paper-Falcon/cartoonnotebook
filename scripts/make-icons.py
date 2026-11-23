"""把一张动漫截图裁成圆形头像，输出 Tauri 需要的多尺寸图标。

源图：src-tauri/icons/src/source.jpg（1048x1048 的动画截图）
做法：按人脸裁一个 660 的方框（留出头顶呆毛的余量）→ 轻微提对比/饱和
     → 圆形遮罩 → 输出 512 主图、32/128/256 与多尺寸 ico。
换图只改 SOURCE 和 CROP 两个常量即可。
"""

from PIL import Image, ImageDraw, ImageEnhance, ImageFilter
import io
import os
import struct

HERE = os.path.dirname(os.path.abspath(__file__))
ICONS = os.path.join(HERE, "..", "src-tauri", "icons")
SOURCE = os.path.join(ICONS, "src", "source.jpg")

# 裁切框（源图坐标 1080x1080）：以人脸为中心，头顶留出头发飘动的余量
CROP = (240, 170, 700, 630)
ANTIALIAS = 4

src = Image.open(SOURCE).convert("RGB").crop(CROP)
S = src.size[0]

# 截图偏暖偏灰，略微拉开对比，否则缩到 32px 会糊成一片橙色
src = ImageEnhance.Contrast(src).enhance(1.10)
src = ImageEnhance.Color(src).enhance(1.18)
src = ImageEnhance.Brightness(src).enhance(1.02)

mask = Image.new("L", (S * ANTIALIAS, S * ANTIALIAS), 0)
ImageDraw.Draw(mask).ellipse([0, 0, S * ANTIALIAS - 1, S * ANTIALIAS - 1], fill=255)
mask = mask.resize((S, S), Image.LANCZOS)

icon = Image.new("RGBA", (S, S), (0, 0, 0, 0))
icon.paste(src, (0, 0), mask)
icon = icon.resize((512, 512), Image.LANCZOS)
icon = icon.filter(ImageFilter.UnsharpMask(radius=2, percent=70, threshold=3))
icon.save(os.path.join(ICONS, "icon.png"))

for size in (32, 128, 256):
    name = "32x32.png" if size == 32 else ("128x128.png" if size == 128 else "128x128@2x.png")
    small = icon.resize((size, size), Image.LANCZOS)
    if size <= 64:
        # 小尺寸再补一道锐化，尽量保住眼睛的暗部
        small = small.filter(ImageFilter.UnsharpMask(radius=1.4, percent=80, threshold=2))
    small.save(os.path.join(ICONS, name))

ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)


def _dib_frame(img):
    """把一帧编成 ICO 里的 BMP 帧：32bpp 自下而上 + 1bpp 全零掩码。"""
    w, h = img.size
    px = img.load()
    rows = []
    for y in range(h - 1, -1, -1):
        row = bytearray()
        for x in range(w):
            r, g, b, a = px[x, y]
            row += bytes((b, g, r, a))
        rows.append(bytes(row))
    mask_stride = ((w + 31) // 32) * 4
    mask = bytes(mask_stride * h)  # 透明由 alpha 决定，掩码全 0 即可
    header = struct.pack(
        "<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, w * h * 4 + len(mask), 0, 0, 0, 0
    )
    return header + b"".join(rows) + mask


def write_ico(img, path, sizes=ICO_SIZES):
    """Pillow 13 默认把每一帧都写成 PNG，Windows 对小尺寸 PNG 帧支持很差，
    资源管理器会退回默认图标。这里自己组装：≤128 用 BMP 帧，256 用 PNG。"""
    frames = []
    for s in sizes:
        frame = img.resize((s, s), Image.LANCZOS)
        if s >= 256:
            buf = io.BytesIO()
            frame.save(buf, "PNG")
            data = buf.getvalue()
        else:
            data = _dib_frame(frame)
        frames.append((s, data))

    offset = 6 + 16 * len(frames)
    out = bytearray(struct.pack("<HHH", 0, 1, len(frames)))
    for s, data in frames:
        out += struct.pack(
            "<BBBBHHII", s if s < 256 else 0, s if s < 256 else 0, 0, 0, 1, 32, len(data), offset
        )
        offset += len(data)
    for _, data in frames:
        out += data
    with open(path, "wb") as f:
        f.write(out)


write_ico(icon, os.path.join(ICONS, "icon.ico"))

# 前端顶栏/空态也用同一个头像，复制一份到 public
public = os.path.join(HERE, "..", "public")
os.makedirs(public, exist_ok=True)
icon.resize((256, 256), Image.LANCZOS).save(os.path.join(public, "icon.png"))

print("icon written:", os.path.abspath(ICONS))
