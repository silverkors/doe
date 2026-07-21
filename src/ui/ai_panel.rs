//! The AI providers panel — a modal (palette: "AI: Providers…") to configure
//! model providers without hand-editing files. Three modes:
//!
//! 1. **List** — the configured providers, with an "Add" row.
//! 2. **PickKind** — a dialog to choose a provider kind (a known preset, or
//!    "custom").
//! 3. **Form** — a field form to fill in name / key / model (and base URL +
//!    protocol for custom), edited inline in the modal.
//!
//! Changes are written to `ai.toml` and take effect immediately (the dispatcher
//! is rebuilt on close). Key handling lives in `App::handle_ai_panel_key`.

use super::screen::{Cell, Screen};
use crate::ai::config::AiConfig;
use crate::ai::presets::preset;
use crate::app::App;
use crossterm::style::Color;

/// The provider kinds offered in the PickKind dialog, with a one-line label.
pub const KINDS: &[(&str, &str)] = &[
    ("anthropic", "Claude — Opus 4.8, Fable 5"),
    ("openai", "OpenAI — GPT-4o and others"),
    ("openrouter", "OpenRouter — many models, one key"),
    ("xai", "xAI — Grok"),
    ("groq", "Groq — fast open models"),
    ("mistral", "Mistral"),
    ("ollama", "Ollama — local, no key"),
    ("lmstudio", "LM Studio — local, no key"),
    ("custom", "Custom — fill in everything yourself"),
];

/// Which panel view is active.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    PickKind,
    Form,
}

/// A form field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Name,
    Key,
    Model,
    BaseUrl,
    Protocol,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Field::Name => "Name",
            Field::Key => "API key",
            Field::Model => "Model",
            Field::BaseUrl => "Base URL",
            Field::Protocol => "Protocol",
        }
    }
    fn secret(self) -> bool {
        matches!(self, Field::Key)
    }
}

/// The add/edit form state.
pub struct Form {
    pub kind: String,
    /// The existing provider name when editing (for rename/replace); `None`
    /// when adding.
    pub editing: Option<String>,
    pub name: String,
    pub key: String,
    pub model: String,
    pub base_url: String,
    pub protocol: String,
    pub needs_key: bool,
    /// Index into [`Form::fields`] of the focused field.
    pub focus: usize,
}

impl Form {
    /// The fields shown for this kind: keyless local providers omit the key,
    /// custom adds base URL + protocol.
    pub fn fields(&self) -> Vec<Field> {
        let mut f = vec![Field::Name];
        if self.needs_key {
            f.push(Field::Key);
        }
        f.push(Field::Model);
        if self.kind == "custom" {
            f.push(Field::BaseUrl);
            f.push(Field::Protocol);
        }
        f
    }
    pub fn focused(&self) -> Field {
        let fs = self.fields();
        fs[self.focus.min(fs.len() - 1)]
    }
    pub fn value(&self, f: Field) -> &str {
        match f {
            Field::Name => &self.name,
            Field::Key => &self.key,
            Field::Model => &self.model,
            Field::BaseUrl => &self.base_url,
            Field::Protocol => &self.protocol,
        }
    }
    pub fn value_mut(&mut self, f: Field) -> &mut String {
        match f {
            Field::Name => &mut self.name,
            Field::Key => &mut self.key,
            Field::Model => &mut self.model,
            Field::BaseUrl => &mut self.base_url,
            Field::Protocol => &mut self.protocol,
        }
    }
    pub fn move_focus(&mut self, delta: isize) {
        let n = self.fields().len() as isize;
        self.focus = (((self.focus as isize + delta) % n + n) % n) as usize;
    }
}

/// Stable, sorted provider names — the List view's row order.
pub fn provider_names(ai: &AiConfig) -> Vec<String> {
    let mut names: Vec<String> = ai.providers.keys().cloned().collect();
    names.sort();
    names
}

#[derive(Default)]
pub struct AiPanel {
    pub open: bool,
    pub selected: usize,
    pub mode: ModeState,
    pub kind_idx: usize,
    pub form: Option<Form>,
}

/// Default-able wrapper so `AiPanel` can derive `Default`.
pub struct ModeState(pub Mode);
impl Default for ModeState {
    fn default() -> Self {
        ModeState(Mode::List)
    }
}

impl AiPanel {
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
        self.mode = ModeState(Mode::List);
        self.form = None;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.form = None;
        self.mode = ModeState(Mode::List);
    }
    pub fn mode(&self) -> Mode {
        self.mode.0
    }
    pub fn set_mode(&mut self, m: Mode) {
        self.mode = ModeState(m);
    }

    /// Rows navigable in List mode: one per provider plus the "Add" row.
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
    pub fn move_kind(&mut self, delta: isize) {
        let n = KINDS.len() as isize;
        self.kind_idx = (((self.kind_idx as isize + delta) % n + n) % n) as usize;
    }

    /// Enter Form mode to add a provider of the given kind, prefilled from its
    /// preset.
    pub fn start_add(&mut self, kind: &str) {
        let p = preset(kind);
        let needs_key = p.as_ref().map(|p| p.needs_key).unwrap_or(true);
        let model = p.as_ref().map(|p| p.model.to_string()).unwrap_or_default();
        let base_url = if kind == "custom" {
            String::new()
        } else {
            p.as_ref().map(|p| p.base_url.to_string()).unwrap_or_default()
        };
        self.form = Some(Form {
            kind: kind.to_string(),
            editing: None,
            name: kind.to_string(),
            key: String::new(),
            model,
            base_url,
            protocol: "openai".to_string(),
            needs_key,
            focus: 0,
        });
        self.set_mode(Mode::Form);
    }

    /// Enter Form mode to edit an existing provider.
    pub fn start_edit(&mut self, name: &str, cfg: &crate::ai::config::ProviderCfg) {
        let needs_key = preset(&cfg.kind).map(|p| p.needs_key).unwrap_or(true);
        self.form = Some(Form {
            kind: cfg.kind.clone(),
            editing: Some(name.to_string()),
            name: name.to_string(),
            key: cfg.api_key.clone().unwrap_or_default(),
            model: cfg.model.clone().unwrap_or_default(),
            base_url: cfg.base_url.clone().unwrap_or_default(),
            protocol: cfg.protocol.clone().unwrap_or_else(|| "openai".to_string()),
            needs_key,
            focus: 0,
        });
        self.set_mode(Mode::Form);
    }
}

fn fill(screen: &mut Screen, x0: u16, y: u16, width: u16, bg: Color) {
    for x in x0..x0 + width {
        screen.set(x, y, Cell { ch: ' ', fg: bg, bg, bold: false, italic: false });
    }
}

/// Draw a titled box and return `(x0, inner_left, content_width)`.
fn frame(screen: &mut Screen, app: &App, title: &str, height: u16) -> (u16, u16, u16) {
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let border = theme.line_number_current;
    let width = app.width.saturating_sub(6).min(72).max(44);
    let x0 = (app.width - width) / 2;
    let y0 = 2u16;
    let inner = (width - 2) as usize;

    let mut top = format!("┌ {title} ");
    while top.chars().count() < (width - 1) as usize {
        top.push('─');
    }
    top.push('┐');
    fill(screen, x0, y0, width, panel_bg);
    screen.put_str(x0, y0, &top, border, panel_bg, false, false);

    for r in 1..height - 1 {
        let y = y0 + r;
        fill(screen, x0, y, width, panel_bg);
        screen.put_str(x0, y, "│", border, panel_bg, false, false);
        screen.put_str(x0 + width - 1, y, "│", border, panel_bg, false, false);
    }
    let mut bot = String::from("└");
    for _ in 0..inner {
        bot.push('─');
    }
    bot.push('┘');
    let by = y0 + height - 1;
    fill(screen, x0, by, width, panel_bg);
    screen.put_str(x0, by, &bot, border, panel_bg, false, false);
    (x0, x0 + 2, width)
}

pub fn render(screen: &mut Screen, app: &App) {
    if app.width < 46 || app.height < 12 {
        return;
    }
    match app.ai_panel.mode() {
        Mode::List => render_list(screen, app),
        Mode::PickKind => render_pick_kind(screen, app),
        Mode::Form => render_form(screen, app),
    }
}

fn render_list(screen: &mut Screen, app: &App) {
    let theme = &app.config.theme;
    let ai = &app.config.ai;
    let names = provider_names(ai);
    let chat_default = ai.defaults.get("chat").cloned();
    let total = names.len() + 1;
    let rows = total.min((app.height as usize).saturating_sub(7)) as u16;
    let height = rows + 4;
    let (x0, ix, width) = frame(screen, app, "AI Providers", height);
    let panel_bg = theme.statusbar_bg;

    let top_idx = scroll_top(app.ai_panel.selected, rows as usize, total);
    for r in 0..rows {
        let i = top_idx + r as usize;
        if i >= total {
            break;
        }
        let y = 3 + r;
        let selected = i == app.ai_panel.selected;
        let row_bg = if selected { theme.selection } else { panel_bg };
        for x in x0 + 1..x0 + width - 1 {
            screen.set(x, y, Cell { ch: ' ', fg: row_bg, bg: row_bg, bold: false, italic: false });
        }
        let caret = if selected { "▶ " } else { "  " };
        screen.put_str(x0 + 1, y, caret, theme.keyword, row_bg, false, false);

        if i == names.len() {
            screen.put_str(ix + 1, y, "＋ Add provider…", theme.keyword, row_bg, false, false);
            continue;
        }
        let name = &names[i];
        let pc = &ai.providers[name];
        let is_default = chat_default.as_deref() == Some(name.as_str());
        let star = if is_default { "★ " } else { "  " };
        screen.put_str(ix + 1, y, star, theme.keyword, row_bg, false, false);
        screen.put_str(ix + 3, y, name, theme.foreground, row_bg, true, false);

        let model = pc.model.clone().unwrap_or_else(|| "(preset)".into());
        let has_key = pc.api_key.as_deref().map(|k| !k.is_empty()).unwrap_or(false)
            || pc.api_key_env.is_some()
            || matches!(pc.kind.as_str(), "ollama" | "lmstudio");
        let key = if has_key { "" } else { "  ⚠ no key" };
        let meta = format!("{}  ·  {}{}", pc.kind, model, key);
        let mx = ix + 3 + name.chars().count() as u16 + 2;
        if (mx as usize) < (x0 + width - 1) as usize {
            screen.put_str(mx, y, &meta, theme.comment, row_bg, false, false);
        }
    }
    let hy = 3 + rows;
    screen.put_str(ix, hy, "↑↓ move · a add · e edit · enter default · d remove · esc save", theme.comment, panel_bg, false, false);
}

fn render_pick_kind(screen: &mut Screen, app: &App) {
    let theme = &app.config.theme;
    let rows = KINDS.len() as u16;
    let height = rows + 4;
    let (_x0, ix, width) = frame(screen, app, "Choose provider", height);
    let panel_bg = theme.statusbar_bg;
    for (r, (kind, desc)) in KINDS.iter().enumerate() {
        let y = 3 + r as u16;
        let selected = r == app.ai_panel.kind_idx;
        let row_bg = if selected { theme.selection } else { panel_bg };
        for x in ix - 1..ix - 1 + (width - 3) {
            screen.set(x, y, Cell { ch: ' ', fg: row_bg, bg: row_bg, bold: false, italic: false });
        }
        let caret = if selected { "▶ " } else { "  " };
        screen.put_str(ix - 1, y, caret, theme.keyword, row_bg, false, false);
        screen.put_str(ix + 1, y, kind, theme.foreground, row_bg, true, false);
        let dx = ix + 1 + 12;
        screen.put_str(dx, y, desc, theme.comment, row_bg, false, false);
    }
    let hy = 3 + rows;
    screen.put_str(ix, hy, "↑↓ choose · enter next · esc back", theme.comment, panel_bg, false, false);
}

fn render_form(screen: &mut Screen, app: &App) {
    let theme = &app.config.theme;
    let panel_bg = theme.statusbar_bg;
    let Some(form) = &app.ai_panel.form else { return };
    let fields = form.fields();
    let rows = fields.len() as u16;
    let height = rows + 4;
    let title = match &form.editing {
        Some(_) => format!("Edit provider ({})", form.kind),
        None => format!("Add provider ({})", form.kind),
    };
    let (_x0, ix, width) = frame(screen, app, &title, height);

    for (r, f) in fields.iter().enumerate() {
        let y = 3 + r as u16;
        let focused = r == form.focus;
        let label = format!("{:>9}: ", f.label());
        screen.put_str(ix, y, &label, theme.comment, panel_bg, false, false);
        let vx = ix + label.chars().count() as u16;
        let raw = form.value(*f);
        let shown = if f.secret() {
            "•".repeat(raw.chars().count())
        } else {
            raw.to_string()
        };
        let fg = if focused { theme.keyword } else { theme.foreground };
        // Clip to the box.
        let maxw = (width as usize).saturating_sub(label.chars().count() + 5);
        let shown: String = shown.chars().take(maxw).collect();
        screen.put_str(vx, y, &shown, fg, panel_bg, focused, false);
        if focused {
            // A caret block just after the value.
            let cx = vx + shown.chars().count() as u16;
            screen.set(cx, y, Cell { ch: '▏', fg: theme.keyword, bg: panel_bg, bold: false, italic: false });
        }
    }
    let hy = 3 + rows;
    screen.put_str(ix, hy, "tab/↑↓ field · type to edit · enter save · esc cancel", theme.comment, panel_bg, false, false);
}

fn scroll_top(selected: usize, rows: usize, len: usize) -> usize {
    if len <= rows || selected < rows {
        0
    } else {
        (selected + 1 - rows).min(len - rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(kind: &str) -> Form {
        let mut p = AiPanel::default();
        p.start_add(kind);
        p.form.unwrap()
    }

    #[test]
    fn preset_form_has_name_key_model() {
        let f = form("openai");
        assert_eq!(f.fields(), vec![Field::Name, Field::Key, Field::Model]);
        assert!(f.needs_key);
        assert_eq!(f.name, "openai");
        assert!(!f.model.is_empty()); // prefilled from preset
    }

    #[test]
    fn keyless_local_form_omits_key() {
        let f = form("ollama");
        assert_eq!(f.fields(), vec![Field::Name, Field::Model]);
        assert!(!f.needs_key);
    }

    #[test]
    fn custom_form_adds_base_url_and_protocol() {
        let f = form("custom");
        assert_eq!(
            f.fields(),
            vec![Field::Name, Field::Key, Field::Model, Field::BaseUrl, Field::Protocol]
        );
    }

    #[test]
    fn focus_wraps_over_active_fields() {
        let mut f = form("ollama"); // 2 fields
        assert_eq!(f.focus, 0);
        f.move_focus(1);
        assert_eq!(f.focus, 1);
        f.move_focus(1);
        assert_eq!(f.focus, 0); // wrapped
        f.move_focus(-1);
        assert_eq!(f.focus, 1);
    }
}
