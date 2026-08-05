pub(super) fn parse_exported_symbols(source: &str) -> Vec<(String, String)> {
    let source = strip_go_comments(source);
    let mut result = Vec::new();
    let mut group_kind: Option<&str> = None;
    let mut brace_depth = 0usize;
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let at_top_level = brace_depth == 0;
        if let Some(kind) = group_kind {
            if line.starts_with(')') {
                group_kind = None;
                brace_depth = update_go_brace_depth(brace_depth, line);
                continue;
            }
            if let Some((name, _)) = read_identifier(line)
                && is_exported(&name)
            {
                result.push((name, kind.to_string()));
            }
            brace_depth = update_go_brace_depth(brace_depth, line);
            continue;
        }
        if !at_top_level {
            brace_depth = update_go_brace_depth(brace_depth, line);
            continue;
        }
        if matches!(line, "const (" | "var (") {
            group_kind = line.split_whitespace().next();
            brace_depth = update_go_brace_depth(brace_depth, line);
            continue;
        }
        if let Some(rest) = line.strip_prefix("func ") {
            if let Some(after_receiver) = rest.strip_prefix('(')
                && let Some(end_receiver) = after_receiver.find(')')
            {
                let receiver = receiver_type_name(&after_receiver[..end_receiver]);
                let after = after_receiver[end_receiver + 1..].trim_start();
                if let Some((name, _)) = read_identifier(after)
                    && is_exported(&receiver)
                    && is_exported(&name)
                {
                    result.push((format!("{receiver}.{name}"), "method".to_string()));
                }
                brace_depth = update_go_brace_depth(brace_depth, line);
                continue;
            }
            if let Some((name, _)) = read_identifier(rest)
                && is_exported(&name)
            {
                result.push((name, "func".to_string()));
            }
            brace_depth = update_go_brace_depth(brace_depth, line);
            continue;
        }
        for (prefix, kind) in [("type ", "type"), ("const ", "const"), ("var ", "var")] {
            if let Some(rest) = line.strip_prefix(prefix)
                && let Some((name, _)) = read_identifier(rest)
                && is_exported(&name)
            {
                result.push((name, kind.to_string()));
            }
        }
        brace_depth = update_go_brace_depth(brace_depth, line);
    }
    result
}

fn update_go_brace_depth(mut depth: usize, line: &str) -> usize {
    let mut chars = line.chars();
    let mut in_string: Option<char> = None;
    while let Some(ch) = chars.next() {
        if let Some(quote) = in_string {
            if ch == '\\' && quote != '`' {
                chars.next();
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' | '`' => in_string = Some(ch),
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
}

fn receiver_type_name(receiver: &str) -> String {
    receiver
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_start_matches('*')
        .split('[')
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_string()
}

fn read_identifier(source: &str) -> Option<(String, usize)> {
    let mut chars = source.char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, ch) in chars {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    Some((source[..end].to_string(), end))
}

fn is_exported(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn strip_go_comments(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut state = CommentState::Code;
    while let Some(ch) = chars.next() {
        match state {
            CommentState::Code => match (ch, chars.peek().copied()) {
                ('/', Some('/')) => {
                    result.push(' ');
                    result.push(' ');
                    chars.next();
                    state = CommentState::Line;
                }
                ('/', Some('*')) => {
                    result.push(' ');
                    result.push(' ');
                    chars.next();
                    state = CommentState::Block;
                }
                ('"', _) => {
                    result.push(ch);
                    state = CommentState::DoubleQuote;
                }
                ('\'', _) => {
                    result.push(ch);
                    state = CommentState::SingleQuote;
                }
                ('`', _) => {
                    result.push(ch);
                    state = CommentState::RawString;
                }
                _ => result.push(ch),
            },
            CommentState::Line => {
                if ch == '\n' {
                    result.push('\n');
                    state = CommentState::Code;
                } else {
                    result.push(' ');
                }
            }
            CommentState::Block => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    result.push(' ');
                    result.push(' ');
                    chars.next();
                    state = CommentState::Code;
                } else {
                    result.push(if ch == '\n' { '\n' } else { ' ' });
                }
            }
            CommentState::DoubleQuote => {
                result.push(ch);
                if ch == '\\' {
                    if let Some(next) = chars.next() {
                        result.push(next);
                    }
                } else if ch == '"' {
                    state = CommentState::Code;
                }
            }
            CommentState::SingleQuote => {
                result.push(ch);
                if ch == '\\' {
                    if let Some(next) = chars.next() {
                        result.push(next);
                    }
                } else if ch == '\'' {
                    state = CommentState::Code;
                }
            }
            CommentState::RawString => {
                result.push(ch);
                if ch == '`' {
                    state = CommentState::Code;
                }
            }
        }
    }
    result
}

#[derive(Clone, Copy)]
enum CommentState {
    Code,
    Line,
    Block,
    DoubleQuote,
    SingleQuote,
    RawString,
}

pub(super) fn should_compile_file(filename: &str, source: &str) -> bool {
    file_name_matches_target(filename) && build_constraint_matches(source)
}

fn file_name_matches_target(filename: &str) -> bool {
    let stem = filename.strip_suffix(".go").unwrap_or(filename);
    let parts = stem.split('_').collect::<Vec<_>>();
    let Some(last) = parts.last().copied() else {
        return true;
    };
    if goarch_names().contains(&last) {
        if last != "gors" {
            return false;
        }
        let os_part = parts.get(parts.len().saturating_sub(2)).copied();
        return os_part
            .is_none_or(|os_part| !goos_names().contains(&os_part) || os_part == host_goos());
    }
    !goos_names().contains(&last) || last == host_goos()
}

fn build_constraint_matches(source: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(expr) = trimmed.strip_prefix("//go:build ") {
            return eval_build_expr(expr);
        }
        if trimmed.starts_with("//") || trimmed.is_empty() {
            continue;
        }
        break;
    }
    true
}

fn eval_build_expr(expr: &str) -> bool {
    if expr.contains(" || ") {
        return expr.split(" || ").any(eval_build_expr);
    }
    if expr.contains(" && ") {
        return expr.split(" && ").all(eval_build_expr);
    }
    let expr = expr.trim().trim_start_matches('(').trim_end_matches(')');
    if let Some(inner) = expr.strip_prefix('!') {
        return !eval_build_expr(inner);
    }
    build_tag_matches(expr)
}

fn build_tag_matches(tag: &str) -> bool {
    tag == host_goos()
        || tag == "gors"
        || tag == "gc"
        || (tag == "unix" && is_unix_goos(host_goos()))
        || tag
            .strip_prefix("go1.")
            .and_then(|minor| minor.parse::<u32>().ok())
            .is_some_and(|minor| minor <= go_version_minor())
}

fn host_goos() -> &'static str {
    std::env::var("GOOS")
        .ok()
        .filter(|value| !value.is_empty())
        .map(|value| Box::leak(value.into_boxed_str()) as &'static str)
        .unwrap_or_else(|| match std::env::consts::OS {
            "macos" => "darwin",
            other => other,
        })
}

fn go_version_minor() -> u32 {
    gors::GO_VERSION
        .split('.')
        .nth(1)
        .and_then(|minor| minor.parse().ok())
        .unwrap_or(0)
}

fn is_unix_goos(goos: &str) -> bool {
    matches!(
        goos,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "freebsd"
            | "hurd"
            | "illumos"
            | "ios"
            | "linux"
            | "netbsd"
            | "openbsd"
            | "solaris"
    )
}

fn goos_names() -> &'static [&'static str] {
    &[
        "aix",
        "android",
        "darwin",
        "dragonfly",
        "freebsd",
        "hurd",
        "illumos",
        "ios",
        "js",
        "linux",
        "netbsd",
        "openbsd",
        "plan9",
        "solaris",
        "wasip1",
        "windows",
    ]
}

fn goarch_names() -> &'static [&'static str] {
    &[
        "386", "amd64", "arm", "arm64", "loong64", "mips", "mips64", "mips64le", "mipsle", "ppc64",
        "ppc64le", "riscv64", "s390x", "wasm", "gors",
    ]
}
