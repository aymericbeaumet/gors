use std::collections::HashMap;

use gors::sourcemap::{SourceMap, SourceMapBuilder};

pub(crate) struct Comments {
    source_name: String,
    comments: Vec<Comment>,
}

struct Comment {
    go_line: u32,
    go_column: u32,
    text: String,
    is_doc: bool,
}

struct CommentMapping {
    go_line: u32,
    go_column: u32,
    output_line: u32,
    output_column: u32,
    text: String,
}

struct RenderedComments {
    output: String,
    mappings: Vec<CommentMapping>,
    /// Exact destination-line displacement for every original Rust line.
    ///
    /// This is measured while rendering instead of reconstructed from the
    /// number of comments: one block comment can contain multiple physical
    /// lines, and leading comments add a separate blank-line separator.
    line_shifts: Vec<u32>,
}

pub(crate) fn collect(
    comments: &gors::compiler::db::FileComments,
    source_name: &str,
    source: &str,
) -> Comments {
    Comments {
        source_name: source_name.to_string(),
        comments: comments
            .comments()
            .iter()
            .map(|comment| {
                let go_line = u32::try_from(comment.line()).unwrap_or(u32::MAX);
                let byte_column = u32::try_from(comment.column()).unwrap_or(u32::MAX);
                let go_column = source
                    .lines()
                    .nth(comment.line().saturating_sub(1))
                    .map(|line| gors::sourcemap::utf8_byte_column_to_utf16(line, byte_column))
                    .unwrap_or(byte_column)
                    .saturating_sub(1);
                Comment {
                    go_line,
                    go_column,
                    text: comment.text().to_string(),
                    is_doc: comment.is_doc(),
                }
            })
            .collect(),
    }
}

pub(crate) fn insert_and_remap(
    rust_source: &str,
    comments: &Comments,
    initial_source_map: &SourceMap,
) -> (String, SourceMap) {
    let rendered = insert_comments(rust_source, &comments.comments, initial_source_map);
    let source_map = remap(
        initial_source_map,
        &rendered.mappings,
        &rendered.line_shifts,
        &comments.source_name,
    );
    (rendered.output, source_map)
}

fn remap(
    initial_source_map: &SourceMap,
    comment_mappings: &[CommentMapping],
    line_shifts: &[u32],
    comment_source_name: &str,
) -> SourceMap {
    let mut builder = SourceMapBuilder::new(initial_source_map.get_file());
    builder.set_debug_id(initial_source_map.get_debug_id());
    // Register every source slot independently before restoring its exact
    // spelling. SourceMapBuilder normally deduplicates equal source strings,
    // which would change source indices in an otherwise lossless remap.
    let mut source_indices = Vec::with_capacity(initial_source_map.get_source_count() as usize);
    for source_id in 0..initial_source_map.get_source_count() {
        let placeholder = format!("\0gors-source-slot-{source_id}");
        let source_index = builder.add_source(&placeholder);
        if let Some(source) = initial_source_map.get_source(source_id) {
            builder.set_source(source_index, source);
        }
        if let Some(content) = initial_source_map.get_source_contents(source_id) {
            builder.set_source_contents(source_index, Some(content));
        }
        source_indices.push(source_index);
    }

    for source_id in initial_source_map.ignore_list() {
        if let Some(source_index) = source_indices.get(*source_id as usize) {
            builder.add_to_ignore_list(*source_index);
        }
    }

    // Add the complete original name table first so existing name IDs remain
    // stable; inserted comment names are appended afterwards.
    let mut name_indices = Vec::with_capacity(initial_source_map.get_name_count() as usize);
    for name_id in 0..initial_source_map.get_name_count() {
        if let Some(name) = initial_source_map.get_name(name_id) {
            name_indices.push(builder.add_name(name));
        }
    }

    let comment_source_index = (0..initial_source_map.get_source_count())
        .find(|source_id| initial_source_map.get_source(*source_id) == Some(comment_source_name))
        .and_then(|source_id| source_indices.get(source_id as usize))
        .copied();

    for index in 0..initial_source_map.get_token_count() {
        let Some(token) = initial_source_map.get_token(index as usize) else {
            continue;
        };
        let original_output_line = token.get_dst_line();
        let shift = line_shifts
            .get(original_output_line as usize)
            .copied()
            .unwrap_or_else(|| *line_shifts.last().unwrap_or(&0));
        let source_index = token
            .has_source()
            .then(|| source_indices.get(token.get_src_id() as usize).copied())
            .flatten();
        let name_index = token
            .has_name()
            .then(|| name_indices.get(token.get_name_id() as usize).copied())
            .flatten();
        builder.add_raw(
            original_output_line + shift,
            token.get_dst_col(),
            token.get_src_line(),
            token.get_src_col(),
            source_index,
            name_index,
            token.is_range(),
        );
    }

    for mapping in comment_mappings {
        let name_index = builder.add_name(&mapping.text);
        builder.add_raw(
            mapping.output_line,
            mapping.output_column,
            mapping.go_line,
            mapping.go_column,
            comment_source_index,
            Some(name_index),
            false,
        );
    }
    builder.into_sourcemap()
}

fn insert_comments(
    rust_source: &str,
    comments: &[Comment],
    source_map: &SourceMap,
) -> RenderedComments {
    let rust_lines = rust_source.lines().collect::<Vec<_>>();
    let mut go_to_output_line = HashMap::<u32, u32>::new();
    for index in 0..source_map.get_token_count() {
        let Some(token) = source_map.get_token(index as usize) else {
            continue;
        };
        let go_line = token.get_src_line();
        let output_line = token.get_dst_line();
        go_to_output_line
            .entry(go_line)
            .and_modify(|existing| *existing = (*existing).min(output_line))
            .or_insert(output_line);
    }

    let maximum_mapped_go_line = go_to_output_line.keys().copied().max().unwrap_or(0);
    let mut comments_by_output_line = HashMap::<u32, Vec<&Comment>>::new();
    let mut leading_comments = Vec::new();
    let mut trailing_comments = Vec::new();

    for comment in comments {
        if comment.is_doc {
            continue;
        }
        let go_line = comment.go_line.saturating_sub(1);
        let target_output_line = go_to_output_line.get(&go_line).copied().or_else(|| {
            ((go_line + 1)..go_line + 20)
                .find_map(|next_line| go_to_output_line.get(&next_line).copied())
        });

        if let Some(output_line) = target_output_line {
            comments_by_output_line
                .entry(output_line)
                .or_default()
                .push(comment);
        } else if comment.go_line <= 2 {
            leading_comments.push(comment);
        } else if go_line > maximum_mapped_go_line {
            trailing_comments.push(comment);
        }
    }

    if !trailing_comments.is_empty()
        && let Some(closing_brace_line) = rust_lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| line.trim() == "}")
            .map(|(index, _)| index as u32)
    {
        comments_by_output_line
            .entry(closing_brace_line)
            .or_default()
            .extend(trailing_comments);
    }

    render(&rust_lines, &leading_comments, &comments_by_output_line)
}

fn render(
    rust_lines: &[&str],
    leading_comments: &[&Comment],
    comments_by_output_line: &HashMap<u32, Vec<&Comment>>,
) -> RenderedComments {
    let mut output = String::new();
    let mut mappings = Vec::new();
    let mut line_shifts = Vec::with_capacity(rust_lines.len());
    let mut current_output_line = 0u32;

    for comment in leading_comments {
        mappings.push(CommentMapping {
            go_line: comment.go_line.saturating_sub(1),
            go_column: comment.go_column,
            output_line: current_output_line,
            output_column: 0,
            text: comment.text.clone(),
        });
        output.push_str(&comment.text);
        output.push('\n');
        current_output_line =
            current_output_line.saturating_add(physical_lines_inserted(&comment.text));
    }
    if !leading_comments.is_empty() && !rust_lines.is_empty() {
        output.push('\n');
        current_output_line += 1;
    }

    for (index, line) in rust_lines.iter().enumerate() {
        let original_output_line = index as u32;
        if let Some(comments) = comments_by_output_line.get(&original_output_line) {
            let indent = if line.trim() == "}" {
                4
            } else {
                line.len() - line.trim_start().len()
            };
            let indent_text = " ".repeat(indent);
            for comment in comments {
                mappings.push(CommentMapping {
                    go_line: comment.go_line.saturating_sub(1),
                    go_column: comment.go_column,
                    output_line: current_output_line,
                    output_column: indent as u32,
                    text: comment.text.clone(),
                });
                output.push_str(&indent_text);
                output.push_str(&comment.text);
                output.push('\n');
                current_output_line =
                    current_output_line.saturating_add(physical_lines_inserted(&comment.text));
            }
        }
        line_shifts.push(current_output_line.saturating_sub(original_output_line));
        output.push_str(line);
        output.push('\n');
        current_output_line += 1;
    }

    RenderedComments {
        output,
        mappings,
        line_shifts,
    }
}

fn physical_lines_inserted(text: &str) -> u32 {
    let embedded_line_breaks = text
        .as_bytes()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count();
    u32::try_from(embedded_line_breaks)
        .unwrap_or(u32::MAX)
        .saturating_add(1)
}

#[cfg(test)]
mod tests;
