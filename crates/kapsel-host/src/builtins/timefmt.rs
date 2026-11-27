//! date and sleep. Time is always UTC (the host has no time zone database).

use super::{Ctx, Spec, getopt};

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December",
];

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// strftime subset in UTC.
pub fn format(secs: i64, fmt: &str) -> String {
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    let wd = (days + 4).rem_euclid(7) as usize;
    let yday = days - days_from_civil(y, 1, 1) + 1;
    let mut out = String::new();
    let mut it = fmt.chars();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(f) = it.next() else {
            out.push('%');
            break;
        };
        match f {
            'Y' => out.push_str(&y.to_string()),
            'C' => out.push_str(&format!("{:02}", y / 100)),
            'y' => out.push_str(&format!("{:02}", y.rem_euclid(100))),
            'm' => out.push_str(&format!("{mo:02}")),
            'd' => out.push_str(&format!("{d:02}")),
            'e' => out.push_str(&format!("{d:2}")),
            'H' => out.push_str(&format!("{h:02}")),
            'k' => out.push_str(&format!("{h:2}")),
            'I' => out.push_str(&format!("{:02}", if h % 12 == 0 { 12 } else { h % 12 })),
            'l' => out.push_str(&format!("{:2}", if h % 12 == 0 { 12 } else { h % 12 })),
            'M' => out.push_str(&format!("{mi:02}")),
            'S' => out.push_str(&format!("{s:02}")),
            'p' => out.push_str(if h < 12 { "AM" } else { "PM" }),
            'a' => out.push_str(&DAYS[wd][..3]),
            'A' => out.push_str(DAYS[wd]),
            'b' | 'h' => out.push_str(&MONTHS[mo as usize - 1][..3]),
            'B' => out.push_str(MONTHS[mo as usize - 1]),
            'j' => out.push_str(&format!("{yday:03}")),
            'u' => out.push_str(&(if wd == 0 { 7 } else { wd }).to_string()),
            'w' => out.push_str(&wd.to_string()),
            's' => out.push_str(&secs.to_string()),
            'Z' => out.push_str("UTC"),
            'z' => out.push_str("+0000"),
            'F' => out.push_str(&format!("{y}-{mo:02}-{d:02}")),
            'T' => out.push_str(&format!("{h:02}:{mi:02}:{s:02}")),
            'R' => out.push_str(&format!("{h:02}:{mi:02}")),
            'D' => out.push_str(&format!("{mo:02}/{d:02}/{:02}", y.rem_euclid(100))),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            '%' => out.push('%'),
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(n) = s.strip_prefix('@') {
        return n.parse().ok();
    }
    let (date, time) = match s.split_once([' ', 'T']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let dp: Vec<&str> = date.split('-').collect();
    if dp.len() != 3 {
        return None;
    }
    let (y, m, d): (i64, u32, u32) = (dp[0].parse().ok()?, dp[1].parse().ok()?, dp[2].parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut secs = days_from_civil(y, m, d) * 86400;
    if let Some(t) = time {
        let t = t.trim_end_matches('Z');
        let tp: Vec<&str> = t.split(':').collect();
        if tp.len() < 2 || tp.len() > 3 {
            return None;
        }
        secs += tp[0].parse::<i64>().ok()? * 3600 + tp[1].parse::<i64>().ok()? * 60;
        if let Some(x) = tp.get(2) {
            secs += x.parse::<i64>().ok()?;
        }
    }
    Some(secs)
}

pub fn date(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec {
        short: "ud:",
        long: &[("utc", 'u'), ("date", 'd')],
        numeric: false,
        supported: "-u -d @SECONDS|YYYY-MM-DD[ HH:MM[:SS]] +FORMAT (time zone is always UTC)",
    };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    let now = match p.val('d') {
        Some(s) => match parse_date(s) {
            Some(t) => t,
            None => {
                ctx.msg(&format!("invalid date '{s}'"));
                return 1;
            }
        },
        None => std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
    };
    let mut fmt = "%a %b %e %H:%M:%S %Z %Y".to_string();
    for o in &p.operands {
        match o.strip_prefix('+') {
            Some(f) => fmt = f.to_string(),
            None => {
                ctx.msg(&format!("invalid date '{o}'; only +FORMAT is supported as an operand"));
                return 1;
            }
        }
    }
    let s = format!("{}\n", format(now, &fmt));
    ctx.puts(&s);
    0
}

pub fn sleep(ctx: &mut Ctx, a: &[String]) -> i32 {
    let spec = Spec { short: "", long: &[], numeric: false, supported: "no flags; usage: sleep NUMBER[s|m|h|d]..." };
    let Ok(p) = getopt(ctx, a, &spec) else { return 2 };
    if p.operands.is_empty() {
        ctx.msg("missing operand");
        return 1;
    }
    let mut total = 0.0f64;
    for o in &p.operands {
        let (num, mult) = match o.chars().last() {
            Some('s') => (&o[..o.len() - 1], 1.0),
            Some('m') => (&o[..o.len() - 1], 60.0),
            Some('h') => (&o[..o.len() - 1], 3600.0),
            Some('d') => (&o[..o.len() - 1], 86400.0),
            _ => (o.as_str(), 1.0),
        };
        match num.parse::<f64>() {
            Ok(v) if v >= 0.0 && v.is_finite() => total += v * mult,
            _ => {
                ctx.msg(&format!("invalid time interval '{o}'"));
                return 1;
            }
        }
    }
    std::thread::sleep(std::time::Duration::from_secs_f64(total));
    0
}
