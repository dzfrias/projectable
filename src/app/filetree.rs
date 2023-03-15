pub use crate::dir::*;
use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};

use tui_tree_widget::{TreeItem, TreeState};

#[derive(Debug)]
pub struct Files<'a> {
    items: Vec<TreeItem<'a>>,
    dir: Dir,
}

impl<'a> Files<'a> {
    pub fn remove_file(&mut self, location: &[usize]) -> Result<Item> {
        if location.len() == 1 {
            let item = self.dir.remove_child(location[0])?;
            self.update();
            return Ok(item);
        }
        let item = if let Item::Dir(dir) = self
            .dir
            .nested_child_mut(&location[..location.len() - 1])
            .ok_or(anyhow!("could not remove file: invalid location"))?
        {
            dir.remove_child(location[location.len() - 1])?
        } else {
            bail!("could not remove file: invalid location")
        };
        self.update();
        Ok(item)
    }

    pub fn add_file(&mut self, location: &[usize], name: &str) -> Result<&File> {
        if location.len() == 1 && {
            self.dir.child(location[0]).is_none()
                || !matches!(
                    self.dir.child(location[0]).expect("should have child"),
                    Item::Dir(_)
                )
        } {
            self.dir.new_file(name)?;
        } else {
            const MESSAGE: &str = "could not add file: invalid location";

            if let Item::Dir(dir) = self
                .dir
                .nested_child_mut(location)
                .ok_or(anyhow!(MESSAGE))?
            {
                dir.new_file(name)?;
            } else {
                bail!(MESSAGE)
            };
        }
        self.update();
        let child = if let Item::Dir(dir) = self
            .dir
            .nested_child(location)
            .expect("path should be valid by by this point")
        {
            if let Item::File(file) = dir
                .iter()
                .find(|child| last_of_path(child.path()) == name)
                .expect("file should be in directory")
            {
                file
            } else {
                unreachable!("path must lead to file")
            }
        } else if location.len() == 1 && {
            self.dir.child(location[0]).is_none()
                || !matches!(
                    self.dir.child(location[0]).expect("should have child"),
                    Item::Dir(_)
                )
        } {
            if let Item::File(file) = self.dir.child(0).expect("file should be created") {
                file
            } else {
                unreachable!("path must lead to file")
            }
        } else {
            unreachable!("path cannot be a dir at this point")
        };
        Ok(child)
    }

    pub fn items(&self) -> &[TreeItem] {
        self.items.as_ref()
    }

    fn update(&mut self) {
        self.items = build_filetree(&self.dir);
    }
}

#[derive(Debug)]
pub struct Filetree<'a> {
    pub state: TreeState,
    pub files: Files<'a>,
    root_path: PathBuf,
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
            files: Files {
                items: file_tree,
                dir: tree,
            },
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        let tree = DirBuilder::new(&self.root_path).build()?;
        let file_tree = build_filetree(&tree);
        self.files = Files {
            items: file_tree,
            dir: tree,
        };
        Ok(())
    }

    pub fn first(&mut self) {
        self.state.select_first();
    }

    pub fn last(&mut self) {
        self.state.select_last(&self.files.items);
    }

    pub fn toggle(&mut self) {
        self.state.toggle_selected();
    }

    pub fn down(&mut self) {
        self.state.key_down(&self.files.items);
    }

    pub fn up(&mut self) {
        self.state.key_up(&self.files.items);
    }

    pub fn get_selected(&self) -> &Item {
        self.files
            .dir
            .nested_child(&self.state.selected())
            .expect("selected item should be in tree")
    }

    pub fn remove_file(&mut self, location: &[usize]) -> Result<Item> {
        let item = self.files.remove_file(location)?;
        // Prevents opening next selected item
        self.state.close(&self.state.selected());
        Ok(item)
    }

    pub fn add_file(&mut self, location: &[usize], name: &str) -> Result<&File> {
        self.files.add_file(location, name)
    }

    pub fn remove_selected(&mut self) -> Result<Item> {
        self.remove_file(&self.state.selected())
    }

    pub fn add_file_at_selected(&mut self, name: &str) -> Result<&File> {
        let selected = self.state.selected();
        if selected.len() == 1 {
            return self.add_file(&selected, name);
        }
        self.add_file(
            self.state
                .selected()
                .split_last()
                .expect("selected should not be empty")
                .1,
            name,
        )
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
    for item in tree {
        let tree_item = match item {
            Item::Dir(dir) => TreeItem::new(last_of_path(dir.path()), build_filetree(dir)),
            Item::File(file) => TreeItem::new_leaf(last_of_path(file.path())),
        };
        items.push(tree_item);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;

    #[test]
    fn last_of_path_only_gets_last_part() {
        let name = last_of_path("t/d/d/s/test.txt");
        assert_eq!("test.txt".to_owned(), name);
    }

    #[test]
    fn last_of_path_works_with_one_part() {
        let name = last_of_path("test.txt");
        assert_eq!("test.txt", name);
    }

    #[test]
    fn new_filetree_gets_all_files() {
        let temp = temp_files!("test/test.txt", "test/test2.txt", "test.txt");
        let filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        assert_eq!(temp.path(), filetree.root_path);
        assert_eq!(
            DirBuilder::new(temp.path()).build().unwrap(),
            filetree.files.dir
        );
        temp.close().unwrap();
    }

    #[test]
    fn new_filetree_selects_first() {
        let temp = temp_files!();
        let filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        assert_eq!(vec![0], filetree.state.selected());
        temp.close().unwrap();
    }

    #[test]
    fn refresh_gets_new_changes() {
        let temp = temp_files!("test.txt");
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        temp.child("new.txt").touch().unwrap();
        assert!(filetree.refresh().is_ok());
        assert_eq!(
            DirBuilder::new(temp.path()).build().unwrap(),
            filetree.files.dir
        );
        temp.close().unwrap();
    }

    #[test]
    fn get_selected_works_with_flat_tree() {
        let temp = temp_files!("test.txt", "test2.txt");
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        filetree.down();
        assert_eq!(
            filetree
                .files
                .dir
                .child(1)
                .expect("should have second child"),
            filetree.get_selected()
        );
        temp.close().unwrap();
    }

    #[test]
    fn get_selected_works_with_nested_item_in_tree() {
        let temp = temp_files!("test/test2.txt");
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        filetree.toggle();
        filetree.down();
        let Some(Item::Dir(dir)) = filetree.files.dir.child(0) else {
            panic!("wrong item of filetree");
        };
        assert_eq!(dir.child(0).unwrap(), filetree.get_selected());
        temp.close().unwrap();
    }

    #[test]
    fn can_remove_file_in_flat_tree() {
        let temp = temp_files!("test.txt", "test2.txt");
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        assert!(filetree.remove_file(&[0]).is_ok());
        assert_eq!(1, filetree.files.dir.iter().len());
        temp.close().unwrap();
    }

    #[test]
    fn can_remove_nested_item_in_tree() {
        let temp = temp_files!("test/test.txt");
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        assert!(filetree.remove_file(&[0, 0]).is_ok());
        let Some(Item::Dir(dir)) = filetree.files.dir.child(0) else {
            panic!("did not get dir for first child");
        };
        assert_eq!(0, dir.iter().len());
        temp.close().unwrap();
    }

    #[test]
    fn removing_item_updates_tree_automatically() {
        let temp = temp_files!("test.txt");
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        assert!(filetree.remove_file(&[0]).is_ok());
        assert_eq!(0, filetree.files.dir.iter().len());
        assert_eq!(0, filetree.files.items.len());
        temp.close().unwrap();
    }

    #[test]
    fn can_add_file_at_location() {
        let temp = temp_files!();
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        assert_eq!(
            temp.path().join("test.txt"),
            filetree
                .add_file_at_selected("test.txt")
                .expect("should be able to make file")
                .path()
        );
        temp.close().unwrap();
    }

    #[test]
    fn can_add_nested_file() {
        let temp = temp_files!("test/test.txt");
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        filetree.toggle();
        filetree.down();
        assert_eq!(
            temp.path().join("test/test2.txt"),
            filetree
                .add_file_at_selected("test2.txt")
                .expect("should be able to make file")
                .path()
        );
        temp.close().unwrap();
    }

    #[test]
    fn adding_file_adds_child_if_current_is_directoy() {
        let temp = temp_files!("test/test.txt");
        let mut filetree = Filetree::from_dir(temp.path()).expect("should be able to create");
        filetree.toggle();
        assert_eq!(
            temp.path().join("test/test2.txt"),
            filetree
                .add_file_at_selected("test2.txt")
                .expect("should be able to make file")
                .path()
        );
        temp.close().unwrap();
    }
}
