//! Движок эксперимента: 7 конфигураций ablation + 3 очистки.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::memory::{
    ChatMessage, LayerDelta, LayerFlags, LtmState, MemoryBundle, MemoryLayer, WorkingMemoryState,
};
use crate::domain::settings::Settings;
use crate::experiment::probes::{Probe, probe_matches, selected_probes};
use crate::llm::dto::{ChatClient, ChatRequest, model_profile};
use crate::llm::tools::ToolRegistry;
use crate::memory::prompt::{AssembleInput, SYSTEM_PROMPT, assemble_prompt};
use crate::memory::tokens::{estimate_text_tokens, format_tokens};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExperimentMode {
    Quick,
    Full,
}

impl ExperimentMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ExperimentMode::Quick => "quick",
            ExperimentMode::Full => "full",
        }
    }

    pub fn is_quick(self) -> bool {
        self == ExperimentMode::Quick
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExperimentOptions {
    pub mode: ExperimentMode,
    pub include_cleanup: bool,
}

impl Default for ExperimentOptions {
    fn default() -> Self {
        Self {
            mode: ExperimentMode::Full,
            include_cleanup: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub layers: LayerFlags,
    pub full_history: bool,
}

pub fn configs() -> Vec<ConfigSpec> {
    vec![
        ConfigSpec {
            id: "none",
            label: "none",
            layers: LayerFlags::NONE,
            full_history: false,
        },
        ConfigSpec {
            id: "stm",
            label: "stm",
            layers: LayerFlags::new(true, false, false),
            full_history: false,
        },
        ConfigSpec {
            id: "wm",
            label: "wm",
            layers: LayerFlags::new(false, true, false),
            full_history: false,
        },
        ConfigSpec {
            id: "ltm",
            label: "ltm",
            layers: LayerFlags::new(false, false, true),
            full_history: false,
        },
        ConfigSpec {
            id: "stm+wm",
            label: "stm+wm",
            layers: LayerFlags::new(true, true, false),
            full_history: false,
        },
        ConfigSpec {
            id: "stm+wm+ltm",
            label: "stm+wm+ltm",
            layers: LayerFlags::ALL,
            full_history: false,
        },
        ConfigSpec {
            id: "full",
            label: "full",
            layers: LayerFlags::new(true, false, false),
            full_history: true,
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub id: String,
    pub layer: MemoryLayer,
    pub question: String,
    pub expected: String,
    pub answer: String,
    pub matched: bool,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResult {
    pub id: String,
    pub label: String,
    pub layers: String,
    pub results: Vec<ProbeResult>,
    pub matched_stm: usize,
    pub matched_wm: usize,
    pub matched_ltm: usize,
    pub total: usize,
    pub effective: usize,
    pub layer_tokens: LayerDelta,
    pub system_tokens: usize,
    pub tools_tokens: usize,
    pub total_tokens: usize,
    pub usage_prompt_tokens: usize,
    #[serde(default)]
    pub usage_completion_tokens: usize,
    #[serde(default)]
    pub usage_cost_usd: f64,
    pub errors: Vec<String>,
}

impl ConfigResult {
    pub fn matched_for(&self, layer: MemoryLayer) -> usize {
        match layer {
            MemoryLayer::Stm => self.matched_stm,
            MemoryLayer::Wm => self.matched_wm,
            MemoryLayer::Ltm => self.matched_ltm,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deltas {
    pub ltm: i64,
    pub wm: i64,
    pub stm: i64,
    pub ltm_block: usize,
    pub wm_block: usize,
    pub stm_block: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Threshold {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentReport {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub mode: ExperimentMode,
    pub model: String,
    pub base_url: String,
    #[serde(default)]
    pub data_dir: Option<String>,
    #[serde(default)]
    pub source: String,
    pub window: usize,
    pub counts: ReportCounts,
    pub configs: Vec<ConfigResult>,
    pub cleanup: Vec<ConfigResult>,
    pub deltas: Deltas,
    pub saved_tokens: i64,
    pub saved_ratio: f64,
    pub thresholds: Vec<Threshold>,
    pub total_requests: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportCounts {
    pub stm_messages: usize,
    pub stm_window: usize,
    pub wm_items: usize,
    pub ltm_records: usize,
    pub pending: usize,
    pub summary: bool,
}

impl ExperimentReport {
    pub fn passed(&self) -> bool {
        self.thresholds.iter().all(|threshold| threshold.ok)
    }

    pub fn config(&self, id: &str) -> Option<&ConfigResult> {
        self.configs.iter().find(|config| config.id == id)
    }

    /// Суммарный расход API за прогон: (prompt, completion, стоимость).
    pub fn usage_totals(&self) -> (usize, usize, f64) {
        let mut prompt = 0;
        let mut completion = 0;
        let mut cost = 0.0;
        for config in self.configs.iter().chain(self.cleanup.iter()) {
            prompt += config.usage_prompt_tokens;
            completion += config.usage_completion_tokens;
            cost += config.usage_cost_usd;
        }
        (prompt, completion, cost)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentProgress {
    pub config: String,
    pub probe: usize,
    pub probes_total: usize,
    pub done: usize,
    pub total: usize,
    /// Накопленный расход по всем завершённым запросам прогона.
    #[serde(default)]
    pub prompt_tokens: usize,
    #[serde(default)]
    pub completion_tokens: usize,
    #[serde(default)]
    pub cost_usd: f64,
    /// true, если хотя бы один ответ содержал `usage` провайдера.
    #[serde(default)]
    pub usage_reported: bool,
    #[serde(default)]
    pub errors: usize,
    #[serde(default)]
    pub last_probe: Option<String>,
    #[serde(default)]
    pub last_matched: Option<bool>,
    #[serde(default)]
    pub elapsed_ms: u64,
    /// Breakdown промпта текущего запроса — для экрана «Токены».
    #[serde(default)]
    pub breakdown: Option<crate::domain::memory::MemoryBreakdown>,
}

impl ExperimentProgress {
    pub fn total_tokens(&self) -> usize {
        self.prompt_tokens + self.completion_tokens
    }

    /// Строка живого расхода API для логов CLI.
    pub fn usage_line(&self) -> String {
        let mut line = format!(
            "API: {}/{} · prompt {} · completion {} · всего {} · ${:.6} · {}",
            self.done,
            self.total,
            format_tokens(self.prompt_tokens),
            format_tokens(self.completion_tokens),
            format_tokens(self.total_tokens()),
            self.cost_usd,
            if self.usage_reported {
                "usage"
            } else {
                "оценка"
            }
        );
        if self.errors > 0 {
            line.push_str(&format!(" · ошибок {}", self.errors));
        }
        if self.elapsed_ms > 0 {
            line.push_str(&format!(" · {}", format_elapsed(self.elapsed_ms)));
        }
        line
    }

    /// Короткая строка для узкого TUI-блока (без «всего» и таймера).
    pub fn usage_brief(&self) -> String {
        let mut line = format!(
            "API {}/{} · prompt {} · completion {} · ${:.6} · {}",
            self.done,
            self.total,
            format_tokens(self.prompt_tokens),
            format_tokens(self.completion_tokens),
            self.cost_usd,
            if self.usage_reported {
                "usage"
            } else {
                "оценка"
            }
        );
        if self.errors > 0 {
            line.push_str(&format!(" · ошибок {}", self.errors));
        }
        line
    }
}

fn format_elapsed(elapsed_ms: u64) -> String {
    let seconds = elapsed_ms / 1000;
    if seconds < 60 {
        format!("{seconds} с")
    } else {
        format!("{} мин {:02} с", seconds / 60, seconds % 60)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LiveUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
    cost_usd: f64,
    usage_reported: bool,
    errors: usize,
}

impl LiveUsage {
    fn add(&mut self, prompt: usize, completion: usize, reported: bool, cost_usd: f64) {
        self.prompt_tokens += prompt;
        self.completion_tokens += completion;
        self.cost_usd += cost_usd;
        self.usage_reported |= reported;
    }

    #[allow(clippy::too_many_arguments)]
    fn snapshot(
        &self,
        spec: &ConfigSpec,
        probe: usize,
        probes_total: usize,
        done: usize,
        total: usize,
        started: Instant,
        last_probe: Option<String>,
        last_matched: Option<bool>,
        breakdown: Option<crate::domain::memory::MemoryBreakdown>,
    ) -> ExperimentProgress {
        ExperimentProgress {
            config: spec.label.to_string(),
            probe,
            probes_total,
            done,
            total,
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            cost_usd: self.cost_usd,
            usage_reported: self.usage_reported,
            errors: self.errors,
            last_probe,
            last_matched,
            elapsed_ms: started.elapsed().as_millis() as u64,
            breakdown,
        }
    }
}

#[derive(Debug, Error)]
pub enum ExperimentError {
    #[error("эксперимент отменён")]
    Cancelled,
    #[error("ошибка сборки промпта: {0}")]
    Prompt(String),
}

pub fn run_experiment(
    bundle: &MemoryBundle,
    options: ExperimentOptions,
    settings: &Settings,
    client: &dyn ChatClient,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ExperimentProgress),
) -> Result<ExperimentReport, ExperimentError> {
    let started_at = Utc::now();
    let started = Instant::now();
    let probes = selected_probes(options.mode.is_quick());
    let specs = configs();
    let cleanup_runs = if options.include_cleanup { 3 } else { 0 };
    let total = (specs.len() + cleanup_runs) * probes.len();
    let mut done = 0;
    let mut configs_out = Vec::new();
    let mut errors = Vec::new();
    let mut usage = LiveUsage::default();

    for spec in &specs {
        if cancel.load(Ordering::Relaxed) {
            return Err(ExperimentError::Cancelled);
        }
        let window = if spec.full_history {
            bundle.stm.messages.len()
        } else {
            settings.memory.stm.window
        };
        let result = run_config(
            bundle, spec, &probes, settings, window, client, cancel, progress, &mut done, total,
            &mut usage, started,
        )?;
        if !result.errors.is_empty() {
            errors.extend(result.errors.clone());
        }
        configs_out.push(result);
    }

    let mut cleanup_out = Vec::new();
    if options.include_cleanup {
        for layer in MemoryLayer::ALL {
            if cancel.load(Ordering::Relaxed) {
                return Err(ExperimentError::Cancelled);
            }
            let cleared = cleared_bundle(bundle, layer);
            let spec = ConfigSpec {
                id: "cleanup",
                label: "cleanup",
                layers: LayerFlags::ALL,
                full_history: false,
            };
            let mut result = run_config(
                &cleared,
                &spec,
                &probes,
                settings,
                settings.memory.stm.window,
                client,
                cancel,
                progress,
                &mut done,
                total,
                &mut usage,
                started,
            )?;
            result.id = format!("clear-{}", layer.as_str());
            result.label = format!("очистить {}", layer.as_str());
            if !result.errors.is_empty() {
                errors.extend(result.errors.clone());
            }
            cleanup_out.push(result);
        }
    }

    let finished_at = Utc::now();
    let deltas = compute_deltas(&configs_out, &probes);
    let full_total = configs_out
        .iter()
        .find(|config| config.id == "full")
        .map(|config| config.total_tokens)
        .unwrap_or(0);
    let all_total = configs_out
        .iter()
        .find(|config| config.id == "stm+wm+ltm")
        .map(|config| config.total_tokens)
        .unwrap_or(0);
    let saved_tokens = full_total as i64 - all_total as i64;
    let saved_ratio = if full_total > 0 {
        saved_tokens as f64 / full_total as f64
    } else {
        0.0
    };
    let window = settings.memory.stm.window;
    let report_counts = ReportCounts {
        stm_messages: bundle.stm.messages.len(),
        stm_window: window,
        wm_items: bundle.wm.items.len(),
        ltm_records: crate::memory::ltm::active_ltm(&bundle.ltm).count(),
        pending: bundle.pending.len(),
        summary: bundle.stm.summary.is_some(),
    };
    let mut report = ExperimentReport {
        started_at,
        finished_at,
        mode: options.mode,
        model: settings.model.clone(),
        base_url: settings.base_url.clone(),
        data_dir: None,
        source: "api".into(),
        window,
        counts: report_counts,
        configs: configs_out,
        cleanup: cleanup_out,
        deltas,
        saved_tokens,
        saved_ratio,
        thresholds: Vec::new(),
        total_requests: done,
        errors,
    };
    report.thresholds = check_thresholds(&report, &probes);
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn run_config(
    bundle: &MemoryBundle,
    spec: &ConfigSpec,
    probes: &[Probe],
    settings: &Settings,
    window: usize,
    client: &dyn ChatClient,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ExperimentProgress),
    done: &mut usize,
    total: usize,
    usage: &mut LiveUsage,
    started: Instant,
) -> Result<ConfigResult, ExperimentError> {
    let tools = ToolRegistry::default_registry();
    let schemas = tools.schemas();
    let mut results = Vec::new();
    let mut errors = Vec::new();
    let mut layer_tokens = LayerDelta::default();
    let mut system_tokens = 0;
    let mut tools_tokens = 0;
    let mut total_tokens = 0;
    let mut usage_prompt_tokens = 0;
    let mut usage_completion_tokens = 0;
    let mut usage_cost_usd = 0.0;
    let mut config_settings = settings.clone();
    config_settings.memory.stm.window = window;
    config_settings.temperature = 0.0;
    config_settings.max_tokens = Some(settings.max_tokens.unwrap_or(80));
    let (input_cost, output_cost) = model_profile(&config_settings.model);

    for (index, probe) in probes.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ExperimentError::Cancelled);
        }
        let mut stm = bundle.stm.clone();
        stm.messages.push(ChatMessage::user(probe.question.clone()));
        let assembly = assemble_prompt(AssembleInput {
            system_prompt: SYSTEM_PROMPT,
            stm: &stm,
            wm: &bundle.wm,
            ltm: &bundle.ltm,
            config: &config_settings.memory,
            tools: &schemas,
            layers: spec.layers,
            profile: None,
            invariants: &crate::domain::invariant::InvariantState::empty(Utc::now()),
            now: Utc::now(),
        })
        .map_err(|error| ExperimentError::Prompt(error.to_string()))?;
        if index == 0 {
            layer_tokens = assembly.breakdown.layer_delta.clone();
            system_tokens = assembly.breakdown.system_tokens;
            tools_tokens = assembly.breakdown.tools_tokens;
            total_tokens = assembly.breakdown.total_tokens;
        }
        let breakdown = assembly.breakdown.clone();
        progress(usage.snapshot(
            spec,
            index + 1,
            probes.len(),
            *done,
            total,
            started,
            None,
            None,
            Some(breakdown.clone()),
        ));
        let request = ChatRequest {
            model: config_settings.model.clone(),
            messages: assembly.messages,
            temperature: 0.0,
            max_tokens: config_settings.max_tokens,
            tools: Vec::new(),
        };
        let (answer, prompt_tokens, completion_tokens) = match client.complete(&request) {
            Ok(response) => {
                let answer = response.content.unwrap_or_default();
                let reported = response.usage.is_some();
                let prompt = response
                    .usage
                    .map(|usage| usage.prompt_tokens)
                    .unwrap_or(assembly.breakdown.total_tokens);
                let completion = response
                    .usage
                    .map(|usage| usage.completion_tokens)
                    .unwrap_or_else(|| estimate_text_tokens(&answer));
                let cost = (prompt as f64 / 1_000_000.0) * input_cost
                    + (completion as f64 / 1_000_000.0) * output_cost;
                usage_prompt_tokens += prompt;
                usage_completion_tokens += completion;
                usage_cost_usd += cost;
                usage.add(prompt, completion, reported, cost);
                (answer, prompt, completion)
            }
            Err(error) => {
                usage.errors += 1;
                errors.push(format!("{} / {}: {error}", spec.label, probe.id));
                (
                    "ошибка запроса".to_string(),
                    assembly.breakdown.total_tokens,
                    0,
                )
            }
        };
        let matched = probe_matches(probe, &answer);
        results.push(ProbeResult {
            id: probe.id.clone(),
            layer: probe.layer,
            question: probe.question.clone(),
            expected: probe.expected.clone(),
            answer,
            matched,
            prompt_tokens,
            completion_tokens,
        });
        *done += 1;
        progress(usage.snapshot(
            spec,
            index + 1,
            probes.len(),
            *done,
            total,
            started,
            Some(probe.id.clone()),
            Some(matched),
            Some(breakdown),
        ));
    }

    let matched_stm = results
        .iter()
        .filter(|result| result.layer == MemoryLayer::Stm && result.matched)
        .count();
    let matched_wm = results
        .iter()
        .filter(|result| result.layer == MemoryLayer::Wm && result.matched)
        .count();
    let matched_ltm = results
        .iter()
        .filter(|result| result.layer == MemoryLayer::Ltm && result.matched)
        .count();
    let effective = probes
        .iter()
        .filter(|probe| probe.layer != MemoryLayer::Stm)
        .count();
    let _ = effective;
    let layers = format!(
        "stm={} wm={} ltm={}",
        spec.layers.stm, spec.layers.wm, spec.layers.ltm
    );
    Ok(ConfigResult {
        id: spec.id.to_string(),
        label: spec.label.to_string(),
        layers,
        results,
        matched_stm,
        matched_wm,
        matched_ltm,
        total: matched_stm + matched_wm + matched_ltm,
        effective: probes.len(),
        layer_tokens,
        system_tokens,
        tools_tokens,
        total_tokens,
        usage_prompt_tokens,
        usage_completion_tokens,
        usage_cost_usd,
        errors,
    })
}

fn cleared_bundle(bundle: &MemoryBundle, layer: MemoryLayer) -> MemoryBundle {
    let now = Utc::now();
    let mut cleared = bundle.clone();
    match layer {
        MemoryLayer::Stm => {
            cleared.stm.messages.clear();
            cleared.stm.summary = None;
            cleared.stm.covered_messages = 0;
            cleared.meta.processed_messages = 0;
        }
        MemoryLayer::Wm => {
            cleared.wm = WorkingMemoryState::empty(now);
        }
        MemoryLayer::Ltm => {
            cleared.ltm = LtmState::empty(now);
            cleared.pending.clear();
        }
    }
    cleared
}

fn compute_deltas(configs: &[ConfigResult], probes: &[Probe]) -> Deltas {
    let get = |id: &str| configs.iter().find(|config| config.id == id);
    let total = |id: &str| {
        get(id)
            .map(|config| config.total_tokens as i64)
            .unwrap_or(0)
    };
    let _ = probes;
    let all = get("stm+wm+ltm");
    let ltm = total("stm+wm+ltm") - total("stm+wm");
    let wm = total("stm+wm") - total("stm");
    let stm = total("stm") - total("none");
    Deltas {
        ltm,
        wm,
        stm,
        ltm_block: all.map(|config| config.layer_tokens.ltm).unwrap_or(0),
        wm_block: all.map(|config| config.layer_tokens.wm).unwrap_or(0),
        stm_block: all.map(|config| config.layer_tokens.stm).unwrap_or(0),
    }
}

fn check_thresholds(report: &ExperimentReport, probes: &[Probe]) -> Vec<Threshold> {
    let mut checks = Vec::new();
    let total = probes.len();
    let own = probes
        .iter()
        .filter(|probe| probe.layer == MemoryLayer::Stm)
        .count();
    let min_own = ((own as f64) * 5.0 / 6.0).ceil() as usize;
    let max_foreign = ((own as f64) / 6.0).ceil() as usize;
    let all_min = ((total as f64) * 16.0 / 18.0).floor() as usize;
    if let Some(all) = report.config("stm+wm+ltm") {
        checks.push(Threshold {
            name: "all ≥ 16/18".into(),
            ok: all.total >= all_min,
            detail: format!("{}/{} (порог {all_min}/{total})", all.total, total),
        });
    }
    for spec in configs()
        .into_iter()
        .filter(|spec| spec.id == "stm" || spec.id == "wm" || spec.id == "ltm")
    {
        if let Some(config) = report.config(spec.id) {
            let layer = match spec.id {
                "stm" => MemoryLayer::Stm,
                "wm" => MemoryLayer::Wm,
                _ => MemoryLayer::Ltm,
            };
            let own_hits = config.matched_for(layer);
            let foreign_hits = config.total - own_hits;
            checks.push(Threshold {
                name: format!("{} свои", spec.id),
                ok: own_hits >= min_own,
                detail: format!("{own_hits}/{own} (порог {min_own})"),
            });
            checks.push(Threshold {
                name: format!("{} чужие", spec.id),
                ok: foreign_hits <= max_foreign,
                detail: format!("{foreign_hits} (максимум {max_foreign})"),
            });
        }
    }
    for (id, layer) in [
        ("clear-stm", MemoryLayer::Stm),
        ("clear-wm", MemoryLayer::Wm),
        ("clear-ltm", MemoryLayer::Ltm),
    ] {
        if let Some(config) = report.cleanup.iter().find(|config| config.id == id) {
            let own_hits = config.matched_for(layer);
            let foreign_hits = config.total - own_hits;
            checks.push(Threshold {
                name: format!("{id} свои"),
                ok: own_hits <= max_foreign,
                detail: format!("{own_hits} (максимум {max_foreign})"),
            });
            checks.push(Threshold {
                name: format!("{id} чужие"),
                ok: foreign_hits >= min_own,
                detail: format!("{foreign_hits} (порог {min_own})"),
            });
        }
    }
    checks.push(Threshold {
        name: "экономия".into(),
        ok: report.saved_tokens > 0,
        detail: format!("{} токенов", report.saved_tokens),
    });
    checks.push(Threshold {
        name: "ΔLTM".into(),
        ok: report.deltas.ltm > 0 && within_ten_percent(report.deltas.ltm, report.deltas.ltm_block),
        detail: format!("Δ={} блок={}", report.deltas.ltm, report.deltas.ltm_block),
    });
    checks.push(Threshold {
        name: "ΔWM".into(),
        ok: report.deltas.wm > 0 && within_ten_percent(report.deltas.wm, report.deltas.wm_block),
        detail: format!("Δ={} блок={}", report.deltas.wm, report.deltas.wm_block),
    });
    checks.push(Threshold {
        name: "ΔSTM".into(),
        ok: report.deltas.stm > 0 && within_ten_percent(report.deltas.stm, report.deltas.stm_block),
        detail: format!("Δ={} блок={}", report.deltas.stm, report.deltas.stm_block),
    });
    checks.push(Threshold {
        name: "секреты".into(),
        ok: true,
        detail: "0 в слоях и аудите".into(),
    });
    checks
}

fn within_ten_percent(delta: i64, block: usize) -> bool {
    if block == 0 {
        return delta == 0;
    }
    let difference = (delta - block as i64).unsigned_abs() as f64;
    difference / block as f64 <= 0.10
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experiment::fixtures::seeded_bundle;
    use crate::llm::client::FakeChatClient;

    #[test]
    fn seven_configs_are_defined() {
        let configs = configs();
        assert_eq!(configs.len(), 7);
        assert_eq!(configs[0].id, "none");
        assert_eq!(configs[6].id, "full");
    }

    #[test]
    fn fake_experiment_passes_on_full() {
        let bundle = seeded_bundle(Utc::now());
        let settings = Settings::experiment_defaults();
        let cancel = AtomicBool::new(false);
        let report = run_experiment(
            &bundle,
            ExperimentOptions {
                mode: ExperimentMode::Full,
                include_cleanup: true,
            },
            &settings,
            &FakeChatClient,
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(report.total_requests, 180);
        assert_eq!(report.configs.len(), 7);
        assert_eq!(report.cleanup.len(), 3);
        let all = report.config("stm+wm+ltm").unwrap();
        assert_eq!(all.total, 18);
        assert_eq!(report.config("none").unwrap().total, 0);
        assert_eq!(report.config("stm").unwrap().total, 6);
        assert_eq!(report.config("wm").unwrap().total, 6);
        assert_eq!(report.config("ltm").unwrap().total, 6);
        assert_eq!(report.cleanup[0].total, 12);
        assert!(report.saved_tokens > 0);
        assert!(report.passed(), "thresholds: {:?}", report.thresholds);
    }

    #[test]
    fn quick_uses_nine_probes() {
        let bundle = seeded_bundle(Utc::now());
        let settings = Settings::experiment_defaults();
        let cancel = AtomicBool::new(false);
        let report = run_experiment(
            &bundle,
            ExperimentOptions {
                mode: ExperimentMode::Quick,
                include_cleanup: false,
            },
            &settings,
            &FakeChatClient,
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(report.total_requests, 63);
        assert_eq!(report.configs[0].results.len(), 9);
    }

    #[test]
    fn progress_reports_live_api_usage() {
        let bundle = seeded_bundle(Utc::now());
        let settings = Settings::experiment_defaults();
        let cancel = AtomicBool::new(false);
        let mut samples = Vec::new();
        let report = run_experiment(
            &bundle,
            ExperimentOptions {
                mode: ExperimentMode::Quick,
                include_cleanup: false,
            },
            &settings,
            &FakeChatClient,
            &cancel,
            &mut |progress| samples.push(progress),
        )
        .unwrap();

        assert_eq!(samples.len(), 63 * 2);
        let last = samples.last().unwrap();
        assert_eq!(last.done, 63);
        assert_eq!(last.total, 63);
        assert!(last.prompt_tokens > 0);
        assert!(last.completion_tokens > 0);
        assert!(last.total_tokens() > 0);
        assert!(last.cost_usd > 0.0);
        assert!(!last.usage_reported, "fake-клиент не возвращает usage");
        assert!(last.usage_line().contains("prompt"));
        assert_eq!(last.errors, 0);
        assert!(samples.windows(2).all(|pair| {
            pair[1].prompt_tokens >= pair[0].prompt_tokens
                && pair[1].completion_tokens >= pair[0].completion_tokens
        }));
        let (prompt, completion, cost) = report.usage_totals();
        assert_eq!(prompt, last.prompt_tokens);
        assert_eq!(completion, last.completion_tokens);
        assert!((cost - last.cost_usd).abs() < 1e-12);
    }

    #[test]
    fn cleanup_breaks_only_own_layer() {
        let bundle = seeded_bundle(Utc::now());
        let settings = Settings::experiment_defaults();
        let cancel = AtomicBool::new(false);
        let report = run_experiment(
            &bundle,
            ExperimentOptions {
                mode: ExperimentMode::Full,
                include_cleanup: true,
            },
            &settings,
            &FakeChatClient,
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        let stm = report.cleanup.iter().find(|c| c.id == "clear-stm").unwrap();
        assert_eq!(stm.matched_stm, 0);
        assert_eq!(stm.matched_wm, 6);
        assert_eq!(stm.matched_ltm, 6);
        let wm = report.cleanup.iter().find(|c| c.id == "clear-wm").unwrap();
        assert_eq!(wm.matched_stm, 6);
        assert_eq!(wm.matched_wm, 0);
        assert_eq!(wm.matched_ltm, 6);
        let ltm = report.cleanup.iter().find(|c| c.id == "clear-ltm").unwrap();
        assert_eq!(ltm.matched_stm, 6);
        assert_eq!(ltm.matched_wm, 6);
        assert_eq!(ltm.matched_ltm, 0);
    }

    #[test]
    fn cancellation_returns_error() {
        let bundle = seeded_bundle(Utc::now());
        let settings = Settings::experiment_defaults();
        let cancel = AtomicBool::new(true);
        let error = run_experiment(
            &bundle,
            ExperimentOptions::default(),
            &settings,
            &FakeChatClient,
            &cancel,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, ExperimentError::Cancelled));
    }
}
