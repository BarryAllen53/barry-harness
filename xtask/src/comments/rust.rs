use ra_ap_rustc_lexer::{FrontmatterAllowed, TokenKind, strip_shebang, tokenize};

use super::toml;

pub fn comment_lines(source: &str) -> Vec<usize> {
    let mut offset = strip_shebang(source).unwrap_or(0);
    let mut line = 1;
    let mut lines = Vec::new();
    for token in tokenize(&source[offset..], FrontmatterAllowed::Yes) {
        let end = offset + token.len as usize;
        let text = &source[offset..end];
        match token.kind {
            TokenKind::LineComment { .. } | TokenKind::BlockComment { .. } => lines.push(line),
            TokenKind::Frontmatter { .. } => {
                lines.extend(
                    frontmatter_comment_lines(text)
                        .into_iter()
                        .map(|body_line| line + body_line),
                );
            }
            _ => {}
        }
        line += text.matches('\n').count();
        offset = end;
    }
    lines
}

fn frontmatter_comment_lines(frontmatter: &str) -> Vec<usize> {
    let (Some(opening_end), Some(closing_start)) =
        (frontmatter.find('\n'), frontmatter.rfind('\n'))
    else {
        return Vec::new();
    };
    if closing_start <= opening_end {
        return Vec::new();
    }
    toml::comment_lines(&frontmatter[opening_end + 1..closing_start])
}
