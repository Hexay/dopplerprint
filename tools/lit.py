"""Literature search + full-text fetch without the agent web-search tool.

usage (via tools/dev.sh lit):
  search "<query>" [--src openalex,arxiv,crossref,ntrs,s2,wiki] [-n 8]
  fetch <url> [--grep REGEX] [--ctx 1] [--max 60]
Fetched files land in a fresh dir under $LIT_DIR (default /tmp/lit-$USER); text is extracted
from PDFs with pypdf (dev.sh supplies it via uv) and from HTML by tag stripping.
"""
import argparse
import html
import json
import os
import re
import sys
import tempfile
import time
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET

UA = "doppler-print-research/0.1 (literature tooling)"
TIMEOUT = 40


def get(url, accept=None, tries=3):
    req = urllib.request.Request(url, headers={"User-Agent": UA, **({"Accept": accept} if accept else {})})
    for i in range(tries):
        try:
            with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
                return r.read(), r.headers.get("Content-Type", ""), r.geturl()
        except urllib.error.HTTPError as e:
            # S2/OpenAlex rate-limit with 429; back off instead of failing the whole query
            if e.code in (429, 503) and i + 1 < tries:
                time.sleep(3 * (i + 1))
                continue
            raise


def q(s):
    return urllib.parse.quote(s)


def openalex(query, n):
    d = json.loads(get(f"https://api.openalex.org/works?search={q(query)}&per-page={n}")[0])
    for w in d.get("results", []):
        loc = w.get("best_oa_location") or {}
        src = (w.get("primary_location") or {}).get("source") or {}
        yield w.get("publication_year"), w.get("title"), src.get("display_name"), w.get("doi"), loc.get("pdf_url") or loc.get("landing_page_url")


def arxiv(query, n):
    terms = "+AND+".join("all:" + q(t) for t in query.split())
    body = get(f"https://export.arxiv.org/api/query?search_query={terms}&max_results={n}")[0]
    ns = {"a": "http://www.w3.org/2005/Atom"}
    for e in ET.fromstring(body).findall("a:entry", ns):
        pdf = next((l.get("href") for l in e.findall("a:link", ns) if l.get("title") == "pdf"), None)
        yield e.findtext("a:published", "", ns)[:4], " ".join(e.findtext("a:title", "", ns).split()), "arXiv", e.findtext("a:id", "", ns), pdf


def crossref(query, n):
    d = json.loads(get(f"https://api.crossref.org/works?query={q(query)}&rows={n}")[0])
    for w in d["message"]["items"]:
        yr = (w.get("issued", {}).get("date-parts") or [[None]])[0][0]
        link = next((l["URL"] for l in w.get("link", []) if "pdf" in l.get("content-type", "")), None)
        yield yr, (w.get("title") or [""])[0], (w.get("container-title") or [""])[0], w.get("DOI"), link


def ntrs(query, n):
    d = json.loads(get(f"https://ntrs.nasa.gov/api/citations/search?q={q(query)}&page.size={n}")[0])
    for w in d.get("results", []):
        dl = next((x["links"].get("pdf") for x in w.get("downloads", []) if x.get("links", {}).get("pdf")), None)
        yield (w.get("publications") or [{}])[0].get("publicationDate", "")[:4], w.get("title"), "NASA NTRS", w.get("id"), dl and "https://ntrs.nasa.gov" + dl


def s2(query, n):
    f = "title,year,venue,externalIds,openAccessPdf"
    d = json.loads(get(f"https://api.semanticscholar.org/graph/v1/paper/search?query={q(query)}&limit={n}&fields={f}")[0])
    for w in d.get("data", []):
        yield w.get("year"), w.get("title"), w.get("venue"), (w.get("externalIds") or {}).get("DOI"), (w.get("openAccessPdf") or {}).get("url")


def wiki(query, n):
    d = json.loads(get(f"https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={q(query)}&srlimit={n}&format=json")[0])
    for w in d["query"]["search"]:
        yield "", w["title"], "Wikipedia", "", "https://en.wikipedia.org/wiki/" + q(w["title"].replace(" ", "_"))


SOURCES = {"openalex": openalex, "arxiv": arxiv, "crossref": crossref, "ntrs": ntrs, "s2": s2, "wiki": wiki}


def cmd_search(a):
    for name in a.src.split(","):
        try:
            rows = list(SOURCES[name](a.query, a.n))
        except Exception as e:  # one dead API must not hide the others' results
            print(f"[{name}] ERROR {e}")
            continue
        for yr, title, venue, ident, url in rows:
            print(f"[{name}] {yr or '?'} | {(title or '')[:140]} | {(venue or '')[:50]} | {ident or ''} | {url or '-'}")


def pdf_text(path):
    from pypdf import PdfReader
    out = []
    for i, p in enumerate(PdfReader(path).pages):
        out.append(f"\n=== page {i + 1} ===\n{p.extract_text() or ''}")
    return "".join(out)


def html_text(raw):
    s = re.sub(r"(?is)<(script|style).*?</\1>", " ", raw.decode("utf-8", "replace"))
    s = re.sub(r"(?i)<br\s*/?>|</(p|div|tr|li|h\d)>", "\n", s)
    return re.sub(r"[ \t]+", " ", html.unescape(re.sub(r"<[^>]+>", " ", s)))


def cmd_fetch(a):
    root = os.environ.get("LIT_DIR", f"/tmp/lit-{os.environ.get('USER', 'u')}")
    os.makedirs(root, exist_ok=True)
    d = tempfile.mkdtemp(dir=root, prefix="f-")
    body, ctype, final = get(a.url)
    is_pdf = body[:5] == b"%PDF-" or "pdf" in ctype
    raw = os.path.join(d, "doc.pdf" if is_pdf else "doc.html")
    with open(raw, "wb") as f:
        f.write(body)
    text = pdf_text(raw) if is_pdf else html_text(body)
    txt = os.path.join(d, "doc.txt")
    with open(txt, "w") as f:
        f.write(text)
    print(f"{final}\n{'pdf' if is_pdf else 'html'} {len(body)} B -> {txt} ({len(text)} chars)")
    if a.grep:
        lines, rx, shown = text.splitlines(), re.compile(a.grep, re.I), 0
        for i, line in enumerate(lines):
            if rx.search(line):
                print(f"--- L{i + 1}\n" + "\n".join(lines[max(0, i - a.ctx): i + a.ctx + 1]))
                shown += 1
                if shown >= a.max:
                    print(f"... more matches truncated at --max {a.max}")
                    break


def main():
    p = argparse.ArgumentParser()
    sp = p.add_subparsers(dest="cmd", required=True)
    s = sp.add_parser("search")
    s.add_argument("query")
    s.add_argument("--src", default="openalex,arxiv,crossref,ntrs")
    s.add_argument("-n", type=int, default=8)
    f = sp.add_parser("fetch")
    f.add_argument("url")
    f.add_argument("--grep")
    f.add_argument("--ctx", type=int, default=1)
    f.add_argument("--max", type=int, default=60)
    a = p.parse_args()
    (cmd_search if a.cmd == "search" else cmd_fetch)(a)


if __name__ == "__main__":
    sys.exit(main())
