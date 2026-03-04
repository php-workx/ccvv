//! Table extraction helpers for UI cell picking.
//!
//! Parses terminal box tables, markdown pipe tables, and delimiter tables
//! into a canonical row/column matrix.

use std::collections::HashMap;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TableFormat {
    Terminal,
    Markdown,
    Delimiter,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableExtraction {
    pub detected: bool,
    pub format: Option<TableFormat>,
    pub confidence: f32,
    pub rows: Vec<Vec<String>>,
    pub warnings: Vec<String>,
}

impl TableExtraction {
    fn none() -> Self {
        TableExtraction {
            detected: false,
            format: None,
            confidence: 0.0,
            rows: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn apply_gate(mut self, enabled: bool, min_confidence: f32) -> Self {
        if !enabled {
            self.detected = false;
            self.format = None;
            self.rows.clear();
            self.warnings
                .push("table picker disabled by configuration".to_string());
            return self;
        }

        if self.detected && self.confidence < min_confidence {
            self.detected = false;
            self.format = None;
            self.rows.clear();
            self.warnings.push(format!(
                "table confidence {:.2} below threshold {:.2}",
                self.confidence, min_confidence
            ));
        }

        self
    }
}

#[derive(Debug, Clone)]
struct ParseCandidate {
    format: TableFormat,
    confidence: f32,
    rows: Vec<Vec<String>>,
    warnings: Vec<String>,
}

pub fn extract_table(input: &str) -> TableExtraction {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return TableExtraction::none();
    }

    let mut candidates = Vec::new();
    if let Some(candidate) = parse_terminal_box(trimmed) {
        candidates.push(candidate);
    }
    if let Some(candidate) = parse_terminal_flattened(trimmed) {
        candidates.push(candidate);
    }
    if let Some(candidate) = parse_aligned_pipe_columns(trimmed) {
        candidates.push(candidate);
    }
    if let Some(candidate) = parse_markdown(trimmed) {
        candidates.push(candidate);
    }
    if let Some(candidate) = parse_delimited(trimmed) {
        candidates.push(candidate);
    }

    let best = candidates.into_iter().max_by(|a, b| {
        a.confidence
            .partial_cmp(&b.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let a_cells = a.rows.len() * a.rows.first().map(|r| r.len()).unwrap_or(0);
                let b_cells = b.rows.len() * b.rows.first().map(|r| r.len()).unwrap_or(0);
                a_cells.cmp(&b_cells)
            })
    });

    if let Some(best) = best {
        TableExtraction {
            detected: true,
            format: Some(best.format),
            confidence: best.confidence,
            rows: best.rows,
            warnings: best.warnings,
        }
    } else {
        TableExtraction::none()
    }
}

fn parse_terminal_flattened(input: &str) -> Option<ParseCandidate> {
    if !looks_like_flattened_box(input) {
        return None;
    }

    let cols_hint = detect_box_column_hint(input);
    let mut normalized = input.to_string();
    normalized = normalized.replace("│ │", "│\n│");
    normalized = normalized.replace(" │ ├", " │\n├");
    normalized = normalized.replace(" │ └", " │\n└");
    normalized = normalized.replace(" │ ┌", " │\n┌");
    normalized = normalized.replace("┤ │", "┤\n│");
    normalized = normalized.replace("┼ │", "┼\n│");
    normalized = normalized.replace("┘ │", "┘\n│");
    normalized = normalized.replace("┐ │", "┐\n│");
    normalized = normalized.replace(" ┤ │ ", " ┤\n│ ");
    normalized = normalized.replace(" ┼ │ ", " ┼\n│ ");
    normalized = insert_newlines_around_inline_borders(&normalized);

    // Split lines that contain multiple flattened row chunks (common when copied from terminal).
    let mut rebuilt_lines: Vec<String> = Vec::new();
    for line in normalized.lines() {
        let chunks = split_row_chunks_by_bar_groups(line, cols_hint);
        if chunks.is_empty() {
            rebuilt_lines.push(line.to_string());
        } else {
            rebuilt_lines.extend(chunks);
        }
    }
    normalized = rebuilt_lines.join("\n");

    if normalized == input {
        return None;
    }

    let mut parsed = parse_terminal_box(&normalized)?;
    parsed.confidence = (parsed.confidence - 0.08).clamp(0.0, 0.99);
    parsed
        .warnings
        .push("reconstructed flattened terminal rows".to_string());
    Some(parsed)
}

fn insert_newlines_around_inline_borders(input: &str) -> String {
    static BEFORE_BORDER_RE: OnceLock<regex::Regex> = OnceLock::new();
    static AFTER_BORDER_RE: OnceLock<regex::Regex> = OnceLock::new();

    let before_re = BEFORE_BORDER_RE.get_or_init(|| {
        regex::Regex::new(r"([│|])\s+([├└┌┬┴┼])")
            .expect("valid regex for inline border split before")
    });
    let after_re = AFTER_BORDER_RE.get_or_init(|| {
        regex::Regex::new(r"([┤┼┘┐])\s+([│|])")
            .expect("valid regex for inline border split after")
    });

    let stage1 = before_re.replace_all(input, "$1\n$2").to_string();
    after_re.replace_all(&stage1, "$1\n$2").to_string()
}

fn looks_like_flattened_box(input: &str) -> bool {
    let vertical_count = input.matches('│').count() + input.matches('|').count();
    if vertical_count < 6 {
        return false;
    }
    let line_count = input.lines().count();
    let has_dense_bar_line = input.lines().any(|line| {
        let bars = line.matches('│').count() + line.matches('|').count();
        bars >= 6
    });
    let has_inline_border = input.lines().any(|line| {
        (line.contains('│') || line.contains('|'))
            && (line.contains('├')
                || line.contains('┤')
                || line.contains('┼')
                || line.contains('└')
                || line.contains('┌'))
    });
    line_count <= 2
        || input.contains("│ │")
        || input.contains("| |")
        || input.contains("│ ├")
        || input.contains("┤ │")
        || has_dense_bar_line
        || has_inline_border
}

fn detect_box_column_hint(input: &str) -> Option<usize> {
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let has_box = trimmed
            .chars()
            .any(|c| "┌┐└┘├┤┬┴┼─╭╮╰╯═╪║╞╡╟╢".contains(c));
        let has_text = trimmed.chars().any(char::is_alphanumeric);
        if !has_box || has_text {
            continue;
        }

        let splits = trimmed.matches('┬').count()
            + trimmed.matches('┼').count()
            + trimmed.matches('┴').count();
        if splits >= 1 {
            return Some(splits + 1);
        }
    }
    None
}

fn split_row_chunks_by_bar_groups(line: &str, cols_hint: Option<usize>) -> Vec<String> {
    let bars: Vec<(usize, char)> = line
        .char_indices()
        .filter(|(_, ch)| *ch == '│' || *ch == '|')
        .collect();
    let Some(cols) = cols_hint else {
        return Vec::new();
    };
    let bars_per_row = cols + 1;
    if bars_per_row < 3 {
        return Vec::new();
    }
    if bars.len() < bars_per_row * 2 {
        return Vec::new();
    }
    if !bars.len().is_multiple_of(bars_per_row) {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let row_count = bars.len() / bars_per_row;
    for row_idx in 0..row_count {
        let start_bar = bars[row_idx * bars_per_row];
        let end_bar = bars[row_idx * bars_per_row + bars_per_row - 1];
        let start = start_bar.0;
        let end = end_bar.0 + end_bar.1.len_utf8();
        if start < end && end <= line.len() {
            let chunk = line[start..end].trim_end();
            if chunk.chars().any(|c| c == '│' || c == '|') {
                chunks.push(chunk.to_string());
            }
        }
    }
    chunks
}

fn parse_aligned_pipe_columns(input: &str) -> Option<ParseCandidate> {
    let lines: Vec<&str> = input
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() < 2 {
        return None;
    }

    let mut bars_by_line: Vec<(&str, Vec<usize>)> = Vec::new();
    for line in &lines {
        let bars = bar_positions(line);
        if bars.len() >= 3 {
            bars_by_line.push((line, bars));
        }
    }
    if bars_by_line.len() < 2 {
        return None;
    }

    let mut freq: HashMap<usize, usize> = HashMap::new();
    for (_, bars) in &bars_by_line {
        *freq.entry(bars.len()).or_insert(0) += 1;
    }
    let (&bar_count, &match_count) = freq.iter().max_by_key(|(_, count)| *count)?;
    if bar_count < 3 || match_count < 2 {
        return None;
    }

    let aligned_candidates: Vec<(&str, &Vec<usize>)> = bars_by_line
        .iter()
        .filter(|(_, bars)| bars.len() == bar_count)
        .map(|(line, bars)| (*line, bars))
        .collect();
    if aligned_candidates.len() < 2 {
        return None;
    }

    // Constant visual column boundaries: each bar slot should be near same position.
    const SLOT_DRIFT_TOLERANCE: usize = 6;
    for slot in 0..bar_count {
        let mut min_pos = usize::MAX;
        let mut max_pos = 0usize;
        for (_, bars) in &aligned_candidates {
            min_pos = min_pos.min(bars[slot]);
            max_pos = max_pos.max(bars[slot]);
        }
        if max_pos.saturating_sub(min_pos) > SLOT_DRIFT_TOLERANCE {
            return None;
        }
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    for (line, _) in &aligned_candidates {
        if is_box_separator_line(line) {
            continue;
        }
        if let Some(cells) = split_box_cells(line) {
            if cells.len() >= 2 {
                rows.push(cells);
            }
        }
    }
    if rows.len() < 2 {
        return None;
    }

    // Require content in at least two distinct columns to avoid false positives.
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count < 2 {
        return None;
    }
    let mut cols_with_text = 0usize;
    for c in 0..col_count {
        if rows
            .iter()
            .any(|row| row.get(c).map(|v| !v.trim().is_empty()).unwrap_or(false))
        {
            cols_with_text += 1;
        }
    }
    if cols_with_text < 2 {
        return None;
    }

    let mut confidence = 0.68f32;
    if aligned_candidates.len() >= 3 {
        confidence += 0.05;
    }
    if input.contains('│') {
        confidence += 0.03;
    }
    if input.contains('├') || input.contains('┼') || input.contains('┤') {
        // If explicit border separators exist, prefer dedicated box-table parsers.
        confidence -= 0.06;
    }
    confidence = confidence.clamp(0.0, 0.99);

    Some(ParseCandidate {
        format: TableFormat::Terminal,
        confidence,
        rows,
        warnings: vec!["inferred table from aligned pipe columns".to_string()],
    })
}

fn bar_positions(line: &str) -> Vec<usize> {
    line.char_indices()
        .filter(|(_, ch)| *ch == '│' || *ch == '|')
        .map(|(idx, _)| idx)
        .collect()
}

fn parse_terminal_box(input: &str) -> Option<ParseCandidate> {
    let raw_lines: Vec<&str> = input.lines().collect();
    let lines = normalize_box_lines_for_clipped_edges(&raw_lines);
    if lines.len() < 2 {
        return None;
    }

    let mut warnings = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current: Option<Vec<String>> = None;
    let mut expected_cols = 0usize;
    let mut saw_separator = false;
    let mut cell_lines = 0usize;

    for line in &lines {
        let trimmed = line.trim_end();

        if is_box_separator_line(trimmed) {
            saw_separator = true;
            finalize_row(&mut rows, &mut current);
            continue;
        }

        let Some(cells) = split_box_cells(trimmed) else {
            continue;
        };
        if cells.len() < 2 {
            continue;
        }

        cell_lines += 1;
        if expected_cols == 0 {
            expected_cols = cells.len();
        }

        let mut normalized = cells;
        if normalized.len() != expected_cols {
            warnings.push(format!(
                "line had {} columns, expected {}",
                normalized.len(),
                expected_cols
            ));
            if normalized.len() > expected_cols {
                normalized.truncate(expected_cols);
            } else {
                normalized.resize(expected_cols, String::new());
            }
        }

        if current.is_none() {
            current = Some(vec![String::new(); expected_cols]);
        }

        if let Some(cur) = current.as_mut() {
            for (idx, part) in normalized.iter().enumerate() {
                let value = part.trim();
                if value.is_empty() {
                    continue;
                }
                if !cur[idx].is_empty() {
                    cur[idx].push(' ');
                }
                cur[idx].push_str(value);
            }
        }
    }
    finalize_row(&mut rows, &mut current);

    rows.retain(|row| row.iter().any(|cell| !cell.trim().is_empty()));
    if rows.is_empty() || expected_cols < 2 {
        return None;
    }

    // Accept a single wrapped row as a table candidate when there is enough structure.
    if rows.len() == 1 && !saw_separator && cell_lines < 2 {
        return None;
    }

    let mut confidence = 0.72f32;
    if saw_separator {
        confidence += 0.12;
    }
    if cell_lines >= rows.len() {
        confidence += 0.08;
    }
    if !warnings.is_empty() {
        confidence -= 0.1;
    }
    confidence = confidence.clamp(0.0, 0.99);

    Some(ParseCandidate {
        format: TableFormat::Terminal,
        confidence,
        rows,
        warnings,
    })
}

fn normalize_box_lines_for_clipped_edges(lines: &[&str]) -> Vec<String> {
    let mut freq: HashMap<usize, usize> = HashMap::new();
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with('│') || trimmed.starts_with('|') {
            let bars = bar_positions(line).len();
            if bars >= 3 {
                *freq.entry(bars).or_insert(0) += 1;
            }
        }
    }

    let expected_bars = freq.iter().max_by_key(|(_, count)| *count).map(|(bars, _)| *bars);
    let Some(expected_bars) = expected_bars else {
        return lines.iter().map(|line| (*line).to_string()).collect();
    };

    lines
        .iter()
        .map(|line| {
            let trimmed = line.trim_start();
            let bars = bar_positions(line).len();
            let starts_with_bar = trimmed.starts_with('│') || trimmed.starts_with('|');
            let likely_clipped_left = !starts_with_bar && bars >= 2 && bars + 1 == expected_bars;
            if likely_clipped_left {
                format!("│ {}", trimmed)
            } else {
                (*line).to_string()
            }
        })
        .collect()
}

fn finalize_row(rows: &mut Vec<Vec<String>>, current: &mut Option<Vec<String>>) {
    let Some(row) = current.take() else {
        return;
    };
    if row.iter().all(|cell| cell.trim().is_empty()) {
        return;
    }
    rows.push(row);
}

fn is_box_separator_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    let has_box_chars = trimmed.chars().any(|c| "┌┐└┘├┤┬┴┼─╭╮╰╯═╪║╞╡╟╢".contains(c));
    let has_ascii_sep = trimmed
        .chars()
        .all(|c| c == '+' || c == '-' || c == '|' || c == ' ');
    let has_text = trimmed.chars().any(char::is_alphanumeric);

    (has_box_chars || has_ascii_sep) && !has_text
}

fn split_box_cells(line: &str) -> Option<Vec<String>> {
    let mut bars: Vec<(usize, char)> = line
        .char_indices()
        .filter(|(_, ch)| *ch == '│' || *ch == '|')
        .collect();

    if bars.len() < 2 {
        return None;
    }

    // Ignore edge sentinels from split lines that do not contain enclosed cells.
    bars.sort_by_key(|(idx, _)| *idx);
    let mut cells = Vec::new();
    for pair in bars.windows(2) {
        let (left_idx, left_char) = pair[0];
        let (right_idx, _) = pair[1];
        let start = left_idx + left_char.len_utf8();
        if start > right_idx || right_idx > line.len() {
            continue;
        }
        cells.push(line[start..right_idx].trim().to_string());
    }

    if cells.is_empty() {
        None
    } else {
        Some(cells)
    }
}

fn parse_markdown(input: &str) -> Option<ParseCandidate> {
    let lines: Vec<&str> = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && line.contains('|'))
        .collect();
    if lines.len() < 2 {
        return None;
    }

    let mut rows = Vec::new();
    let mut expected_cols = 0usize;
    let mut saw_separator = false;

    for line in lines {
        let cells = parse_markdown_cells(line);
        if cells.len() < 2 {
            continue;
        }
        if is_markdown_separator_row(&cells) {
            saw_separator = true;
            continue;
        }
        if expected_cols == 0 {
            expected_cols = cells.len();
        }
        if cells.len() == expected_cols {
            rows.push(cells);
        }
    }

    if rows.len() < 2 || expected_cols < 2 {
        return None;
    }

    Some(ParseCandidate {
        format: TableFormat::Markdown,
        confidence: if saw_separator { 0.93 } else { 0.78 },
        rows,
        warnings: Vec::new(),
    })
}

fn parse_markdown_cells(line: &str) -> Vec<String> {
    let mut text = line.trim();
    if let Some(rest) = text.strip_prefix('|') {
        text = rest;
    }
    if let Some(rest) = text.strip_suffix('|') {
        text = rest;
    }

    let mut out = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '|' {
            out.push(cur.trim().to_string());
            cur.clear();
            continue;
        }
        cur.push(ch);
    }
    out.push(cur.trim().to_string());
    out
}

fn is_markdown_separator_row(cells: &[String]) -> bool {
    if cells.is_empty() {
        return false;
    }
    cells.iter().all(|cell| {
        let token = cell.trim();
        if token.is_empty() {
            return false;
        }
        let core = token.trim_matches(':');
        core.len() >= 3 && core.chars().all(|ch| ch == '-')
    })
}

fn parse_delimited(input: &str) -> Option<ParseCandidate> {
    let lines: Vec<&str> = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.len() < 2 {
        return None;
    }

    let mut best: Option<ParseCandidate> = None;
    for delimiter in ['\t', ',', ';'] {
        let split_lines: Vec<Vec<String>> = lines
            .iter()
            .map(|line| split_csv_line(line, delimiter))
            .collect();
        if split_lines.is_empty() {
            continue;
        }

        let mut freq: HashMap<usize, usize> = HashMap::new();
        for row in &split_lines {
            *freq.entry(row.len()).or_insert(0) += 1;
        }
        let Some((&most_common_cols, &count)) = freq.iter().max_by_key(|(_, c)| *c) else {
            continue;
        };
        if most_common_cols < 2 {
            continue;
        }
        if count * 100 / split_lines.len() < 80 {
            continue;
        }

        let rows: Vec<Vec<String>> = split_lines
            .into_iter()
            .filter(|row| row.len() == most_common_cols)
            .collect();
        if rows.len() < 2 {
            continue;
        }

        let confidence = match delimiter {
            '\t' => 0.92,
            ',' => 0.84,
            ';' => 0.83,
            _ => 0.8,
        };

        let candidate = ParseCandidate {
            format: TableFormat::Delimiter,
            confidence,
            rows,
            warnings: Vec::new(),
        };

        if let Some(existing) = &best {
            if candidate.confidence > existing.confidence {
                best = Some(candidate);
            }
        } else {
            best = Some(candidate);
        }
    }

    best
}

pub fn split_csv_line(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == delimiter {
            fields.push(current.clone());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    fields.push(current);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_terminal_table_with_wrapped_cells() {
        let input = r#"
├─────────────────────────┼────────────────────────────────────────────────────────────┤
│ Review if max replica   │ Still applies — you're capped at max_containers=3. Worth   │
│ limit is sufficient     │ reviewing as you grow.                                      │
├─────────────────────────┼────────────────────────────────────────────────────────────┤
│ Keep spare instances    │ Provided by Modal — buffer_containers=2 pre-warms          │
│                         │ containers.                                                  │
├─────────────────────────┼────────────────────────────────────────────────────────────┤
"#;
        let parsed = extract_table(input);
        assert!(parsed.detected);
        assert_eq!(parsed.format, Some(TableFormat::Terminal));
        assert_eq!(parsed.rows.len(), 2);
        assert_eq!(
            parsed.rows[0][1],
            "Still applies — you're capped at max_containers=3. Worth reviewing as you grow."
        );
    }

    #[test]
    fn extracts_markdown_table() {
        let input = "| Org | Cleaned Up |\n| --- | --- |\n| doc | value |";
        let parsed = extract_table(input);
        assert!(parsed.detected);
        assert_eq!(parsed.format, Some(TableFormat::Markdown));
        assert_eq!(parsed.rows.len(), 2);
        assert_eq!(parsed.rows[1][1], "value");
    }

    #[test]
    fn extracts_tsv_table() {
        let input = "name\tstatus\nmodal\tok\nskypilot\tcold";
        let parsed = extract_table(input);
        assert!(parsed.detected);
        assert_eq!(parsed.format, Some(TableFormat::Delimiter));
        assert_eq!(parsed.rows.len(), 3);
        assert_eq!(parsed.rows[2][1], "cold");
    }

    #[test]
    fn extracts_flattened_terminal_table_line() {
        let input = "│ Review if max replica │ Still applies -- you're capped at `max_containers=3`. Worth │ │ limit is sufficient │ reviewing as you grow. │ ├─────────────────────────┼────────────────────────────────────────────────────────────┤ │ Reduce/optimize │ Largely solved by Modal -- ~30s cold start vs 5-7 min with  │ │ provisioning times │ `SkyPilot`. Modal's memory snapshots (@modal.enter split) │ │ │ help here too. │";
        let parsed = extract_table(input);
        assert!(parsed.detected, "expected detection, got: {:?}", parsed);
        assert_eq!(parsed.format, Some(TableFormat::Terminal));
        let joined = parsed.rows[0].join(" | ");
        assert!(
            joined.contains("Still applies -- you're capped at `max_containers=3`. Worth reviewing as you grow."),
            "rows: {:?}",
            parsed.rows
        );
    }

    #[test]
    fn extracts_single_tap_terminal_copy_with_inline_borders() {
        let input = r#"
┌──────────────────────────────┬───────────────────────────────────────────────────────┐
│          Mitigation          │                    Status on Modal                    │     ├──────────────────────────────┼───────────────────────────────────────────────────────┤
│                              │ Less critical — Modal manages container lifecycle.    │     │ Daily resource review        │ Containers shut down after timeout=600 and your       │
│                              │ evening cron scales to 0.                             │     ├──────────────────────────────┼───────────────────────────────────────────────────────┤
│ Automated                    │ Handled by Modal — Modal won't leave orphaned VMs     │
│ monitoring/reconciliation    │ running. However, orphaned Modal apps or forgotten    │     │                              │ deployments could still leak cost.                    │
"#;
        let parsed = extract_table(input);
        assert!(parsed.detected, "expected detection, got: {:?}", parsed);
        assert_eq!(parsed.format, Some(TableFormat::Terminal));
        assert!(
            parsed.rows.iter().any(|r| {
                r.get(1)
                    .map(|v| v.contains("Less critical — Modal manages container lifecycle."))
                    .unwrap_or(false)
            }),
            "rows: {:?}",
            parsed.rows
        );
    }

    #[test]
    fn extracts_aligned_pipe_cells_without_separators() {
        let input = r#"
  │ Memory snapshot    │ @modal.enter snapshot may become  │ [M] Periodic forced re-snapshot (redeploy) [S]     │
  │ drift              │ stale, causing subtle bugs after  │ Test snapshot restore in staging                   │
  │                    │ Modal infra updates               │                                                    │
"#;
        let parsed = extract_table(input);
        assert!(parsed.detected, "expected detection, got: {:?}", parsed);
        assert_eq!(parsed.format, Some(TableFormat::Terminal));
        assert!(
            parsed.rows
                .iter()
                .any(|r| r.get(0).map(|v| v.contains("Memory snapshot")).unwrap_or(false)),
            "rows: {:?}",
            parsed.rows
        );
        assert!(
            parsed.rows
                .iter()
                .any(|r| r.get(1).map(|v| v.contains("Modal infra updates")).unwrap_or(false)),
            "rows: {:?}",
            parsed.rows
        );
    }

    #[test]
    fn extracts_single_wrapped_row_table() {
        let input = r#"
  │ production-web-server │ Port 8080 SG rule removed entirely (open: false   │
  │                       │ on phpMyAdmin listener)                           │
"#;
        let parsed = extract_table(input);
        assert!(parsed.detected, "expected detection, got: {:?}", parsed);
        assert_eq!(parsed.format, Some(TableFormat::Terminal));
        assert_eq!(parsed.rows.len(), 1, "rows: {:?}", parsed.rows);
        assert!(
            parsed.rows[0][0].contains("production-web-server"),
            "rows: {:?}",
            parsed.rows
        );
        assert!(
            parsed.rows[0][1]
                .contains("Port 8080 SG rule removed entirely (open: false on phpMyAdmin listener)"),
            "rows: {:?}",
            parsed.rows
        );
    }

    #[test]
    fn extracts_wrapped_row_with_clipped_left_border() {
        let input = r#"
production-web-server │ Port 8080 SG rule removed entirely (open: false   │
  │                       │ on phpMyAdmin listener)                           │
"#;
        let parsed = extract_table(input);
        assert!(parsed.detected, "expected detection, got: {:?}", parsed);
        assert_eq!(parsed.format, Some(TableFormat::Terminal));
        assert_eq!(parsed.rows.len(), 1, "rows: {:?}", parsed.rows);
        assert!(
            parsed.rows[0][0].contains("production-web-server"),
            "rows: {:?}",
            parsed.rows
        );
        assert!(
            parsed.rows[0][1]
                .contains("Port 8080 SG rule removed entirely (open: false on phpMyAdmin listener)"),
            "rows: {:?}",
            parsed.rows
        );
    }

    #[test]
    fn gate_disables_detection() {
        let input = "name\tstatus\nmodal\tok";
        let parsed = extract_table(input).apply_gate(false, 0.75);
        assert!(!parsed.detected);
        assert!(parsed.rows.is_empty());
    }
}
