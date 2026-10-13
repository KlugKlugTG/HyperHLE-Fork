#!/usr/bin/env python3
"""Generate the device-front ("bezel") overlay PNGs used by the
"Device frame" setting (see src/device_frame.rs).

Each image is a portrait-orientation RGBA PNG of an iPhone front with a
fully transparent rectangle where the screen is.  The screen hole rect
(x, y, w, h) in image pixels is part of the layout table below and must
stay in sync with `HOLE_RECT` in src/device_frame.rs.

Run:  python3 dev-scripts/gen_bezels.py   (needs Pillow)
Output: res/bezels/<model>.png
"""

import os

from PIL import Image, ImageChops, ImageDraw

OUT = os.path.join(os.path.dirname(__file__), "..", "res", "bezels")
SS = 2  # supersampling factor for anti-aliasing

# hole width/height per screen aspect
H32 = (960, 640)    # 3:2  (iPhone .. 4s)
H568 = (1136, 640)  # 16:9 (iPhone 5 / 5c)

# model: hole, side/top/bottom margins, corner radius, rim colours,
# earpiece size, front camera?, home button diameter/glyph/colour
MODELS = {
    "iphone": dict(hole=H32, side=62, top=104, bottom=176, radius=95,
                   rim=(216, 216, 220), rim_dark=(120, 120, 126),
                   ear=(150, 26), cam=False, home=112, glyph=True,
                   home_fill=(10, 10, 12), home_ring=(70, 70, 74)),
    "iphone_3g": dict(hole=H32, side=62, top=104, bottom=176, radius=95,
                      rim=(70, 70, 74), rim_dark=(38, 38, 42),
                      ear=(150, 26), cam=False, home=112, glyph=True,
                      home_fill=(10, 10, 12), home_ring=(58, 58, 62)),
    "iphone_3gs": dict(hole=H32, side=62, top=104, bottom=176, radius=95,
                       rim=(216, 216, 220), rim_dark=(120, 120, 126),
                       ear=(150, 26), cam=False, home=112, glyph=True,
                       home_fill=(10, 10, 12), home_ring=(70, 70, 74)),
    "iphone_4": dict(hole=H32, side=54, top=92, bottom=152, radius=64,
                     rim=(150, 154, 158), rim_dark=(84, 88, 92),
                     ear=(120, 20), cam=True, home=98, glyph=True,
                     home_fill=(10, 10, 12), home_ring=(64, 66, 70)),
    "iphone_4s": dict(hole=H32, side=54, top=92, bottom=152, radius=64,
                      rim=(150, 154, 158), rim_dark=(84, 88, 92),
                      ear=(120, 20), cam=True, home=98, glyph=True,
                      home_fill=(10, 10, 12), home_ring=(64, 66, 70)),
    "iphone_5": dict(hole=H568, side=50, top=82, bottom=142, radius=58,
                     rim=(120, 124, 130), rim_dark=(70, 74, 80),
                     ear=(110, 18), cam=True, home=94, glyph=True,
                     home_fill=(10, 10, 12), home_ring=(60, 62, 66)),
    "iphone_5c": dict(hole=H568, side=54, top=84, bottom=146, radius=64,
                      rim=(48, 168, 224), rim_dark=(24, 116, 168),
                      ear=(110, 18), cam=True, home=94, glyph=False,
                      home_fill=(40, 152, 206), home_ring=(96, 200, 240)),
}


def rounded_mask(size, radius):
    m = Image.new("L", size, 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], radius=radius,
                        fill=255)
    return m


def vertical_gradient(size, top, bottom):
    """Linear vertical gradient between two RGB colours."""
    w, h = size
    px = bytearray()
    for y in range(h):
        t = y / max(1, h - 1)
        c = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(3))
        px.extend(bytes(c) * w)
    return Image.frombytes("RGB", (w, h), bytes(px))


def sheen(size, body_mask, hole_rect):
    """Diagonal glass reflection, masked to the body minus the screen."""
    w, h = size
    px = bytearray()
    # direction of the gradient (top-left towards bottom-right)
    import math
    ang = math.radians(24)
    dx, dy = math.cos(ang), math.sin(ang)
    span = w * dx + h * dy
    for y in range(h):
        for x in range(w):
            t = (x * dx + y * dy) / span
            # bright band over the first ~45% then fading out
            a = max(0.0, 1.0 - abs(t - 0.18) / 0.34)
            a = a * a * 26
            px.append(int(a))
    grad = Image.frombytes("L", (w, h), bytes(px))
    cut = Image.new("L", (w, h), 255)
    dc = ImageDraw.Draw(cut)
    dc.rectangle(hole_rect, fill=0)
    mask = ImageChops.multiply(grad, body_mask)
    mask = ImageChops.multiply(mask, cut)
    white = Image.new("RGBA", (w, h), (255, 255, 255, 0))
    white.putalpha(mask)
    return white


def generate(name, spec):
    hw, hh = spec["hole"]
    side, top, bottom = spec["side"], spec["top"], spec["bottom"]
    w = hw + 2 * side
    h = hh + top + bottom
    size = (w * SS, h * SS)
    hole = (side * SS, top * SS, (side + hw) * SS, (top + hh) * SS)

    body = rounded_mask(size, spec["radius"] * SS)
    # rim ring: the body shape inset by the metal band width
    band = 4 * SS
    inner_mask = Image.new("L", size, 0)
    ImageDraw.Draw(inner_mask).rounded_rectangle(
        [band, band, size[0] - 1 - band, size[1] - 1 - band],
        radius=max(1, spec["radius"] * SS - band), fill=255)

    # body = rim colour gradient, inner = black glass gradient
    rim_img = vertical_gradient(size, spec["rim"], spec["rim_dark"])
    glass_img = vertical_gradient(size, (30, 30, 33), (4, 4, 6))
    img = Image.composite(glass_img, rim_img, inner_mask).convert("RGBA")

    draw = ImageDraw.Draw(img, "RGBA")

    # earpiece slot, centred in the top margin
    ew, eh = spec["ear"][0] * SS, spec["ear"][1] * SS
    ex0 = (w * SS - ew) // 2
    ey0 = top * SS // 2 - eh // 2
    draw.rounded_rectangle([ex0, ey0, ex0 + ew, ey0 + eh], radius=eh // 2,
                           fill=(16, 17, 19, 255))
    draw.rounded_rectangle([ex0 + 2 * SS, ey0 + 2 * SS,
                            ex0 + ew - 2 * SS, ey0 + eh - 2 * SS],
                           radius=eh // 3, fill=(34, 36, 39, 255))

    # front camera lens, left of the earpiece
    if spec["cam"]:
        cr = 11 * SS
        cx = ex0 - 34 * SS
        cy = ey0 + eh // 2
        draw.ellipse([cx - cr, cy - cr, cx + cr, cy + cr],
                     fill=(12, 14, 18, 255))
        draw.ellipse([cx - cr // 2, cy - cr // 2, cx + cr // 2, cy + cr // 2],
                     fill=(24, 30, 42, 255))
        draw.ellipse([cx - cr // 5, cy - cr // 5 - cr // 6,
                      cx + cr // 5, cy + cr // 5 - cr // 6],
                     fill=(120, 140, 170, 160))

    # home button, centred in the bottom margin
    hd = spec["home"] * SS
    hx = (w * SS) // 2
    hy = (top + hh) * SS + bottom * SS // 2
    draw.ellipse([hx - hd // 2, hy - hd // 2, hx + hd // 2, hy + hd // 2],
                 fill=spec["home_fill"] + (255,),
                 outline=spec["home_ring"] + (255,), width=2 * SS)
    if spec["glyph"]:
        g = hd // 3
        draw.rounded_rectangle([hx - g // 2, hy - g // 2,
                                hx + g // 2, hy + g // 2],
                               radius=g // 4, outline=(72, 74, 78, 255),
                               width=2 * SS)

    # glass reflection over the body (never over the screen hole)
    refl = sheen(size, body, hole)
    img = Image.alpha_composite(img.convert("RGBA"), refl)

    # clip everything to the body shape and punch out the screen hole
    img.putalpha(ImageChops.multiply(img.getchannel("A"), body))
    down = img.resize((w, h), Image.LANCZOS)
    d2 = ImageDraw.Draw(down)
    d2.rectangle((side, top, side + hw - 1, top + hh - 1), fill=(0, 0, 0, 0))
    return down


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, spec in MODELS.items():
        img = generate(name, spec)
        path = os.path.join(OUT, name + ".png")
        img.save(path, optimize=True)
        hw, hh = spec["hole"]
        print("%-10s %4dx%-4d hole=(%d, %d, %d, %d)" %
              (name, img.width, img.height, spec["side"], spec["top"],
               hw, hh))


if __name__ == "__main__":
    main()
