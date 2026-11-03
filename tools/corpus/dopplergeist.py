"""DOPPLERGEIST release assets -> oysterbay, synthetic and multi-event cases (truth + DG baselines)."""
import json
from pathlib import Path

from . import sources as S
from .common import (CASES, DOWNLOADS, SHARED_ADSB, SHARED_DEM, extract_tar_safely, gh_release_download,
                     log, place, place_tree, utc_iso, verify_sha256sums, write_json)

REL = DOWNLOADS / "dopplergeist-release"
EXT = DOWNLOADS / "dopplergeist-extracted"
MULTI = DOWNLOADS / "dg-multievent"
SIX = DOWNLOADS / "dg-sixevent-replay"
DG_LICENSE = ("DOPPLERGEIST code MIT; release data retain upstream rights (see its docs/DATA_SOURCES.md); "
              "private repo, do not redistribute")


def fetch():
    gh_release_download(S.DG_REPO, S.DG_CHECKPOINT, REL, S.DG_CHECKPOINT_ASSETS + ["manifest.json", "SHA256SUMS"])
    verify_sha256sums(REL, only=S.DG_CHECKPOINT_ASSETS)
    for name in S.DG_CHECKPOINT_ASSETS:
        extract_tar_safely(REL / name, EXT)
    gh_release_download(S.DG_REPO, S.DG_MULTIEVENT, MULTI, ["*"])
    verify_sha256sums(MULTI)
    for n in (3, 4, 6):
        extract_tar_safely(MULTI / f"multi-event-{n}.tar.gz", MULTI / "extracted")
    gh_release_download(S.DG_REPO, S.DG_SIXEVENT_REPLAY, SIX, ["*"])
    verify_sha256sums(SIX)
    extract_tar_safely(SIX / "six-event-cli-replay-20261103.tar.gz", SIX / "extracted")


def assemble():
    _shared()
    _oysterbay()
    _synthetic()
    _multievent()


def _shared():
    src = EXT / "data/adsb/2026-09-08"
    place_tree(src, SHARED_ADSB / "2026-09-08" / "dopplergeist-derived")
    place(EXT / "data/terrain/us_nga_egm96_15.tif", SHARED_DEM / "geoid/us_nga_egm96_15.tif")
    place(EXT / "data/terrain/Copernicus_DSM_COG_10_N40_00_W074_00_DEM.tif",
          SHARED_DEM / "glo30/Copernicus_DSM_COG_10_N40_00_W074_00_DEM.tif")
    place(EXT / "data/source/icao_aircraft_types.json", SHARED_ADSB / "type-db/tar1090-icao_aircraft_types.json")


def _oysterbay():
    o = S.OYSTERBAY
    case = CASES / o["case_id"]
    srcd = EXT / "data/source"
    place(srcd / "reference-recording.wav", case / "audio/reference-recording.wav")
    for name in ["reference-recording.info.json", "video.info.json", "transcript.txt", "transcript.json",
                 "true_camera.png", "known_time_result.png", "event_ranges.png", "faint-measurement-frame.png",
                 "source_manifest.json", "reproduce-local.json", "reproduce-faint-alltypes-local.json"]:
        place(srcd / name, case / "source" / name)
    adsb = EXT / "data/adsb/2026-09-08"
    for name in ["reference-region.parquet", "conus-alltypes-faint-window.parquet", "type-database.csv",
                 "release.json"]:
        place(adsb / name, case / "adsb/dopplergeist" / name)
    native = SHARED_ADSB / "2026-09-08" / "oysterbay.parquet"
    if native.exists():
        place(native, case / "adsb/oysterbay.parquet")
        place(native.with_suffix(".json"), case / "adsb/oysterbay.json")
    for tile in ["N40_00_W074_00", "N41_00_W074_00"]:
        src = SHARED_DEM / "glo30" / f"Copernicus_DSM_COG_10_{tile}_DEM.tif"
        if src.exists():
            place(src, case / "dem" / src.name)
    place(EXT / "data/terrain/us_nga_egm96_15.tif", case / "dem/us_nga_egm96_15.tif")
    for run in ["reference-local", "reference-local-fast", "reference-faint-alltypes-local",
                "reference-faint-local", "reference-faint-recovered-clock", "reference"]:
        place_tree(EXT / "results" / run, case / "baseline/dopplergeist-release" / run)
    t0 = o["recording_start_unix"]
    events = [dict(e, cpa_receive_utc=utc_iso(t0 + e["cpa_receive_s"])) for e in o["events"]]
    write_json(case / "truth.json", dict(
        case_id=o["case_id"], kind="real", lat=o["truth_lat"], lon=o["truth_lon"],
        location_source=o["truth_source"], location_cross_checks=o["cross_checks"],
        location_confidence="medium-high: author's top-ranked point + Street View match; not surveyed (~50 m)",
        date_utc="2026-09-08", year_source="adsb.lol day v2026.09.08 matched flights a1dfeb/ace391; video says Sept 8",
        recording_start_utc=utc_iso(t0), recording_start_unix=t0,
        recording_start_confidence=("low-medium: inferred by DOPPLERGEIST locked two-event fit (not an "
                                    "independent clock); faint-event config rounds to 20:17:30Z"),
        event_offset_s=round(o["events"][1]["cpa_receive_s"] - o["events"][0]["cpa_receive_s"], 3),
        events=events,
        audio=dict(file="audio/reference-recording.wav", duration_s=3600.184, sample_rate=48000, channels=2,
                   source_url="https://www.youtube.com/watch?v=4CxVxuMVkMc",
                   note="Decoded by DOPPLERGEIST from colsto's unlisted 'Unknown location - raw audio' (opus)."),
        source_url="https://www.youtube.com/watch?v=UytYrhYLOKI",
        license="colsto retains all rights to audio/video; research use only, do not redistribute; ADS-B ODbL",
        missing=["original video MP4 (YouTube blocked here; not needed for audio)",
                 "independent recording clock", "surveyed camera coordinate",
                 "author's flight IDs 1100069709/1100048665 are internal and unmapped to ICAO"]))


def _synthetic():
    syn = EXT / "data/synthetic"
    truth = json.loads((syn / "truth.json").read_text())
    for cid, cfg, run in [("dg_synthetic_two_events", "two-events.json", "synthetic-two-events"),
                          ("dg_synthetic_known_minute", "known-time-minute.json", "synthetic-known-minute")]:
        case = CASES / cid
        place(syn / "recording.wav", case / "audio/recording.wav")
        place(syn / "tracks.csv", case / "adsb/tracks.csv")
        place(syn / "types.csv", case / "adsb/types.csv")
        place(syn / cfg, case / "source" / cfg)
        place(syn / "truth.json", case / "source/dopplergeist-truth.json")
        place_tree(EXT / "results" / run, case / "baseline/dopplergeist-release" / run)
        write_json(case / "truth.json", dict(
            case_id=cid, kind="synthetic", lat=truth["lat"], lon=truth["lon"], elev_m=truth["elev_m"],
            recording_start_unix=truth["recording_start_time_s"],
            recording_start_utc=utc_iso(truth["recording_start_time_s"]),
            events=[dict(e, f0_hz=f) for e, f in zip(truth["events"], truth["f0_hz"])],
            flight_ids=truth["flight_ids"], terrain="constant 120 m (no DEM)",
            config=f"source/{cfg}", location_confidence="exact (synthetic)",
            source_url=f"https://github.com/{S.DG_REPO}/releases/tag/{S.DG_CHECKPOINT}", license=DG_LICENSE))


def _multievent():
    for n in (3, 4, 6):
        root = MULTI / "extracted" / f"multi-event-{n}"
        case = CASES / f"dg_multievent_{n}"
        place_tree(root / "inputs", case / "audio", "recording.mp3")
        place(root / "renderer/recording.wav", case / "audio/recording.wav")
        place(root / "inputs/tracks.parquet", case / "adsb/tracks.parquet")
        place(root / "inputs/terrain.tif", case / "dem/terrain.tif")
        place(root / "inputs/geoid.tif", case / "dem/geoid.tif")
        for name in ["coverage.json", "request.json"]:
            place(root / "inputs" / name, case / "source" / name)
        place(root / "generation.json", case / "source/generation.json")
        place(root / "truth/labels.json", case / "source/dopplergeist-labels.json")
        labels = json.loads((root / "truth/labels.json").read_text())
        if n == 6:
            place_tree(SIX / "extracted", case / "baseline/dopplergeist-six-event-cli-replay")
        write_json(case / "truth.json", dict(
            case_id=f"dg_multievent_{n}", kind="synthetic", lat=labels["location"][0], lon=labels["location"][1],
            elev_m=labels["elevation_m"], recording_start_utc=labels["recording_start_utc"],
            events=labels["events"], flight_ids=[e["flight_id"] for e in labels["events"]],
            distractors="traffic_identity_map in source/dopplergeist-labels.json (variants 1,2 are remote decoys)",
            location_confidence="exact (synthetic)",
            source_url=f"https://github.com/{S.DG_REPO}/releases/tag/{S.DG_MULTIEVENT}", license=DG_LICENSE))
    log("dopplergeist cases assembled")
