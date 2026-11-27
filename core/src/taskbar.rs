//! The taskbar setup (`nero taskbar`) as pure logic: the options, the Dash to Panel
//! settings it sets, the names of the backups and the Argos wrapper. The `nero`
//! command (cli/) runs the programs (dconf, gsettings, gnome-extensions); what it
//! runs, and in which order, is decided here and tested here.

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

/// The newest backup among these file names (the stamps sort by time).
pub fn latest_backup(names: &[String]) -> Option<&String> {
    names
        .iter()
        .filter(|n| n.starts_with("dash-to-panel-") && n.ends_with(".dconf"))
        .max()
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
        assert_eq!(
            backup_name("20261127-013000"),
            "dash-to-panel-20261127-013000.dconf"
        );
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
    }
}
