# День 15 — трассируемость

| Требование | Реализация | Тесты |
|---|---|---|
| Правила LTM: replace / append / task-scoped | `FactPolicy`, `policy_for`, `src/memory/lifecycle.rs` | `memory::lifecycle::tests::policies_are_explicit`, `tests/lifecycle.rs::append_facts_are_stored_as_a_list` |
| Заменяемые факты (срок, статус, версия, подход, требование) | `canonical_key`, `policy_for`, `candidates.rs` | `memory::lifecycle::tests::aliases_map_to_one_key`, `memory::ltm::tests::alias_key_replaces_existing_record`, `tests/lifecycle.rs::dialogue_updates_four_facts...` |
| Накапливаемые списком (решения, история) | `FactPolicy::Append`, `upsert_ltm`, `LtmRevision` | `memory::ltm::tests::append_policy_accumulates_without_superseding`, `tests/lifecycle.rs::append_facts_are_stored_as_a_list` |
| Удаление после завершения задачи | `cleanup_after_task`, `AppController::finish_task_memory` | `memory::lifecycle::tests::cleanup_removes_task_scope_and_keeps_project`, `tests/lifecycle.rs::task_completion_clears_wm...`, `successful_task_auto_cleans_memory` |
| У каждого факта время записи | `LtmRecord.recorded_at`, `write_time()`, рендер `записано ...` | `tests/lifecycle.rs::dialogue_updates_four_facts...`, `memory::prompt` |
| Конфликт заменяет, а не добавляет рядом | R3 в `router.rs`, `replace_ltm` | `memory::ltm::tests::replace_sets_superseded_by`, `tests/router.rs::r3_conflict_replaces_old_value_even_in_confirm_mode` |
| Более поздняя запись приоритетнее | сравнение `recorded_at` в `upsert_ltm`/`replace_ltm`, `LtmChange::Stale` | `memory::ltm::tests::earlier_recorded_at_cannot_override_later_value` |
| Замена в журнале «было → стало» | `LtmRevision`, `record_revision`, `LtmState.revisions` | `memory::lifecycle::tests::record_revision_keeps_old_and_new`, `tests/lifecycle.rs::dialogue_updates...` |
| Старые значения не попадают в промпт | активные записи + `stale_values` + `mask_stale_values` в `assemble_prompt` | `memory::lifecycle::tests::stale_old_value_is_masked_but_current_message_kept`, `tests/lifecycle.rs` (0 stale из 8 промптов) |
| Агент после изменения называет актуальное значение | роутер обновляет LTM до сборки промпта | `tests/lifecycle.rs::dialogue_updates_four_facts...` (0 stale ответов) |
| Очистка WM, сохранение LTM | `cleanup_after_task`, `finish_task_memory` | `tests/lifecycle.rs::task_completion_clears_wm...`, `successful_task_auto_cleans_memory` |
| Работа через вкладку 1 | `send_user_message` → `route_with` → `assemble_prompt` → `run_turn` | `tests/lifecycle.rs`, `tests/controller.rs` |
| Журнал и правила в CLI | `memory conflicts`, `memory policies` | `tests/cli.rs::memory_conflict_log_and_policies_work_without_key` |
| Журнал в TUI | `Screen::Updates`, `widgets/updates.rs`, клавиша `0` | `tests/tui.rs::number_keys_switch_screens`, `updates_screen_shows_conflict_log...`, `every_screen_renders_without_panic` |
| Совместимость со старыми JSON | serde defaults для `policy/scope/revision/recorded_at/revisions` | `tests/storage.rs`, полный `cargo test` |

## Команды

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
cargo test --test lifecycle dialogue_updates -- --nocapture
```

Фактический прогон: `cargo fmt` чистый, clippy без предупреждений,
`cargo test` — **349 тестов**, 0 падений. Сценарий изменений:
`changes=4 prompts_checked=8 stale_prompts=0 stale_answers=0`.
