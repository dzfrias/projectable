use anyhow::{Context, Result};
use ignore::{
    gitignore::{Gitignore, GitignoreBuilder},
    overrides::{Override, OverrideBuilder},
    Match,
};
use log::{debug, trace, warn};
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
            let gitignore = gitignore_builder
                .build()
                .context("failed to build gitignore")?;
            debug!("built gitignore with {} matches", gitignore.len());
            Some(gitignore)
        } else {
            None
        };
        let global_gitignore = if self.use_gitignore {
            let (gitignore, err) = gitignore_builder.build_global();
            if let Some(err) = err {
                warn!("problem making global gitignore: {err}");
            }
            debug!("built global gitignore with {} matches", gitignore.len());
            Some(gitignore)
        } else {
            None
        };
        let mut override_builder = OverrideBuilder::new(self.root);
        for pat in self.ignore {
            // ! because overrides normally act like only-inclusive ignores
            override_builder
                .add(&format!("!{pat}"))
                .with_context(|| format!("failed to add glob for: \"!{pat}\""))?;
            trace!("added {pat} to ignore");
        }
        override_builder
            .add("!/.git")
            .context("failed to add glob for: \"!/.git\"")?;
        trace!("added .git to ignore");
        let overrides = override_builder
            .build()
            .context("failed to build override ignorer")?;
        debug!("built overrides with {} matches", overrides.num_ignores());
        Ok(Ignore {
            global_gitignore,
            gitignore,
            overrides,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Ignore {
    gitignore: Option<Gitignore>,
    overrides: Override,
    global_gitignore: Option<Gitignore>,
}

impl Ignore {
    pub fn is_ignored(&self, path: impl AsRef<Path>) -> bool {
        return !matches!(
            self.overrides.matched(&path, path.as_ref().is_dir()),
            Match::None
        ) || self
            .gitignore
            .as_ref()
            .map(|ignore| {
                !ignore
                    .matched_path_or_any_parents(&path, path.as_ref().is_dir())
                    .is_none()
            })
            .unwrap_or_default()
            || self
                .global_gitignore
                .as_ref()
                .map(|ignore| {
                    !ignore
                        .matched_path_or_any_parents(&path, path.as_ref().is_dir())
                        .is_none()
                })
                .unwrap_or_default();
    }
}

impl Default for Ignore {
    fn default() -> Self {
        Self {
            gitignore: None,
            overrides: Override::empty(),
            global_gitignore: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dir::temp_files;
    use assert_fs::prelude::{FileWriteStr, PathChild};
    use test_log::test;

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
