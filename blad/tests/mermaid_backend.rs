//! R-037–R-040: spatial text, explicit failures, and bounded native execution.
#[path = "../src/mermaid.rs"]
mod mermaid;

use mermaid::{DiagramControl, DiagramError, render, render_controlled};

const CORPUS: &[(&str, &[&str])] = &[
    (
        "flowchart LR\nA[Reader] -->|request| B[Service]\nB --> A",
        &["Reader", "Service", "request"],
    ),
    (
        "sequenceDiagram\nparticipant Reader\nparticipant Service\nReader->>Service: request\nService-->>Reader: response",
        &["Reader", "Service", "request", "response"],
    ),
    (
        "stateDiagram-v2\ndirection LR\nIdle --> Active: start\nActive --> Idle: stop",
        &["Idle", "Active", "start", "stop"],
    ),
    (
        "classDiagram\ndirection LR\nclass Reader {\n+String name\n+read()\n}\nReader --> Service : calls",
        &["Reader", "Service", "name", "read", "calls"],
    ),
    (
        "erDiagram\ndirection LR\nREADER ||--o{ BOOK : reads\nREADER {\nstring name PK\n}\nBOOK {\nstring title\n}",
        &["READER", "BOOK", "reads", "name", "title", "PK"],
    ),
];

#[test]
fn all_five_families_draw_boxes_connectors_and_preserve_labels() {
    for &(source, labels) in CORPUS {
        for ascii in [false, true] {
            let result = render(source, ascii, Some(80)).unwrap();
            for label in labels {
                assert!(result.contains(label), "missing {label:?} in {result}");
            }
            assert!(!result.contains('\x1b'));
            if ascii {
                assert!(result.is_ascii(), "{result}");
            }
            assert!(!result.contains("<svg"));
            assert!(result.lines().count() >= 3, "{result}");
            assert!(result.contains(if ascii { '+' } else { '┌' }), "{result}");
            assert!(result.contains(if ascii { '-' } else { '─' }), "{result}");
        }
    }
}

#[test]
fn width_never_truncates_or_reflows_completed_rows() {
    for &(source, _) in CORPUS {
        let natural = render(source, false, None).unwrap();
        for width in [0, 1, 40, 80, 120, usize::MAX] {
            assert_eq!(render(source, false, Some(width)).unwrap(), natural);
        }
    }
}

#[test]
fn authored_flowchart_directions_are_preserved() {
    for (direction, reverse) in [("LR", false), ("RL", true)] {
        let result = render(
            &format!("flowchart {direction}\nA[Alpha] -->|go| B[Beta]"),
            false,
            Some(1),
        )
        .unwrap();
        let a = result.lines().find(|line| line.contains("Alpha")).unwrap();
        let b = result.lines().find(|line| line.contains("Beta")).unwrap();
        assert_eq!(a, b, "{result}");
        assert_eq!(a.find("Alpha") > a.find("Beta"), reverse, "{result}");
    }
    for (direction, reverse) in [("TD", false), ("TB", false), ("BT", true)] {
        let result = render(
            &format!("flowchart {direction}\nA[Alpha] --> B[Beta]"),
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            result.find("Alpha") > result.find("Beta"),
            reverse,
            "{result}"
        );
    }
}

#[test]
fn unicode_labels_are_not_transliterated_in_ascii_mode() {
    for ascii in [false, true] {
        let result = render("flowchart LR\nA[東京 café] -->|λ| B[受信]", ascii, None).unwrap();
        for label in ["東京", "café", "λ", "受信"] {
            assert!(result.contains(label), "{result}");
        }
    }
}

#[test]
fn unsupported_invalid_and_resources_are_different_errors() {
    assert!(matches!(
        render("pie\n\"A\": 2", false, None),
        Err(DiagramError::Unsupported(_))
    ));
    assert!(matches!(
        render("flowchart LR\nA[", false, None),
        Err(DiagramError::Invalid(_))
    ));
    assert!(matches!(
        render(&"x".repeat(65_537), false, None),
        Err(DiagramError::Resource(_))
    ));
}

#[test]
fn sequence_geometry_retains_participant_order_and_message_direction() {
    let text = render(
        "sequenceDiagram\nparticipant West\nparticipant East\nWest->>East: eastbound\nEast-->>West: westbound",
        true,
        None,
    )
    .unwrap();
    let actors = text.lines().find(|line| line.contains("West")).unwrap();
    assert!(actors.find("West") < actors.find("East"), "{text}");
    let lines: Vec<_> = text.lines().collect();
    let east = lines
        .iter()
        .position(|line| line.contains("eastbound"))
        .unwrap();
    let west = lines
        .iter()
        .position(|line| line.contains("westbound"))
        .unwrap();
    assert!(east < west, "{text}");
    assert!(
        lines[east..west].iter().any(|line| line.contains('>')),
        "{text}"
    );
    assert!(
        lines[west..].iter().any(|line| line.contains('<')),
        "{text}"
    );
}

#[test]
fn class_er_and_state_preserve_four_authored_directions() {
    for family in ["class", "er", "state"] {
        for (direction, horizontal, reversed) in [
            ("LR", true, false),
            ("RL", true, true),
            ("TB", false, false),
            ("BT", false, true),
        ] {
            let source = match family {
                "class" => format!("classDiagram\ndirection {direction}\nAlpha --> Beta : uses"),
                "er" => format!("erDiagram\ndirection {direction}\nAlpha ||--o{{ Beta : owns"),
                _ => format!("stateDiagram-v2\ndirection {direction}\nAlpha --> Beta: starts"),
            };
            let text = render(&source, true, Some(0)).unwrap();
            let lines: Vec<_> = text.lines().collect();
            let a = lines
                .iter()
                .position(|line| line.contains("Alpha"))
                .unwrap();
            let b = lines.iter().position(|line| line.contains("Beta")).unwrap();
            if horizontal {
                assert_eq!(a, b, "{source}\n{text}");
                assert_eq!(
                    lines[a].find("Alpha") > lines[b].find("Beta"),
                    reversed,
                    "{source}\n{text}",
                );
            } else {
                assert_eq!(a > b, reversed, "{source}\n{text}");
            }
        }
    }
}

#[test]
fn nested_groups_branches_cycles_notes_and_multiline_labels_are_spatial() {
    let cases: &[(&str, &[&str])] = &[
        (
            "flowchart TD\nsubgraph Pipeline\nA[First<br/>second] -->|yes| B{Choose}\nB --> C[Left]\nB --> D[Right]\nC --> A\nend",
            &[
                "Pipeline", "First", "second", "Choose", "Left", "Right", "yes",
            ],
        ),
        (
            "sequenceDiagram\nparticipant Reader\nparticipant Service\nloop retry\nReader->>Service: request\nalt ready\nService-->>Reader: done\nelse busy\nNote over Reader,Service: waiting\nend\nend",
            &[
                "Reader", "Service", "retry", "ready", "request", "done", "busy", "waiting",
            ],
        ),
        (
            "stateDiagram-v2\nstate Running {\nIdle --> Busy: begin\n}",
            &["Running", "Idle", "Busy", "begin"],
        ),
        (
            "stateDiagram-v2\nIdle --> Busy: begin\nnote right of Busy: active",
            &["Idle", "Busy", "begin", "active"],
        ),
        (
            "classDiagram\nnamespace Domain {\nclass Reader {\n+String name\n+read()\n}\nclass Book\n}\nReader --> Book : reads",
            &["Domain", "Reader", "Book", "name", "read", "reads"],
        ),
        (
            "classDiagram\nReader --> Book : reads\nnote for Reader \"Human\"",
            &["Reader", "Book", "reads", "Human"],
        ),
        (
            "classDiagram\nclass Shape {\n<<interface>>\n+draw()\n}\nShape <|-- Circle",
            &["Shape", "interface", "draw", "Circle"],
        ),
        (
            "erDiagram\nPERSON ||--o{ ORDER : places\nORDER ||--|{ ITEM : contains\nPERSON {\nstring name PK \"display name\"\n}\nITEM {\nint id PK\n}",
            &[
                "PERSON",
                "ORDER",
                "ITEM",
                "places",
                "contains",
                "name",
                "PK",
                "display name",
                "id",
            ],
        ),
    ];
    for &(source, labels) in cases {
        for ascii in [false, true] {
            let text =
                render(source, ascii, Some(40)).unwrap_or_else(|error| panic!("{source}\n{error}"));
            for label in labels {
                assert!(text.contains(label), "{source}\nmissing {label}: {text}");
            }
            assert!(text.lines().count() > 5, "{text}");
        }
    }
}

#[test]
fn unsupported_semantics_never_disappear_into_success() {
    for source in [
        "flowchart LR\nA --> B\nclick A \"https://example.org\"",
        "flowchart LR\nA & B --> C",
        "sequenceDiagram\nparticipant A\nparticipant B\nlink A: Site @ https://example.org\nA->>B: hello",
        "flowchart LR\naccTitle: Secret context\nA --> B",
        "%%{init: {\"theme\": \"dark\"}}%%\nflowchart LR\nA --> B",
        "flowchart LR; %%{init: {\"theme\": \"dark\"}}%%\nA --> B",
        "flowchart LR; accTitle: Secret context\nA --> B",
        "classDiagram\nAnimal <|-- Dog : parent\nAnimal <|-- Dog : base",
        "classDiagram\nnamespace Domain {\nclass Reader {\n+String name\n+read()\n}\nclass Book\n}\nReader --> Book : reads\nnote for Reader \"Human\"",
        "erDiagram\nA ||--|| B : owns\nA ||..o{ B : contains",
        "stateDiagram-v2\nstate Running {\nIdle --> Busy: begin\nBusy --> Idle: finish\n}\n[*] --> Running\nnote right of Running: active",
        "stateDiagram-v2\nstate Running {\nIdle --> Busy: begin\nBusy --> Idle: finish\n}",
        "flowchart LR\nA[<img src=\"https://example.org/image\" alt=\"Portrait\">] --> B",
        "flowchart LR\nA[<script>Important label</script>] --> B",
    ] {
        assert!(
            matches!(
                render(source, false, None),
                Err(DiagramError::Unsupported(_))
            ),
            "{source}",
        );
    }
}

#[test]
fn terminal_controls_are_rejected_without_emitting_terminal_commands() {
    for control in ['\x00', '\x07', '\x1b', '\u{009b}', '\u{202e}', '\u{2066}'] {
        let source = format!("flowchart LR\nA[before{control}after] --> B");
        let error = render(&source, false, None).unwrap_err();
        assert!(matches!(error, DiagramError::Invalid(_)), "{error}");
        assert!(!error.to_string().contains(control));
    }
}

#[test]
fn malformed_input_is_explicit_for_every_family() {
    for source in [
        "",
        "%% only a comment",
        "flowchart LR\nA[",
        "sequenceDiagram\nA->>:",
        "stateDiagram-v2\nstate {",
        "classDiagram\nclass {",
        "erDiagram\nA ||--",
    ] {
        assert!(
            matches!(render(source, false, None), Err(DiagramError::Invalid(_))),
            "{source}",
        );
    }
}

#[test]
fn node_edge_nesting_and_grapheme_ceiling_rejections_are_resources() {
    let nodes = format!(
        "flowchart TD\n{}",
        (0..257).map(|i| format!("N{i}\n")).collect::<String>()
    );
    let edges = format!("flowchart LR\n{}", "A --> B\n".repeat(513));
    let participants = format!(
        "sequenceDiagram\n{}",
        (0..257)
            .map(|i| format!("participant P{i}\n"))
            .collect::<String>()
    );
    let nesting = format!(
        "flowchart TD\n{}A\n{}",
        "subgraph Group\n".repeat(33),
        "end\n".repeat(33)
    );
    let grapheme = format!("flowchart LR\nA[\"a{}\"] --> B", "\u{0301}".repeat(256));
    let inline_nesting = format!(
        "flowchart TD;{}A;{}",
        (0..33)
            .map(|i| format!("subgraph G{i};"))
            .collect::<String>(),
        "end;".repeat(33)
    );
    let lexical = format!("flowchart TD\n{}", "A-->B;".repeat(2048));
    for source in [
        nodes,
        edges,
        participants,
        nesting,
        inline_nesting,
        lexical,
        grapheme,
    ] {
        let error = render(&source, false, None).unwrap_err();
        assert!(matches!(error, DiagramError::Resource(_)), "{error}");
    }
}

#[test]
fn exact_source_ceiling_and_long_labels_retain_all_authored_words() {
    let base = "flowchart LR\nA[Alpha] --> B[Beta]\n%%";
    let exact = format!("{base}{}", " ".repeat(65_536 - base.len()));
    let expected = render(base, false, None).unwrap();
    assert_eq!(render(&exact, false, None).unwrap(), expected);
    assert!(matches!(
        render(&format!("{exact} "), false, None),
        Err(DiagramError::Resource(_)),
    ));
    let words: Vec<_> = (0..40).map(|i| format!("word{i:02}")).collect();
    let source = format!("flowchart LR\nA[\"{}\"] --> B[Done]", words.join(" "));
    for width in [Some(0), Some(40), Some(120), None] {
        let text = render(&source, false, width).unwrap();
        for word in &words {
            assert!(text.contains(word), "missing {word} in {text}");
        }
        assert!(text.contains("Done"));
        assert!(!text.contains('…'));
    }
}

#[test]
fn cancelled_and_expired_operations_never_become_source_fallbacks() {
    let control = DiagramControl::new();
    control.cancel();
    assert!(matches!(
        render_controlled(CORPUS[0].0, false, None, &control),
        Err(DiagramError::Cancelled(_)),
    ));
    let expired = DiagramControl::new().with_deadline(std::time::Duration::ZERO);
    assert!(matches!(
        render_controlled(CORPUS[0].0, false, None, &expired),
        Err(DiagramError::Cancelled(_)),
    ));
    let reusable = DiagramControl::new();
    assert_eq!(
        render_controlled(CORPUS[0].0, false, None, &reusable).unwrap(),
        render_controlled(CORPUS[0].0, false, None, &reusable).unwrap(),
    );
    let clone = reusable.clone();
    std::thread::spawn(move || clone.cancel()).join().unwrap();
    assert!(matches!(
        render_controlled(CORPUS[0].0, false, None, &reusable),
        Err(DiagramError::Cancelled(_)),
    ));
}

/// Explicit opt-in measurement, not a timing-sensitive pass/fail test.
#[test]
#[ignore = "run with --ignored --nocapture --test-threads=1 to measure native corpus"]
fn characterize_native_corpus() {
    use std::time::Instant;
    for &(source, _) in CORPUS {
        let start = Instant::now();
        let result = render(source, false, Some(80)).unwrap();
        let first = start.elapsed();
        let start = Instant::now();
        for _ in 0..100 {
            assert_eq!(render(source, false, Some(80)).unwrap(), result);
        }
        eprintln!(
            "{}: first={first:?}, repeated_mean={:?}, rows={}, max_columns={}, bytes={}",
            source.lines().next().unwrap(),
            start.elapsed() / 100,
            result.lines().count(),
            result
                .lines()
                .map(unicode_width::UnicodeWidthStr::width)
                .max()
                .unwrap_or(0),
            result.len(),
        );
    }
    for nodes in [16, 64, 128, 256] {
        let source = format!(
            "flowchart LR\n{}",
            (1..nodes)
                .map(|i| format!("N{} --> N{i}\n", i - 1))
                .collect::<String>()
        );
        let start = Instant::now();
        let result = render(&source, false, None);
        eprintln!(
            "{nodes}-node chain: {:?}; {:?}",
            start.elapsed(),
            result.as_ref().map(String::len)
        );
    }
}
