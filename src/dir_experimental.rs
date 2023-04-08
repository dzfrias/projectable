#![allow(dead_code)]

use itertools::Itertools;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::HashMap,
    iter,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Item {
    File(PathBuf),
    Dir(PathBuf),
}

impl Ord for Item {
    fn cmp(&self, other: &Self) -> Ordering {
        let cow1 = self.path().as_os_str().to_string_lossy();
        let cow2 = other.path().as_os_str().to_string_lossy();
        match (cow1, cow2) {
            (Cow::Borrowed(s1), Cow::Borrowed(s2)) => human_sort::compare(s1, s2),
            (Cow::Borrowed(s1), Cow::Owned(s2)) => human_sort::compare(s1, &s2),
            (Cow::Owned(s1), Cow::Borrowed(s2)) => human_sort::compare(&s1, s2),
            (Cow::Owned(s1), Cow::Owned(s2)) => human_sort::compare(&s1, &s2),
        }
    }
}

impl PartialOrd for Item {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Item {
    pub fn path(&self) -> &Path {
        match self {
            Self::File(file) => file,
            Self::Dir(dir) => dir,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Items(Vec<Item>);

#[derive(Debug)]
pub struct ItemsBuilder<'a> {
    root: PathBuf,
    files: &'a [PathBuf],
}

impl<'a> ItemsBuilder<'a> {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            files: &[],
        }
    }

    pub fn with_files(mut self, files: &'a [PathBuf]) -> Self {
        self.files = files;
        self
    }

    pub fn build(self) -> Items {
        // Keeps track of directories as keys and the DIRECT children as values
        let mut items: HashMap<PathBuf, Vec<Item>> = HashMap::new();
        // This loop will fill up `items` in the form of (DIR, Vec<CHILDREN>).
        for file in self.files {
            let path = self.root.join(file);
            for dir in file.ancestors().skip(1).map(|path| self.root.join(path)) {
                // Remove dir from parent dir if it has been mistaken as a file
                if let Some(parent) = dir.parent().and_then(|parent| items.get_mut(parent)) {
                    parent.retain(|item| item.path() != dir);
                }
                // Put every ancestor of file in `items` as an empty slot if it does not exist
                items.entry(dir).or_default();
            }
            // Do not add if path is a directory, because all directories are keys in `items`. This
            // prevents dirs from being mistaken as files
            if items.contains_key(&path) {
                continue;
            }
            // Put file in `items` under its parent
            items
                .entry(
                    path.parent()
                        .expect("item should have parent")
                        .to_path_buf(),
                )
                .or_default()
                .push(Item::File(path));
        }
        // Sort items first by directory name, then flatten into an iterator of `Item`s
        let items = items
            .into_iter()
            .map(|(dir, children)| (Item::Dir(dir), children))
            .sorted_by(|a, b| Ord::cmp(&a.0, &b.0))
            .flat_map(|pair| iter::once(pair.0).chain(pair.1))
            .filter(|item| item.path() != self.root)
            .collect();
        Items(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_build_dir_flat() {
        let files = &["test.txt".into(), "test2.txt".into(), "test3.txt".into()];
        let items = ItemsBuilder::new(PathBuf::from("/test"))
            .with_files(files)
            .build();

        assert_eq!(
            vec![
                Item::File("/test/test.txt".into()),
                Item::File("/test/test2.txt".into()),
                Item::File("/test/test3.txt".into()),
            ],
            items.0
        );
    }

    #[test]
    fn can_build_dir_with_directory() {
        let files = &["test/test.txt".into()];
        let items = ItemsBuilder::new("/test").with_files(files).build();
        assert_eq!(
            vec![
                Item::Dir("/test/test".into()),
                Item::File("/test/test/test.txt".into()),
            ],
            items.0
        );
    }

    #[test]
    fn can_build_dir_with_many_nested_directories() {
        let files = &["test/test/test/test.txt".into()];
        let items = ItemsBuilder::new("/test").with_files(files).build();
        assert_eq!(
            vec![
                Item::Dir("/test/test".into()),
                Item::Dir("/test/test/test".into()),
                Item::Dir("/test/test/test/test".into()),
                Item::File("/test/test/test/test/test.txt".into()),
            ],
            items.0
        );
    }

    #[test]
    fn can_build_with_multiple_files_with_same_parent() {
        let files = &[
            "test.txt".into(),
            "test/test.txt".into(),
            "test/test2.txt".into(),
        ];
        let items = ItemsBuilder::new("/test").with_files(files).build();
        assert_eq!(
            vec![
                Item::File("/test/test.txt".into()),
                Item::Dir("/test/test".into()),
                Item::File("/test/test/test.txt".into()),
                Item::File("/test/test/test2.txt".into())
            ],
            items.0
        );
    }

    #[test]
    fn items_are_alphanumerically_ordered() {
        let items = &mut [
            Item::File("test2.txt".into()),
            Item::File("test1.txt".into()),
            Item::File("test99.txt".into()),
        ];
        items.sort();
        assert_eq!(
            &[
                Item::File("test1.txt".into()),
                Item::File("test2.txt".into()),
                Item::File("test99.txt".into())
            ],
            items
        );
    }

    #[test]
    fn items_are_merged_under_parent_directory() {
        let files = &[
            "test.txt".into(),
            "test/test.txt".into(),
            "test2.txt".into(),
            "test/test2.txt".into(),
        ];
        let items = ItemsBuilder::new("/test").with_files(files).build();
        assert_eq!(
            vec![
                Item::File("/test/test.txt".into()),
                Item::File("/test/test2.txt".into()),
                Item::Dir("/test/test".into()),
                Item::File("/test/test/test.txt".into()),
                Item::File("/test/test/test2.txt".into()),
            ],
            items.0
        );
    }

    #[test]
    fn dirs_when_building_are_properly_handled() {
        let files = &["test".into(), "test/test.txt".into()];
        let items = ItemsBuilder::new("/test").with_files(files).build();
        assert_eq!(
            vec![
                Item::Dir("/test/test".into()),
                Item::File("/test/test/test.txt".into())
            ],
            items.0
        );
    }

    #[test]
    fn dirs_when_building_are_properly_handled_in_both_directions() {
        let files = &["test/test.txt".into(), "test".into()];
        let items = ItemsBuilder::new("/test").with_files(files).build();
        assert_eq!(
            vec![
                Item::Dir("/test/test".into()),
                Item::File("/test/test/test.txt".into())
            ],
            items.0
        );
    }

    #[test]
    fn dirs_when_building_are_properly_handled_in_any_level_of_nesting() {
        let files = &["test/test/test/test.txt".into(), "test".into()];
        let items = ItemsBuilder::new("/root").with_files(files).build();
        assert_eq!(
            vec![
                Item::Dir("/root/test".into()),
                Item::Dir("/root/test/test".into()),
                Item::Dir("/root/test/test/test".into()),
                Item::File("/root/test/test/test/test.txt".into())
            ],
            items.0
        );
    }

    #[test]
    fn dirs_when_building_are_properly_handled_in_any_level_of_nesting_in_both_directions() {
        let files = &["test".into(), "test/test/test/test.txt".into()];
        let items = ItemsBuilder::new("/root").with_files(files).build();
        assert_eq!(
            vec![
                Item::Dir("/root/test".into()),
                Item::Dir("/root/test/test".into()),
                Item::Dir("/root/test/test/test".into()),
                Item::File("/root/test/test/test/test.txt".into())
            ],
            items.0
        );
    }
}
