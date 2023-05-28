use crate::ignore::IgnoreBuilder;
use anyhow::{anyhow, bail, Context, Result};
use itertools::Itertools;
use log::{debug, trace};
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::HashMap,
    iter, mem,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemsIndex<'a> {
    Number(usize),
    Path(Cow<'a, Path>),
}

impl<'a> From<usize> for ItemsIndex<'a> {
    fn from(value: usize) -> Self {
        Self::Number(value)
    }
}

impl<'a> From<&'a Path> for ItemsIndex<'a> {
    fn from(value: &'a Path) -> Self {
        Self::Path(Cow::Borrowed(value))
    }
}

impl<'a> From<PathBuf> for ItemsIndex<'a> {
    fn from(value: PathBuf) -> Self {
        Self::Path(Cow::Owned(value))
    }
}

impl<'a> From<&'a str> for ItemsIndex<'a> {
    fn from(value: &'a str) -> Self {
        Self::Path(Cow::Borrowed(Path::new(value)))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Items {
    items: Vec<Item>,
    root: PathBuf,
}

impl Items {
    pub fn new<T: AsRef<Path>>(files: &[T]) -> Self {
        let mut root = files
            .get(0)
            .map_or(Some(Path::new("")), |path| path.as_ref().parent())
            .unwrap_or(Path::new(""))
            .to_path_buf();
        // Keeps track of directories as keys and the DIRECT children as values
        let mut items: HashMap<PathBuf, Vec<Item>> = HashMap::new();
        // This loop will fill up `items` in the form of (DIR, Vec<CHILDREN>).
        for file in files {
            let file = file.as_ref();
            if file.parent().is_none() || file.is_relative() {
                panic!("should not be given root or relative file as a item");
            }

            root = root
                .ancestors()
                .find(|ancestor| file.starts_with(ancestor))
                .expect("should have an ancestor, checked at top of loop")
                .to_path_buf();

            for dir in file.ancestors().skip(1) {
                // Remove dir from parent dir if it has been mistaken as a file
                if let Some(parent) = dir.parent().and_then(|parent| items.get_mut(parent)) {
                    parent.retain(|item| item.path() != dir);
                }
                // Put every ancestor of file in `items` as an empty slot if it does not exist
                items.entry(dir.to_path_buf()).or_default();
            }
            // Do not add if path is a directory, because all directories are keys in `items`. This
            // prevents dirs from being mistaken as files
            if items.contains_key(file) {
                continue;
            }
            // Put file in `items` under its parent
            items
                .entry(
                    file.parent()
                        .expect("item should have parent, checked at top of loop")
                        .to_path_buf(),
                )
                .or_default()
                .push(Item::File(file.to_path_buf()));
        }
        // Sort items first by directory name, then flatten into an iterator of `Item`s
        let items = items
            .into_iter()
            .map(|(dir, children)| (Item::Dir(dir), children))
            .sorted_by(|a, b| Ord::cmp(&a.0, &b.0))
            .flat_map(|pair| iter::once(pair.0).chain(pair.1))
            .filter(|item| item.path() != root && item.path().starts_with(&root))
            .collect();
        Self { items, root }
    }

    pub fn ignore(mut self, globs: &[impl AsRef<str>]) -> Result<Items> {
        let ignore = IgnoreBuilder::new(&self.root).ignore(globs).build()?;
        self.items = mem::take(&mut self.items)
            .into_iter()
            .filter(|item| !ignore.is_ignored(item.path()))
            .collect();
        Ok(self)
    }

    pub fn len(&self) -> usize {
        self.items().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn items_mut(&mut self) -> &mut [Item] {
        &mut self.items
    }

    pub fn into_items(self) -> Vec<Item> {
        self.items
    }

    pub fn get<'a, T>(&self, index: T) -> Option<&Item>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let index = self.resolve_index(index)?;
        self.items.get(index)
    }

    pub fn get_mut<'a, T>(&mut self, index: T) -> Option<&mut Item>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let index = self.resolve_index(index)?;
        self.items.get_mut(index)
    }

    pub fn remove<'a, T>(&mut self, index: T) -> Option<Item>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let index = self.resolve_index(index)?;

        if index >= self.items.len() {
            return None;
        }
        let removed = self.items.remove(index);
        debug!("removed item at index: {index}");
        if let Item::Dir(ref path) = removed {
            // Gets index of the last item that has `path` as one of its ancestors
            let end = self
                .items
                .iter()
                .skip(index)
                .position(|item| !item.path().starts_with(path))
                .unwrap_or(self.items.len() - 1);
            self.items.drain(index..end + 1);
            debug!("removed items from index: {index} to {}", end + 1);
        }
        Some(removed)
    }

    pub fn add(&mut self, item: Item) -> Result<()> {
        if self.iter().any(|i| i.path() == item.path()) {
            bail!("cannot add duplicate item");
        }

        self.items.push(item);
        self.sort().context("error while adding new item")
    }

    pub fn iter(&self) -> std::slice::Iter<Item> {
        self.into_iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<Item> {
        self.into_iter()
    }

    fn resolve_index<'a, T>(&self, index: T) -> Option<usize>
    where
        T: Into<ItemsIndex<'a>>,
    {
        match index.into() {
            ItemsIndex::Number(n) => Some(n),
            ItemsIndex::Path(path) => self.iter().position(|p| p.path() == path),
        }
    }

    fn sort(&mut self) -> Result<()> {
        // Each key is the path of a directory, and the value is all of its children
        let mut items: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        let unsorted = mem::take(&mut self.items);

        // Put all directories and their direct children into `items`
        for item in unsorted {
            match item {
                Item::Dir(path) => drop(items.entry(path).or_default()),
                Item::File(path) => items
                    .entry(
                        path.parent()
                            .ok_or(anyhow!("found item without a parent"))?
                            .to_path_buf(),
                    )
                    .or_default()
                    .push(path),
            }
        }

        // Sort directories, and then flatten so that all the children come after their parents.
        // After, remove the (almost) inevitable root that appears in the `items`. Finally,
        // deduplicate everything, turning any duplication into an error.
        self.items = items
            .into_iter()
            .map(|(dir, children)| (Item::Dir(dir), children.into_iter().map(Item::File)))
            .sorted_by(|a, b| Ord::cmp(&a.0, &b.0))
            .flat_map(|(dir, children)| iter::once(dir).chain(children))
            .filter(|item| item.path() != self.root)
            .dedup_with_count()
            .map(|(count, item)| {
                if count == 1 {
                    Ok(item)
                } else {
                    Err(anyhow!(
                        "duplicate item found: {item:?} with {count} duplicates"
                    ))
                }
            })
            .try_collect()?;
        trace!("completed sorting items");

        Ok(())
    }
}

impl IntoIterator for Items {
    type Item = Item;
    type IntoIter = <Vec<Item> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.into_items().into_iter()
    }
}

impl<'a> IntoIterator for &'a Items {
    type Item = &'a Item;
    type IntoIter = std::slice::Iter<'a, Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.items().into_iter()
    }
}

impl<'a> IntoIterator for &'a mut Items {
    type Item = &'a mut Item;
    type IntoIter = std::slice::IterMut<'a, Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.items_mut().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_build_dir_flat() {
        let files = &["/test.txt", "/test2.txt", "/test3.txt"];
        let items = Items::new(files);

        assert_eq!(
            vec![
                Item::File("/test.txt".into()),
                Item::File("/test2.txt".into()),
                Item::File("/test3.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn can_build_dir_with_directory() {
        let files = &["/test/test.txt", "/test2.txt"];
        let items = Items::new(files);
        assert_eq!(
            vec![
                Item::File("/test2.txt".into()),
                Item::Dir("/test".into()),
                Item::File("/test/test.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn can_build_dir_with_many_nested_directories() {
        let files = &["/test/test/test/test.txt", "/test2.txt"];
        let items = Items::new(files);
        assert_eq!(
            vec![
                Item::File("/test2.txt".into()),
                Item::Dir("/test".into()),
                Item::Dir("/test/test".into()),
                Item::Dir("/test/test/test".into()),
                Item::File("/test/test/test/test.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn can_build_with_multiple_files_with_same_parent() {
        let files = &["/test.txt", "/test/test.txt", "/test/test2.txt"];
        let items = Items::new(files);
        assert_eq!(
            vec![
                Item::File("/test.txt".into()),
                Item::Dir("/test".into()),
                Item::File("/test/test.txt".into()),
                Item::File("/test/test2.txt".into())
            ],
            items.items
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
            "/test.txt",
            "/test/test.txt",
            "/test2.txt",
            "/test/test2.txt",
        ];
        let items = Items::new(files);
        assert_eq!(
            vec![
                Item::File("/test.txt".into()),
                Item::File("/test2.txt".into()),
                Item::Dir("/test".into()),
                Item::File("/test/test.txt".into()),
                Item::File("/test/test2.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn dirs_when_building_are_properly_handled() {
        let files = &["/test", "/test/test.txt", "/test2.txt"];
        let items = Items::new(files);
        assert_eq!(
            vec![
                Item::File("/test2.txt".into()),
                Item::Dir("/test".into()),
                Item::File("/test/test.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn dirs_when_building_are_properly_handled_in_both_directions() {
        let files = &["/test/test.txt", "/test", "/test2.txt"];
        let items = Items::new(files);
        assert_eq!(
            vec![
                Item::File("/test2.txt".into()),
                Item::Dir("/test".into()),
                Item::File("/test/test.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn dirs_when_building_are_properly_handled_in_any_level_of_nesting() {
        let files = &["/test/test/test/test.txt", "/test", "/test2.txt"];
        let items = Items::new(files);
        assert_eq!(
            vec![
                Item::File("/test2.txt".into()),
                Item::Dir("/test".into()),
                Item::Dir("/test/test".into()),
                Item::Dir("/test/test/test".into()),
                Item::File("/test/test/test/test.txt".into())
            ],
            items.items
        );
    }

    #[test]
    fn dirs_when_building_are_properly_handled_in_any_level_of_nesting_in_both_directions() {
        let files = &["/test", "/test/test/test/test.txt", "/test2.txt"];
        let items = Items::new(files);
        assert_eq!(
            vec![
                Item::File("/test2.txt".into()),
                Item::Dir("/test".into()),
                Item::Dir("/test/test".into()),
                Item::Dir("/test/test/test".into()),
                Item::File("/test/test/test/test.txt".into())
            ],
            items.items
        );
    }

    #[test]
    #[should_panic]
    fn panic_on_building_with_root() {
        let files = &["/"];
        Items::new(files);
    }

    #[test]
    fn can_iterate_over_items() {
        let mut items = Items::new(&["/foo", "/bar", "/baz"]);
        assert_eq!(
            vec![
                &Item::File("/foo".into()),
                &Item::File("/bar".into()),
                &Item::File("/baz".into()),
            ],
            items.iter().collect_vec()
        );
        for item in items.iter_mut() {
            match item {
                Item::Dir(path) => *path = PathBuf::new(),
                Item::File(path) => *path = PathBuf::new(),
            }
        }
        assert_eq!(
            vec![
                Item::File(PathBuf::new()),
                Item::File(PathBuf::new()),
                Item::File(PathBuf::new()),
            ],
            items.into_iter().collect_vec()
        );
    }

    #[test]
    fn can_remove_files() {
        let mut items = Items::new(&["/root/test.txt", "/root/test2.txt"]);
        assert_eq!(Some(Item::File("/root/test.txt".into())), items.remove(0));
        assert_eq!(vec![Item::File("/root/test2.txt".into())], items.items);
    }

    #[test]
    fn can_remove_directories_and_deletes_all_children() {
        let mut items = Items::new(&[
            "/root/test.txt",
            "/root/test2.txt",
            "/root/test",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test3.txt",
        ]);
        assert_eq!(Some(Item::Dir("/root/test".into())), items.remove(2));
        assert_eq!(
            vec![
                Item::File("/root/test.txt".into()),
                Item::File("/root/test2.txt".into())
            ],
            items.items
        );
    }

    #[test]
    fn can_remove_directories_and_removes_until_end_if_children_are_at_end() {
        let mut items = Items::new(&[
            "/root/test.txt",
            "/root/test",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test3.txt",
        ]);
        assert_eq!(Some(Item::Dir("/root/test".into())), items.remove(1));
        assert_eq!(vec![Item::File("/root/test.txt".into()),], items.items);
    }

    #[test]
    fn can_remove_single_directory() {
        let mut items = Items::new(&["/root/test.txt", "/root/test", "/root/test/test.txt"]);
        assert_eq!(Some(Item::Dir("/root/test".into())), items.remove(1));
        assert_eq!(vec![Item::File("/root/test.txt".into()),], items.items);
    }

    #[test]
    fn can_add_item() {
        let mut items = Items::new(&["/root/test.txt"]);
        assert!(items.add(Item::File("/root/test2.txt".into())).is_ok());
        assert_eq!(
            vec![
                Item::File("/root/test.txt".into()),
                Item::File("/root/test2.txt".into())
            ],
            items.items
        )
    }

    #[test]
    fn adding_duplicate_item_does_not_add_and_returns_none() {
        let mut items = Items::new(&["/root/test.txt"]);
        assert!(items.add(Item::File("/root/test.txt".into())).is_err());
        assert_eq!(vec![Item::File("/root/test.txt".into())], items.items)
    }

    #[test]
    fn can_pass_path_into_remove() {
        let mut items = Items::new(&["/root/test.txt", "/root/test2.txt"]);
        assert!(items.remove("/root/test2.txt").is_some());
        assert_eq!(vec![Item::File("/root/test.txt".into())], items.items)
    }

    #[test]
    fn adding_items_will_resort_everything() {
        let mut items = Items::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test2/test.txt",
        ]);
        assert!(items.add(Item::File("/root/test/test2.txt".into())).is_ok());
        assert_eq!(
            vec![
                Item::File("/root/test.txt".into()),
                Item::Dir("/root/test".into()),
                Item::File("/root/test/test.txt".into()),
                Item::File("/root/test/test2.txt".into()),
                Item::Dir("/root/test2".into()),
                Item::File("/root/test2/test.txt".into()),
            ],
            items.items
        )
    }

    #[test]
    #[should_panic]
    fn new_items_panic_with_relative_path() {
        Items::new(&["relative.txt"]);
    }

    #[test]
    fn can_pass_empty_into_items() {
        Items::new::<&Path>(&[]);
    }

    #[test]
    fn can_ignore_certain_globs() {
        let items = Items::new(&["/root/test.txt", "/root/test2.txt", "/root/foo.txt"])
            .ignore(&["test*"])
            .unwrap();
        assert_eq!(vec![Item::File("/root/foo.txt".into())], items.items)
    }
}
