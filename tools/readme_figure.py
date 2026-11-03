"""README hero figure: one Livermore flyover's spectrogram + CV fit, and the search heatmap it implies.

Display only: spectrogram, event fit, CV curve, tracks, DEM heights and search scores all come from
dopplerprint._core. The map photo is USGS The National Map orthoimagery (USDA NAIP, public domain), cached
under data/_downloads/hero/; the DEM hillshade is the fallback when it can't be fetched.
Run: .venv/bin/python -I tools/readme_figure.py [--case <id>] [--out <png>]
"""

import argparse
import json
import urllib.request
from datetime import datetime
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.collections import PolyCollection
from matplotlib.colors import LightSource, LinearSegmentedColormap, PowerNorm
from matplotlib.ticker import FixedLocator, NullLocator, ScalarFormatter
from mpl_toolkits.axes_grid1.anchored_artists import AnchoredSizeBar

from dopplerprint import _core

ROOT = Path(__file__).resolve().parent.parent
STORE = ROOT / "data/adsb/store"
DEM = ROOT / "data/dem/5070/livermore_30m.u16"
CACHE = ROOT / "data/_downloads/hero"
IMAGERY = ("https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/export"
           "?bbox={w},{s},{e},{n}&bboxSR=4326&imageSR=4326&size={px},{px}&format=png&f=image")
IMAGERY_PX = 1024
DEFAULT_CASE = "livermore_N9658K_R24"
REGION_M = 4_000.0
MAP_PAD_M = 300.0
F_MAX = 1_000.0
M_PER_DEG = 111_320.0

# Reference palette (dataviz skill, light mode): surface, inks, sequential blue ramp, slot-2 orange.
SURFACE, INK, INK2, MUTED = "#fcfcfb", "#0b0b0b", "#52514e", "#898781"
SEQ = LinearSegmentedColormap.from_list("seq_blue", ["#fcfcfb", "#cde2fb", "#86b6ef", "#3987e5", "#1c5cab", "#0d366b"])
ORANGE = "#eb6834"
LABEL_BOX = dict(boxstyle="round,pad=0.25", fc=SURFACE, ec="none", alpha=0.9)


def heat_cmap() -> LinearSegmentedColormap:
    """YlOrRd with alpha rising from 0, so weak cells let the photo through and hot spots stay opaque."""
    base = plt.get_cmap("YlOrRd")
    u = np.linspace(0, 1, 256)
    rgba = base(0.15 + 0.85 * u)
    rgba[:, 3] = np.clip(u * 1.6, 0, 0.9)
    return LinearSegmentedColormap.from_list("heat_fade", rgba)


def unix(iso: str) -> float:
    return datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp()


def load_case(case: str) -> tuple[dict, np.ndarray, int]:
    d = ROOT / "data/cases" / case
    truth = json.loads((d / "truth.json").read_text())
    x, sr = _core.decode(str(d / "audio/audio.wav"), f_max=F_MAX)
    return truth, x, sr


def strongest_event(x: np.ndarray, sr: int) -> dict:
    events = _core.discover(x, sr)
    if not events:
        raise SystemExit("no Doppler event discovered")
    return max(events, key=lambda e: e["snr"])


def run_search(truth: dict, x: np.ndarray, sr: int, event: dict) -> tuple[dict, list[dict]]:
    lat, lon = truth["lat"], truth["lon"]
    prior = tuple(unix(s) for s in truth["recording_start_window_utc"])
    t0, t1 = prior[0] - 900.0, prior[1] + len(x) / sr + 900.0
    dlat = 0.25
    dlon = dlat / np.cos(np.radians(lat))
    bbox = (lat - dlat, lon - dlon, lat + dlat, lon + dlon)
    res = _core.search(x, sr, [event], str(STORE), t0, t1, (lat, lon, REGION_M), prior, bbox=bbox, dem=str(DEM))
    tracks = _core.load_tracks(str(STORE), t0, t1, bbox=bbox, max_alt_m=9_000.0)
    return res, tracks


def draw_spectrogram(ax, x: np.ndarray, sr: int, event: dict) -> None:
    img, t, f = _core.log_spectrogram(x, sr, win_s=1.0, hop_s=0.25, f_min=40.0, f_max=F_MAX, whiten=False)
    lo, hi = np.percentile(img, [35, 99.7])
    ax.pcolormesh(t, f, img.T, cmap=SEQ, vmin=lo, vmax=hi, shading="nearest", rasterized=True)
    fit = event["fit"]["best"]
    ridge_t = np.asarray(event["ridge"]["t"])
    tt = np.linspace(ridge_t.min(), ridge_t.max(), 400)
    ff = _core.cv_frequency(fit["f0"], fit["v"], fit["d_cpa"], fit["t_cpa_arrive"], tt, c=fit["c"])
    ax.plot(tt, ff, color=ORANGE, lw=1.6, solid_capstyle="round")
    tc = fit["t_cpa_arrive"]
    ax.axvline(tc, color=INK2, lw=1.0, zorder=1)
    f_lo, f_hi = max(f[0], fit["f0"] / 3), min(f[-1], fit["f0"] * 3)
    ax.text(tc + 1.5, f_hi * 0.94, f"closest approach\n{fit['v'] * 3.6:.0f} km/h, {fit['d_cpa'] / 1000:.1f} km away",
            fontsize=10, color=INK, ha="left", va="top", bbox=LABEL_BOX)
    ax.set_yscale("log")
    ax.yaxis.set_major_locator(FixedLocator([50, 100, 200, 500, 1000]))
    ax.yaxis.set_major_formatter(ScalarFormatter())
    ax.yaxis.set_minor_locator(NullLocator())
    ax.set_ylim(f_lo, f_hi)
    ax.set_xlim(t[0], t[-1])
    ax.set_xlabel("time in recording (s)")
    ax.set_ylabel("frequency (Hz)")


def fetch_imagery(extent: tuple[float, float, float, float]) -> np.ndarray | None:
    """Orthophoto for exactly `extent` (w, e, s, n) in EPSG:4326, cached; None if unavailable."""
    w, e, s, n = extent
    path = CACHE / f"usgs_{w:.6f}_{s:.6f}_{e:.6f}_{n:.6f}_{IMAGERY_PX}.png"
    if not path.exists():
        try:
            with urllib.request.urlopen(IMAGERY.format(w=w, s=s, e=e, n=n, px=IMAGERY_PX), timeout=60) as r:
                body = r.read()
        except OSError as err:
            print(f"imagery fetch failed ({err}); using hillshade")
            return None
        if not body.startswith(b"\x89PNG\r\n\x1a\n"):
            print("imagery response is not a PNG; using hillshade")
            return None
        CACHE.mkdir(parents=True, exist_ok=True)
        path.write_bytes(body)
    img = plt.imread(path)
    return img if img.ndim == 3 and img.shape[:2] == (IMAGERY_PX, IMAGERY_PX) else None


def draw_background(ax, extent: tuple[float, float, float, float]) -> str:
    photo = fetch_imagery(extent)
    if photo is not None:
        ax.imshow(photo, extent=extent, zorder=0, interpolation="lanczos")
        return "imagery"
    lon = np.linspace(extent[0], extent[1], 500)
    lat = np.linspace(extent[3], extent[2], 500)
    LON, LAT = np.meshgrid(lon, lat)
    h = _core.Dem(str(DEM)).height_msl_latlon(LAT, LON)
    h = np.where(np.isfinite(h), h, np.nanmean(h))
    shade = LightSource(azdeg=315, altdeg=45).hillshade(h, vert_exag=3, dx=30, dy=30)
    ax.imshow(shade, extent=extent, cmap="gray", vmin=-0.6, vmax=1.4, alpha=0.55, zorder=0, interpolation="bilinear")
    return "hillshade"


def cell_poly(c: dict) -> np.ndarray:
    ix, iy, s = c["cell"]["ix"], c["cell"]["iy"], c["cell"]["step_m"]
    x = np.array([ix, ix + 1, ix + 1, ix], float) * s
    y = np.array([iy, iy, iy + 1, iy + 1], float) * s
    la, lo = _core.albers_inverse(x, y)
    return np.column_stack([lo, la])


def draw_heatmap(ax, ranked: list[dict]):
    """Coarse cells first, refined children on top, so detail shows where the search zoomed in."""
    cells = sorted(ranked, key=lambda c: -c["cell"]["step_m"])
    scores = np.array([c["score"] for c in cells])
    norm = PowerNorm(gamma=2.0, vmin=scores.min(), vmax=scores.max())
    pc = PolyCollection([cell_poly(c) for c in cells], array=scores, cmap=heat_cmap(), norm=norm,
                        edgecolors="none", zorder=1, rasterized=True)
    ax.add_collection(pc)
    return pc


def map_extent(ranked: list[dict], lat: float) -> tuple[float, float, float, float]:
    """Square view framing every scored cell, MAP_PAD_M of margin."""
    las, los = [c["lat_deg"] for c in ranked], [c["lon_deg"] for c in ranked]
    k = np.cos(np.radians(lat))
    clat, clon = (min(las) + max(las)) / 2, (min(los) + max(los)) / 2
    half = max(max(las) - min(las), (max(los) - min(los)) * k) / 2 + MAP_PAD_M / M_PER_DEG
    return clon - half / k, clon + half / k, clat - half, clat + half


def draw_map(ax, truth: dict, res: dict, tracks: list[dict]):
    lat, lon = truth["lat"], truth["lon"]
    extent = map_extent(res["ranked"], lat)
    bg = draw_background(ax, extent)
    pc = draw_heatmap(ax, res["ranked"])

    best = res["ranked"][0]
    tr = next(t for t in tracks if t["id"] == best["track_ids"][0])
    ax.plot(tr["lon"], tr["lat"], color=INK, lw=4.0, alpha=0.6, zorder=3, solid_capstyle="round")
    ax.plot(tr["lon"], tr["lat"], color="white", lw=2.0, zorder=4, solid_capstyle="round")
    ax.scatter([lon], [lat], marker="*", s=300, color="white", edgecolors=INK, linewidths=1.2, zorder=6)
    ax.annotate("true mic", (lon, lat), xytext=(12, -16), textcoords="offset points", fontsize=10, color=INK,
                bbox=LABEL_BOX, zorder=7)
    km = 1e3 / (M_PER_DEG * np.cos(np.radians(lat)))
    bar = AnchoredSizeBar(ax.transData, km, "1 km", "lower left", pad=0.25, borderpad=0.6, sep=3, color=INK,
                          size_vertical=(extent[3] - extent[2]) * 0.005, label_top=True, frameon=True,
                          fontproperties={"size": 9})
    bar.patch.set(facecolor=SURFACE, alpha=0.85, edgecolor="none", boxstyle="round,pad=0.1")
    ax.add_artist(bar)
    ax.set_xlim(extent[0], extent[1])
    ax.set_ylim(extent[2], extent[3])
    ax.set_aspect(1.0 / np.cos(np.radians(lat)))
    ax.set_xticks([])
    ax.set_yticks([])
    err = np.hypot((best["lat_deg"] - lat) * M_PER_DEG, (best["lon_deg"] - lon) * M_PER_DEG * np.cos(np.radians(lat)))
    print(f"background {bg}, cells {len(res['ranked'])}, best cell {err:.0f} m from truth, "
          f"score {best['score']:.1f}, flight {best['track_ids'][0]} ({tr['icao24']} {tr['type_code']})")
    return pc


def colorbar_below(fig, ax, pc) -> None:
    ax.apply_aspect()
    box = ax.get_position()
    cax = fig.add_axes((box.x0 + box.width * 0.45, box.y0 - 0.075, box.width * 0.55, 0.022))
    cb = fig.colorbar(pc, cax=cax, orientation="horizontal")
    lo, hi = pc.get_clim()
    cb.set_ticks([lo, hi], labels=[f"{lo:.0f}", f"{hi:.0f}"])
    cb.outline.set_visible(False)
    cax.tick_params(colors=INK2, length=0, labelsize=9, pad=2)
    cax.text(-0.03, 0.5, "search score (z)", transform=cax.transAxes, ha="right", va="center", fontsize=9, color=INK2)


def style(ax) -> None:
    ax.set_facecolor(SURFACE)
    for s in ax.spines.values():
        s.set_visible(False)
    ax.tick_params(colors=MUTED, length=0, labelsize=9)
    ax.xaxis.label.set_color(INK2)
    ax.yaxis.label.set_color(INK2)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--case", default=DEFAULT_CASE)
    ap.add_argument("--out", type=Path, default=ROOT / "docs/img/hero.png")
    a = ap.parse_args()
    truth, x, sr = load_case(a.case)
    event = strongest_event(x, sr)
    res, tracks = run_search(truth, x, sr, event)
    if not res["ranked"]:
        raise SystemExit("search scored no cells; no heatmap to draw")

    plt.rcParams.update({"font.family": ["Lato", "DejaVu Sans"], "font.size": 10.5, "axes.titlesize": 17,
                         "axes.titleweight": "semibold", "axes.titlelocation": "center", "axes.titlecolor": INK,
                         "axes.titlepad": 10})
    fig = plt.figure(figsize=(8, 4), dpi=200, facecolor=SURFACE)
    left = fig.add_axes((0.075, 0.13, 0.47, 0.76))
    right = fig.add_axes((0.585, 0.13, 0.40, 0.76))
    for ax in (left, right):
        style(ax)
    draw_spectrogram(left, x, sr, event)
    left.set_title("What the microphone heard")
    pc = draw_map(right, truth, res, tracks)
    right.set_title("Where it was recorded")
    colorbar_below(fig, right, pc)
    a.out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(a.out, facecolor=SURFACE)
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
