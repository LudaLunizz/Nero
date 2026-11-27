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
        // gnome-extensions info: the labels (Path:, Enabled:) are read, keep them English
        .env("LC_ALL", "C")
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
        .env("LC_ALL", "C")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `gnome-extensions info`, or None when the extension is not installed.
fn extension_info(uuid: &str) -> Option<String> {
    Command::new("gnome-extensions")
        .args(["info", uuid])
        .env("LC_ALL", "C")
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn have_extension(uuid: &str) -> bool {
    extension_info(uuid).is_some()
}

fn is_enabled(uuid: &str) -> bool {
    extension_info(uuid).is_some_and(|i| tb::info_enabled(&i))
}

/// `YYYYMMDD-HHMMSS` in local time, from date(1); UTC if date can't be run.
fn stamp() -> String {
    run("date", &["+%Y%m%d-%H%M%S"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| {
            tb::utc_stamp(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            )
        })
}

/// A new file in `dir` named `name`, or `name` with -2, -3, ... before the
/// extension when that is taken (two runs in the same second). Never overwrites.
fn write_new(dir: &Path, name: &str, text: &[u8]) -> Res<PathBuf> {
    let (base, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    for n in 1..100 {
        let file = if n == 1 {
            name.to_string()
        } else {
            format!("{base}-{n}.{ext}")
        };
        let p = dir.join(&file);
        match fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(mut f) => {
                f.write_all(text)
                    .map_err(|e| format!("cannot write {}: {e}", p.display()))?;
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot write {}: {e}", p.display())),
        }
    }
    Err(format!("too many files named {name} in {}", dir.display()))
}

/// Write `target` in one step: a temp file next to it, its mode set, then renamed
/// over it. Argos runs the script every second and never sees half of it; a
/// symlink at `target` is replaced, not written through.
fn write_atomic(target: &Path, text: &[u8], mode: u32) -> Res {
    let name = target
        .file_name()
        .map_or("file".into(), |n| n.to_string_lossy().into_owned());
    let tmp = target.with_file_name(format!(".{name}.nero-tmp"));
    let _ = fs::remove_file(&tmp);
    fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot write {}: {e}", target.display())
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

fn read_record(state: &Path) -> Res<Option<tb::Record>> {
    let p = state.join(tb::RECORD);
    match fs::read_to_string(&p) {
        Ok(t) => serde_json::from_str(&t).map(Some).map_err(|e| {
            format!(
                "{} is damaged ({e}); move it away to start over",
                p.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", p.display())),
    }
}

fn write_record(state: &Path, r: &tb::Record) -> Res {
    let text = serde_json::to_string_pretty(r).map_err(|e| e.to_string())?;
    write_atomic(&state.join(tb::RECORD), text.as_bytes(), 0o600)
}

fn sysmon_path() -> PathBuf {
    home().join(".config/argos/sysmon.1s.sh")
}

fn split_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
        .join("gnome-shell/extensions")
        .join(tb::SPLIT)
}

/// Load a dconf dump into the Dash to Panel path. The settings there now are kept
/// aside first and put back if the load fails, so a failed undo changes nothing.
fn load_dconf(state: &Path, file: &Path) -> Res {
    let saved = fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let now = run("dconf", &["dump", tb::DCONF_PATH])?;
    let load = |text: &[u8]| -> Res {
        run("dconf", &["reset", "-f", tb::DCONF_PATH])?;
        let mut child = Command::new("dconf")
            .args(["load", tb::DCONF_PATH])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run dconf: {e}"))?;
        child
            .stdin
            .take()
            .ok_or("cannot run dconf")?
            .write_all(text)
            .map_err(|e| format!("cannot run dconf: {e}"))?;
        let status = child.wait().map_err(|e| format!("cannot run dconf: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("dconf load failed ({status})"))
        }
    };
    if let Err(e) = load(&saved) {
        let back = write_new(
            state,
            &tb::backup_name(&format!("{}-before-undo", stamp())),
            now.as_bytes(),
        );
        return match load(now.as_bytes()) {
            Ok(()) => Err(format!(
                "{e}; the settings are as they were before the undo"
            )),
            Err(e2) => Err(format!(
                "{e}; putting the settings back failed too ({e2}), they are saved in {}",
                back.map_or_else(|x| x, |p| p.display().to_string())
            )),
        };
    }
    Ok(())
}

fn undo() -> Res {
    let state = state_dir();
    let Some(record) = read_record(&state)? else {
        // a setup from before the record file: only its Dash to Panel backup
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
        load_dconf(&state, &file)?;
        println!("Dash to Panel settings put back from {}", file.display());
        return Ok(());
    };
    for step in tb::undo_steps(&record) {
        match step {
            tb::UndoStep::LoadDconf(name) => {
                let file = state.join(&name);
                load_dconf(&state, &file)?;
                println!("Dash to Panel settings put back from {}", file.display());
            }
            tb::UndoStep::RemoveSplit => {
                run_quiet("gnome-extensions", &["disable", tb::SPLIT]);
                let dir = split_dir();
                if dir.exists() {
                    fs::remove_dir_all(&dir)
                        .map_err(|e| format!("cannot remove {}: {e}", dir.display()))?;
                }
                println!("removed {}", tb::SPLIT);
            }
            tb::UndoStep::RestoreSysmon(name) => {
                let bak = state.join(&name);
                let old =
                    fs::read(&bak).map_err(|e| format!("cannot read {}: {e}", bak.display()))?;
                write_atomic(&sysmon_path(), &old, 0o755)?;
                println!("your old sysmon.1s.sh is back");
            }
            tb::UndoStep::RemoveSysmon => {
                let target = sysmon_path();
                let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned());
                // only our wrapper goes, never a script written since
                if exe.is_ok_and(|exe| {
                    fs::read_to_string(&target).is_ok_and(|t| t == tb::argos_wrapper(&exe))
                }) {
                    fs::remove_file(&target)
                        .map_err(|e| format!("cannot remove {}: {e}", target.display()))?;
                    println!("removed sysmon.1s.sh");
                }
            }
            tb::UndoStep::Enable(uuid) => {
                if !run_quiet("gnome-extensions", &["enable", uuid]) {
                    println!("could not enable {uuid}: gnome-extensions enable {uuid}");
                }
            }
            tb::UndoStep::Disable(uuid) => {
                run_quiet("gnome-extensions", &["disable", uuid]);
            }
        }
    }
    // the next setup starts a new record; this one stays for reference
    let rec = state.join(tb::RECORD);
    let done = state.join(format!("taskbar-setup.{}.undone.json", stamp()));
    fs::rename(&rec, &done).map_err(|e| format!("cannot move {}: {e}", rec.display()))?;
    println!("undo done (a change to the extensions shows after the next log in on Wayland)");
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
    let old = read_record(&state)?;
    let argos = have_extension(tb::ARGOS);
    let found = tb::Found {
        dock_enabled: is_enabled(tb::UBUNTU_DOCK),
        dtp_enabled: is_enabled(tb::DTP),
        argos_enabled: argos.then(|| is_enabled(tb::ARGOS)),
    };
    let mut record = if old.is_some() {
        println!("setup ran before without an undo: keeping the settings saved by that first run");
        tb::merge_record(old, &found, "")
    } else {
        let dump = run("dconf", &["dump", tb::DCONF_PATH])?;
        let backup = write_new(&state, &tb::backup_name(&stamp), dump.as_bytes())?;
        println!(
            "saved the current Dash to Panel settings to {}",
            backup.display()
        );
        let name = backup.file_name().unwrap_or_default().to_string_lossy();
        tb::merge_record(None, &found, &name)
    };
    // on disk before anything changes, so a setup that stops halfway can be undone
    write_record(&state, &record)?;

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
    if argos {
        let before = install_sysmon(&state, &stamp)?;
        tb::note_sysmon(&mut record, before);
        write_record(&state, &record)?;
        run("gnome-extensions", &["enable", tb::ARGOS])?;
        println!("sysmon.1s.sh installed in ~/.config/argos");
    } else {
        println!(
            "Argos is not installed, skipped the sysmon script: https://extensions.gnome.org/extension/1176/argos/"
        );
    }

    // 3. optional: the Argos buttons on another monitor's taskbar
    if let Some(m) = monitor {
        tb::note_split(&mut record, split_dir().exists());
        write_record(&state, &record)?;
        install_split(m)?;
    }
    println!("done (a new extension shows up after the next log in on Wayland)");
    Ok(())
}

/// The Argos script that hands over to this program. An older script of another
/// kind (the bash version, or your own) is saved first. Says what was there.
fn install_sysmon(state: &Path, stamp: &str) -> Res<tb::SysmonBefore> {
    let target = sysmon_path();
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    let wrapper = tb::argos_wrapper(&exe.to_string_lossy());
    let before = match fs::read(&target) {
        Ok(old) if old == wrapper.as_bytes() => tb::SysmonBefore::Wrapper,
        Ok(old) => {
            let bak = write_new(state, &format!("sysmon.1s.sh.{stamp}.bak"), &old)
                .map_err(|e| format!("cannot save the old script: {e}"))?;
            tb::SysmonBefore::Saved(
                bak.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => tb::SysmonBefore::Absent,
        Err(e) => return Err(format!("cannot read {}: {e}", target.display())),
    };
    write_atomic(&target, wrapper.as_bytes(), 0o755)?;
    Ok(before)
}

fn install_split(monitor: &str) -> Res {
    let dir = split_dir();
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
