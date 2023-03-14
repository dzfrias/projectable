#![allow(dead_code)]

use std::{
    fs::{self, File as FsFile},
    path::{Path, PathBuf},
    slice,
};

use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    File(File),
    Dir(Dir),
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
pub struct DirBuilder {
    path: PathBuf,
    ignore: Vec<PathBuf>,
}

impl DirBuilder {
    pub fn new(path: impl AsRef<Path>) -> Self {
        DirBuilder {
            path: path.as_ref().to_path_buf(),
            ignore: Vec::new(),
        }
    }

    pub fn ignore(mut self, ignore: Vec<PathBuf>) -> Self {
        self.ignore = ignore;
        self
    }

    pub fn build(self) -> Result<Dir> {
        let dir = build_tree(self.path, &self.ignore)?;
        Ok(dir)
    }
}

impl Dir {
    pub fn new_file(&mut self, name: impl AsRef<Path>) -> Result<&File> {
        FsFile::create(self.path.join(&name))?;
        self.children.push(Item::File(File {
            path: self.path.join(&name),
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

fn build_tree(path: impl AsRef<Path>, ignore: &[PathBuf]) -> Result<Dir> {
    let mut children = Vec::new();
    for entry in fs::read_dir(&path)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| !ignore.contains(&entry.path()))
    {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let dir = build_tree(path, ignore)?;
            children.push(Item::Dir(dir))
        } else {
            children.push(Item::File(File { path }))
        }
    }
    Ok(Dir {
        path: path.as_ref().to_path_buf(),
        children,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::{prelude::*, TempDir};

    #[test]
    fn can_build_new_dir() {
        let temp = TempDir::new().unwrap();
        temp.child("test.txt").touch().unwrap();
        temp.child("test/test.txt").touch().unwrap();

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
    fn can_add_nested_file_to_dir() {
        let temp = TempDir::new().unwrap();
        fs::create_dir(temp.path().join("test")).unwrap();
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        let file = dir
            .new_file("test/test.txt")
            .expect("should be able to make new file");
        assert_eq!(
            &File {
                path: temp.to_path_buf().join("test/test.txt")
            },
            file
        );
        temp.close().unwrap();
    }

    #[test]
    fn can_remove_file() {
        let temp = TempDir::new().unwrap();
        let child = temp.child("test.txt");
        child.touch().unwrap();
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        assert!(dir.remove_child(0).is_ok());
        assert!(dir.children.is_empty());
        assert!(!child.exists());
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
        let temp = TempDir::new().unwrap();
        let child = temp.child("test/test.txt");
        child.touch().unwrap();
        let mut dir = DirBuilder::new(temp.path())
            .build()
            .expect("should be able to read directory");
        assert!(dir.remove_child(0).is_ok());
        assert!(dir.children.is_empty());
        assert!(!child.exists());
        temp.close().unwrap();
    }

    #[test]
    fn dir_can_ignore() {
        let temp = TempDir::new().unwrap();
        temp.child("test.txt").touch().unwrap();
        temp.child("ignore.txt").touch().unwrap();
        let dir = DirBuilder::new(temp.path())
            .ignore(vec![temp.path().join("ignore.txt")])
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
}
