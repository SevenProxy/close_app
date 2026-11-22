use std::{
    collections::{HashSet, VecDeque},
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Margin, Rect, RichText, Shadow, Stroke, Vec2, Visuals, pos2,
};
use egui_phosphor::{Variant, regular as icon};
use sysinfo::{Pid, Signal, System};

use crate::{
    hardware::{self, HISTORY_LEN, HardwareMonitor, Snapshot},
    ports::{self, OpenPort, Transport},
    processes::{self, ProcessInfo},
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const ESCALATE_AFTER: Duration = Duration::from_secs(2);
const PORT_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

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
    success: Color32,
    warning: Color32,
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
        accent: Color32::from_rgb(255, 205, 86),
        accent_deep: Color32::from_rgb(176, 132, 30),
        success: Color32::from_rgb(120, 220, 160),
        warning: Color32::from_rgb(255, 160, 80),
        danger: Color32::from_rgb(255, 110, 125),
        surface: Color32::from_rgb(14, 14, 17),
        surface_raised: Color32::from_rgb(26, 24, 29),
        surface_hover: Color32::from_rgb(40, 37, 44),
        input_bg: Color32::from_rgb(11, 11, 14),
        border: Color32::from_rgb(56, 52, 61),
        muted: Color32::from_rgb(154, 148, 164),
        text: Color32::from_rgb(245, 241, 232),
    };

    const LIGHT: Self = Self {
        accent: Color32::from_rgb(163, 106, 0),
        accent_deep: Color32::from_rgb(214, 158, 32),
        success: Color32::from_rgb(35, 130, 80),
        warning: Color32::from_rgb(176, 104, 20),
        danger: Color32::from_rgb(198, 52, 66),
        surface: Color32::from_rgb(250, 247, 238),
        surface_raised: Color32::from_rgb(255, 253, 247),
        surface_hover: Color32::from_rgb(244, 237, 219),
        input_bg: Color32::from_rgb(255, 255, 255),
        border: Color32::from_rgb(224, 216, 195),
        muted: Color32::from_rgb(110, 102, 88),
        text: Color32::from_rgb(40, 35, 27),
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Processes,
    Hardware,
    Network,
}

pub struct ProcessApp {
    tab: Tab,
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
    hardware: HardwareMonitor,
    ports: Vec<OpenPort>,
    port_query: String,
    external_only: bool,
    last_ports_refresh: Instant,
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

        let mut hardware = HardwareMonitor::new();
        hardware.sample();

        Self {
            tab: Tab::default(),
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
            hardware,
            ports: ports::collect(),
            port_query: String::new(),
            external_only: false,
            last_ports_refresh: Instant::now(),
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
        if self.auto_refresh && self.hardware.due() {
            self.hardware.sample();
        }
        if self.auto_refresh && self.last_ports_refresh.elapsed() >= PORT_REFRESH_INTERVAL {
            self.ports = ports::collect();
            self.last_ports_refresh = Instant::now();
        }
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
                        let theme_glyph = match self.theme {
                            Theme::Dark => icon::SUN,
                            Theme::Light => icon::MOON,
                        };
                        if ui
                            .button(RichText::new(theme_glyph).color(palette.accent))
                            .on_hover_text("Alternar tema claro/escuro")
                            .clicked()
                        {
                            self.theme = match self.theme {
                                Theme::Dark => Theme::Light,
                                Theme::Light => Theme::Dark,
                            };
                            self.theme.apply(ui.ctx());
                        }
                        ui.label(
                            RichText::new(format!("{}  Vulkan", icon::CPU)).color(palette.accent),
                        );
                    });
                });

                ui.add_space(2.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (tab, glyph, label) in [
                        (Tab::Processes, icon::LIST, "Processos"),
                        (Tab::Hardware, icon::GAUGE, "Hardware"),
                        (Tab::Network, icon::PLUG, "Rede"),
                    ] {
                        let selected = self.tab == tab;
                        let text = RichText::new(format!("{glyph}  {label}"))
                            .color(if selected {
                                palette.accent
                            } else {
                                palette.muted
                            })
                            .strong();
                        let response = ui.selectable_label(selected, text);
                        if response.clicked() {
                            self.tab = tab;
                        }
                        if selected {
                            let rect = response.rect;
                            ui.painter().rect_filled(
                                Rect::from_min_max(
                                    pos2(rect.left() + 6.0, rect.bottom() - 2.0),
                                    pos2(rect.right() - 6.0, rect.bottom()),
                                ),
                                CornerRadius::same(2),
                                palette.accent,
                            );
                        }
                    }
                });

                if self.tab == Tab::Processes {
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
                            .on_hover_text("Recarregar processos e dados de hardware")
                            .clicked()
                        {
                            self.refresh();
                            self.hardware.sample();
                        }

                        ui.checkbox(
                            &mut self.auto_refresh,
                            RichText::new("Auto").color(palette.muted),
                        )
                        .on_hover_text("Processos a cada 2s, hardware a cada 1s");

                        let clear = ui.button(RichText::new(icon::BROOM).color(palette.muted));
                        if clear.on_hover_text("Limpar seleção").clicked() {
                            self.selected.clear();
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        let refresh = ui.button(
                            RichText::new(format!("{}  Atualizar", icon::ARROWS_CLOCKWISE))
                                .color(palette.text),
                        );
                        if refresh.on_hover_text("Ler novamente os dados").clicked() {
                            self.hardware.sample();
                            if self.tab == Tab::Network {
                                self.ports = ports::collect();
                                self.last_ports_refresh = Instant::now();
                            }
                        }

                        ui.checkbox(
                            &mut self.auto_refresh,
                            RichText::new("Auto").color(palette.muted),
                        )
                        .on_hover_text("Hardware a cada 1s, portas a cada 3s");
                    });
                }
            });

        egui::Panel::bottom("footer")
            .frame(edge_frame(palette))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let count = self.selected.len();
                    if self.tab == Tab::Processes {
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
                    } else {
                        ui.label(
                            RichText::new(format!("{}  Somente leitura", icon::INFO))
                                .color(palette.success)
                                .small(),
                        );
                    }

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
                match self.tab {
                    Tab::Processes => {}
                    Tab::Hardware => {
                        ui_hardware(ui, &self.hardware.snapshot, palette);
                        return;
                    }
                    Tab::Network => {
                        ui_network(
                            ui,
                            &self.hardware.snapshot,
                            &self.ports,
                            &mut self.port_query,
                            &mut self.external_only,
                            palette,
                        );
                        return;
                    }
                }

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

fn ui_hardware(ui: &mut egui::Ui, snapshot: &Snapshot, palette: Palette) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{}  Uso de hardware", icon::CHART_LINE_UP))
                .size(17.0)
                .strong(),
        );
        ui.label(
            RichText::new("somente leitura")
                .small()
                .color(palette.muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{}  amostragem a cada 1s", icon::ARROWS_CLOCKWISE))
                    .small()
                    .color(palette.muted),
            );
        });
    });

    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            hardware_overview(ui, snapshot, palette);
            ui.add_space(10.0);
            hardware_cpu(ui, snapshot, palette);
            ui.add_space(10.0);
            hardware_memory(ui, snapshot, palette);
            ui.add_space(10.0);
            hardware_disks(ui, snapshot, palette);
            ui.add_space(10.0);
            hardware_sensors(ui, snapshot, palette);
        });
}

fn hardware_overview(ui: &mut egui::Ui, snapshot: &Snapshot, palette: Palette) {
    card_frame(palette).show(ui, |ui| {
        let load = snapshot.load;
        let fields = [
            ("Host", truncate(&snapshot.hostname, 28)),
            ("Sistema", truncate(&snapshot.os, 28)),
            ("Kernel", truncate(&snapshot.kernel, 28)),
            ("Uptime", hardware::format_uptime(snapshot.uptime)),
            (
                "Carga",
                format!("{:.2} / {:.2} / {:.2}", load[0], load[1], load[2]),
            ),
            (
                "Núcleos",
                format!(
                    "{} físicos / {} lógicos",
                    snapshot
                        .physical_cores
                        .map_or_else(|| "?".into(), |count| count.to_string()),
                    snapshot.logical_cores
                ),
            ),
        ];

        ui.columns(3, |columns| {
            let column_count = columns.len();
            for (index, (label, value)) in fields.into_iter().enumerate() {
                let column = &mut columns[index % column_count];
                if index >= column_count {
                    column.add_space(6.0);
                }
                column.label(RichText::new(label).small().color(palette.muted));
                column.label(RichText::new(value).small().strong());
            }
        });
    });
}

fn hardware_cpu(ui: &mut egui::Ui, snapshot: &Snapshot, palette: Palette) {
    card_frame(palette).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}  CPU", icon::CPU)).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.0}%", snapshot.cpu_usage))
                        .monospace()
                        .strong()
                        .color(palette.accent),
                );
            });
        });

        ui.add_space(5.0);
        meter(ui, snapshot.cpu_usage / 100.0, palette.accent);
        ui.add_space(5.0);
        history_bars(
            ui,
            &snapshot.cpu_history,
            palette.accent,
            palette,
            56.0,
            100.0,
        );

        ui.add_space(12.0);
        ui.label(RichText::new("Núcleos").small().color(palette.muted));
        egui::Grid::new("cpu-cores")
            .num_columns(2)
            .spacing([14.0, 4.0])
            .show(ui, |ui| {
                for (index, usage) in snapshot.cpu_cores.iter().enumerate() {
                    ui.label(
                        RichText::new(format!("CPU {index}"))
                            .monospace()
                            .small()
                            .color(palette.muted),
                    );
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::ProgressBar::new(usage / 100.0)
                                .desired_width(110.0)
                                .desired_height(6.0)
                                .corner_radius(CornerRadius::same(4))
                                .fill(palette.accent),
                        );
                        ui.label(
                            RichText::new(format!("{usage:.0}%"))
                                .monospace()
                                .small()
                                .color(palette.muted),
                        );
                    });
                    ui.end_row();
                }
            });
    });
}

fn hardware_memory(ui: &mut egui::Ui, snapshot: &Snapshot, palette: Palette) {
    card_frame(palette).show(ui, |ui| {
        ui.label(RichText::new(format!("{}  Memória", icon::MEMORY)).strong());
        ui.add_space(6.0);

        let ram_fraction = hardware::fraction(snapshot.memory_used, snapshot.memory_total);
        let swap_fraction = hardware::fraction(snapshot.swap_used, snapshot.swap_total);

        egui::Grid::new("memory-grid")
            .num_columns(3)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                memory_row(
                    ui,
                    "RAM",
                    ram_fraction,
                    &format!(
                        "{} / {}  ({:.0}%)",
                        hardware::format_bytes(snapshot.memory_used),
                        hardware::format_bytes(snapshot.memory_total),
                        ram_fraction * 100.0
                    ),
                    palette.success,
                );
                memory_row(
                    ui,
                    "Swap",
                    swap_fraction,
                    &format!(
                        "{} / {}  ({:.0}%)",
                        hardware::format_bytes(snapshot.swap_used),
                        hardware::format_bytes(snapshot.swap_total),
                        swap_fraction * 100.0
                    ),
                    palette.warning,
                );
            });

        ui.add_space(8.0);
        history_bars(
            ui,
            &snapshot.memory_history,
            palette.success,
            palette,
            44.0,
            100.0,
        );
    });
}

fn memory_row(ui: &mut egui::Ui, label: &str, fraction: f32, value: &str, color: Color32) {
    ui.label(
        RichText::new(label)
            .small()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add(
        egui::ProgressBar::new(fraction)
            .desired_width(140.0)
            .desired_height(9.0)
            .corner_radius(CornerRadius::same(5))
            .fill(color),
    );
    ui.label(RichText::new(value).monospace().small());
    ui.end_row();
}

fn network_traffic_card(ui: &mut egui::Ui, snapshot: &Snapshot, palette: Palette) {
    card_frame(palette).show(ui, |ui| {
        ui.label(RichText::new(format!("{}  Rede", icon::NETWORK)).strong());
        ui.add_space(6.0);

        let scale = snapshot
            .rx_history
            .iter()
            .chain(snapshot.tx_history.iter())
            .copied()
            .fold(1.0, f32::max);

        ui.columns(2, |columns| {
            columns[0].vertical(|ui| {
                ui.label(RichText::new("Recebido").small().color(palette.accent));
                ui.label(
                    RichText::new(hardware::format_rate(
                        snapshot.rx_history.back().copied().unwrap_or(0.0),
                    ))
                    .strong()
                    .monospace(),
                );
                history_bars(
                    ui,
                    &snapshot.rx_history,
                    palette.accent,
                    palette,
                    48.0,
                    scale,
                );
            });
            columns[1].vertical(|ui| {
                ui.label(RichText::new("Enviado").small().color(palette.accent_deep));
                ui.label(
                    RichText::new(hardware::format_rate(
                        snapshot.tx_history.back().copied().unwrap_or(0.0),
                    ))
                    .strong()
                    .monospace(),
                );
                history_bars(
                    ui,
                    &snapshot.tx_history,
                    palette.accent_deep,
                    palette,
                    48.0,
                    scale,
                );
            });
        });

        if !snapshot.interfaces.is_empty() {
            ui.add_space(10.0);
            egui::Grid::new("network-grid")
                .num_columns(3)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("Interface");
                    ui.strong("RX");
                    ui.strong("TX");
                    ui.end_row();

                    for interface in &snapshot.interfaces {
                        ui.label(
                            RichText::new(truncate(&interface.name, 24))
                                .monospace()
                                .small(),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{}  ·  {}",
                                hardware::format_rate(interface.rx_rate),
                                hardware::format_bytes(interface.total_received)
                            ))
                            .small()
                            .color(palette.muted),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{}  ·  {}",
                                hardware::format_rate(interface.tx_rate),
                                hardware::format_bytes(interface.total_transmitted)
                            ))
                            .small()
                            .color(palette.muted),
                        );
                        ui.end_row();
                    }
                });
        }
    });
}

fn hardware_disks(ui: &mut egui::Ui, snapshot: &Snapshot, palette: Palette) {
    card_frame(palette).show(ui, |ui| {
        ui.label(RichText::new(format!("{}  Discos", icon::HARD_DRIVES)).strong());
        ui.add_space(6.0);

        if snapshot.disks.is_empty() {
            ui.label(
                RichText::new("Nenhum disco encontrado.")
                    .small()
                    .color(palette.muted),
            );
            return;
        }

        egui::Grid::new("disk-grid")
            .num_columns(3)
            .spacing([14.0, 4.0])
            .show(ui, |ui| {
                ui.strong("Disco");
                ui.strong("Montagem");
                ui.strong("Uso");
                ui.end_row();

                for disk in &snapshot.disks {
                    let used = disk.total.saturating_sub(disk.available);
                    let fraction = hardware::fraction(used, disk.total);
                    ui.label(RichText::new(truncate(&disk.name, 18)).monospace().small());
                    ui.label(
                        RichText::new(truncate(&disk.mount, 24))
                            .small()
                            .color(palette.muted),
                    );
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(80.0)
                                .desired_height(6.0)
                                .corner_radius(CornerRadius::same(4))
                                .fill(palette.accent),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} / {}  ({:.0}%)",
                                hardware::format_bytes(used),
                                hardware::format_bytes(disk.total),
                                fraction * 100.0
                            ))
                            .small()
                            .monospace()
                            .color(palette.muted),
                        );
                    });
                    ui.end_row();
                }
            });
    });
}

fn hardware_sensors(ui: &mut egui::Ui, snapshot: &Snapshot, palette: Palette) {
    card_frame(palette).show(ui, |ui| {
        ui.label(RichText::new(format!("{}  Temperatura", icon::THERMOMETER)).strong());
        ui.add_space(6.0);

        if snapshot.sensors.is_empty() {
            ui.label(
                RichText::new("Nenhum sensor de temperatura disponível.")
                    .small()
                    .color(palette.muted),
            );
            return;
        }

        egui::Grid::new("sensor-grid")
            .num_columns(2)
            .spacing([14.0, 4.0])
            .show(ui, |ui| {
                for sensor in &snapshot.sensors {
                    ui.label(RichText::new(truncate(&sensor.label, 32)).small());
                    ui.label(
                        RichText::new(format!("{:.1} °C", sensor.temperature))
                            .monospace()
                            .small()
                            .color(temperature_color(
                                sensor.temperature,
                                sensor.max,
                                sensor.critical,
                                palette,
                            )),
                    );
                    ui.end_row();
                }
            });
    });
}

fn temperature_color(
    temperature: f32,
    max: f32,
    critical: Option<f32>,
    palette: Palette,
) -> Color32 {
    if critical.is_some_and(|limit| temperature >= limit) {
        palette.danger
    } else if max > 0.0 && temperature >= max * 0.85 {
        palette.warning
    } else {
        palette.success
    }
}

fn meter(ui: &mut egui::Ui, fraction: f32, color: Color32) {
    ui.add(
        egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
            .desired_width(ui.available_width())
            .desired_height(9.0)
            .corner_radius(CornerRadius::same(5))
            .fill(color),
    );
}

fn history_bars(
    ui: &mut egui::Ui,
    values: &VecDeque<f32>,
    color: Color32,
    palette: Palette,
    height: f32,
    max: f32,
) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), palette.input_bg);

    let slot = rect.width() / HISTORY_LEN as f32;
    let max = max.max(0.001);
    for (index, value) in values.iter().rev().take(HISTORY_LEN).enumerate() {
        let fraction = (*value / max).clamp(0.0, 1.0);
        if fraction <= 0.0 {
            continue;
        }

        let x = rect.right() - (index + 1) as f32 * slot;
        let bar = Rect::from_min_max(
            pos2(x + 0.5, rect.bottom() - fraction * rect.height()),
            pos2(x + slot - 0.5, rect.bottom()),
        );
        if bar.width() > 0.0 {
            painter.rect_filled(bar, CornerRadius::ZERO, color);
        }
    }
}

fn ui_network(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    ports: &[OpenPort],
    query: &mut String,
    external_only: &mut bool,
    palette: Palette,
) {
    let exposed = ports.iter().filter(|port| port.is_exposed()).count();

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("{}  Tráfego e portas", icon::PLUG))
                .size(17.0)
                .strong(),
        );
        ui.label(
            RichText::new("somente leitura")
                .small()
                .color(palette.muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            chip(
                ui,
                format!("{} abertas · {} expostas", ports.len(), exposed),
                if exposed > 0 {
                    palette.warning
                } else {
                    palette.success
                },
            );
        });
    });

    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            network_traffic_card(ui, snapshot, palette);
            ui.add_space(10.0);

            card_frame(palette).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{}  Portas em escuta", icon::SHIELD)).strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.checkbox(
                            external_only,
                            RichText::new("Somente externas").small().color(palette.muted),
                        )
                        .on_hover_text("Ocultar portas ligadas apenas ao loopback");
                    });
                });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::MAGNIFYING_GLASS).color(palette.accent));
                    ui.add(
                        egui::TextEdit::singleline(query)
                            .hint_text("porta, PID, processo ou endereço")
                            .desired_width(260.0),
                    );
                });

                ui.add_space(8.0);

                let filtered: Vec<&OpenPort> = ports
                    .iter()
                    .filter(|port| (!*external_only || port.is_exposed()) && port.matches(query))
                    .collect();

                if filtered.is_empty() {
                    ui.label(RichText::new("Nenhuma porta encontrada.").small().color(palette.muted));
                } else {
                    egui::Grid::new("port-grid")
                        .striped(true)
                        .num_columns(6)
                        .spacing([14.0, 4.0])
                        .show(ui, |ui| {
                            ui.strong("Proto");
                            ui.strong("Endereço");
                            ui.strong("Porta");
                            ui.strong("Estado");
                            ui.strong("PID");
                            ui.strong("Processo");
                            ui.end_row();

                            for port in filtered {
                                ui.label(
                                    RichText::new(match port.transport {
                                        Transport::Tcp => "TCP",
                                        Transport::Udp => "UDP",
                                    })
                                    .monospace()
                                    .small()
                                    .color(palette.accent),
                                );
                                ui.label(
                                    RichText::new(&port.address)
                                        .monospace()
                                        .small()
                                        .color(if port.is_exposed() {
                                            palette.warning
                                        } else {
                                            palette.muted
                                        }),
                                );
                                ui.label(RichText::new(port.port.to_string()).monospace().strong());
                                ui.label(RichText::new(port.state).small().color(palette.muted));
                                ui.label(
                                    RichText::new(
                                        port.pid
                                            .map_or_else(|| "—".into(), |pid| pid.to_string()),
                                    )
                                    .monospace()
                                    .small()
                                    .color(palette.accent),
                                );
                                ui.label(
                                    RichText::new(
                                        port.process.as_deref().unwrap_or("desconhecido"),
                                    )
                                    .small()
                                    .color(if port.process.is_some() {
                                        palette.text
                                    } else {
                                        palette.muted
                                    }),
                                );
                                ui.end_row();
                            }
                        });
                }

                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "{}  Processos de outros usuários podem aparecer como desconhecidos sem root.",
                        icon::INFO
                    ))
                    .small()
                    .color(palette.muted),
                );
            });
        });
}

fn edge_frame(palette: Palette) -> Frame {
    Frame {
        inner_margin: Margin::symmetric(18, 11),
        fill: faded(palette.surface_raised, 238),
        stroke: Stroke::new(1.0, faded(palette.border, 190)),
        corner_radius: CornerRadius::ZERO,
        outer_margin: Margin::ZERO,
        shadow: Shadow::NONE,
    }
}

fn card_frame(palette: Palette) -> Frame {
    Frame {
        inner_margin: Margin::same(14),
        fill: faded(palette.surface_raised, 205),
        stroke: Stroke::new(1.0, faded(palette.border, 170)),
        corner_radius: CornerRadius::same(14),
        outer_margin: Margin::symmetric(0, 3),
        shadow: Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(70),
        },
    }
}

fn modal_frame(palette: Palette) -> Frame {
    Frame {
        inner_margin: Margin::same(20),
        fill: faded(palette.surface_raised, 246),
        stroke: Stroke::new(1.0, faded(palette.accent, 110)),
        corner_radius: CornerRadius::same(16),
        outer_margin: Margin::ZERO,
        shadow: Shadow {
            offset: [0, 12],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(120),
        },
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
