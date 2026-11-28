//! The taskbar setup (`nero taskbar`) as pure logic: the options, the Dash to Panel
//! settings it sets, the names of the backups and the Argos wrapper. The `nero`
//! command (cli/) runs the programs (dconf, gsettings, gnome-extensions); what it
//! runs, and in which order, is decided here and tested here.

use serde::{Deserialize, Serialize};

/// The extensions the setup works with.
pub const DTP: &str = "dash-to-panel@jderose9.github.com";
pub const ARGOS: &str = "argos@pew.worldwidemann.com";
pub const SPLIT: &str = "nero-argos-split@LudaLunizz";
pub const UBUNTU_DOCK: &str = "ubuntu-dock@ubuntu.com";
pub const DTP_SCHEMA: &str = "org.gnome.shell.extensions.dash-to-panel";
pub const SPLIT_SCHEMA: &str = "org.gnome.shell.extensions.nero-argos-split";
pub const DCONF_PATH: &str = "/org/gnome/shell/extensions/dash-to-panel/";

/// What `nero taskbar` was asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Task {
    /// Set up the taskbar; the Argos buttons go to `monitor` if given.
    Setup { monitor: Option<String> },
    /// Load the Dash to Panel settings saved by the last setup.
    Undo,
}

/// A connector name like `DP-1` or `HDMI-A-2`.
pub fn valid_connector(c: &str) -> bool {
    !c.is_empty() && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The words after `nero taskbar`: `setup [--monitor CONNECTOR]` or `undo`.
pub fn parse_args(args: &[String]) -> Result<Task, String> {
    let mut it = args.iter();
    match it.next().map(String::as_str) {
        Some("undo") => match it.next() {
            None => Ok(Task::Undo),
            Some(x) => Err(format!("undo takes no options (got {x})")),
        },
        Some("setup") => {
            let mut monitor = None;
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--monitor" => {
                        let c = it
                            .next()
                            .ok_or("--monitor needs a connector name, e.g. DP-1")?;
                        if !valid_connector(c) {
                            return Err(format!("bad connector name: {c}"));
                        }
                        monitor = Some(c.clone());
                    }
                    other => return Err(format!("unknown option: {other}")),
                }
            }
            Ok(Task::Setup { monitor })
        }
        Some(other) => Err(format!(
            "unknown taskbar command: {other} (use setup or undo)"
        )),
        None => Err("taskbar needs a command: setup [--monitor CONNECTOR] or undo".into()),
    }
}

/// The Dash to Panel settings of the Windows-like taskbar: (key, value as
/// `gsettings set` takes it).
pub const DTP_SETTINGS: [(&str, &str); 8] = [
    ("panel-position", "BOTTOM"),
    // no per-monitor override: every monitor uses panel-position
    ("panel-positions", "{}"),
    // running-app dots under the icons
    ("dot-position", "BOTTOM"),
    // pinned apps on every monitor's taskbar
    ("show-favorites-all-monitors", "true"),
    // one icon per app, like Windows' combined buttons
    ("group-apps", "true"),
    // click an app with several windows: window previews
    ("click-action", "TOGGLE-SHOWPREVIEW"),
    ("window-preview-title-position", "TOP"),
    // Dash to Panel's default order: apps button left, clock + tray right
    ("panel-element-positions", "{}"),
];

/// The file a Dash to Panel backup is written to.
pub fn backup_name(stamp: &str) -> String {
    format!("dash-to-panel-{stamp}.dconf")
}

/// The newest setup backup among these file names: `dash-to-panel-STAMP.dconf`
/// or `dash-to-panel-STAMP-N.dconf` (a second one in the same second). Other
/// files (the `-before-undo` save of a failed undo) are never picked. As plain
/// strings `...-2.dconf` would sort before `....dconf`, so it is (stamp, N).
pub fn latest_backup(names: &[String]) -> Option<&String> {
    let key = |n: &String| -> Option<(String, u32)> {
        let stem = n.strip_prefix("dash-to-panel-")?.strip_suffix(".dconf")?;
        let stamp = stem.get(..15)?;
        let ok = stamp.bytes().enumerate().all(|(i, c)| {
            if i == 8 {
                c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
        let n = match &stem[15..] {
            "" => 1,
            rest => {
                let d = rest.strip_prefix('-')?;
                if d.is_empty() || !d.bytes().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                d.parse().ok()?
            }
        };
        ok.then(|| (stamp.to_string(), n))
    };
    names
        .iter()
        .filter_map(|n| key(n).map(|k| (k, n)))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, n)| n)
}

/// True when the folder holds a setup record that was undone already: then the
/// old no-record path must not load a Dash to Panel backup a second time.
pub fn already_undone(names: &[String]) -> bool {
    names
        .iter()
        .any(|n| n.starts_with("taskbar-setup.") && n.ends_with(".undone.json"))
}

/// A `YYYYMMDD-HHMMSS` stamp (UTC) from seconds since 1970, for when date(1) can't
/// be run. Same shape as the local one, so backups keep sorting by time.
pub fn utc_stamp(secs: u64) -> String {
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    // days to a civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// The file that records what a setup changed, so undo can put each thing back.
/// It is written by the first setup and kept by later ones until an undo.
pub const RECORD: &str = "taskbar-setup.json";

/// What was there before the first `nero taskbar setup` (since the last undo).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// The Dash to Panel dconf backup (a file name in the state folder).
    pub dconf_backup: String,
    pub dock_enabled: bool,
    pub dtp_enabled: bool,
    /// `None` while Argos was not installed at setup time.
    pub argos_enabled: Option<bool>,
    pub sysmon: SysmonBefore,
    /// The split extension was put in by setup (not there before).
    pub split_added: bool,
}

/// The Argos script `sysmon.1s.sh` before setup.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum SysmonBefore {
    /// Setup did not write the script (Argos was not installed).
    #[default]
    Untouched,
    /// There was no script: undo removes the wrapper.
    Absent,
    /// There was the wrapper already: nothing to put back.
    Wrapper,
    /// Another script, saved under this file name in the state folder.
    Saved(String),
}

/// What a setup run found, before it changes anything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub dock_enabled: bool,
    pub dtp_enabled: bool,
    pub argos_enabled: Option<bool>,
}

/// The record after this setup run. A record of an earlier setup (no undo since)
/// keeps what it saw first: that is the state undo goes back to. Only what the
/// earlier run did not touch is filled in now.
pub fn merge_record(old: Option<Record>, found: &Found, dconf_backup: &str) -> Record {
    match old {
        None => Record {
            dconf_backup: dconf_backup.to_string(),
            dock_enabled: found.dock_enabled,
            dtp_enabled: found.dtp_enabled,
            argos_enabled: found.argos_enabled,
            sysmon: SysmonBefore::Untouched,
            split_added: false,
        },
        Some(mut r) => {
            if r.argos_enabled.is_none() {
                r.argos_enabled = found.argos_enabled;
            }
            r
        }
    }
}

/// The record once the sysmon script was looked at (`before` = what was there).
pub fn note_sysmon(r: &mut Record, before: SysmonBefore) {
    if r.sysmon == SysmonBefore::Untouched {
        r.sysmon = before;
    }
}

/// The record once the split extension is about to be written.
pub fn note_split(r: &mut Record, present_before: bool) {
    if !present_before {
        r.split_added = true;
    }
}

/// One step of an undo, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum UndoStep {
    LoadDconf(String),
    Enable(&'static str),
    Disable(&'static str),
    /// Put the saved script back.
    RestoreSysmon(String),
    /// Remove the wrapper (there was no script before).
    RemoveSysmon,
    /// Disable and remove the split extension.
    RemoveSplit,
}

/// The steps that put back what `r` recorded.
pub fn undo_steps(r: &Record) -> Vec<UndoStep> {
    let mut v = vec![UndoStep::LoadDconf(r.dconf_backup.clone())];
    if r.split_added {
        v.push(UndoStep::RemoveSplit);
    }
    match &r.sysmon {
        SysmonBefore::Saved(f) => v.push(UndoStep::RestoreSysmon(f.clone())),
        SysmonBefore::Absent => v.push(UndoStep::RemoveSysmon),
        SysmonBefore::Untouched | SysmonBefore::Wrapper => {}
    }
    if r.argos_enabled == Some(false) {
        v.push(UndoStep::Disable(ARGOS));
    }
    if !r.dtp_enabled {
        v.push(UndoStep::Disable(DTP));
    }
    if r.dock_enabled {
        v.push(UndoStep::Enable(UBUNTU_DOCK));
    }
    v
}

/// Whether `gnome-extensions info` (run with LC_ALL=C) says the extension is enabled.
pub fn info_enabled(info: &str) -> bool {
    info.lines()
        .find_map(|l| l.trim_start().strip_prefix("Enabled:"))
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("yes"))
}

/// The folder of an extension from `gnome-extensions info` (its `Path:` line).
pub fn extension_path(info: &str) -> Option<String> {
    info.lines()
        .find_map(|l| l.trim_start().strip_prefix("Path:"))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
}

/// The Argos script `sysmon.1s.sh`: Argos runs it every second, it hands over to
/// the `nero` program at `exe`.
pub fn argos_wrapper(exe: &str) -> String {
    format!(
        "#!/bin/sh\n# Installed by `nero taskbar setup`: the system monitor is the nero program.\nexec {} sysmon \"$@\"\n",
        shell_quote(exe)
    )
}

/// True for a script `argos_wrapper` wrote, whatever program path it names (a
/// setup by one `nero` binary, an undo by another, e.g. after `cargo install`).
pub fn is_wrapper(text: &str) -> bool {
    let mut lines = text.split_inclusive('\n');
    let (Some(a), Some(b), Some(c), None) =
        (lines.next(), lines.next(), lines.next(), lines.next())
    else {
        return false;
    };
    let template = argos_wrapper("X");
    let mut t = template.split_inclusive('\n');
    a == t.next().unwrap_or_default()
        && b == t.next().unwrap_or_default()
        && c.starts_with("exec ")
        && c.ends_with(" sysmon \"$@\"\n")
}

/// A word for a POSIX shell, in single quotes when it needs them.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+=:,@%".contains(&b))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn arguments() {
        assert_eq!(
            parse_args(&a(&["setup"])),
            Ok(Task::Setup { monitor: None })
        );
        assert_eq!(
            parse_args(&a(&["setup", "--monitor", "HDMI-A-2"])),
            Ok(Task::Setup {
                monitor: Some("HDMI-A-2".into())
            })
        );
        assert_eq!(parse_args(&a(&["undo"])), Ok(Task::Undo));
        for bad in [
            &[][..],
            &["setup", "--monitor"],
            &["setup", "--monitor", "DP 1"],
            &["setup", "--monitor", "$(x)"],
            &["setup", "--monitor", ""],
            &["setup", "--fast"],
            &["undo", "now"],
            &["nope"],
        ] {
            assert!(parse_args(&a(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn settings_are_the_bash_ones() {
        let keys: Vec<&str> = DTP_SETTINGS.iter().map(|s| s.0).collect();
        assert_eq!(
            keys,
            [
                "panel-position",
                "panel-positions",
                "dot-position",
                "show-favorites-all-monitors",
                "group-apps",
                "click-action",
                "window-preview-title-position",
                "panel-element-positions"
            ]
        );
        assert_eq!(DTP_SETTINGS[0].1, "BOTTOM");
        assert_eq!(DTP_SETTINGS[1].1, "{}");
    }

    #[test]
    fn backups() {
        let names = a(&[
            "dash-to-panel-20261126-090000.dconf",
            "dash-to-panel-20261127-013000.dconf",
            "sysmon.1s.sh.20261127-013000.bak",
            "dash-to-panel-20261125-230000.dconf",
        ]);
        assert_eq!(
            latest_backup(&names).unwrap(),
            "dash-to-panel-20261127-013000.dconf"
        );
        assert_eq!(latest_backup(&a(&["layout.json"])), None);
        // a second backup in the same second is the newer one
        let same = a(&[
            "dash-to-panel-20261127-013000.dconf",
            "dash-to-panel-20261127-013000-2.dconf",
        ]);
        assert_eq!(latest_backup(&same).unwrap(), &same[1]);
        // the save a failed undo makes is never loaded as a setup backup
        let undo = a(&[
            "dash-to-panel-20261127-013000.dconf",
            "dash-to-panel-20261128-090000-before-undo.dconf",
        ]);
        assert_eq!(latest_backup(&undo).unwrap(), &undo[0]);
        assert!(already_undone(&a(&[
            "taskbar-setup.20261128-090000.undone.json"
        ])));
        assert!(!already_undone(&a(&["taskbar-setup.json"])));
        assert_eq!(
            backup_name("20261127-013000"),
            "dash-to-panel-20261127-013000.dconf"
        );
    }

    #[test]
    fn utc_stamps() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        assert_eq!(utc_stamp(1_795_826_096), "20261128-003456");
        assert_eq!(utc_stamp(951_782_400), "20000229-000000");
        // the fallback sorts with the date(1) ones
        let names = a(&[
            &backup_name("20261126-090000"),
            &backup_name(&utc_stamp(1_795_826_096)),
        ]);
        assert_eq!(latest_backup(&names).unwrap(), &names[1]);
    }

    fn found(dock: bool, dtp: bool, argos: Option<bool>) -> Found {
        Found {
            dock_enabled: dock,
            dtp_enabled: dtp,
            argos_enabled: argos,
        }
    }

    #[test]
    fn a_second_setup_keeps_the_first_record() {
        let mut first = merge_record(None, &found(true, false, None), "dash-to-panel-1.dconf");
        note_sysmon(&mut first, SysmonBefore::Saved("sysmon.1s.sh.1.bak".into()));
        // second run: the dock is now off, DtP on, the script is the wrapper, Argos now there
        let mut second = merge_record(
            Some(first.clone()),
            &found(false, true, Some(true)),
            "dash-to-panel-2.dconf",
        );
        note_sysmon(&mut second, SysmonBefore::Wrapper);
        note_split(&mut second, false);
        assert_eq!(second.dconf_backup, "dash-to-panel-1.dconf");
        assert!(second.dock_enabled && !second.dtp_enabled);
        assert_eq!(
            second.sysmon,
            SysmonBefore::Saved("sysmon.1s.sh.1.bak".into())
        );
        assert_eq!(second.argos_enabled, Some(true));
        assert!(second.split_added);
        // a split that was there before setup is never removed
        let mut r = merge_record(None, &found(false, true, None), "x.dconf");
        note_split(&mut r, true);
        assert!(!r.split_added);
    }

    #[test]
    fn undo_puts_back_each_thing() {
        let r = Record {
            dconf_backup: "dash-to-panel-1.dconf".into(),
            dock_enabled: true,
            dtp_enabled: false,
            argos_enabled: Some(false),
            sysmon: SysmonBefore::Saved("s.bak".into()),
            split_added: true,
        };
        assert_eq!(
            undo_steps(&r),
            [
                UndoStep::LoadDconf("dash-to-panel-1.dconf".into()),
                UndoStep::RemoveSplit,
                UndoStep::RestoreSysmon("s.bak".into()),
                UndoStep::Disable(ARGOS),
                UndoStep::Disable(DTP),
                UndoStep::Enable(UBUNTU_DOCK),
            ]
        );
        // everything was already as setup leaves it: only the settings go back
        let r = Record {
            dconf_backup: "b.dconf".into(),
            dtp_enabled: true,
            argos_enabled: Some(true),
            sysmon: SysmonBefore::Wrapper,
            ..Record::default()
        };
        assert_eq!(undo_steps(&r), [UndoStep::LoadDconf("b.dconf".into())]);
        let r = Record {
            sysmon: SysmonBefore::Absent,
            dtp_enabled: true,
            ..Record::default()
        };
        assert!(undo_steps(&r).contains(&UndoStep::RemoveSysmon));
        // the record survives a round trip through its file
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), r);
    }

    #[test]
    fn enabled_line() {
        assert!(info_enabled(
            "x\n  Path: /p\n  Enabled: Yes\n  State: ACTIVE\n"
        ));
        assert!(!info_enabled("x\n  Enabled: No\n"));
        assert!(!info_enabled("x\n  State: ACTIVE\n"));
    }

    #[test]
    fn extension_info() {
        let info = "dash-to-panel@jderose9.github.com\n  Name: Dash to Panel\n  Path: /home/u/.local/share/gnome-shell/extensions/dash-to-panel@jderose9.github.com\n  State: ACTIVE\n";
        assert_eq!(
            extension_path(info).unwrap(),
            "/home/u/.local/share/gnome-shell/extensions/dash-to-panel@jderose9.github.com"
        );
        assert_eq!(extension_path("no path here"), None);
    }

    #[test]
    fn wrapper_and_quoting() {
        assert_eq!(
            argos_wrapper("/home/u/.local/bin/nero"),
            "#!/bin/sh\n# Installed by `nero taskbar setup`: the system monitor is the nero program.\nexec /home/u/.local/bin/nero sysmon \"$@\"\n"
        );
        assert!(
            argos_wrapper("/home/my user/it's/nero")
                .contains("exec '/home/my user/it'\\''s/nero' sysmon")
        );
        assert_eq!(shell_quote(""), "''");
        // any nero wrapper is ours, a changed one is not
        assert!(is_wrapper(&argos_wrapper("/home/u/.cargo/bin/nero")));
        assert!(is_wrapper(&argos_wrapper("/home/my user/it's/nero")));
        let mut edited = argos_wrapper("/x/nero");
        edited.push_str("echo mine\n");
        assert!(!is_wrapper(&edited));
        assert!(!is_wrapper("#!/bin/bash\necho cpu\n"));
    }
}
