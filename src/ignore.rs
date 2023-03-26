use anyhow::Result;
use ignore::{
    gitignore::{Gitignore, GitignoreBuilder},
    overrides::{Override, OverrideBuilder},
    Match,
};
use log::warn;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct IgnoreBuilder<'a> {
    root: &'a Path,
    use_gitignore: bool,
    ignore: &'a [String],
}

impl<'a> IgnoreBuilder<'a> {
    pub fn new(path: &'a Path) -> Self {
        IgnoreBuilder {
            root: path,
            use_gitignore: true,
            ignore: &[],
        }
    }

    #[must_use]
    pub fn ignore(mut self, globs: &'a [String]) -> Self {
        self.ignore = globs;
        self
    }

    #[must_use]
    pub fn use_gitignore(mut self, yes: bool) -> Self {
        self.use_gitignore = yes;
        self
    }

    pub fn build(self) -> Result<Ignore> {
        let mut gitignore_builder = GitignoreBuilder::new(self.root);
        let gitignore = if self.use_gitignore {
            if let Some(err) = gitignore_builder.add(self.root.join(".gitignore")) {
                // Partial errors can occur, so do not cancel the build, just warn
                warn!("problem adding gitignore file: {err}");
            }
            let (gitignore, err) = gitignore_builder.build_global();
            if let Some(err) = err {
                warn!("problem building gitignore: {err}")
            }
            Some(gitignore)
        } else {
            // Swallow errors creating gitignore: not relevant if user chooses not to use it
            gitignore_builder.build().ok()
        };
        let mut override_builder = OverrideBuilder::new(self.root);
        for pat in self.ignore {
            // ! because overrides normally act like only-inclusive ignores
            override_builder.add(&format!("!{pat}"))?;
        }
        override_builder.add("!/.git")?;
        Ok(Ignore {
            gitignore,
            overrides: override_builder.build().unwrap(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Ignore {
    gitignore: Option<Gitignore>,
    overrides: Override,
}

impl Ignore {
    pub fn is_ignored(&self, path: impl AsRef<Path>) -> bool {
        if !matches!(
            self.overrides.matched(&path, path.as_ref().is_dir()),
            Match::None
        ) {
            true
        } else {
            self.gitignore
                .as_ref()
                .map(|ignore| {
                    let matched = ignore.matched_path_or_any_parents(&path, path.as_ref().is_dir());
                    matches!(matched, Match::Ignore(_))
                })
                .unwrap_or_default()
        }
    }
}

impl Default for Ignore {
    fn default() -> Self {
        Self {
            gitignore: None,
            overrides: Override::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dir::temp_files;
    use assert_fs::prelude::{FileWriteStr, PathChild};

    #[test]
    fn ignores_nothing_with_no_gitignore() {
        let temp = temp_files!("test.txt", "test2.txt");
        let ignore = IgnoreBuilder::new(temp.path()).build().unwrap();
        assert!(!ignore.is_ignored(temp.join("test.txt")));
        assert!(!ignore.is_ignored(temp.join("test2.txt")));
    }

    #[test]
    fn uses_gitignore() {
        let temp = temp_files!("test.txt", "test2.txt");
        temp.child(".gitignore").write_str("/test2.txt").unwrap();
        let ignore = IgnoreBuilder::new(&temp).build().unwrap();
        assert!(!ignore.is_ignored("test.txt"));
        assert!(ignore.is_ignored("test2.txt"));
    }

    #[test]
    fn can_add_ignored_files() {
        let temp = temp_files!("test.txt", "test2.txt");
        let ignore = IgnoreBuilder::new(&temp)
            .ignore(&["test.txt".to_owned()])
            .build()
            .unwrap();
        assert!(ignore.is_ignored("test.txt"));
        assert!(!ignore.is_ignored("test2.txt"));
    }

    #[test]
    fn can_opt_out_of_gitignore() {
        let temp = temp_files!("test.txt", "test2.txt");
        temp.child(".gitignore").write_str("/test2.txt").unwrap();
        let ignore = IgnoreBuilder::new(&temp)
            .use_gitignore(false)
            .build()
            .unwrap();
        assert!(!ignore.is_ignored("test.txt"));
        assert!(!ignore.is_ignored("test2.txt"));
    }
}
