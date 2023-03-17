use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc};

use crate::app::{InputOperations, PendingOperations};

#[derive(Debug, Clone)]
pub struct Queue(Rc<RefCell<VecDeque<AppEvent>>>);

impl Queue {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(VecDeque::new())))
    }

    pub fn add(&self, event: AppEvent) {
        self.0.borrow_mut().push_front(event);
    }

    pub fn pop(&self) -> Option<AppEvent> {
        self.0.borrow_mut().pop_front()
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    OpenPopup(PendingOperations),
    OpenFile(PathBuf),
    DeleteFile(PathBuf),
    OpenInput(InputOperations),
    NewFile(PathBuf),
    NewDir(PathBuf),
}
