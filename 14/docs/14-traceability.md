# День 14 — трассируемость

| Требование | Реализация | Тесты |
|---|---|---|
| Инварианты хранятся отдельно от диалога | `data/invariants/invariants.json`, `src/storage/{paths,mod}.rs` | `tests/invariants.rs::invariants_survive_controller_reload` |
| Категории: архитектура, техрешение, стек, бизнес-правило | `InvariantCategory`, `with_defaults` | `domain::invariant::tests::defaults_cover_four_categories` |
| Инварианты явно учитываются в рассуждениях | блок `## Инварианты проекта` в `assemble_prompt` после профиля | `memory::prompt::tests::invariant_block_is_after_profile_and_before_memory`, `tests/invariants.rs::allowed_request_carries_invariants_in_prompt_and_is_marked_checked` |
| Ассистент отказывается нарушать | `refusal_text`, детерминированный отказ | `invariants::engine::tests::direct_conflict_is_blocked_by_code` |
| Проверка запроса кодом до сети | `send_user_message` → `assess_request` до LLM | `tests/invariants.rs::direct_conflict_is_refused_before_llm_and_audited` |
| Проверка ответа кодом до выдачи | `assess_response` после LLM, замена ответа | `tests/invariants.rs::violating_model_response_is_replaced_by_refusal` |
| Конфликт в задаче | проверка цели и каждого этапа в `run_task` | `task_goal_conflict_fails_without_llm_request`, `violating_task_stage_output_is_blocked_before_done` |
| Изменение только явной командой | `InvariantState::change` из CLI/TUI, причина обязательна | `domain::invariant::tests::change_requires_reason_and_records_old_new`, `tests/invariants.rs::dialogue_cannot_change_invariant_but_explicit_command_can`, `tests/cli.rs::invariants_set_requires_reason` |
| Изменение фиксирует «было → стало» | `InvariantChange.old_* / new_* / reason / source` | `tests/cli.rs::invariants_list_set_history_and_probe_work_without_key` |
| 5–10 попыток обхода | 10 формулировок, `invariants probe`, вкладка `9` `p` | `invariants::engine::tests::ten_bypass_formulations_are_all_blocked`, `tests/invariants.rs::ten_bypass_attempts_through_chat_are_all_blocked_and_counted` |
| Подсчёт попыток | `InvariantAuditState`, `attempt_count/blocked_count/probe_count` | `invariants::engine` и `tests/cli.rs` |
| Объяснение отказа | ID, правило, совпадение, запрет обхода, команда изменения | `invariants::engine::tests::direct_conflict_is_blocked_by_code` |
| Новая вкладка 9 | `Screen::Invariants`, `widgets/invariants.rs` | `tests/tui.rs::number_keys_switch_screens`, `invariants_screen_lists_and_requires_explicit_edit_command` |
| Единое ядро: память + профиль + автомат + инварианты | `AppController`, блоки промпта, экраны `1`, `7`, `8`, `9` | полный `cargo test` |
| Секреты не попадают в аудит | маскирование до `assess_request` | `tests/invariants.rs::no_secret_reaches_invariant_audit` |

## Команды

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
```

Фактический прогон: `cargo fmt` чистый, clippy без предупреждений,
`cargo test` — **329 тестов**, 0 падений.
