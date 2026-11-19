//! Текстовые таблицы и JSON для CLI.

use crate::domain::chat::{LlmExchange, TokenStats, UsageSummary, summarize_exchanges};
use crate::domain::memory::{MemoryBundle, MemoryLayer};
use crate::domain::profile::UserProfile;
use crate::experiment::personalization::PersonalizationReport;
use crate::experiment::runner::ExperimentReport;
use crate::memory::tokens::format_tokens;

pub fn print_bundle(bundle: &MemoryBundle, layer: Option<MemoryLayer>) {
    match layer {
        Some(MemoryLayer::Stm) | None => print_stm(bundle),
        _ => {}
    }
    match layer {
        Some(MemoryLayer::Wm) | None => print_wm(bundle),
        _ => {}
    }
    match layer {
        Some(MemoryLayer::Ltm) | None => print_ltm(bundle),
        _ => {}
    }
}

fn print_stm(bundle: &MemoryBundle) {
    println!(
        "STM: {} сообщений, сводка: {}",
        bundle.stm.messages.len(),
        if bundle.stm.summary.is_some() {
            "есть"
        } else {
            "нет"
        }
    );
    for (index, message) in bundle.stm.messages.iter().enumerate() {
        println!(
            "  #{:<3} {:<9} {}",
            index + 1,
            format!("{:?}", message.role).to_lowercase(),
            message.preview(100)
        );
    }
}

fn print_wm(bundle: &MemoryBundle) {
    println!(
        "WM: {}, пунктов: {}",
        if bundle.wm.task_title.is_empty() {
            "(задача не задана)"
        } else {
            &bundle.wm.task_title
        },
        bundle.wm.items.len()
    );
    for item in &bundle.wm.items {
        let order = item
            .order
            .map(|order| format!("{order}."))
            .unwrap_or_else(|| "-".into());
        println!(
            "  {} [{}] {:<13} {} · {}",
            order,
            item.status.as_str(),
            item.kind.as_str(),
            item.key,
            item.value
        );
    }
}

fn print_ltm(bundle: &MemoryBundle) {
    let active: Vec<&crate::domain::memory::LtmRecord> = bundle
        .ltm
        .records
        .iter()
        .filter(|record| record.is_active())
        .collect();
    println!("LTM: {} активных записей", active.len());
    for record in active {
        println!(
            "  {}{:<10} {:<24} {}",
            if record.pinned { "★ " } else { "  " },
            record.kind.as_str(),
            record.key,
            record.value
        );
    }
    if !bundle.pending.is_empty() {
        println!("Pending: {}", bundle.pending.len());
        for pending in &bundle.pending {
            println!(
                "  {} {:<24} {} → {}",
                pending.rule_id,
                pending.candidate.key,
                pending.old_value.as_deref().unwrap_or("—"),
                pending.candidate.value
            );
        }
    }
}

pub fn print_token_stats(stats: &TokenStats) {
    println!(
        "Токены: prompt={} completion={} total={} cost={:.6}$",
        stats.prompt_tokens, stats.completion_tokens, stats.total_tokens, stats.cost_usd
    );
}

pub fn print_breakdown(breakdown: &crate::domain::memory::MemoryBreakdown) {
    println!("Breakdown:");
    println!("  system      {}", breakdown.system_tokens);
    println!("  профиль     {}", breakdown.profile_tokens);
    println!("  LTM профиль {}", breakdown.ltm_profile_tokens);
    println!("  LTM релевант {}", breakdown.ltm_relevant_tokens);
    println!("  WM          {}", breakdown.wm_tokens);
    println!("  STM сводка  {}", breakdown.stm_summary_tokens);
    println!("  STM окно    {}", breakdown.stm_tokens);
    println!("  вопрос      {}", breakdown.current_tokens);
    println!("  tools       {}", breakdown.tools_tokens);
    println!("  Итого       {}", breakdown.total_tokens);
    println!(
        "  слои: ltm={} wm={} stm={}",
        breakdown.layer_delta.ltm, breakdown.layer_delta.wm, breakdown.layer_delta.stm
    );
}

pub fn print_exchanges(exchanges: &[LlmExchange]) {
    let summary: UsageSummary = summarize_exchanges(exchanges);
    println!(
        "Запросы: {} (chat={}, memory={}, experiment={})",
        summary.requests,
        summary.chat_requests,
        summary.memory_requests,
        summary.experiment_requests
    );
    println!(
        "Токены: prompt={} completion={} total={} cost={:.6}$",
        summary.prompt_tokens, summary.completion_tokens, summary.total_tokens, summary.cost_usd
    );
}

pub fn print_experiment(report: &ExperimentReport) {
    println!(
        "Эксперимент {} · {} · запросов {}",
        report.mode.as_str(),
        report.model,
        report.total_requests
    );
    let (prompt_tokens, completion_tokens, cost_usd) = report.usage_totals();
    println!(
        "API: prompt {} · completion {} · всего {} · ${:.6}",
        format_tokens(prompt_tokens),
        format_tokens(completion_tokens),
        format_tokens(prompt_tokens + completion_tokens),
        cost_usd
    );
    println!();
    println!("Конфигурация   LTM   WM  STM  System Tools  Total   S W L  Всего");
    for config in &report.configs {
        println!(
            "{:<13} {:>5} {:>4} {:>4} {:>7} {:>5} {:>6}   {} {} {}  {}/{}",
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
    }
    if !report.cleanup.is_empty() {
        println!();
        println!("Очистка        S W L  Всего");
        for config in &report.cleanup {
            println!(
                "{:<13} {} {} {}  {}/{}",
                config.id,
                config.matched_stm,
                config.matched_wm,
                config.matched_ltm,
                config.total,
                config.effective
            );
        }
    }
    println!();
    println!(
        "Вклад: ΔLTM={} (блок {}), ΔWM={} (блок {}), ΔSTM={} (блок {})",
        report.deltas.ltm,
        report.deltas.ltm_block,
        report.deltas.wm,
        report.deltas.wm_block,
        report.deltas.stm,
        report.deltas.stm_block
    );
    println!(
        "Экономия: {} токенов ({:.1} %)",
        format_tokens(report.saved_tokens.max(0) as usize),
        report.saved_ratio * 100.0
    );
    for threshold in &report.thresholds {
        println!(
            "  [{}] {} — {}",
            if threshold.ok { "ok" } else { "FAIL" },
            threshold.name,
            threshold.detail
        );
    }
    println!(
        "Итог: {}",
        if report.passed() {
            "пороги пройдены"
        } else {
            "пороги не пройдены"
        }
    );
}

pub fn print_profile(profile: &UserProfile, active: Option<&str>) {
    println!(
        "{} {} · {}",
        if active == Some(profile.id.as_str()) {
            "*"
        } else {
            " "
        },
        profile.id,
        profile.title
    );
    println!("  предпочтения: {}", profile.summary());
    println!(
        "  пользователь: {} · роль: {} · язык: {}",
        profile.identity.name.as_deref().unwrap_or("—"),
        profile.identity.role.as_deref().unwrap_or("—"),
        profile.identity.language
    );
    if !profile.constraints.is_empty() {
        println!("  ограничения: {}", profile.constraints.join("; "));
    }
    println!(
        "  источник: {} · revision: {} · обновлён: {}",
        profile.source.as_str(),
        profile.revision,
        profile.updated_at.to_rfc3339()
    );
    if !profile.preferences.is_empty() {
        println!("  история изменений:");
        for preference in profile.preferences.iter().rev().take(8) {
            println!(
                "    {} = {} ({}{})",
                preference.key,
                preference.value,
                preference.source.as_str(),
                preference
                    .evidence
                    .as_deref()
                    .map(|evidence| format!(": {evidence}"))
                    .unwrap_or_default()
            );
        }
    }
}

pub fn print_personalization(report: &PersonalizationReport) {
    println!(
        "Персонализация · {} · запросов {} · источник {}",
        report.model,
        report.total_requests,
        if report.source.is_empty() {
            "api"
        } else {
            &report.source
        }
    );
    let (prompt, completion, cost) = report.usage_totals();
    println!(
        "API: prompt {} · completion {} · всего {} · ${:.6}",
        format_tokens(prompt),
        format_tokens(completion),
        format_tokens(prompt + completion),
        cost
    );
    println!();
    println!("Профиль        Предпочтения                                      Соблюдено");
    for profile in &report.profiles {
        println!(
            "{:<14} {:<48} {}",
            profile.profile_id,
            profile.preferences,
            if profile.compliant { "да" } else { "нет" }
        );
    }
    println!(
        "Соблюдено профилей: {} из {}",
        report.compliant_profiles, report.total_profiles
    );
    if let Some(stability) = &report.stability {
        println!();
        println!(
            "Устойчивость ({}) · {}/{} · первое нарушение: {}",
            stability.profile_id,
            stability.compliant_count,
            stability.total,
            stability
                .first_violation
                .map(|index| format!("#{index}"))
                .unwrap_or_else(|| "нет".into())
        );
    }
    for threshold in &report.thresholds {
        println!(
            "  [{}] {} — {}",
            if threshold.ok { "ok" } else { "FAIL" },
            threshold.name,
            threshold.detail
        );
    }
    println!(
        "Итог: {}",
        if report.passed() {
            "персонализация подтверждена"
        } else {
            "есть нарушения предпочтений"
        }
    );
}
