mod filetree;

use anyhow::Result;
use filetree::Filetree;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct App<'a> {
    pub path: PathBuf,
    pub tree: Filetree<'a>,
    pub should_quit: bool,
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

    fn refresh_tree(&mut self) {
        let res = self.tree.refresh();
        self.handle_result(res);
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
                self.refresh_tree();
            }

            // Movement
            'h' => self.on_left(),
            'j' => self.on_down(),
            'k' => self.on_up(),
            'l' => self.on_right(),
            _ => {}
        }
    }

    pub fn on_enter(&mut self) {
        self.tree.toggle();
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
}
