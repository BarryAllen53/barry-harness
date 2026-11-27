pub fn comment_lines(source: &str) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.starts_with('#'))
        .map(|(index, _)| index + 1)
        .collect()
}
