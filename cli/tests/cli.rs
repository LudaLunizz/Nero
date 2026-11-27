//! The `nero` command, end to end, in a throwaway HOME. The real desktop is never
//! touched: the sysmon runs without a Claude login (so no network call), and the
//! taskbar setup runs against fake dconf / gsettings / gnome-extensions programs
//! that only write down what they were asked.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nero-cli-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn nero(home: &Path, extra_path: Option<&Path>, args: &[&str]) -> Output {
    let mut path = String::from("/usr/bin:/bin");
    if let Some(p) = extra_path {
        path = format!("{}:{path}", p.display());
    }
    Command::new(env!("CARGO_BIN_EXE_nero"))
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("PATH", path)
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn sysmon_panel_and_dropdown() {
    let home = tmp("sysmon");
    fs::create_dir_all(home.join("run")).unwrap();
    let first = nero(&home, None, &["sysmon"]);
    assert!(first.status.success());
    std::thread::sleep(Duration::from_millis(1100));
    let out = stdout(&nero(&home, None, &["sysmon"]));
    // the background usage fetch has no login to use: it says so
    let usage = home.join("run/argos-sysmon/claude");
    let t = Instant::now();
    while !fs::read_to_string(&usage).is_ok_and(|s| s.starts_with("nologin")) {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "no usage state: {:?}",
            fs::read_to_string(&usage)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        out.lines().next().unwrap().starts_with("<tt>")
            && out
                .lines()
                .next()
                .unwrap()
                .ends_with("</tt> | useMarkup=true"),
        "{out}"
    );
    assert!(out.lines().any(|l| l == "---"), "{out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("RAM used: ") && l.ends_with('%')),
        "{out}"
    );
    assert!(out.lines().any(|l| l.starts_with("CPU: ")), "{out}");
    assert!(out.contains("param1=sysmon param2=refresh"), "{out}");
    let mode = fs::metadata(home.join("run/argos-sysmon"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    // the next panel shows the sign-in hint
    let out = stdout(&nero(&home, None, &["sysmon"]));
    assert!(
        out.contains("Claude usage: sign in to Claude Code first (run: claude)"),
        "{out}"
    );
    // a refresh within 30 s of the last fetch does nothing and exits well
    assert!(nero(&home, None, &["sysmon", "refresh"]).status.success());
    // state with something planted in it is not trusted
    fs::write(&usage, "$(touch /tmp/nero-planted) 1 2 a b c\n").unwrap();
    let out = stdout(&nero(&home, None, &["sysmon"]));
    assert!(out.contains("Claude usage: loading…"), "{out}");
    assert!(!Path::new("/tmp/nero-planted").exists());
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn unknown_commands_are_refused() {
    let home = tmp("unknown");
    assert_eq!(nero(&home, None, &["frobnicate"]).status.code(), Some(2));
    assert_eq!(nero(&home, None, &["sysmon", "x"]).status.code(), Some(2));
    assert_eq!(nero(&home, None, &["taskbar"]).status.code(), Some(2));
    assert_eq!(
        nero(&home, None, &["taskbar", "setup", "--monitor", "DP 1"])
            .status
            .code(),
        Some(2)
    );
    assert!(stdout(&nero(&home, None, &["--help"])).contains("nero taskbar setup"));
    assert!(stdout(&nero(&home, None, &["--version"])).starts_with("nero "));
    let _ = fs::remove_dir_all(&home);
}

/// A folder with fake programs that log their arguments to `calls.log`.
fn fake_tools(home: &Path, argos_installed: bool, dtp_installed: bool) -> PathBuf {
    let bin = home.join("fakebin");
    fs::create_dir_all(&bin).unwrap();
    let dtp = home.join("data/gnome-shell/extensions/dash-to-panel@jderose9.github.com");
    fs::create_dir_all(dtp.join("schemas")).unwrap();
    let script = |name: &str, body: &str| {
        let p = bin.join(name);
        fs::write(
            &p,
            format!(
                "#!/bin/sh\necho \"{name} $*\" >> \"{}/calls.log\"\n{body}\n",
                home.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    };
    script(
        "gnome-extensions",
        &format!(
            "case \"$1 $2\" in\n\
             'info dash-to-panel@jderose9.github.com') [ {dtp_installed} = true ] && echo \"  Path: {}\" || exit 1;;\n\
             'info argos@pew.worldwidemann.com') [ {argos_installed} = true ] || exit 1;;\n\
             'enable nero-argos-split@LudaLunizz') exit 1;;\n\
             esac\nexit 0",
            dtp.display()
        ),
    );
    script(
        "dconf",
        "case \"$1\" in dump) echo '[/]'; echo \"panel-position='TOP'\";; esac",
    );
    script("gsettings", "exit 0");
    script("glib-compile-schemas", "exit 0");
    bin
}

fn calls(home: &Path) -> Vec<String> {
    fs::read_to_string(home.join("calls.log"))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn taskbar_setup_and_undo() {
    let home = tmp("taskbar");
    let bin = fake_tools(&home, true, true);
    // an old bash script of the user's is saved before the wrapper replaces it
    fs::create_dir_all(home.join(".config/argos")).unwrap();
    fs::write(
        home.join(".config/argos/sysmon.1s.sh"),
        "#!/bin/bash\necho old\n",
    )
    .unwrap();

    let o = nero(
        &home,
        Some(&bin),
        &["taskbar", "setup", "--monitor", "DP-1"],
    );
    assert!(
        o.status.success(),
        "{} {}",
        stdout(&o),
        String::from_utf8_lossy(&o.stderr)
    );
    let c = calls(&home);

    // 8 Dash to Panel settings, each with the schema folder of the extension
    let sets: Vec<&String> = c
        .iter()
        .filter(|l| {
            l.starts_with("gsettings")
                && l.contains(" set org.gnome.shell.extensions.dash-to-panel ")
        })
        .collect();
    assert_eq!(sets.len(), 8, "{c:#?}");
    assert!(
        sets[0].contains("--schemadir") && sets[0].ends_with("panel-position BOTTOM"),
        "{}",
        sets[0]
    );
    assert!(c.contains(&"dconf dump /org/gnome/shell/extensions/dash-to-panel/".to_string()));
    // the dump is on disk before the first setting is changed
    let dump_at = c.iter().position(|l| l.starts_with("dconf dump")).unwrap();
    let first_set = c
        .iter()
        .position(|l| l.contains(" set org.gnome.shell.extensions.dash-to-panel "))
        .unwrap();
    assert!(dump_at < first_set);
    let backups: Vec<_> = fs::read_dir(home.join("state/nero"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        backups
            .iter()
            .any(|n| n.starts_with("dash-to-panel-") && n.ends_with(".dconf")),
        "{backups:?}"
    );
    assert!(
        backups
            .iter()
            .any(|n| n.starts_with("sysmon.1s.sh.") && n.ends_with(".bak")),
        "{backups:?}"
    );
    let mode = fs::metadata(home.join("state/nero"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    assert!(c.contains(&"gnome-extensions disable ubuntu-dock@ubuntu.com".to_string()));
    assert!(c.contains(&"gnome-extensions enable dash-to-panel@jderose9.github.com".to_string()));

    // the Argos script hands over to this program, and is executable
    let wrapper = fs::read_to_string(home.join(".config/argos/sysmon.1s.sh")).unwrap();
    assert!(
        wrapper.starts_with("#!/bin/sh\n") && wrapper.contains(" sysmon \"$@\""),
        "{wrapper}"
    );
    assert!(wrapper.contains(env!("CARGO_BIN_EXE_nero")), "{wrapper}");
    assert_eq!(
        fs::metadata(home.join(".config/argos/sysmon.1s.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert!(c.contains(&"gnome-extensions enable argos@pew.worldwidemann.com".to_string()));

    // the split extension is written out, its schema compiled, the monitor set
    let ext = home.join("data/gnome-shell/extensions/nero-argos-split@LudaLunizz");
    for f in [
        "extension.js",
        "metadata.json",
        "schemas/org.gnome.shell.extensions.nero-argos-split.gschema.xml",
    ] {
        assert!(ext.join(f).is_file(), "{f}");
    }
    assert!(
        c.iter().any(|l| l.starts_with("glib-compile-schemas ")
            && l.ends_with("nero-argos-split@LudaLunizz/schemas")),
        "{c:#?}"
    );
    assert!(
        c.iter()
            .any(|l| l.contains(" set org.gnome.shell.extensions.nero-argos-split monitor DP-1")),
        "{c:#?}"
    );
    // enabling a brand new extension fails until the next login: say so
    assert!(
        stdout(&o).contains(
            "log out and back in once, then: gnome-extensions enable nero-argos-split@LudaLunizz"
        ),
        "{}",
        stdout(&o)
    );

    // undo puts the newest backup back
    let o = nero(&home, Some(&bin), &["taskbar", "undo"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let c = calls(&home);
    assert!(c.contains(&"dconf reset -f /org/gnome/shell/extensions/dash-to-panel/".to_string()));
    assert!(c.contains(&"dconf load /org/gnome/shell/extensions/dash-to-panel/".to_string()));
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn taskbar_without_argos_and_without_dash_to_panel() {
    let home = tmp("noargos");
    let bin = fake_tools(&home, false, true);
    let o = nero(&home, Some(&bin), &["taskbar", "setup"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("Argos is not installed, skipped the sysmon script"));
    assert!(!home.join(".config/argos/sysmon.1s.sh").exists());
    assert!(
        !home
            .join("data/gnome-shell/extensions/nero-argos-split@LudaLunizz")
            .exists()
    );
    let _ = fs::remove_dir_all(&home);

    let home = tmp("nodtp");
    let bin = fake_tools(&home, true, false);
    let o = nero(&home, Some(&bin), &["taskbar", "setup"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("Dash to Panel is not installed"));
    // nothing was changed or backed up
    assert!(
        !calls(&home)
            .iter()
            .any(|l| l.starts_with("dconf") || l.starts_with("gsettings"))
    );
    // undo with nothing saved says so
    let o = nero(&home, Some(&bin), &["taskbar", "undo"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("nothing to undo"));
    let _ = fs::remove_dir_all(&home);
}
