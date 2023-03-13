mod filetree;

use anyhow::Result;
use filetree::{Filetree, Item};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct App<'a> {
    tree: Filetree<'a>,
    path: PathBuf,
    should_quit: bool,
}

impl<'a> App<'a> {
    pub fn new(path: impl AsRef<Path>) -> Result<App<'a>> {
        let app = App {
            path: path.as_ref().to_path_buf(),
            tree: Filetree::from_dir(&path)?,
            should_quit: false,
        };

        Ok(app)
    }

    pub fn handle_result(&self, res: Result<()>) {
        // TODO: Error handling
        _ = res;
    }

    pub fn handle_key(&mut self, key: char) {
        match key {
            'q' => self.should_quit = true,

            'g' => self.tree.first(),
            'G' => self.tree.last(),
            'r' => {
                let res = self.tree.refresh();
                self.handle_result(res);
            }

            // Movement
            'h' => self.on_left(),
            'j' => self.on_down(),
            'k' => self.on_up(),
            'l' => self.on_right(),
            _ => {}
        }
    }

    pub fn activate(&mut self) -> Option<PathBuf> {
        let selected = self.tree.get_selected();
        match selected {
            Item::Dir(_) => self.tree.toggle(),
            Item::File(file) => return Some(file.path().to_path_buf()),
        }
        None
    }

    pub fn on_left(&mut self) {
        // TODO: Something here
    }

    pub fn on_up(&mut self) {
        self.tree.up();
    }

    pub fn on_right(&mut self) {
        // TODO: Something here
    }

    pub fn on_down(&mut self) {
        self.tree.down();
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn tree(&self) -> &Filetree<'a> {
        &self.tree
    }

    pub fn tree_mut(&mut self) -> &mut Filetree<'a> {
        &mut self.tree
    }
}
