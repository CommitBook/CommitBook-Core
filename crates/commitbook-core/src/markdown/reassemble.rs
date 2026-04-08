use crate::domain::section::{FrontmatterFormat, SectionTree};

/// Reassemble a SectionTree back into a markdown string.
///
/// This is the inverse of `parse_document`. It reconstructs the markdown
/// from the frontmatter, preamble, and sections.
pub fn reassemble(tree: &SectionTree) -> String {
    let mut output = String::new();

    // Frontmatter
    if let Some(fm) = &tree.frontmatter {
        let delimiter = match fm.format {
            FrontmatterFormat::Yaml => "---",
            FrontmatterFormat::Toml => "+++",
        };
        output.push_str(delimiter);
        output.push('\n');
        output.push_str(&fm.raw);
        output.push('\n');
        output.push_str(delimiter);
        output.push('\n');
    }

    // Preamble
    if !tree.preamble.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&tree.preamble);
        output.push('\n');
    }

    // Sections
    for section in &tree.sections {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push('\n');

        // Emit heading
        let hashes = "#".repeat(section.level as usize);
        output.push_str(&hashes);
        output.push(' ');
        output.push_str(&section.heading_text);
        output.push('\n');

        // Emit content
        if !section.content.is_empty() {
            output.push('\n');
            output.push_str(&section.content);
            output.push('\n');
        }
    }

    output
}

#[cfg(test)]
#[path = "reassemble_tests.rs"]
mod tests;
