//! Canonical private tags. Display spelling is independent of the comparison key.
use crate::Error;
use std::collections::HashSet;

pub const MAX_TAGS: usize = 32;
pub const MAX_TAG_CHARS: usize = 64;

pub fn label(input: &str) -> Result<String, Error> {
    if input.len() > MAX_TAG_CHARS * 16
        || input.chars().any(|c| c.is_control() && !c.is_whitespace())
    {
        return Err(Error::InvalidArgument("tag"));
    }
    let label = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if label.chars().count() > MAX_TAG_CHARS {
        return Err(Error::InvalidArgument("tag"));
    }
    Ok(label)
}

pub fn key(input: &str) -> Result<String, Error> {
    Ok(label(input)?.to_lowercase())
}

pub fn normalize(tags: &[String]) -> Result<Vec<String>, Error> {
    if tags.len() > 1024 {
        return Err(Error::InvalidArgument("tags"));
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for input in tags {
        let tag = label(input)?;
        if !tag.is_empty() && seen.insert(tag.to_lowercase()) {
            result.push(tag);
        }
        if result.len() > MAX_TAGS {
            return Err(Error::InvalidArgument("tags"));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_whitespace_case_and_duplicate_rules_preserve_first_display_spelling() {
        assert_eq!(
            normalize(&[
                "  Work ".into(),
                "work".into(),
                "  Project\tAlpha ".into(),
                "\u{2003}École\u{2003}".into(),
                "école".into(),
                "".into()
            ])
            .unwrap(),
            vec!["Work", "Project Alpha", "École"]
        );
        assert_eq!(key(" École ").unwrap(), "école");
        assert!(label("bad\0tag").is_err());
        assert!(label(&"x".repeat(65)).is_err());
        assert_eq!(label(&"x".repeat(64)).unwrap().len(), 64);
        assert!(normalize(&(0..33).map(|i| format!("tag-{i}")).collect::<Vec<_>>()).is_err());
    }
}
