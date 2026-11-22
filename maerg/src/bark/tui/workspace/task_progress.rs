use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    Result,
    context::ProjectContext,
    task_commands::{read_queue, resolve_task_spec, validate_context},
    task_queue::{Task, TaskStatus},
};

pub(super) struct TaskProgress {
    pub(super) component: String,
    directory: PathBuf,
    selected: Option<String>,
    already_active: BTreeSet<String>,
    next_read: Instant,
    pub(super) state: std::result::Result<QueueProgress, String>,
}

#[derive(PartialEq, Eq)]
pub(super) struct QueueProgress {
    pub(super) active: Option<(usize, String)>,
    pub(super) completed: usize,
    pub(super) total: usize,
}

impl TaskProgress {
    pub(super) fn new(project: &Path, component: &Path, spec: Option<&str>) -> Result<Self> {
        let component = ProjectContext {
            project_dir: project.to_path_buf(),
        }
        .resolve_component(Some(component))?;
        let context = validate_context(project, &component)?;
        let queue = read_queue(&context.component_dir)?;
        let selected = spec.and_then(|spec| resolve_task_spec(&queue.tasks, spec));
        let already_active = queue
            .tasks
            .iter()
            .filter(|task| task.status == TaskStatus::InProgress)
            .map(|task| task.id.clone())
            .collect::<BTreeSet<_>>();
        let state = Ok(summarize(
            &queue.tasks,
            selected.as_deref(),
            &already_active,
        ));
        Ok(Self {
            component: if component.as_os_str().is_empty() {
                ".".into()
            } else {
                component.display().to_string()
            },
            directory: context.component_dir,
            selected,
            already_active,
            next_read: Instant::now(),
            state,
        })
    }

    pub(super) fn refresh(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next_read {
            return false;
        }
        self.next_read = now + Duration::from_millis(500);
        let state = read_queue(&self.directory)
            .map(|queue| summarize(&queue.tasks, self.selected.as_deref(), &self.already_active))
            .map_err(|error| error.to_string());
        let changed = state != self.state;
        self.state = state;
        changed
    }
}

fn summarize(
    tasks: &[Task],
    selected: Option<&str>,
    already_active: &BTreeSet<String>,
) -> QueueProgress {
    let active = tasks
        .iter()
        .enumerate()
        .find(|(_, task)| match selected {
            Some(id) => {
                task.id == id && matches!(task.status, TaskStatus::Pending | TaskStatus::InProgress)
            }
            None => task.status == TaskStatus::InProgress && !already_active.contains(&task.id),
        })
        .map(|(index, task)| (index + 1, task.id.clone()));
    QueueProgress {
        active,
        completed: tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Completed)
            .count(),
        total: tasks.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CommitFrequency;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn task_header_uses_executing_component_and_declared_queue_progress() {
        let mut queue = crate::task_queue::parse(include_str!("../../../../TODOS.yaml")).unwrap();
        queue.tasks.truncate(3);
        for (task, (id, status)) in queue.tasks.iter_mut().zip([
            ("earlier-completed", TaskStatus::Completed),
            ("pre-existing-in-progress", TaskStatus::InProgress),
            ("actual-active-task", TaskStatus::InProgress),
        ]) {
            task.id = id.to_owned();
            task.status = status;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = super::super::Workspace::new(dir.path()).unwrap();
        *workspace.focus.lock().unwrap() = Some("transcript-focus".into());
        workspace.commit_frequency = Some(CommitFrequency::Task);
        let already_active = BTreeSet::from(["pre-existing-in-progress".into()]);
        workspace.task_progress = Some(TaskProgress {
            component: "running-component".into(),
            directory: dir.path().into(),
            selected: None,
            next_read: Instant::now(),
            state: Ok(summarize(&queue.tasks, None, &already_active)),
            already_active,
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| workspace.draw(frame).unwrap())
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert!(row(0).contains("running-component | 1/3 complete (33%) | Auto-commit: task"));
        assert!(!row(0).contains("transcript-focus"));
        assert!(row(1).contains("Task 3/3: actual-active-task"));
    }
}
