//! The short human-readable report printed by `dopplerprint locate`.

use std::fmt::Write;

use super::report::{Alternative, Matched, Report, Status};
use super::when::fmt_duration;

pub fn render(r: &Report) -> String {
    let mut s = String::new();
    let name = std::path::Path::new(&r.audio).file_name().map_or(r.audio.clone(), |n| n.to_string_lossy().into_owned());
    let _ = write!(s, "dopplerprint · {name}");
    if r.duration_s > 0.0 {
        let _ = write!(s, " ({}, {} aircraft pass{})", fmt_duration(r.duration_s), r.events_found, plural(r.events_found, "es"));
    }
    s.push('\n');
    match (r.status, &r.location) {
        (Status::Found, Some(l)) => {
            let _ = writeln!(s, "\nFound:       {:.5}, {:.5}\n             {}", l.lat, l.lon, l.osm_url);
        }
        (Status::Inconclusive, _) => {
            let _ = writeln!(s, "\nInconclusive: {}", r.message.as_deref().unwrap_or("no location stands out"));
        }
        _ => {
            let _ = writeln!(s, "\n{}", r.message.as_deref().unwrap_or("Need more input."));
            return s;
        }
    }
    if let (Some(c), Some(l)) = (&r.confidence, &r.location) {
        let why = match (c.margin, r.alternatives.first()) {
            (Some(m), Some(a)) => format!("the next spot ({}) scores {m:.1} lower", describe(a)),
            _ => "no other spot comes close".into(),
        };
        let shown = if r.status == Status::Found { "Confidence:" } else { "Best guess: " };
        let _ = writeln!(s, "{shown}  {}, {why} (score {:.1})", c.level, l.score);
        for a in r.alternatives.iter().skip(1) {
            let _ = writeln!(s, "             also {}, {:.1} lower", describe(a), a.score_gap);
        }
    }
    if !r.aircraft.is_empty() {
        s.push_str("Aircraft:\n");
        for m in &r.aircraft {
            let _ = writeln!(s, "  {}", aircraft_line(m));
        }
    }
    if let Some(c) = &r.clock {
        let rel = match c.offset_from_given_s {
            o if o.abs() < 0.5 => "matches the time you gave".to_string(),
            o => format!("{} {} the time you gave", fmt_duration(o), if o > 0.0 { "after" } else { "before" }),
        };
        let _ = writeln!(s, "Clock:       recording started {} ({rel})", c.start_utc);
    }
    if let Some(a) = &r.area {
        let _ = writeln!(s, "Searched:    {} km around {}", (a.radius_km * 10.0).round() / 10.0, a.label);
    }
    if !r.terrain.is_empty() {
        let _ = writeln!(s, "Terrain:     {}", r.terrain);
    }
    if !r.files.is_empty() {
        let dir = std::path::Path::new(&r.files[0]).parent().map(|p| p.display().to_string()).unwrap_or_default();
        let names: Vec<String> = r.files.iter().filter_map(|f| std::path::Path::new(f).file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
        let _ = writeln!(s, "Wrote:       {dir}/  {}", names.join("  "));
    }
    s
}

fn describe(a: &Alternative) -> String {
    let what = if a.same_aircraft { "mirror solution" } else { "other aircraft" };
    format!("{what} {:.1} km {}", a.distance_km, a.direction)
}

fn aircraft_line(m: &Matched) -> String {
    let t = m.t_audio_s.max(0.0) as i64;
    let mut who = vec![m.registration.clone().unwrap_or_else(|| m.icao24.clone())];
    who.extend(m.type_code.clone());
    who.push(if m.jet { "jet (flagged)".into() } else { m.engine.clone() });
    format!(
        "pass {} at {}:{:02} in the audio   {}   closest {}   tone {:.0} Hz   match z {:.1}",
        m.event,
        t / 60,
        t % 60,
        who.join(" "),
        m.cpa_utc,
        m.f0_hz,
        m.z
    )
}

fn plural(n: usize, suffix: &str) -> &str {
    if n == 1 { "" } else { suffix }
}
