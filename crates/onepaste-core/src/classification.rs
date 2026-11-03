//! Private, offline text classification. The original content is never rewritten.
use crate::{envelope::ItemPayload, Error};
use serde::{Deserialize, Serialize};

const FIELD: &str = "text_classification";
/// Parser work is bounded independently of the maximum clipboard item size.
const MAX_PARSE_BYTES: usize = 1 << 20;
pub const LANGUAGES: &[&str] = &[
    "plain",
    "yaml",
    "toml",
    "json",
    "rust",
    "javascript",
    "typescript",
    "python",
    "shell",
    "sql",
    "html",
    "css",
    "other",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub manual: bool,
}
impl Classification {
    fn text() -> Self {
        Self {
            kind: "text".into(),
            language: None,
            manual: false,
        }
    }
    fn code(language: &str) -> Self {
        Self {
            kind: "code".into(),
            language: Some(language.into()),
            manual: false,
        }
    }
    pub fn corrected(text: &str, kind: &str, language: Option<&str>) -> Result<Self, Error> {
        if !["text", "link", "code"].contains(&kind) || kind == "link" && !is_web_url(text) {
            return Err(Error::InvalidArgument("text classification"));
        }
        let language = if kind == "code" {
            let language = language.unwrap_or("other");
            if !LANGUAGES.contains(&language) {
                return Err(Error::InvalidArgument("code language"));
            }
            Some(language.into())
        } else {
            None
        };
        Ok(Self {
            kind: kind.into(),
            language,
            manual: true,
        })
    }
}

pub fn is_web_url(text: &str) -> bool {
    let text = text.trim();
    if text.is_empty()
        || text.chars().any(|c| c.is_whitespace() || c.is_control())
        || text.contains('\\')
    {
        return false;
    }
    // Require an explicit authority, rather than accepting WHATWG's recovery of https:example.org.
    if !text
        .get(..8)
        .map(|s| s.eq_ignore_ascii_case("https://"))
        .unwrap_or(false)
        && !text
            .get(..7)
            .map(|s| s.eq_ignore_ascii_case("http://"))
            .unwrap_or(false)
    {
        return false;
    }
    url::Url::parse(text)
        .map(|u| ["http", "https"].contains(&u.scheme()) && u.has_host())
        .unwrap_or(false)
}

// Validate collection syntax while skipping its values. Building a generic YAML Value would expand
// aliases into owned trees even though classification needs only the root shape.
struct YamlCollection;
impl<'de> Deserialize<'de> for YamlCollection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CollectionVisitor;
        impl<'de> serde::de::Visitor<'de> for CollectionVisitor {
            type Value = YamlCollection;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a YAML collection")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                while map
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {}
                Ok(YamlCollection)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(YamlCollection)
            }
        }
        deserializer.deserialize_any(CollectionVisitor)
    }
}

pub fn classify(text: &str) -> Classification {
    if is_web_url(text) {
        return Classification {
            kind: "link".into(),
            language: None,
            manual: false,
        };
    }
    if text.len() > MAX_PARSE_BYTES {
        return Classification::text();
    }
    let t = text.trim();
    if t.is_empty() {
        return Classification::text();
    }
    if (t.starts_with('{') || t.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(t)
            .map(|v| v.is_object() || v.is_array())
            .unwrap_or(false)
    {
        return Classification::code("json");
    }
    if t.contains('=')
        && toml::from_str::<toml::Table>(t)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    {
        return Classification::code("toml");
    }
    // YAML accepts prose scalars and ordinary colon-separated sentences. Require several structured lines or
    // nesting before invoking the parser, and a collection at the root after parsing.
    let lines: Vec<_> = t
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    let mapping_lines = lines
        .iter()
        .filter(|l| {
            l.split_once(':')
                .map(|(key, value)| {
                    let key = key.trim().trim_matches('"').trim_matches('\'');
                    !key.is_empty()
                        && key
                            .chars()
                            .all(|c| c.is_alphanumeric() || "_.-".contains(c))
                        && (value.is_empty() || value.starts_with(' '))
                })
                .unwrap_or(false)
        })
        .count();
    let nested = lines
        .iter()
        .any(|l| l.starts_with(' ') || l.trim_start().starts_with("- "));
    if lines.len() >= 2
        && mapping_lines >= 1
        && (mapping_lines >= 2 || nested || t.starts_with("---\n"))
    {
        if serde_yaml_ng::from_str::<YamlCollection>(t).is_ok() {
            return Classification::code("yaml");
        }
    }
    let language = if (t.contains("fn ") && t.contains('{'))
        || t.starts_with("use ") && t.contains("::") && t.contains(';')
    {
        Some("rust")
    } else if t.starts_with("interface ") && t.contains('{')
        || t.starts_with("type ") && t.contains(" = ")
    {
        Some("typescript")
    } else if t.contains("function ") && t.contains('{')
        || (t.starts_with("const ") || t.starts_with("let "))
            && (t.contains("=>") || t.ends_with(';'))
    {
        Some("javascript")
    } else if t.starts_with("def ") && t.contains("):")
        || t.starts_with("class ") && t.contains(':')
        || t.starts_with("import ") && t.contains('\n')
    {
        Some("python")
    } else if t.starts_with("#!/bin/")
        || t.starts_with("#!/usr/bin/env ")
        || [
            "docker run ",
            "docker compose ",
            "kubectl ",
            "curl -",
            "git clone ",
        ]
        .iter()
        .any(|s| t.starts_with(s))
    {
        Some("shell")
    } else if ["SELECT ", "INSERT INTO ", "CREATE TABLE ", "UPDATE "]
        .iter()
        .any(|s| t.to_ascii_uppercase().starts_with(s))
        && (t.contains(';') || t.to_ascii_uppercase().contains(" FROM "))
    {
        Some("sql")
    } else if (t.starts_with("<!DOCTYPE html") || t.starts_with("<html") || t.starts_with("<div"))
        && t.contains('>')
        && t.contains("</")
    {
        Some("html")
    } else if t.contains('{') && t.contains('}') && t.contains(':') && t.contains(';') {
        Some("css")
    } else {
        None
    };
    language
        .map(Classification::code)
        .unwrap_or_else(Classification::text)
}

/// Unknown optional fields are held in `extra`, so older clients preserve corrections during unrelated edits.
pub fn classification(payload: &ItemPayload) -> Classification {
    if payload.kind != "text" {
        return Classification {
            kind: payload.kind.clone(),
            language: None,
            manual: false,
        };
    }
    if let Some(stored) = payload
        .extra
        .get(FIELD)
        .and_then(|v| serde_json::from_value::<Classification>(v.clone()).ok())
    {
        if Classification::corrected(
            payload.content.as_deref().unwrap_or(""),
            &stored.kind,
            stored.language.as_deref(),
        )
        .is_ok()
        {
            return stored;
        }
    }
    classify(payload.content.as_deref().unwrap_or(""))
}

pub fn set_classification(payload: &mut ItemPayload, value: Classification) -> Result<(), Error> {
    if payload.kind != "text" {
        return Err(Error::InvalidArgument("not text"));
    }
    let checked = Classification::corrected(
        payload.content.as_deref().unwrap_or(""),
        &value.kind,
        value.language.as_deref(),
    )?;
    payload.extra.insert(
        FIELD.into(),
        serde_json::to_value(Classification {
            manual: value.manual,
            ..checked
        })
        .map_err(|_| Error::Internal)?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corpus_distinguishes_structured_content_from_ordinary_sentences() {
        for (text, kind, language) in [
            ("HTTPS://example.org/path?x=1", "link", None),
            ("https://[::1]:8443/", "link", None),
            ("https:example.org", "text", None),
            ("https://", "text", None),
            ("javascript:alert(1)", "text", None),
            ("file:///private/example", "text", None),
            ("https://example.org is useful", "text", None),
            ("https://example.org\nhttps://example.net", "text", None),
            ("Reminder: bring notes", "text", None),
            ("Meeting: tomorrow\nBring your notes.", "text", None),
            ("Hello world\nThis is ordinary prose.", "text", None),
            ("{\"name\":\"demo\"}", "code", Some("json")),
            ("[1, 2, 3]", "code", Some("json")),
            (
                "[package]\nname = \"demo\"\nversion = \"1.0\"",
                "code",
                Some("toml"),
            ),
            (
                "services:\n  app:\n    image: demo\n    ports:\n      - 8080:80",
                "code",
                Some("yaml"),
            ),
            ("fn main() { println!(\"hi\"); }", "code", Some("rust")),
            ("def hello():\n    return 1", "code", Some("python")),
        ] {
            let actual = classify(text);
            assert_eq!(
                (actual.kind.as_str(), actual.language.as_deref()),
                (kind, language),
                "{text}"
            );
        }
    }
    #[test]
    fn malformed_and_deep_inputs_are_bounded_and_manual_corrections_are_validated() {
        for text in [
            "[".repeat(500),
            "x:\n  y: [".repeat(200),
            "a".repeat(MAX_PARSE_BYTES + 1),
        ] {
            assert_eq!(classify(&text).kind, "text");
        }
        assert!(Classification::corrected("not a URL", "link", None).is_err());
        assert!(Classification::corrected("text", "code", Some("unknown")).is_err());
        let mut p = ItemPayload {
            kind: "text".into(),
            content: Some("https://example.org".into()),
            ..Default::default()
        };
        let correction =
            Classification::corrected(p.content.as_deref().unwrap(), "text", None).unwrap();
        set_classification(&mut p, correction).unwrap();
        assert_eq!(classification(&p).kind, "text");
        assert!(classification(&p).manual);
        assert_eq!(p.content.as_deref(), Some("https://example.org"));
    }
}
