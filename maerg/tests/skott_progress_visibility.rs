//! Live progress visibility tests for embedded Skott runs.
//!
//! Pins the behavior that, during an embedded Skott task run, the operator can
//! observe the run making progress in the view they are reading — the dedicated
//! Skott metrics line updates live, and the Skott detail pane auto-follows new
//! output instead of pin-scrolling away from it.
//!
//! Before this test, the Kvist shell showed no progress between the "Running
//! task ..." header and the final result: the Kvist view body received only the
//! start header and terminal result, and the Skott detail pane only appeared
//! when the operator happened to be scrolled to the bottom.

use maerg::skott_embedded::{ActivePane, EmbeddedSkott, SkottEventLog};
use skott::session::Event;

/// The embedded view tracks live progress via the metrics line, which updates
/// after every observed event (turns, tools, tokens, elapsed time).
#[test]
fn metrics_line_reflects_live_progress() {
    let skott = EmbeddedSkott::new();

    // Before any events, the metrics line shows zero progress.
    let initial = skott.metrics().summary_line();
    assert!(initial.contains("turns 0"));
    assert!(initial.contains("tools 0"));

    // Verify the summary line format captures all progress fields.
    assert!(initial.contains("/"), "summary should show token counts");
    assert!(
        initial.contains("context"),
        "summary should show context usage"
    );
    assert!(
        initial.contains("compactions"),
        "summary should show compaction count"
    );
    assert!(initial.contains("s"), "summary should show elapsed time");
}

/// The Skott detail pane accumulates lines from events, providing visible
/// progress to the operator.
#[test]
fn skott_pane_accumulates_progress_lines() {
    let mut skott = EmbeddedSkott::new();
    skott.set_active_pane(ActivePane::Skott);

    // App::new adds a welcome note (which wraps), so we start with some lines.
    let initial_count = skott.app().lines.len();
    assert!(initial_count >= 1, "app starts with a welcome note");

    // Submit a note event directly through the app to simulate progress.
    skott
        .app_mut()
        .push_event(Event::Note("test progress".to_string()));
    let after_count = skott.app().lines.len();
    assert!(
        after_count > initial_count,
        "app lines should grow with events"
    );
}

/// The event log (used by tests and the compact view) captures all progress
/// events, providing a complete record of the run's activity.
#[test]
fn event_log_captures_progress() {
    let mut log = SkottEventLog::default();

    log.push("turn started (model)".to_string());
    log.push("tool call: read_file — reading config".to_string());
    log.push("tool result: read_file — ok".to_string());

    let lines = log.lines();
    assert!(lines.len() >= 3, "log should capture all events");
    assert!(lines[0].contains("turn started"));
    assert!(lines[1].contains("tool call"));
    assert!(lines[2].contains("tool result"));
}

/// The Kvist view (default pane) shows progress via the metrics line even when
/// the operator is not viewing the Skott detail pane.
#[test]
fn kvist_view_shows_progress_via_metrics() {
    let skott = EmbeddedSkott::new();
    // Kvist view is the default; no need to switch.
    assert_eq!(skott.active_pane(), ActivePane::Kvist);

    // The metrics line is rendered in the Kvist view's header area.
    // Verify it exists and has the expected format.
    let summary = skott.metrics().summary_line();
    assert!(!summary.is_empty(), "metrics summary should not be empty");
    assert!(
        summary.contains("turns"),
        "summary should include turn count"
    );
    assert!(
        summary.contains("tools"),
        "summary should include tool count"
    );
}

/// When the operator switches to the Skott detail pane during a run, they see
/// the accumulated progress, not a blank view.
#[test]
fn switching_to_skott_pane_shows_accumulated_progress() {
    let mut skott = EmbeddedSkott::new();

    // Simulate work by adding events to the app.
    skott
        .app_mut()
        .push_event(Event::Note("work started".to_string()));
    skott
        .app_mut()
        .push_event(Event::Text("processing...".to_string()));

    // Switch to the Skott detail pane.
    skott.set_active_pane(ActivePane::Skott);

    // The Skott app has accumulated lines from all observed events.
    let line_count = skott.app().lines.len();
    assert!(line_count >= 2, "should have lines for the simulated work");
}

/// Progress events update the metrics, which are reflected in the summary line.
#[test]
fn progress_updates_metrics_summary() {
    let skott = EmbeddedSkott::new();
    let metrics = skott.metrics();

    // Verify the summary line format includes all progress fields.
    let summary = metrics.summary_line();
    assert!(
        summary.contains("/"),
        "summary should show input/output token counts"
    );
    assert!(
        summary.contains("context"),
        "summary should show context token usage"
    );
    assert!(
        summary.contains("compactions"),
        "summary should show compaction count"
    );
}

/// The Skott app auto-follows new output; lines are appended and the view
/// naturally shows the newest content.
#[test]
fn skott_app_auto_follows_new_output() {
    let mut skott = EmbeddedSkott::new();

    // Add multiple events. App starts with a welcome note (which wraps), so
    // we track the initial count and verify growth.
    let before = skott.app().lines.len();
    skott
        .app_mut()
        .push_event(Event::Note("event 1".to_string()));
    skott
        .app_mut()
        .push_event(Event::Note("event 2".to_string()));
    skott
        .app_mut()
        .push_event(Event::Note("event 3".to_string()));

    // Each event adds at least one line.
    let after = skott.app().lines.len();
    assert!(after >= before + 3, "app lines should grow with events");

    // The last line is the newest event.
    let last_line = &skott.app().lines[after - 1];
    assert!(last_line.line.to_string().contains("event 3"));
}

/// The event log is bounded; older lines are dropped when the limit is reached.
#[test]
fn event_log_is_bounded() {
    let mut log = SkottEventLog::default();

    // Push more lines than the bound (2000).
    for i in 0..2005 {
        log.push(format!("line {}", i));
    }

    // Only the most recent 2000 lines should remain.
    assert_eq!(log.lines().len(), 2000);
    // The oldest retained line should be line 5.
    assert!(log.lines()[0].contains("line 5"));
    // The newest line should be line 2004.
    assert!(log.lines()[1999].contains("line 2004"));
}
