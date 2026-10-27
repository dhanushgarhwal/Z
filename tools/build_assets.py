#!/usr/bin/env python3
"""Regenerates every visual asset for Z from code: logo, icons, banners
and the and the subsetted UI font.  Requires: pip install pillow fonttools
Usage: python tools/build_assets.py [path/to/folder/containing/Poppins-*.ttf]
The logo mark is flat fills only: no strokes, outlines, gradients or text."""
import sys, os, io, struct
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
A = lambda *p: os.path.join(ROOT, *p)

VIOLET = (108, 71, 255)      # logo tile
INK    = (20, 16, 43)        # banner background
INK_2  = (30, 25, 66)        # banner silhouette
TXT    = (193, 197, 206)     # #c1c5ce: the only "light" colour in the product (replaces white everywhere)

# Brand gradient (installer + app background): three stops, top-left -> bottom-right.
GRAD   = [(44, 28, 112), (28, 20, 72), (14, 11, 34)]
HEADER = GRAD[1]             # installer header strip / small wizard image

def z_polygon(L=.27, T=.27, R=.73, B=.73, t=.118, k=.17):
    return [(L,T),(R,T),(R,T+t),(L+k,B-t),(R,B-t),(R,B),(L,B),(L,B-t),(R-k,T+t),(L,T+t)]

def mask_for(poly, size, ss=4, soften=0.012):
    """Anti-aliased filled polygon mask with gently rounded corners (blur + threshold)."""
    S = size * ss
    m = Image.new("L", (S, S), 0)
    ImageDraw.Draw(m).polygon([(x*S, y*S) for x, y in poly], fill=255)
    r = soften * S
    if r > 0:
        m = m.filter(ImageFilter.GaussianBlur(r)).point(lambda v: 255 if v > 127 else 0)
    return m.resize((size, size), Image.LANCZOS)

def rounded_tile_mask(size, radius=.225, ss=4):
    S = size * ss
    m = Image.new("L", (S, S), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, S-1, S-1], radius=radius*S, fill=255)
    return m.resize((size, size), Image.LANCZOS)

def logo_tuned(s, L=.27, T=.27, R=.73, B=.73, t=.118, k=.17):
    img = Image.new("RGBA", (s, s), (0,0,0,0))
    img.paste(Image.new("RGBA", (s, s), VIOLET + (255,)), (0,0), rounded_tile_mask(s))
    img.paste(Image.new("RGBA", (s, s), TXT + (255,)), (0,0), mask_for(z_polygon(L,T,R,B,t,k), s))
    return img

def logo(s): return logo_tuned(s)

def small_logo(s):
    """Bolder, larger Z at tiny sizes so it stays legible."""
    if s <= 32: return logo_tuned(s, .20, .20, .80, .80, .17, .25)
    if s <= 64: return logo_tuned(s, .24, .24, .76, .76, .14, .20)
    return logo(s)

def logo_svg():
    pts = " ".join(f"{x*1024:.1f},{y*1024:.1f}" for x, y in z_polygon())
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="1024" height="1024">\n'
            '<rect width="1024" height="1024" rx="230" fill="#6C47FF"/>\n'
            f'<polygon points="{pts}" fill="#C1C5CE"/>\n</svg>\n')

def save_ico(path, render, sizes):
    """PNG-compressed multi-size .ico; every size rendered natively so small sizes stay crisp."""
    blobs = []
    for s in sizes:
        b = io.BytesIO(); render(s).save(b, "PNG", optimize=True); blobs.append((s, b.getvalue()))
    head = struct.pack("<HHH", 0, 1, len(blobs)); off = 6 + 16*len(blobs); body = b""; ent = b""
    for s, data in blobs:
        d = 0 if s >= 256 else s
        ent += struct.pack("<BBBBHHII", d, d, 0, 0, 1, 32, len(data), off + len(body)); body += data
    open(path, "wb").write(head + ent + body)

def file_icon(s):
    """Archive document icon: page with a folded corner and the Z mark. Flat fills only."""
    S = s*4
    x0, y0, x1, y1, fold = .17*S, .06*S, .83*S, .94*S, .26*S
    def poly_mask(pts, blur):
        m = Image.new("L", (S, S), 0); ImageDraw.Draw(m).polygon(pts, fill=255)
        return m.filter(ImageFilter.GaussianBlur(blur*S)).point(lambda v: 255 if v > 127 else 0)
    page = poly_mask([(x0,y0),(x1-fold,y0),(x1,y0+fold),(x1,y1),(x0,y1)], .012)
    dog  = poly_mask([(x1-fold,y0),(x1,y0+fold),(x1-fold,y0+fold)], .008)
    img = Image.new("RGBA", (S, S), (0,0,0,0))
    img.paste(Image.new("RGBA", (S, S), VIOLET + (255,)), (0,0), page)
    img.paste(Image.new("RGBA", (S, S), (70, 40, 200, 255)), (0,0), dog)
    zm = mask_for(z_polygon(.34, .42, .66, .78, .09, .12), S, ss=1, soften=.008)
    img.paste(Image.new("RGBA", (S, S), TXT + (255,)), (0,0), zm)
    return img.resize((s, s), Image.LANCZOS)

def silhouette(w, h, side, ox, oy, color, alpha=255):
    """Giant decorative Z in a tone-on-tone fill, cropped by the canvas."""
    side = int(side)
    m = mask_for(z_polygon(0,0,1,1,.2,.30), side, ss=2, soften=.01)
    layer = Image.new("RGBA", (w, h), (0,0,0,0))
    layer.paste(Image.new("RGBA", (side, side), color + (alpha,)), (int(ox), int(oy)), m)
    return layer

def twin_bands(w, h, side, ox, oy, color, alpha=255):
    """The diagonal stroke entering at the top-right, plus an identical copy entering at the
    bottom-left (same angle, same direction: a pure point reflection of the layout)."""
    a = silhouette(w, h, side, ox, oy, color, alpha)
    return Image.alpha_composite(a, a.rotate(180))

def diagonal_band(w, h, x_top, width, color, alpha=255, ss=2):
    """A straight band at the exact angle of the Z's diagonal stroke (dx/dy = -0.7/0.6),
    crossing the canvas top edge at x_top.  Used where the full Z silhouette would not fit."""
    slope = -0.7 / 0.6
    y0, y1 = -h * 0.2, h * 1.2
    xs = lambda y: x_top + slope * y
    pts = [(xs(y0) - width/2, y0), (xs(y0) + width/2, y0), (xs(y1) + width/2, y1), (xs(y1) - width/2, y1)]
    m = Image.new("L", (w*ss, h*ss), 0)
    ImageDraw.Draw(m).polygon([(x*ss, y*ss) for x, y in pts], fill=255)
    m = m.resize((w, h), Image.LANCZOS)
    layer = Image.new("RGBA", (w, h), (0,0,0,0)); layer.paste(Image.new("RGBA", (w, h), color + (alpha,)), (0,0), m)
    return layer

def gradient(w, h, stops=GRAD):
    """Diagonal multi-stop gradient, top-left -> bottom-right."""
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    t = np.clip((x / max(w - 1, 1)) * 0.45 + (y / max(h - 1, 1)) * 0.55, 0, 1)
    seg = t * (len(stops) - 1); i = np.minimum(seg.astype(int), len(stops) - 2); f = (seg - i)[..., None]
    c = np.array(stops, dtype=np.float32)
    out = c[i] * (1 - f) + c[i + 1] * f
    return Image.fromarray(np.rint(out).astype(np.uint8), "RGB").convert("RGBA")

def wizard_large(w, h):
    img = gradient(w, h)
    a = diagonal_band(w, h, w*1.25, w*0.30, (150, 120, 255), 26)
    img.alpha_composite(Image.alpha_composite(a, a.rotate(180)))
    lw = int(w*0.52); img.alpha_composite(logo(lw), ((w-lw)//2, int(h*0.10)))
    return img.convert("RGB")

def wizard_small(w, h):
    img = Image.new("RGBA", (w, h), HEADER + (255,)); s = min(w, h)
    img.alpha_composite(small_logo(s), ((w-s)//2, (h-s)//2))
    return img.convert("RGB")

def wide_banner(w, h):
    img = Image.new("RGBA", (w, h), INK + (255,))
    img.alpha_composite(twin_bands(w, h, h*1.9, w*0.60, -h*0.45, INK_2))
    lw = int(h*0.46); img.alpha_composite(logo(lw), ((w-lw)//2, (h-lw)//2))
    return img.convert("RGB")

def build_font(src_dir):
    from fontTools import subset
    from fontTools.ttLib import TTFont
    cps = (list(range(0x20,0x7F)) + list(range(0xA0,0x180)) + list(range(0x370,0x400)) + list(range(0x400,0x530))
           + list(range(0x2010,0x2030)) + [0x2022,0x2026,0x20AC,0x20B9,0x2190,0x2191,0x2192,0x2193,0x2713,0x00D7,0x2212])
    for style, out in (("Regular", "ZSans-Regular.ttf"), ("Medium", "ZSans-Medium.ttf")):
        o = subset.Options(); o.layout_features = ["kern","liga","calt"]; o.name_IDs = ["*"]
        o.notdef_outline = True; o.hinting = False
        f = TTFont(os.path.join(src_dir, f"Poppins-{style}.ttf"))
        s = subset.Subsetter(o); s.populate(unicodes=cps); s.subset(f)
        # Rebrand as "Z Sans" (allowed: Poppins declares no Reserved Font Name under OFL 1.1)
        for rec in list(f["name"].names):
            if   rec.nameID in (1, 16): rec.string = "Z Sans"
            elif rec.nameID in (2, 17): rec.string = style
            elif rec.nameID == 3:       rec.string = f"Z Sans {style}; derived from Poppins"
            elif rec.nameID == 4:       rec.string = f"Z Sans {style}"
            elif rec.nameID == 6:       rec.string = f"ZSans-{style}"
        f.save(A("assets/font", out)); print(out, os.path.getsize(A("assets/font", out)), "bytes")

def main():
    logo(1024).save(A("assets/icons/z.png"), optimize=True)
    for s in (512, 256, 128, 64, 32): small_logo(s).save(A(f"assets/icons/z-{s}.png"), optimize=True)
    open(A("assets/icons/z.svg"), "w").write(logo_svg())
    save_ico(A("assets/icons/z.ico"), small_logo, (16,20,24,32,40,48,64,96,128,256))
    save_ico(A("assets/icons/z-archive.ico"), file_icon, (16,24,32,48,64,128,256))
    wide_banner(1280, 640).save(A("assets/banners/social-1280x640.png"), optimize=True)
    wide_banner(1920, 480).save(A("assets/banners/readme-1920x480.png"), optimize=True)
    src = sys.argv[1] if len(sys.argv) > 1 else "/usr/share/fonts/truetype/google-fonts"
    if os.path.exists(os.path.join(src, "Poppins-Regular.ttf")): build_font(src)
    else: print("font subset skipped (Poppins sources not found); existing assets/font kept")
    # raw RGBA used for the window / taskbar icon at runtime
    open(A("assets/icons/z-64.rgba"), "wb").write(small_logo(64).tobytes())
    print("assets ok")

if __name__ == "__main__": main()
