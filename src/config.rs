use std::{
    fmt::Display,
    path::{Path, PathBuf},
    str::FromStr,
};

use linked_hash_map::LinkedHashMap;
use serde::Deserializer;
use serde_with::{serde_as, DisplayFromStr};

pub fn deserialize_optional_from_str<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    let result: Option<String> =
        serde_with::rust::unwrap_or_skip::deserialize(deserializer).unwrap_or(None);

    Ok(if let Some(s) = result {
        Some(
            T::from_str(&s)
                .map_err(|e| serde::de::Error::custom(format!("Failed to parse - {e}")))?,
        )
    } else {
        None
    })
}

#[serde_as]
#[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RawConfig {
    #[serde(default)]
    #[serde_as(as = "DisplayFromStr")]
    pub root: bool,
    #[serde(flatten)]
    pub configs: RawConfigs,
}

#[derive(serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IndentStyle {
    Space,
    Tab,
}

#[derive(serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    Lf,
    Crlf,
    Cr,
}

#[derive(serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    #[serde(rename = "latin1")]
    Latin1,
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-8-bom")]
    Utf8WithBom,
    #[serde(rename = "utf-16be")]
    Utf16BigEndian,
    #[serde(rename = "utf-16le")]
    Utf16LittleEndian,
}

impl Default for Charset {
    fn default() -> Self {
        Charset::Utf8
    }
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub indent_style: Option<IndentStyle>,
    #[serde(default, deserialize_with = "deserialize_optional_from_str")]
    pub indent_size: Option<usize>,
    #[serde(default, deserialize_with = "deserialize_optional_from_str")]
    pub tab_width: Option<usize>,
    pub end_of_line: Option<LineEnding>,
    pub charset: Option<Charset>,
    #[serde(default, deserialize_with = "deserialize_optional_from_str")]
    pub trim_trailing_whitespace: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_from_str")]
    pub insert_final_newline: Option<bool>,
}

/// last item has high priority
#[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RawConfigs(pub LinkedHashMap<String, Config>);

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed to parse {1}: {0}")]
    ParseError(serde_ini::de::Error, PathBuf),
    #[error("Failed to canonicalize given path")]
    PathCanonicalizeError(std::io::Error),
    #[error("Failed to open config file at {1}: {0}")]
    ConfigOpenError(std::io::Error, String),
    #[error("Failed to parse glob pattern: {0}")]
    PathPatternError(String),
    #[error("Failed to find matched config")]
    NotFound,
}

const CONFIG_FILENAME: &str = ".editorconfig";

fn parse_pattern(mut s: &str) -> Result<impl Iterator<Item = Result<glob::Pattern, Error>>, Error> {
    fn expand(mut prefixes: Vec<String>, s: &str) -> Result<(Vec<String>, &str), Error> {
        if let Some(begin_pos) = s.find('{') {
            if let Some(end_pos) = s[begin_pos..].find('}') {
                let prev = &s[0..begin_pos];
                let inner = &s[(begin_pos + 1)..end_pos];
                if let Some((num1, num2)) = inner.split_once("..") {
                    let num1: i32 = num1.parse().map_err(|e| {
                        Error::PathPatternError(format!(
                            "Failed to expand number range pattern. Found invalid number - {e}"
                        ))
                    })?;
                    let num2: i32 = num2.parse().map_err(|e| {
                        Error::PathPatternError(format!(
                            "Failed to expand number range pattern. Found invalid number - {e}"
                        ))
                    })?;

                    Ok((
                        prefixes
                            .iter()
                            .flat_map(|prefix| {
                                (num1..=num2)
                                    .into_iter()
                                    .map(move |i| format!("{prefix}{prev}{i}"))
                            })
                            .collect(),
                        &s[end_pos..],
                    ))
                } else {
                    let items = inner.split(',');

                    Ok((
                        prefixes
                            .iter()
                            .flat_map(|prefix| {
                                items.clone().map(move |i| format!("{prefix}{prev}{i}"))
                            })
                            .collect(),
                        &s[(end_pos + 1)..],
                    ))
                }
            } else {
                Err(Error::PathPatternError(
                    "Matched } is not found".to_string(),
                ))
            }
        } else {
            for pref in &mut prefixes {
                pref.push_str(s);
            }

            Ok((prefixes, ""))
        }
    }
    let mut expanded_patterns = vec!["".to_string()];
    while !s.is_empty() {
        (expanded_patterns, s) = expand(expanded_patterns, s)?;
    }

    Ok(expanded_patterns.into_iter().map(|pattern| {
        glob::Pattern::new(&pattern).map_err(|e| Error::PathPatternError(e.to_string()))
    }))
}

impl Config {
    pub fn get_config_for(path: &Path) -> Result<Config, Error> {
        let canonicalized_path = path.canonicalize().map_err(Error::PathCanonicalizeError)?;
        for dir in canonicalized_path.ancestors() {
            let mut config_path = dir.with_file_name(CONFIG_FILENAME);
            if config_path.is_file() {
                let file = std::fs::File::open(&config_path).map_err(|e| {
                    Error::ConfigOpenError(e, config_path.to_string_lossy().to_string())
                })?;
                let config: RawConfig = match serde_ini::from_read(file) {
                    Ok(c) => c,
                    Err(e) => return Err(Error::ParseError(e, config_path)),
                };
                let is_root = config.root;

                config_path.pop();
                let relative_path = canonicalized_path.strip_prefix(&config_path).unwrap();
                for (pattern, config) in config.configs.0.into_iter().rev() {
                    for pattern in parse_pattern(&pattern)? {
                        let pattern = pattern?;
                        if pattern.matches_path(relative_path) {
                            return Ok(config);
                        }
                    }
                }

                if is_root {
                    break;
                }
            }
        }

        Err(Error::NotFound)
    }
}
