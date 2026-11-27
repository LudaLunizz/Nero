//! `nero sysmon`: the input and output of the Argos system monitor. The numbers,
//! the checks and the text are `nero_core::sysmon`.

use nero_core::sysmon as sm;
use std::fs;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Never falls back to /tmp: another user could pre-create the folder and plant state.
fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"));
    base.join("argos-sysmon")
}

fn make_state_dir(dir: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

fn now() -> (i64, i64) {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (d.as_secs() as i64, d.as_nanos() as i64)
}

/// Seconds since the file was last written (999 when it does not exist).
fn age_of(path: &Path, now_s: i64) -> i64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(999, |t| now_s - t.as_secs() as i64)
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Two whole numbers from a state file, or None (anything else is ignored).
fn read_numbers(path: &Path, n: usize) -> Option<Vec<i64>> {
    let text = read(path)?;
    let v: Vec<i64> = text
        .split_whitespace()
        .take(n)
        .filter(|w| !w.is_empty() && w.bytes().all(|c| c.is_ascii_digit()))
        .filter_map(|w| w.parse().ok())
        .collect();
    (v.len() == n).then_some(v)
}

pub fn run(args: &[String]) -> i32 {
    let dir = state_dir();
    if let Err(e) = make_state_dir(&dir) {
        eprintln!("nero sysmon: cannot make {}: {e}", dir.display());
        return 1;
    }
    match args.first().map(String::as_str) {
        None => {
            panel(&dir);
            0
        }
        Some("refresh") => {
            refresh(&dir);
            0
        }
        // the background fetch started by the panel
        Some("fetch") => {
            fetch_into(&dir.join("claude"));
            let _ = fs::remove_dir(dir.join("claude.lock"));
            0
        }
        Some(other) => {
            eprintln!("nero sysmon: unknown argument {other} (use none or refresh)");
            2
        }
    }
}

/// "Refresh usage" in the dropdown: fetch now, unless the last fetch was under
/// 30 s ago (keeps clicks clear of the rate limit).
fn refresh(dir: &Path) {
    let usage = dir.join("claude");
    let lock = dir.join("claude.lock");
    let (now_s, _) = now();
    if age_of(&usage, now_s) >= sm::USAGE_MIN_AGE_S && fs::create_dir(&lock).is_ok() {
        fetch_into(&usage);
        let _ = fs::remove_dir(&lock);
    }
}

fn panel(dir: &Path) {
    let (now_s, now_ns) = now();
    let usage_path = dir.join("claude");
    let lock = dir.join("claude.lock");

    // Claude usage: fetched in the background every 5 minutes
    if age_of(&usage_path, now_s) >= sm::USAGE_MAX_AGE_S
        && fs::create_dir(&lock).is_ok()
        && !spawn_fetch()
    {
        let _ = fs::remove_dir(&lock);
    }
    // clear a stale lock (e.g. after a crash)
    if lock.is_dir() && age_of(&lock, now_s) > sm::LOCK_STALE_S {
        let _ = fs::remove_dir(&lock);
    }
    let usage = read(&usage_path)
        .map(|t| sm::parse_usage(&t))
        .unwrap_or_default();

    // network (MB/s, all real interfaces)
    let (rx, tx) = read("/proc/net/dev").map_or((0, 0), |t| sm::parse_net_dev(&t));
    let net_path = dir.join("net");
    let (down, up) = match read_numbers(&net_path, 3) {
        Some(p) => sm::net_rates(now_ns - p[0], rx - p[1], tx - p[2]),
        None => ("0".into(), "0".into()),
    };
    let _ = fs::write(&net_path, format!("{now_ns} {rx} {tx}\n"));

    let ram = read("/proc/meminfo").map_or(0, |t| sm::ram_pct(&t));

    // CPU use
    let cpu_path = dir.join("cpu");
    let cpu_now = read("/proc/stat").and_then(|t| sm::parse_cpu_stat(&t));
    let cpu_use = match (cpu_now, read_numbers(&cpu_path, 2)) {
        (Some(now), Some(p)) => sm::cpu_use((p[0], p[1]), now),
        _ => 0,
    };
    if let Some((total, idle)) = cpu_now {
        let _ = fs::write(&cpu_path, format!("{total} {idle}\n"));
    }

    let (gpu_temp, gpu_use) = gpu();
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "nero".into());
    let snapshot = sm::Snapshot {
        usage,
        down,
        up,
        ram_pct: ram,
        cpu_temp: cpu_temp().map_or("-".into(), |t| t.to_string()),
        cpu_use,
        gpu_temp,
        gpu_use,
        refresh_cmd: sm::argos_bash(&exe),
        now: now_s,
    };
    let _ = std::io::stdout().write_all(sm::render(&snapshot).as_bytes());
}

/// The paths matching `parent/prefix*`, sorted like a shell glob.
fn glob(parent: &str, prefix: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

fn cpu_temp() -> Option<i64> {
    for h in glob("/sys/class/hwmon", "hwmon") {
        let name = read(h.join("name")).unwrap_or_default();
        if sm::is_cpu_hwmon(&name)
            && let Some(t) = read(h.join("temp1_input")).and_then(|t| sm::milli_to_deg(&t))
        {
            return Some(t);
        }
    }
    for z in glob("/sys/class/thermal", "thermal_zone") {
        if read(z.join("type")).is_some_and(|t| t.trim() == "x86_pkg_temp")
            && let Some(t) = read(z.join("temp")).and_then(|t| sm::milli_to_deg(&t))
        {
            return Some(t);
        }
    }
    None
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// GPU temperature and use: NVIDIA through nvidia-smi, else AMD through sysfs.
fn gpu() -> (String, String) {
    let dash = || ("-".to_string(), "-".to_string());
    if on_path("nvidia-smi") {
        return Command::new("nvidia-smi")
            .args([
                "--query-gpu=temperature.gpu,utilization.gpu",
                "--format=csv,noheader,nounits",
            ])
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| sm::parse_nvidia(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_else(dash);
    }
    for card in glob("/sys/class/drm", "card") {
        let d = card.join("device");
        if let Some(busy) = read(d.join("gpu_busy_percent")) {
            let temp = glob(&d.join("hwmon").to_string_lossy(), "hwmon")
                .iter()
                .find_map(|h| read(h.join("temp1_input")))
                .and_then(|t| sm::milli_to_deg(&t))
                .map_or("-".to_string(), |t| t.to_string());
            return (temp, sm::number(&busy).unwrap_or_else(|| "-".into()));
        }
    }
    dash()
}

/// Start the fetch as its own session, so it outlives this call.
fn spawn_fetch() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    Command::new("setsid")
        .arg(exe)
        .args(["sysmon", "fetch"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

// ---- the usage fetch ----------------------------------------------------------------

enum Fail {
    /// No Claude Code login on this machine.
    NoLogin,
    /// A named error; shown in the dropdown, the last numbers are kept.
    Err(String),
}

fn fetch_into(out: &Path) {
    let prev = read(out).map_or("- - - - -".to_string(), |t| sm::previous_numbers(&t));
    let line = match fetch() {
        Ok(l) => l,
        Err(Fail::NoLogin) => "nologin - - - - -".to_string(),
        Err(Fail::Err(name)) => format!("err:{name} {prev}"),
    };
    let tmp = out.with_extension("tmp");
    if fs::write(&tmp, format!("{line}\n")).is_ok() {
        let _ = fs::rename(&tmp, out);
    }
}

fn fetch() -> Result<String, Fail> {
    let err = |n: &str| Fail::Err(n.to_string());
    let text = match fs::read_to_string(home().join(".claude/.credentials.json")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Fail::NoLogin),
        Err(_) => return Err(err("ReadError")),
    };
    let cred: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| err("JSONDecodeError"))?;
    let token = cred["claudeAiOauth"]["accessToken"]
        .as_str()
        .ok_or_else(|| err("KeyError"))?;
    // the token goes into a curl config on stdin (never on the command line, where `ps`
    // shows it): only characters that cannot break out of the quoted value
    if token.is_empty()
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._~+/=-".contains(&b))
    {
        return Err(err("BadToken"));
    }
    let config = format!(
        "url = \"https://api.anthropic.com/api/oauth/usage\"\n\
         header = \"Authorization: Bearer {token}\"\n\
         header = \"anthropic-beta: oauth-2025-04-20\"\n\
         header = \"User-Agent: argos-sysmon\"\n"
    );
    let mut child = Command::new("curl")
        .args([
            // first: no ~/.curlrc (a --verbose or --trace there would write the token out)
            "-q",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--connect-timeout",
            "10",
            "--max-time",
            "15",
        ])
        .args(["--write-out", "\n%{http_code}", "--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| err("NoCurl"))?;
    child
        .stdin
        .take()
        .ok_or_else(|| err("NoCurl"))?
        .write_all(config.as_bytes())
        .map_err(|_| err("NoCurl"))?;
    let out = child.wait_with_output().map_err(|_| err("NoCurl"))?;
    if !out.status.success() {
        // curl's exit code 28 is a timeout
        return Err(err(if out.status.code() == Some(28) {
            "TimeoutError"
        } else {
            "URLError"
        }));
    }
    let all = String::from_utf8_lossy(&out.stdout);
    let (body, code) = all.rsplit_once('\n').unwrap_or(("", &all));
    let code: u32 = code.trim().parse().unwrap_or(0);
    if !(200..300).contains(&code) {
        return Err(Fail::Err(format!("HTTP{code}")));
    }
    sm::usage_from_response(body, now().0).map_err(Fail::Err)
}
