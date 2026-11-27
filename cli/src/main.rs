//! The `nero` command. Everything it decides is in the core crate
//! (`core/src/sysmon.rs`, `core/src/taskbar.rs`, both tested without a desktop);
//! this crate reads files, runs programs and prints.
//!
//!   nero sysmon [refresh]                   the Argos system monitor (what Argos runs every second)
//!   nero taskbar setup [--monitor CONNECTOR]   Windows-like taskbar, system monitor, Argos buttons
//!   nero taskbar undo                       put back what the setup changed

mod sysmon;
mod taskbar;

const HELP: &str = "nero: Nero's command line tool

  nero sysmon [refresh]                      Argos panel text: Claude usage, network, RAM, CPU, GPU
  nero taskbar setup [--monitor CONNECTOR]   Dash to Panel at the bottom of every monitor, the
                                             system monitor in Argos, and (with --monitor, e.g.
                                             DP-1) the Argos buttons on that monitor's taskbar
  nero taskbar undo                          put back what setup changed (Dash to Panel settings,
                                             Ubuntu Dock, the Argos script, the split extension)
  nero --version

Needs Dash to Panel (and Argos for the system monitor) from extensions.gnome.org.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("sysmon") => sysmon::run(&args[1..]),
        Some("taskbar") => taskbar::run_args(&args[1..]),
        Some("--version" | "version") => {
            println!("nero {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help" | "-h" | "help") | None => {
            println!("{HELP}");
            0
        }
        Some(other) => {
            eprintln!("nero: unknown command {other}\n\n{HELP}");
            2
        }
    };
    std::process::exit(code);
}
