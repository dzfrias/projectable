use anyhow::{anyhow, bail, Error};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use itertools::Itertools;
use serde::{
    de::{self, Visitor},
    Deserialize,
};
use std::{
    collections::HashMap,
    env,
    fmt::{self, Display},
    path::PathBuf,
    str::FromStr,
};
use strum::Display;
use tui::style::{Color as TuiColor, Modifier as TuiModifier, Style as TuiStyle};

pub fn get_config_home() -> Option<PathBuf> {
    if let Some(config_dir) = env::var_os("PROJECTABLE_CONFIG_DIR") {
        return Some(PathBuf::from(config_dir).join("projectable"));
    }

    #[cfg(target_os = "macos")]
    let dir = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs_next::home_dir().map(|dir| dir.join(".config")))?;

    #[cfg(not(target_os = "macos"))]
    let dir = dirs_next::config_dir()?;

    Some(dir.join("projectable"))
}

pub trait Merge<Other = Self> {
    fn merge(&mut self, other: Other);
}

impl<T, U> Merge<U> for Vec<T>
where
    U: IntoIterator<Item = T>,
{
    fn merge(&mut self, other: U) {
        self.extend(other);
    }
}

macro_rules! merge {
    ($first:expr, $second:expr; $($field:ident),+) => {{
        let base = Self::default();
        $(if $second.$field != base.$field {
            $first.$field = $second.$field;
        })+
    }};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[strum(serialize_all = "snake_case")]
pub enum Action {
    Quit,
    Help,
    PreviewDown,
    PreviewUp,
    FiletreeDown,
    FiletreeUp,
    FiletreeAllUp,
    FiletreeAllDown,
    FiletreeDownThree,
    FiletreeUpThree,
    FiletreeExecCmd,
    FiletreeDelete,
    FiletreeSearch,
    FiletreeClear,
    FiletreeOpen,
    FiletreeNewFile,
    FiletreeNewDir,
    FiletreeGitFilter,
    FiletreeDiffMode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub quit: Key,
    pub help: Key,

    pub preview: PreviewConfig,
    pub filetree: FiletreeConfig,
}

impl Config {
    pub fn check_conflicts(&self) -> Vec<KeyConflict> {
        let keys = [
            (Action::Quit, &self.quit),
            (Action::Help, &self.help),
            (Action::PreviewDown, &self.preview.down_key),
            (Action::PreviewUp, &self.preview.up_key),
            (Action::FiletreeUp, &self.filetree.up),
            (Action::FiletreeDown, &self.filetree.down),
            (Action::FiletreeAllUp, &self.filetree.all_up),
            (Action::FiletreeAllDown, &self.filetree.all_down),
            (Action::FiletreeUpThree, &self.filetree.up_three),
            (Action::FiletreeDownThree, &self.filetree.down_three),
            (Action::FiletreeExecCmd, &self.filetree.exec_cmd),
            (Action::FiletreeDelete, &self.filetree.delete),
            (Action::FiletreeSearch, &self.filetree.search),
            (Action::FiletreeClear, &self.filetree.clear),
            (Action::FiletreeOpen, &self.filetree.open),
            (Action::FiletreeNewFile, &self.filetree.new_file),
            (Action::FiletreeNewDir, &self.filetree.new_dir),
            (Action::FiletreeGitFilter, &self.filetree.git_filter),
            (Action::FiletreeDiffMode, &self.filetree.diff_mode),
        ];
        let mut uses: HashMap<&Key, Vec<Action>> = HashMap::with_capacity(keys.len());

        for (name, key) in keys {
            // Update uses
            uses.entry(key)
                .and_modify(|actions| actions.push(name))
                .or_insert_with(|| vec![name]);
        }

        uses.into_iter()
            .filter_map(|(key, actions)| {
                if actions.len() == 1 {
                    return None;
                }
                Some(KeyConflict {
                    on: key,
                    conflictors: actions,
                })
            })
            .collect()
    }
}

impl Merge for Config {
    fn merge(&mut self, other: Self) {
        merge!(self, other; quit, help);
        self.preview.merge(other.preview);
        self.filetree.merge(other.filetree);
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            quit: Key::normal('q'),
            help: Key::normal('?'),

            preview: PreviewConfig::default(),
            filetree: FiletreeConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyConflict<'a> {
    on: &'a Key,
    conflictors: Vec<Action>,
}

impl Display for KeyConflict<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "key conflict on \"{}\" with associated actions: {}",
            self.on,
            self.conflictors
                .iter()
                .map(|item| format!("\"{item}\""))
                .join(", ")
        )
    }
}

impl KeyConflict<'_> {
    pub fn on(&self) -> &Key {
        self.on
    }

    pub fn conflictors(&self) -> &[Action] {
        self.conflictors.as_ref()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PreviewConfig {
    pub preview_cmd: String,
    pub git_pager: Option<String>,
    pub down_key: Key,
    pub up_key: Key,
    pub scroll_amount: u16,
    pub border_color: Style,
    pub scroll_bar_color: Style,
    pub unreached_bar_color: Style,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            preview_cmd: "cat {}".to_owned(),
            git_pager: None,
            down_key: Key::ctrl('d'),
            up_key: Key::ctrl('u'),
            scroll_amount: 10,
            border_color: Style::default(),
            scroll_bar_color: Style::default(),
            unreached_bar_color: Style::default(),
        }
    }
}

impl Merge for PreviewConfig {
    fn merge(&mut self, other: Self) {
        merge!(
            self, other;
            preview_cmd,
            git_pager,
            down_key,
            up_key,
            scroll_bar_color,
            scroll_amount,
            border_color,
            scroll_bar_color,
            unreached_bar_color
        );
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FiletreeConfig {
    pub use_git: bool,
    pub ignore: Vec<String>,
    pub use_gitignore: bool,
    pub refresh_time: u64,
    pub dirs_first: bool,

    pub selected: Style,
    pub filtered_out_message: Style,
    pub border_color: Style,
    pub added_style: Style,
    pub git_new_style: Style,
    pub git_modified_style: Style,

    pub down: Key,
    pub up: Key,
    pub all_up: Key,
    pub all_down: Key,
    pub down_three: Key,
    pub up_three: Key,
    pub exec_cmd: Key,
    pub delete: Key,
    pub search: Key,
    pub clear: Key,
    pub open: Key,
    pub new_file: Key,
    pub new_dir: Key,
    pub git_filter: Key,
    pub diff_mode: Key,
}

impl Default for FiletreeConfig {
    fn default() -> Self {
        Self {
            use_git: true,
            use_gitignore: true,
            dirs_first: true,
            ignore: Vec::new(),
            refresh_time: 1000,
            down: Key::normal('j'),
            up: Key::normal('k'),
            all_up: Key::normal('g'),
            all_down: Key::normal('G'),
            down_three: Key::ctrl('n'),
            up_three: Key::ctrl('p'),
            exec_cmd: Key::normal('e'),
            delete: Key::normal('d'),
            search: Key::normal('/'),
            clear: Key::normal('\\'),
            open: Key {
                code: KeyCode::Enter,
                mods: KeyModifiers::NONE,
            },
            new_file: Key::normal('n'),
            new_dir: Key::normal('N'),
            git_filter: Key::normal('T'),
            diff_mode: Key::normal('t'),

            selected: Style::bg(Color::Black, Color::LightGreen),
            filtered_out_message: Style::color(Color::Yellow),
            border_color: Style::default(),
            added_style: Style::color(Color::Green),
            git_new_style: Style::color(Color::Red),
            git_modified_style: Style::color(Color::Blue),
        }
    }
}

impl Merge for FiletreeConfig {
    fn merge(&mut self, other: Self) {
        self.ignore.merge(other.ignore);
        merge!(
            self, other;
            use_git,
            use_gitignore,
            dirs_first,
            refresh_time,
            down,
            up,
            all_up,
            all_down,
            down_three,
            up_three,
            exec_cmd,
            delete,
            search,
            clear,
            open,
            new_dir,
            git_filter,
            diff_mode,
            selected,
            filtered_out_message,
            border_color,
            added_style,
            git_new_style,
            git_modified_style
        );
    }
}

struct ColorVisitor;

impl<'de> Visitor<'de> for ColorVisitor {
    type Value = Color;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "an ANSI-compatible color")
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        s.parse().map_err(E::custom)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Rgb(u8, u8, u8),
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    #[default]
    White,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    Reset,
}

impl From<Color> for TuiColor {
    fn from(color: Color) -> Self {
        match color {
            Color::Black => Self::Black,
            Color::Red => Self::Red,
            Color::Green => Self::Green,
            Color::Yellow => Self::Yellow,
            Color::Blue => Self::Blue,
            Color::Magenta => Self::Magenta,
            Color::Cyan => Self::Cyan,
            Color::Reset => Self::Reset,
            Color::White => Self::White,
            Color::LightRed => Self::LightRed,
            Color::LightGreen => Self::LightGreen,
            Color::LightYellow => Self::LightYellow,
            Color::LightBlue => Self::LightBlue,
            Color::LightMagenta => Self::LightMagenta,
            Color::LightCyan => Self::LightCyan,
            Color::Rgb(r, g, b) => Self::Rgb(r, g, b),
        }
    }
}

impl FromStr for Color {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "black" => Self::Black,
            "red" => Self::Red,
            "green" => Self::Green,
            "yellow" => Self::Yellow,
            "blue" => Self::Blue,
            "magenta" => Self::Magenta,
            "cyan" => Self::Cyan,
            "white" => Self::White,
            "none" => Self::Reset,
            "lightred" => Self::LightRed,
            "lightgreen" => Self::LightGreen,
            "lightyellow" => Self::LightYellow,
            "lightblue" => Self::LightBlue,
            "lightmagenta" => Self::LightMagenta,
            "lightcyan" => Self::LightCyan,
            mut string => {
                const MESSAGE: &str = "invalid color";
                let replaced = string.replace(' ', "");
                string = replaced
                    .strip_prefix("rgb(")
                    .ok_or(anyhow!(MESSAGE))?
                    .strip_suffix(')')
                    .ok_or(anyhow!(MESSAGE))?;
                let mut rgb_vec = Vec::with_capacity(3);
                rgb_vec.extend(string.split(',').filter_map(|v| v.parse::<u8>().ok()));
                let [red, green, blue] = rgb_vec[..] else { bail!(MESSAGE) };
                Self::Rgb(red, green, blue)
            }
        })
    }
}
impl<'de> Deserialize<'de> for Color {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(ColorVisitor)
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct Style {
    pub color: Color,
    pub bg: Color,
    pub mods: Modifier,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: Color::default(),
            bg: Color::Reset,
            mods: Modifier(TuiModifier::empty()),
        }
    }
}

impl Style {
    pub fn color(color: Color) -> Self {
        Self {
            color,
            bg: Color::Reset,
            mods: Modifier(TuiModifier::empty()),
        }
    }

    pub fn bg(fg: Color, bg: Color) -> Self {
        Self {
            color: fg,
            bg,
            mods: Modifier(TuiModifier::empty()),
        }
    }
}

impl From<Style> for TuiStyle {
    fn from(style: Style) -> Self {
        TuiStyle::default()
            .fg(style.color.into())
            .bg(style.bg.into())
            .add_modifier(style.mods.0)
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct Modifier(pub TuiModifier);

impl<'de> Deserialize<'de> for Modifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ModifierVistor;

        impl<'de> Visitor<'de> for ModifierVistor {
            type Value = Modifier;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "a sequence of ANSI text modifiers")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut mods = TuiModifier::empty();
                while let Some(val) = seq.next_element::<String>()? {
                    match val.as_str() {
                        "bold" => mods |= TuiModifier::BOLD,
                        "italic" => mods |= TuiModifier::ITALIC,
                        _ => return Err(de::Error::custom("invalid modifiers")),
                    }
                }
                Ok(Modifier(mods))
            }
        }

        deserializer.deserialize_seq(ModifierVistor)
    }
}

struct KeyVisitor;

impl<'de> Visitor<'de> for KeyVisitor {
    type Value = Key;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "expecting a valid key")
    }

    fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let mut split = s.split('-').rev();
        let key = split.next().ok_or(E::custom("key cannot be empty"))?;
        let code = match key {
            "down" => KeyCode::Down,
            "up" => KeyCode::Up,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            k => {
                if k.len() > 1 || key.is_empty() {
                    return Err(E::custom("invalid key"));
                }
                KeyCode::Char(k.chars().next().expect("should have at least on char"))
            }
        };
        let mods = split.try_fold(KeyModifiers::NONE, |acc, modifier| match modifier {
            "ctrl" => Ok(acc | KeyModifiers::CONTROL),
            "alt" => Ok(acc | KeyModifiers::ALT),
            _ => Err(E::custom("invalid modifier")),
        })?;

        Ok(Key { code, mods })
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut key_parts = String::new();
        if self.mods.intersects(KeyModifiers::CONTROL) {
            key_parts.push_str("ctrl-");
        }
        if self.mods.intersects(KeyModifiers::ALT) {
            key_parts.push_str("alt-");
        }
        match self.code {
            KeyCode::Char(c) => key_parts.push(c),
            KeyCode::Up => key_parts.push_str("up"),
            KeyCode::Down => key_parts.push_str("down"),
            KeyCode::Right => key_parts.push_str("right"),
            KeyCode::Left => key_parts.push_str("left"),
            _ => unreachable!(),
        }

        write!(f, "{key_parts}")
    }
}

impl Key {
    pub fn normal(c: char) -> Self {
        Self {
            code: KeyCode::Char(c),
            mods: KeyModifiers::NONE,
        }
    }

    pub fn ctrl(c: char) -> Self {
        Self {
            code: KeyCode::Char(c),
            mods: KeyModifiers::CONTROL,
        }
    }
}

impl PartialEq<&KeyEvent> for Key {
    fn eq(&self, other: &&KeyEvent) -> bool {
        self == *other
    }
}

impl PartialEq<KeyEvent> for Key {
    fn eq(&self, other: &KeyEvent) -> bool {
        let mut mods = self.mods;
        if let KeyCode::Char(c) = self.code {
            // Uppercase characters need to have the SHIFT modifier on
            if c.is_uppercase() {
                mods |= KeyModifiers::SHIFT;
            }
        }
        other.code == self.code && other.modifiers == mods
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(KeyVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};
    use test_log::test;

    #[test]
    fn parse_rgb_from_color_string() {
        let color = "rgb(1, 2, 3)";
        assert_eq!(Color::Rgb(1, 2, 3), color.parse().unwrap());
    }

    #[test]
    fn parsing_rgb_ignores_whitespace() {
        let color = "  rgb     ( 1  , 3 , 10    )  ";
        assert_eq!(Color::Rgb(1, 3, 10), color.parse().unwrap());
    }

    #[test]
    fn get_config_home_gets_right_config_dir_on_all_platforms() {
        #[cfg(target_os = "linux")]
        let correct_path = dirs_next::home_dir().unwrap().join(".config/projectable");
        #[cfg(target_os = "windows")]
        let correct_path = dirs_next::home_dir()
            .unwrap()
            .join("AppData\\Roaming\\projectable");
        #[cfg(target_os = "macos")]
        let correct_path = dirs_next::home_dir().unwrap().join(".config/projectable");

        assert_eq!(correct_path, get_config_home().unwrap())
    }

    #[test]
    fn uses_env_var_for_config_home_if_set() {
        env::set_var("PROJECTABLE_CONFIG_DIR", "path");

        assert_eq!(
            PathBuf::from("path/projectable"),
            get_config_home().unwrap()
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn use_xdg_config_home_on_mac() {
        env::set_var("XDG_CONFIG_HOME", "path");

        assert_eq!(
            PathBuf::from("path/projectable"),
            get_config_home().unwrap()
        );
    }

    #[test]
    fn comparing_key_event_and_key_properly_recognizes_uppercase() {
        let key = Key {
            code: KeyCode::Char('D'),
            mods: KeyModifiers::empty(),
        };
        let key_event = KeyEvent {
            code: KeyCode::Char('D'),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };

        assert_eq!(key, key_event);
    }

    #[test]
    fn merge_keeps_lhs_when_rhs_is_default() {
        let mut lhs = Config::default();
        lhs.quit = Key::normal('z');
        let rhs = Config::default();
        lhs.merge(rhs);
        assert_eq!(Key::normal('z'), lhs.quit);
    }

    #[test]
    fn merge_has_rhs_take_precedence_over_lhs() {
        let mut lhs = Config::default();
        lhs.quit = Key::normal('z');
        let mut rhs = Config::default();
        rhs.quit = Key::normal('v');
        lhs.merge(rhs);
        assert_eq!(Key::normal('v'), lhs.quit);
    }

    #[test]
    fn merge_has_rhs_override_lhs_when_lhs_is_default() {
        let mut lhs = Config::default();
        let mut rhs = Config::default();
        rhs.quit = Key::normal('v');
        lhs.merge(rhs);
        assert_eq!(Key::normal('v'), lhs.quit);
    }

    #[test]
    fn merging_filetree_config_extends_ignore_vec() {
        let mut lhs = Config::default();
        lhs.filetree.ignore = vec!["test".to_owned(), "test2".to_owned()];
        let mut rhs = Config::default();
        rhs.filetree.ignore = vec!["test3".to_owned(), "test4".to_owned()];
        lhs.merge(rhs);
        assert_eq!(
            vec![
                "test".to_owned(),
                "test2".to_owned(),
                "test3".to_owned(),
                "test4".to_owned()
            ],
            lhs.filetree.ignore
        );
    }
}
