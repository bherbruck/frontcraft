"""Samples an SRTM 1 arc-second .hgt tile onto a square grid in metres, for a map's survey.

scripts/survey.py hgt lat0 lon0 lat lon size_m cells sigma_m out.txt

The grid is centred on lat/lon, `cells` across, rows from the north-west corner, `cells + 1` heights to a
row, in decimetres, as whitespace-separated integers. The tile is blurred by a Gaussian of `sigma_m` first,
which takes out single-pixel noise and the steps that void filling leaves.
"""
import math, struct, sys

path, lat0, lon0 = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
lat, lon, size, cells, sigma = float(sys.argv[4]), float(sys.argv[5]), float(sys.argv[6]), int(sys.argv[7]), float(sys.argv[8])
out = sys.argv[9]
N = 3601
raw = open(path, "rb").read()
if len(raw) != N * N * 2:
    sys.exit("not a 1 arc-second tile")
mlat = 111320.0 / 3600
mlon = 111320.0 * math.cos(math.radians(lat)) / 3600
half = size / 2
r_mid = (lat0 + 1 - lat) * 3600
c_mid = (lon - lon0) * 3600
margin = int(4 * sigma / min(mlat, mlon)) + 4
r0 = int(r_mid - half / mlat) - margin
r1 = int(r_mid + half / mlat) + margin
c0 = int(c_mid - half / mlon) - margin
c1 = int(c_mid + half / mlon) + margin
if r0 < 0 or c0 < 0 or r1 >= N or c1 >= N:
    sys.exit("window runs off the tile")
H, W = r1 - r0 + 1, c1 - c0 + 1
patch = [[float(struct.unpack_from(">h", raw, ((r0 + j) * N + c0 + i) * 2)[0]) for i in range(W)] for j in range(H)]
# Voids are -32768 in SRTM. None should be left in a filled product, but never let one through.
for row in patch:
    for i, v in enumerate(row):
        if v < -1000:
            row[i] = 0.0

def blur(grid, s_rows, s_cols):
    def kernel(s):
        n = max(1, int(3 * s))
        k = [math.exp(-0.5 * (d / s) ** 2) if s > 0 else (1.0 if d == 0 else 0.0) for d in range(-n, n + 1)]
        t = sum(k)
        return [x / t for x in k], n
    kr, nr = kernel(s_rows)
    kc, nc = kernel(s_cols)
    h, w = len(grid), len(grid[0])
    tmp = [[sum(kc[d + nc] * grid[j][min(w - 1, max(0, i + d))] for d in range(-nc, nc + 1)) for i in range(w)] for j in range(h)]
    return [[sum(kr[d + nr] * tmp[min(h - 1, max(0, j + d))][i] for d in range(-nr, nr + 1)) for i in range(w)] for j in range(h)]

smooth = blur(patch, sigma / mlat, sigma / mlon) if sigma > 0 else patch

def sample(x, z):
    r = r_mid + z / mlat - r0
    c = c_mid + x / mlon - c0
    j, i = int(r), int(c)
    fr, fc = r - j, c - i
    g = smooth
    return (g[j][i] * (1 - fc) + g[j][i + 1] * fc) * (1 - fr) + (g[j + 1][i] * (1 - fc) + g[j + 1][i + 1] * fc) * fr

step = size / cells
rows = []
for j in range(cells + 1):
    rows.append([round(sample(-half + i * step, -half + j * step) * 10) for i in range(cells + 1)])
with open(out, "w") as f:
    for row in rows:
        f.write(" ".join(str(v) for v in row) + "\n")
flat = [v for row in rows for v in row]
print(out, f"{cells + 1}x{cells + 1}", "min", min(flat) / 10, "max", max(flat) / 10)

