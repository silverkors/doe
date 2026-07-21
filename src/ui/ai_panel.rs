//! The AI providers panel — a modal (palette: "AI: Providers…") to configure
//! model providers without hand-editing files. Add a provider from a preset
//! kind, set its API key/model, mark the chat default, or remove it; changes
//! are written to `ai.toml` and take effect immediately (the dispatcher is
//! rebuilt on close).

use super::screen::{Cell, Screen};
use crate::ai::config::AiConfig;
use crate::app::App;
use crossterm::style::Color;

/// Stable, sorted provider names — the panel's row order.
pub fn provider_names(ai: &AiConfig) -> Vec<String> {
    let mut names: Vec<String> = ai.providers.keys().cloned().collect();
    names.sort();
    names
}

#[derive(Default)]
pub struct AiPanel {
    pub open: bool,
    pub selected: usize,
}

impl AiPanel {
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
    }
    pub fn close(&mut self) {
        self.open = false;
    }
    /// Number of navigable rows: one per provider plus the trailing "Add" row.
    pub fn row_count(ai: &AiConfig) -> usize {
        ai.providers.len() + 1
    }
    pub fn move_selection(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let n = len as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }
}

fn fill(screen: &mut Screen, x0: u16, y: u16, width: u16, bg: Color) {
    for x in x0..x0 + width {
        screen.set(x, y, Cell { ch: ' ', fg: bg, bg, bold: false, italic: false });
    }
}

pub fn render(screen: &mut Screen, app: &App) {
    if app.width < 40 || app.height < 12 {
        return;
    }
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let border = theme.line_number_current;
    let ai = &app.config.ai;
    let names = provider_names(ai);
    let chat_default = ai.defaults.get("chat").cloned();

    let width = app.width.saturating_sub(6).min(72).max(40);
    let x0 = (app.width - width) / 2;
    let y0 = 2u16;
    let inner = (width - 2) as usize;
    // Rows = providers + the "Add" affordance.
    let total = names.len() + 1;
    let rows = total.min((app.height as usize).saturating_sub(7)) as u16;

    let mut top = String::from("┌ AI Providers ");
    while top.chars().count() < (width - 1) as usize {
        top.push('─');
    }
    top.push('┐');
    fill(screen, x0, y0, width, panel_bg);
    screen.put_str(x0, y0, &top, border, panel_bg, false, false);

    let top_idx = scroll_top(app.ai_panel.selected, rows as usize, total);
    for r in 0..rows {
        let i = top_idx + r as usize;
        if i >= total {
            break;
        }
        let y = y0 + 1 + r;
        let selected = i == app.ai_panel.selected;
        let row_bg = if selected { theme.selection } else { panel_bg };
        fill(screen, x0, y, width, row_bg);
        screen.put_str(x0, y, "│", border, panel_bg, false, false);
        screen.put_str(x0 + width - 1, y, "│", border, panel_bg, false, false);
        let caret = if selected { "▶ " } else { "  " };
        screen.put_str(x0 + 1, y, caret, theme.keyword, row_bg, false, false);

        if i == names.len() {
            // The "Add" row.
            screen.put_str(x0 + 3, y, "＋ Add provider…", theme.keyword, row_bg, false, false);
            continue;
        }

        let name = &names[i];
        let pc = &ai.providers[name];
        let is_default = chat_default.as_deref() == Some(name.as_str());

        // ★ default marker · name · (kind) · model · key status
        let star = if is_default { "★ " } else { "  " };
        screen.put_str(x0 + 3, y, star, theme.keyword, row_bg, false, false);
        screen.put_str(x0 + 5, y, name, theme.foreground, row_bg, true, false);

        let model = pc.model.clone().unwrap_or_else(|| "(preset)".into());
        let has_key = pc.api_key.as_deref().map(|k| !k.is_empty()).unwrap_or(false)
            || pc.api_key_env.is_some()
            || matches!(pc.kind.as_str(), "ollama" | "lmstudio");
        let key = if has_key { "" } else { "  ⚠ no key" };
        let meta = format!("{}  ·  {}{}", pc.kind, model, key);
        let mx = x0 + 5 + name.chars().count() as u16 + 2;
        if (mx as usize) < x0 as usize + inner {
            screen.put_str(mx, y, &meta, theme.comment, row_bg, false, false);
        }
    }

    // Footer.
    let fy = y0 + 1 + rows;
    let mut sep = String::from("├");
    for _ in 0..inner {
        sep.push('─');
    }
    sep.push('┤');
    fill(screen, x0, fy, width, panel_bg);
    screen.put_str(x0, fy, &sep, border, panel_bg, false, false);

    let hy = fy + 1;
    fill(screen, x0, hy, width, panel_bg);
    screen.put_str(x0, hy, "│", border, panel_bg, false, false);
    screen.put_str(x0 + width - 1, hy, "│", border, panel_bg, false, false);
    let hint = "↑↓ move · a add · k key · m model · enter default · d remove · esc save";
    screen.put_str(x0 + 2, hy, hint, theme.comment, panel_bg, false, false);

    let mut bot = String::from("└");
    for _ in 0..inner {
        bot.push('─');
    }
    bot.push('┘');
    let by = hy + 1;
    fill(screen, x0, by, width, panel_bg);
    screen.put_str(x0, by, &bot, border, panel_bg, false, false);
}

fn scroll_top(selected: usize, rows: usize, len: usize) -> usize {
    if len <= rows || selected < rows {
        0
    } else {
        (selected + 1 - rows).min(len - rows)
    }
}
