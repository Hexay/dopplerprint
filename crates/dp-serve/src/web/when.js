// Turns a datetime-local wall time + chosen zone into ISO 8601 with an explicit offset, and back.

const local = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";

export function fillZones(select) {
  const zones = Intl.supportedValuesOf ? Intl.supportedValuesOf("timeZone") : [];
  const add = (value, text) => select.append(new Option(text, value));
  add(local, `Your time zone (${local.replaceAll("_", " ")})`);
  if (local !== "UTC") add("UTC", "UTC");
  for (const z of zones) if (z !== local && z !== "UTC") add(z, z.replaceAll("_", " "));
}

// "offset:<minutes>" values come from re-loading a stored job; anything else is an IANA zone.
function offsetMinutes(zone, wallMs) {
  if (zone.startsWith("offset:")) return Number(zone.slice(7));
  const fmt = new Intl.DateTimeFormat("en-US", {
    timeZone: zone, hourCycle: "h23", year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit", second: "2-digit",
  });
  const at = (ms) => {
    const p = Object.fromEntries(fmt.formatToParts(new Date(ms)).map((x) => [x.type, x.value]));
    return (Date.UTC(+p.year, p.month - 1, +p.day, +p.hour, +p.minute, +p.second) - Math.floor(ms / 1000) * 1000) / 60000;
  };
  // The offset depends on the instant, which depends on the offset: two fixed-point steps settle DST edges.
  let off = at(wallMs);
  off = at(wallMs - off * 60000);
  return Math.round(at(wallMs - off * 60000));
}

const pad = (n) => String(Math.abs(Math.trunc(n))).padStart(2, "0");
const offsetText = (m) => `${m < 0 ? "-" : "+"}${pad(m / 60)}:${pad(m % 60)}`;

export function toIso(value, zone) {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?/.exec(value || "");
  if (!m) return null;
  const [y, mo, d, h, mi, s = "00"] = m.slice(1);
  const off = offsetMinutes(zone, Date.UTC(+y, mo - 1, +d, +h, +mi, +s));
  return `${y}-${mo}-${d}T${h}:${mi}:${s}${offsetText(off)}`;
}

export function utcText(iso) {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? "" : new Date(t).toISOString().replace("T", " ").slice(0, 19) + " UTC";
}

// Puts a stored ISO string back into the inputs, adding an offset option if needed.
export function fromIso(iso, input, select) {
  const m = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})([+-])(\d{2}):(\d{2})$/.exec(iso || "");
  if (!m) return;
  input.value = m[1];
  const mins = (m[2] === "-" ? -1 : 1) * (60 * +m[3] + +m[4]);
  const value = `offset:${mins}`;
  if (![...select.options].some((o) => o.value === value)) select.prepend(new Option(`UTC${m[2]}${m[3]}:${m[4]}`, value));
  select.value = value;
}
