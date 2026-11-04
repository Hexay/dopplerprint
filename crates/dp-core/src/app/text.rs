//! The short human-readable report printed by `dopplerprint locate`.

use std::fmt::Write;

use super::posterior::Posterior;
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
        (Status::Found, Some(l)) => match &r.posterior {
            Some(p) => {
                let a = &p.answer;
                let _ = writeln!(s, "\nFound:       {:.5}, {:.5} (centre of the best solution)\n             {}", a.lat, a.lon, a.osm_url);
                let _ = writeln!(s, "             best-scoring cell {:.5}, {:.5} is {:.0} m away", l.lat, l.lon, a.from_peak_m);
            }
            None => {
                let _ = writeln!(s, "\nFound:       {:.5}, {:.5}\n             {}", l.lat, l.lon, l.osm_url);
            }
        },
        (Status::Inconclusive, _) => {
            let _ = writeln!(s, "\nInconclusive: {}", r.message.as_deref().unwrap_or("no location stands out"));
        }
        _ => {
            let _ = writeln!(s, "\n{}", r.message.as_deref().unwrap_or("Need more input."));
            return s;
        }
    }
    if r.location.as_ref().is_some_and(|l| l.hypothesis_cap_hit) {
        s.push_str("Warning:     so many flights fit here that only the most likely pairings were scored; the true\n             pairing may have been skipped.\n");
    }
    if let Some(p) = &r.posterior {
        posterior_lines(&mut s, p);
    } else if let (Some(c), Some(l)) = (&r.confidence, &r.location) {
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
    if !r.air.is_empty() {
        let _ = writeln!(s, "Air:         {}", r.air);
    }
    if !r.files.is_empty() {
        let dir = std::path::Path::new(&r.files[0]).parent().map(|p| p.display().to_string()).unwrap_or_default();
        let names: Vec<String> = r.files.iter().filter_map(|f| std::path::Path::new(f).file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
        let _ = writeln!(s, "Wrote:       {dir}/  {}", names.join("  "));
    }
    s
}

fn posterior_lines(s: &mut String, p: &Posterior) {
    let how = match (p.calibrated, p.source.as_str()) {
        (false, _) => "uncalibrated for this many passes".to_string(),
        (true, "real") => "calibrated on real recordings".to_string(),
        (true, src) => format!("calibrated on {src} recordings"),
    };
    let islands = p.modes.iter().filter(|m| m.region_km2 > 0.0).count();
    let _ = writeln!(
        s,
        "90% region:  {:.2} km² ({how}){}; best spot's solution holds {:.0}%",
        p.area_km2,
        if islands > 1 { format!(" in {islands} islands") } else { String::new() },
        100.0 * p.modes[0].probability
    );
    for m in p.modes.iter().skip(1) {
        let what = if m.mirror { "mirror solution" } else if m.same_aircraft { "same aircraft" } else { "other aircraft" };
        let _ = writeln!(s, "             {what} {:.1} km {} holds {:.0}%", m.distance_km, m.direction, 100.0 * m.probability);
    }
    if !p.side_resolved && p.modes.iter().any(|m| m.mirror) {
        s.push_str("             one aircraft heard: a straight pass cannot tell the two sides of its track apart, and only a turn\n");
        s.push_str("             (which a ~2% sound-speed error can fake) breaks the tie; do not rule out the mirror\n");
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::posterior::ModeReport;

    fn mode(probability: f64, mirror: bool) -> ModeReport {
        let direction = "NE".into();
        ModeReport { lat: 0.0, lon: 0.0, centroid_lat: 0.0, centroid_lon: 0.0, probability, region_km2: 0.2, distance_km: 1.2, direction, same_aircraft: true, mirror }
    }

    #[test]
    fn single_aircraft_mirror_is_listed_with_caveat() {
        let p = Posterior {
            nominal: 0.9,
            hpd_mass: 0.988,
            calibrated: true,
            source: "real".into(),
            area_km2: 0.4,
            answer: crate::app::posterior::Answer { lat: 0.0, lon: 0.0, osm_url: String::new(), from_peak_m: 0.0 },
            modes: vec![mode(0.77, false), mode(0.23, true)],
            side_resolved: false,
        };
        let mut s = String::new();
        posterior_lines(&mut s, &p);
        assert!(s.contains("0.40 km² (calibrated on real recordings) in 2 islands; best spot's solution holds 77%"));
        assert!(s.contains("mirror solution 1.2 km NE holds 23%"));
        assert!(s.contains("do not rule out the mirror"));
    }
}
