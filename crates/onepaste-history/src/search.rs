//! Searching the history.

use crate::store::{HistoryError, HistoryStore};
use rusqlite::types::Value;
use rusqlite::params_from_iter;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    Favorites,
    Text,
    Images,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Hit {
    pub item_id: String,
    pub kind: String,
    /// The first 200 characters of the text.
    pub text: String,
    pub favorite: bool,
    pub pinned: bool,
    pub note: String,
    /// The type and size in bytes of an image (empty and 0 for text).
    pub mime: String,
    pub size: i64,
    /// Where the first search term occurs in the text, as a character offset and length (none when it matched the
    /// note or the tags, or the query was empty).
    pub match_start: Option<usize>,
    pub match_len: usize,
}

const PREFIX_CHARS: usize = 200;
/// Shortest term the trigram index can look up.
const MIN_INDEXED_CHARS: usize = 3;

impl HistoryStore {
    /// Searches text, note and tags. An empty query lists the most recent items. Pinned items come first, then the
    /// newest. A term of three or more characters uses the full-text index; a shorter one (common in Chinese, where
    /// words are often two characters) falls back to a plain substring match. The query is always plain text: quotes,
    /// `*`, `-`, `:` and parentheses mean nothing special. ASCII case is ignored.
    pub fn search(&self, query: &str, filter: Filter, limit: usize, offset: usize) -> Result<Vec<Hit>, HistoryError> {
        let mut conds: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        match filter {
            Filter::All => {}
            Filter::Favorites => conds.push("favorite = 1".into()),
            Filter::Text => conds.push("kind = 'text'".into()),
            Filter::Images => conds.push("kind = 'image'".into()),
        }
        let terms: Vec<&str> = query.split_whitespace().collect();
        for term in &terms {
            if term.chars().count() >= MIN_INDEXED_CHARS {
                // A quoted phrase is a literal; only the quote itself has to be doubled
                args.push(Value::Text(format!("\"{}\"", term.replace('"', "\"\""))));
                conds.push(format!("rowid IN (SELECT rowid FROM history_fts WHERE history_fts MATCH ?{})", args.len()));
            } else {
                args.push(Value::Text((*term).to_string()));
                let n = args.len();
                conds.push(format!(
                    "(instr(lower(text), lower(?{n})) > 0 OR instr(lower(note), lower(?{n})) > 0 OR instr(lower(tags), lower(?{n})) > 0)"
                ));
            }
        }
        let where_clause = if conds.is_empty() { String::new() } else { format!("WHERE {}", conds.join(" AND ")) };
        args.push(Value::Integer(limit as i64));
        args.push(Value::Integer(offset as i64));
        let sql = format!(
            "SELECT item_id, kind, text, favorite, pinned, note, mime, size FROM history {where_clause}
             ORDER BY pinned DESC, created_at DESC, item_id LIMIT ?{} OFFSET ?{}",
            args.len() - 1,
            args.len()
        );
        let first = terms.first().map(|t| t.to_ascii_lowercase());
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)? != 0, r.get::<_, i64>(4)? != 0, r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, i64>(7)?))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (item_id, kind, text, favorite, pinned, note, mime, size) = row?;
            let (match_start, match_len) = match &first {
                Some(t) => match text.to_ascii_lowercase().find(t.as_str()) {
                    Some(byte) => (Some(text[..byte].chars().count()), t.chars().count()),
                    None => (None, 0),
                },
                None => (None, 0),
            };
            hits.push(Hit { item_id, kind, text: text.chars().take(PREFIX_CHARS).collect(), favorite, pinned, note, mime, size, match_start, match_len });
        }
        Ok(hits)
    }
}
