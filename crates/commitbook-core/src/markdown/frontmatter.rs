use std::collections::BTreeMap;

use crate::domain::section::{Frontmatter, FrontmatterFormat};

/// Extract frontmatter from the beginning of a markdown document.
/// Returns the parsed frontmatter (if any) and the remaining content after it.
pub fn extract_frontmatter(content: &str) -> (Option<Frontmatter>, &str) {
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            let raw = &rest[..end];
            let remaining = &rest[end + 5..]; // skip "\n---\n"
            let fields = parse_yaml_fields(raw);
            return (
                Some(Frontmatter {
                    format: FrontmatterFormat::Yaml,
                    raw: raw.to_string(),
                    fields,
                }),
                remaining,
            );
        }
        // Check for frontmatter at very end of file (no trailing newline after closing ---)
        if let Some(end) = rest.find("\n---") {
            if rest[end + 4..].is_empty() || &rest[end + 4..] == "\n" {
                let raw = &rest[..end];
                let remaining_start = end + 4;
                let remaining = if remaining_start < rest.len() {
                    &rest[remaining_start..]
                } else {
                    ""
                };
                let fields = parse_yaml_fields(raw);
                return (
                    Some(Frontmatter {
                        format: FrontmatterFormat::Yaml,
                        raw: raw.to_string(),
                        fields,
                    }),
                    remaining,
                );
            }
        }
    }

    if let Some(rest) = content.strip_prefix("+++\n") {
        if let Some(end) = rest.find("\n+++\n") {
            let raw = &rest[..end];
            let remaining = &rest[end + 5..];
            let fields = parse_toml_fields(raw);
            return (
                Some(Frontmatter {
                    format: FrontmatterFormat::Toml,
                    raw: raw.to_string(),
                    fields,
                }),
                remaining,
            );
        }
        if let Some(end) = rest.find("\n+++") {
            if rest[end + 4..].is_empty() || &rest[end + 4..] == "\n" {
                let raw = &rest[..end];
                let remaining_start = end + 4;
                let remaining = if remaining_start < rest.len() {
                    &rest[remaining_start..]
                } else {
                    ""
                };
                let fields = parse_toml_fields(raw);
                return (
                    Some(Frontmatter {
                        format: FrontmatterFormat::Toml,
                        raw: raw.to_string(),
                        fields,
                    }),
                    remaining,
                );
            }
        }
    }

    (None, content)
}

/// Simple YAML key-value parser for flat frontmatter.
/// Handles `key: value` lines. Not a full YAML parser.
fn parse_yaml_fields(raw: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_string();
            let value = line[colon_pos + 1..].trim().to_string();
            // Strip surrounding quotes if present.
            let value = if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value[1..value.len() - 1].to_string()
            } else {
                value
            };
            if !key.is_empty() {
                fields.insert(key, value);
            }
        }
    }
    fields
}

/// Simple TOML key-value parser for flat frontmatter.
/// Handles `key = value` and `key = "value"` lines.
fn parse_toml_fields(raw: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let value = line[eq_pos + 1..].trim().to_string();
            let value = if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                value[1..value.len() - 1].to_string()
            } else {
                value
            };
            if !key.is_empty() {
                fields.insert(key, value);
            }
        }
    }
    fields
}

#[cfg(test)]
#[path = "frontmatter_tests.rs"]
mod tests;
