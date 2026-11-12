//! Typed pages and bounded hierarchical menu navigation.

use std::{collections::BTreeSet, io, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

use crate::{config::Role, presentation};

use super::{Overlay, Workspace};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ModelKey {
    pub name: String,
    pub source: PathBuf,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Root,
    Components,
    Roles,
    Role { role: Role, global: bool },
    RoleModel { role: Role, global: bool },
    Effort { role: Role, global: bool },
    Models,
    Model(ModelKey),
    Appearance,
    Theme,
    History,
}

impl Page {
    pub fn searchable(&self) -> bool {
        matches!(
            self,
            Self::Components | Self::Models | Self::RoleModel { .. }
        )
    }
}

pub(super) struct Entry {
    pub label: String,
    pub action: Action,
    pub current: bool,
    search: String,
}

impl Entry {
    pub fn new(label: impl Into<String>, action: Action) -> Self {
        let label = label.into();
        let search = label.to_lowercase();
        Self {
            label,
            action,
            current: false,
            search,
        }
    }

    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
}

#[derive(Clone)]
pub(super) enum Action {
    Open(Page),
    Replace(Page),
    Back,
    Focus(Option<String>),
    Theme(String),
    Segment(u64),
    Follow,
    MaskAnswers(bool),
    Help,
    Quit,
    Cancel,
    SwitchView(crate::skott_embedded::ActivePane),
    Command {
        arguments: Vec<String>,
        confirm: Option<String>,
        return_to: Page,
    },
    Details {
        title: String,
        markdown: String,
    },
}

pub(super) struct Menu {
    pub page: Page,
    pub title: String,
    pub choices: Vec<Entry>,
    pub selected: usize,
    pub query: String,
}

impl Menu {
    pub fn new(page: Page, title: impl Into<String>, choices: Vec<Entry>) -> Self {
        let selected = choices
            .iter()
            .position(|choice| choice.current)
            .unwrap_or(0);
        Self {
            page,
            title: title.into(),
            choices,
            selected,
            query: String::new(),
        }
    }

    pub fn visible(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.choices
            .iter()
            .enumerate()
            .filter(|(_, entry)| query.is_empty() || entry.search.contains(&query))
            .map(|(index, _)| index)
            .collect()
    }

    pub fn filter_insert(&mut self, text: &str) -> bool {
        let text: String = text
            .chars()
            .filter_map(|ch| match ch {
                '\n' | '\r' | '\t' => Some(' '),
                ch if ch.is_control() => None,
                ch => Some(ch),
            })
            .collect();
        if self.query.len() + text.len() > 256 {
            return false;
        }
        self.query.push_str(&text);
        self.selected = 0;
        true
    }
}

fn document_scroll(scroll: usize, key: KeyCode, viewport: usize) -> usize {
    match key {
        KeyCode::Up => scroll.saturating_sub(1),
        KeyCode::Down => scroll.saturating_add(1),
        KeyCode::PageUp => scroll.saturating_sub(viewport),
        KeyCode::PageDown => scroll.saturating_add(viewport),
        KeyCode::Home => 0,
        KeyCode::End => usize::MAX,
        _ => scroll,
    }
}

impl Workspace {
    pub(super) fn build_menu(&self, page: Page) -> io::Result<Menu> {
        let choices = match &page {
            Page::Root => {
                let mut entries = vec![
                    Entry::new("Components", Action::Open(Page::Components)),
                    Entry::new("Role assignments", Action::Open(Page::Roles)),
                    Entry::new("Models", Action::Open(Page::Models)),
                    Entry::new("Appearance", Action::Open(Page::Appearance)),
                    Entry::new("History", Action::Open(Page::History)),
                    Entry::new("Help", Action::Help),
                ];
                if self.running.is_some() {
                    entries.push(Entry::new("Cancel command", Action::Cancel));
                }
                if let Some(skott) = self.skott.as_ref().filter(|skott| skott.is_active()) {
                    let pane = skott.active_pane();
                    let next = match pane {
                        crate::skott_embedded::ActivePane::Kvist => {
                            crate::skott_embedded::ActivePane::Skott
                        }
                        crate::skott_embedded::ActivePane::Skott => {
                            crate::skott_embedded::ActivePane::Both
                        }
                        crate::skott_embedded::ActivePane::Both => {
                            crate::skott_embedded::ActivePane::Kvist
                        }
                    };
                    let label = format!(
                        "Switch view (Ctrl+Tab): {}",
                        match next {
                            crate::skott_embedded::ActivePane::Kvist => "Kvist metrics",
                            crate::skott_embedded::ActivePane::Skott => "Skott detail",
                            crate::skott_embedded::ActivePane::Both => "side by side",
                        }
                    );
                    entries.push(Entry::new(label, Action::SwitchView(next)));
                }
                entries.push(Entry::new("Quit", Action::Quit));
                return Ok(Menu::new(page, "Workspace", entries));
            }
            Page::Components => {
                let known = self.state.components();
                let known_set: BTreeSet<_> = known.iter().map(String::as_str).collect();
                let mut seen = BTreeSet::new();
                let mut ordered = Vec::new();
                for name in self
                    .preferences
                    .pinned
                    .iter()
                    .chain(&self.preferences.recent)
                    .chain(known.iter())
                {
                    if known_set.contains(name.as_str()) && seen.insert(name.as_str()) {
                        ordered.push(name.clone());
                    }
                }
                let focus = self.focus();
                let mut entries =
                    vec![Entry::new("(project)", Action::Focus(None)).current(focus.is_none())];
                for name in ordered {
                    let tag = if self.preferences.pinned.contains(&name) {
                        " [pinned]"
                    } else if self.preferences.recent.contains(&name) {
                        " [recent]"
                    } else {
                        ""
                    };
                    entries.push(
                        Entry::new(format!("{name}{tag}"), Action::Focus(Some(name.clone())))
                            .current(focus.as_ref() == Some(&name)),
                    );
                }
                return Ok(Menu::new(
                    page,
                    "Components | type to filter | Ctrl+P pin",
                    entries,
                ));
            }
            Page::Appearance => {
                let mut entries = vec![Entry::new(
                    format!("Theme: {}", self.theme.name()),
                    Action::Open(Page::Theme),
                )];
                entries.push(
                    Entry::new(
                        if self.mask_answers {
                            "Mask command answers: on"
                        } else {
                            "Mask command answers: off"
                        },
                        Action::MaskAnswers(!self.mask_answers),
                    )
                    .current(self.mask_answers),
                );
                entries
            }
            Page::Theme => crate::bark::theme::registered()
                .into_iter()
                .map(|(name, _, _)| {
                    let current = name == self.theme.name();
                    Entry::new(name.clone(), Action::Theme(name)).current(current)
                })
                .collect(),
            Page::History => {
                let last = self.transcript.latest();
                let mut entries =
                    vec![Entry::new("Follow latest output", Action::Follow).current(self.follow)];
                entries.extend((last.saturating_sub(99)..=last).rev().map(|sequence| {
                    Entry::new(
                        format!("Transcript segment {}", sequence + 1),
                        Action::Segment(sequence),
                    )
                    .current(!self.follow && sequence == self.segment)
                }));
                entries
            }
            _ => return self.model_menu(page),
        };
        // Only `Appearance`, `Theme` and `History` fall through to the title
        // match (every other page returns above); the fallback keeps the
        // invariant a debug build still verifies without a runtime panic.
        let title = match &page {
            Page::Appearance => "Appearance",
            Page::Theme => "Appearance / Theme",
            Page::History => "History (PageUp reaches all older segments)",
            other => {
                debug_assert!(false, "unhandled menu page: {other:?}");
                "Menu"
            }
        };
        Ok(Menu::new(page, title, choices))
    }

    pub(super) fn open_menu(&mut self, page: Page) -> io::Result<()> {
        let menu = self.build_menu(page)?;
        let mut parents = Vec::new();
        if menu.page != Page::Root {
            let target = if menu.page == Page::Theme {
                Page::Appearance
            } else {
                menu.page.clone()
            };
            let mut root = self.build_menu(Page::Root)?;
            root.selected = root
                .choices
                .iter()
                .position(|entry| matches!(&entry.action, Action::Open(page) if page == &target))
                .unwrap_or(0);
            parents.push(root);
            if menu.page == Page::Theme {
                parents.push(self.build_menu(Page::Appearance)?);
            }
        }
        self.candidates.clear();
        self.menu_stack = parents;
        self.overlay = Some(Overlay::Menu(menu));
        Ok(())
    }

    pub(super) fn back(&mut self) {
        self.overlay = self.menu_stack.pop().map(Overlay::Menu);
    }

    pub(super) fn request_quit(&mut self) {
        if self.running.is_some() {
            if !matches!(self.overlay, Some(Overlay::Quit)) {
                self.quit_return = self.overlay.take();
            }
            self.overlay = Some(Overlay::Quit);
        } else {
            self.quit = true;
            self.quit_return = None;
        }
    }

    fn setting_command(&mut self, arguments: Vec<String>, return_to: Page) -> io::Result<()> {
        if self.running.is_some() {
            return Err(io::Error::other(
                "wait for the running command before changing settings",
            ));
        }
        self.append(&format!(
            "\n\n## Settings command\n\n{}\n\n",
            presentation::fence(
                &arguments
                    .iter()
                    .map(|arg| super::super::editor::quote(arg))
                    .collect::<Vec<_>>()
                    .join(" "),
                "text"
            )
        ))?;
        self.start(arguments)?;
        if self
            .menu_stack
            .last()
            .is_some_and(|parent| parent.page == return_to)
        {
            self.menu_stack.pop();
        }
        self.pending_page = Some(return_to);
        Ok(())
    }

    pub(super) fn overlay_key(&mut self, key: KeyEvent) -> io::Result<()> {
        if key.code == KeyCode::F(2) {
            let cancel = matches!(self.overlay, Some(Overlay::Confirm { .. }));
            self.open_menu(Page::Root)?;
            self.quit_return = None;
            if cancel {
                self.prompt_file.take();
                self.pending_page = None;
                self.append("\n\nOperation cancelled.\n")?;
            }
            return Ok(());
        }
        let Some(overlay) = self.overlay.take() else {
            return Ok(());
        };
        match overlay {
            Overlay::Quit => match key.code {
                KeyCode::Char('y' | 'Y') => {
                    if let Some(run) = &mut self.running {
                        run.cancel()?;
                    }
                    self.quit = true;
                    self.menu_stack.clear();
                    self.pending_page = None;
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => self.overlay = self.quit_return.take(),
                _ => self.overlay = Some(Overlay::Quit),
            },
            Overlay::Help { mut scroll } => {
                match key.code {
                    KeyCode::Esc | KeyCode::F(1) => {
                        self.back();
                        return Ok(());
                    }
                    _ => scroll = document_scroll(scroll, key.code, self.viewport),
                }
                self.overlay = Some(Overlay::Help { scroll });
            }
            Overlay::Details {
                title,
                markdown,
                mut scroll,
            } => {
                match key.code {
                    KeyCode::Esc => {
                        self.back();
                        return Ok(());
                    }
                    _ => scroll = document_scroll(scroll, key.code, self.viewport),
                }
                self.overlay = Some(Overlay::Details {
                    title,
                    markdown,
                    scroll,
                });
            }
            Overlay::Confirm {
                prompt,
                arguments,
                scroll,
            } => match key.code {
                KeyCode::Char('y' | 'Y') => {
                    if let Some(page) = self.pending_page.take() {
                        self.menu_stack.pop();
                        self.setting_command(arguments, page)?;
                    } else {
                        self.start(arguments)?;
                    }
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    self.prompt_file.take();
                    self.pending_page = None;
                    self.back();
                    self.append("\n\nOperation cancelled.\n")?;
                }
                _ => {
                    self.overlay = Some(Overlay::Confirm {
                        prompt,
                        arguments,
                        scroll: document_scroll(scroll, key.code, self.viewport),
                    })
                }
            },
            Overlay::Menu(mut menu) => {
                if key.code == KeyCode::Esc {
                    self.back();
                    return Ok(());
                }
                if key.code == KeyCode::F(1) {
                    self.menu_stack.push(menu);
                    self.overlay = Some(Overlay::Help { scroll: 0 });
                    return Ok(());
                }
                let visible = menu.visible();
                let count = visible.len();
                match key.code {
                    KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
                    KeyCode::Down => {
                        menu.selected = (menu.selected + 1).min(count.saturating_sub(1))
                    }
                    KeyCode::Home => menu.selected = 0,
                    KeyCode::End => menu.selected = count.saturating_sub(1),
                    KeyCode::PageUp => menu.selected = menu.selected.saturating_sub(10),
                    KeyCode::PageDown => {
                        menu.selected = (menu.selected + 10).min(count.saturating_sub(1))
                    }
                    KeyCode::Char('p')
                        if key.modifiers.contains(KeyModifiers::CONTROL)
                            && menu.page == Page::Components =>
                    {
                        if self.running.is_some() {
                            self.overlay = Some(Overlay::Menu(menu));
                            return Err(io::Error::other(
                                "wait for the running command before changing component shortcuts",
                            ));
                        }
                        if let Some(&index) = visible.get(menu.selected)
                            && let Action::Focus(Some(component)) = &menu.choices[index].action
                        {
                            let component = component.clone();
                            let mut preferences = self.preferences.clone();
                            if preferences.pinned.contains(&component) {
                                preferences.pinned.retain(|entry| entry != &component);
                            } else if preferences.pinned.len() < 12 {
                                preferences.pinned.push(component.clone());
                            } else {
                                self.overlay = Some(Overlay::Menu(menu));
                                return Err(io::Error::other(
                                    "at most 12 pinned components; unpin one first",
                                ));
                            }
                            if let Err(error) = preferences.save(&self.project) {
                                self.overlay = Some(Overlay::Menu(menu));
                                return Err(error);
                            }
                            self.preferences = preferences;
                            let mut refreshed = match self.build_menu(Page::Components) {
                                Ok(refreshed) => refreshed,
                                Err(error) => {
                                    self.overlay = Some(Overlay::Menu(menu));
                                    return Err(error);
                                }
                            };
                            refreshed.query = menu.query;
                            menu = refreshed;
                            menu.selected = menu.visible().iter().position(|&i| {
                                matches!(&menu.choices[i].action, Action::Focus(Some(name)) if name == &component)
                            }).unwrap_or(0);
                        }
                    }
                    KeyCode::Backspace if menu.page.searchable() => {
                        let end = menu
                            .query
                            .grapheme_indices(true)
                            .next_back()
                            .map(|(index, _)| index)
                            .unwrap_or(0);
                        menu.query.truncate(end);
                        menu.selected = 0;
                    }
                    KeyCode::Char(ch)
                        if menu.page.searchable()
                            && !ch.is_control()
                            && !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        if !menu.filter_insert(&ch.to_string()) {
                            self.notice = "Search exceeds 256 UTF-8 bytes".into();
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(&index) = visible.get(menu.selected) {
                            let action = menu.choices[index].action.clone();
                            return self.menu_action(menu, action);
                        }
                    }
                    _ => {}
                }
                self.overlay = Some(Overlay::Menu(menu));
            }
        }
        Ok(())
    }

    fn menu_action(&mut self, menu: Menu, action: Action) -> io::Result<()> {
        match action {
            Action::Open(page) => {
                let next = match self.build_menu(page) {
                    Ok(next) => next,
                    Err(error) => {
                        self.overlay = Some(Overlay::Menu(menu));
                        return Err(error);
                    }
                };
                if self.menu_stack.len() >= 8 {
                    self.overlay = Some(Overlay::Menu(menu));
                    return Err(io::Error::other("menu nesting exceeds eight levels"));
                }
                self.menu_stack.push(menu);
                self.overlay = Some(Overlay::Menu(next));
            }
            Action::Replace(page) => match self.build_menu(page) {
                Ok(mut next) => {
                    next.selected = menu.selected.min(next.choices.len().saturating_sub(1));
                    self.overlay = Some(Overlay::Menu(next));
                }
                Err(error) => {
                    self.overlay = Some(Overlay::Menu(menu));
                    return Err(error);
                }
            },
            Action::Back => self.back(),
            Action::SwitchView(pane) => {
                if let Some(skott) = &mut self.skott {
                    skott.set_active_pane(pane);
                    self.dirty = true;
                }
                self.back();
            }
            Action::Focus(focus) => {
                if let Err(error) = self.change_focus(focus) {
                    self.overlay = Some(Overlay::Menu(menu));
                    return Err(error);
                }
                self.back();
            }
            Action::Theme(theme) => {
                if let Err(error) = self.set_theme(&theme) {
                    self.overlay = Some(Overlay::Menu(menu));
                    return Err(error);
                }
                if let Some(parent) = self.menu_stack.pop() {
                    self.overlay = Some(Overlay::Menu(self.build_menu(parent.page)?));
                }
            }
            Action::Segment(segment) => {
                self.follow = false;
                self.load_segment(segment, false)?;
                self.back();
            }
            Action::Follow => {
                self.follow = true;
                self.load_segment(self.transcript.latest(), true)?;
                self.back();
            }
            Action::MaskAnswers(mask) => {
                self.mask_answers = mask;
                self.back();
            }
            Action::Help => {
                self.menu_stack.push(menu);
                self.overlay = Some(Overlay::Help { scroll: 0 });
            }
            Action::Quit => {
                self.overlay = Some(Overlay::Menu(menu));
                self.request_quit();
            }
            Action::Cancel => {
                let Some(run) = &mut self.running else {
                    self.overlay = Some(Overlay::Menu(menu));
                    return Err(io::Error::other(
                        "the command has already finished; nothing to cancel",
                    ));
                };
                run.cancel()?;
                self.notice = "Cancellation requested; task effects are not rolled back".into();
                self.menu_stack.clear();
            }
            Action::Command {
                arguments,
                confirm,
                return_to,
            } => {
                if self.running.is_some() {
                    self.overlay = Some(Overlay::Menu(menu));
                    return Err(io::Error::other(
                        "wait for the running command before changing settings",
                    ));
                }
                if let Some(prompt) = confirm {
                    self.pending_page = Some(return_to);
                    self.menu_stack.push(menu);
                    self.overlay = Some(Overlay::Confirm {
                        prompt,
                        arguments,
                        scroll: 0,
                    });
                } else {
                    self.setting_command(arguments, return_to)?;
                }
            }
            Action::Details { title, markdown } => {
                self.menu_stack.push(menu);
                self.overlay = Some(Overlay::Details {
                    title,
                    markdown,
                    scroll: 0,
                });
            }
        }
        Ok(())
    }
}
