use super::items::*;
use anyhow::{anyhow, Context, Result};
use bitvec::prelude::*;
use log::debug;
use std::path::Path;

#[derive(Debug, Default)]
pub struct FileListing {
    /// The list of files. Everything is absolutely positioned
    items: Items,
    /// An absolute index to the selected item
    selected: usize,
    /// A 1:1 track of folded items. It's length should **always** be the same as `items`
    folded: BitVec,
    cache: Vec<usize>,
    selected_cache: Option<usize>,
}

impl FileListing {
    pub fn new<T: AsRef<Path>>(items: &[T], dirs_first: bool) -> Self {
        let items = Items::new(items, dirs_first);
        let len = items.len();
        let mut listing = Self {
            items,
            folded: BitVec::repeat(false, len),
            selected: 0,
            cache: Vec::new(),
            selected_cache: Some(0),
        };
        listing.populate_cache();
        listing
    }

    fn populate_cache(&mut self) {
        self.cache = self.iter().map(|(abs_index, _)| abs_index).collect();
    }

    pub fn items(&self) -> Vec<&Item> {
        self.cache
            .iter()
            .map(|idx| self.items.get(*idx).unwrap())
            .collect()
    }

    pub fn all_items(&self) -> &[Item] {
        self.items.items()
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn fold<'a, T>(&mut self, index: T) -> Option<usize>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let idx = self.relative_to_absolute(index)?;
        if self
            .items
            .get(idx)
            .expect("should be in listing, checked above")
            .is_file()
        {
            return Some(idx);
        }
        self.folded.get_mut(idx)?.set(true);
        self.populate_cache();
        Some(idx)
    }

    pub fn unfold<'a, T>(&mut self, index: T) -> Option<usize>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let idx = self.relative_to_absolute(index)?;
        self.folded.get_mut(idx)?.set(false);
        self.populate_cache();
        Some(idx)
    }

    pub fn toggle_fold(&mut self) {
        if self.selected_item().map_or(true, |item| item.is_file()) {
            return;
        }
        let current = self.folded[self.selected];
        self.folded
            .get_mut(self.selected)
            .expect("selected should be in folded")
            .set(!current);
        self.populate_cache();
    }

    pub fn is_folded<'a, T>(&self, index: T) -> Option<bool>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let idx = self.relative_to_absolute(index)?;
        Some(self.folded[idx])
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected_cache
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    pub fn select_next(&mut self) {
        self.select_next_n(1);
    }

    pub fn select_prev(&mut self) {
        self.select_prev_n(1);
    }

    pub fn select_next_n(&mut self, n: usize) {
        let Some(new_selected) = self.cache.get(self.selected().unwrap_or_default() + n) else {
            // Set to last if the jump is over the limit
            self.selected = self.relative_to_absolute(self.len() - 1).unwrap_or_default();
            self.selected_cache = Some(self.len() - 1);
            return;
        };
        self.selected = *new_selected;
        self.selected_cache = Some(self.selected().unwrap_or_default() + n);
    }

    pub fn select_prev_n(&mut self, n: usize) {
        // Stop at 0 instead of overflowing
        let new = self.selected().unwrap_or_default().saturating_sub(n);
        self.selected = self
            .relative_to_absolute(new)
            .expect("should be within bounds of listing");
        self.selected_cache = Some(new);
    }

    pub fn select<'a, T>(&mut self, index: T) -> Option<&Item>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let index = index.into();
        if let ItemsIndex::Path(path) = &index {
            // Fold everything before selected
            let mut folds_changed = false;
            for index in self
                .items
                .iter()
                .enumerate()
                .take_while(|(_, item)| item.path() != path)
                .filter(|(_, item)| path.starts_with(item.path()))
                .map(|(idx, _)| idx)
            {
                self.folded.get_mut(index).unwrap().set(false);
                folds_changed = true;
            }
            if folds_changed {
                self.populate_cache();
            }
        }
        self.selected_cache = match &index {
            ItemsIndex::Number(n) => Some(*n),
            ItemsIndex::Path(p) => {
                self.iter()
                    .enumerate()
                    .find_map(|(idx, (_, item))| if item.path() == p { Some(idx) } else { None })
            }
        };
        let index = self.relative_to_absolute(index)?;
        self.selected = index;
        Some(
            self.items
                .get(index)
                .expect("should be within listing, checked at top of method"),
        )
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
        self.selected_cache = Some(0);
    }

    pub fn select_last(&mut self) {
        self.selected = self.len() - 1;
        self.selected_cache = Some(self.len() - 1);
    }

    pub fn iter(&self) -> Iter<'_> {
        Iter {
            listing: self,
            index: 0,
            current_fold: None,
        }
    }

    pub fn root(&self) -> &Path {
        self.items.root()
    }

    pub fn add(&mut self, item: Item) {
        let is_dir = !item.is_file();
        match self.items.add(item) {
            Ok(inserted_at) => self.folded.insert(inserted_at, is_dir),
            Err(err) => debug!("swallowed error: {err}"),
        }
        self.populate_cache();
    }

    pub fn remove<'a, T>(&mut self, index: T) -> Result<()>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let removed = self
            .items
            .remove(index)
            .ok_or_else(|| anyhow!("invalid remove target"))?;

        self.folded.drain(removed);
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
        self.populate_cache();

        Ok(())
    }

    pub fn mv<'a, T>(&mut self, index: T, new: impl AsRef<Path>) -> Result<()>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let (moved, idx) = self.items.mv(index, new)?;
        self.folded.as_mut_bitslice().swap_range(moved, idx);
        self.populate_cache();
        Ok(())
    }

    pub fn rename<'a, T>(&mut self, index: T, new: impl AsRef<Path>) -> Result<()>
    where
        T: Into<ItemsIndex<'a>>,
    {
        self.items.rename(index, new)
    }

    pub fn fold_all(&mut self) {
        for dir_idx in self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| !item.is_file())
            .map(|(idx, _)| idx)
        {
            self.folded
                .get_mut(dir_idx)
                .expect("folded should be same length as items")
                .set(true);
        }
        self.populate_cache();
    }

    pub fn unfold_all(&mut self) {
        if self.folded.not_any() {
            self.fold_all();
            return;
        }
        self.folded.fill(false);
        self.populate_cache();
    }

    pub fn fold_under<'a, T>(&mut self, index: T) -> Result<()>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let index = self
            .relative_to_absolute(index)
            .context("invalid fold under target")?;
        let target_item = self
            .items
            .get(index)
            .expect("should be in items, checked at top of method");

        for target_idx in self
            .items
            .iter()
            .enumerate()
            .skip(index)
            .take_while(|(_, item)| item.path().starts_with(target_item.path()))
            .filter(|(_, item)| !item.is_file())
            .map(|(idx, _)| idx)
        {
            self.folded
                .get_mut(target_idx)
                .expect("folded should have same length as items")
                .set(true);
        }
        self.populate_cache();

        Ok(())
    }

    pub fn unfold_under<'a, T>(&mut self, index: T) -> Result<()>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let index = self
            .relative_to_absolute(index)
            .context("invalid unfold under target")?;
        let target_item = self
            .items
            .get(index)
            .expect("should be in items, checked at top of method");

        let mut unfolded_one = false;
        for target_idx in self
            .items
            .iter()
            .enumerate()
            .skip(index)
            .take_while(|(_, item)| item.path().starts_with(target_item.path()))
            .filter(|(_, item)| !item.is_file())
            .map(|(idx, _)| idx)
        {
            if *self
                .folded
                .get(target_idx)
                .expect("folded should have same length as items")
            {
                unfolded_one = true;
            }
            self.folded
                .get_mut(target_idx)
                .expect("folded should have same length as items")
                .set(false);
        }
        if !unfolded_one {
            self.fold_under(target_item.path().to_path_buf())?;
        }
        self.populate_cache();

        Ok(())
    }

    fn relative_to_absolute<'a, T>(&self, index: T) -> Option<usize>
    where
        T: Into<ItemsIndex<'a>>,
    {
        match index.into() {
            ItemsIndex::Number(n) => {
                if n == 0 {
                    Some(0)
                } else {
                    self.cache.get(n).copied()
                }
            }
            ItemsIndex::Path(path) => self.iter().find_map(|(abs_index, item)| {
                if item.path() == path {
                    Some(abs_index)
                } else {
                    None
                }
            }),
        }
    }
}

impl<'a> IntoIterator for &'a FileListing {
    type Item = (usize, &'a Item);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct Iter<'a> {
    listing: &'a FileListing,
    index: usize,
    current_fold: Option<&'a Path>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = (usize, &'a Item);

    fn next(&mut self) -> Option<Self::Item> {
        // Loop until found a visible item, that is, an item that's not in a fold
        loop {
            let item = self.listing.items.get(self.index)?;
            let folded = self
                .listing
                .folded
                .get(self.index)
                .expect("folded should be same length as items");
            let original_index = self.index;
            self.index += 1;
            if *folded {
                let old_current = self.current_fold;
                // If the current item does not start with the old previous fold, include it. This
                // makes directories visible when they're folded.
                if !old_current
                    .map(|path| item.path().starts_with(path))
                    .unwrap_or_default()
                {
                    // The innermost most recent fold is stored in order to determine if preceding
                    // items should be visible or not
                    self.current_fold = Some(item.path());
                    break Some((original_index, item));
                }
                continue;
            }

            // Check if under a folded directory
            if self
                .current_fold
                .map(|path| item.path().starts_with(path))
                .unwrap_or_default()
            {
                continue;
            }

            break Some((original_index, item));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;

    #[test]
    fn can_fold_dirs() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
            ],
            false,
        );

        items.fold(1);
        assert_eq!(bitvec![0, 1, 0, 0], items.folded);
    }

    #[test]
    fn folded_dirs_are_not_included_in_items() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
            ],
            false,
        );

        items.fold(1);
        assert_eq!(
            vec![
                &Item::File("/root/test.txt".into()),
                &Item::Dir("/root/test".into())
            ],
            items.items()
        );
    }

    #[test]
    fn nested_dirs_are_folded() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test/test.txt",
            ],
            false,
        );

        items.fold(1);
        assert_eq!(
            vec![
                &Item::File("/root/test.txt".into()),
                &Item::Dir("/root/test".into())
            ],
            items.items()
        );
    }

    #[test]
    fn next_selection_does_not_go_past_length() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test/test.txt",
            ],
            false,
        );

        items.select_next_n(100);
        assert_eq!(5, items.selected().unwrap());
    }

    #[test]
    fn prev_selection_does_not_go_past_0() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test/test.txt",
            ],
            false,
        );

        items.select_prev_n(1);
        assert_eq!(0, items.selected().unwrap());
    }

    #[test]
    fn nested_folds_are_concealed_by_parent_fold() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test/test.txt",
            ],
            false,
        );

        items.fold(4);
        items.fold(1);
        assert_eq!(
            vec![
                &Item::File("/root/test.txt".into()),
                &Item::Dir("/root/test".into())
            ],
            items.items()
        );
    }

    #[test]
    fn folding_out_of_range_returns_none() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test/test.txt",
            ],
            false,
        );

        assert!(items.fold(100).is_none());
    }

    #[test]
    fn can_iterate_over_visible_items() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test/test.txt",
            ],
            false,
        );

        items.fold(1);
        let visible = items.iter().collect_vec();
        assert_eq!(
            vec![
                (0, &Item::File("/root/test.txt".into())),
                (1, &Item::Dir("/root/test".into()))
            ],
            visible
        );
    }

    #[test]
    fn selection_is_relative() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test/test.txt",
                "/root/test2/test.txt",
            ],
            false,
        );

        items.fold(1);
        assert_eq!(0, items.selected().unwrap());
        items.select_next_n(2);
        assert_eq!(2, items.selected().unwrap());
        items.select_prev_n(2);
        assert_eq!(0, items.selected().unwrap());
        assert_eq!(&Item::Dir("/root/test2".into()), items.select(2).unwrap());
    }

    #[test]
    fn folding_is_relative() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test/test.txt",
                "/root/test2/test.txt",
            ],
            false,
        );
        items.fold(1);
        items.fold(2);
        assert_eq!(
            vec![
                &Item::File("/root/test.txt".into()),
                &Item::Dir("/root/test".into()),
                &Item::Dir("/root/test2".into())
            ],
            items.items()
        );
    }

    #[test]
    fn can_toggle_fold() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
            ],
            false,
        );

        items.select_next();
        items.toggle_fold();
        assert!(items.is_folded(items.selected().unwrap()).unwrap());
    }

    #[test]
    fn can_handle_multiple_folds() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test2/test.txt",
                "/root/test2/test2.txt",
            ],
            false,
        );

        items.select_next();
        items.toggle_fold();
        items.select_next();
        items.select_next();
        assert_eq!(5, items.selected);
    }

    #[test]
    fn cannot_fold_files() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
            ],
            false,
        );

        items.toggle_fold();
        assert!(!items.is_folded(0).unwrap());
        items.fold(0);
        assert!(!items.is_folded(0).unwrap());
    }

    #[test]
    fn fold_all_folds_only_directories() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test2/test.txt",
            ],
            false,
        );

        items.fold_all();
        assert_eq!(bitvec![0, 1, 0, 0, 1, 0], items.folded);
    }

    #[test]
    fn unfold_all_unfolds_everything() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test2/test.txt",
            ],
            false,
        );

        items.fold_all();
        items.unfold_all();
        assert_eq!(bitvec![0, 0, 0, 0, 0, 0], items.folded);
    }

    #[test]
    fn adding_items_updates_folded() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
            ],
            false,
        );

        items.fold_all();
        items.add(Item::File("/root/test2.txt".into()));
        assert_eq!(bitvec![0, 0, 1, 0, 0], items.folded);
    }

    #[test]
    fn nested_dirs_are_not_visible_when_topmost_is_folded() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test/test2/test.txt",
                "/root/test/test3/test.txt",
            ],
            false,
        );

        items.fold_all();
        assert_eq!(2, items.len());
    }

    #[test]
    fn removing_from_listing_removes_from_folded() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test2.txt",
                "/root/test2/test.txt",
            ],
            false,
        );

        items.fold_all();
        assert!(items.remove(1).is_ok());
        assert_eq!(
            vec![
                &Item::File("/root/test.txt".into()),
                &Item::Dir("/root/test2".into()),
            ],
            items.items()
        );
        assert_eq!(bitvec![0, 1, 0], items.folded);
    }

    #[test]
    fn removing_with_1_item_left_doesnt_panic() {
        let mut items = FileListing::new(&["/root/test.txt"], false);
        assert!(items.remove(0).is_ok());
    }

    #[test]
    fn selected_returns_none_if_empty() {
        let items = FileListing::default();
        assert!(items.selected().is_none());
        assert!(items.selected_item().is_none());
    }

    #[test]
    fn opening_path_opens_all_nested_dirs() {
        let mut items =
            FileListing::new(&["/root/test/test/test/test.txt", "/root/test.txt"], false);

        assert!(items.select("/root/test/test/test").is_some());
        assert_eq!(3, items.selected().unwrap());
    }

    #[test]
    fn going_past_selection_with_fold_does_not_panic() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test.txt",
                "/root/test/test/test.txt",
                "/root/test/test2/test.txt",
            ],
            false,
        );

        items.fold_all();
        items.unfold(1);
        items.select_last();
        items.select_next();

        assert_eq!(4, items.selected().unwrap());
    }

    #[test]
    fn can_fold_recursively_under_certain_paths() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test/test.txt",
                "/root/test/test2/test.txt",
                "/root/test2/test.txt",
            ],
            false,
        );

        assert!(items.fold_under(1).is_ok());
        assert_eq!(bitvec![0, 1, 1, 0, 1, 0, 0, 0], items.folded);
    }

    #[test]
    fn can_unfold_recursively_under_certain_paths() {
        let mut items = FileListing::new(
            &[
                "/root/test.txt",
                "/root/test/test/test.txt",
                "/root/test/test2/test.txt",
                "/root/test2/test.txt",
            ],
            false,
        );

        items.fold_all();
        assert!(items.unfold_under(1).is_ok());
        assert_eq!(bitvec![0, 0, 0, 0, 0, 0, 1, 0], items.folded);
    }

    #[test]
    fn adding_dir_starts_it_as_folded() {
        let mut items = FileListing::new(&["/root/test.txt"], false);
        items.add(Item::Dir("/root/dir".into()));
        assert!(items.folded.first().is_some_and(|is_folded| *is_folded));
    }

    #[test]
    fn moving_items_also_moves_folds() {
        let mut items = FileListing::new(
            &[
                "/root/test/test.txt",
                "/root/test/testing.txt",
                "/root/test2/test2.txt",
            ],
            false,
        );
        items.fold(0);
        assert!(items.mv(0, "/root/test2/").is_ok());

        assert_eq!(bitvec![0, 0, 1, 0, 0], items.folded);
    }

    #[test]
    fn unfold_all_folds_all_if_everything_already_unfolded() {
        let mut items = FileListing::new(
            &[
                "/root/test/test.txt",
                "/root/test/testing.txt",
                "/root/test2/test2.txt",
            ],
            false,
        );
        items.unfold_all();
        assert_eq!(bitvec![1, 0, 0, 1, 0], items.folded);
    }
}
