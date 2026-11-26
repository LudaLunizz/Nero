// Rust-side tests: the same cases as the JS tests in ../tests (which run the
// WebAssembly build through the JS wrappers), plus the JSON boundary.
use nero_core::free_space::*;
use nero_core::line_groups::*;
use nero_core::resize::{Side, is_adjacent, is_collinear};
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
fn fill_and_resolve() {
    assert_eq!(
        fill_gaps(&[Some(TOP), Some(BOT)], &W),
        vec![Some(row(0.0, 1280.0)), Some(row(1280.0, 1280.0))]
    );
    assert_eq!(fill_gaps(&[Some(TOP)], &W), vec![Some(row(0.0, 2560.0))]);
    let l = r(0.0, 0.0, 2560.0, 1440.0);
    let got = fill_gaps(
        &[
            Some(r(0.0, 0.0, 563.0, 720.0)),
            Some(r(0.0, 720.0, 563.0, 720.0)),
            Some(r(1997.0, 0.0, 563.0, 720.0)),
            Some(r(1997.0, 720.0, 563.0, 720.0)),
        ],
        &l,
    );
    let want = [
        r(0.0, 0.0, 1280.0, 720.0),
        r(0.0, 720.0, 1280.0, 720.0),
        r(1280.0, 0.0, 1280.0, 720.0),
        r(1280.0, 720.0, 1280.0, 720.0),
    ];
    assert_eq!(got, want.map(Some).to_vec());
    assert_eq!(
        resolve_overlaps(&[row(0.0, 2560.0), MID], &[0.0, 1.0], &[TOP, MID, BOT]),
        vec![Some(TOP), Some(MID)]
    );
    assert_eq!(
        resolve_overlaps(&[row(0.0, 1920.0), MID, BOT], &[0.0, 2.0, 1.0], &[]),
        vec![Some(TOP), Some(MID), Some(BOT)]
    );
    assert_eq!(
        resolve_overlaps(&[row(700.0, 100.0), MID], &[0.0, 1.0], &[]),
        vec![None, Some(MID)]
    );
    assert_eq!(
        resolve_overlaps(&[row(640.0, 1480.0), MID], &[0.0, 1.0], &[TOP, MID, BOT]),
        vec![None, Some(MID)]
    );
    assert_eq!(
        free_tiles(&[TOP, MID, BOT], &[row(0.0, 1920.0), BOT]).len(),
        1
    );
}

#[test]
fn frames() {
    let t = [TOP, MID, BOT];
    let mv = |i, rect| Move { i, rect };
    assert_eq!(place_in_frames(&t, &[TOP], &TOP), vec![mv(0, MID)]);
    assert_eq!(place_in_frames(&t, &[TOP, MID], &MID), vec![mv(1, BOT)]);
    assert_eq!(place_in_frames(&t, &[TOP, MID, BOT], &MID), vec![]);
    assert_eq!(
        place_in_frames(&t, &[row(0.0, 1920.0), BOT], &TOP),
        vec![mv(0, MID)]
    );
    assert_eq!(
        place_in_frames(&t, &[MID, MID], &MID),
        vec![mv(0, TOP), mv(1, BOT)]
    );
    assert_eq!(refill_frames(&t, &[MID, MID, BOT]), vec![mv(1, TOP)]);
    assert_eq!(refill_frames(&t, &[MID]), vec![]);
    assert_eq!(
        place_in_frames(&t, &[row(0.0, 300.0)], &TOP),
        vec![mv(0, MID)]
    );
    assert_eq!(frames_of(&t, &row(0.0, 1920.0)), vec![0, 1]);
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
    assert_eq!(abi::call(br#"{"f":"cutAway","a":[{"x":0,"y":0,"width":10,"height":10},{"x":0,"y":0,"width":10,"height":10}]}"#), r#"{"ok":null}"#);
    assert_eq!(
        abi::call(br#"{"f":"fitToWorkArea","a":[{"x":-5,"y":0,"width":10,"height":10},{"x":0,"y":0,"width":100,"height":100}]}"#),
        r#"{"ok":{"x":0.0,"y":0.0,"width":10.0,"height":10.0}}"#
    );
    // defaults: touchingRects without tol uses 10
    assert_eq!(abi::call(br#"{"f":"touchingRects","a":[{"x":0,"y":0,"width":10,"height":10},[{"x":15,"y":0,"width":10,"height":10}]]}"#), r#"{"ok":[0]}"#);
    assert!(abi::call(br#"{"f":"nope","a":[]}"#).starts_with(r#"{"err":"#));
    assert_eq!(
        abi::call(br#"{"f":"version","a":[]}"#),
        format!(r#"{{"ok":"{}"}}"#, env!("CARGO_PKG_VERSION"))
    );
    assert!(abi::call(br#"{"f":"cutAway","a":[1]}"#).starts_with(r#"{"err":"argument 0"#));
    assert!(abi::call(b"not json").starts_with(r#"{"err":"bad request"#));
    // a chain pointing at a tile that does not exist is an error, not a panic
    let bad = br#"{"f":"toggleChain","a":[[],{"vertical":true,"c":0.5,"from":0,"to":1,"segments":[{"prev":[3],"next":[],"from":0,"to":1}]}]}"#;
    assert!(abi::call(bad).starts_with(r#"{"err":"#));
}

#[test]
fn layout_through_the_json_boundary() {
    assert_eq!(
        abi::call(br#"{"f":"toSpot","a":[{"x":1380,"y":1200,"width":1280,"height":696},{"x":1080,"y":1120,"width":2560,"height":1392}]}"#),
        r#"{"ok":{"fx":0.1172,"fy":0.0575,"fw":0.5,"fh":0.5}}"#
    );
    assert_eq!(
        abi::call(br#"{"f":"fromSpot","a":[{"fx":0.9,"fy":0,"fw":0.5,"fh":0.5},{"x":0,"y":0,"width":1000,"height":800}]}"#),
        r#"{"ok":{"x":500.0,"y":0.0,"width":500.0,"height":400.0}}"#
    );
    assert_eq!(
        abi::call(br#"{"f":"matchWindow","a":[[{"app":"a.desktop","title":"One"},{"app":"a.desktop","title":"Two"}],[false,false],"a.desktop","Two"]}"#),
        r#"{"ok":1}"#
    );
    assert_eq!(
        abi::call(br#"{"f":"matchWindow","a":[[{"app":"a.desktop","title":"One"}],[true],"a.desktop","One"]}"#),
        r#"{"ok":null}"#
    );
    assert!(
        abi::call(br#"{"f":"matchWindow","a":[[],[],5,""]}"#)
            .starts_with(r#"{"err":"argument 2 (app)"#)
    );
}
