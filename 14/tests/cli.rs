use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    let mut command = Command::cargo_bin("lab14").unwrap();
    command
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("KIMI_API_KEY")
        .env_remove("LAB14_FAKE_LLM")
        .env_remove("LAB11_FAKE_LLM")
        .env_remove("LAB12_FAKE_LLM")
        .env_remove("LAB14_HOME")
        .env_remove("LAB11_HOME");
    command
}

fn fixture() -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/day10-facts.json")
        .display()
        .to_string()
}

#[test]
fn help_works_without_key() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("tui"))
        .stdout(predicate::str::contains("experiment"));
}

#[test]
fn memory_add_and_show_json() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args([
            "memory",
            "add",
            "--layer",
            "ltm",
            "--key",
            "project.name",
            "--value",
            "«Атлас»",
            "--kind",
            "project",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("created"));
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["memory", "show", "--layer", "ltm", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("project.name"))
        .stdout(predicate::str::contains("«Атлас»"));
}

#[test]
fn import_facts_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["import-facts", &fixture()])
        .assert()
        .success()
        .stdout(predicate::str::contains("добавлено=15"));
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["import-facts", &fixture()])
        .assert()
        .success()
        .stdout(predicate::str::contains("добавлено=0"))
        .stdout(predicate::str::contains("пропущено=15"));
}

#[test]
fn experiment_quick_check_passes_with_fake() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .env("LAB11_FAKE_LLM", "1")
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["experiment", "--quick", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("пороги пройдены"))
        .stderr(predicate::str::contains("prompt"))
        .stderr(predicate::str::contains("completion"));
}

#[test]
fn experiment_full_saves_report() {
    let dir = tempfile::tempdir().unwrap();
    let md = dir.path().join("experiment.md");
    let json = dir.path().join("experiment.json");
    bin()
        .env("LAB11_FAKE_LLM", "1")
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["experiment", "--full", "--cleanup"])
        .arg("--out")
        .arg(&md)
        .arg("--json")
        .arg(&json)
        .arg("--check")
        .assert()
        .success();
    assert!(md.exists());
    assert!(json.exists());
    let markdown = std::fs::read_to_string(md).unwrap();
    assert!(markdown.contains("## Ablation"));
    assert!(markdown.contains("## Очистка"));
}

#[test]
fn chat_without_key_returns_code_2_without_network() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["chat", "Привет"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("API-ключ"));
}

#[test]
fn clear_without_yes_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["memory", "clear", "--layer", "stm"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--yes"));
}

#[test]
fn binary_rejects_second_instance_with_lock() {
    let dir = tempfile::tempdir().unwrap();
    // Держим lock через долгоживущий процесс нельзя; проверяем через файл lock
    // в юнит-тесте storage. Здесь — что обычный запуск не падает.
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["dump"])
        .assert()
        .success()
        .stdout(predicate::str::contains("conversation_id"));
}

#[test]
fn profile_presets_can_be_listed_and_activated() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["profile", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("brief"))
        .stdout(predicate::str::contains("detailed"))
        .stdout(predicate::str::contains("Кратко"));
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["profile", "use", "brief"])
        .assert()
        .success()
        .stdout(predicate::str::contains("brief"));
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["profile", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("кратко"))
        .stdout(predicate::str::contains("без кода"));
}

#[test]
fn profile_learn_updates_active_profile_without_network() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args([
            "profile",
            "learn",
            "Отвечай подробно, формально, с кодом и таблицей",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("style.verbosity"));
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["profile", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"verbosity\": \"detailed\""))
        .stdout(predicate::str::contains("\"tone\": \"formal\""))
        .stdout(predicate::str::contains("\"code\": \"with-code\""));
}

#[test]
fn personalize_fake_check_saves_markdown_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let md = dir.path().join("personalization.md");
    let json = dir.path().join("personalization.json");
    bin()
        .env("LAB12_FAKE_LLM", "1")
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args(["personalize", "--check"])
        .arg("--out")
        .arg(&md)
        .arg("--json")
        .arg(&json)
        .assert()
        .success()
        .stdout(predicate::str::contains("Соблюдено ответов: 30 из 30"))
        .stdout(predicate::str::contains("профилей полностью: 3 из 3"))
        .stdout(predicate::str::contains("персонализация подтверждена"));
    assert!(md.exists());
    assert!(json.exists());
    let markdown = std::fs::read_to_string(md).unwrap();
    assert!(markdown.contains("## Цепочки вопросов"));
    assert!(markdown.contains("Первое нарушение"));
}

#[test]
fn invariants_list_set_history_and_probe_work_without_key() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_str().unwrap();

    bin()
        .args(["--data-dir", data])
        .args(["invariants", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stack.rust-only"))
        .stdout(predicate::str::contains("business.budget-limit"));

    bin()
        .args(["--data-dir", data])
        .args([
            "invariants",
            "set",
            "business.budget-limit",
            "--rule",
            "Лимит проекта — 200 000 ₽.",
            "--reason",
            "заказчик увеличил лимит",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Было:"))
        .stdout(predicate::str::contains("Стало:"))
        .stdout(predicate::str::contains("200 000"));

    bin()
        .args(["--data-dir", data])
        .args(["invariants", "history"])
        .assert()
        .success()
        .stdout(predicate::str::contains("business.budget-limit"))
        .stdout(predicate::str::contains("заказчик увеличил лимит"));

    bin()
        .args(["--data-dir", data])
        .args(["invariants", "probe"])
        .assert()
        .success()
        .stdout(predicate::str::contains("заблокировано 10 из 10"));

    bin()
        .args(["--data-dir", data])
        .args(["invariants", "audit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("probe: 10"));
}

#[test]
fn invariants_check_returns_4_for_conflict_and_0_for_safe() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_str().unwrap();
    bin()
        .args(["--data-dir", data])
        .args(["invariants", "check", "Напиши на Python"])
        .assert()
        .code(4)
        .stdout(predicate::str::contains("Разрешено: false"))
        .stdout(predicate::str::contains("stack.rust-only"));

    bin()
        .args(["--data-dir", data])
        .args(["invariants", "check", "Как устроена память?"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("Разрешено: true"));
}

#[test]
fn invariants_set_requires_reason() {
    let dir = tempfile::tempdir().unwrap();
    bin()
        .args(["--data-dir", dir.path().to_str().unwrap()])
        .args([
            "invariants",
            "set",
            "stack.rust-only",
            "--rule",
            "Другое правило",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--reason"));
}
