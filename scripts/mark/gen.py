"""Echo mark geometry: a keyhole and evenly spaced outlines around it, as real paths.

The keyhole is a circle joined to a trapezoid. An outline at distance D from it is the circle
grown by D, plus the trapezoid's sides pushed out by D with rounded bottom corners.
"""
import math, os, sys

CX, CY, R = 66.0, 52.5, 15.0
A, B = (73.5, 64.5), (79.0, 94.5)          # right side of the stem, top to bottom
AL, BL = (58.5, 64.5), (53.0, 94.5)        # left side (mirror)

def outline(D):
    """Closed path at distance D outside the raw keyhole shape."""
    ux, uy = B[0]-A[0], B[1]-A[1]; L = math.hypot(ux, uy); ux, uy = ux/L, uy/L
    nx, ny = uy, -ux                         # outward normal on the right side
    px, py = A[0]+D*nx, A[1]+D*ny            # a point on the pushed-out right side
    # where that line leaves the grown circle, going down
    fx, fy = px-CX, py-CY
    b = fx*ux + fy*uy; c = fx*fx + fy*fy - (R+D)**2
    t = -b + math.sqrt(b*b - c)
    rx, ry = px+t*ux, py+t*uy                # right join point
    lx, ly = 2*CX-rx, ry                     # left join point (mirror)
    brx, bry = B[0]+D*nx, B[1]+D*ny          # end of right side
    blx, bly = 2*CX-brx, bry
    f = lambda v: f"{v:.2f}".rstrip('0').rstrip('.')
    Rg = f(R+D); Dg = f(D)
    return (f"M{f(lx)},{f(ly)} A{Rg},{Rg} 0 1,1 {f(rx)},{f(ry)} L{f(brx)},{f(bry)} "
            f"A{Dg},{Dg} 0 0,1 {f(B[0])},{f(B[1]+D)} L{f(BL[0])},{f(BL[1]+D)} "
            f"A{Dg},{Dg} 0 0,1 {f(blx)},{f(bly)} Z")

KEY = outline(1.5)
RING1 = outline(14) + " " + outline(8.5)     # even-odd: the band between the two
RING2 = outline(26) + " " + outline(21)
RING_SMALL = outline(19.5) + " " + outline(10.5)

if __name__ == "__main__":
    for name in ("KEY", "RING1", "RING2", "RING_SMALL"):
        print(name, "=", globals()[name])
