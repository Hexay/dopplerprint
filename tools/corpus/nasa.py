"""NASA small-UAV flyover acoustics: pull 4 pinned .mat members by HTTP range (archive is never fully downloaded).

These are GPS-tracked small UAVs (no ADS-B). Truth = publisher microphone table + GPS trajectory, in a local NED
frame tied to a published origin; clocks are UTC seconds-of-day with unresolved calendar dates.
"""
import io
import urllib.request
import zipfile

from . import sources as S
from .common import CASES, DOWNLOADS, USER_AGENT, download, log, place, sha256, write_json
from .dem import tile_path

NDIR = DOWNLOADS / "nasa-rfk401li"


class HTTPRangeFile(io.RawIOBase):
    def __init__(self, url):
        self.url, self.pos = url, 0
        self.size = int(self._get(0, 0)[1].split("/")[1])

    def _get(self, lo, hi):
        req = urllib.request.Request(self.url, headers={"Range": f"bytes={lo}-{hi}", "User-Agent": USER_AGENT})
        for attempt in range(4):
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    if r.status != 206:
                        raise IOError(f"range request not honoured ({r.status})")
                    return r.read(), r.headers["Content-Range"]
            except (TimeoutError, OSError) as e:
                log("NASA range retry", attempt, e)
        raise IOError("NASA range request failed 4 times")

    def readable(self):
        return True

    def seekable(self):
        return True

    def tell(self):
        return self.pos

    def seek(self, off, whence=0):
        self.pos = {0: off, 1: self.pos + off, 2: self.size + off}[whence]
        return self.pos

    def readinto(self, b):
        if self.pos >= self.size or not len(b):
            return 0
        hi = min(self.pos + len(b), self.size) - 1
        data, _ = self._get(self.pos, hi)
        b[:len(data)] = data
        self.pos += len(data)
        return len(data)


def fetch():
    NDIR.mkdir(parents=True, exist_ok=True)
    download(S.NASA_DESC, NDIR / "Data_Description_20160203.pdf")
    want = {f"data/{c}.mat": NDIR / f"{c}.mat" for c in S.NASA_CASES}
    if all(p.exists() and sha256(p) == S.NASA_CASES[p.stem] for p in want.values()):
        return
    raw = HTTPRangeFile(S.NASA_ZIP)
    with zipfile.ZipFile(io.BufferedReader(raw, buffer_size=1 << 20)) as z:
        infos = {i.filename: i for i in z.infolist()}
    (NDIR / "archive-members.txt").write_text("\n".join(infos) + "\n")
    want["Flight Number Description.xlsx"] = NDIR / "Flight Number Description.xlsx"
    for member, dest in want.items():
        if dest.exists() and (dest.suffix != ".mat" or sha256(dest) == S.NASA_CASES[dest.stem]):
            continue
        log("NASA member", member)
        try:
            dest.write_bytes(_member_bytes(raw, infos[member]))
        except IOError as e:
            if dest.suffix == ".mat":
                raise
            log("optional member skipped", member, e)
            continue
        if dest.suffix == ".mat" and sha256(dest) != S.NASA_CASES[dest.stem]:
            raise ValueError(f"sha256 mismatch for {member}")


def _ned_to_geo(ned, o):
    """Flat-earth NED -> lat/lon/height around the origin (sub-cm error over this ~1 km footprint)."""
    import numpy as np
    lat = o["lat"] + ned[:, 0] / 111_132.0
    lon = o["lon"] + ned[:, 1] / (111_320.0 * np.cos(np.radians(o["lat"])))
    return lat, lon, o["height_m"] - ned[:, 2]


def _export(mat, case, o):
    """Write audio/mic2.wav (float32 Pa), adsb/gps_track.csv and return mic-2 truth fields."""
    import numpy as np
    import scipy.io as sio
    from scipy.io import wavfile
    m = sio.loadmat(mat, simplify_cells=True)
    a, v = m["acoustics"], m["vehicle_data"]
    t = np.asarray(a["utc_time"], dtype=float)
    fs = round(1 / np.median(np.diff(t)))
    wav = case / "audio/mic2.wav"
    if not wav.exists():
        wav.parent.mkdir(parents=True, exist_ok=True)
        wavfile.write(wav, fs, np.asarray(a["incident_pascals"][:, 1], dtype=np.float32))
    gps = np.asarray(v["gps_ned_meters"], dtype=float)
    lat, lon, h = _ned_to_geo(gps, o)
    track = case / "adsb/gps_track.csv"
    track.parent.mkdir(parents=True, exist_ok=True)
    with open(track, "w") as f:
        f.write("utc_seconds_of_day,north_m,east_m,down_m,lat,lon,height_m\n")
        for row in zip(v["gps_utc_time"], gps[:, 0], gps[:, 1], gps[:, 2], lat, lon, h):
            f.write(",".join(f"{x:.8f}" if i in (4, 5) else f"{x:.3f}" for i, x in enumerate(row)) + "\n")
    met = m.get("met_data")
    mic = np.asarray(a["mic_ned_meters"], dtype=float)
    mlat, mlon, mh = _ned_to_geo(mic[1:2], o)
    return dict(lat=float(mlat[0]), lon=float(mlon[0]), height_m=float(mh[0]),
                mic_ned_m=mic.tolist(), audio_sample_rate_hz=fs,
                audio_start_utc_seconds_of_day=float(t[0]), audio_duration_s=float(t[-1] - t[0]),
                weather=({k: np.asarray(val).tolist() for k, val in met.items()} if isinstance(met, dict)
                         else "met_data present but not a single struct; see the .mat"))


def _member_bytes(raw, info):
    """One ranged GET per member; zipfile's small reads would each cost a round-trip."""
    import struct
    import zlib
    head, _ = raw._get(info.header_offset, info.header_offset + 29)
    name_len, extra_len = struct.unpack("<HH", head[26:30])
    start = info.header_offset + 30 + name_len + extra_len
    data, _ = raw._get(start, start + info.compress_size - 1)
    if info.compress_type == zipfile.ZIP_DEFLATED:
        data = zlib.decompress(data, -15)
    elif info.compress_type != zipfile.ZIP_STORED:
        raise ValueError(f"unsupported compression {info.compress_type}")
    if zlib.crc32(data) != info.CRC:
        raise ValueError(f"CRC mismatch for {info.filename}")
    return data


def build():
    fetch()
    for case_id in S.NASA_CASES:
        family = case_id.split("_")[0]
        o = S.NASA_ORIGINS[family]
        case = CASES / f"nasa_uav_{case_id}"
        place(NDIR / f"{case_id}.mat", case / "audio" / f"{case_id}.mat")
        place(NDIR / "Data_Description_20160203.pdf", case / "source/Data_Description_20160203.pdf")
        if (NDIR / "Flight Number Description.xlsx").exists():
            place(NDIR / "Flight Number Description.xlsx", case / "source/Flight Number Description.xlsx")
        tile = tile_path(*S.DEM_TILES[f"nasa_{family}"][0])
        if tile.exists():
            place(tile, case / "dem" / tile.name)
        mic2 = _export(NDIR / f"{case_id}.mat", case, o)
        write_json(case / "truth.json", dict(
            case_id=case.name, kind="real-uav", reference_origin=o, **mic2,
            receiver=("microphone 2 (one-based) of mic_ned_meters in the .mat, NED metres from the origin; "
                      "DOPPLERGEIST benchmark uses mono channel 2"),
            trajectory="gps_ned_meters vs gps_utc_time (seconds of day) inside the .mat",
            recording_clock="UTC seconds of day (utc_time); calendar date unresolved",
            adsb="not applicable (small UAV, supplied GPS track)",
            location_confidence="high for relative geometry; origin is publisher GPS (RTK unused)",
            baseline="DOPPLERGEIST benchmarks/nasa-session-results-20261103.csv (only edge_flyover_065 fitted, 474 m)",
            source_url=S.NASA_ZIP, license=S.NASA_LICENSE))
