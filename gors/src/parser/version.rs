/// Extract Go version from `//go:build` directive in source.
///
/// Returns the Go version like `go1.9` if found, or an empty string otherwise.
pub(super) fn extract_go_version(buffer: &str) -> &str {
    // Look for //go:build directive before package declaration
    for line in buffer.lines() {
        let trimmed = line.trim();

        // Stop at package declaration
        if trimmed.starts_with("package ") {
            break;
        }

        // Look for //go:build directive
        if let Some(constraint) = trimmed.strip_prefix("//go:build ") {
            // Find go version constraint (e.g., go1.9, go1.18)
            if let Some(version) = find_go_version_in_constraint(constraint) {
                // Return a reference into the original buffer
                if let Some(pos) = buffer.find(version) {
                    return &buffer[pos..pos + version.len()];
                }
            }
        }
    }
    ""
}

fn is_go_version(s: &str) -> bool {
    if let Some(rest) = s.strip_prefix("go") {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit() || c == '.')
    } else {
        false
    }
}

fn compare_go_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts: Vec<u32> = a
        .strip_prefix("go")
        .unwrap_or("")
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    let b_parts: Vec<u32> = b
        .strip_prefix("go")
        .unwrap_or("")
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    a_parts.cmp(&b_parts)
}

/// Extract effective go version from a build constraint expression.
/// Handles AND (&&), OR (||), NOT (!), and parenthesized groups.
fn find_go_version_in_constraint(constraint: &str) -> Option<&str> {
    // Split into top-level OR branches (respecting parentheses)
    let branches = split_top_level(constraint, b'|');

    let mut result: Option<&str> = None;

    for branch in &branches {
        let branch = branch.trim();
        if branch.is_empty() {
            continue;
        }

        match find_version_in_and_branch(branch) {
            None => return None,
            Some(v) => {
                result = Some(match result {
                    None => v,
                    Some(prev) => {
                        if compare_go_versions(v, prev) == std::cmp::Ordering::Less {
                            v
                        } else {
                            prev
                        }
                    }
                });
            }
        }
    }

    result
}

/// Split a constraint string by a top-level operator (|| or &&).
fn split_top_level(s: &str, op: u8) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;

    while let Some(byte) = bytes.get(i).copied() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            c if c == op && depth == 0 && bytes.get(i + 1).is_some_and(|next| *next == op) => {
                if let Some(part) = s.get(start..i) {
                    parts.push(part);
                }
                i += 2;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if let Some(part) = s.get(start..) {
        parts.push(part);
    }
    parts
}

/// Find the effective go version in an AND branch (no top-level ||).
/// Returns None if the branch has no go version requirement.
fn find_version_in_and_branch(branch: &str) -> Option<&str> {
    let terms = split_top_level(branch, b'&');
    let mut max_version: Option<&str> = None;

    for term in &terms {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }

        // Negated term — skip any go version inside
        if term.starts_with('!') {
            continue;
        }

        // Parenthesized group — recurse
        if term.starts_with('(') && term.ends_with(')') {
            if let Some(v) = find_go_version_in_constraint(&term[1..term.len() - 1]) {
                max_version = Some(match max_version {
                    None => v,
                    Some(prev) => {
                        if compare_go_versions(v, prev) == std::cmp::Ordering::Greater {
                            v
                        } else {
                            prev
                        }
                    }
                });
            }
            continue;
        }

        // Check if this term is a go version
        if is_go_version(term) {
            max_version = Some(match max_version {
                None => term,
                Some(prev) => {
                    if compare_go_versions(term, prev) == std::cmp::Ordering::Greater {
                        term
                    } else {
                        prev
                    }
                }
            });
        }
    }

    max_version
}
