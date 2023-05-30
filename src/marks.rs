use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, path::PathBuf};

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

#[derive(Debug, Deserialize, Default, Serialize)]
#[serde(default)]
pub struct Marks {
    pub marks: HashMap<PathBuf, Vec<PathBuf>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn getting_marks_file_uses_custom_environment_variable() {
        env::set_var("PROJECTABLE_DATA_DIR", ".");
        assert_eq!(
            PathBuf::from("./projectable/marks.json"),
            get_marks_file().unwrap()
        );
        env::remove_var("PROJECTABLE_DATA_DIR");
    }

    #[test]
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
