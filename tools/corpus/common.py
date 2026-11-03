"""Shared helpers: paths, hashing, idempotent downloads, reflink placement, JSON output."""
import hashlib
import json
import shutil
import subprocess
import urllib.request
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[2]
DATA = PROJECT / "data"
DOWNLOADS = DATA / "_downloads"
CASES = DATA / "cases"
SHARED_DEM = DATA / "dem"
SHARED_ADSB = DATA / "adsb"
USER_AGENT = "doppler_print-corpus/1.0"


def force_ipv4():
    """Restrict name resolution to IPv4: this host's IPv6 route blackholes (data.nasa.gov hung indefinitely)."""
    import socket
    orig = socket.getaddrinfo

    def v4(host, port, family=0, *a, **k):
        return orig(host, port, socket.AF_INET, *a, **k)
    socket.getaddrinfo = v4


def log(*parts):
    print("[corpus]", *parts, flush=True)


def sha256(path, chunk=1 << 22):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(chunk), b""):
            h.update(block)
    return h.hexdigest()


def md5(path, chunk=1 << 22):
    h = hashlib.md5()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(chunk), b""):
            h.update(block)
    return h.hexdigest()


def download(url, dest, *, sha=None, md5sum=None, size=None, headers=None):
    """Fetch url to dest unless an existing file already matches; verify after download."""
    dest = Path(dest)
    if dest.exists() and _matches(dest, sha, md5sum, size):
        return dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_name(dest.name + ".part")
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
    log("GET", url)
    with urllib.request.urlopen(req, timeout=120) as r, open(part, "wb") as f:
        shutil.copyfileobj(r, f, 1 << 22)
    if not _matches(part, sha, md5sum, size):
        raise ValueError(f"checksum/size mismatch for {url}")
    part.rename(dest)
    return dest


def _matches(path, sha, md5sum, size):
    if size is not None and path.stat().st_size != size:
        return False
    if sha and sha256(path) != sha:
        return False
    if md5sum and md5(path) != md5sum:
        return False
    return path.stat().st_size > 0


def gh_release_download(repo, tag, dest_dir, patterns):
    dest_dir = Path(dest_dir)
    dest_dir.mkdir(parents=True, exist_ok=True)
    cmd = ["gh", "release", "download", tag, "-R", repo, "-D", str(dest_dir), "--skip-existing"]
    for p in patterns:
        cmd += ["-p", p]
    log(" ".join(cmd))
    subprocess.run(cmd, check=True)


def verify_sha256sums(directory, sums_file="SHA256SUMS", only=None):
    directory = Path(directory)
    for line in (directory / sums_file).read_text().splitlines():
        digest, name = line.split(maxsplit=1)
        name = name.lstrip("*")
        if only and not any(name.endswith(o) or o in name for o in only):
            continue
        if sha256(directory / name) != digest:
            raise ValueError(f"sha256 mismatch: {directory / name}")


def extract_tar_safely(archive, dest):
    """Extract into dest; the 'data' filter rejects absolute paths, '..', links outside dest and devices."""
    import tarfile
    marker = Path(dest) / f".extracted-{Path(archive).name}"
    if marker.exists():
        return
    Path(dest).mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as t:
        t.extractall(dest, filter="data")
    marker.write_text(sha256(archive) + "\n")


def place(src, dest):
    """Copy src into a case directory as a reflink (no extra disk on btrfs); skip if identical size."""
    src, dest = Path(src), Path(dest)
    if dest.exists() and dest.stat().st_size == src.stat().st_size:
        return dest
    dest.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["cp", "--reflink=auto", "-f", str(src), str(dest)], check=True)
    return dest


def place_tree(src_dir, dest_dir, pattern="*"):
    for p in sorted(Path(src_dir).rglob(pattern)):
        if p.is_file():
            place(p, Path(dest_dir) / p.relative_to(src_dir))


def write_json(path, obj):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj, indent=2, sort_keys=False, allow_nan=False) + "\n")


def write_case_checksums(case_dir):
    """Write <case>/SHA256SUMS for every file except the checksum file itself."""
    case_dir = Path(case_dir)
    lines = [f"{sha256(p)}  {p.relative_to(case_dir)}" for p in sorted(case_dir.rglob("*"))
             if p.is_file() and p.name != "SHA256SUMS"]
    (case_dir / "SHA256SUMS").write_text("\n".join(lines) + "\n")


def utc_iso(unix_s):
    from datetime import datetime, timezone
    return datetime.fromtimestamp(unix_s, tz=timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
