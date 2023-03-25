use anyhow::{anyhow, Result};
use std::{
    fs::{self, File as FsFile},
    path::{Path, PathBuf},
    slice,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    File(File),
    Dir(Dir),
}

impl Item {
    pub fn path(&self) -> &Path {
        match self {
            Self::Dir(d) => d.path(),
            Self::File(f) => f.path(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dir {
    path: PathBuf,
    children: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DirBuilder<'a> {
    path: &'a Path,
    ignore: &'a [PathBuf],
    dirs_first: bool,
    only_include: Option<&'a [PathBuf]>,
}

impl<'a> DirBuilder<'a> {
    pub fn new(path: &'a Path) -> Self {
        DirBuilder {
            path,
            ignore: &[],
            dirs_first: false,
            only_include: None,
        }
    }

    pub fn dirs_first(mut self, dirs_first: bool) -> Self {
        self.dirs_first = dirs_first;
        self
    }

    pub fn ignore(mut self, ignore: &'a [PathBuf]) -> Self {
        self.ignore = ignore;
        self
    }

    pub fn only_include(mut self, only_include: &'a [PathBuf]) -> Self {
        self.only_include = Some(only_include);
        self
    }

    pub fn build(self) -> Result<Dir> {
        let dir = build_tree(self.path, self.ignore, self.dirs_first, self.only_include)?;
        Ok(dir)
    }
}

impl Dir {
    pub fn new_file(&mut self, name: &str) -> Result<&File> {
        // Reject if contains '/' or '\' (on windows)
        if name.contains('/') || (cfg!(windows) && name.contains('\\')) {
            panic!("invalid path name");
        }
        FsFile::create(self.path.join(name))?;
        self.children.push(Item::File(File {
            path: self.path.join(name),
        }));
        // Access from new `children` to get a reference
        if let Item::File(f) = self.children.last().unwrap() {
            Ok(f)
        } else {
            unreachable!()
        }
    }

    pub fn remove_child(&mut self, index: usize) -> Result<Item> {
        let item = self.children.remove(index);
        match &item {
            Item::File(f) => fs::remove_file(f.path())?,
            Item::Dir(dir) => fs::remove_dir_all(dir.path())?,
        }
        Ok(item)
    }

    pub fn remove(&mut self, path: impl AsRef<Path>) -> Result<Item> {
        let location = self
            .location_by_path(path)
            .ok_or(anyhow!("invalid remove target"))?;

        if location.len() == 1 {
            return Ok(self.children.remove(location[0]));
        }

        let item = if let Some((_, parent_loc)) = location.split_last() {
            let Item::Dir(parent) = self
                .nested_child_mut(parent_loc)
                .expect("parent should exist") else { unreachable!() };
            parent
                .children
                .remove(*location.last().expect("should have last item"))
        } else {
            panic!("cannot remove self");
        };
        Ok(item)
    }

    pub fn add(&mut self, path: impl AsRef<Path>) -> Result<()> {
        fn push_child(path: PathBuf, items: &mut Vec<Item>) {
            if items.iter().any(|item| item.path() == path) {
                return;
            }
            if path.is_dir() {
                items.push(Item::Dir(Dir {
                    path,
                    children: Vec::new(),
                }));
            } else {
                items.push(Item::File(File { path }));
            }
        }

        let parent = path.as_ref().parent().unwrap();
        if parent == self.path {
            push_child(path.as_ref().to_path_buf(), &mut self.children);
            return Ok(());
        }

        let location = self
            .location_by_path(parent)
            .ok_or(anyhow!("invalid remove target"))?;

        let Item::Dir(parent) = self
            .nested_child_mut(&location)
            .expect("parent should exist") else { unreachable!() };
        push_child(path.as_ref().to_path_buf(), &mut parent.children);
        Ok(())
    }

    pub fn nested_child(&self, location: &[usize]) -> Option<&Item> {
        let mut item = self.child(*location.first()?)?;
        for index in location.iter().skip(1) {
            item = if let Item::Dir(d) = item {
                d.child(*index)?
            } else {
                return None;
            };
        }
        Some(item)
    }

    pub fn nested_child_mut(&mut self, location: &[usize]) -> Option<&mut Item> {
        let mut item = self.child_mut(*location.first()?)?;
        for index in location.iter().skip(1) {
            item = if let Item::Dir(d) = item {
                d.child_mut(*index)?
            } else {
                return None;
            };
        }
        Some(item)
    }

    pub fn location_by_path(&self, path: impl AsRef<Path>) -> Option<Vec<usize>> {
        if path.as_ref() == self.path() {
            return Some(Vec::new());
        }

        let idx = self
            .children
            .iter()
            .position(|item| path.as_ref().starts_with(item.path()))?;
        let mut item = self.child(idx).expect("index should be in dir");
        let mut location = vec![idx];
        while item.path() != path.as_ref() {
            match item {
                Item::Dir(dir) => {
                    let idx = dir
                        .children
                        .iter()
                        .position(|item| path.as_ref().starts_with(item.path()))?;
                    item = dir.child(idx).expect("index should be in dir");
                    location.push(idx);
                }
                Item::File(_) => unreachable!("should not be possible"),
            }
        }

        Some(location)
    }

    pub fn child(&self, index: usize) -> Option<&Item> {
        self.children.get(index)
    }

    pub fn child_mut(&mut self, index: usize) -> Option<&mut Item> {
        self.children.get_mut(index)
    }

    pub fn iter(&self) -> slice::Iter<'_, Item> {
        self.into_iter()
    }

    pub fn iter_mut(&mut self) -> slice::IterMut<'_, Item> {
        self.into_iter()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl IntoIterator for Dir {
    type Item = Item;
    type IntoIter = <Vec<Item> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.children.into_iter()
    }
}

impl<'a> IntoIterator for &'a Dir {
    type Item = &'a Item;
    type IntoIter = slice::Iter<'a, Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.children.iter()
    }
}

impl<'a> IntoIterator for &'a mut Dir {
    type Item = &'a mut Item;
    type IntoIter = slice::IterMut<'a, Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.children.iter_mut()
    }
}

impl File {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn build_tree(
    path: impl AsRef<Path>,
    ignore: &[PathBuf],
    dirs_first: bool,
    only_include: Option<&[PathBuf]>,
) -> Result<Dir> {
    let mut children = Vec::new();
    for entry in fs::read_dir(&path)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| !ignore.contains(&entry.path()))
    {
        let path = entry.path();
        if let Some(include) = only_include {
            if !include.iter().any(|include_path| {
                include_path.ancestors().any(|p| p == path)
                    || path.ancestors().any(|p| p == include_path)
            }) {
                // Skip past entry if it's not in `only_include`
                continue;
            }
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let dir = build_tree(path, ignore, dirs_first, only_include)?;
            if dirs_first {
                children.insert(0, Item::Dir(dir));
            } else {
                children.push(Item::Dir(dir));
            }
        } else {
            children.push(Item::File(File { path }))
        }
    }
    Ok(Dir {
        path: path.as_ref().to_path_buf(),
        children,
    })
}

#[allow(unused_macros)]
/// Create temporary files and return the temp dir
macro_rules! temp_files {
    ($($name:expr),*) => {
        {
            #[allow(unused_imports)]
            use ::assert_fs::prelude::*;

            let __temp = ::assert_fs::TempDir::new().unwrap();
            $(
                __temp.child($name).touch().unwrap();
             )*
            __temp
        }
    };
}
#[allow(unused_imports)]
pub(crate) use temp_files;

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;

    #[test]
    #[ignore]
    fn can_build_new_dir() {
        let temp = temp_files!("test.txt", "test/test.txt");
        let dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        assert_eq!(
            Dir {
                path: temp.path().to_path_buf(),
                children: vec![
                    Item::Dir(Dir {
                        path: temp.path().join("test"),
                        children: vec![Item::File(File {
                            path: temp.path().join("test/test.txt")
                        })],
                    }),
                    Item::File(File {
                        path: temp.path().join("test.txt")
                    })
                ],
            },
            dir
        );

        temp.close().unwrap();
    }

    #[test]
    fn can_add_file_to_dir() {
        let temp = TempDir::new().unwrap();
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        let file = dir
            .new_file("test.txt")
            .expect("should be able to make new file");
        assert_eq!(
            &File {
                path: temp.to_path_buf().join("test.txt")
            },
            file
        );
        temp.close().unwrap();
    }

    #[test]
    #[should_panic]
    fn cannot_add_nested_file_to_dir() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("test")).unwrap();
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        dir.new_file("test/test.txt")
            .expect("should be able to make new file");
        temp.close().unwrap();
    }

    #[test]
    #[should_panic]
    #[cfg(target_os = "windows")]
    fn cannot_make_file_with_backslash_on_windows() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("test")).unwrap();
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        dir.new_file("test\\test.txt")
            .expect("should be able to make new file");
        temp.close().unwrap();
    }

    #[test]
    fn can_remove_file() {
        let temp = temp_files!("test.txt");
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        assert!(dir.remove_child(0).is_ok());
        assert!(dir.children.is_empty());
        assert!(!temp.path().join("test.txt").exists());
        temp.close().unwrap();
    }

    #[test]
    fn can_remove_dir() {
        let temp = TempDir::new().unwrap();
        let child_path = temp.path().join("test");
        fs::create_dir(&child_path).unwrap();
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        assert!(dir.remove_child(0).is_ok());
        assert!(dir.children.is_empty());
        assert!(!child_path.exists());
        temp.close().unwrap();
    }

    #[test]
    fn can_remove_dir_with_child() {
        let temp = temp_files!("test/test.txt");
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        assert!(dir.remove_child(0).is_ok());
        assert!(dir.children.is_empty());
        assert!(!temp.path().join("test/test.txt").exists());
        temp.close().unwrap();
    }

    #[test]
    fn dir_can_ignore() {
        let temp = temp_files!("test.txt", "ignore.txt");
        let dir = DirBuilder::new(temp.path())
            .ignore(&[temp.path().join("ignore.txt")])
            .build()
            .unwrap();
        assert_eq!(
            vec![Item::File(File {
                path: temp.path().join("test.txt")
            })],
            dir.children
        );
        temp.close().unwrap();
    }

    #[test]
    fn dir_can_order_directories_first() {
        let temp = temp_files!("test.txt", "test/test.txt", "test2/test.txt");
        let path = temp.path().to_owned();
        let dir = DirBuilder::new(&path)
            .dirs_first(true)
            .build()
            .expect("should be able to build dir");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert_eq!(path.join("test.txt"), dir.children.last().unwrap().path());
    }

    #[test]
    fn can_only_include_certain_files() {
        let temp = temp_files!("test.txt", "test2.txt", "test3.txt");
        let path = temp.path().to_owned();
        let dir = DirBuilder::new(temp.path())
            .only_include(&[path.join("test.txt")])
            .build()
            .expect("should be able to build dir");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert_eq!(1, dir.iter().len());
        assert_eq!(path.join("test.txt"), dir.child(0).unwrap().path());
    }

    #[test]
    fn only_including_a_dir_keeps_children() {
        let temp = temp_files!("test/test.txt", "ignore.txt", "test/test2.txt");
        let path = temp.path().to_owned();
        let dir = DirBuilder::new(temp.path())
            .only_include(&[path.join("test")])
            .build()
            .expect("should be able to build dir");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert_eq!(1, dir.iter().len());
        assert_eq!(path.join("test"), dir.child(0).unwrap().path());
        if let Item::Dir(dir) = dir.child(0).unwrap() {
            assert_eq!(2, dir.iter().len());
        } else {
            panic!("item should be a dir");
        }
    }

    #[test]
    fn only_including_nested_file_keeps_ancestors() {
        let temp = temp_files!("test/keep/keep.txt", "test.txt");
        let path = temp.path().to_owned();
        let dir = DirBuilder::new(temp.path())
            .only_include(&[path.join("test/keep/keep.txt")])
            .build()
            .expect("should be able to build dir");
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert_eq!(1, dir.iter().len());
        assert_eq!(
            path.join("test/keep/keep.txt"),
            dir.nested_child(&[0, 0, 0]).unwrap().path()
        )
    }

    #[test]
    fn can_get_location_by_path() {
        let temp = temp_files!("test/test/test.txt");
        let path = temp.path().to_owned();
        let dir = DirBuilder::new(temp.path()).build().unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert_eq!(
            Some(vec![0, 0, 0]),
            dir.location_by_path(path.join("test/test/test.txt"))
        )
    }

    #[test]
    fn location_by_path_returns_empty_when_root_path_is_given() {
        let temp = temp_files!();
        let path = temp.path().to_owned();
        let dir = DirBuilder::new(temp.path()).build().unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert_eq!(Some(Vec::new()), dir.location_by_path(path))
    }

    #[test]
    fn location_by_path_returns_dir_when_locating_dir() {
        let temp = temp_files!("test/test/test.txt");
        let path = temp.path().to_owned();
        let dir = DirBuilder::new(temp.path()).build().unwrap();
        scopeguard::guard(temp, |temp| temp.close().unwrap());

        assert_eq!(
            Some(vec![0, 0]),
            dir.location_by_path(path.join("test/test"))
        )
    }
}
