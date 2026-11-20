use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use eframe::egui;
use sysinfo::{Pid, Signal, System};

use crate::processes::{self, ProcessInfo};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const ESCALATE_AFTER: Duration = Duration::from_secs(2);

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
}

impl ProcessApp {
    pub fn new(ctx: &egui::Context) -> Self {
        ctx.set_theme(egui::Theme::Dark);

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
        egui::Panel::top("header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Gerenciador de processos");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("Vulkan")
                            .color(egui::Color32::from_rgb(130, 190, 255))
                            .strong(),
                    );
                    ui.label("Backend:");
                });
            });

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Buscar:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("nome, PID ou comando (ex.: vesktop)")
                        .desired_width(320.0),
                );
                if ui.button("Atualizar").clicked() {
                    self.refresh();
                }
                ui.checkbox(&mut self.auto_refresh, "Auto (2s)");
                if ui.button("Limpar seleção").clicked() {
                    self.selected.clear();
                }
            });

            ui.add_space(4.0);
        });

        egui::Panel::bottom("footer").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let count = self.selected.len();
                ui.add_enabled_ui(count > 0, |ui| {
                    if ui.button("Encerrar (SIGTERM)").clicked() {
                        self.pending_action = Some(Action::Terminate);
                    }
                    if ui.button("Forçar (SIGKILL)").clicked() {
                        self.pending_action = Some(Action::Kill);
                    }
                });
                ui.label(format!("{count} selecionado(s)"));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(&self.status);
                });
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let filtered: Vec<&ProcessInfo> = self
                .processes
                .iter()
                .filter(|process| process.matches(&self.query))
                .collect();

            ui.label(format!(
                "{} de {} processos",
                filtered.len(),
                self.processes.len()
            ));
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    egui::Grid::new("process-grid")
                        .striped(true)
                        .num_columns(5)
                        .spacing([12.0, 4.0])
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

                                ui.label(process.pid.to_string());
                                ui.label(&process.name);
                                ui.label(truncate(&process.command, 140));
                                ui.label(format!(
                                    "{:.1} MiB",
                                    process.memory_bytes as f64 / 1048576.0
                                ));
                                ui.end_row();
                            }
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

        let response = egui::Modal::new(egui::Id::unique("confirm-action")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(action.title());
            ui.label("Processos selecionados:");

            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for line in &affected {
                        ui.label(line);
                    }
                });

            ui.add_space(6.0);
            if action == Action::Terminate {
                ui.label("Se o processo não responder, SIGKILL será enviado após 2 segundos.");
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 140, 130),
                    "SIGKILL não pode ser ignorado e não salva dados pendentes.",
                );
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancelar").clicked() {
                    cancel = true;
                }
                if ui.button(action.button()).clicked() {
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
    use super::truncate;

    #[test]
    fn truncates_long_text_by_characters() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("ação", 3), "açã…");
    }
}
