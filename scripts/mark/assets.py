"""Writes every Rune mark asset: the desktop SVGs and raster, and the Android vector drawables.

The mark is ᚱ (Raido), the Elder Futhark R: a stave, a bowl drawn as two strokes, and a leg.
Run from anywhere; rune-android is expected next to this repo. Needs ImageMagick for the raster.
"""
import os, subprocess

ACCENT, SURFACE = "#FFB020", "#181512"
RUNE, WIDTH = "M50,28 V104 M50,28 L82,48 L50,68 L84,104", 12   # in a 132 box, centred
HERE = os.path.dirname(os.path.abspath(__file__))
D = os.path.join(HERE, "../../assets")
A = os.path.join(HERE, "../../../rune-android/app/src/main/res/drawable")

def svg(body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 132 132">{body}</svg>\n'
glyph = (f'<path d="{RUNE}" fill="none" stroke="{ACCENT}" stroke-width="{WIDTH}" '
         f'stroke-linecap="round" stroke-linejoin="round"/>')
open(f"{D}/logo.svg", "w").write(svg(f'<rect width="132" height="132" rx="30" fill="{SURFACE}"/>' + glyph))
open(f"{D}/mark.svg", "w").write(svg(glyph))
# the in-app mark for egui, which can't draw SVG: raw RGBA, mipmapped at load
subprocess.run(["magick", "-background", "none", "-density", "384", f"{D}/mark.svg",
                "-resize", "192x192", "-depth", "8", f"rgba:{D}/mark-192.rgba"], check=True)

# ---- Android vector drawables ----
def vd(size, viewport, body, comment):
    return (f'<!-- {comment} -->\n'
            f'<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
            f'    android:width="{size}dp" android:height="{size}dp"\n'
            f'    android:viewportWidth="{viewport}" android:viewportHeight="{viewport}">\n{body}</vector>\n')
def rune(color, indent="    "):
    return (f'{indent}<path android:pathData="{RUNE}"\n'
            f'{indent}    android:strokeColor="{color}"\n'
            f'{indent}    android:strokeWidth="{WIDTH}"\n'
            f'{indent}    android:strokeLineCap="round"\n'
            f'{indent}    android:strokeLineJoin="round" />\n')

open(f"{A}/ic_rune_mark.xml", "w").write(vd(48, 132, rune("#FFFFFFFF"),
    "The Rune mark: ᚱ, the R rune. Tinted in code."))

# Launcher: 108 canvas, the inner 72 is what shows. The rune is 88 tall in its 132 box; 0.5 puts it at 44dp.
S = 0.5; T = 54 - 66 * S
def launcher(color):
    return (f'    <group android:scaleX="{S}" android:scaleY="{S}" android:translateX="{T:g}" android:translateY="{T:g}">\n'
            + rune(color, "        ") + "    </group>\n")
open(f"{A}/ic_launcher_foreground.xml", "w").write(vd(108, 108, launcher("@color/accent"),
    "The Rune mark on the 108dp adaptive-icon canvas, kept inside the 66dp safe zone."))
open(f"{A}/ic_launcher_monochrome.xml", "w").write(vd(108, 108, launcher("#FFFFFFFF"),
    "The Rune mark for themed icons: the system supplies the colour."))
open(f"{A}/ic_launcher_background.xml", "w").write(vd(108, 108,
    '    <path android:fillColor="@color/surface_100" android:pathData="M0,0 H108 V108 H0 Z" />\n',
    "The launcher tile: the app's card surface."))
print("written")
