use tui::widgets::ListState;

#[derive(Debug)]
pub struct PendingPopup {
    pub operation: PendingOperations,
    pub state: ListState,
}

impl Default for PendingPopup {
    fn default() -> Self {
        let operation_type = PendingOperations::default();
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            operation: operation_type,
            state,
        }
    }
}

impl PendingPopup {
    pub fn has_work(&self) -> bool {
        self.operation != PendingOperations::NoPending
    }

    pub fn reset_work(&mut self) {
        self.operation = PendingOperations::NoPending;
        self.state = ListState::default();
        self.state.select(Some(0));
    }

    pub fn select_next(&mut self) {
        let current = self.selected();
        if current == 1 {
            return;
        }
        self.state.select(Some(current + 1));
    }

    pub fn select_prev(&mut self) {
        let current = self.selected();
        if current == 0 {
            return;
        }
        self.state.select(Some(current - 1));
    }

    pub fn selected(&mut self) -> usize {
        self.state.selected().unwrap_or_else(|| {
            self.state.select(Some(0));
            0
        })
    }
}

#[derive(Debug, PartialEq, Eq, Default)]
pub enum PendingOperations {
    DeleteFile,
    #[default]
    NoPending,
}
