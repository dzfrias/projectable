use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc};

use crate::app::{FuzzyOperation, InputOperation, PendingOperation};

/// Single-threaded queue for events within the app
#[derive(Debug, Clone)]
pub struct Queue(Rc<RefCell<VecDeque<AppEvent>>>);

impl Queue {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(VecDeque::new())))
    }

    pub fn add(&self, event: AppEvent) {
        self.0.borrow_mut().push_back(event);
    }

    pub fn pop(&self) -> Option<AppEvent> {
        self.0.borrow_mut().pop_front()
    }

    pub fn contains(&self, event: &AppEvent) -> bool {
        self.0.borrow().contains(event)
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    OpenPopup(PendingOperation),
    OpenFile(PathBuf),
    DeleteFile(PathBuf),
    OpenInput(InputOperation),
    NewFile(PathBuf),
    NewDir(PathBuf),
    PreviewFile(PathBuf),
    RunCommand(String),
    RunCommandWithTmux(String, TmuxOpts),
    SearchFiles(Vec<PathBuf>),
    TogglePreviewMode,
    SpecialCommand(PathBuf),
    GotoFile(PathBuf),
    Mark(PathBuf),
    DeleteMark(PathBuf),
    OpenFuzzy(Vec<String>, FuzzyOperation),
    FilterFor(Vec<PathBuf>),
    StopAllCommands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TmuxOpts {
    VerticalSplit,
    HorizontalSplit,
    NewWindow,
    FloatingWindow,
}
