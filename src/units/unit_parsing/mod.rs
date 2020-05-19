mod service_unit;
mod socket_unit;
mod target_unit;
mod unit_parser;

pub use service_unit::*;
pub use socket_unit::*;
pub use target_unit::*;
pub use unit_parser::*;

use std::path::PathBuf;

pub struct ParsedCommonConfig {
    pub unit: ParsedUnitSection,
    pub install: ParsedInstallSection,
    pub name: String,
}
pub struct ParsedServiceConfig {
    pub common: ParsedCommonConfig,
    pub srvc: ParsedServiceSection,
}
pub struct ParsedSocketConfig {
    pub common: ParsedCommonConfig,
    pub sock: ParsedSocketSection,
}
pub struct ParsedTargetConfig {
    pub common: ParsedCommonConfig,
}

#[derive(Default)]
pub struct ParsedUnitSection {
    pub description: String,

    pub wants: Vec<String>,
    pub requires: Vec<String>,
    pub before: Vec<String>,
    pub after: Vec<String>,
}
#[derive(Clone)]
pub struct ParsedSingleSocketConfig {
    pub kind: crate::sockets::SocketKind,
    pub specialized: crate::sockets::SpecializedSocketConfig,
}

impl std::fmt::Debug for ParsedSingleSocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(
            f,
            "SocketConfig {{ kind: {:?}, specialized: {:?} }}",
            self.kind, self.specialized
        )?;
        Ok(())
    }
}

pub struct ParsedSocketSection {
    pub sockets: Vec<ParsedSingleSocketConfig>,
    pub filedesc_name: Option<String>,
    pub services: Vec<String>,

    pub exec_section: ParsedExecSection,
}
pub struct ParsedServiceSection {
    pub restart: ServiceRestart,
    pub accept: bool,
    pub notifyaccess: NotifyKind,
    pub exec: Commandline,
    pub stop: Vec<Commandline>,
    pub stoppost: Vec<Commandline>,
    pub startpre: Vec<Commandline>,
    pub startpost: Vec<Commandline>,
    pub srcv_type: ServiceType,
    pub starttimeout: Option<Timeout>,
    pub stoptimeout: Option<Timeout>,
    pub generaltimeout: Option<Timeout>,

    pub dbus_name: Option<String>,

    pub sockets: Vec<String>,

    pub exec_section: ParsedExecSection,
}

#[derive(Default)]
pub struct ParsedInstallSection {
    pub wanted_by: Vec<String>,
    pub required_by: Vec<String>,
}
pub struct ParsedExecSection {
    pub user: Option<String>,
    pub group: Option<String>,
    pub stdout_path: Option<StdIoOption>,
    pub stderr_path: Option<StdIoOption>,
    pub supplementary_groups: Vec<String>,
}

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum ServiceType {
    Simple,
    Notify,
    Dbus,
    OneShot,
}

impl ServiceType {
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw.to_uppercase().as_str() {
            "SIMPLE" => Some(Self::Simple),
            "NOTIFY" => Some(Self::Notify),
            "DBUS" => Some(Self::Dbus),
            "ONESHOT" => Some(Self::OneShot),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::Simple => "simple".to_owned(),
            Self::Notify => "notify".to_owned(),
            Self::Dbus => "dbus".to_owned(),
            Self::OneShot => "oneshot".to_owned(),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum NotifyKind {
    Main,
    Exec,
    All,
    None,
}

impl NotifyKind {
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw.to_uppercase().as_str() {
            "MAIN" => Some(Self::Main),
            "EXEC" => Some(Self::Exec),
            "ALL" => Some(Self::All),
            "NONE" => Some(Self::None),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::Main => "main".to_owned(),
            Self::Exec => "exec".to_owned(),
            Self::All => "all".to_owned(),
            Self::None => "none".to_owned(),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ServiceRestart {
    Always,
    No,
}

impl ServiceRestart {
    pub fn from_str(raw: &str) -> Option<ServiceRestart> {
        match raw.to_uppercase().as_str() {
            "ALWAYS" => Some(ServiceRestart::Always),
            "NO" => Some(ServiceRestart::No),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            ServiceRestart::Always => "always".to_owned(),
            ServiceRestart::No => "no".to_owned(),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum Timeout {
    Duration(std::time::Duration),
    Infinity,
}

impl Timeout {
    pub fn from_str(raw: &str) -> Option<Self> {
        if let Ok(secs) = raw.parse::<u64>() {
            Some(Self::Duration(std::time::Duration::from_secs(secs)))
        } else if raw.to_uppercase().as_str() == "INFINITY" {
            Some(Self::Infinity)
        } else {
            None
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::Duration(dur) => dur.as_secs().to_string(),
            Self::Infinity => "infinity".to_owned(),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum StdIoOption {
    File(PathBuf),
    AppendFile(PathBuf),
}

impl StdIoOption {
    pub fn export_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        match self {
            Self::File(p) => {
                map["path"] = serde_json::Value::String(p.to_str().unwrap().to_owned());
                map["type"] = serde_json::Value::String("file".to_owned());
            }
            Self::AppendFile(p) => {
                map["path"] = serde_json::Value::String(p.to_str().unwrap().to_owned());
                map["type"] = serde_json::Value::String("append".to_owned());
            }
        }
        serde_json::Value::Object(map)
    }

    pub fn import_json(raw: &serde_json::Value) -> Self {
        let typ = raw["type"].as_str().unwrap();
        let p = raw["path"].as_str().unwrap().into();
        match typ {
            "file" => StdIoOption::File(p),
            "append" => StdIoOption::AppendFile(p),
            _ => todo!(),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CommandlinePrefix {
    AtSign,
    Minus,
    Colon,
    Plus,
    Exclamation,
    DoubleExclamation,
}

impl CommandlinePrefix {
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "@" => Some(Self::AtSign),
            "-" => Some(Self::Minus),
            ":" => Some(Self::Colon),
            "+" => Some(Self::Plus),
            "!" => Some(Self::Exclamation),
            "!!" => Some(Self::DoubleExclamation),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::AtSign => "@".to_owned(),
            Self::Minus => "-".to_owned(),
            Self::Colon => ":".to_owned(),
            Self::Plus => "+".to_owned(),
            Self::Exclamation => "!".to_owned(),
            Self::DoubleExclamation => "!!".to_owned(),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Commandline {
    pub cmd: String,
    pub args: Vec<String>,
    pub prefixes: Vec<CommandlinePrefix>,
}

impl Commandline {
    pub fn export_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map["cmd"] = serde_json::Value::String(self.cmd.clone());
        map["args"] = serde_json::Value::Array(
            self.args
                .iter()
                .map(|arg| serde_json::Value::String(arg.clone()))
                .collect(),
        );
        map["prefixes"] = serde_json::Value::Array(
            self.prefixes
                .iter()
                .map(|prefix| serde_json::Value::String(prefix.to_string()))
                .collect(),
        );
        serde_json::Value::Object(map)
    }

    pub fn import_json(raw: &serde_json::Value) -> Self {
        let cmd = raw["cmd"].as_str().unwrap().to_owned();
        let args = raw["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned())
            .collect();
        let prefixes = raw["prefixes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| CommandlinePrefix::from_str(arg.as_str().unwrap()).unwrap())
            .collect();
        Commandline {
            cmd,
            args,
            prefixes,
        }
    }
}

impl ToString for Commandline {
    fn to_string(&self) -> String {
        format!("{:?}", self)
    }
}

#[derive(Debug)]
pub struct ParsingError {
    inner: ParsingErrorReason,
    path: std::path::PathBuf,
}

impl ParsingError {
    pub fn new(reason: ParsingErrorReason, path: std::path::PathBuf) -> ParsingError {
        ParsingError {
            inner: reason,
            path,
        }
    }
}

#[derive(Debug)]
pub enum ParsingErrorReason {
    UnknownSetting(String, String),
    UnusedSetting(String),
    UnsupportedSetting(String),
    MissingSetting(String),
    SettingTooManyValues(String, Vec<String>),
    SectionTooOften(String),
    SectionNotFound(String),
    UnknownSection(String),
    UnknownSocketAddr(String),
    FileError(Box<dyn std::error::Error>),
    Generic(String),
}

impl std::fmt::Display for ParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.inner {
            ParsingErrorReason::UnknownSetting(name, value) => {
                write!(
                    f,
                    "In file {:?}: setting {} was set to unrecognized value: {}",
                    self.path, name, value
                )?;
            }
            ParsingErrorReason::UnusedSetting(name) => {
                write!(
                    f,
                    "In file {:?}: unused setting {} occured",
                    self.path, name
                )?;
            }
            ParsingErrorReason::MissingSetting(name) => {
                write!(
                    f,
                    "In file {:?}: required setting {} missing",
                    self.path, name
                )?;
            }
            ParsingErrorReason::SectionNotFound(name) => {
                write!(
                    f,
                    "In file {:?}: Section {} wasn't found but is required",
                    self.path, name
                )?;
            }
            ParsingErrorReason::UnknownSection(name) => {
                write!(f, "In file {:?}: Section {} is unknown", self.path, name)?;
            }
            ParsingErrorReason::SectionTooOften(name) => {
                write!(
                    f,
                    "In file {:?}: section {} occured multiple times",
                    self.path, name
                )?;
            }
            ParsingErrorReason::UnknownSocketAddr(addr) => {
                write!(
                    f,
                    "In file {:?}: Can not open sockets of addr: {}",
                    self.path, addr
                )?;
            }
            ParsingErrorReason::UnsupportedSetting(addr) => {
                write!(
                    f,
                    "In file {:?}: Setting not supported by this build (maybe need to enable feature flag?): {}",
                    self.path, addr
                )?;
            }
            ParsingErrorReason::SettingTooManyValues(name, values) => {
                write!(
                    f,
                    "In file {:?}: setting {} occured with too many values: {:?}",
                    self.path, name, values
                )?;
            }
            ParsingErrorReason::FileError(e) => {
                write!(f, "While parsing file {:?}: {}", self.path, e)?;
            }
            ParsingErrorReason::Generic(e) => {
                write!(f, "While parsing file {:?}: {}", self.path, e)?;
            }
        }

        Ok(())
    }
}

// This is important for other errors to wrap this one.
impl std::error::Error for ParsingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Generic error, underlying cause isn't tracked.
        if let ParsingErrorReason::FileError(err) = &self.inner {
            Some(err.as_ref())
        } else {
            None
        }
    }
}

impl std::convert::From<Box<std::io::Error>> for ParsingErrorReason {
    fn from(err: Box<std::io::Error>) -> Self {
        ParsingErrorReason::FileError(err)
    }
}
