//! The system monitor for the Argos panel, as pure logic: parsing what /proc and
//! the Claude usage API give, the numbers, and the text Argos shows. The `nero`
//! command (cli/) reads the files and runs the programs; everything it decides
//! is here, so `cargo test` checks it without a desktop.
//!
//! Panel line: `[Claude 5h bar] % [Claude week bar] %  DOWN UP RAM CPU°C CPU% GPU°C GPU%`
//! with a dropdown below it. It is a port of the bash script it replaced
//! (`taskbar/argos/sysmon.1s.sh`); the text is the same, down to the spaces.

use serde_json::Value;

const RED: &str = "#ff3b3b";
const GREEN: &str = "#3bd13b";
const BLUE: &str = "#4a9bff";
// Claude usage bar colours: <50 green, 50+ blue, 80+ red
const C_GREEN: &str = "#5fb85f";
const C_BLUE: &str = "#3d8bfd";
const C_RED: &str = "#e5484d";
const C_TRACK: &str = "#4a4a4a";

/// How old the usage numbers may get before they are fetched again.
pub const USAGE_MAX_AGE_S: i64 = 300;
/// A refresh asked by hand is ignored when the last fetch was this recent.
pub const USAGE_MIN_AGE_S: i64 = 30;
/// A fetch lock older than this is left over from a crash.
pub const LOCK_STALE_S: i64 = 30;

// ---- the Claude usage bar ------------------------------------------------------

/// A percentage, or None for "-" (no number yet).
pub type Pct = Option<u32>;

/// The filling bar: 6 cells, 1/8-cell resolution, coloured by threshold.
pub fn ubar(p: Pct) -> String {
    let Some(p) = p else {
        return format!("<span color='{C_TRACK}'>██████</span>");
    };
    let p = p.min(100);
    let col = ucol(Some(p));
    let w = 6;
    let eighths = (p * w * 8 / 100) as usize;
    let (full, part) = (eighths / 8, eighths % 8);
    let parts = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let mut fill = "█".repeat(full);
    let mut used = full;
    if part > 0 {
        fill.push_str(parts[part]);
        used += 1;
    }
    let rest = "█".repeat((w as usize).saturating_sub(used));
    // the partial cell sits on the track colour so the bar reads as one piece
    format!(
        "<span color='{col}' bgcolor='{C_TRACK}'>{fill}</span><span color='{C_TRACK}'>{rest}</span>"
    )
}

/// The number next to a bar: right-aligned in 3 columns, " - " when unknown.
pub fn upct(p: Pct) -> String {
    p.map_or(" - ".into(), |p| format!("{p:>3}"))
}

pub fn ucol(p: Pct) -> &'static str {
    match p {
        None => C_TRACK,
        Some(p) if p >= 80 => C_RED,
        Some(p) if p >= 50 => C_BLUE,
        Some(_) => C_GREEN,
    }
}

fn span(color: &str, text: &str) -> String {
    format!("<span color='{color}'>{text}</span>")
}

// ---- time ---------------------------------------------------------------------

/// Seconds since the epoch for an ISO 8601 time like `2026-11-27T11:00:00.123456+00:00`
/// (also `Z` and no zone, which counts as UTC), or None.
pub fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let t = s.get(from..from + len)?;
        t.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| t.parse().ok())?
    };
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (h, mi, sec) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &frac[digits..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        r if (r.starts_with('+') || r.starts_with('-'))
            && r.len() == 6
            && r.as_bytes()[3] == b':' =>
        {
            let sign = if r.starts_with('-') { -1 } else { 1 };
            let oh: i64 = r.get(1..3)?.parse().ok()?;
            let om: i64 = r.get(4..6)?.parse().ok()?;
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };
    // days from civil (Howard Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + sec - offset)
}

/// "2h 14m" or "3d 5h" until `iso`; "-" when there is no time or it does not parse.
pub fn reset_in(iso: &str, now: i64) -> String {
    if iso.is_empty() || iso == "-" {
        return "-".into();
    }
    let Some(t) = parse_iso(iso) else {
        return "-".into();
    };
    let s = (t - now).max(0);
    if s >= 86400 {
        format!("{}d {}h", s / 86400, s % 86400 / 3600)
    } else {
        format!("{}h {}m", s / 3600, s % 3600 / 60)
    }
}

// ---- the usage state file ------------------------------------------------------

/// What `claude` (the state file) holds: `status sess week sess_reset week_reset ok_at`.
#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    /// `ok`, `nologin`, `err:<name>` or `wait` (no file yet).
    pub status: String,
    pub sess: Pct,
    pub week: Pct,
    pub sess_reset: String,
    pub week_reset: String,
    /// When the last successful fetch finished (epoch seconds).
    pub ok_at: Option<i64>,
}

impl Default for Usage {
    fn default() -> Self {
        Usage {
            status: "wait".into(),
            sess: None,
            week: None,
            sess_reset: "-".into(),
            week_reset: "-".into(),
            ok_at: None,
        }
    }
}

fn digits_only(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())
}

/// Read the state file's first line. The values end up on the panel, so each one
/// is checked: anything unexpected becomes "-" (or `wait` for the status).
pub fn parse_usage(text: &str) -> Usage {
    let mut f = text.lines().next().unwrap_or("").split_whitespace();
    let status = f.next().unwrap_or("wait");
    let ok_status = status == "ok"
        || status == "nologin"
        || status.strip_prefix("err:").is_some_and(|n| {
            !n.is_empty() && n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        });
    let pct = |v: Option<&str>| v.filter(|v| digits_only(v)).and_then(|v| v.parse().ok());
    let reset = |v: Option<&str>| {
        v.filter(|v| {
            !v.is_empty()
                && v.bytes().all(|c| {
                    c.is_ascii_digit()
                        || matches!(c, b'T' | b':' | b'.' | b'+' | b'-' | b'Z' | b'z')
                })
        })
        .unwrap_or("-")
        .to_string()
    };
    let sess = pct(f.next());
    let week = pct(f.next());
    let sess_reset = reset(f.next());
    let week_reset = reset(f.next());
    let ok_at = f
        .next()
        .filter(|v| digits_only(v))
        .and_then(|v| v.parse().ok());
    Usage {
        status: if ok_status {
            status.into()
        } else {
            "wait".into()
        },
        sess,
        week,
        sess_reset,
        week_reset,
        ok_at,
    }
}

/// The state file's line (without newline).
pub fn usage_line(
    status: &str,
    sess: &str,
    week: &str,
    sess_reset: &str,
    week_reset: &str,
    at: &str,
) -> String {
    format!("{status} {sess} {week} {sess_reset} {week_reset} {at}")
}

/// The last good numbers from the state file, kept when a fetch fails:
/// `sess week sess_reset week_reset at`, padded with "-".
pub fn previous_numbers(text: &str) -> String {
    let mut v: Vec<&str> = text.split_whitespace().skip(1).take(5).collect();
    while v.len() < 5 {
        v.push("-");
    }
    v.join(" ")
}

/// The state file line for a usage API answer. `body` is the JSON text.
pub fn usage_from_response(body: &str, now: i64) -> Result<String, String> {
    let d: Value = serde_json::from_str(body).map_err(|_| "JSONDecodeError".to_string())?;
    let get = |k: &str| -> (String, String) {
        let p = d.get(k).filter(|p| p.is_object());
        let u = p.and_then(|p| p.get("utilization")).and_then(Value::as_f64);
        // Python's round(): halves go to the even number
        let s = u.map_or("-".into(), |u| format!("{}", u.round_ties_even() as i64));
        let r = p
            .and_then(|p| p.get("resets_at"))
            .and_then(Value::as_str)
            .filter(|r| !r.is_empty())
            .unwrap_or("-")
            // the state line is split on spaces: one field
            .replace(char::is_whitespace, "T");
        (s, r)
    };
    let (s, sr) = get("five_hour");
    let (w, wr) = get("seven_day");
    Ok(usage_line("ok", &s, &w, &sr, &wr, &now.to_string()))
}

// ---- the numbers from /proc and /sys ---------------------------------------------

/// Total bytes received and sent by the real network interfaces. Virtual and VPN
/// interfaces are skipped: VPN traffic also passes through the real one.
pub fn parse_net_dev(text: &str) -> (i64, i64) {
    const SKIP: [&str; 16] = [
        "lo",
        "docker",
        "veth",
        "br-",
        "virbr",
        "vnet",
        "tun",
        "tap",
        "wg",
        "ppp",
        "ipsec",
        "pvpn",
        "proton",
        "nordlynx",
        "tailscale",
        "zt",
    ];
    let (mut rx, mut tx) = (0, 0);
    for line in text.lines().skip(2) {
        let line = line.trim_start();
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if SKIP.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let f: Vec<&str> = rest.split_whitespace().collect();
        rx += f.first().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        tx += f.get(8).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    }
    (rx, tx)
}

/// A speed in MB/s as the panel shows it: "0" below 0.05, one decimal below 10.
fn fmt_rate(x: f64) -> String {
    if x < 0.05 {
        "0".into()
    } else if x < 10.0 {
        format!("{x:.1}")
    } else {
        format!("{x:.0}")
    }
}

/// Download and upload in MB/s from two readings, `dt_ns` apart.
pub fn net_rates(dt_ns: i64, drx: i64, dtx: i64) -> (String, String) {
    let dt = if dt_ns <= 0 { 1 } else { dt_ns } as f64;
    let mb = |d: i64| d as f64 * 1e9 / dt / 1_048_576.0;
    (fmt_rate(mb(drx)), fmt_rate(mb(dtx)))
}

/// The CPU's total and idle ticks from the first line of /proc/stat.
pub fn parse_cpu_stat(text: &str) -> Option<(i64, i64)> {
    let line = text.lines().next()?;
    let mut f = line.split_whitespace();
    if f.next()? != "cpu" {
        return None;
    }
    let v: Vec<i64> = f.take(8).map(|x| x.parse().unwrap_or(0)).collect();
    if v.len() < 8 {
        return None;
    }
    let total: i64 = v.iter().sum();
    Some((total, v[3] + v[4]))
}

/// CPU use in % between two readings (0 when time has not moved on). Always
/// 0..=100: the kernel's iowait counter may step back, so idle can drop.
pub fn cpu_use(prev: (i64, i64), now: (i64, i64)) -> i64 {
    let dt = now.0 - prev.0;
    if dt > 0 {
        let idle = (now.1 - prev.1).clamp(0, dt);
        (100 * (dt - idle) + dt / 2) / dt
    } else {
        0
    }
}

/// RAM use in % from /proc/meminfo.
pub fn ram_pct(meminfo: &str) -> i64 {
    let field = |name: &str| {
        meminfo
            .lines()
            .find_map(|l| {
                l.strip_prefix(name)?
                    .trim_start_matches(':')
                    .split_whitespace()
                    .next()?
                    .parse::<i64>()
                    .ok()
            })
            .unwrap_or(0)
    };
    let (t, a) = (field("MemTotal"), field("MemAvailable"));
    if t > 0 { (t - a) * 100 / t } else { 0 }
}

/// A vertical bar for the RAM use.
pub fn ram_bar(pct: i64) -> &'static str {
    const BARS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    BARS[(((pct.clamp(0, 100) * 7 + 50) / 100) as usize).min(7)]
}

/// A "temp, use" pair from `nvidia-smi --format=csv,noheader,nounits`.
/// Only the first GPU is shown; anything that is not a number (an error text when
/// the driver is not loaded) gives None.
pub fn parse_nvidia(text: &str) -> Option<(String, String)> {
    let mut f = text
        .lines()
        .next()?
        .split([',', ' '])
        .filter(|s| !s.is_empty());
    let (t, u) = (f.next()?, f.next()?);
    Some((number(t)?, number(u)?))
}

/// A whole number as text (it goes into Pango markup), or None.
pub fn number(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.to_string())
}

/// The `bash=` value of an Argos line for the program at `exe`. Argos splits the
/// line with GLib.shell_parse_argv (one level of quotes off) and then runs the
/// value with `bash -c` (a second level), so the path is quoted twice.
pub fn argos_bash(exe: &str) -> String {
    let q = crate::taskbar::shell_quote(exe);
    crate::taskbar::shell_quote(&q)
}

/// Hardware monitor names that carry the CPU's temperature.
pub fn is_cpu_hwmon(name: &str) -> bool {
    matches!(
        name.trim(),
        "coretemp" | "k10temp" | "zenpower" | "cpu_thermal" | "acpitz"
    )
}

/// Millidegrees from a sysfs file to whole degrees.
pub fn milli_to_deg(text: &str) -> Option<i64> {
    text.trim().parse::<i64>().ok().map(|m| m / 1000)
}

// ---- the text Argos shows ------------------------------------------------------------

/// Everything the panel line and the dropdown show.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub usage: Usage,
    pub down: String,
    pub up: String,
    pub ram_pct: i64,
    pub cpu_temp: String,
    pub cpu_use: i64,
    pub gpu_temp: String,
    pub gpu_use: String,
    /// The command Argos runs for "Refresh usage" (absolute path).
    pub refresh_cmd: String,
    pub now: i64,
}

/// The whole Argos output: panel line, then the dropdown.
pub fn render(s: &Snapshot) -> String {
    let u = &s.usage;
    let mut out = String::new();
    out.push_str(&format!(
        "<tt>{} {}  {} {}  {} {} {} {} {} {} {}</tt> | useMarkup=true\n",
        ubar(u.sess),
        span(ucol(u.sess), &upct(u.sess)),
        ubar(u.week),
        span(ucol(u.week), &upct(u.week)),
        span(RED, &format!("{:>4}", s.down)),
        span(RED, &format!("{:<4}", s.up)),
        span(GREEN, ram_bar(s.ram_pct)),
        span(BLUE, &format!("{:>3}", s.cpu_temp)),
        span(BLUE, &format!("{:>3}", s.cpu_use)),
        span(BLUE, &format!("{:>3}", s.gpu_temp)),
        span(BLUE, &format!("{:>3}", s.gpu_use)),
    ));
    out.push_str("---\n");
    let dash = |p: Pct| p.map_or("-".to_string(), |p| p.to_string());
    out.push_str(&format!(
        "<tt>Session {} {}%   Week {} {}%</tt> | useMarkup=true\n",
        ubar(u.sess),
        dash(u.sess),
        ubar(u.week),
        dash(u.week)
    ));
    out.push_str(&format!(
        "Session resets in {}   Week resets in {}\n",
        reset_in(&u.sess_reset, s.now),
        reset_in(&u.week_reset, s.now)
    ));
    match u.status.as_str() {
        "nologin" => out.push_str("Claude usage: sign in to Claude Code first (run: claude)\n"),
        "err:HTTP401" | "err:HTTP403" => out.push_str(&format!(
            "Claude usage: login expired ({}), run: claude\n",
            &u.status[4..]
        )),
        "err:HTTP429" => out.push_str("Claude usage: rate-limited, showing last values\n"),
        e if e.starts_with("err") => out.push_str(&format!(
            "Claude usage: couldn't fetch ({}), showing last values\n",
            e.strip_prefix("err:").unwrap_or(e)
        )),
        "wait" => out.push_str("Claude usage: loading…\n"),
        _ => {}
    }
    // time since the last successful fetch, not the last attempt (errors rewrite the file too)
    let updated = match u.ok_at {
        None => "never".to_string(),
        Some(at) => {
            let age = s.now - at;
            if age < 60 {
                "just now".into()
            } else if age < 86400 {
                format!("{}m ago", age / 60)
            } else {
                format!("{}d ago", age / 86400)
            }
        }
    };
    out.push_str(&format!(
        "Refresh usage (updated {updated}) | bash={} param1=sysmon param2=refresh terminal=false refresh=true\n",
        s.refresh_cmd
    ));
    out.push_str("Open usage page | href=https://claude.ai/settings/usage\n");
    out.push_str("---\n");
    out.push_str(&format!(
        "Download: {} MB/s   Upload: {} MB/s\n",
        s.down, s.up
    ));
    out.push_str(&format!("RAM used: {}%\n", s.ram_pct));
    out.push_str(&format!("CPU: {}%   {}°C\n", s.cpu_use, s.cpu_temp));
    out.push_str(&format!("GPU: {}%   {}°C\n", s.gpu_use, s.gpu_temp));
    out.push_str("---\n");
    out.push_str("System Monitor | bash=gnome-system-monitor terminal=false\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // The expected strings of ubar come from running the bash function it replaces.
    #[test]
    fn bars_equal_the_bash_ones() {
        let t = |p: Pct, want: &str| assert_eq!(ubar(p), want, "{p:?}");
        t(None, "<span color='#4a4a4a'>██████</span>");
        t(
            Some(0),
            "<span color='#5fb85f' bgcolor='#4a4a4a'></span><span color='#4a4a4a'>██████</span>",
        );
        t(
            Some(1),
            "<span color='#5fb85f' bgcolor='#4a4a4a'></span><span color='#4a4a4a'>██████</span>",
        );
        t(
            Some(7),
            "<span color='#5fb85f' bgcolor='#4a4a4a'>▍</span><span color='#4a4a4a'>█████</span>",
        );
        t(
            Some(16),
            "<span color='#5fb85f' bgcolor='#4a4a4a'>▉</span><span color='#4a4a4a'>█████</span>",
        );
        t(
            Some(33),
            "<span color='#5fb85f' bgcolor='#4a4a4a'>█▉</span><span color='#4a4a4a'>████</span>",
        );
        t(
            Some(49),
            "<span color='#5fb85f' bgcolor='#4a4a4a'>██▉</span><span color='#4a4a4a'>███</span>",
        );
        t(
            Some(50),
            "<span color='#3d8bfd' bgcolor='#4a4a4a'>███</span><span color='#4a4a4a'>███</span>",
        );
        t(
            Some(79),
            "<span color='#3d8bfd' bgcolor='#4a4a4a'>████▋</span><span color='#4a4a4a'>█</span>",
        );
        t(
            Some(80),
            "<span color='#e5484d' bgcolor='#4a4a4a'>████▊</span><span color='#4a4a4a'>█</span>",
        );
        t(
            Some(99),
            "<span color='#e5484d' bgcolor='#4a4a4a'>█████▉</span><span color='#4a4a4a'></span>",
        );
        t(
            Some(100),
            "<span color='#e5484d' bgcolor='#4a4a4a'>██████</span><span color='#4a4a4a'></span>",
        );
        t(
            Some(120),
            "<span color='#e5484d' bgcolor='#4a4a4a'>██████</span><span color='#4a4a4a'></span>",
        );
    }

    #[test]
    fn percent_text_and_colours() {
        assert_eq!(
            (upct(None), upct(Some(5)), upct(Some(100))),
            (" - ".into(), "  5".into(), "100".into())
        );
        assert_eq!(
            (ucol(None), ucol(Some(49)), ucol(Some(50)), ucol(Some(80))),
            ("#4a4a4a", "#5fb85f", "#3d8bfd", "#e5484d")
        );
    }

    #[test]
    fn iso_times() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2026-11-27T11:00:00+00:00"), Some(1795777200));
        assert_eq!(
            parse_iso("2026-11-27T12:00:00.123456+01:00"),
            Some(1795777200)
        );
        assert_eq!(parse_iso("2026-11-27T10:30:00-00:30"), Some(1795777200));
        assert_eq!(parse_iso("2024-02-29T00:00:00"), Some(1709164800));
        for bad in [
            "",
            "-",
            "2026-13-01T00:00:00Z",
            "2026-11-27",
            "2026-11-27T11:00:00+0100",
            "2026-11-27T11:00:00.Z",
            "x026-11-27T11:00:00Z",
        ] {
            assert_eq!(parse_iso(bad), None, "{bad}");
        }
    }

    #[test]
    fn reset_texts_equal_the_bash_ones() {
        let now = 1795777200 - (2 * 3600 + 14 * 60 + 30);
        assert_eq!(reset_in("2026-11-27T11:00:00+00:00", now), "2h 14m");
        assert_eq!(
            reset_in(
                "2026-11-27T11:00:00+00:00",
                1795777200 - (3 * 86400 + 5 * 3600 + 59)
            ),
            "3d 5h"
        );
        assert_eq!(
            reset_in("2026-11-27T11:00:00+00:00", 1795777200 + 99),
            "0h 0m"
        );
        assert_eq!(reset_in("-", 0), "-");
        assert_eq!(reset_in("", 0), "-");
        assert_eq!(reset_in("later", 0), "-");
    }

    #[test]
    fn usage_state_is_checked() {
        let u = parse_usage(
            "ok 42 7 2026-11-27T11:00:00.5+00:00 2026-12-01T00:00:00+00:00 1795766430\n",
        );
        assert_eq!(
            (u.status.as_str(), u.sess, u.week, u.ok_at),
            ("ok", Some(42), Some(7), Some(1795766430))
        );
        assert_eq!(u.sess_reset, "2026-11-27T11:00:00.5+00:00");
        let bad = parse_usage("$(touch x) a b c d e");
        assert_eq!(
            (bad.status.as_str(), bad.sess, bad.week, bad.ok_at),
            ("wait", None, None, None)
        );
        assert_eq!(
            (bad.sess_reset.as_str(), bad.week_reset.as_str()),
            ("-", "-")
        );
        // a reset time in Z form is kept, not dropped to "-"
        let z = parse_usage("ok 42 7 2026-11-27T11:00:00Z 2026-12-01T00:00:00Z 1795766430");
        assert_eq!(z.sess_reset, "2026-11-27T11:00:00Z");
        assert_eq!(parse_usage("").status, "wait");
        assert_eq!(parse_usage("err:HTTP429 - - - - -").status, "err:HTTP429");
        assert_eq!(parse_usage("err:bad-name 1").status, "wait");
        assert_eq!(parse_usage("nologin - - - - -").status, "nologin");
        assert_eq!(previous_numbers("ok 1 2 a b 5"), "1 2 a b 5");
        assert_eq!(previous_numbers("err:X"), "- - - - -");
        assert_eq!(previous_numbers(""), "- - - - -");
    }

    #[test]
    fn usage_answer_to_state_line() {
        let body = r#"{"five_hour":{"utilization":41.5,"resets_at":"2026-11-27T11:00:00+00:00"},"seven_day":{"utilization":null,"resets_at":null},"other":1}"#;
        assert_eq!(
            usage_from_response(body, 99).unwrap(),
            "ok 42 - 2026-11-27T11:00:00+00:00 - 99"
        );
        // halves round to even, like Python's round()
        let body = r#"{"five_hour":{"utilization":0.5},"seven_day":{"utilization":2.5}}"#;
        assert_eq!(usage_from_response(body, 1).unwrap(), "ok 0 2 - - 1");
        assert_eq!(usage_from_response("{}", 1).unwrap(), "ok - - - - 1");
        assert!(usage_from_response("no json", 1).is_err());
    }

    #[test]
    fn network_counters() {
        let t = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n    lo: 1000 1 0 0 0 0 0 0 1000 1 0 0 0 0 0 0\n  eth0: 5000 10 0 0 0 0 0 0 7000 9 0 0 0 0 0 0\ndocker0: 9 9 0 0 0 0 0 0 9 9 0 0 0 0 0 0\nenp3s0:300 1 0 0 0 0 0 0 400 1 0 0 0 0 0 0\n   wg0: 1 1 0 0 0 0 0 0 1 1 0 0 0 0 0 0\n";
        assert_eq!(parse_net_dev(t), (5300, 7400));
        assert_eq!(parse_net_dev(""), (0, 0));
    }

    #[test]
    fn speeds() {
        let mb = 1_048_576;
        assert_eq!(net_rates(1_000_000_000, 0, 0), ("0".into(), "0".into()));
        assert_eq!(
            net_rates(1_000_000_000, mb * 3 / 2, mb * 25),
            ("1.5".into(), "25".into())
        );
        assert_eq!(
            net_rates(1_000_000_000, -5000, 10),
            ("0".into(), "0".into())
        );
        // no time passed: one nanosecond, a big number, no panic
        assert_eq!(net_rates(0, 1, 0).1, "0");
        assert_eq!(net_rates(2_000_000_000, mb * 8, 0).0, "4.0");
    }

    #[test]
    fn cpu_and_ram() {
        assert_eq!(
            parse_cpu_stat("cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 1 1 1 1 1 1 1 1\n"),
            Some((1000, 850))
        );
        assert_eq!(parse_cpu_stat("intr 1"), None);
        assert_eq!(parse_cpu_stat("cpu 1 2"), None);
        // 200 ticks later, 150 of them idle: 25 %
        assert_eq!(cpu_use((1000, 850), (1200, 1000)), 25);
        assert_eq!(cpu_use((1000, 850), (1000, 850)), 0);
        // idle stepping back (iowait) or running ahead stays within 0..=100
        assert_eq!(cpu_use((1000, 850), (1010, 845)), 100);
        assert_eq!(cpu_use((1000, 850), (1010, 900)), 0);
        let m = "MemTotal:       32000000 kB\nMemFree: 1 kB\nMemAvailable:   24000000 kB\n";
        assert_eq!(ram_pct(m), 25);
        assert_eq!(ram_pct(""), 0);
        assert_eq!((ram_bar(0), ram_bar(50), ram_bar(100)), ("▁", "▅", "█"));
    }

    #[test]
    fn gpu_and_temps() {
        assert_eq!(parse_nvidia("45, 12\n"), Some(("45".into(), "12".into())));
        assert_eq!(parse_nvidia(""), None);
        // driver not loaded: the error text is not a temperature
        assert_eq!(
            parse_nvidia("NVIDIA-SMI has failed because it couldn't communicate\n"),
            None
        );
        // two GPUs: the first
        assert_eq!(
            parse_nvidia("45, 12\n60, 99\n"),
            Some(("45".into(), "12".into()))
        );
        assert_eq!(number(" 7\n"), Some("7".into()));
        assert_eq!(number("<b>"), None);
        assert_eq!(
            argos_bash("/home/u/.local/bin/nero"),
            "/home/u/.local/bin/nero"
        );
        // after shell_parse_argv: '/my dir/nero' for bash
        assert_eq!(argos_bash("/my dir/nero"), r"''\''/my dir/nero'\'''");
        assert!(is_cpu_hwmon("k10temp\n") && !is_cpu_hwmon("nvme"));
        assert_eq!(
            (milli_to_deg("45500\n"), milli_to_deg("x")),
            (Some(45), None)
        );
    }

    fn snap(usage: Usage) -> Snapshot {
        Snapshot {
            usage,
            down: "1.5".into(),
            up: "0".into(),
            ram_pct: 25,
            cpu_temp: "45".into(),
            cpu_use: 7,
            gpu_temp: "-".into(),
            gpu_use: "-".into(),
            refresh_cmd: "/home/u/.local/bin/nero".into(),
            now: 1795766460,
        }
    }

    #[test]
    fn the_panel_line() {
        let u =
            parse_usage("ok 42 7 2026-11-27T11:00:00+00:00 2026-12-01T00:00:00+00:00 1795766430");
        let out = render(&snap(u));
        let first = out.lines().next().unwrap();
        assert!(first.starts_with("<tt>") && first.ends_with("</tt> | useMarkup=true"));
        // the tail, as the bash printf made it: DOWN UP RAM CPU°C CPU% GPU°C GPU%
        assert!(first.ends_with(
            "<span color='#ff3b3b'> 1.5</span> <span color='#ff3b3b'>0   </span> <span color='#3bd13b'>▃</span> \
             <span color='#4a9bff'> 45</span> <span color='#4a9bff'>  7</span> <span color='#4a9bff'>  -</span> \
             <span color='#4a9bff'>  -</span></tt> | useMarkup=true"
        ), "{first}");
        assert!(
            first.contains("<span color='#5fb85f'> 42</span>")
                || first.contains("<span color='#5fb85f'>"),
            "{first}"
        );
    }

    #[test]
    fn the_dropdown() {
        let u =
            parse_usage("ok 42 7 2026-11-27T11:00:00+00:00 2026-12-01T00:00:00+00:00 1795766430");
        let out = render(&snap(u));
        for want in [
            "---\n",
            "Session resets in 2h 59m   Week resets in 3d 15h\n",
            "Refresh usage (updated just now) | bash=/home/u/.local/bin/nero param1=sysmon param2=refresh terminal=false refresh=true\n",
            "Open usage page | href=https://claude.ai/settings/usage\n",
            "Download: 1.5 MB/s   Upload: 0 MB/s\n",
            "RAM used: 25%\n",
            "CPU: 7%   45°C\n",
            "GPU: -%   -°C\n",
            "System Monitor | bash=gnome-system-monitor terminal=false\n",
        ] {
            assert!(out.contains(want), "missing {want:?} in\n{out}");
        }
        assert!(
            out.contains("Session <span") && out.contains(" 42%   Week "),
            "{out}"
        );
        assert!(!out.contains("Claude usage:"), "no error line when ok");
    }

    #[test]
    fn the_dropdown_status_lines() {
        let line = |status: &str| {
            let u = parse_usage(&format!("{status} - - - - -"));
            let out = render(&snap(u));
            out.lines()
                .find(|l| l.starts_with("Claude usage:"))
                .map(String::from)
        };
        assert_eq!(
            line("nologin").unwrap(),
            "Claude usage: sign in to Claude Code first (run: claude)"
        );
        assert_eq!(
            line("err:HTTP401").unwrap(),
            "Claude usage: login expired (HTTP401), run: claude"
        );
        assert_eq!(
            line("err:HTTP429").unwrap(),
            "Claude usage: rate-limited, showing last values"
        );
        assert_eq!(
            line("err:NetError").unwrap(),
            "Claude usage: couldn't fetch (NetError), showing last values"
        );
        assert_eq!(line("wait").unwrap(), "Claude usage: loading…");
        assert_eq!(line("ok"), None);
        let out = render(&snap(Usage::default()));
        assert!(out.contains("(updated never)"));
        let at = |secs: i64| Usage {
            ok_at: Some(1795766460 - secs),
            ..Usage::default()
        };
        assert!(render(&snap(at(7200))).contains("(updated 120m ago)"));
        assert!(render(&snap(at(3 * 86400))).contains("(updated 3d ago)"));
    }
}
