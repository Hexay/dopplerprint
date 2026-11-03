// Renders one job and polls it until it finishes.

const $ = (id) => document.getElementById(id);
export function el(tag, props = {}, ...kids) {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...kids);
  return e;
}

export const STATUS_TEXT = {
  queued: "Waiting", running: "Working", found: "Found", needs_input: "Needs details",
  inconclusive: "Inconclusive", error: "Didn't work",
};
export const statusOf = (job) => job.outcome?.status || job.state;

let watching = null;

export async function watch(id, onDone) {
  watching = id;
  let delay = 800;
  for (;;) {
    let job;
    try {
      const r = await fetch(`/api/jobs/${encodeURIComponent(id)}`, { cache: "no-store" });
      if (r.status === 404) return render(null);
      job = await r.json();
    } catch {
      $("job-status").textContent = "Lost contact with the app. Is it still running? Retrying…";
      job = null;
    }
    if (watching !== id) return;
    if (job) {
      render(job);
      if (job.state === "done") return onDone(job);
    }
    await new Promise((res) => setTimeout(res, delay));
    delay = Math.min(delay * 1.2, 2500);
  }
}

export function stopWatching() {
  watching = null;
}

function render(job) {
  const section = $("job");
  section.hidden = false;
  if (!job) {
    $("job-title").textContent = "That recording isn't here any more";
    $("job-pill").textContent = "";
    $("running").hidden = true;
    $("outcome").hidden = true;
    return;
  }
  const status = statusOf(job);
  $("job-title").textContent = job.file_name;
  $("job-pill").textContent = STATUS_TEXT[status] || status;
  $("job-pill").className = `pill ${status}`;
  const done = job.state === "done";
  $("running").hidden = done;
  $("outcome").hidden = !done;
  if (!done) return renderProgress(job);
  const o = job.outcome;
  $("outcome-msg").textContent = o.message;
  $("outcome-msg").className = `callout ${o.status}`;
  $("found").hidden = o.status !== "found";
  if (o.status === "found") renderFound(job);
  // needs_input is answered through the form below, whose button then reads "Run again".
  $("retry").hidden = o.status === "found" || o.status === "needs_input";
}

function renderProgress(job) {
  const last = job.events.at(-1);
  const p = $("job-progress");
  if (job.state === "queued") {
    $("job-status").textContent = job.ahead > 0
      ? `Waiting for ${job.ahead} other recording${job.ahead > 1 ? "s" : ""} to finish first.`
      : "Starting…";
  } else {
    $("job-status").textContent = last ? `${last.message}…` : "Working…";
  }
  if (last && last.fraction != null) p.value = last.fraction; else p.removeAttribute("value");
  const byStage = new Map();
  for (const e of job.events) byStage.set(e.stage, e.message);
  $("stages").replaceChildren(...[...byStage.values()].map((m) => el("li", { textContent: m })));
}

const fmtCoord = (loc) => `${loc.lat.toFixed(5)}, ${loc.lon.toFixed(5)}`;
const fmtDist = (m) => (m < 1000 ? `${Math.round(m / 10) * 10} m` : `${(m / 1000).toFixed(1)} km`);
const osm = (loc) => `https://www.openstreetmap.org/?mlat=${loc.lat}&mlon=${loc.lon}#map=15/${loc.lat}/${loc.lon}`;

function renderFound(job) {
  const s = job.outcome.summary || {};
  const best = s.best;
  $("coords").textContent = best ? fmtCoord(best) : "—";
  $("copy").hidden = !best;
  $("copy").onclick = () => navigator.clipboard?.writeText(fmtCoord(best)).then(() => { $("copy").textContent = "Copied"; });
  $("copy").textContent = "Copy";
  $("radius-note").textContent = best?.radius_m ? `Probably within about ${fmtDist(best.radius_m)} of this point.` : "";
  $("confidence").textContent = s.confidence || "—";

  const amb = $("ambiguity");
  amb.hidden = !s.ambiguity;
  amb.replaceChildren();
  if (s.ambiguity) {
    amb.append(s.ambiguity);
    if (s.alternative) {
      amb.append(" The other possible spot is ",
        el("a", { href: osm(s.alternative), target: "_blank", rel: "noopener", textContent: fmtCoord(s.alternative) }), ".");
    }
  }

  const planes = s.aircraft || [];
  $("aircraft").replaceChildren(...(planes.length ? planes.map(planeItem) : [el("li", { className: "muted", textContent: "No aircraft details were reported." })]));

  const f = job.files || {};
  $("map-wrap").hidden = !f.html;
  if (f.html && $("map").getAttribute("src") !== f.html) $("map").src = f.html;
  const links = [];
  if (f.html) links.push(el("a", { href: f.html, target: "_blank", rel: "noopener", textContent: "Open map in a new tab" }));
  if (best) links.push(el("a", { href: osm(best), target: "_blank", rel: "noopener", textContent: "View on OpenStreetMap" }));
  if (f.geojson) links.push(el("a", { href: f.geojson, download: `${stem(job)}.geojson`, textContent: "Download GeoJSON" }));
  if (f.json) links.push(el("a", { href: f.json, download: `${stem(job)}.json`, textContent: "Download full result (JSON)" }));
  $("links").replaceChildren(...links);
}

function planeItem(a) {
  const li = el("li", {}, el("strong", { textContent: a.label }));
  if (a.type_code) li.append(el("span", { className: "muted", textContent: a.type_code }));
  if (a.t_audio_s != null) li.append(el("span", { className: "muted", textContent: `closest at ${fmtTime(a.t_audio_s)} into the recording` }));
  if (a.jet) li.append(el("span", { className: "jet", textContent: "jet · weaker evidence" }));
  return li;
}

const fmtTime = (s) => { const t = Math.round(s); return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, "0")}`; };
const stem = (job) => job.file_name.replace(/\.[^.]+$/, "") || "result";
