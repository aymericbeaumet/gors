use std::collections::HashMap;

use gors::sourcemap::{SourceMap, SourceMapBuilder};

pub(crate) struct Comments(Vec<Comment>);

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
    inserted_before_output_line: u32,
    text: String,
}

pub(crate) fn collect(comments: &gors::compiler::db::FileComments, source: &str) -> Comments {
    Comments(
        comments
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
    )
}

pub(crate) fn insert_and_remap(
    rust_source: &str,
    comments: &Comments,
    initial_source_map: &SourceMap,
) -> (String, SourceMap) {
    let (output, comment_mappings) = insert_comments(rust_source, &comments.0, initial_source_map);
    let source_map = remap(&output, initial_source_map, &comment_mappings);
    (output, source_map)
}

fn remap(
    _rust_source: &str,
    initial_source_map: &SourceMap,
    comment_mappings: &[CommentMapping],
) -> SourceMap {
    let mut builder = SourceMapBuilder::new(Some("output.rs"));
    let source_index = builder.add_source("main.go");
    if let Some(content) = initial_source_map.get_source_contents(0) {
        builder.set_source_contents(source_index, Some(content));
    }

    let mut comments_before_line = HashMap::<u32, u32>::new();
    for mapping in comment_mappings {
        *comments_before_line
            .entry(mapping.inserted_before_output_line)
            .or_default() += 1;
    }

    let maximum_output_line = initial_source_map
        .tokens()
        .map(|token| token.get_dst_line())
        .max()
        .unwrap_or(0);
    let mut cumulative_shift = vec![0u32; (maximum_output_line + 2) as usize];
    let mut running_shift = 0u32;
    for line in 0..=maximum_output_line + 1 {
        running_shift += comments_before_line.get(&line).copied().unwrap_or(0);
        cumulative_shift[line as usize] = running_shift;
    }

    for index in 0..initial_source_map.get_token_count() {
        let Some(token) = initial_source_map.get_token(index as usize) else {
            continue;
        };
        let original_output_line = token.get_dst_line();
        let shift = cumulative_shift
            .get(original_output_line as usize)
            .copied()
            .unwrap_or_else(|| *cumulative_shift.last().unwrap_or(&0));
        let name_index = token.get_name().map(|name| builder.add_name(name));
        builder.add_raw(
            original_output_line + shift,
            token.get_dst_col(),
            token.get_src_line(),
            token.get_src_col(),
            Some(source_index),
            name_index,
            false,
        );
    }

    for mapping in comment_mappings {
        let name_index = builder.add_name(&mapping.text);
        builder.add_raw(
            mapping.output_line,
            mapping.output_column,
            mapping.go_line,
            mapping.go_column,
            Some(source_index),
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
) -> (String, Vec<CommentMapping>) {
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
) -> (String, Vec<CommentMapping>) {
    let mut output = String::new();
    let mut mappings = Vec::new();
    let mut current_output_line = 0u32;

    for comment in leading_comments {
        mappings.push(CommentMapping {
            go_line: comment.go_line.saturating_sub(1),
            go_column: comment.go_column,
            output_line: current_output_line,
            output_column: 0,
            inserted_before_output_line: 0,
            text: comment.text.clone(),
        });
        output.push_str(&comment.text);
        output.push('\n');
        current_output_line += 1;
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
                    inserted_before_output_line: original_output_line,
                    text: comment.text.clone(),
                });
                output.push_str(&indent_text);
                output.push_str(&comment.text);
                output.push('\n');
                current_output_line += 1;
            }
        }
        output.push_str(line);
        output.push('\n');
        current_output_line += 1;
    }

    (output, mappings)
}
