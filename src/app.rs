use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Shadow, Stroke, Visuals};
use egui_phosphor::{Variant, regular as icon};
use sysinfo::{Pid, Signal, System};

use crate::processes::{self, ProcessInfo};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const ESCALATE_AFTER: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    const ALL: [Self; 2] = [Self::Dark, Self::Light];

    fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        }
    }

    fn palette(self) -> Palette {
        match self {
            Self::Dark => Palette::DARK,
            Self::Light => Palette::LIGHT,
        }
    }

    fn apply(self, ctx: &egui::Context) {
        let egui_theme = self.egui_theme();
        let palette = self.palette();
        ctx.set_theme(egui_theme);

        let mut visuals = match egui_theme {
            egui::Theme::Dark => Visuals::dark(),
            egui::Theme::Light => Visuals::light(),
        };
        visuals.panel_fill = palette.surface;
        visuals.window_fill = palette.surface_raised;
        visuals.extreme_bg_color = palette.input_bg;
        visuals.text_edit_bg_color = Some(palette.input_bg);
        visuals.faint_bg_color = palette.surface_raised;
        visuals.override_text_color = Some(palette.text);
        visuals.weak_text_color = Some(palette.muted);
        visuals.hyperlink_color = palette.accent;
        visuals.window_stroke = Stroke::new(1.0, palette.border);
        visuals.window_corner_radius = CornerRadius::same(12);
        visuals.menu_corner_radius = CornerRadius::same(10);
        visuals.selection.bg_fill = faded(palette.accent, 48);
        visuals.selection.stroke = Stroke::new(1.0, palette.accent);

        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = CornerRadius::same(8);
        }

        visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        visuals.widgets.inactive.bg_fill = palette.surface_raised;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.border);
        visuals.widgets.hovered.weak_bg_fill = palette.surface_hover;
        visuals.widgets.hovered.bg_fill = palette.surface_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent_deep);
        visuals.widgets.active.weak_bg_fill = palette.accent_deep;
        visuals.widgets.active.bg_fill = palette.accent_deep;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);

        ctx.set_visuals(visuals);
        for theme in Self::ALL {
            ctx.style_mut_of(theme.egui_theme(), |style| {
                style.spacing.item_spacing = egui::vec2(9.0, 7.0);
                style.spacing.button_padding = egui::vec2(12.0, 6.0);
                style.spacing.window_margin = Margin::same(18);
            });
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    accent: Color32,
    accent_deep: Color32,
    danger: Color32,
    surface: Color32,
    surface_raised: Color32,
    surface_hover: Color32,
    input_bg: Color32,
    border: Color32,
    muted: Color32,
    text: Color32,
}

impl Palette {
    const DARK: Self = Self {
        accent: Color32::from_rgb(132, 160, 255),
        accent_deep: Color32::from_rgb(88, 108, 190),
        danger: Color32::from_rgb(255, 116, 135),
        surface: Color32::from_rgb(13, 15, 23),
        surface_raised: Color32::from_rgb(20, 23, 33),
        surface_hover: Color32::from_rgb(30, 35, 50),
        input_bg: Color32::from_rgb(10, 12, 19),
        border: Color32::from_rgb(39, 44, 62),
        muted: Color32::from_rgb(137, 147, 176),
        text: Color32::from_rgb(232, 236, 248),
    };

    const LIGHT: Self = Self {
        accent: Color32::from_rgb(94, 84, 214),
        accent_deep: Color32::from_rgb(72, 62, 176),
        danger: Color32::from_rgb(201, 52, 78),
        surface: Color32::from_rgb(246, 247, 251),
        surface_raised: Color32::from_rgb(255, 255, 255),
        surface_hover: Color32::from_rgb(236, 239, 248),
        input_bg: Color32::from_rgb(255, 255, 255),
        border: Color32::from_rgb(216, 221, 235),
        muted: Color32::from_rgb(98, 106, 132),
        text: Color32::from_rgb(29, 34, 52),
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Terminate,
    Kill,
}

impl Action {
    fn signal(self) -> Signal {
        match self {
            Self::Terminate => Signal::Term,
            Self::Kill => Signal::Kill,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Terminate => "Encerrar processos",
            Self::Kill => "Forçar encerramento",
        }
    }

    fn button(self) -> &'static str {
        match self {
            Self::Terminate => "SIGTERM",
            Self::Kill => "SIGKILL",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Terminate => icon::HOURGLASS,
            Self::Kill => icon::LIGHTNING,
        }
    }

    fn color(self, palette: Palette) -> Color32 {
        match self {
            Self::Terminate => palette.accent,
            Self::Kill => palette.danger,
        }
    }
}

pub struct ProcessApp {
    system: System,
    own_pid: Pid,
    processes: Vec<ProcessInfo>,
    query: String,
    selected: HashSet<Pid>,
    auto_refresh: bool,
    last_refresh: Instant,
    pending_action: Option<Action>,
    escalating: Vec<(Pid, Instant)>,
    status: String,
    theme: Theme,
}

impl ProcessApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let theme = Theme::default();
        theme.apply(ctx);

        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, Variant::Regular);
        ctx.set_fonts(fonts);

        let mut system = System::new();
        let processes = processes::collect(&mut system);
        let status = format!("{} processos encontrados.", processes.len());

        Self {
            system,
            own_pid: Pid::from_u32(std::process::id()),
            processes,
            query: String::new(),
            selected: HashSet::new(),
            auto_refresh: true,
            last_refresh: Instant::now(),
            pending_action: None,
            escalating: Vec::new(),
            status,
            theme,
        }
    }

    fn refresh(&mut self) {
        self.processes = processes::collect(&mut self.system);
        self.last_refresh = Instant::now();

        let alive: HashSet<Pid> = self.processes.iter().map(|process| process.pid).collect();
        self.selected.retain(|pid| alive.contains(pid));
    }

    fn confirm(&mut self, action: Action) {
        let pids: Vec<Pid> = self.selected.iter().copied().collect();
        let mut succeeded = 0;
        let mut failures = Vec::new();
        let now = Instant::now();

        for pid in pids {
            match processes::signal(&mut self.system, pid, action.signal(), self.own_pid) {
                Ok(()) => {
                    succeeded += 1;
                    if action == Action::Terminate {
                        self.escalating.push((pid, now));
                    }
                }
                Err(error) => failures.push(format!("{pid}: {error}")),
            }
        }

        self.selected.clear();

        let mut status = format!("Sinal enviado a {succeeded} processo(s).");
        if !failures.is_empty() {
            status.push_str(" Falhas: ");
            status.push_str(&failures.join(", "));
        }
        self.status = status;
        self.refresh();
    }

    fn escalate_stale(&mut self) {
        if self.escalating.is_empty() {
            return;
        }

        let now = Instant::now();
        let due: Vec<Pid> = self
            .escalating
            .iter()
            .filter(|(_, sent_at)| now.duration_since(*sent_at) >= ESCALATE_AFTER)
            .map(|(pid, _)| *pid)
            .collect();

        if due.is_empty() {
            return;
        }
        self.escalating.retain(|(pid, _)| !due.contains(pid));

        let mut forced = 0;
        for pid in due {
            if processes::is_alive(&mut self.system, pid)
                && processes::signal(&mut self.system, pid, Signal::Kill, self.own_pid).is_ok()
            {
                forced += 1;
            }
        }

        if forced > 0 {
            self.status = format!("{forced} processo(s) ignoraram SIGTERM e receberam SIGKILL.");
        }
        self.refresh();
    }
}

impl eframe::App for ProcessApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.auto_refresh && self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh();
        }
        self.escalate_stale();
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let palette = self.theme.palette();

        egui::Panel::top("header")
            .frame(edge_frame(palette))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}  Close App", icon::SPARKLE))
                            .size(19.0)
                            .strong()
                            .color(palette.text),
                    );
                    chip(
                        ui,
                        format!("v{}", env!("CARGO_PKG_VERSION")),
                        palette.accent,
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{}  Vulkan", icon::CPU))
                                .small()
                                .color(palette.accent),
                        );
                        ui.label(RichText::new("backend").small().color(palette.muted));
                    });
                });

                ui.add_space(2.0);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(icon::MAGNIFYING_GLASS)
                            .color(palette.accent)
                            .size(16.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text("nome exato ou PID — ex.: vesktop")
                            .desired_width(300.0),
                    );

                    let refresh = ui.button(
                        RichText::new(format!("{}  Atualizar", icon::ARROWS_CLOCKWISE))
                            .color(palette.text),
                    );
                    if refresh
                        .on_hover_text("Recarregar a lista de processos")
                        .clicked()
                    {
                        self.refresh();
                    }

                    ui.checkbox(
                        &mut self.auto_refresh,
                        RichText::new("Auto").color(palette.muted),
                    )
                    .on_hover_text("Atualizar a cada 2 segundos");

                    let clear = ui.button(RichText::new(icon::BROOM).color(palette.muted));
                    if clear.on_hover_text("Limpar seleção").clicked() {
                        self.selected.clear();
                    }
                });
            });

        egui::Panel::bottom("footer")
            .frame(edge_frame(palette))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let count = self.selected.len();
                    ui.add_enabled_ui(count > 0, |ui| {
                        let term = ui.button(
                            RichText::new(format!("{}  Encerrar", icon::HOURGLASS))
                                .color(palette.text),
                        );
                        if term.on_hover_text("SIGTERM, com SIGKILL após 2s").clicked() {
                            self.pending_action = Some(Action::Terminate);
                        }

                        let kill = ui.button(
                            RichText::new(format!("{}  Forçar", icon::LIGHTNING))
                                .color(palette.danger),
                        );
                        if kill.on_hover_text("SIGKILL imediato").clicked() {
                            self.pending_action = Some(Action::Kill);
                        }
                    });

                    ui.label(
                        RichText::new(format!("{count} selecionado(s)"))
                            .color(palette.muted)
                            .small(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        chip(
                            ui,
                            format!(
                                "{}  v{} ({})",
                                icon::INFO,
                                env!("CARGO_PKG_VERSION"),
                                env!("CLOSE_APP_BUILD_HASH")
                            ),
                            palette.muted,
                        );
                        if !self.status.is_empty() {
                            ui.label(RichText::new(&self.status).small().color(palette.muted));
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(Frame {
                inner_margin: Margin::symmetric(18, 14),
                fill: palette.surface,
                stroke: Stroke::NONE,
                corner_radius: CornerRadius::ZERO,
                outer_margin: Margin::ZERO,
                shadow: Shadow::NONE,
            })
            .show(ui, |ui| {
                let filtered: Vec<&ProcessInfo> = self
                    .processes
                    .iter()
                    .filter(|process| process.matches(&self.query))
                    .collect();
                let max_memory = filtered
                    .iter()
                    .map(|process| process.memory_bytes)
                    .max()
                    .unwrap_or(1)
                    .max(1);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}  {} processos", icon::LIST, filtered.len()))
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("de {} no total", self.processes.len()))
                            .small()
                            .color(palette.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} selecionados", self.selected.len()))
                                .small()
                                .color(palette.accent),
                        );
                    });
                });

                ui.add_space(8.0);

                if filtered.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(56.0);
                        ui.label(
                            RichText::new(icon::MAGNIFYING_GLASS)
                                .size(36.0)
                                .color(palette.muted),
                        );
                        ui.add_space(6.0);
                        ui.label(RichText::new("Nenhum processo encontrado.").strong());
                        ui.label(
                            RichText::new("Tente outro nome ou PID.")
                                .small()
                                .color(palette.muted),
                        );
                    });
                    return;
                }

                card_frame(palette).show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            egui::Grid::new("process-grid")
                                .striped(true)
                                .num_columns(5)
                                .spacing([14.0, 6.0])
                                .show(ui, |ui| {
                                    ui.strong("");
                                    ui.strong("PID");
                                    ui.strong("Nome");
                                    ui.strong("Comando");
                                    ui.strong("Memória");
                                    ui.end_row();

                                    for process in filtered {
                                        let mut checked = self.selected.contains(&process.pid);
                                        if ui.checkbox(&mut checked, "").changed() {
                                            if checked {
                                                self.selected.insert(process.pid);
                                            } else {
                                                self.selected.remove(&process.pid);
                                            }
                                        }

                                        ui.label(
                                            RichText::new(process.pid.to_string())
                                                .monospace()
                                                .color(palette.accent),
                                        );
                                        ui.label(RichText::new(&process.name).strong());
                                        ui.label(
                                            RichText::new(truncate(&process.command, 120))
                                                .small()
                                                .color(palette.muted),
                                        );
                                        ui.horizontal(|ui| {
                                            ui.add(
                                                egui::ProgressBar::new(
                                                    process.memory_bytes as f32 / max_memory as f32,
                                                )
                                                .desired_width(64.0)
                                                .desired_height(6.0)
                                                .corner_radius(CornerRadius::same(4))
                                                .fill(palette.accent),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "{:.1} MiB",
                                                    process.memory_bytes as f64 / 1048576.0
                                                ))
                                                .monospace()
                                                .small()
                                                .color(palette.muted),
                                            );
                                        });
                                        ui.end_row();
                                    }
                                });
                        });
                });
            });

        self.show_confirmation(ui.ctx());
    }
}

impl ProcessApp {
    fn show_confirmation(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_action else {
            return;
        };

        let palette = self.theme.palette();
        let affected: Vec<String> = self
            .selected
            .iter()
            .filter_map(|pid| {
                self.processes
                    .iter()
                    .find(|process| process.pid == *pid)
                    .map(|process| format!("{} — {}", process.pid, process.name))
            })
            .collect();

        let mut cancel = false;
        let mut confirm = false;

        let response = egui::Modal::new(egui::Id::unique("confirm-action"))
            .frame(modal_frame(palette))
            .show(ctx, |ui| {
                ui.set_width(390.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(action.icon())
                            .size(26.0)
                            .color(action.color(palette)),
                    );
                    ui.vertical(|ui| {
                        ui.heading(action.title());
                        ui.label(
                            RichText::new("Processos selecionados:")
                                .color(palette.muted)
                                .small(),
                        );
                    });
                });

                ui.add_space(10.0);

                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for line in &affected {
                            ui.label(RichText::new(line).monospace().small());
                        }
                    });

                ui.add_space(10.0);
                if action == Action::Terminate {
                    ui.label(
                        RichText::new("Se não responder, SIGKILL será enviado após 2 segundos.")
                            .small()
                            .color(palette.muted),
                    );
                } else {
                    ui.label(
                        RichText::new("SIGKILL não pode ser ignorado e não salva dados pendentes.")
                            .small()
                            .color(palette.danger),
                    );
                }

                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let cancel_button = ui.button(
                        RichText::new(format!("{}  Cancelar", icon::PROHIBIT)).color(palette.muted),
                    );
                    if cancel_button.clicked() {
                        cancel = true;
                    }

                    let confirm_button = ui.button(
                        RichText::new(format!("{}  {}", action.icon(), action.button()))
                            .color(action.color(palette)),
                    );
                    if confirm_button.clicked() {
                        confirm = true;
                    }
                });
            });

        if cancel || confirm {
            if confirm {
                self.confirm(action);
            }
            self.pending_action = None;
        } else if response.should_close() {
            self.pending_action = None;
        }
    }
}

fn edge_frame(palette: Palette) -> Frame {
    Frame {
        inner_margin: Margin::symmetric(18, 11),
        fill: palette.surface_raised,
        stroke: Stroke::new(1.0, palette.border),
        corner_radius: CornerRadius::ZERO,
        outer_margin: Margin::ZERO,
        shadow: Shadow::NONE,
    }
}

fn card_frame(palette: Palette) -> Frame {
    Frame {
        inner_margin: Margin::same(12),
        fill: palette.surface_raised,
        stroke: Stroke::new(1.0, palette.border),
        corner_radius: CornerRadius::same(12),
        outer_margin: Margin::symmetric(0, 2),
        shadow: Shadow::NONE,
    }
}

fn modal_frame(palette: Palette) -> Frame {
    Frame {
        inner_margin: Margin::same(20),
        fill: palette.surface_raised,
        stroke: Stroke::new(1.0, palette.border),
        corner_radius: CornerRadius::same(16),
        outer_margin: Margin::ZERO,
        shadow: Shadow::NONE,
    }
}

fn chip(ui: &mut egui::Ui, text: String, color: Color32) {
    Frame {
        inner_margin: Margin::symmetric(8, 3),
        fill: faded(color, 24),
        stroke: Stroke::new(1.0, faded(color, 90)),
        corner_radius: CornerRadius::same(20),
        outer_margin: Margin::ZERO,
        shadow: Shadow::NONE,
    }
    .show(ui, |ui| {
        ui.label(RichText::new(text).small().color(color));
    });
}

fn faded(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_owned();
    }

    let mut result: String = value.chars().take(limit).collect();
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::{Theme, truncate};

    #[test]
    fn truncates_long_text_by_characters() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("ação", 3), "açã…");
    }

    #[test]
    fn default_theme_is_dark() {
        assert_eq!(Theme::default(), Theme::Dark);
    }

    #[test]
    fn every_theme_maps_to_an_egui_theme_and_palette() {
        for theme in Theme::ALL {
            let _ = theme.egui_theme();
            let palette = theme.palette();
            assert_ne!(palette.text, palette.surface);
        }
    }
}
