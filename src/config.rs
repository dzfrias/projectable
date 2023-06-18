use anyhow::Error;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use globset::{Glob, GlobSet, GlobSetBuilder};
use itertools::Itertools;
use nom::{
    branch::alt,
    bytes::complete::{tag, take, take_while_m_n},
    character::complete::{digit1, multispace0},
    combinator::{map_res, opt},
    multi::{separated_list0, separated_list1},
    sequence::{delimited, tuple},
    AsChar, IResult,
};
use serde::{
    de::{self, Visitor},
    ser::SerializeSeq,
    Deserialize, Deserializer, Serialize,
};
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
    env,
    fmt::{self, Display},
    path::{Path, PathBuf},
    str::FromStr,
};
use strum::Display;
use tui::style::{Color as TuiColor, Modifier as TuiModifier, Style as TuiStyle};

pub fn get_config_home() -> Option<PathBuf> {
    if let Some(config_dir) = env::var_os("PROJECTABLE_CONFIG_DIR") {
        return Some(PathBuf::from(config_dir));
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

impl<T, U, E> Merge<U> for E
where
    U: IntoIterator<Item = T>,
    E: Extend<T>,
{
    fn merge(&mut self, other: U) {
        self.extend(other);
    }
}

/// Merge two structs against their default. As long as the right-hand merge is not the default,
/// it replaces the left-hande merge.
macro_rules! merge {
    ($first:expr, $second:expr; $($field:ident),+) => {{
        let base = Self::default();
        $(if $second.$field != base.$field {
            $first.$field = $second.$field;
        })+
    }};
}

/// Every possible key action that can be pressed and is not part of a popup
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display)]
#[strum(serialize_all = "snake_case")]
pub enum Action<'a> {
    Quit,
    Help,
    PreviewDown,
    PreviewUp,
    Down,
    Up,
    AllUp,
    AllDown,
    Open,
    OpenMarks,
    FiletreeDownThree,
    FiletreeUpThree,
    FiletreeExecCmd,
    FiletreeDelete,
    FiletreeSearch,
    FiletreeClear,
    FiletreeNewFile,
    FiletreeNewDir,
    FiletreeGitFilter,
    FiletreeDiffMode,
    FiletreeSpecialCommand,
    FiletreeMarkSelected,
    FiletreeCloseUnder,
    FiletreeOpenUnder,
    FiletreeShowDotfiles,
    KillProcesses,
    Arbitrary(&'a str),
}

#[derive(Debug, Clone)]
pub struct GlobList(GlobSet);

impl GlobList {
    pub fn is_match<P: AsRef<Path>>(&self, path: P) -> bool {
        self.0.is_match(path)
    }
}

impl Default for GlobList {
    fn default() -> Self {
        Self(
            GlobSetBuilder::new()
                .add(Glob::new("**/.git").expect("should be valid pattern"))
                .build()
                .expect("should build static globset correctly"),
        )
    }
}

impl<'de> Deserialize<'de> for GlobList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct GlobListVisitor;

        impl<'de> Visitor<'de> for GlobListVisitor {
            type Value = GlobList;

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut set = GlobSetBuilder::new();
                while let Some(pat) = seq.next_element::<String>()? {
                    // Add **/ before for ease of use
                    set.add(Glob::new(&format!("**/{pat}")).map_err(de::Error::custom)?);
                }

                Ok(GlobList(set.build().map_err(de::Error::custom)?))
            }

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "a sequence of glob patterns")
            }
        }

        deserializer.deserialize_seq(GlobListVisitor)
    }
}

impl Serialize for GlobList {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(1))?;
        seq.serialize_element("**/.git")?;
        seq.end()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub quit: Key,
    pub help: Key,
    pub down: Key,
    pub up: Key,
    pub all_down: Key,
    pub all_up: Key,
    pub open: Key,
    pub kill_processes: Key,
    pub special_commands: HashMap<String, Vec<String>>,
    pub commands: HashMap<Key, String>,
    pub project_roots: GlobList,

    pub selected: Style,
    pub popup_border_style: Style,
    pub help_key_style: Style,

    pub preview: PreviewConfig,
    pub filetree: FiletreeConfig,
    pub log: LogConfig,
    pub marks: MarksConfig,
}

impl Config {
    pub fn check_conflicts(&self) -> Vec<KeyConflict> {
        let mut keys = vec![
            (Action::Quit, &self.quit),
            (Action::Help, &self.help),
            (Action::Down, &self.down),
            (Action::Open, &self.open),
            (Action::Up, &self.up),
            (Action::AllDown, &self.all_down),
            (Action::AllUp, &self.all_up),
            (Action::PreviewDown, &self.preview.down_key),
            (Action::PreviewUp, &self.preview.up_key),
            (Action::FiletreeUpThree, &self.filetree.up_three),
            (Action::FiletreeDownThree, &self.filetree.down_three),
            (Action::FiletreeExecCmd, &self.filetree.exec_cmd),
            (Action::FiletreeDelete, &self.filetree.delete),
            (Action::FiletreeSearch, &self.filetree.search),
            (Action::FiletreeClear, &self.filetree.clear),
            (Action::FiletreeNewFile, &self.filetree.new_file),
            (Action::FiletreeNewDir, &self.filetree.new_dir),
            (Action::FiletreeGitFilter, &self.filetree.git_filter),
            (Action::FiletreeDiffMode, &self.filetree.diff_mode),
            (
                Action::FiletreeSpecialCommand,
                &self.filetree.special_command,
            ),
            (Action::FiletreeMarkSelected, &self.filetree.mark_selected),
            (Action::OpenMarks, &self.marks.open),
            (Action::FiletreeOpenUnder, &self.filetree.open_under),
            (Action::FiletreeCloseUnder, &self.filetree.close_under),
            (Action::FiletreeShowDotfiles, &self.filetree.show_dotfiles),
            (Action::KillProcesses, &self.kill_processes),
        ];
        // Put custom key binds actions
        keys.extend(
            self.commands
                .iter()
                .map(|(key, cmd)| (Action::Arbitrary(cmd), key)),
        );
        let mut uses: HashMap<&Key, Vec<Action>> = HashMap::with_capacity(keys.len());

        for (name, key) in keys {
            match uses.entry(key) {
                Entry::Occupied(mut actions) => actions.get_mut().push(name),
                Entry::Vacant(slot) => drop(slot.insert(vec![name])),
            }
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
        merge!(
            self, other;
            quit,
            help,
            down,
            up,
            all_down,
            all_up,
            open,
            selected,
            popup_border_style,
            help_key_style,
            kill_processes,
            commands
        );
        self.special_commands.merge(other.special_commands);
        self.preview.merge(other.preview);
        self.filetree.merge(other.filetree);
        self.log.merge(other.log);
        self.marks.merge(other.marks);
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            quit: Key::normal('q'),
            help: Key::normal('?'),
            down: Key::normal('j'),
            up: Key::normal('k'),
            open: Key::key_code(KeyCode::Enter),
            all_up: Key::normal('g'),
            all_down: Key::normal('G'),
            kill_processes: Key::ctrl('c'),
            special_commands: HashMap::new(),
            selected: Style::bg(Color::Black, Color::Magenta),
            popup_border_style: Style::default(),
            help_key_style: Style {
                color: Color::LightCyan,
                bg: Color::Reset,
                mods: Modifier(TuiModifier::BOLD),
            },
            commands: HashMap::new(),
            project_roots: GlobList::default(),

            preview: PreviewConfig::default(),
            filetree: FiletreeConfig::default(),
            log: LogConfig::default(),
            marks: MarksConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyConflict<'a> {
    on: &'a Key,
    conflictors: Vec<Action<'a>>,
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

#[derive(Debug, Deserialize, Serialize)]
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
            #[cfg(target_os = "windows")]
            preview_cmd: "type {}".to_owned(),
            #[cfg(not(target_os = "windows"))]
            preview_cmd: "cat {}".to_owned(),

            git_pager: None,
            down_key: Key::ctrl('d'),
            up_key: Key::ctrl('u'),
            scroll_amount: 10,
            border_color: Style::color(Color::Cyan),
            scroll_bar_color: Style::color(Color::Magenta),
            unreached_bar_color: Style::color(Color::Blue),
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct FiletreeConfig {
    pub use_git: bool,
    pub ignore: Vec<String>,
    pub use_gitignore: bool,
    pub refresh_time: u64,
    // TODO: Actually implement
    pub dirs_first: bool,
    pub show_hidden_by_default: bool,

    pub filtered_out_message: Style,
    pub border_color: Style,
    // TODO: Actually implement
    pub added_style: Style,
    pub git_new_style: Style,
    pub git_modified_style: Style,
    pub marks_style: Style,
    pub dir_style: Style,

    pub special_command: Key,
    pub down_three: Key,
    pub up_three: Key,
    pub exec_cmd: Key,
    pub delete: Key,
    pub search: Key,
    pub clear: Key,
    pub new_file: Key,
    pub new_dir: Key,
    pub git_filter: Key,
    pub diff_mode: Key,
    pub open_all: Key,
    pub close_all: Key,
    pub mark_selected: Key,
    pub open_under: Key,
    pub close_under: Key,
    pub show_dotfiles: Key,
}

impl Default for FiletreeConfig {
    fn default() -> Self {
        Self {
            use_git: true,
            use_gitignore: true,
            dirs_first: true,
            show_hidden_by_default: false,
            ignore: Vec::new(),
            refresh_time: 1000,
            down_three: Key::ctrl('n'),
            up_three: Key::ctrl('p'),
            exec_cmd: Key::normal('e'),
            delete: Key::normal('d'),
            search: Key::normal('/'),
            clear: Key::normal('\\'),
            open_all: Key::normal('o'),
            close_all: Key::normal('O'),
            new_file: Key::normal('n'),
            new_dir: Key::normal('N'),
            git_filter: Key::normal('T'),
            diff_mode: Key::normal('t'),
            special_command: Key::normal('v'),
            mark_selected: Key::normal('m'),
            open_under: Key::normal('l'),
            close_under: Key::normal('h'),
            show_dotfiles: Key::normal('.'),

            filtered_out_message: Style::color(Color::Yellow),
            border_color: Style::color(Color::Magenta),
            added_style: Style::color(Color::Green),
            git_new_style: Style::color(Color::Red),
            git_modified_style: Style::color(Color::Cyan),
            marks_style: Style::color(Color::Yellow),
            dir_style: Style {
                color: Color::Blue,
                bg: Color::Reset,
                mods: Modifier(TuiModifier::ITALIC),
            },
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
            down_three,
            up_three,
            exec_cmd,
            delete,
            search,
            clear,
            new_dir,
            git_filter,
            diff_mode,
            filtered_out_message,
            border_color,
            added_style,
            git_new_style,
            git_modified_style,
            special_command,
            mark_selected,
            marks_style,
            open_under,
            close_under,
            show_dotfiles,
            show_hidden_by_default,
            dir_style
        );
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogConfig {
    pub error: Style,
    pub debug: Style,
    pub warn: Style,
    pub trace: Style,
    pub info: Style,
    pub border_color: Style,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            error: Style::color(Color::Red),
            debug: Style::color(Color::Green),
            warn: Style::color(Color::Yellow),
            trace: Style::color(Color::Magenta),
            info: Style::default(),
            border_color: Style::color(Color::Blue),
        }
    }
}

impl Merge for LogConfig {
    fn merge(&mut self, other: Self) {
        merge!(self, other; error, debug, warn, trace, info, border_color);
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct MarksConfig {
    pub marks_dir: Option<PathBuf>,
    pub relative: bool,

    pub open: Key,
    pub delete: Key,
    pub mark_style: Style,
}

impl Default for MarksConfig {
    fn default() -> Self {
        Self {
            marks_dir: None,
            relative: true,
            open: Key::normal('M'),
            delete: Key::normal('d'),
            mark_style: Style::default(),
        }
    }
}

impl Merge for MarksConfig {
    fn merge(&mut self, other: Self) {
        merge!(
            self, other;
            marks_dir,
            relative,
            open,
            delete,
            mark_style
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
            _ => {
                fn hex_primary(input: &str) -> IResult<&str, u8> {
                    map_res(take_while_m_n(2, 2, |c: char| c.is_hex_digit()), |input| {
                        u8::from_str_radix(input, 16)
                    })(input)
                }
                fn hex_color(input: &str) -> IResult<&str, Color> {
                    let (input, _) = tag("#")(input)?;
                    let (input, (red, green, blue)) =
                        tuple((hex_primary, hex_primary, hex_primary))(input)?;

                    Ok((input, Color::Rgb(red, green, blue)))
                }
                fn u8_digit(input: &str) -> IResult<&str, u8> {
                    map_res(digit1, |s: &str| s.parse())(input)
                }
                fn rgb_color(input: &str) -> IResult<&str, Color> {
                    let (input, _) = delimited(multispace0, tag("rgb"), multispace0)(input)?;
                    let (input, _) = delimited(multispace0, tag("("), multispace0)(input)?;
                    let (input, digits) = separated_list1(
                        delimited(multispace0, tag(","), multispace0),
                        u8_digit,
                    )(input)?;
                    let (input, _) = delimited(multispace0, tag(")"), multispace0)(input)?;

                    let [r, g, b] = digits[..] else {
                        return Err(nom::Err::Error(nom::error::Error { input, code: nom::error::ErrorKind::SeparatedList }));
                    };

                    Ok((input, Color::Rgb(r, g, b)))
                }

                // Turn into owned string so it is 'static (for anyhow)
                let (_, color) = alt((hex_color, rgb_color))(s).map_err(|err| err.to_owned())?;
                color
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

impl Serialize for Color {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            Self::Black => Cow::Borrowed("black"),
            Self::Red => Cow::Borrowed("red"),
            Self::Green => Cow::Borrowed("green"),
            Self::Yellow => Cow::Borrowed("yellow"),
            Self::Blue => Cow::Borrowed("blue"),
            Self::Magenta => Cow::Borrowed("magenta"),
            Self::Cyan => Cow::Borrowed("cyan"),
            Self::White => Cow::Borrowed("white"),
            Self::Reset => Cow::Borrowed("none"),
            Self::LightRed => Cow::Borrowed("lightred"),
            Self::LightGreen => Cow::Borrowed("lightgreen"),
            Self::LightYellow => Cow::Borrowed("lightyellow"),
            Self::LightBlue => Cow::Borrowed("lightblue"),
            Self::LightMagenta => Cow::Borrowed("lightmagenta"),
            Self::LightCyan => Cow::Borrowed("lightcyan"),
            Self::Rgb(r, g, b) => Cow::Owned(format!("rgb({r}, {g}, {b})")),
        };

        serializer.serialize_str(&s)
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy, Serialize)]
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

impl Serialize for Modifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(2))?;
        if self.0 & TuiModifier::BOLD == TuiModifier::BOLD {
            seq.serialize_element("bold")?;
        }
        if self.0 & TuiModifier::ITALIC == TuiModifier::ITALIC {
            seq.serialize_element("italic")?;
        }
        seq.end()
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
        s.parse::<Key>().map_err(|err| E::custom(err.to_string()))
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl FromStr for Key {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        fn parse_keycode(input: &str) -> IResult<&str, KeyCode> {
            let (input, key) = alt((
                tag("down"),
                tag("up"),
                tag("left"),
                tag("right"),
                tag("enter"),
                tag("backspace"),
                tag("tab"),
                tag("backtab"),
                take(1usize),
            ))(input)?;

            let code = match key {
                "down" => KeyCode::Down,
                "up" => KeyCode::Up,
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                "enter" => KeyCode::Enter,
                "backspace" => KeyCode::Backspace,
                "tab" => KeyCode::Tab,
                "backtab" => KeyCode::BackTab,
                k if k.len() == 1 => {
                    KeyCode::Char(k.chars().next().expect("checked in match guard"))
                }
                _ => unreachable!("checked in alt combinator"),
            };

            Ok((input, code))
        }
        fn parse_mods(input: &str) -> IResult<&str, KeyModifiers> {
            let (input, mods_str) =
                separated_list0(tag("-"), alt((tag("ctrl"), tag("alt"))))(input)?;
            let (input, _) = opt(tag("-"))(input)?;

            let mods =
                mods_str
                    .into_iter()
                    .fold(KeyModifiers::NONE, |acc, modifier| match modifier {
                        "ctrl" => acc | KeyModifiers::CONTROL,
                        "alt" => acc | KeyModifiers::ALT,
                        _ => unreachable!(),
                    });

            Ok((input, mods))
        }

        let (input, mods) = parse_mods(s).map_err(|err| err.to_owned())?;
        let (_, code) = parse_keycode(input).map_err(|err| err.to_owned())?;

        Ok(Key { code, mods })
    }
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
            KeyCode::Enter => key_parts.push_str("enter"),
            KeyCode::Backspace => key_parts.push_str("backspace"),
            KeyCode::Tab => key_parts.push_str("tab"),
            KeyCode::BackTab => key_parts.push_str("backtab"),
            _ => panic!("key conversion not set for: \"{:?}\"", self.code),
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

    pub fn esc() -> Self {
        Self {
            code: KeyCode::Esc,
            mods: KeyModifiers::empty(),
        }
    }

    pub fn key_code(code: KeyCode) -> Self {
        Self {
            code,
            mods: KeyModifiers::NONE,
        }
    }
}

impl From<&KeyEvent> for Key {
    fn from(value: &KeyEvent) -> Self {
        Self {
            code: value.code,
            mods: value.modifiers,
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

impl Serialize for Key {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = self.to_string();
        serializer.serialize_str(&s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use collect_all::collect;
    use crossterm::event::{KeyEventKind, KeyEventState};
    use scopeguard::defer;
    use serial_test::serial;
    use test_log::test;

    #[test]
    fn parse_rgb_from_hex_form() {
        let color = "#010203";
        assert_eq!(Color::Rgb(1, 2, 3), color.parse().unwrap());
    }

    #[test]
    fn parse_rgb_from_function_form() {
        let color = "rgb(1, 2, 3)";
        assert_eq!(Color::Rgb(1, 2, 3), color.parse().unwrap());
    }

    #[test]
    fn parsing_rgb_ignores_whitespace() {
        let color = "  rgb     ( 1  , 3 , 10    )  ";
        assert_eq!(Color::Rgb(1, 3, 10), color.parse().unwrap());
    }

    #[test]
    #[serial]
    fn get_config_home_gets_right_config_dir_on_all_platforms() {
        #[cfg(target_os = "linux")]
        let correct_path = dirs_next::home_dir().unwrap().join(".config/projectable");
        #[cfg(target_os = "windows")]
        let correct_path = dirs_next::home_dir()
            .unwrap()
            .join("AppData\\Roaming\\projectable");
        #[cfg(target_os = "macos")]
        let correct_path = dirs_next::home_dir().unwrap().join(".config/projectable");

        assert_eq!(correct_path, get_config_home().unwrap());
    }

    #[test]
    #[serial]
    fn uses_env_var_for_config_home_if_set() {
        env::set_var("PROJECTABLE_CONFIG_DIR", "path");
        defer! {
            env::remove_var("PROJECTABLE_CONFIG_DIR");
        }

        assert_eq!(PathBuf::from("path"), get_config_home().unwrap());
    }

    #[test]
    #[serial]
    #[cfg(target_os = "macos")]
    fn use_xdg_config_home_on_mac() {
        env::set_var("XDG_CONFIG_HOME", "path");
        defer! {
            env::remove_var("XDG_CONFIG_HOME");
        }

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
        let mut lhs = Config {
            quit: Key::normal('z'),
            ..Default::default()
        };
        let rhs = Config::default();
        lhs.merge(rhs);
        assert_eq!(Key::normal('z'), lhs.quit);
    }

    #[test]
    fn merge_has_rhs_take_precedence_over_lhs() {
        let mut lhs = Config {
            quit: Key::normal('z'),
            ..Default::default()
        };
        let rhs = Config {
            quit: Key::normal('v'),
            ..Default::default()
        };
        lhs.merge(rhs);
        assert_eq!(Key::normal('v'), lhs.quit);
    }

    #[test]
    fn merge_has_rhs_override_lhs_when_lhs_is_default() {
        let mut lhs = Config::default();
        let rhs = Config {
            quit: Key::normal('v'),
            ..Default::default()
        };
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

    #[test]
    fn properly_reports_keybind_conflicts() {
        let config = Config {
            help: Key::normal('q'),
            down: Key::normal('q'),
            ..Default::default()
        };
        assert_eq!(
            vec![KeyConflict {
                on: &Key::normal('q'),
                conflictors: vec![Action::Quit, Action::Help, Action::Down]
            }],
            config.check_conflicts()
        );
    }

    #[test]
    fn stringifies_keys_properly_with_no_mods() {
        let key = Key::normal('j');
        assert_eq!("j", &key.to_string());
    }

    #[test]
    fn stringifies_keys_properly_with_multiple_mods() {
        let key = Key {
            code: KeyCode::Char('d'),
            mods: KeyModifiers::CONTROL | KeyModifiers::ALT,
        };
        assert_eq!("ctrl-alt-d", &key.to_string());
    }

    #[test]
    fn stringifies_keys_properly_with_one_mod() {
        let key = Key::ctrl('j');
        assert_eq!("ctrl-j", &key.to_string());
    }

    #[test]
    fn merges_custom_keybinds() {
        let config = Config {
            commands: collect![HashMap<_, _>: (Key::normal('v'), "echo testing".to_owned())],
            ..Default::default()
        };
        assert_eq!(
            vec![KeyConflict {
                on: &Key::normal('v'),
                conflictors: vec![
                    Action::FiletreeSpecialCommand,
                    Action::Arbitrary("echo testing"),
                ]
            }],
            config.check_conflicts()
        );
    }

    #[test]
    fn can_parse_key_with_no_mods() {
        let keys = ["a", "b", "z", "r", "d", "?"];

        for key in keys {
            let k = key.parse::<Key>().expect("key should parse correctly");
            assert_eq!(
                Key {
                    code: KeyCode::Char(key.chars().next().unwrap()),
                    mods: KeyModifiers::NONE
                },
                k
            );
        }
    }

    #[test]
    fn can_parse_special_keys() {
        let tests = [
            ("tab", KeyCode::Tab),
            ("backtab", KeyCode::BackTab),
            ("enter", KeyCode::Enter),
            ("up", KeyCode::Up),
        ];

        for (input, expected) in tests {
            assert_eq!(
                expected,
                input.parse::<Key>().expect("should parse correctly").code
            );
        }
    }

    #[test]
    fn can_parse_keys_with_mods() {
        let tests = [
            (
                "ctrl-y",
                Key {
                    code: KeyCode::Char('y'),
                    mods: KeyModifiers::CONTROL,
                },
            ),
            (
                "ctrl-b",
                Key {
                    code: KeyCode::Char('b'),
                    mods: KeyModifiers::CONTROL,
                },
            ),
            (
                "alt-y",
                Key {
                    code: KeyCode::Char('y'),
                    mods: KeyModifiers::ALT,
                },
            ),
            (
                "ctrl-alt-y",
                Key {
                    code: KeyCode::Char('y'),
                    mods: KeyModifiers::CONTROL | KeyModifiers::ALT,
                },
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(expected, input.parse::<Key>().expect("should parse"));
        }
    }
}
