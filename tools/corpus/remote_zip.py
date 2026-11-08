"""Single members of a remote zip via HTTP Range (central directory, then one local entry). Stdlib only."""
import struct
import urllib.request
import zlib

from .common import USER_AGENT


def _get(url, a=None, b=None, method="GET"):
    headers = {"User-Agent": USER_AGENT, **({"Range": f"bytes={a}-{b}"} if a is not None else {})}
    return urllib.request.urlopen(urllib.request.Request(url, headers=headers, method=method), timeout=120)


def _range(url, a, b):
    with _get(url, a, b) as r:
        data = r.read()
    if len(data) != b - a + 1:
        raise ValueError(f"{url}: short range read {a}-{b}")
    return data


def central_directory(url):
    """name -> dict(method, csize, usize, offset, crc) for every member."""
    with _get(url, method="HEAD") as r:
        n = int(r.headers["Content-Length"])
    tail = _range(url, max(0, n - 65558), n - 1)
    i = tail.rfind(b"PK\x05\x06")
    cd_size, cd_off = struct.unpack("<II", tail[i + 12:i + 20])
    if cd_off == 0xFFFFFFFF:
        j = tail.rfind(b"PK\x06\x06")
        cd_size, cd_off = struct.unpack("<QQ", tail[j + 40:j + 56])
    cd = _range(url, cd_off, cd_off + cd_size - 1)
    out, p = {}, 0
    while cd[p:p + 4] == b"PK\x01\x02":
        meth, = struct.unpack("<H", cd[p + 10:p + 12])
        crc, csz, usz = struct.unpack("<III", cd[p + 16:p + 28])
        nl, el, cl = struct.unpack("<HHH", cd[p + 28:p + 34])
        off, = struct.unpack("<I", cd[p + 42:p + 46])
        name = cd[p + 46:p + 46 + nl].decode("utf-8", "replace")
        extra, q = cd[p + 46 + nl:p + 46 + nl + el], 0
        while q + 4 <= len(extra):
            hid, hl = struct.unpack("<HH", extra[q:q + 4])
            if hid == 1:  # zip64: only the saturated fields are present, in this order
                vals = list(struct.unpack("<" + "Q" * (hl // 8), extra[q + 4:q + 4 + hl // 8 * 8]))
                usz = vals.pop(0) if usz == 0xFFFFFFFF else usz
                csz = vals.pop(0) if csz == 0xFFFFFFFF else csz
                off = vals.pop(0) if off == 0xFFFFFFFF else off
            q += 4 + hl
        out[name] = dict(method=meth, csize=csz, usize=usz, offset=off, crc=crc)
        p += 46 + nl + el + cl
    return out


def read_member(url, ent):
    """Bytes of one member, CRC-checked."""
    h = _range(url, ent["offset"], ent["offset"] + 29)
    nl, el = struct.unpack("<HH", h[26:30])
    start = ent["offset"] + 30 + nl + el
    data = _range(url, start, start + ent["csize"] - 1)
    if ent["method"] == 8:
        data = zlib.decompress(data, -15)
    elif ent["method"] != 0:
        raise ValueError(f"unsupported zip method {ent['method']}")
    if len(data) != ent["usize"] or zlib.crc32(data) != ent["crc"]:
        raise ValueError("zip member size/CRC mismatch")
    return data
