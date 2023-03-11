use crate::filetree::{Dir, DirBuilder, Item};
use anyhow::Result;
use std::path::{Path, PathBuf};

use tui_tree_widget::{TreeItem, TreeState};

#[derive(Debug)]
pub struct Filetree<'a> {
    pub root_path: PathBuf,
    pub state: TreeState,
    pub items: Vec<TreeItem<'a>>,
}

impl<'a> Filetree<'a> {
    pub fn from_dir(path: impl AsRef<Path>) -> Result<Self> {
        let tree = DirBuilder::new(&path).build()?;
        let file_tree = build_filetree(&tree);
        let mut state = TreeState::default();
        state.select_first();
        Ok(Filetree {
            root_path: path.as_ref().to_path_buf(),
            state,
            items: file_tree,
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        let tree = DirBuilder::new(&self.root_path).build()?;
        let file_tree = build_filetree(&tree);
        self.items = file_tree;
        Ok(())
    }

    pub fn first(&mut self) {
        self.state.select_first();
    }

    pub fn last(&mut self) {
        self.state.select_last(&self.items);
    }

    pub fn toggle(&mut self) {
        self.state.toggle_selected();
    }

    pub fn down(&mut self) {
        self.state.key_down(&self.items);
    }

    pub fn up(&mut self) {
        self.state.key_up(&self.items);
    }
}

fn last_of_path(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .iter()
        .last()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn build_filetree<'a>(tree: &Dir) -> Vec<TreeItem<'a>> {
    let mut items = Vec::new();
    for item in tree.children() {
        let tree_item = match item {
            Item::Dir(dir) => TreeItem::new(last_of_path(dir.path()), build_filetree(dir)),
            Item::File(file) => TreeItem::new_leaf(last_of_path(file.path())),
        };
        items.push(tree_item);
    }
    items
}
