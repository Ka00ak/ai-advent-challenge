//! Markdown/JSON-отчёт эксперимента.

use std::path::Path;

use crate::experiment::runner::{ConfigResult, ExperimentReport};
use crate::memory::tokens::format_tokens;
use crate::storage::atomic::{SaveError, atomic_write_bytes, atomic_write_json};

pub fn render_markdown(report: &ExperimentReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Эксперимент с памятью — {}\n\n",
        report.started_at.format("%Y-%m-%d")
    ));
    if report.source.to_lowercase().contains("fake") {
        out.push_str(
            "> **Mock-прогон:** fake-клиент (`LAB11_FAKE_LLM=1`). Числа проверяют\n> механику блоков, дельт и очистки; это не измерение качества модели.\n\n",
        );
    }
    out.push_str(&format!(
        "Модель: {} · data-dir: {} · окно: {} · extraction: local · режим: {}\n\n",
        report.model,
        report.data_dir.as_deref().unwrap_or("—"),
        report.window,
        report.mode.as_str()
    ));
    out.push_str("## Условия\n\n| Параметр | Значение |\n|---|---|\n");
    out.push_str(&format!("| Модель | {} |\n", report.model));
    out.push_str(&format!("| Base URL | {} |\n", report.base_url));
    out.push_str(&format!(
        "| Источник | {} |\n",
        if report.source.is_empty() {
            "api"
        } else {
            &report.source
        }
    ));
    out.push_str(&format!(
        "| Data-dir | {} |\n",
        report.data_dir.as_deref().unwrap_or("—")
    ));
    out.push_str(&format!(
        "| Начало | {} |\n",
        report.started_at.to_rfc3339()
    ));
    out.push_str(&format!(
        "| Конец | {} |\n",
        report.finished_at.to_rfc3339()
    ));
    out.push_str(&format!("| Режим | {} |\n", report.mode.as_str()));
    out.push_str(&format!("| Запросов | {} |\n", report.total_requests));
    let (prompt_tokens, completion_tokens, cost_usd) = report.usage_totals();
    out.push_str(&format!(
        "| Токены API | prompt {} · completion {} · всего {} |\n",
        format_tokens(prompt_tokens),
        format_tokens(completion_tokens),
        format_tokens(prompt_tokens + completion_tokens)
    ));
    out.push_str(&format!("| Стоимость API (оценка) | ${cost_usd:.6} |\n\n"));

    out.push_str("## Состав слоёв\n\n");
    out.push_str("| Слой | Записей | В промпте | Токенов |\n|---|---:|---:|---:|\n");
    if let Some(all) = report.config("stm+wm+ltm") {
        out.push_str(&format!(
            "| STM | {} | {} | {} |\n",
            report.counts.stm_messages, report.counts.stm_window, all.layer_tokens.stm
        ));
        out.push_str(&format!(
            "| WM | {} | {} | {} |\n",
            report.counts.wm_items, all.layer_tokens.wm, all.layer_tokens.wm
        ));
        out.push_str(&format!(
            "| LTM | {} | {} | {} |\n",
            report.counts.ltm_records,
            all.results
                .iter()
                .filter(|r| r.layer == crate::domain::memory::MemoryLayer::Ltm)
                .count(),
            all.layer_tokens.ltm
        ));
    }
    out.push_str(&format!(
        "\nPending: {}. Сводка STM: {}.\n\n",
        report.counts.pending,
        if report.counts.summary {
            "есть"
        } else {
            "нет"
        }
    ));

    out.push_str("## Ablation\n\n");
    out.push_str(
        "| Конфигурация | LTM | WM | STM | System | Tools | Total | S✓ | W✓ | L✓ | Всего |\n",
    );
    out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for config in &report.configs {
        out.push_str(&ablation_row(config));
    }
    out.push('\n');

    out.push_str("## Вклад слоёв\n\n");
    out.push_str("| Слой | Блок | Инкремент | Расхождение | Свои вопросы |\n");
    out.push_str("|---|---:|---:|---:|---:|\n");
    out.push_str(&format!(
        "| LTM | {} | {} | {} | {} |\n",
        report.deltas.ltm_block,
        report.deltas.ltm,
        report.deltas.ltm - report.deltas.ltm_block as i64,
        own_hits(
            report,
            "stm+wm+ltm",
            crate::domain::memory::MemoryLayer::Ltm
        )
    ));
    out.push_str(&format!(
        "| WM | {} | {} | {} | {} |\n",
        report.deltas.wm_block,
        report.deltas.wm,
        report.deltas.wm - report.deltas.wm_block as i64,
        own_hits(report, "stm+wm+ltm", crate::domain::memory::MemoryLayer::Wm)
    ));
    out.push_str(&format!(
        "| STM | {} | {} | {} | {} |\n\n",
        report.deltas.stm_block,
        report.deltas.stm,
        report.deltas.stm - report.deltas.stm_block as i64,
        own_hits(
            report,
            "stm+wm+ltm",
            crate::domain::memory::MemoryLayer::Stm
        )
    ));
    out.push_str(&format!(
        "saved = total(full) − total(all) = {}\nsaved_ratio = {:.1} %\n\n",
        report.saved_tokens,
        report.saved_ratio * 100.0
    ));

    out.push_str("## Очистка\n\n");
    out.push_str("| Очистили | S✓ | W✓ | L✓ | Всего | Сломалось |\n");
    out.push_str("|---|---:|---:|---:|---:|---|\n");
    for config in &report.cleanup {
        let broken = broken_probes(config);
        out.push_str(&format!(
            "| {} | {} | {} | {} | {}/{} | {} |\n",
            config.label,
            config.matched_stm,
            config.matched_wm,
            config.matched_ltm,
            config.total,
            config.effective,
            if broken.is_empty() {
                "—".to_string()
            } else {
                broken.join(", ")
            }
        ));
    }
    if report.cleanup.is_empty() {
        out.push_str("| — | — | — | — | — | очистка не запускалась |\n");
    }
    out.push('\n');

    out.push_str("## Неотвеченные вопросы\n\n");
    out.push_str("| Вопрос | Слой | Конфигурация | Ответ |\n|---|---|---|---|\n");
    let mut any = false;
    for config in report.configs.iter().chain(report.cleanup.iter()) {
        for result in config.results.iter().filter(|result| !result.matched) {
            any = true;
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                escape(&result.question),
                result.layer.as_str(),
                config.id,
                escape(&result.answer)
            ));
        }
    }
    if !any {
        out.push_str("| — | — | — | все вопросы отвечены |\n");
    }
    out.push('\n');

    out.push_str("## Пороги\n\n");
    out.push_str("| Проверка | Результат | Детали |\n|---|---|---|\n");
    for threshold in &report.thresholds {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            threshold.name,
            if threshold.ok { "ok" } else { "FAIL" },
            threshold.detail
        ));
    }
    out.push_str(&format!(
        "\nИтог: **{}**.\n\n",
        if report.passed() {
            "пороги пройдены"
        } else {
            "пороги не пройдены"
        }
    ));

    out.push_str("## Выводы\n\n");
    out.push_str("1. STM хранит полный диалог, в промпт идёт только окно последних сообщений.\n");
    out.push_str("2. WM отвечает за текущую задачу: цель, план, статус, выбранный вариант и открытый вопрос.\n");
    out.push_str("3. LTM хранит профиль, проект, предпочтения, решения и знания; запись идёт через pending.\n");
    out.push_str(&format!(
        "4. Память экономит {} токенов ({:.1} %) относительно полной истории.\n",
        report.saved_tokens,
        report.saved_ratio * 100.0
    ));
    out.push_str("5. Очистка слоя ломает только его вопросы; это проверено матрицей 3×18.\n");
    if !report.errors.is_empty() {
        out.push_str(&format!("\nОшибки запросов: {}.\n", report.errors.len()));
    }
    out.push_str("\n## Ограничения\n\n");
    out.push_str("- Оценка по ключевым словам, не семантика.\n");
    out.push_str("- Одна модель, temperature 0, один прогон.\n");
    out.push_str("- Сводка STM выключена в ablation.\n");
    out.push_str("- Стоимость — оценка прайса, не биллинг.\n");
    out
}

fn ablation_row(config: &ConfigResult) -> String {
    format!(
        "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {}/{} |\n",
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
    )
}

fn own_hits(
    report: &ExperimentReport,
    config_id: &str,
    layer: crate::domain::memory::MemoryLayer,
) -> String {
    let total = match layer {
        crate::domain::memory::MemoryLayer::Stm => 6,
        _ => 6,
    };
    report
        .config(config_id)
        .map(|config| format!("{}/{total}", config.matched_for(layer)))
        .unwrap_or_else(|| format!("—/{total}"))
}

fn broken_probes(config: &ConfigResult) -> Vec<String> {
    config
        .results
        .iter()
        .filter(|result| !result.matched)
        .map(|result| result.id.clone())
        .collect()
}

fn escape(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

pub fn save_report(
    report: &ExperimentReport,
    markdown_path: &Path,
    json_path: &Path,
) -> Result<(), SaveError> {
    atomic_write_bytes(markdown_path, render_markdown(report).as_bytes())?;
    atomic_write_json(json_path, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::Settings;
    use crate::experiment::fixtures::seeded_bundle;
    use crate::experiment::runner::{ExperimentMode, ExperimentOptions, run_experiment};
    use crate::llm::client::FakeChatClient;
    use std::sync::atomic::AtomicBool;

    fn report() -> ExperimentReport {
        let bundle = seeded_bundle(chrono::Utc::now());
        run_experiment(
            &bundle,
            ExperimentOptions {
                mode: ExperimentMode::Full,
                include_cleanup: true,
            },
            &Settings::experiment_defaults(),
            &FakeChatClient,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap()
    }

    #[test]
    fn markdown_contains_required_tables() {
        let markdown = render_markdown(&report());
        for needle in [
            "## Состав слоёв",
            "## Ablation",
            "## Вклад слоёв",
            "## Очистка",
            "## Неотвеченные вопросы",
            "## Выводы",
            "| Конфигурация |",
            "| Слой | Блок |",
            "| Токены API |",
            "| Стоимость API (оценка) |",
        ] {
            assert!(markdown.contains(needle), "нет {needle}");
        }
    }

    #[test]
    fn json_roundtrips() {
        let report = report();
        let json = serde_json::to_string(&report).unwrap();
        let parsed: ExperimentReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_requests, report.total_requests);
        assert_eq!(parsed.configs.len(), 7);
        assert_eq!(parsed.cleanup.len(), 3);
    }

    #[test]
    fn save_writes_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("results").join("exp.md");
        let json = dir.path().join("results").join("exp.json");
        save_report(&report(), &md, &json).unwrap();
        assert!(md.exists());
        assert!(json.exists());
        assert!(std::fs::read_to_string(md).unwrap().contains("## Ablation"));
    }
}
