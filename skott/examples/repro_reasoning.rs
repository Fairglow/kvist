use skott::session::Event;
use skott::tui::app::{App, LineKind, REASONING_EDGE_CONT, REASONING_EDGE_FIRST};
use skott::tui::theme::Theme;
use sav::ReasoningEffort;

fn build(width: u16) -> App {
    App::new(
        &["local".to_owned()],
        Some("local"),
        ReasoningEffort::Medium,
        None,
        width,
        24,
        Theme::dark(),
    )
}

fn main() {
    let mut app = build(40);
    app.lines.clear();
    app.push_event(Event::Reasoning(
        "The quick brown fox jumps over the lazy dog again and again and again".to_owned(),
    ));
    app.push_event(Event::Finished {
        message: "done".to_owned(),
    });
    println!("=== reasoning rows (width 40) ===");
    for (i, line) in app.lines.iter().enumerate() {
        let t = line.line.to_string();
        let tag = if t.starts_with(REASONING_EDGE_FIRST) {
            "F"
        } else if t.starts_with(REASONING_EDGE_CONT) {
            "C"
        } else {
            "-"
        };
        let kind = match line.kind {
            LineKind::Reasoning => "R",
            _ => ".",
        };
        println!("{i:2} [{}|{}] {:?}", kind, tag, t);
    }
}
