//! Parsing of runnable fenced code blocks and their generated output regions.
//! Pure and line-based so it is easy to test; the editor maps the returned line
//! spans to rope offsets when splicing.
//!
//! A runnable block is a fenced code block whose info string contains `run` (or
//! `eval`):
//!
//! ````text
//! ```lua run id=sum
//! return 2 + 40
//! ```
//! <!-- doe:output id=sum -->
//! 42
//! <!-- /doe:output -->
//! ````
//!
//! The output region (HTML comments + the generated lines between them) belongs
//! to DOE: it is rewritten on each run and the rest of the document is untouched.

/// Where a block's output is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutMode {
    /// A `doe:output` region right after the block (the default).
    Below,
    /// Run for side effects only; write no output region.
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directives {
    pub run: bool,
    pub auto: bool,
    pub id: Option<String>,
    pub out: OutMode,
    pub lang: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBlock {
    pub directives: Directives,
    pub source: String,
    /// Line of the opening ``` fence.
    pub fence_open_line: usize,
    /// Line of the closing ``` fence.
    pub fence_close_line: usize,
    /// Existing output region as a `[start, end)` line span (the marker lines
    /// inclusive), if one directly follows the block.
    pub output_region: Option<(usize, usize)>,
}

fn fence_len(line: &str) -> usize {
    let t = line.trim_start();
    let c = t.chars().next();
    if c == Some('`') || c == Some('~') {
        let f = c.unwrap();
        let n = t.chars().take_while(|&x| x == f).count();
        if n >= 3 {
            return n;
        }
    }
    0
}

/// The text after the fence characters (the info string), trimmed.
fn info_string(line: &str) -> &str {
    let t = line.trim_start();
    t.trim_start_matches(|c| c == '`' || c == '~').trim()
}

fn parse_info(info: &str) -> Directives {
    let mut tokens = info.split_whitespace();
    let mut lang = tokens.next().unwrap_or("").to_string();
    let mut d = Directives { run: false, auto: false, id: None, out: OutMode::Below, lang: String::new() };
    for tok in tokens {
        match tok {
            "run" | "eval" => d.run = true,
            "auto" => {
                d.auto = true;
                d.run = true;
            }
            _ => {
                if let Some(v) = tok.strip_prefix("id=") {
                    d.id = Some(v.to_string());
                } else if let Some(v) = tok.strip_prefix("lang=") {
                    lang = v.to_string();
                } else if let Some(v) = tok.strip_prefix("out=") {
                    d.out = match v {
                        "hidden" => OutMode::Hidden,
                        _ => OutMode::Below, // below/replace
                    };
                }
            }
        }
    }
    d.lang = lang;
    d
}

fn output_open_id(line: &str) -> Option<Option<String>> {
    let t = line.trim();
    let inner = t.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = inner.strip_prefix("doe:output")?.trim();
    if rest.is_empty() {
        Some(None)
    } else if let Some(v) = rest.strip_prefix("id=") {
        Some(Some(v.trim().to_string()))
    } else {
        Some(None)
    }
}

fn is_output_close(line: &str) -> bool {
    let t = line.trim();
    t.strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .map(|s| s.trim() == "/doe:output")
        .unwrap_or(false)
}

/// Parse all runnable code blocks in `lines`. Non-runnable fenced blocks are
/// skipped (their fences are still consumed so their contents aren't rescanned).
pub fn parse_blocks(lines: &[&str]) -> Vec<ParsedBlock> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let open = fence_len(lines[i]);
        if open == 0 {
            i += 1;
            continue;
        }
        let directives = parse_info(info_string(lines[i]));
        // Find the matching closing fence (same char count or more).
        let mut j = i + 1;
        while j < lines.len() && fence_len(lines[j]) < open {
            j += 1;
        }
        let close = j.min(lines.len().saturating_sub(1));
        let fence_close_line = if j < lines.len() { j } else { close };

        if directives.run {
            let source = if i + 1 <= fence_close_line {
                lines[i + 1..fence_close_line].join("\n")
            } else {
                String::new()
            };
            // An output region may directly follow the closing fence.
            let output_region = parse_output_region(lines, fence_close_line + 1, &directives.id);
            blocks.push(ParsedBlock {
                directives,
                source,
                fence_open_line: i,
                fence_close_line,
                output_region,
            });
            i = output_region.map(|(_, e)| e).unwrap_or(fence_close_line + 1);
        } else {
            i = fence_close_line + 1;
        }
    }
    blocks
}

/// If an output region starts at `at`, return its `[start, end)` line span.
fn parse_output_region(lines: &[&str], at: usize, id: &Option<String>) -> Option<(usize, usize)> {
    let open_id = output_open_id(lines.get(at)?)?;
    // If the block has an id, the region must carry the same id.
    if id.is_some() && open_id != *id {
        return None;
    }
    let mut k = at + 1;
    while k < lines.len() && !is_output_close(lines[k]) {
        k += 1;
    }
    if k < lines.len() {
        Some((at, k + 1))
    } else {
        None
    }
}

/// The runnable block whose fences enclose `line` (cursor on the fence or the
/// source), if any.
pub fn block_at_line(lines: &[&str], line: usize) -> Option<ParsedBlock> {
    parse_blocks(lines)
        .into_iter()
        .find(|b| line >= b.fence_open_line && line <= b.fence_close_line)
}

/// Render an output region's text (the marker lines plus `output`) for a block.
pub fn render_output_region(id: &Option<String>, output: &str) -> String {
    let open = match id {
        Some(id) => format!("<!-- doe:output id={id} -->"),
        None => "<!-- doe:output -->".to_string(),
    };
    let body = output.trim_end_matches('\n');
    if body.is_empty() {
        format!("{open}\n<!-- /doe:output -->")
    } else {
        format!("{open}\n{body}\n<!-- /doe:output -->")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<&str> {
        s.lines().collect()
    }

    #[test]
    fn parses_runnable_block_and_directives() {
        let doc = "# t\n\n```lua run id=sum\nreturn 2 + 40\n```\n\nmore\n";
        let bs = parse_blocks(&lines(doc));
        assert_eq!(bs.len(), 1);
        let b = &bs[0];
        assert!(b.directives.run);
        assert_eq!(b.directives.lang, "lua");
        assert_eq!(b.directives.id.as_deref(), Some("sum"));
        assert_eq!(b.source, "return 2 + 40");
        assert_eq!(b.output_region, None);
    }

    #[test]
    fn non_runnable_block_ignored() {
        let doc = "```lua\nreturn 1\n```\n";
        assert!(parse_blocks(&lines(doc)).is_empty());
    }

    #[test]
    fn finds_existing_output_region() {
        let doc = "```lua run\nreturn 1\n```\n<!-- doe:output -->\nstale\n<!-- /doe:output -->\nafter\n";
        let bs = parse_blocks(&lines(doc));
        assert_eq!(bs.len(), 1);
        // Region spans the three marker/body lines (indices 3,4,5) → [3,6).
        assert_eq!(bs[0].output_region, Some((3, 6)));
    }

    #[test]
    fn id_must_match_output_region() {
        // Output region with a different id is not claimed by this block.
        let doc = "```lua run id=a\nreturn 1\n```\n<!-- doe:output id=b -->\nx\n<!-- /doe:output -->\n";
        assert_eq!(parse_blocks(&lines(doc))[0].output_region, None);
    }

    #[test]
    fn block_at_line_locates_cursor() {
        let doc = "x\n```lua run\nreturn 1\n```\n";
        assert!(block_at_line(&lines(doc), 2).is_some()); // on the source line
        assert!(block_at_line(&lines(doc), 0).is_none());
    }

    #[test]
    fn render_region_roundtrips() {
        assert_eq!(
            render_output_region(&Some("sum".into()), "42\n"),
            "<!-- doe:output id=sum -->\n42\n<!-- /doe:output -->"
        );
        assert_eq!(
            render_output_region(&None, ""),
            "<!-- doe:output -->\n<!-- /doe:output -->"
        );
    }
}
