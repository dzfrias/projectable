use super::items::*;
use bitvec::prelude::*;
use std::path::Path;

#[derive(Debug)]
pub struct FileListing {
    /// The list of files. Everything is absolutely positioned
    items: Items,
    /// An absolute index to the selected item
    selected: usize,
    /// A 1:1 track of folded items. It's length should **always** be the same as `items`
    folded: BitVec,
}

// TODO: Adding/removing files
// TODO: Only include
// TODO: Hook up ignore
impl FileListing {
    pub fn new<T: AsRef<Path>>(items: &[T]) -> Self {
        let items = Items::new(items);
        let len = items.len();
        Self {
            items,
            folded: BitVec::repeat(false, len),
            selected: 0,
        }
    }

    pub fn items(&self) -> Vec<&Item> {
        self.iter().map(|(_, item)| item).collect()
    }

    pub fn len(&self) -> usize {
        // Doesn't take len of `self.items()` because the Iter<'_> doesn't allocate
        self.iter().count()
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
        Some(idx)
    }

    pub fn unfold<'a, T>(&mut self, index: T) -> Option<usize>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let idx = self.relative_to_absolute(index)?;
        self.folded.get_mut(idx)?.set(false);
        Some(idx)
    }

    pub fn toggle_fold(&mut self) {
        if self.selected_item().is_file() {
            return;
        }
        let current = self.folded[self.selected];
        self.folded
            .get_mut(self.selected)
            .expect("selected should be in folded")
            .set(!current);
    }

    pub fn is_folded<'a, T>(&self, index: T) -> Option<bool>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let idx = self.relative_to_absolute(index)?;
        Some(self.folded[idx])
    }

    pub fn selected(&self) -> usize {
        self.iter()
            .enumerate()
            .find_map(|(relative_idx, (abs_index, _))| {
                if self.selected == abs_index {
                    Some(relative_idx)
                } else {
                    None
                }
            })
            .expect("selection should be in visible items")
    }

    pub fn selected_item(&self) -> &Item {
        self.items
            .get(self.selected)
            .expect("selected should be in items")
    }

    pub fn select_next(&mut self) {
        self.select_next_n(1);
    }

    pub fn select_prev(&mut self) {
        self.select_prev_n(1);
    }

    pub fn select_next_n(&mut self, n: usize) {
        let Some(new_selected) = self.iter().skip(self.selected()).nth(n) else {
            // Set to last if the jump is over the limit
            self.selected = self.len() - 1;
            return;
        };
        self.selected = new_selected.0;
    }

    pub fn select_prev_n(&mut self, n: usize) {
        // Stop at 0 instead of overflowing
        let new = self.selected().saturating_sub(n);
        self.selected = self
            .relative_to_absolute(new)
            .expect("should be within bounds of listing");
    }

    pub fn select<'a, T>(&mut self, index: T) -> Option<&Item>
    where
        T: Into<ItemsIndex<'a>>,
    {
        let index = self.relative_to_absolute(index)?;
        self.selected = index;
        Some(
            self.items
                .get(index)
                .expect("should be within listing, checked at top of method"),
        )
    }

    pub fn select_first(&mut self) {
        self.select(0);
    }

    pub fn select_last(&mut self) {
        self.select(self.len() - 1);
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

    fn relative_to_absolute<'a, T>(&self, index: T) -> Option<usize>
    where
        T: Into<ItemsIndex<'a>>,
    {
        match index.into() {
            ItemsIndex::Number(n) => {
                if n == 0 {
                    Some(0)
                } else {
                    Some(self.iter().nth(n)?.0)
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
                // The innermost most recent fold is stored in order to determine if preceding
                // items should be visible or not
                self.current_fold = Some(item.path());
                // If the current item does not start with the old previous fold, include it. This
                // makes directories visible when they're folded.
                if !old_current
                    .map(|path| item.path().starts_with(path))
                    .unwrap_or_default()
                {
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
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
        ]);

        items.fold(1);
        assert_eq!(bitvec![0, 1, 0, 0], items.folded);
    }

    #[test]
    fn folded_dirs_are_not_included_in_items() {
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
        ]);

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
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test/test.txt",
        ]);

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
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test/test.txt",
        ]);

        items.select_next_n(100);
        assert_eq!(5, items.selected());
    }

    #[test]
    fn prev_selection_does_not_go_past_0() {
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test/test.txt",
        ]);

        items.select_prev_n(1);
        assert_eq!(0, items.selected());
    }

    #[test]
    fn nested_folds_are_concealed_by_parent_fold() {
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test/test.txt",
        ]);

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
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test/test.txt",
        ]);

        assert!(items.fold(100).is_none());
    }

    #[test]
    fn can_iterate_over_visible_items() {
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test/test.txt",
        ]);

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
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test/test.txt",
            "/root/test2/test.txt",
        ]);

        items.fold(1);
        assert_eq!(0, items.selected());
        items.select_next_n(2);
        assert_eq!(2, items.selected());
        items.select_prev_n(2);
        assert_eq!(0, items.selected());
        assert_eq!(&Item::Dir("/root/test2".into()), items.select(2).unwrap());
    }

    #[test]
    fn folding_is_relative() {
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test/test/test.txt",
            "/root/test2/test.txt",
        ]);
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
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
        ]);

        items.select_next();
        items.toggle_fold();
        assert!(items.is_folded(items.selected()).unwrap())
    }

    #[test]
    fn can_handle_multiple_folds() {
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
            "/root/test2/test.txt",
            "/root/test2/test2.txt",
        ]);

        items.select_next();
        items.toggle_fold();
        items.select_next();
        items.select_next();
        assert_eq!(5, items.selected);
    }

    #[test]
    fn cannot_fold_files() {
        let mut items = FileListing::new(&[
            "/root/test.txt",
            "/root/test/test.txt",
            "/root/test/test2.txt",
        ]);

        items.toggle_fold();
        assert!(!items.is_folded(0).unwrap());
        items.fold(0);
        assert!(!items.is_folded(0).unwrap());
    }
}