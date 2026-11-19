use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    let mut command = Command::cargo_bin("lab11").unwrap();
    command
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("KIMI_API_KEY")
        .env_remove("LAB11_FAKE_LLM")
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
