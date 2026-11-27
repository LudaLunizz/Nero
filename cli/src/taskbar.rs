//! `nero taskbar`: runs dconf, gsettings and gnome-extensions as the core's
//! `taskbar` module says (which settings, which names, in what order).

use nero_core::taskbar as tb;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

// The extension that moves the Argos buttons, shipped inside this program.
const SPLIT_JS: &str = include_str!("../../taskbar/nero-argos-split@LudaLunizz/extension.js");
const SPLIT_META: &str = include_str!("../../taskbar/nero-argos-split@LudaLunizz/metadata.json");
const SPLIT_XML: &str = include_str!(
    "../../taskbar/nero-argos-split@LudaLunizz/schemas/org.gnome.shell.extensions.nero-argos-split.gschema.xml"
);

type Res<T = ()> = Result<T, String>;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
}

fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("nero")
}

fn run_out(program: &str, args: &[&str]) -> Res<Output> {
    Command::new(program)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run {program}: {e}"))
}

/// Run a program that has to succeed.
fn run(program: &str, args: &[&str]) -> Res<String> {
    let o = run_out(program, args)?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        Err(format!(
            "{program} {} failed ({})",
            args.join(" "),
            o.status
        ))
    }
}

/// Run a program whose failure does not matter.
fn run_quiet(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn have_extension(uuid: &str) -> bool {
    run_quiet("gnome-extensions", &["info", uuid])
}

/// `YYYYMMDD-HHMMSS` in local time, from date(1).
fn stamp() -> String {
    run("date", &["+%Y%m%d-%H%M%S"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            format!("t{secs}")
        })
}

pub fn run_args(args: &[String]) -> i32 {
    let result = match tb::parse_args(args) {
        Ok(tb::Task::Setup { monitor }) => setup(monitor.as_deref()),
        Ok(tb::Task::Undo) => undo(),
        Err(e) => {
            eprintln!("nero taskbar: {e}");
            return 2;
        }
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn undo() -> Res {
    let state = state_dir();
    let names: Vec<String> = fs::read_dir(&state)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let last = tb::latest_backup(&names).ok_or(format!(
        "nothing to undo: no saved settings in {}",
        state.display()
    ))?;
    let file = state.join(last);
    let saved =
        fs::File::open(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    run("dconf", &["reset", "-f", tb::DCONF_PATH])?;
    let status = Command::new("dconf")
        .args(["load", tb::DCONF_PATH])
        .stdin(saved)
        .status()
        .map_err(|e| format!("cannot run dconf: {e}"))?;
    if !status.success() {
        return Err(format!("dconf load failed ({status})"));
    }
    println!("Dash to Panel settings put back from {}", file.display());
    Ok(())
}

fn setup(monitor: Option<&str>) -> Res {
    if !have_extension(tb::DTP) {
        return Err("Dash to Panel is not installed: https://extensions.gnome.org/extension/1160/dash-to-panel/".into());
    }
    let state = state_dir();
    let stamp = stamp();

    // 1. back up, then the taskbar at the bottom of every monitor
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&state)
        .map_err(|e| format!("cannot make {}: {e}", state.display()))?;
    let dump = run("dconf", &["dump", tb::DCONF_PATH])?;
    let backup = state.join(tb::backup_name(&stamp));
    fs::write(&backup, dump).map_err(|e| format!("cannot write {}: {e}", backup.display()))?;
    println!(
        "saved the current Dash to Panel settings to {}",
        backup.display()
    );

    // Dash to Panel keeps its schema in its own folder, where plain gsettings does not look
    let dtp_dir = tb::extension_path(&run("gnome-extensions", &["info", tb::DTP])?)
        .filter(|d| Path::new(d).join("schemas").is_dir())
        .ok_or("can't find Dash to Panel's schemas folder")?;
    let schemas = format!("{dtp_dir}/schemas");
    for (key, value) in tb::DTP_SETTINGS {
        run(
            "gsettings",
            &["--schemadir", &schemas, "set", tb::DTP_SCHEMA, key, value],
        )?;
    }
    // Ubuntu's own dock would sit next to it
    run_quiet("gnome-extensions", &["disable", tb::UBUNTU_DOCK]);
    run("gnome-extensions", &["enable", tb::DTP])?;

    // 2. the system monitor in Argos
    if have_extension(tb::ARGOS) {
        install_sysmon(&state, &stamp)?;
        run("gnome-extensions", &["enable", tb::ARGOS])?;
        println!("sysmon.1s.sh installed in ~/.config/argos");
    } else {
        println!(
            "Argos is not installed, skipped the sysmon script: https://extensions.gnome.org/extension/1176/argos/"
        );
    }

    // 3. optional: the Argos buttons on another monitor's taskbar
    if let Some(m) = monitor {
        install_split(m)?;
    }
    println!("done (a new extension shows up after the next log in on Wayland)");
    Ok(())
}

/// The Argos script that hands over to this program. An older script of another
/// kind (the bash version, or your own) is saved first.
fn install_sysmon(state: &Path, stamp: &str) -> Res {
    let dir = home().join(".config/argos");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    let wrapper = tb::argos_wrapper(&exe.to_string_lossy());
    let target = dir.join("sysmon.1s.sh");
    if let Ok(old) = fs::read_to_string(&target)
        && old != wrapper
    {
        let bak = state.join(format!("sysmon.1s.sh.{stamp}.bak"));
        fs::write(&bak, old)
            .map_err(|e| format!("cannot save the old script to {}: {e}", bak.display()))?;
    }
    fs::write(&target, wrapper).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

fn install_split(monitor: &str) -> Res {
    let dir = xdg("XDG_DATA_HOME", ".local/share")
        .join("gnome-shell/extensions")
        .join(tb::SPLIT);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("cannot replace {}: {e}", dir.display()))?;
    }
    let schemas = dir.join("schemas");
    fs::create_dir_all(&schemas).map_err(|e| format!("cannot make {}: {e}", schemas.display()))?;
    let put = |p: PathBuf, text: &str| {
        fs::write(&p, text).map_err(|e| format!("cannot write {}: {e}", p.display()))
    };
    put(dir.join("extension.js"), SPLIT_JS)?;
    put(dir.join("metadata.json"), SPLIT_META)?;
    put(
        schemas.join("org.gnome.shell.extensions.nero-argos-split.gschema.xml"),
        SPLIT_XML,
    )?;
    let schemas = schemas.to_string_lossy().into_owned();
    run("glib-compile-schemas", &[&schemas])?;
    run(
        "gsettings",
        &[
            "--schemadir",
            &schemas,
            "set",
            tb::SPLIT_SCHEMA,
            "monitor",
            monitor,
        ],
    )?;
    if !run_quiet("gnome-extensions", &["enable", tb::SPLIT]) {
        println!(
            "log out and back in once, then: gnome-extensions enable {}",
            tb::SPLIT
        );
    }
    println!("Argos buttons go to the taskbar on {monitor}");
    let _ = std::io::stdout().flush();
    Ok(())
}
