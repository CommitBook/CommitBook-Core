use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::domain::section::SectionTree;

use super::frontmatter::extract_frontmatter;
use super::section_tree::build_sections;

/// Parse a markdown document into a structured SectionTree.
///
/// This extracts frontmatter (YAML or TOML), the preamble (content before the
/// first heading), and a flat list of heading-based sections with hierarchical paths.
pub fn parse_document(content: &str) -> SectionTree {
    let (frontmatter, body) = extract_frontmatter(content);

    let options = Options::all();

    let mut in_heading = false;
    let mut current_heading_text = String::new();
    let mut current_heading_level: u8 = 0;

    // Collect heading positions and text via offset-based parsing.
    let mut headings_info: Vec<(u8, String, usize)> = Vec::new(); // (level, text, end_offset)

    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                in_heading = true;
                current_heading_text.clear();
                current_heading_level = heading_level_to_u8(level);
            }
            Event::Text(text) if in_heading => {
                current_heading_text.push_str(&text);
            }
            Event::Code(code) if in_heading => {
                current_heading_text.push_str(&code);
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = false;
                headings_info.push((
                    current_heading_level,
                    current_heading_text.clone(),
                    range.end,
                ));
            }
            _ => {}
        }
    }

    if headings_info.is_empty() {
        // No headings: everything is preamble.
        return SectionTree {
            frontmatter,
            preamble: body.to_string(),
            sections: Vec::new(),
        };
    }

    // Extract preamble: content before the first heading.
    let first_heading_source_start =
        find_heading_start(body, 0, &headings_info[0].1, headings_info[0].0);
    let preamble = body[..first_heading_source_start].trim_end().to_string();

    // Extract section content: text between consecutive headings.
    let mut raw_sections: Vec<(u8, String, String)> = Vec::new();
    for (i, (level, heading_text, heading_end_offset)) in headings_info.iter().enumerate() {
        let content_start = *heading_end_offset;
        let content_end = if i + 1 < headings_info.len() {
            find_heading_start(
                body,
                content_start,
                &headings_info[i + 1].1,
                headings_info[i + 1].0,
            )
        } else {
            body.len()
        };

        let section_content = body[content_start..content_end].trim().to_string();
        raw_sections.push((*level, heading_text.clone(), section_content));
    }

    let sections = build_sections(raw_sections);

    SectionTree {
        frontmatter,
        preamble,
        sections,
    }
}

/// Find the byte offset where a heading's markdown source starts (the `#` characters).
fn find_heading_start(body: &str, search_from: usize, heading_text: &str, level: u8) -> usize {
    let prefix = "#".repeat(level as usize);
    let search_area = &body[search_from..];

    for line in search_area.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&prefix) {
            let after_hashes = trimmed[prefix.len()..].trim_start();
            if after_hashes.starts_with(heading_text.trim()) {
                // Find this line's byte position in the search area.
                if let Some(line_offset) = search_area.find(line) {
                    return search_from + line_offset;
                }
            }
        }
    }

    search_from
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
