mod filetree;
mod pending_popup;

use anyhow::Result;
use filetree::{Filetree, Item};
pub use pending_popup::*;
use std::path::{Path, PathBuf};

pub struct App<'a> {
    tree: Filetree<'a>,
    path: PathBuf,
    should_quit: bool,
    pub pending: PendingPopup,
}

impl<'a> App<'a> {
    pub fn new(path: impl AsRef<Path>) -> Result<App<'a>> {
        let app = App {
            path: path.as_ref().to_path_buf(),
            tree: Filetree::from_dir(&path)?,
            should_quit: false,
            pending: PendingPopup::default(),
        };

        Ok(app)
    }

    pub fn handle_key(&mut self, key: char) -> Result<()> {
        if self.pending.has_work() {
            match key {
                'q' => drop(self.complete_pending(false)),
                'j' => self.pending.select_next(),
                'k' => self.pending.select_prev(),
                _ => {}
            }
            return Ok(());
        }

        match key {
            'q' => self.should_quit = true,

            'g' => self.tree.first(),
            'G' => self.tree.last(),
            'd' => self.pending.operation = PendingOperations::DeleteFile,

            // Movement
            'j' => self.on_down(),
            'k' => self.on_up(),
            _ => {}
        }
        Ok(())
    }

    pub fn on_enter(&mut self) -> Result<Option<PathBuf>> {
        if self.pending.has_work() {
            let confirmed = self.pending.selected() == 0;
            return self
                .complete_pending(confirmed)
                .expect("should have work")
                .and_then(|_| Ok(None));
        }

        match self.tree.get_selected() {
            Item::Dir(_) => self.tree.toggle(),
            Item::File(file) => return Ok(Some(file.path().to_path_buf())),
        }
        Ok(None)
    }

    pub fn on_esc(&mut self) -> Result<()> {
        if let Some(result) = self.complete_pending(false) {
            return result;
        }

        self.should_quit = true;
        Ok(())
    }

    pub fn on_up(&mut self) {
        self.tree.up();
    }

    pub fn on_down(&mut self) {
        self.tree.down();
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn tree(&self) -> &Filetree<'a> {
        &self.tree
    }

    pub fn tree_mut(&mut self) -> &mut Filetree<'a> {
        &mut self.tree
    }

    /// Returns `None` if there is no work, and `Some` if the operation is executed, with the
    /// result of the work
    fn complete_pending(&mut self, confirmed: bool) -> Option<Result<()>> {
        if self.pending.has_work() && !confirmed {
            self.pending.reset_work();
            return Some(Ok(()));
        }
        let res = match self.pending.operation {
            PendingOperations::NoPending => None,
            PendingOperations::DeleteFile => {
                Some(self.tree_mut().remove_selected().and_then(|_| Ok(())))
            }
        };
        self.pending.reset_work();
        res
    }
}
