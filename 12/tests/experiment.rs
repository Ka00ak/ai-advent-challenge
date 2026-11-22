mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::Duration;

use chrono::Utc;
use lab12::app::events::{AppEvent, Job};
use lab12::app::jobs::spawn_job;
use lab12::domain::settings::Settings;
use lab12::experiment::fixtures::seeded_bundle;
use lab12::experiment::report::render_markdown;
use lab12::experiment::runner::{ExperimentMode, ExperimentOptions, run_experiment};
use lab12::llm::client::FakeChatClient;

fn run(cleanup: bool, quick: bool) -> lab12::experiment::ExperimentReport {
    let bundle = seeded_bundle(Utc::now());
    let settings = Settings::experiment_defaults();
    run_experiment(
        &bundle,
        ExperimentOptions {
            mode: if quick {
                ExperimentMode::Quick
            } else {
                ExperimentMode::Full
            },
            include_cleanup: cleanup,
        },
        &settings,
        &FakeChatClient,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap()
}

#[test]
fn tui_experiment_job_sends_individual_api_entries() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = common::runtime_at(dir.path());
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    spawn_job(
        runtime,
        Job::Experiment(ExperimentOptions {
            mode: ExperimentMode::Quick,
            include_cleanup: false,
        }),
        tx,
        cancel,
    );
    let mut api_entries = 0;
    let mut api_events = 0;
    loop {
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(AppEvent::ApiLogged(entries)) => {
                api_entries += entries.len();
                api_events += 1;
            }
            Ok(AppEvent::JobFinished) => break,
            Ok(_) => {}
            Err(error) => panic!("job не завершился: {error}"),
        }
    }
    assert_eq!(
        api_entries, 63,
        "quick-прогон должен дать 63 отдельные записи"
    );
    assert_eq!(api_events, 63, "каждый запрос приходит отдельным событием");
}

#[test]
fn full_fake_run_has_126_requests_and_seven_configs() {
    let report = run(false, false);
    assert_eq!(report.total_requests, 126);
    assert_eq!(report.configs.len(), 7);
    assert!(report.cleanup.is_empty());
    assert_eq!(report.config("none").unwrap().total, 0);
    assert_eq!(report.config("stm").unwrap().total, 6);
    assert_eq!(report.config("wm").unwrap().total, 6);
    assert_eq!(report.config("ltm").unwrap().total, 6);
    assert_eq!(report.config("stm+wm").unwrap().total, 12);
    assert_eq!(report.config("stm+wm+ltm").unwrap().total, 18);
    assert_eq!(report.config("full").unwrap().total, 18);
    assert!(report.passed(), "{:?}", report.thresholds);
}

#[test]
fn cleanup_gives_12_of_18_and_breaks_only_own_layer() {
    let report = run(true, false);
    assert_eq!(report.total_requests, 180);
    assert_eq!(report.cleanup.len(), 3);
    for cleanup in &report.cleanup {
        assert_eq!(cleanup.total, 12);
    }
    let stm = report.cleanup.iter().find(|c| c.id == "clear-stm").unwrap();
    assert_eq!(
        (stm.matched_stm, stm.matched_wm, stm.matched_ltm),
        (0, 6, 6)
    );
    let wm = report.cleanup.iter().find(|c| c.id == "clear-wm").unwrap();
    assert_eq!((wm.matched_stm, wm.matched_wm, wm.matched_ltm), (6, 0, 6));
    let ltm = report.cleanup.iter().find(|c| c.id == "clear-ltm").unwrap();
    assert_eq!(
        (ltm.matched_stm, ltm.matched_wm, ltm.matched_ltm),
        (6, 6, 0)
    );
}

#[test]
fn token_deltas_are_positive_and_converge_with_blocks() {
    let report = run(false, false);
    assert!(report.deltas.ltm > 0);
    assert!(report.deltas.wm > 0);
    assert!(report.deltas.stm > 0);
    let difference = (report.deltas.ltm - report.deltas.ltm_block as i64).abs();
    assert!(difference as f64 / report.deltas.ltm_block as f64 <= 0.10);
    assert!(report.saved_tokens > 0);
    assert!(report.saved_ratio > 0.0);
}

#[test]
fn quick_has_nine_questions() {
    let report = run(false, true);
    assert_eq!(report.total_requests, 63);
    assert_eq!(report.configs[0].results.len(), 9);
}

#[test]
fn markdown_contains_all_tables_and_json_roundtrips() {
    let report = run(true, false);
    let markdown = render_markdown(&report);
    for needle in [
        "## Состав слоёв",
        "## Ablation",
        "## Вклад слоёв",
        "## Очистка",
        "## Неотвеченные вопросы",
        "## Выводы",
    ] {
        assert!(markdown.contains(needle), "нет {needle}");
    }
    let json = serde_json::to_string(&report).unwrap();
    let parsed: lab12::experiment::ExperimentReport = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.total_requests, 180);
}

#[test]
fn controller_saves_report_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::controller_at(dir.path());
    controller
        .replace_bundle(seeded_bundle(Utc::now()))
        .unwrap();
    let report = controller
        .run_experiment(ExperimentOptions::default(), &mut |_| {})
        .unwrap();
    let md = dir.path().join("results").join("exp.md");
    let json = dir.path().join("results").join("exp.json");
    controller.save_report(&report, &md, &json).unwrap();
    assert!(md.exists());
    assert!(json.exists());
    assert!(std::fs::read_to_string(md).unwrap().contains("## Ablation"));
    assert!(
        std::fs::read_to_string(json)
            .unwrap()
            .contains("\"configs\"")
    );
    assert_eq!(controller.exchanges.len(), 126);
    assert!(controller.exchanges.iter().all(|exchange| {
        exchange.kind == lab12::domain::chat::ExchangeKind::Experiment
            && exchange.request_body.is_some()
            && exchange.response_body.is_some()
    }));
}

#[test]
fn cancellation_has_no_report() {
    let bundle = seeded_bundle(Utc::now());
    let settings = Settings::experiment_defaults();
    let cancel = AtomicBool::new(true);
    let result = run_experiment(
        &bundle,
        ExperimentOptions::default(),
        &settings,
        &FakeChatClient,
        &cancel,
        &mut |_| {},
    );
    assert!(result.is_err());
}

#[test]
fn tui_personalization_job_emits_progress_report_and_api_entries() {
    use lab12::experiment::personalization::PersonalizationOptions;
    let dir = tempfile::tempdir().unwrap();
    let runtime = common::runtime_at(dir.path());
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    spawn_job(
        runtime,
        Job::Personalization(PersonalizationOptions::default()),
        tx,
        cancel,
    );
    let mut finished = None;
    let mut progress_events = 0;
    let mut api_entries = 0;
    loop {
        match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(AppEvent::PersonalizationProgress(_)) => progress_events += 1,
            Ok(AppEvent::ApiLogged(entries)) => api_entries += entries.len(),
            Ok(AppEvent::PersonalizationFinished(report)) => finished = Some(report),
            Ok(AppEvent::JobFinished) => break,
            Ok(_) => {}
            Err(error) => panic!("job не завершился: {error}"),
        }
    }
    let report = finished.expect("нет отчёта персонализации");
    assert_eq!(report.total_profiles, 3);
    assert_eq!(report.questions.len(), 10);
    assert_eq!(report.total_requests, 30);
    assert_eq!(report.total_answers, 30);
    assert_eq!(report.compliant_answers, 30);
    assert!(progress_events > 0);
    assert_eq!(api_entries, report.total_requests);
}
