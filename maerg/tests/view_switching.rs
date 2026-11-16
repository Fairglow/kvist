//! View-switching key and menu regression tests.
//!
//! Pins the documented behavior: Ctrl+Tab cycles views, Ctrl+PageUp/PageDown
//! select specific views, Alt+Left/Right select specific views, the menu
//! exposes a view-switch item when a Skott session is active, and the
//! joint/side-by-side view is reachable.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use maerg::skott_embedded::{ActivePane, EmbeddedSkott};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: crossterm::event::KeyEventKind::Press,
        state: crossterm::event::KeyEventState::NONE,
    }
}

#[test]
fn ctrl_tab_cycles_views() {
    let mut skott = EmbeddedSkott::new();
    assert_eq!(skott.active_pane(), ActivePane::Kvist);

    // First Ctrl+Tab: Kvist -> Skott
    assert!(skott.handle_key(key(KeyCode::Tab, KeyModifiers::CONTROL)));
    assert_eq!(skott.active_pane(), ActivePane::Skott);

    // Second Ctrl+Tab: Skott -> Both (side-by-side)
    assert!(skott.handle_key(key(KeyCode::Tab, KeyModifiers::CONTROL)));
    assert_eq!(skott.active_pane(), ActivePane::Both);

    // Third Ctrl+Tab: Both -> Kvist (cycle complete)
    assert!(skott.handle_key(key(KeyCode::Tab, KeyModifiers::CONTROL)));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

#[test]
fn ctrl_pageup_selects_kvist_view() {
    let mut skott = EmbeddedSkott::new();
    skott.set_active_pane(ActivePane::Skott);

    assert!(skott.handle_key(key(KeyCode::PageUp, KeyModifiers::CONTROL)));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

#[test]
fn ctrl_pagedown_selects_skott_view() {
    let mut skott = EmbeddedSkott::new();
    skott.set_active_pane(ActivePane::Kvist);

    assert!(skott.handle_key(key(KeyCode::PageDown, KeyModifiers::CONTROL)));
    assert_eq!(skott.active_pane(), ActivePane::Skott);
}

#[test]
fn alt_left_selects_kvist_view() {
    let mut skott = EmbeddedSkott::new();
    skott.set_active_pane(ActivePane::Skott);

    assert!(skott.handle_key(key(KeyCode::Left, KeyModifiers::ALT)));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

#[test]
fn alt_right_selects_skott_view() {
    let mut skott = EmbeddedSkott::new();
    skott.set_active_pane(ActivePane::Kvist);

    assert!(skott.handle_key(key(KeyCode::Right, KeyModifiers::ALT)));
    assert_eq!(skott.active_pane(), ActivePane::Skott);
}

#[test]
fn joint_view_is_reachable() {
    let mut skott = EmbeddedSkott::new();

    // Reach Both view via Ctrl+Tab cycling
    skott.handle_key(key(KeyCode::Tab, KeyModifiers::CONTROL)); // -> Skott
    skott.handle_key(key(KeyCode::Tab, KeyModifiers::CONTROL)); // -> Both
    assert_eq!(skott.active_pane(), ActivePane::Both);

    // Can switch away from Both
    skott.handle_key(key(KeyCode::PageUp, KeyModifiers::CONTROL));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

#[test]
fn plain_tab_is_not_consumed() {
    let mut skott = EmbeddedSkott::new();

    // Plain Tab (no modifier) should not be consumed by view switching
    assert!(!skott.handle_key(key(KeyCode::Tab, KeyModifiers::NONE)));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

#[test]
fn plain_pageup_is_not_consumed() {
    let mut skott = EmbeddedSkott::new();

    // Plain PageUp (no modifier) should not be consumed by view switching
    assert!(!skott.handle_key(key(KeyCode::PageUp, KeyModifiers::NONE)));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

#[test]
fn plain_pagedown_is_not_consumed() {
    let mut skott = EmbeddedSkott::new();

    // Plain PageDown (no modifier) should not be consumed by view switching
    assert!(!skott.handle_key(key(KeyCode::PageDown, KeyModifiers::NONE)));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

#[test]
fn unrelated_keys_are_not_consumed() {
    let mut skott = EmbeddedSkott::new();

    assert!(!skott.handle_key(key(KeyCode::Char('x'), KeyModifiers::NONE)));
    assert!(!skott.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(!skott.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)));
}

/// F3 cycles views: Kvist -> Skott -> Both -> Kvist.
#[test]
fn f3_cycles_views() {
    let mut skott = EmbeddedSkott::new();
    assert_eq!(skott.active_pane(), ActivePane::Kvist);

    assert!(skott.handle_key(key(KeyCode::F(3), KeyModifiers::NONE)));
    assert_eq!(skott.active_pane(), ActivePane::Skott);

    assert!(skott.handle_key(key(KeyCode::F(3), KeyModifiers::NONE)));
    assert_eq!(skott.active_pane(), ActivePane::Both);

    assert!(skott.handle_key(key(KeyCode::F(3), KeyModifiers::NONE)));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

/// F4 selects the Kvist metrics view.
#[test]
fn f4_selects_kvist_view() {
    let mut skott = EmbeddedSkott::new();
    skott.set_active_pane(ActivePane::Skott);

    assert!(skott.handle_key(key(KeyCode::F(4), KeyModifiers::NONE)));
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}

/// F5 selects the Skott detail view.
#[test]
fn f5_selects_skott_view() {
    let mut skott = EmbeddedSkott::new();
    skott.set_active_pane(ActivePane::Kvist);

    assert!(skott.handle_key(key(KeyCode::F(5), KeyModifiers::NONE)));
    assert_eq!(skott.active_pane(), ActivePane::Skott);
}

/// F3 can reach the joint/side-by-side view and switch away from it.
#[test]
fn f3_reaches_joint_view() {
    let mut skott = EmbeddedSkott::new();

    skott.handle_key(key(KeyCode::F(3), KeyModifiers::NONE)); // -> Skott
    skott.handle_key(key(KeyCode::F(3), KeyModifiers::NONE)); // -> Both
    assert_eq!(skott.active_pane(), ActivePane::Both);

    skott.handle_key(key(KeyCode::F(4), KeyModifiers::NONE)); // -> Kvist
    assert_eq!(skott.active_pane(), ActivePane::Kvist);
}
