"""Pinned upstream sources and per-case facts. Facts carry their provenance; see data/MANIFEST.md."""

DG_REPO = "1337isnot1337/DOPPLERGEIST"
DG_CHECKPOINT = "checkpoint-2026-11-02"
DG_CHECKPOINT_ASSETS = ["data-source.tar.gz", "data-adsb-derived.tar.gz", "data-terrain.tar.gz",
                        "data-synthetic.tar.gz", "data-research.tar.gz", "results.tar.gz"]
DG_MULTIEVENT = "multi-event-fixtures-2026-11-03"
DG_SIXEVENT_REPLAY = "six-event-cli-replay-2026-11-03"

ADSBLOL_LICENSE = "ODbL 1.0 (adsb.lol globe_history); derived clips retain ODbL attribution/share-alike"


def adsblol_repo(tag):
    """adsb.lol publishes one release repo per year; tags are v<year>.MM.DD-..."""
    return f"adsblol/globe_history_{tag[1:5]}"


# Regions are clip boxes (west, south, east, north) for ADS-B; ~70 km half-width keeps silence-filter context.
REGIONS = {
    "oysterbay": (-74.30, 40.30, -72.70, 41.40),
    "livermore": (-122.60, 37.10, -121.00, 38.30),
    "adelaide": (138.235, -35.19, 138.95, -34.63),  # AeroSonicDB sites 0-2, +-25 km
}
# adsb.lol day tag -> regions clipped from it. Commented days are known gaps (budget); see MANIFEST.
ADSB_DAYS = {
    "v2026.09.08-planes-readsb-staging-0": ["oysterbay"],
    "v2026.05.17-planes-readsb-prod-0": ["livermore"],
    "v2026.05.16-planes-readsb-prod-0": ["livermore"],
    "v2026.05.14-planes-readsb-prod-0": ["livermore"],
    "v2026.05.13-planes-readsb-prod-0": ["livermore"],
    # "v2026.05.12-planes-readsb-prod-0": ["livermore"],  # 2 passages, ~3.2 GB
    # "v2026.05.09-planes-readsb-prod-0": ["livermore"],  # 3 passages, ~3.2 GB
    # AeroSonicDB local dates; 04-14 (UTC) holds the 04-15 clips recorded before 09:30 ACST.
    # adsb.lol 2023 has no feeder near Adelaide: these clips are empty (prod-1/staging-0 too, checked 05-09),
    # so they stay off until another ADS-B source (OpenSky) fills them. ~1.1 GB each.
    # "v2023.04.06-planes-readsb-prod-0": ["adelaide"],
    # "v2023.04.14-planes-readsb-prod-0": ["adelaide"],
    # "v2023.04.15-planes-readsb-prod-0": ["adelaide"],
    # "v2023.04.24-planes-readsb-prod-0": ["adelaide"],
    # "v2023.05.09-planes-readsb-prod-0": ["adelaide"],
}

COPERNICUS_URL = ("https://copernicus-dem-30m.s3.amazonaws.com/{name}/{name}.tif")
COPERNICUS_LICENSE = ("Copernicus DEM GLO-30, (c) DLR e.V. 2010-2014 and (c) Airbus Defence and Space GmbH "
                      "2014-2018, provided under COPERNICUS by the EU and ESA; free licence, attribution required")
# (lat_floor, lon_floor) tiles per case group.
DEM_TILES = {
    "oysterbay": [(40, -74), (41, -74)],
    "livermore": [(37, -122), (37, -123)],
    "nasa_cub": [(38, -78)],
    "nasa_edge": [(36, -77)],
    "aerosonic": [(-35, 138), (-36, 138)],
}

ZENODO_AEROSONIC = "8371595"
AEROSONIC_LICENSE = "CC BY-NC 4.0 (AeroSonicDB YPAD-0523 v1.1.1, Zenodo 8371595, Blake Downward); non-commercial"
AEROSONIC_TZ = "Australia/Adelaide"  # sample_meta date/time are local wall clock; build() gates on the CPA
AEROSONIC_DATES = ["2023-04-06", "2023-04-15", "2023-04-24", "2023-05-09"]  # local dates with an ADS-B day
AEROSONIC_ENGINES = {"Piston", "Turboprop", "Turboshaft"}
AEROSONIC_LOCATIONS = {"0", "1", "2"}

ZENODO_PATHAK = "23424278"
PATHAK_LICENSE = "CC BY 4.0 (Zenodo record 23424278, Praveen Pathak); flight tracks from public ADS-B services"

NASA_ZIP = "https://data.nasa.gov/docs/datasets/rfk401li/small_uav_acoustics.zip"
NASA_DESC = "https://data.nasa.gov/docs/datasets/rfk401li/Data_Description_20160203.pdf"
NASA_LICENSE = "NASA open data (data.nasa.gov rfk401li); US Government work, no copyright in the US"
# Members pinned by DOPPLERGEIST benchmarks/nasa-session-cases-20261103.json.
NASA_CASES = {
    "cub_flyover_107": "908ba29a68891fb6cb13087cbea9caf74f8385082d78842e1b91165e174365df",
    "edge_flyover_038": "81e02d387845614c4f9eea8761ec35211edcb5456a28d1b1c21dffb41fbd8cc8",
    "edge_flyover_065": "d06b708a460da233c550099698e5b69dadfbbe4f5404f77c30f32e1808717ccc",
    "edge_flyover_066": "0d4a1f7415e42f38ca64087400182e104ed7efface36de3cd7ed669aec9ad9bf",
}
# Publisher reference origins, as used by DOPPLERGEIST scripts/benchmark_nasa_session.py.
NASA_ORIGINS = {
    "cub": dict(origin="mic1", lat=38.0953507, lon=-77.2633382, height_m=20.726),
    "edge": dict(origin="mic3", lat=36.6797443, lon=-76.0376444, height_m=0.0),
}

# The video's own case. Recording start/event times are DOPPLERGEIST measurements on the 1 h raw-audio WAV.
OYSTERBAY = dict(
    case_id="oysterbay_20260908_colsto",
    truth_lat=40.876466, truth_lon=-73.524951,
    truth_source=("colsto video UytYrhYLOKI, ranking frame '1/7 -- flight1=1100069709, flight2=1100048665, "
                  "(40.876466, -73.524951), combined score=20.48' (DOPPLERGEIST data/source/known_time_result.png); "
                  "Street View confirmation at 19 Bayview Ave, Oyster Bay, NY (true_camera.png)"),
    cross_checks={
        "osprey_watch_nest_3916": [40.8766, -73.5247],
        "dopplergeist_reference_local_best": [40.87693175890468, -73.52498229500986],
        "transcript_spoken_lat": "40.87 ... 40.8735 (auto-caption, garbled)",
    },
    recording_start_unix=1788898649.6620934,
    events=[
        dict(name="event1_prop", cpa_receive_s=2155.715465159944, window_s=[2100, 2280], band_hz=[75, 190],
             flight_icao="a1dfeb", flight_segment="a1dfeb:1", aircraft_type="PC12", f0_hz=112.4,
             note="Video spectrogram shows this at elapsed 5700-5880 s of a longer source recording (+3600 s)."),
        dict(name="event2_prop", cpa_receive_s=3076.437754489226, window_s=[3040, 3125], band_hz=[95, 185],
             flight_icao="ace391", flight_segment="ace391:3", aircraft_type="B350", f0_hz=126.0),
        dict(name="event3_faint", cpa_receive_s=1281.0697518824172, window_s=[1250, 1310], band_hz=[280, 530],
             flight_icao=None, aircraft_type=None,
             note=("Video: 1-min clip 'at 8:37 pm UTC'. DOPPLERGEIST all-types single-event search picked "
                   "a38237 (CRJ9) at 357 m error, so the flight is unconfirmed.")),
    ],
)
