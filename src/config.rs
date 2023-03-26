use anyhow::{anyhow, bail, Error};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{
    de::{self, Visitor},
    Deserialize,
};
use std::{env, fmt, path::PathBuf, str::FromStr};
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub quit: Key,
    pub help: Key,

    pub preview: PreviewConfig,
    pub filetree: FiletreeConfig,
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
            down_key: Key {
                code: KeyCode::Char('d'),
                mods: KeyModifiers::CONTROL,
            },
            up_key: Key {
                code: KeyCode::Char('u'),
                mods: KeyModifiers::CONTROL,
            },
            scroll_amount: 10,
            border_color: Style::default(),
            scroll_bar_color: Style::default(),
            unreached_bar_color: Style::default(),
        }
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

#[derive(Debug, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
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
}
