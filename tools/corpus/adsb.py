"""adsb.lol globe_history day archives -> per-region Parquet clips keeping native readsb trace fields.

Schema (one row per trace point, all aircraft types; filter jets downstream):
icao, reg, typecode, desc, t_unix, lat, lon, alt_baro_ft (NaN when on ground), on_ground, gs_kt, track_deg,
flags (readsb bitfield: 1 stale, 2 new leg, 4 vrate geometric, 8 altitude geometric), baro_rate_fpm,
source, alt_geom_ft, geom_rate_fpm, ias_kt, roll_deg, callsign (only where the trace carries it).
One out-of-box neighbour is kept on each side of an in-box run so interpolation is continuous at the edge.
"""
import gzip
import io
import json
import math
import tarfile

from . import sources as S
from .common import DOWNLOADS, SHARED_ADSB, gh_release_download, log, write_json

RAW = DOWNLOADS / "adsblol"
COLUMNS = ["icao", "reg", "typecode", "desc", "t_unix", "lat", "lon", "alt_baro_ft", "on_ground", "gs_kt",
           "track_deg", "flags", "baro_rate_fpm", "source", "alt_geom_ft", "geom_rate_fpm", "ias_kt", "roll_deg",
           "callsign"]
FLUSH_ROWS = 250_000


def day_of(tag):
    return tag.split("-")[0][1:].replace(".", "-")


def clip_path(tag, region):
    return SHARED_ADSB / day_of(tag) / f"{region}.parquet"


def fetch(tags=None):
    for tag in tags or S.ADSB_DAYS:
        if all(clip_path(tag, r).exists() for r in S.ADSB_DAYS[tag]):
            continue
        gh_release_download(S.adsblol_repo(tag), tag, RAW / tag, ["*.tar.a?", "*.tar"])


def clip(tags=None):
    for tag in tags or S.ADSB_DAYS:
        regions = [r for r in S.ADSB_DAYS[tag] if not clip_path(tag, r).exists()]
        if regions:
            clip_day(tag, regions)


class _SplitReader(io.RawIOBase):
    def __init__(self, paths):
        self.paths, self.cur = iter(paths), None

    def readable(self):
        return True

    def readinto(self, b):
        while True:
            if self.cur is None:
                p = next(self.paths, None)
                if p is None:
                    return 0
                self.cur = open(p, "rb")
            n = self.cur.readinto(b)
            if n:
                return n
            self.cur.close()
            self.cur = None


def _num(v):
    return float(v) if isinstance(v, (int, float)) and not isinstance(v, bool) else math.nan


def _rows(d, box):
    w, s, e, n = box
    tr = d.get("trace") or []
    inside = [isinstance(p[1], (int, float)) and s <= p[1] <= n and w <= p[2] <= e for p in tr]
    keep = [inside[i] or (i > 0 and inside[i - 1]) or (i + 1 < len(tr) and inside[i + 1]) for i in range(len(tr))]
    base = d["timestamp"]
    meta = (d.get("icao"), d.get("r"), d.get("t"), d.get("desc"))
    for p, k in zip(tr, keep):
        if not k:
            continue
        p = list(p) + [None] * (14 - len(p))
        ac = p[8] if isinstance(p[8], dict) else {}
        ground = p[3] == "ground"
        cs = ac.get("flight")
        yield (*meta, base + p[0], _num(p[1]), _num(p[2]), math.nan if ground else _num(p[3]), ground,
               _num(p[4]), _num(p[5]), int(p[6] or 0), _num(p[7]), p[9], _num(p[10]), _num(p[11]),
               _num(p[12]), _num(p[13]), cs.strip() if isinstance(cs, str) else None)


def _schema():
    import pyarrow as pa
    strs = {"icao", "reg", "typecode", "desc", "source", "callsign"}
    types = {c: pa.string() if c in strs else pa.bool_() if c == "on_ground" else
             pa.int16() if c == "flags" else pa.float32() for c in COLUMNS}
    types["t_unix"] = pa.float64()
    types["lat"] = types["lon"] = pa.float64()
    return pa.schema([(c, types[c]) for c in COLUMNS])


def clip_day(tag, regions):
    import pyarrow as pa
    import pyarrow.parquet as pq
    split = sorted((RAW / tag).glob("*.tar.a?"))
    parts = split or sorted((RAW / tag).glob("*.tar"))
    if len(split) == 1 or not parts:
        raise FileNotFoundError(f"{tag}: archive incomplete; run the fetch stage")
    schema = _schema()
    bufs = {r: [] for r in regions}
    writers, stats = {}, {r: dict(aircraft=0, rows=0) for r in regions}
    tmp = {r: clip_path(tag, r).with_suffix(".parquet.part") for r in regions}

    def flush(r):
        if not bufs[r]:
            return
        cols = list(zip(*bufs[r]))
        table = pa.Table.from_arrays([pa.array(c, type=schema.field(i).type) for i, c in enumerate(cols)],
                                     schema=schema)
        if r not in writers:
            tmp[r].parent.mkdir(parents=True, exist_ok=True)
            writers[r] = pq.ParquetWriter(tmp[r], schema, compression="zstd", compression_level=9)
        writers[r].write_table(table)
        bufs[r].clear()

    files = 0
    log(f"clip {tag} -> {regions}")
    with io.BufferedReader(_SplitReader(parts), buffer_size=1 << 22) as stream, \
            tarfile.open(fileobj=stream, mode="r|") as archive:
        for m in archive:
            if not m.isfile() or "trace_full_" not in m.name:
                continue
            raw = archive.extractfile(m).read()
            d = json.loads(gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw)
            files += 1
            for r in regions:
                rows = list(_rows(d, S.REGIONS[r]))
                if rows:
                    bufs[r].extend(rows)
                    stats[r]["aircraft"] += 1
                    stats[r]["rows"] += len(rows)
                    if len(bufs[r]) >= FLUSH_ROWS:
                        flush(r)
            if files % 20000 == 0:
                log(tag, files, "traces", stats)
    for r in regions:
        flush(r)
        if r in writers:
            writers[r].close()
        else:  # no aircraft in the box (sparse feeder coverage): an empty clip still marks the day as done
            tmp[r].parent.mkdir(parents=True, exist_ok=True)
            pq.write_table(schema.empty_table(), tmp[r])
        tmp[r].rename(clip_path(tag, r))
        write_json(clip_path(tag, r).with_suffix(".json"), dict(
            source=f"https://github.com/{S.adsblol_repo(tag)}/releases/tag/{tag}", license=S.ADSBLOL_LICENSE,
            bbox_wsen=S.REGIONS[r], trace_files_scanned=files, **stats[r], columns=COLUMNS,
            note="All types retained; one neighbour outside the bbox kept per in-box run."))
    log(tag, "done", stats)
