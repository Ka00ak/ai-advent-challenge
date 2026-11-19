//! Экран «Проверка»: настройки прогона, прогресс и таблицы.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Wrap};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(9),
            Constraint::Length(3),
            Constraint::Min(6),
        ])
        .split(area);

    let mut options = vec![
        Line::from(format!(
            "Режим: {}   Окно STM: {}   Cleanup: {}",
            if state.experiment_quick {
                "quick (9 вопросов)"
            } else {
                "full (18 вопросов)"
            },
            state.settings.memory.stm.window,
            if state.experiment_cleanup {
                "включён (3 прогона)"
            } else {
                "выключен"
            }
        )),
        Line::from(Span::styled(
            "Конфигурации: none · stm · wm · ltm · stm+wm · stm+wm+ltm · full",
            theme::muted(),
        )),
        Line::from(format!(
            "Запросов: ~{}",
            if state.experiment_quick { 63 } else { 126 }
                + if state.experiment_cleanup {
                    if state.experiment_quick { 27 } else { 54 }
                } else {
                    0
                }
        )),
        Line::from(""),
        Line::from(
            "[r] запустить · [m] режим · [Space] очистка · [s] сохранить · [d] детали · [i] импорт facts",
        ),
    ];
    if let Some(progress) = &state.experiment_progress {
        let last = match (&progress.last_probe, progress.last_matched) {
            (Some(id), Some(true)) => format!(" · {id} ✓"),
            (Some(id), Some(false)) => format!(" · {id} ✗"),
            _ => String::new(),
        };
        let elapsed = if progress.elapsed_ms >= 1000 {
            format!(" · {} с", progress.elapsed_ms / 1000)
        } else {
            String::new()
        };
        options.push(Line::from(format!(
            "Прогресс: {} · вопрос {}/{} · {}/{}{}{}",
            progress.config,
            progress.probe,
            progress.probes_total,
            progress.done,
            progress.total,
            last,
            elapsed
        )));
        options.push(Line::from(Span::styled(
            progress.usage_brief(),
            theme::muted(),
        )));
    }
    let options_block = Paragraph::new(options)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Контрольный диалог "),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(options_block, chunks[0]);

    let ratio = state
        .experiment_progress
        .as_ref()
        .map(|progress| {
            if progress.total == 0 {
                0.0
            } else {
                progress.done as f64 / progress.total as f64
            }
        })
        .unwrap_or(0.0);
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Прогресс "))
        .gauge_style(if state.busy.is_some() {
            theme::accent()
        } else {
            theme::muted()
        })
        .ratio(ratio.clamp(0.0, 1.0))
        .label(format!("{:.0}%", ratio * 100.0));
    frame.render_widget(gauge, chunks[1]);

    let mut lines = Vec::new();
    if let Some(report) = &state.experiment {
        lines.push(Line::from(
            "Конфигурация      LTM   WM  STM  System Tools  Total   S W L  Всего",
        ));
        for (index, config) in report.configs.iter().enumerate() {
            let marker = if index == state.experiment_selection {
                "> "
            } else {
                "  "
            };
            let line = format!(
                "{marker}{:<13} {:>5} {:>4} {:>4} {:>7} {:>5} {:>6}   {} {} {}  {}/{}",
                config.id,
                config.layer_tokens.ltm,
                config.layer_tokens.wm,
                config.layer_tokens.stm,
                config.system_tokens,
                config.tools_tokens,
                config.total_tokens,
                config.matched_stm,
                config.matched_wm,
                config.matched_ltm,
                config.total,
                config.effective
            );
            let style = if index == state.experiment_selection {
                theme::selected()
            } else {
                Default::default()
            };
            lines.push(Line::from(Span::styled(line, style)));
        }
        if !report.cleanup.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from("Очистка               S W L  Всего  Сломалось"));
            for config in &report.cleanup {
                let broken: Vec<&str> = config
                    .results
                    .iter()
                    .filter(|result| !result.matched)
                    .map(|result| result.id.as_str())
                    .collect();
                lines.push(Line::from(format!(
                    "{:<20} {} {} {}  {}/{}  {}",
                    config.id,
                    config.matched_stm,
                    config.matched_wm,
                    config.matched_ltm,
                    config.total,
                    config.effective,
                    if broken.is_empty() {
                        "—".into()
                    } else {
                        broken.join(",")
                    }
                )));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "ΔLTM={} (блок {}) · ΔWM={} (блок {}) · ΔSTM={} (блок {}) · экономия {} ({:.1}%)",
            report.deltas.ltm,
            report.deltas.ltm_block,
            report.deltas.wm,
            report.deltas.wm_block,
            report.deltas.stm,
            report.deltas.stm_block,
            report.saved_tokens,
            report.saved_ratio * 100.0
        )));
        let failed: Vec<&str> = report
            .thresholds
            .iter()
            .filter(|threshold| !threshold.ok)
            .map(|threshold| threshold.name.as_str())
            .collect();
        if failed.is_empty() {
            lines.push(Line::from(Span::styled("Пороги пройдены", theme::ok())));
        } else {
            lines.push(Line::from(Span::styled(
                format!("Пороги не пройдены: {}", failed.join(", ")),
                theme::warn(),
            )));
        }
    } else {
        lines.push(Line::from("Нажмите r, чтобы запустить проверку"));
        lines.push(Line::from(
            "Запросы идут в фоновом потоке, TUI остаётся отзывчивым",
        ));
        if let Some(progress) = &state.experiment_progress {
            lines.push(Line::from(format!(
                "{} · {}/{}",
                progress.config, progress.done, progress.total
            )));
        }
    }
    let results = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Ablation / очистка "),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(results, chunks[2]);
}
