import os, sys
sys.path.insert(0, os.path.dirname(__file__))
from gen import KEY, RING1, RING2, RING_SMALL

ACCENT, SURFACE, GROUND, BORDER = "#FFB020", "#181512", "#0D0B09", "#2C2722"
D = "/home/bassamalim/Projects/echo-desktop/assets"
A = "/home/bassamalim/Projects/echo-android/app/src/main/res/drawable"

def svg_full(color=ACCENT):
    return (f'<path d="{RING2}" fill="{color}" fill-rule="evenodd" opacity="0.3"/>'
            f'<path d="{RING1}" fill="{color}" fill-rule="evenodd" opacity="0.6"/>'
            f'<path d="{KEY}" fill="{color}"/>')
def svg_small(color=ACCENT):
    return (f'<path d="{RING_SMALL}" fill="{color}" fill-rule="evenodd" opacity="0.5"/>'
            f'<path d="{KEY}" fill="{color}"/>')

HEAD = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}">'
open(f"{D}/logo.svg", "w").write(
    HEAD.format(vb="0 0 132 132") +
    f'<rect width="132" height="132" rx="30" fill="{SURFACE}"/>' + svg_full() + "</svg>\n")
open(f"{D}/mark.svg", "w").write(HEAD.format(vb="0 0 132 132") + svg_full() + "</svg>\n")
open(f"{D}/mark-small.svg", "w").write(HEAD.format(vb="14 14 104 104") + svg_small() + "</svg>\n")

# ---- Android vector drawables ----
def vd(size, viewport, body, comment):
    return (f'<!-- {comment} -->\n'
            f'<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
            f'    android:width="{size}dp" android:height="{size}dp"\n'
            f'    android:viewportWidth="{viewport}" android:viewportHeight="{viewport}">\n{body}</vector>\n')
def p(d, color, alpha=None, even=False, indent="    "):
    s = f'{indent}<path android:pathData="{d}"\n{indent}    android:fillColor="{color}"'
    if alpha is not None: s += f'\n{indent}    android:fillAlpha="{alpha}"'
    if even: s += f'\n{indent}    android:fillType="evenOdd"'
    return s + " />\n"
def full(color, indent="    "):
    return p(RING2, color, 0.3, True, indent) + p(RING1, color, 0.6, True, indent) + p(KEY, color, None, False, indent)
def small(color, indent="    "):
    return p(RING_SMALL, color, 0.5, True, indent) + p(KEY, color, None, False, indent)

open(f"{A}/ic_echo_mark.xml", "w").write(vd(48, 132, full("#FFFFFFFF"),
    "The Echo mark: a keyhole and the two echoes rippling out from it. Tinted in code; the rings keep their fade."))
# viewport cropped to the mark, via a group, so the one-ring version fills its box
small_body = ('    <group android:translateX="-14" android:translateY="-14">\n' + small("#FFFFFFFF", "        ") + "    </group>\n")
open(f"{A}/ic_echo_mark_small.xml", "w").write(vd(24, 104, small_body,
    "The Echo mark for small sizes (app bar, autofill rows): the keyhole and one echo."))

# Launcher: 108 canvas, the inner 72 is what shows. The mark is 109 tall in its 132 box; 0.42 puts it at ~46dp.
S = 0.42; T = 54 - 66 * S
def launcher(color):
    return (f'    <group android:scaleX="{S}" android:scaleY="{S}" android:translateX="{T:.2f}" android:translateY="{T:.2f}">\n'
            + full(color, "        ") + "    </group>\n")
open(f"{A}/ic_launcher_foreground.xml", "w").write(vd(108, 108, launcher("@color/accent"),
    "The Echo mark on the 108dp adaptive-icon canvas, kept inside the 66dp safe zone."))
open(f"{A}/ic_launcher_monochrome.xml", "w").write(vd(108, 108, launcher("#FFFFFFFF"),
    "The Echo mark for themed icons: the system supplies the colour, the rings keep their fade."))
open(f"{A}/ic_launcher_background.xml", "w").write(vd(108, 108,
    '    <path android:fillColor="@color/surface_100" android:pathData="M0,0 H108 V108 H0 Z" />\n',
    "The launcher tile: the app's card surface."))
print("written")
