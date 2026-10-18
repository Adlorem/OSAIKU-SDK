#![cfg(feature = "compiler")]

use osaiku_ui::{
    compiler::compile_unstyled as compile,
    layout::Rect,
    paint::{Painter, Scene},
    Bindings, Document, Point, Value,
};

#[derive(Default)]
struct Capture {
    labels: Vec<(Rect, String, Rect)>,
    lines: Vec<((i32, i32), (i32, i32), u32, Rect)>,
    dots: Vec<(Rect, Rect)>,
}
impl Painter for Capture {
    fn fill_alpha(&mut self, rect: Rect, _: u32, _: u8, clip: Rect) {
        self.dots.push((rect, clip));
    }
    fn text(&mut self, rect: Rect, text: &str, _: u16, _: u32, clip: Rect) {
        self.labels.push((rect, text.into(), clip));
    }
    fn line(&mut self, from: (i32, i32), to: (i32, i32), color: u32, clip: Rect) {
        self.lines.push((from, to, color, clip));
    }
}
fn document(points: &[Point]) -> Document {
    struct Series<'a>(&'a [Point]);
    impl Bindings for Series<'_> {
        fn get(&self, _: u32) -> Option<Value<'_>> {
            Some(Value::Series(self.0))
        }
    }
    let template = compile(
        r#"<main id="root"><ui-chart id="chart" bind:series="history"></ui-chart></main>"#,
        "ui-chart { height: 120px; color: #123456; }",
    )
    .unwrap();
    let mut bound = Document::EMPTY;
    template.bind_into(&Series(points), &mut bound).unwrap();
    bound
}
fn paint(points: &[Point], width: u32, height: u32) -> Capture {
    let document = document(points);
    let mut capture = Capture::default();
    Scene::from_document(&document, width, height)
        .unwrap()
        .paint(&mut capture);
    capture
}
fn axis(capture: &Capture) -> Vec<&str> {
    capture
        .labels
        .iter()
        .filter_map(|(_, text, _)| text.ends_with(" s").then_some(text.as_str()))
        .collect()
}

#[test]
fn seconds_axis_uses_actual_timestamps_and_keeps_unavailable_intervals() {
    let points = [
        Point {
            time_ms: 100,
            value: 0,
        },
        Point {
            time_ms: 1100,
            value: 2500,
        },
        Point {
            time_ms: 1900,
            value: 10001,
        },
        Point {
            time_ms: 6101,
            value: 10000,
        },
    ];
    let capture = paint(&points, 700, 120);
    assert_eq!(axis(&capture), ["0 s", "-6.001 s", "-3 s"]);
    let segments: Vec<_> = capture.lines.iter().filter(|l| l.2 == 0x123456).collect();
    // The unavailable sample breaks the line. Zero remains a real point;
    // unequal time intervals retain their true horizontal proportions.
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].0, (36, 103));
    assert_eq!(segments[0].1, (146, 79));
    assert_eq!(capture.dots.len(), 3);
    assert_eq!(capture.dots[2].0.x + 1, 699);
    for (_, clip) in &capture.dots {
        assert_eq!(
            *clip,
            Rect {
                x: 36,
                y: 6,
                width: 664,
                height: 98
            }
        );
    }
    let doc = document(&points);
    let scene = Scene::from_document(&doc, 700, 120).unwrap();
    assert_eq!(scene.semantics().last().unwrap().series, points);
}

#[test]
fn empty_first_and_unavailable_samples_do_not_invent_a_time_window() {
    assert!(axis(&paint(&[], 700, 120)).is_empty());
    for value in [0, 10001] {
        let capture = paint(
            &[Point {
                time_ms: u64::MAX,
                value,
            }],
            700,
            120,
        );
        assert_eq!(axis(&capture), ["0 s"]);
        assert_eq!(capture.dots.len(), usize::from(value == 0));
    }
    let missing = paint(
        &[
            Point {
                time_ms: 0,
                value: 10001,
            },
            Point {
                time_ms: 500,
                value: 10001,
            },
        ],
        700,
        120,
    );
    assert_eq!(axis(&missing), ["0 s", "-0.5 s", "-0.25 s"]);
    assert!(missing.dots.is_empty());
}

#[test]
fn time_labels_remain_exact_at_subseconds_and_u64_limits() {
    for (duration, expected) in [
        (1, "-0.001 s"),
        (10, "-0.01 s"),
        (100, "-0.1 s"),
        (101, "-0.101 s"),
        (110, "-0.11 s"),
        (1000, "-1 s"),
        (u64::MAX, "-18446744073709551.615 s"),
    ] {
        let points = [
            Point {
                time_ms: u64::MAX - duration,
                value: 0,
            },
            Point {
                time_ms: u64::MAX,
                value: 10000,
            },
        ];
        let capture = paint(&points, 4096, 120);
        assert_eq!(axis(&capture)[1], expected);
        let segment = capture.lines.iter().find(|l| l.2 == 0x123456).unwrap();
        assert_eq!((segment.0 .0, segment.1 .0), (36, 4095));
    }
}

#[test]
fn resizing_omits_overlapping_labels_and_scrolling_clips_the_axis() {
    let points = [
        Point {
            time_ms: 10,
            value: 0,
        },
        Point {
            time_ms: 60133,
            value: 10000,
        },
    ];
    for width in [48, 71, 72, 100, 200, 320, 640, 4096] {
        let capture = paint(&points, width, 120);
        let mut labels: Vec<_> = capture
            .labels
            .iter()
            .filter(|l| l.1.ends_with(" s"))
            .collect();
        labels.sort_by_key(|l| l.0.x);
        for label in &labels {
            assert_eq!(label.0, label.2);
            assert!(label.0.x >= 36 && label.0.right() <= width as i32);
            assert!(label.0.bottom() <= 120);
        }
        for pair in labels.windows(2) {
            assert!(pair[0].0.right() + 8 <= pair[1].0.x);
        }
        assert_eq!(labels.is_empty(), width < 72);
    }
    let capture = paint(&points, 640, 110);
    for (bounds, _, clip) in capture.labels.iter().filter(|l| l.1.ends_with(" s")) {
        assert_eq!(bounds.y, 108);
        assert_eq!(clip.height, 2);
        assert_eq!(clip.bottom(), 110);
    }
}
