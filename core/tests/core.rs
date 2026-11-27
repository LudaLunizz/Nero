// Rust-side tests of the placement logic, plus the JSON boundary. These were the
// pure-logic JS tests once; ../tests keeps only what needs GJS (the wrappers
// in the Tiling Shell patch, the loader, the Registry).
use nero_core::free_space::*;
use nero_core::line_groups::*;
use nero_core::resize::{
    Gaps, Linked, MinSize, Put, SettleWin, Side, clamp_edge, find_linked, is_adjacent,
    is_collinear, linked_min, linked_rect, linked_settle, reach,
};
use nero_core::{Point, Rect, abi, fit::fit_to_work_area};

const fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, w, h)
}
const fn row(y: f64, h: f64) -> Rect {
    r(0.0, y, 1440.0, h)
}
const W: Rect = r(0.0, 0.0, 1440.0, 2560.0);
const TOP: Rect = row(0.0, 640.0);
const MID: Rect = row(640.0, 1280.0);
const BOT: Rect = row(1920.0, 640.0);

#[test]
fn fit_and_vacancy() {
    assert!(is_tile_vacant(&MID, &[row(0.0, 1000.0)]));
    assert_eq!(
        fit_to_free_space(&MID, &[row(0.0, 1000.0)], &W, &[BOT]),
        row(1000.0, 920.0)
    );
    assert_eq!(
        fit_to_free_space(&MID, &[row(0.0, 400.0), BOT], &W, &[]),
        row(400.0, 1520.0)
    );
    assert_eq!(fit_to_free_space(&MID, &[row(0.0, 640.0)], &W, &[BOT]), MID);
    assert_eq!(
        fit_to_free_space(&row(1280.0, 1280.0), &[row(0.0, 1500.0)], &W, &[]),
        row(1500.0, 1060.0)
    );
    assert_eq!(
        fit_to_free_space(&TOP, &[row(0.0, 700.0)], &W, &[MID, BOT]),
        TOP
    );
    assert_eq!(fit_to_free_space(&MID, &[row(0.0, 2000.0)], &W, &[]), MID);
    assert_eq!(
        fit_to_free_space(&MID, &[row(0.0, 610.0)], &W, &[BOT]),
        row(610.0, 1310.0)
    );
    assert_eq!(
        fit_to_free_space(&TOP, &[row(2200.0, 360.0)], &W, &[MID]),
        TOP
    );
    assert_eq!(
        fit_to_free_space(&BOT, &[TOP, row(640.0, 1920.0)], &W, &[]),
        BOT
    );
    assert_eq!(
        touching_rects(
            &row(610.0, 1310.0),
            &[
                row(0.0, 610.0),
                row(1925.0, 635.0),
                r(0.0, 0.0, 100.0, 100.0)
            ],
            10.0
        ),
        vec![0, 1]
    );
}

#[test]
fn grow_into_free_space_cases() {
    let a = r(0.0, 0.0, 2560.0, 1440.0);
    let g = |rect, others: &[Rect]| grow_into_free_space(&rect, others, &a, 10.0);
    assert_eq!(g(r(800.0, 300.0, 600.0, 400.0), &[]), a);
    assert_eq!(
        g(
            r(600.0, 0.0, 1000.0, 1440.0),
            &[r(1600.0, 0.0, 960.0, 1440.0)]
        ),
        r(0.0, 0.0, 1600.0, 1440.0)
    );
    assert_eq!(
        g(
            r(600.0, 0.0, 1000.0, 1440.0),
            &[r(600.0, 0.0, 1000.0, 1440.0), r(1600.0, 0.0, 960.0, 1440.0)]
        ),
        r(0.0, 0.0, 1600.0, 1440.0)
    );
    assert_eq!(
        g(r(900.0, 0.0, 700.0, 1440.0), &[r(0.0, 0.0, 400.0, 1440.0)]),
        r(400.0, 0.0, 2160.0, 1440.0)
    );
    assert_eq!(
        g(r(0.0, 400.0, 1000.0, 400.0), &[r(0.0, 0.0, 2560.0, 300.0)]),
        r(0.0, 300.0, 2560.0, 1140.0)
    );
    assert_eq!(
        g(
            r(0.0, 600.0, 1000.0, 400.0),
            &[
                r(1200.0, 0.0, 1360.0, 500.0),
                r(1200.0, 500.0, 1360.0, 940.0)
            ]
        ),
        r(0.0, 0.0, 1200.0, 1440.0)
    );
}

#[test]
fn fluid() {
    let area = r(0.0, 0.0, 1000.0, 600.0);
    let tiles = [
        r(0.0, 0.0, 500.0, 600.0),
        r(500.0, 0.0, 500.0, 300.0),
        r(500.0, 300.0, 500.0, 300.0),
    ];
    let p = |x, y| Point { x, y };
    assert_eq!(
        fluid_shape(&tiles, &[], &area, p(700.0, 100.0), 0.33),
        Some(tiles[1])
    );
    assert_eq!(
        fluid_shape(&tiles, &[tiles[0]], &area, p(100.0, 100.0), 0.33),
        None
    );
    let shrunk = [r(0.0, 0.0, 400.0, 600.0)];
    assert_eq!(
        fluid_shape(&tiles, &shrunk, &area, p(700.0, 100.0), 0.33),
        Some(r(400.0, 0.0, 600.0, 300.0))
    );
    assert_eq!(
        fluid_shape(&tiles, &shrunk, &area, p(450.0, 100.0), 0.33),
        Some(r(400.0, 0.0, 600.0, 300.0))
    );
    let busy = [r(0.0, 0.0, 520.0, 600.0), r(500.0, 0.0, 500.0, 260.0)];
    assert_eq!(
        fluid_shape(&tiles, &busy, &area, p(800.0, 400.0), 0.33),
        Some(r(520.0, 260.0, 480.0, 340.0))
    );
    assert_eq!(
        fluid_shape(
            &tiles,
            &[r(0.0, 0.0, 990.0, 600.0)],
            &area,
            p(995.0, 300.0),
            0.33
        ),
        None
    );
}

#[test]
fn adjacency() {
    let (left, right) = (r(0.0, 1631.0, 360.0, 929.0), r(317.0, 1681.0, 806.0, 879.0));
    assert!(is_adjacent(&left, &right, Side::Right, 10.0));
    assert!(is_adjacent(&right, &left, Side::Left, 10.0));
    assert!(is_adjacent(
        &r(0.0, 0.0, 300.0, 500.0),
        &r(305.0, 0.0, 300.0, 500.0),
        Side::Right,
        10.0
    ));
    assert!(!is_adjacent(
        &r(0.0, 0.0, 300.0, 500.0),
        &r(320.0, 0.0, 300.0, 500.0),
        Side::Right,
        10.0
    ));
    assert!(!is_adjacent(
        &r(0.0, 0.0, 300.0, 500.0),
        &r(100.0, 0.0, 300.0, 500.0),
        Side::Right,
        10.0
    ));
    assert!(!is_adjacent(
        &r(0.0, 0.0, 500.0, 300.0),
        &r(0.0, 0.0, 500.0, 300.0),
        Side::Bottom,
        10.0
    ));
    let (l, m, rt) = (
        r(0.0, 215.0, 465.0, 600.0),
        r(465.0, 215.0, 1195.0, 600.0),
        r(1660.0, 215.0, 600.0, 600.0),
    );
    assert!(is_collinear(&m, &l, Side::Top, 10.0) && is_collinear(&m, &rt, Side::Top, 10.0));
    assert!(!is_collinear(
        &m,
        &r(0.0, 400.0, 465.0, 400.0),
        Side::Top,
        10.0
    ));
    assert!(!is_collinear(
        &m,
        &r(2400.0, 215.0, 300.0, 600.0),
        Side::Top,
        10.0
    ));
}

fn l1() -> Vec<Tile> {
    let t = |x, y, width, height, groups: &[f64]| Tile {
        x,
        y,
        width,
        height,
        groups: groups.to_vec(),
    };
    vec![
        t(0.0, 0.0, 0.22, 0.5, &[1.0]),
        t(0.0, 0.5, 0.22, 0.5, &[3.0]),
        t(0.22, 0.0, 0.56, 0.5, &[4.0, 1.0]),
        t(0.22, 0.5, 0.56, 0.5, &[5.0, 3.0]),
        t(0.78, 0.0, 0.22, 0.5, &[4.0]),
        t(0.78, 0.5, 0.22, 0.5, &[5.0]),
    ]
}

#[test]
fn line_groups() {
    let mut t = l1();
    let h = find_chains(&t).into_iter().find(|c| !c.vertical).unwrap();
    assert_eq!(h.segments.len(), 3);
    assert!(!is_attached(&t, &h));
    let p = pick_chain(&t, 0.1, 0.51, 2560.0, 1440.0, 60.0).unwrap();
    assert!(!p.vertical);
    assert_eq!(toggle_chain(&mut t, &p), Toggle::Attached);
    let h = find_chains(&t).into_iter().find(|c| !c.vertical).unwrap();
    assert!(is_attached(&t, &h));
    let g = *t[0]
        .groups
        .iter()
        .find(|g| t[1].groups.contains(g))
        .unwrap();
    let members: Vec<Tile> = t
        .iter()
        .filter(|u| u.groups.contains(&g))
        .cloned()
        .collect();
    assert_eq!(members.len(), 6);
    let gl = group_line(&members).unwrap();
    assert!(!gl.vertical && (gl.c - 0.5).abs() < 0.001);
    assert_eq!(toggle_chain(&mut t, &h), Toggle::Separated);
    assert!(pick_chain(&l1(), 0.11, 0.25, 2560.0, 1440.0, 40.0).is_none());
}

#[test]
fn fit_keep_onscreen() {
    let wa = r(0.0, 32.0, 1024.0, 568.0);
    assert_eq!(
        fit_to_work_area(&r(500.0, 300.0, 800.0, 900.0), &wa),
        r(224.0, 32.0, 800.0, 568.0)
    );
    assert_eq!(
        fit_to_work_area(&r(10.0, 40.0, 100.0, 100.0), &wa),
        r(10.0, 40.0, 100.0, 100.0)
    );
}

#[test]
fn json_boundary() {
    // keys keep the JS order, so JSON.stringify of a result matches the old JS
    assert_eq!(abi::call(br#"{"f":"fluidShape","a":[[{"x":0,"y":0,"width":500,"height":600}],[{"x":0,"y":0,"width":500,"height":600}],{"x":0,"y":0,"width":1000,"height":600},{"x":100,"y":100},0.33]}"#), r#"{"ok":null}"#);
    assert_eq!(
        abi::call(br#"{"f":"koFit","a":[{"windowType":"NORMAL","monitor":0,"frame":{"x":-5,"y":0,"width":10,"height":10},"workArea":{"x":0,"y":0,"width":100,"height":100},"maximized":false,"fullscreen":false}]}"#),
        r#"{"ok":{"x":0.0,"y":0.0,"width":10.0,"height":10.0}}"#
    );
    // defaults: touchingRects without tol uses 10
    assert_eq!(abi::call(br#"{"f":"touchingRects","a":[{"x":0,"y":0,"width":10,"height":10},[{"x":15,"y":0,"width":10,"height":10}]]}"#), r#"{"ok":[0]}"#);
    assert!(abi::call(br#"{"f":"nope","a":[]}"#).starts_with(r#"{"err":"#));
    assert_eq!(
        abi::call(br#"{"f":"version","a":[]}"#),
        format!(r#"{{"ok":"{}"}}"#, env!("CARGO_PKG_VERSION"))
    );
    assert!(abi::call(br#"{"f":"isTileVacant","a":[1]}"#).starts_with(r#"{"err":"argument 0"#));
    assert!(abi::call(b"not json").starts_with(r#"{"err":"bad request"#));
    // a chain pointing at a tile that does not exist is an error, not a panic
    let bad = br#"{"f":"toggleChain","a":[[],{"vertical":true,"c":0.5,"from":0,"to":1,"segments":[{"prev":[3],"next":[],"from":0,"to":1}]}]}"#;
    assert!(abi::call(bad).starts_with(r#"{"err":"#));
}

#[test]
fn layout_through_the_json_boundary() {
    // the same JSON shapes nero-layout/extension.js sends
    let win = r#"{"app":"a.desktop","normal":true,"skipTaskbar":false,"transient":false,"title":"Two","monitor":0,"connector":"DP-1","frame":{"x":1380,"y":1200,"width":1280,"height":696},"workArea":{"x":1080,"y":1120,"width":2560,"height":1392},"onAll":false,"workspace":0,"maximized":false,"fullscreen":false,"minimized":false,"above":false}"#;
    let ok = |req: String| -> serde_json::Value {
        let out: serde_json::Value = serde_json::from_str(&abi::call(req.as_bytes())).unwrap();
        out.get("ok").unwrap_or_else(|| panic!("{out}")).clone()
    };
    let save = ok(format!(
        r#"{{"f":"lySave","a":[{{}},[{win}],{{"skip":[],"onlyOnPrimary":false,"primary":0}},false,"now"]}}"#
    ));
    let file = save["write"].as_str().unwrap().to_string();
    assert!(
        file.contains(r#""spot": {"#) && file.contains(r#""fx": 0.1172"#),
        "{file}"
    );
    let start = ok(format!(
        r#"{{"f":"lyStart","a":[{}]}}"#,
        serde_json::to_string(&file).unwrap()
    ));
    assert_eq!(start["hasSaved"], true);
    let restore = ok(format!(
        r#"{{"f":"lyRestoreStart","a":[{},{},[],false]}}"#,
        start["model"],
        serde_json::to_string(&file).unwrap()
    ));
    assert_eq!(restore["run"], 1);
    // ClaimWin is the snapshot fields plus id and live, flat
    let w: serde_json::Value = serde_json::from_str(win).unwrap();
    let mut claim_win = w.as_object().unwrap().clone();
    claim_win.insert("id".into(), 4294967297u64.into());
    claim_win.insert("live".into(), true.into());
    let claimed = ok(format!(
        r#"{{"f":"lyClaim","a":[{},{}]}}"#,
        restore["model"],
        serde_json::Value::Object(claim_win)
    ));
    assert_eq!(claimed["run"], 1);
    assert!(
        abi::call(br#"{"f":"lyClaim","a":[{},{"id":1}]}"#)
            .starts_with(r#"{"err":"argument 1 (window)"#)
    );
}

// The DP-1 case: two Brave bands side by side, the right one already at its
// 500 px minimum. Ctrl-dragging the left one's right edge 100 px further must
// not push the right one's far (right) edge into the next column.
#[test]
fn linked_resize_keeps_the_far_edge() {
    let right = r(1497.0, 0.0, 500.0, 1440.0);
    let grow = r(0.0, 0.0, 100.0, 0.0);
    let (rs, ls) = (Some(Side::Right), Some(Side::Left));
    // no minimum known: plain shrink, far edge in place
    assert_eq!(
        linked_rect(&right, &grow, ls, None, rs, None, MinSize::default()),
        r(1597.0, 0.0, 400.0, 1440.0)
    );
    // Mutter kept 500 px at x 1597: far edge pushed to 2097 -> learn 500
    let min = linked_min(
        &right,
        &r(1597.0, 0.0, 500.0, 1440.0),
        ls,
        None,
        MinSize::default(),
    );
    assert_eq!(
        min,
        MinSize {
            width: 500.0,
            height: 0.0
        }
    );
    // a normal shrink teaches nothing
    assert_eq!(
        linked_min(
            &right,
            &r(1597.0, 0.0, 400.0, 1440.0),
            ls,
            None,
            MinSize::default()
        ),
        MinSize::default()
    );
    // with the minimum: stays at 1497..1997
    assert_eq!(
        linked_rect(&right, &grow, ls, None, rs, None, min),
        r(1497.0, 0.0, 500.0, 1440.0)
    );
    // shrinking the dragged window still grows the neighbour, far edge fixed
    let shrink = r(0.0, 0.0, -50.0, 0.0);
    assert_eq!(
        linked_rect(&right, &shrink, ls, None, rs, None, min),
        r(1447.0, 0.0, 550.0, 1440.0)
    );
    // neighbour on the left of a left-edge drag: its x (far edge) stays
    let left = r(0.0, 0.0, 563.0, 1440.0);
    let pull = r(100.0, 0.0, -100.0, 0.0);
    assert_eq!(
        linked_rect(&left, &pull, rs, None, ls, None, MinSize::default()),
        r(0.0, 0.0, 663.0, 1440.0)
    );
    // vertical: neighbour below, top edge moves, bottom stays
    let below = r(0.0, 700.0, 800.0, 300.0);
    let down = r(0.0, 0.0, 0.0, 200.0);
    let m = MinSize {
        width: 0.0,
        height: 250.0,
    };
    assert_eq!(
        linked_rect(
            &below,
            &down,
            None,
            Some(Side::Top),
            None,
            Some(Side::Bottom),
            m
        ),
        r(0.0, 750.0, 800.0, 250.0)
    );
    // collinear window (same side as the drag): moves the same edge
    let beside = r(0.0, 1500.0, 1497.0, 500.0);
    assert_eq!(
        linked_rect(&beside, &grow, rs, None, rs, None, MinSize::default()),
        r(0.0, 1500.0, 1597.0, 500.0)
    );
}

#[test]
fn clamp_edge_stops_at_a_stuck_neighbour() {
    let start = r(563.0, 0.0, 934.0, 1440.0);
    let now = r(563.0, 0.0, 1034.0, 1440.0); // grew 100 px
    let p_start = r(1497.0, 0.0, 500.0, 1440.0);
    let stuck = (p_start, p_start); // could not shrink
    assert_eq!(clamp_edge(&start, &now, Side::Right, &[stuck], 10.0), start);
    // neighbour gave 60 of the 100 px
    let part = (p_start, r(1557.0, 0.0, 440.0, 1440.0));
    assert_eq!(
        clamp_edge(&start, &now, Side::Right, &[part], 10.0),
        r(563.0, 0.0, 994.0, 1440.0)
    );
    // neighbour followed fully: nothing changes
    let full = (p_start, r(1597.0, 0.0, 400.0, 1440.0));
    assert_eq!(clamp_edge(&start, &now, Side::Right, &[full], 10.0), now);
    // a window that did not touch the edge at grab start is ignored
    let far = (r(1600.0, 0.0, 300.0, 1440.0), r(1600.0, 0.0, 300.0, 1440.0));
    assert_eq!(clamp_edge(&start, &now, Side::Right, &[far], 10.0), now);
    // overlap that was there at grab start is kept, not cut away
    let lap_start = r(1477.0, 0.0, 520.0, 1440.0);
    assert_eq!(
        clamp_edge(&start, &now, Side::Right, &[(lap_start, lap_start)], 10.0),
        start
    );
    // left edge, dragged left into a stuck neighbour
    let s2 = r(1497.0, 0.0, 500.0, 1440.0);
    let n2 = r(1397.0, 0.0, 600.0, 1440.0);
    let l = r(563.0, 0.0, 934.0, 1440.0);
    assert_eq!(clamp_edge(&s2, &n2, Side::Left, &[(l, l)], 10.0), s2);
}

#[test]
fn linked_resize_through_the_json_boundary() {
    let out = abi::call(br#"{"f":"linkedRect","a":[{"x":1497,"y":0,"width":500,"height":1440},{"x":0,"y":0,"width":100,"height":0},"left",null,"right",null,{"width":500,"height":0}]}"#);
    assert_eq!(
        out,
        r#"{"ok":{"x":1497.0,"y":0.0,"width":500.0,"height":1440.0}}"#
    );
}

#[test]
fn grow_into_free_space_overlap_still_blocks() {
    let a = r(0.0, 0.0, 2560.0, 1440.0);
    // 5 px overlap with the right neighbour still blocks
    assert_eq!(
        grow_into_free_space(
            &r(600.0, 0.0, 1005.0, 1440.0),
            &[r(1600.0, 0.0, 960.0, 1440.0)],
            &a,
            10.0
        ),
        r(0.0, 0.0, 1600.0, 1440.0)
    );
}

#[test]
fn fluid_more_cases() {
    let area = r(0.0, 0.0, 1000.0, 600.0);
    let tiles = [
        r(0.0, 0.0, 500.0, 600.0),
        r(500.0, 0.0, 500.0, 300.0),
        r(500.0, 300.0, 500.0, 300.0),
    ];
    let p = |x, y| Point { x, y };
    let f = |obstacles: &[Rect], pt| fluid_shape(&tiles, obstacles, &area, pt, 0.33);
    assert_eq!(f(&[], p(100.0, 100.0)), Some(tiles[0]));
    // left window shrunk to 400: the bottom free tile stretches too
    let shrunk = [r(0.0, 0.0, 400.0, 600.0)];
    assert_eq!(
        f(&shrunk, p(700.0, 500.0)),
        Some(r(400.0, 300.0, 600.0, 300.0))
    );
    // L-shaped space: a window in the top-right tile only
    let top = [r(500.0, 0.0, 500.0, 300.0)];
    assert_eq!(f(&top, p(100.0, 100.0)), Some(tiles[0]));
    assert_eq!(f(&top, p(700.0, 500.0)), Some(tiles[2]));
}

#[test]
fn adjacency_more_cases() {
    let (a, b) = (r(0.0, 0.0, 300.0, 500.0), r(300.0, 600.0, 300.0, 500.0));
    assert!(!is_adjacent(&a, &b, Side::Right, 10.0), "no shared length");
    let (up, down) = (r(0.0, 0.0, 500.0, 300.0), r(0.0, 290.0, 500.0, 300.0));
    assert!(is_adjacent(&up, &down, Side::Bottom, 10.0));
    assert!(is_adjacent(&down, &up, Side::Top, 10.0));
    assert!(!is_collinear(
        &r(465.0, 215.0, 1195.0, 600.0),
        &r(0.0, 1000.0, 465.0, 300.0),
        Side::Top,
        10.0
    ));
}

#[test]
fn line_groups_attach_and_separate() {
    // every horizontal chain is three pieces, none attached, each piece joins its
    // two sides through exactly one shared group
    let check_separate = |t: &[Tile]| {
        for c in find_chains(t).iter().filter(|c| !c.vertical) {
            assert_eq!(c.segments.len(), 3);
            assert!(!is_attached(t, c));
            for s in &c.segments {
                let shared = t[s.prev[0]]
                    .groups
                    .iter()
                    .filter(|g| t[s.next[0]].groups.contains(g))
                    .count();
                assert_eq!(shared, 1);
            }
        }
    };
    let mut t = l1();
    let h = pick_chain(&t, 0.1, 0.51, 2560.0, 1440.0, 60.0).unwrap();
    assert_eq!(toggle_chain(&mut t, &h), Toggle::Attached);
    let h = find_chains(&t).into_iter().find(|c| !c.vertical).unwrap();
    assert_eq!(toggle_chain(&mut t, &h), Toggle::Separated);
    check_separate(&t);

    // vertical line x = .22: attach, then separate
    let mut t = l1();
    let v = pick_chain(&t, 0.221, 0.25, 2560.0, 1440.0, 60.0).unwrap();
    assert!(v.vertical);
    let at22 = |t: &[Tile]| {
        find_chains(t)
            .into_iter()
            .find(|c| c.vertical && (c.c - 0.22).abs() < 0.01)
            .unwrap()
    };
    assert_eq!(toggle_chain(&mut t, &v), Toggle::Attached);
    assert!(is_attached(&t, &at22(&t)));
    let c = at22(&t);
    assert_eq!(toggle_chain(&mut t, &c), Toggle::Separated);
    // tile count and geometry untouched
    assert_eq!(t.len(), 6);
    assert_eq!(t[2].x, 0.22);
}

const GAPS: Gaps = Gaps {
    top: 8.0,
    bottom: 8.0,
    left: 8.0,
    right: 8.0,
};

#[test]
fn reach_of_the_gaps() {
    assert_eq!(reach(&GAPS, Side::Left), 8.0 + 8.0 * 4.0);
    // a gap of 0 counts as 2 (also the right gap in the error margin)
    assert_eq!(reach(&Gaps::default(), Side::Top), 2.0 + 2.0 * 4.0);
    let g = Gaps {
        top: 0.0,
        bottom: 4.0,
        left: 6.0,
        right: 10.0,
    };
    assert_eq!(reach(&g, Side::Top), 2.0 + 40.0);
    assert_eq!(reach(&g, Side::Bottom), 4.0 + 40.0);
}

#[test]
fn linked_windows_follow_the_line() {
    // three columns; the middle one's right edge is dragged: the right one
    // follows with its left edge, the left one is not touched
    let cols = [
        r(0.0, 0.0, 400.0, 1000.0),
        r(400.0, 0.0, 400.0, 1000.0),
        r(800.0, 0.0, 400.0, 1000.0),
    ];
    let drag = cols[1];
    let found = find_linked(&drag, None, Some(Side::Right), &[cols[0], cols[2]], &GAPS);
    assert_eq!(
        found,
        vec![Linked {
            i: 1,
            v: None,
            h: Some(Side::Left)
        }]
    );
    // two windows stacked on the right: both follow; the one below is found
    // through the line, not by touching the dragged window
    let stack = [r(800.0, 0.0, 400.0, 500.0), r(800.0, 500.0, 400.0, 500.0)];
    let found = find_linked(&drag, None, Some(Side::Right), &stack, &GAPS);
    assert_eq!(
        found.iter().map(|l| (l.i, l.h)).collect::<Vec<_>>(),
        vec![(0, Some(Side::Left)), (1, Some(Side::Left))]
    );
    // a corner drag links both directions; the window below sits on the line
    // of the right edge (collinear) and the one on the right on the bottom line,
    // so each is linked both ways
    let below = r(400.0, 1000.0, 400.0, 500.0);
    let right = r(800.0, 0.0, 400.0, 1000.0);
    let found = find_linked(
        &drag,
        Some(Side::Bottom),
        Some(Side::Right),
        &[below, right],
        &GAPS,
    );
    assert_eq!(
        found,
        vec![
            Linked {
                i: 0,
                v: Some(Side::Top),
                h: Some(Side::Right)
            },
            Linked {
                i: 1,
                v: Some(Side::Bottom),
                h: Some(Side::Left)
            },
        ]
    );
    // nothing near: nothing linked
    assert!(
        find_linked(
            &drag,
            None,
            Some(Side::Right),
            &[r(2000.0, 0.0, 100.0, 100.0)],
            &GAPS
        )
        .is_empty()
    );
}

#[test]
fn settle_puts_the_edge_back() {
    // the dragged window grew 100 px to the right, its neighbour (at 500 px)
    // could not shrink: it is pulled back to where the neighbour could reach
    let start = r(563.0, 0.0, 934.0, 1440.0);
    let p_start = r(1497.0, 0.0, 500.0, 1440.0);
    let neighbour = SettleWin {
        start: p_start,
        actual: p_start,
        h: Some(Side::Left),
        v: None,
        gone: false,
    };
    let put = linked_settle(
        &start,
        &r(563.0, 0.0, 1034.0, 1440.0),
        None,
        None,
        &[neighbour],
        &GAPS,
    );
    assert!(put.is_empty(), "no drag side, nothing to clamp");
    let put = linked_settle(
        &start,
        &r(563.0, 0.0, 1034.0, 1440.0),
        Some(Side::Right),
        None,
        &[neighbour],
        &GAPS,
    );
    assert_eq!(put, vec![Put { i: -1, rect: start }]);
    // the neighbour followed: nothing to do
    let followed = SettleWin {
        actual: r(1597.0, 0.0, 400.0, 1440.0),
        ..neighbour
    };
    assert!(
        linked_settle(
            &start,
            &r(563.0, 0.0, 1034.0, 1440.0),
            Some(Side::Right),
            None,
            &[followed],
            &GAPS
        )
        .is_empty()
    );
    // a window that is gone is left out
    let gone = SettleWin {
        gone: true,
        ..neighbour
    };
    assert!(
        linked_settle(
            &start,
            &r(563.0, 0.0, 1034.0, 1440.0),
            Some(Side::Right),
            None,
            &[gone],
            &GAPS
        )
        .is_empty()
    );
}

#[test]
fn linked_find_and_settle_through_the_json_boundary() {
    let out = abi::call(br#"{"f":"linkedFind","a":[{"x":400,"y":0,"width":400,"height":1000},null,"right",[{"x":800,"y":0,"width":400,"height":1000}],{"top":8,"bottom":8,"left":8,"right":8}]}"#);
    assert_eq!(out, r#"{"ok":[{"i":0,"v":null,"h":"left"}]}"#);
    let out = abi::call(br#"{"f":"fitToWorkArea","a":[{"x":500,"y":300,"width":800,"height":900},{"x":0,"y":32,"width":1024,"height":568}]}"#);
    assert_eq!(
        out,
        r#"{"ok":{"x":224.0,"y":32.0,"width":800.0,"height":568.0}}"#
    );
}
