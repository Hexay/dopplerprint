"""README hero figure: one Livermore flyover's spectrogram + CV fit, and the search heatmap it implies.

Display only: spectrogram, event fit, CV curve, tracks, DEM heights and search scores all come from
dopplerprint._core. Run: .venv/bin/python -I tools/readme_figure.py [--case <id>] [--out <png>]
"""

import argparse
import json
from datetime import datetime
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.collections import PolyCollection
from matplotlib.colors import LightSource, LinearSegmentedColormap
from matplotlib.ticker import FixedLocator, NullLocator, ScalarFormatter

from dopplerprint import _core

ROOT = Path(__file__).resolve().parent.parent
STORE = ROOT / "data/adsb/store"
DEM = ROOT / "data/dem/5070/livermore_30m.u16"
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
    cv = lambda tt: _core.cv_frequency(fit["f0"], fit["v"], fit["d_cpa"], fit["t_cpa_arrive"], tt, c=fit["c"])
    ridge_t = np.asarray(event["ridge"]["t"])
    tt = np.linspace(ridge_t.min(), ridge_t.max(), 400)
    ff = cv(tt)
    ax.plot(tt, ff, color=ORANGE, lw=1.6, solid_capstyle="round")
    ax.text(tt[-1], ff[-1] * 0.93, "fitted Doppler curve", color=ORANGE, fontsize=10, fontweight="bold",
            ha="right", va="top", bbox=LABEL_BOX)
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


def draw_hillshade(ax, extent: tuple[float, float, float, float]) -> None:
    lon = np.linspace(extent[0], extent[1], 500)
    lat = np.linspace(extent[3], extent[2], 500)
    LON, LAT = np.meshgrid(lon, lat)
    h = _core.Dem(str(DEM)).height_msl_latlon(LAT, LON)
    h = np.where(np.isfinite(h), h, np.nanmean(h))
    shade = LightSource(azdeg=315, altdeg=45).hillshade(h, vert_exag=3, dx=30, dy=30)
    ax.imshow(shade, extent=extent, cmap="gray", vmin=-0.6, vmax=1.4, alpha=0.55, zorder=0, interpolation="bilinear")


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
    pc = PolyCollection([cell_poly(c) for c in cells], array=scores, cmap=SEQ, clim=(scores.min(), scores.max()),
                        edgecolors="none", alpha=0.85, zorder=1, rasterized=True)
    ax.add_collection(pc)
    return pc


def local_km(lat0: float, lon0: float, lat, lon) -> tuple[np.ndarray, np.ndarray]:
    k = np.cos(np.radians(lat0))
    return (np.asarray(lon) - lon0) * k * M_PER_DEG / 1e3, (np.asarray(lat) - lat0) * M_PER_DEG / 1e3


def label_mirror(ax, ranked: list[dict], tr: dict, lat: float, lon: float) -> None:
    """Label placement only: best cell on the far side of the track from the mic is the mirror lobe."""
    tx, ty = local_km(lat, lon, tr["lat"], tr["lon"])
    i = int(np.argmin(np.hypot(tx, ty)))
    j = min(i + 1, len(tx) - 1) if i + 1 < len(tx) else i - 1
    dx, dy = tx[j] - tx[i], ty[j] - ty[i]
    side = lambda x, y: np.sign(dx * (y - ty[i]) - dy * (x - tx[i]))
    mic_side = side(0.0, 0.0)
    far = [c for c in ranked if side(*local_km(lat, lon, c["lat_deg"], c["lon_deg"])) == -mic_side]
    if not far:
        return
    c = max(far, key=lambda c: c["score"])
    ax.annotate("mirror image:\nsame curve, other\nside of the track",
                (c["lon_deg"], c["lat_deg"]), xytext=(-10, 0), textcoords="offset points", fontsize=10,
                color=INK, ha="right", va="center", arrowprops=dict(arrowstyle="-", color=INK2, lw=1.0),
                bbox=LABEL_BOX, zorder=7)


def draw_map(ax, truth: dict, res: dict, tracks: list[dict]) -> None:
    lat, lon = truth["lat"], truth["lon"]
    extent = map_extent(res["ranked"], lat)
    draw_hillshade(ax, extent)
    pc = draw_heatmap(ax, res["ranked"])

    best = res["ranked"][0]
    tr = next(t for t in tracks if t["id"] == best["track_ids"][0])
    ax.plot(tr["lon"], tr["lat"], color=SURFACE, lw=4.0, zorder=3)
    ax.plot(tr["lon"], tr["lat"], color=ORANGE, lw=2.0, zorder=4)
    ax.scatter([lon], [lat], marker="*", s=260, color=INK, edgecolors=SURFACE, linewidths=1.5, zorder=6)
    ax.annotate("true mic", (lon, lat), xytext=(12, -16), textcoords="offset points", fontsize=10, color=INK,
                bbox=LABEL_BOX, zorder=7)
    label_track(ax, tr, extent, f"ADS-B track ({tr['type_code'] or tr['icao24']})")
    label_mirror(ax, res["ranked"], tr, lat, lon)
    scale_bar(ax, extent, lat)
    ax.set_xlim(extent[0], extent[1])
    ax.set_ylim(extent[2], extent[3])
    ax.set_aspect(1.0 / np.cos(np.radians(lat)))
    ax.set_xticks([])
    ax.set_yticks([])
    cax = ax.inset_axes([0.62, 0.86, 0.33, 0.035])
    cb = ax.figure.colorbar(pc, cax=cax, orientation="horizontal")
    lo, hi = pc.get_clim()
    cb.set_ticks([lo, hi], labels=[f"{lo:.0f}", f"{hi:.0f}"])
    cb.outline.set_visible(False)
    cax.tick_params(colors=INK2, length=0, labelsize=9)
    cax.set_title("search score (z)", fontsize=9, color=INK2, loc="left", fontweight="normal", pad=3)
    err = np.hypot(*local_km(lat, lon, best["lat_deg"], best["lon_deg"])) * 1e3
    print(f"cells {len(res['ranked'])}, best cell {err:.0f} m from truth, score {best['score']:.1f}, "
          f"flight {best['track_ids'][0]} ({tr['icao24']} {tr['type_code']})")


def map_extent(ranked: list[dict], lat: float) -> tuple[float, float, float, float]:
    """Square view framing every scored cell, MAP_PAD_M of margin."""
    las, los = [c["lat_deg"] for c in ranked], [c["lon_deg"] for c in ranked]
    k = np.cos(np.radians(lat))
    clat, clon = (min(las) + max(las)) / 2, (min(los) + max(los)) / 2
    half = max(max(las) - min(las), (max(los) - min(los)) * k) / 2 + MAP_PAD_M / M_PER_DEG
    return clon - half / k, clon + half / k, clat - half, clat + half


def label_track(ax, tr: dict, extent, text: str) -> None:
    order = np.argsort(tr["lon"])
    x = extent[0] + 0.03 * (extent[1] - extent[0])
    y = np.interp(x, tr["lon"][order], tr["lat"][order], left=np.nan, right=np.nan)
    if np.isfinite(y):
        ax.annotate(text, (x, y), xytext=(0, -10), textcoords="offset points", fontsize=10, ha="left", va="top",
                    color=ORANGE, fontweight="bold", bbox=LABEL_BOX, zorder=7)


def scale_bar(ax, extent, lat: float) -> None:
    km = 1e3 / (M_PER_DEG * np.cos(np.radians(lat)))
    x0 = extent[0] + 0.06 * (extent[1] - extent[0])
    y0 = extent[2] + 0.07 * (extent[3] - extent[2])
    ax.plot([x0, x0 + km], [y0, y0], color=INK, lw=2.0, solid_capstyle="butt", zorder=6)
    ax.text(x0 + km / 2, y0 + 0.015 * (extent[3] - extent[2]), "1 km", ha="center", va="bottom", fontsize=9,
            color=INK, zorder=6)


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

    plt.rcParams.update({"font.family": "sans-serif", "font.size": 10, "axes.titlesize": 14,
                         "axes.titleweight": "bold", "axes.titlelocation": "left", "axes.titlecolor": INK,
                         "axes.titlepad": 8})
    fig = plt.figure(figsize=(8, 4), dpi=200, facecolor=SURFACE)
    left = fig.add_axes((0.075, 0.12, 0.47, 0.78))
    right = fig.add_axes((0.585, 0.12, 0.40, 0.78))
    for ax in (left, right):
        style(ax)
    draw_spectrogram(left, x, sr, event)
    left.set_title("What the microphone heard")
    draw_map(right, truth, res, tracks)
    right.set_title("Where that puts you")
    a.out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(a.out, facecolor=SURFACE)
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
