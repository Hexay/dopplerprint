"""Build the doppler_print validation corpus under data/. Idempotent: verified files are skipped.

Run (Python must be isolated because it parses downloaded data):
  uv run --no-project --with numpy --with scipy --with pyarrow --with h5py -- \
      python -I tools/fetch_corpus.py [stage ...]
Stages, in order: dopplergeist adsb-fetch adsb-clip dem livermore nasa aerosonicdb checksums manifest. Default: all
except aerosonicdb (opt-in: blocked on ADS-B coverage, see docs/research/hum_sources_2026-11-07.md). It also needs `--with rasterio` (site heights) and a built target/fastrel/dopplerprint.
See data/MANIFEST.md for what each stage produces.
"""
import argparse
import sys
from pathlib import Path

# -I drops the script directory from sys.path; add our own tools dir explicitly (never a download dir).
sys.path.insert(0, str(Path(__file__).resolve().parent))

from corpus import adsb, aerosonicdb, common, dem, dopplergeist, livermore, nasa, summary  # noqa: E402

STAGES = {
    "dopplergeist": lambda a: (dopplergeist.fetch(), dopplergeist.assemble()),
    "adsb-fetch": lambda a: adsb.fetch(a.days),
    "adsb-clip": lambda a: adsb.clip(a.days),
    "dem": lambda a: dem.fetch(),
    "livermore": lambda a: livermore.build(),
    "nasa": lambda a: nasa.build(),
    "aerosonicdb": lambda a: aerosonicdb.build(),
    "checksums": lambda a: [common.write_case_checksums(c) for c in sorted(common.CASES.iterdir()) if c.is_dir()],
    "manifest": lambda a: summary.write(),
}
# Not in "all": no ADS-B day covers its clips yet (sources.ADSB_DAYS), so it would build nothing.
OPT_IN = {"aerosonicdb"}


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("stages", nargs="*", choices=[*STAGES, "all"], default=["all"])
    p.add_argument("--days", nargs="*", help="adsb.lol tags (default: sources.ADSB_DAYS)")
    a = p.parse_args()
    common.force_ipv4()
    for name in ([s for s in STAGES if s not in OPT_IN] if "all" in a.stages else a.stages):
        common.log("stage", name)
        STAGES[name](a)


if __name__ == "__main__":
    main()
