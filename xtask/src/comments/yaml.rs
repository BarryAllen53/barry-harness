#[derive(Clone, Copy, PartialEq, Eq)]
enum Quote {
    None,
    Double,
    Single,
}

#[derive(Clone, Copy)]
enum BlockScalar {
    AwaitingContent { parent_indent: usize },
    Content { indent: usize },
}

#[derive(Clone, Copy)]
struct ScanState {
    quote: Quote,
    flow_depth: usize,
    flow_indent: usize,
}

struct LineScan {
    state: ScanState,
    content_end: usize,
    has_comment: bool,
}

pub fn comment_lines(source: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    let mut state = ScanState {
        quote: Quote::None,
        flow_depth: 0,
        flow_indent: 0,
    };
    let mut block_scalar = None;
    for (index, line) in source.lines().enumerate() {
        if let Some(scalar) = block_scalar {
            if line.trim().is_empty() {
                continue;
            }
            let line_indent = indentation(line);
            match scalar {
                BlockScalar::AwaitingContent { parent_indent } if line_indent > parent_indent => {
                    block_scalar = Some(BlockScalar::Content {
                        indent: line_indent,
                    });
                    continue;
                }
                BlockScalar::Content { indent } if line_indent >= indent => continue,
                _ => block_scalar = None,
            }
        }
        if closes_flow_by_indentation(line, state) {
            state.flow_depth = 0;
        }
        let scan = scan_line(line, state);
        if scan.has_comment {
            lines.push(index + 1);
        }
        state = scan.state;
        if state.quote == Quote::None && state.flow_depth == 0 {
            block_scalar = block_scalar_start(&line[..scan.content_end]);
        }
    }
    lines
}

fn scan_line(line: &str, mut state: ScanState) -> LineScan {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match state.quote {
            Quote::Double => match byte {
                b'\\' => index += 1,
                b'"' => state.quote = Quote::None,
                _ => {}
            },
            Quote::Single if byte == b'\'' => {
                if bytes.get(index + 1) == Some(&b'\'') {
                    index += 1;
                } else {
                    state.quote = Quote::None;
                }
            }
            Quote::Single => {}
            Quote::None => {
                let in_flow = state.flow_depth > 0;
                match byte {
                    b'#' if index == 0 || is_blank(bytes[index - 1]) => {
                        return LineScan {
                            state,
                            content_end: index,
                            has_comment: true,
                        };
                    }
                    b'"' if starts_value(bytes, index, in_flow) => state.quote = Quote::Double,
                    b'\'' if starts_value(bytes, index, in_flow) => state.quote = Quote::Single,
                    b'[' | b'{' if starts_value(bytes, index, in_flow) => {
                        if !in_flow {
                            state.flow_indent = indentation(line);
                        }
                        state.flow_depth += 1;
                    }
                    b']' | b'}' => state.flow_depth = state.flow_depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
        index += 1;
    }
    LineScan {
        state,
        content_end: bytes.len(),
        has_comment: false,
    }
}

fn closes_flow_by_indentation(line: &str, state: ScanState) -> bool {
    let content = line.trim_start_matches(' ');
    state.flow_depth > 0
        && state.quote == Quote::None
        && !content.trim().is_empty()
        && indentation(line) <= state.flow_indent
        && !content.starts_with([']', '}', ','])
}

fn starts_value(bytes: &[u8], index: usize, in_flow: bool) -> bool {
    let mut end = index;
    loop {
        let Some(last) = bytes[..end].iter().rposition(|&byte| !is_blank(byte)) else {
            return true;
        };
        let separated = last + 1 < end;
        let token_start = bytes[..=last]
            .iter()
            .rposition(|&byte| is_blank(byte) || b"[{,".contains(&byte))
            .map_or(0, |position| position + 1);
        if separated && token_start <= last && matches!(bytes[token_start], b'&' | b'!') {
            end = token_start;
            continue;
        }
        return match bytes[last] {
            b'[' | b'{' => true,
            b',' => in_flow,
            b':' => {
                separated
                    || (in_flow
                        && last > 0
                        && matches!(bytes[last - 1], b'"' | b'\'' | b']' | b'}'))
            }
            b'-' if last == 2 && bytes.starts_with(b"---") => separated,
            b'-' | b'?' => separated && (last == 0 || is_blank(bytes[last - 1])),
            _ => false,
        };
    }
}

fn block_scalar_start(content: &str) -> Option<BlockScalar> {
    let mut tokens: Vec<&str> = content.split_whitespace().collect();
    let indicator = tokens.pop()?;
    while tokens
        .last()
        .is_some_and(|token| token.starts_with(['&', '!']))
    {
        tokens.pop();
    }
    let introduced = tokens.last().is_none_or(|&token| {
        token.ends_with(':')
            || token == "-"
            || token == "?"
            || (token == "---" && content.starts_with("---"))
    });
    if !introduced {
        return None;
    }
    let modifiers = indicator.strip_prefix(['|', '>'])?;
    if modifiers.len() > 2 {
        return None;
    }
    let mut explicit_indent = None;
    let mut chomping_seen = false;
    for character in modifiers.chars() {
        match character {
            '+' | '-' if !chomping_seen => chomping_seen = true,
            '1'..='9' if explicit_indent.is_none() => {
                explicit_indent = character.to_digit(10).map(|digit| digit as usize);
            }
            _ => return None,
        }
    }
    let parent_indent = parent_indent(content);
    Some(match explicit_indent {
        Some(offset) => BlockScalar::Content {
            indent: parent_indent + offset,
        },
        None => BlockScalar::AwaitingContent { parent_indent },
    })
}

fn parent_indent(content: &str) -> usize {
    let mut column = indentation(content);
    let mut last_dash_column = None;
    while content[column..].starts_with('-') && content[column + 1..].starts_with([' ', '\t']) {
        last_dash_column = Some(column);
        column += 1;
        column += content[column..].len() - content[column..].trim_start().len();
    }
    if content[column..].starts_with(['|', '>', '&', '!']) {
        last_dash_column.unwrap_or(column)
    } else {
        column
    }
}

fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}
