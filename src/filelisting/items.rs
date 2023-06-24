use anyhow::{bail, Context, Result};
use bitvec::slice::BitSlice;
use itertools::Itertools;
use log::debug;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::HashMap,
    iter,
    ops::RangeInclusive,
    path::{Component, Path, PathBuf},
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

    pub fn path_mut(&mut self) -> &mut PathBuf {
        match self {
            Self::File(file) => file,
            Self::Dir(dir) => dir,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Item::File(_))
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
            if file.as_ref().components().count() == 1
                && file.as_ref().components().next() == Some(Component::RootDir)
            {
                root = file.as_ref().to_path_buf();
                continue;
            }

            let file = file.as_ref();
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
                .entry(file.parent().unwrap_or(Path::new("")).to_path_buf())
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

    pub fn remove<'a, T>(&mut self, index: T) -> Option<RangeInclusive<usize>>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let range = self.get_children(index).ok()?;
        debug!("removed items in range: {:?}", &range);
        self.items.drain(range.clone());
        Some(range)
    }

    fn get_children<'a, T>(&self, index: T) -> Result<RangeInclusive<usize>>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let index = self.resolve_index(index).context("item not found")?;
        let item = &self.items[index];
        if let Item::Dir(path) = item {
            // Gets index of the last item that has `path` as one of its ancestors
            let end = self
                .items
                .iter()
                .enumerate()
                .skip(index)
                .find_map(|(idx, item)| {
                    if !item.path().starts_with(path) {
                        Some(idx.saturating_sub(1))
                    } else {
                        None
                    }
                })
                .unwrap_or(self.items.len() - 1);
            Ok(index..=end)
        } else {
            Ok(index..=index)
        }
    }

    pub fn add(&mut self, item: Item) -> Result<usize> {
        if self.iter().any(|i| i.path() == item.path()) {
            bail!("cannot add duplicate item");
        }

        if item.path().parent().unwrap_or(Path::new("")) == self.root() {
            self.items.insert(0, item);
            return Ok(0);
        }

        let insertion_index = self
            .items
            .iter()
            .position(|existing_item| item.path().parent().unwrap() == existing_item.path())
            .context("item's parent not found")?
            + 1;
        self.items.insert(insertion_index, item);
        Ok(insertion_index)
    }

    pub fn mv<'a, T>(
        &mut self,
        idx: T,
        new_name: impl AsRef<Path>,
    ) -> Result<(RangeInclusive<usize>, usize)>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let first_idx = self.resolve_index(idx).context("item to move not found")?;
        let first = self.items[first_idx].clone();
        let items_to_move = self.get_children(first_idx)?;

        let (rename, move_to, insertion_index) = {
            let mut rename = false;
            let mut move_to = None;
            let mut insertion_index = None;

            for (i, item) in self.items.iter().enumerate() {
                let Item::Dir(path) = item else {
                    continue;
                };

                if path == new_name.as_ref() {
                    move_to = Some(path.clone());
                    insertion_index = Some(i);
                    break;
                } else if path == new_name.as_ref().parent().unwrap() {
                    rename = true;
                    move_to = Some(path.clone());
                    insertion_index = Some(i);
                    break;
                }
            }

            if move_to.is_none() && new_name.as_ref().parent().unwrap() == self.root() {
                rename = true;
                move_to = Some(self.root().to_owned());
                insertion_index = Some(0);
            }

            (
                rename,
                move_to.context("directory to move to not found")?,
                insertion_index.unwrap(),
            )
        };

        if !rename {
            for item in &mut self.items[items_to_move.clone()] {
                let new_path =
                    move_to.join(item.path().strip_prefix(first.path().parent().unwrap())?);
                *item.path_mut() = new_path;
            }
        } else {
            *self.items[first_idx].path_mut() = new_name.as_ref().to_owned();
            for item in self.items[items_to_move.clone()].iter_mut().skip(1) {
                let new_path = new_name
                    .as_ref()
                    .join(item.path().strip_prefix(first.path())?);
                *item.path_mut() = new_path;
            }
        }

        self.items
            .as_mut_slice()
            .swap_range(items_to_move.clone(), insertion_index);

        Ok((items_to_move, insertion_index))
    }

    pub fn root(&self) -> &Path {
        &self.root
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
        self.items().iter()
    }
}

impl<'a> IntoIterator for &'a mut Items {
    type Item = &'a mut Item;
    type IntoIter = std::slice::IterMut<'a, Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.items_mut().iter_mut()
    }
}

// Defines trait for swapping items, provides `swap_range`
pub trait Swappable {
    fn len(&self) -> usize;
    fn swap(&mut self, a: usize, b: usize);

    fn swap_range(&mut self, old: RangeInclusive<usize>, new: usize) {
        let mut swap_idx = *old.start();
        let ratio = old.end() - old.start();
        while swap_idx != new {
            for i in (0..=ratio).rev() {
                if swap_idx + i + 1 == self.len() {
                    break;
                }
                self.swap(swap_idx + i, swap_idx + i + 1);
            }
            if *old.start() > new {
                swap_idx -= 1;
            } else {
                swap_idx += 1;
            }
        }
    }
}

impl Swappable for &mut BitSlice {
    fn len(&self) -> usize {
        BitSlice::len(self)
    }

    fn swap(&mut self, a: usize, b: usize) {
        BitSlice::swap(self, a, b);
    }
}

impl<T> Swappable for &mut [T] {
    fn len(&self) -> usize {
        <[T]>::len(self)
    }

    fn swap(&mut self, a: usize, b: usize) {
        <[T]>::swap(self, a, b);
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
    fn does_not_panic_building_with_root() {
        let items = Items::new(&["/", "/test.txt", "/test2.txt"]);
        assert_eq!(
            vec![
                Item::File("/test.txt".into()),
                Item::File("/test2.txt".into())
            ],
            items.items
        );
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
                Item::Dir(path) | Item::File(path) => *path = PathBuf::new(),
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
        assert_eq!(Some(0..=0), items.remove(0));
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
        assert_eq!(Some(2..=5), items.remove(2));
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
        assert_eq!(Some(1..=4), items.remove(1));
        assert_eq!(vec![Item::File("/root/test.txt".into()),], items.items);
    }

    #[test]
    fn can_remove_single_directory() {
        let mut items = Items::new(&["/root/test.txt", "/root/test", "/root/test/test.txt"]);
        assert_eq!(Some(1..=2), items.remove(1));
        assert_eq!(vec![Item::File("/root/test.txt".into()),], items.items);
    }

    #[test]
    fn can_add_item() {
        let mut items = Items::new(&["/root/test.txt"]);
        assert!(items.add(Item::File("/root/test2.txt".into())).is_ok());
        assert_eq!(
            vec![
                Item::File("/root/test2.txt".into()),
                Item::File("/root/test.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn adding_duplicate_item_does_not_add_and_is_err() {
        let mut items = Items::new(&["/root/test.txt"]);
        assert!(items.add(Item::File("/root/test.txt".into())).is_err());
        assert_eq!(vec![Item::File("/root/test.txt".into())], items.items);
    }

    #[test]
    fn can_pass_path_into_remove() {
        let mut items = Items::new(&["/root/test.txt", "/root/test2.txt"]);
        assert!(items.remove("/root/test2.txt").is_some());
        assert_eq!(vec![Item::File("/root/test.txt".into())], items.items);
    }

    #[test]
    fn adding_items_will_insert_in_correct_spot() {
        let mut items = Items::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test2/test.txt",
        ]);
        assert_eq!(
            2,
            items
                .add(Item::File("/root/test/test2.txt".into()))
                .unwrap()
        );
        assert_eq!(
            vec![
                Item::File("/root/test.txt".into()),
                Item::Dir("/root/test".into()),
                Item::File("/root/test/test2.txt".into()),
                Item::File("/root/test/test.txt".into()),
                Item::Dir("/root/test2".into()),
                Item::File("/root/test2/test.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn can_pass_empty_into_items() {
        Items::new::<&Path>(&[]);
    }

    #[test]
    fn works_with_relative_paths() {
        let items = Items::new(&["relative.txt", "test/relative.txt"]);
        assert_eq!(
            vec![
                Item::File("relative.txt".into()),
                Item::Dir("test".into()),
                Item::File("test/relative.txt".into())
            ],
            items.items
        );
    }

    #[test]
    fn adding_item_with_no_parent_throws_error() {
        let mut items = Items::new(&["/root/test.txt", "/root/testing.txt"]);
        assert!(items
            .add(Item::File("/root/testing/test.txt".into()))
            .is_err());
    }

    #[test]
    fn deleting_dir_that_comes_before_file_does_not_delete_the_file() {
        let mut items = Items::new(&["/root/test.txt"]);
        items.add(Item::Dir("/root/test".into())).unwrap();
        assert!(items.remove(0).is_some());
        assert_eq!(vec![Item::File("/root/test.txt".into())], items.items);
    }

    #[test]
    fn can_move_files() {
        let mut items = Items::new(&["/root/test.txt", "/root/test/test2.txt"]);
        assert!(items.mv(0, "/root/test").is_ok());
        assert_eq!(
            vec![
                Item::Dir("/root/test".into()),
                Item::File("/root/test/test.txt".into()),
                Item::File("/root/test/test2.txt".into())
            ],
            items.items
        );
    }

    #[test]
    fn can_move_files_and_rename() {
        let mut items = Items::new(&["/root/test.txt", "/root/test/test2.txt"]);
        assert!(items.mv(0, "/root/test/testing.txt").is_ok());
        assert_eq!(
            vec![
                Item::Dir("/root/test".into()),
                Item::File("/root/test/testing.txt".into()),
                Item::File("/root/test/test2.txt".into())
            ],
            items.items
        );
    }

    #[test]
    fn can_move_directories() {
        let mut items = Items::new(&["/root/test/test.txt", "/root/test2/test2.txt"]);
        assert!(items.mv(0, "/root/test2").is_ok());
        assert_eq!(
            vec![
                Item::Dir("/root/test2".into()),
                Item::File("/root/test2/test2.txt".into()),
                Item::Dir("/root/test2/test".into()),
                Item::File("/root/test2/test/test.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn can_move_directories_and_rename() {
        let mut items = Items::new(&[
            "/root/test/test.txt",
            "/root/test/testing.txt",
            "/root/test2/test2.txt",
        ]);
        assert!(items.mv(0, "/root/test2/testing").is_ok());
        assert_eq!(
            vec![
                Item::Dir("/root/test2".into()),
                Item::File("/root/test2/test2.txt".into()),
                Item::Dir("/root/test2/testing".into()),
                Item::File("/root/test2/testing/test.txt".into()),
                Item::File("/root/test2/testing/testing.txt".into()),
            ],
            items.items
        );
    }

    #[test]
    fn can_move_items_backwards() {
        let mut items = Items::new(&[
            "/root/test/test.txt",
            "/root/test/testing.txt",
            "/root/test2/test2.txt",
        ]);
        assert!(items.mv(4, "/root/test").is_ok());
        assert_eq!(
            vec![
                Item::Dir("/root/test".into()),
                Item::File("/root/test/test2.txt".into()),
                Item::File("/root/test/test.txt".into()),
                Item::File("/root/test/testing.txt".into()),
                Item::Dir("/root/test2".into()),
            ],
            items.items
        );
    }

    #[test]
    fn can_rename_at_root() {
        let mut items = Items::new(&["/root/test.txt"]);
        assert!(items.mv(0, "/root/test2.txt").is_ok());
        assert_eq!(vec![Item::File("/root/test2.txt".into())], items.items);
    }
}
