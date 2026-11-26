// The JS side names core functions by string: call("fitToFreeSpace", ...). A typo
// there only fails at runtime in GNOME Shell, and a function nobody calls any more
// stays in the dispatch for ever. This test reads both sides and compares them:
//   - core side: the arms of `fn dispatch` in src/abi.rs
//   - JS side: every call("name", ...) in the extensions, the loader and the
//     added lines of the Tiling Shell patch, plus the loader's own run(e, "version", ...)
// Both sets must be found (a scanner that finds nothing would pass anything).
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// The names of the `"name" =>` arms of `fn dispatch`.
fn dispatch_names(abi_rs: &str) -> BTreeSet<String> {
    let start = abi_rs.find("fn dispatch(").expect("fn dispatch in abi.rs");
    let end = start
        + abi_rs[start..]
            .find("unknown function")
            .expect("the unknown-function arm");
    let mut names = BTreeSet::new();
    for line in abi_rs[start..end].lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix('"')
            && let Some((name, after)) = rest.split_once('"')
            && after.trim_start().starts_with("=>")
        {
            names.insert(name.to_string());
        }
    }
    names
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// The string literal at the start of `s` (after whitespace), if there is one.
fn literal(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let quote = s.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &s[1..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

/// Names passed as the first argument of `call(` (and of the loader's `run(e, `),
/// plus the number of `call(` sites whose first argument is not a literal.
fn js_names(js: &str) -> (BTreeSet<String>, usize) {
    let mut names = BTreeSet::new();
    let mut not_literal = 0;
    for (word, skip_arg) in [("call(", false), ("run(", true)] {
        let mut from = 0;
        while let Some(i) = js[from..].find(word) {
            let at = from + i;
            from = at + word.len();
            let before = js[..at].chars().next_back();
            if before.is_some_and(|c| is_ident(c) || c == '.') {
                continue; // x.call(...), recall(...)
            }
            if js[..at].trim_end().ends_with("function") {
                continue; // the definition, not a call site
            }
            let mut args = &js[from..];
            if skip_arg {
                // run(e, "name", ...): drop the first argument
                match args.split_once(',') {
                    Some((_, rest)) => args = rest,
                    None => continue,
                }
            }
            match literal(args) {
                Some(name) if name.chars().all(is_ident) => {
                    names.insert(name.to_string());
                }
                _ if skip_arg => {}
                _ => not_literal += 1,
            }
        }
    }
    (names, not_literal)
}

/// The added lines of a patch, without the leading '+'.
fn added_lines(patch: &str) -> String {
    patch
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .map(|l| &l[1..])
        .collect::<Vec<_>>()
        .join("\n")
}

/// The JS files that call the core. The per-extension copies of nero.js and
/// helpers.js are build outputs: core/js has the sources.
fn js_sources() -> Vec<(String, String)> {
    let root = root();
    let mut out = vec![];
    let mut read = |path: PathBuf, label: String, patch: bool| {
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        out.push((label, if patch { added_lines(&text) } else { text }));
    };
    for ext in ["keep-onscreen", "nero-layout", "nero-workspaces"] {
        for dir in fs::read_dir(root.join(ext)).unwrap().flatten() {
            if !dir.path().is_dir() {
                continue;
            }
            for f in fs::read_dir(dir.path()).unwrap().flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                if name.ends_with(".js") && name != "nero.js" && name != "helpers.js" {
                    read(f.path(), format!("{ext}/{name}"), false);
                }
            }
        }
    }
    read(
        root.join("core/js/nero.js"),
        "core/js/nero.js".into(),
        false,
    );
    read(
        root.join("tilingshell-patch/tilingshell-18.0.patch"),
        "tilingshell-patch (added lines)".into(),
        true,
    );
    out
}

#[test]
fn scanner_reads_the_forms_the_js_uses() {
    let (n, bad) = js_names(
        "call('a', 1); const x = call(\"b\"); call(\n  'c',\n  2); other.call(this, 1); recall('z'); \
         export function call(fn, ...args) {} run(e, \"v\", []); call(name, 1);",
    );
    assert_eq!(n.into_iter().collect::<Vec<_>>(), ["a", "b", "c", "v"]);
    assert_eq!(
        bad, 1,
        "call(name, 1) is a call site without a literal name"
    );
    let abi = "fn dispatch(f: &str) {\n    match f {\n        // c\n        \"one\" => ok(1),\n        \"two\" => ok(\n            2),\n        _ => Err(format!(\"unknown function\")),";
    assert_eq!(
        dispatch_names(abi).into_iter().collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert_eq!(
        added_lines("--- a\n+++ b\n context\n-gone\n+call('p')\n"),
        "call('p')"
    );
}

#[test]
fn js_calls_and_dispatch_arms_agree() {
    let dispatch = dispatch_names(include_str!("../src/abi.rs"));
    assert!(
        dispatch.len() > 20,
        "found only {} dispatch arms: the scanner is blind",
        dispatch.len()
    );

    let mut called = BTreeSet::new(); // name -> first file, for the message
    let mut where_ = std::collections::BTreeMap::new();
    let mut dynamic = vec![];
    for (label, text) in js_sources() {
        let (names, not_literal) = js_names(&text);
        if not_literal > 0 {
            dynamic.push(format!("{label}: {not_literal}"));
        }
        for n in names {
            where_.entry(n.clone()).or_insert_with(|| label.clone());
            called.insert(n);
        }
    }
    assert!(
        called.len() > 20,
        "found only {} JS call names: the scanner is blind",
        called.len()
    );

    let unknown: Vec<_> = called
        .difference(&dispatch)
        .map(|n| format!("{n} (called in {})", where_[n]))
        .collect();
    assert!(
        unknown.is_empty(),
        "JS calls core functions that abi.rs does not have: {unknown:?}"
    );
    let unused: Vec<_> = dispatch.difference(&called).collect();
    assert!(
        unused.is_empty(),
        "abi.rs has arms no JS calls (dead, or called by a name the scan cannot see): {unused:?}"
    );
    assert!(
        dynamic.is_empty(),
        "call sites whose function name is not a string literal (the check cannot see them): {dynamic:?}"
    );
}
