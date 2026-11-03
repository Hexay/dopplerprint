import { fillZones, toIso, fromIso, utcText } from "/when.js";
import { watch, stopWatching, statusOf, STATUS_TEXT, el } from "/job.js";

const $ = (id) => document.getElementById(id);
const START_HINT = $("start-hint").textContent;
let file = null;      // a newly chosen file, to upload
let current = null;   // the job on screen; its audio is reused when no new file is chosen
let maxUpload = Infinity;

fillZones($("tz"));
fetch("/api/info").then((r) => r.json()).then((i) => { maxUpload = i.max_upload_bytes; }).catch(() => {});

const fmtSize = (b) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB` : `${Math.ceil(b / 1e3)} kB`);

function showChosen(name, note) {
  $("chosen").hidden = !name;
  $("pick").hidden = !!name;
  $("file-name").textContent = name || "";
  $("file-size").textContent = note || "";
}

function chooseFile(f) {
  setError("");
  if (!f) return;
  if (f.size > maxUpload) return setError(`That file is ${fmtSize(f.size)}; the limit is ${fmtSize(maxUpload)}. Trim it to the part with aircraft.`);
  file = f;
  showChosen(f.name, fmtSize(f.size));
  busy(false);
}

$("file").addEventListener("change", (e) => chooseFile(e.target.files[0]));
const drop = $("drop");
addEventListener("dragover", (e) => { e.preventDefault(); drop.classList.add("over"); });
addEventListener("dragleave", (e) => { if (!e.relatedTarget) drop.classList.remove("over"); });
addEventListener("drop", (e) => {
  e.preventDefault();
  drop.classList.remove("over");
  chooseFile(e.dataTransfer.files[0]);
});

function updateStartHint() {
  const iso = toIso($("start").value, $("tz").value);
  $("start-hint").textContent = iso ? `That is ${utcText(iso)}.` : START_HINT;
}
$("start").addEventListener("input", updateStartHint);
$("tz").addEventListener("change", updateStartHint);

$("here").addEventListener("click", () => {
  if (!navigator.geolocation) return setError("This browser can't share its location; type a place instead.");
  $("here").disabled = true;
  navigator.geolocation.getCurrentPosition(
    (p) => { $("near").value = `${p.coords.latitude.toFixed(4)}, ${p.coords.longitude.toFixed(4)}`; $("here").disabled = false; },
    () => { setError("Couldn't get your location; type a place instead."); $("here").disabled = false; },
    { timeout: 15000 },
  );
});

function formValues() {
  const num = (v) => (v === "" ? null : Number(v));
  return {
    start: toIso($("start").value, $("tz").value),
    tolerance_s: num($("tol").value),
    near: $("near").value.trim() || null,
    radius_km: num($("radius").value),
  };
}

function setError(msg) {
  $("form-error").textContent = msg;
}

function busy(on) {
  $("submit").disabled = on;
  $("submit").textContent = on ? (file ? "Uploading…" : "Starting…") : !file && current ? "Run again" : "Locate";
  $("upload").hidden = !on || !file;
}

function upload(f, form) {
  return new Promise((resolve, reject) => {
    const x = new XMLHttpRequest();
    x.open("POST", "/api/jobs");
    x.setRequestHeader("Content-Type", "application/octet-stream");
    x.setRequestHeader("X-Filename", encodeURIComponent(f.name));
    x.setRequestHeader("X-Locate", encodeURIComponent(JSON.stringify(form)));
    x.upload.onprogress = (e) => { if (e.lengthComputable) $("upload").value = e.loaded / e.total; };
    x.onload = () => resolve({ status: x.status, body: JSON.parse(x.responseText || "{}") });
    x.onerror = () => reject(new Error("The upload was interrupted. Is the app still running?"));
    x.send(f);
  });
}

async function rerun(id, form) {
  const r = await fetch(`/api/jobs/${encodeURIComponent(id)}/rerun`, {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(form),
  });
  return { status: r.status, body: await r.json() };
}

async function submit() {
  setError("");
  if (!file && !current) return setError("Choose or drop a recording first.");
  const form = formValues();
  busy(true);
  try {
    const { status, body } = file ? await upload(file, form) : await rerun(current.id, form);
    if (status !== 202) return setError(body.error || `Something went wrong (HTTP ${status}).`);
    file = null;
    $("file").value = "";
    location.hash = `job=${body.id}`;
    scrollTo({ top: 0, behavior: "smooth" });
  } catch (e) {
    setError(e.message);
  } finally {
    busy(false);
  }
}

$("form").addEventListener("submit", (e) => { e.preventDefault(); submit(); });
$("retry-btn").addEventListener("click", submit);

function markNeeds(needs) {
  for (const k of ["start", "near"]) $(`f-${k}`).classList.toggle("need", needs.includes(k));
  if (needs.length) {
    $(`f-${needs[0]}`).scrollIntoView({ behavior: "smooth", block: "center" });
    $(needs[0]).focus({ preventScroll: true });
  }
}

function fillForm(f) {
  if (f.start) fromIso(f.start, $("start"), $("tz"));
  if (f.tolerance_s != null) $("tol").value = String(f.tolerance_s);
  $("near").value = f.near || "";
  $("radius").value = f.radius_km != null ? String(f.radius_km) : "";
  updateStartHint();
}

async function open(id, prefill) {
  stopWatching();
  const first = await fetch(`/api/jobs/${encodeURIComponent(id)}`).then((r) => (r.ok ? r.json() : null)).catch(() => null);
  current = first;
  busy(false);
  if (!first) { $("job").hidden = true; return; }
  if (prefill) fillForm(first.form);
  if (!file) showChosen(first.file_name, "already uploaded");
  markNeeds([]);
  watch(id, (job) => {
    current = job;
    loadRecent();
    if (statusOf(job) === "needs_input") markNeeds(job.outcome.needs || []);
  });
}

function route(prefill) {
  const id = new URLSearchParams(location.hash.slice(1)).get("job");
  if (id) open(id, prefill);
  else { stopWatching(); current = null; busy(false); $("job").hidden = true; if (!file) showChosen(null); }
}
addEventListener("hashchange", () => route(false));

const ago = (t) => {
  const s = Date.now() / 1000 - t;
  if (s < 90) return "just now";
  if (s < 5400) return `${Math.round(s / 60)} min ago`;
  if (s < 129600) return `${Math.round(s / 3600)} h ago`;
  return new Date(t * 1000).toLocaleDateString();
};

async function loadRecent() {
  const { jobs } = await fetch("/api/jobs").then((r) => r.json()).catch(() => ({ jobs: [] }));
  $("recent").hidden = !jobs.length;
  $("recent-list").replaceChildren(...jobs.map((j) => {
    const s = statusOf(j);
    return el("li", {}, el("a", { href: `#job=${j.id}` },
      el("span", {}, j.file_name, el("span", { className: "when", textContent: ago(j.created) })),
      el("span", { className: `pill ${s}`, textContent: STATUS_TEXT[s] || s })));
  }));
}

loadRecent();
route(true);
