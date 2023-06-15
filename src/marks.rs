use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    env, fs, io,
    path::{Path, PathBuf},
};

pub fn get_marks_file() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("PROJECTABLE_DATA_DIR") {
        return Some(PathBuf::from(dir).join("projectable/marks.json"));
    }

    #[cfg(target_os = "macos")]
    let dir = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs_next::home_dir().map(|dir| dir.join(".local/share")))?;

    #[cfg(not(target_os = "macos"))]
    let dir = dirs_next::data_dir()?;

    Some(dir.join("projectable/marks.json"))
}

#[derive(Debug)]
pub struct Marks {
    project: PathBuf,
    pub marks: Vec<PathBuf>,
}

impl Default for Marks {
    fn default() -> Self {
        Self {
            project: PathBuf::new(),
            marks: Vec::new(),
        }
    }
}

impl Marks {
    pub fn from_marks_file(project: impl AsRef<Path>) -> Result<Self> {
        get_marks_file()
            .map(|path| -> Result<Marks> {
                let contents = fs::read_to_string(path).unwrap_or(String::from("{}"));
                let mut all_marks: HashMap<PathBuf, Vec<PathBuf>> =
                    serde_json::from_str(&contents)?;
                Ok(Marks {
                    project: project.as_ref().to_path_buf(),
                    marks: all_marks.remove(project.as_ref()).unwrap_or_default(),
                })
            })
            .unwrap_or(Ok(Marks {
                project: project.as_ref().to_path_buf(),
                marks: Vec::new(),
            }))
    }

    pub fn write(&self) -> Result<()> {
        let mut all_marks = get_marks_file()
            .map(|path| -> Result<HashMap<PathBuf, Vec<PathBuf>>> {
                let contents = match fs::read_to_string(&path) {
                    Ok(contents) => contents,
                    Err(err) => {
                        if err.kind() == io::ErrorKind::NotFound {
                            fs::create_dir_all(
                                path.parent().expect("marks file should have parent"),
                            )
                            .context("error creating marks dir")?;
                            return Ok(HashMap::new());
                        } else {
                            return Err(err.into());
                        }
                    }
                };
                let marks: HashMap<PathBuf, Vec<PathBuf>> = serde_json::from_str(&contents)?;
                Ok(marks)
            })
            .unwrap_or(Ok(HashMap::default()))
            .context("error writing marks file")?;
        all_marks.insert(self.project.to_path_buf(), self.marks.clone());
        let json = serde_json::to_string(&all_marks)?;
        fs::write(
            get_marks_file().expect("should not error here, would have errored earlier"),
            json,
        )
        .context("error writing marks file")?;
        Ok(())
    }

    pub fn project(&self) -> &Path {
        &self.project
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use test_log::test;

    #[test]
    #[serial]
    fn getting_marks_file_uses_custom_environment_variable() {
        env::set_var("PROJECTABLE_DATA_DIR", ".");
        assert_eq!(
            PathBuf::from("./projectable/marks.json"),
            get_marks_file().unwrap()
        );
        env::remove_var("PROJECTABLE_DATA_DIR");
    }

    #[test]
    #[serial]
    fn gets_correct_data_location() {
        #[cfg(not(target_os = "windows"))]
        let correct_path = dirs_next::home_dir()
            .unwrap()
            .join(".local/share/projectable/marks.json");
        #[cfg(target_os = "windows")]
        let correct_path = dirs_next::home_dir()
            .unwrap()
            .join("AppData\\Roaming\\projectable\\marks.json");

        assert_eq!(correct_path, get_marks_file().unwrap());
    }

    #[test]
    #[serial]
    #[cfg(target_os = "macos")]
    fn getting_correct_data_location_looks_at_xdg_data_home() {
        env::set_var("XDG_DATA_HOME", ".");
        assert_eq!(
            PathBuf::from("./projectable/marks.json"),
            get_marks_file().unwrap()
        );
        // For test sanitization. Would cause a rare failing test case if not set
        env::remove_var("XDG_DATA_HOME");
    }
}
