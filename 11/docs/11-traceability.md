# 11. Трассируемость

Живой документ. Агент обновляет статус и тесты по мере работы.

Статусы: `spec` — описано, `todo` — в работе, `done` — принято, `p2` — отложено.

## Функциональные требования

| ID | Кратко | Задача | Тесты | Статус |
|---|---|---|---|---|
| FR-1 | STM в отдельных файлах | T2, T3 | T-STM-1…7, T-S-4 | spec |
| FR-2 | WM в отдельном файле | T2, T3 | T-WM-1…7 | spec |
| FR-3 | LTM в отдельном файле | T2, T3 | T-LTM-1…7 | spec |
| FR-4 | Нет записи между слоями | T2, T3 | T-S-4 | spec |
| FR-5 | Независимая очистка | T2, T6 | T-S-4, T-S-5, T-A-9 | spec |
| FR-6 | Атомарность, битый JSON | T2 | T-S-1…3, T-S-7 | spec |
| FR-7 | STM не удаляется при усечении | T3 | T-STM-7 | spec |
| FR-8 | Data-dir | T2 | ручной/CLI | spec |
| FR-9 | Lock | T2 | T-S-8 | spec |
| FR-10 | Извлечение Local/Llm | T4 | T-R-11, T-A-7, T-A-8 | spec |
| FR-11 | Router R1–R6 | T4 | T-R-1…8 | spec |
| FR-12 | Pending/Approval | T4 | T-R-3, T-R-4, T-R-10, T-A-3 | spec |
| FR-13 | Tool `memory_write` | T6 | T-R-12, T-A-2, T-A-3 | spec |
| FR-14 | Ручные операции | T9, T10 | ручной, CLI | spec |
| FR-15 | Обновление/версии | T3, T4 | T-WM-2, T-LTM-2…4 | spec |
| FR-16 | Секреты | T1, T4 | T-R-1, T-P-10, T-S-9 | spec |
| FR-17 | Аудит | T4, T9 | T-R-9, T-A-5 | spec |
| FR-18 | TUI+CLI паритет | T6, T9, T10 | CLI, T-A-10 | spec |
| FR-19 | Импорт facts | T7 | T-M-1 | spec |
| FR-20 | Идемпотентность | T7 | T-M-3, T-M-4 | spec |
| FR-21 | Маппинг/дословность | T7 | T-M-1, T-M-2, T-M-6, T-M-7 | spec |
| FR-22 | Отчёт импорта | T7, T9 | T-M-5 | spec |
| FR-23 | Нет facts — нет падения | T7 | T-M-5 | spec |
| FR-24 | Блок на слой | T5 | T-P-1…5 | spec |
| FR-25 | Бюджеты/усечение | T5 | T-STM-4, T-STM-5, T-WM-5, T-LTM-6, T-P-12 | spec |
| FR-26 | LTM profile + relevant | T5 | T-RET-1…7, T-LTM-5 | spec |
| FR-27 | STM окно/current | T3, T5 | T-STM-1…6, T-P-13 | spec |
| FR-28 | Дедупликация | T5 | T-WM-6, T-RET-6, T-P-11 | spec |
| FR-29 | Приоритет в промпте | T5 | T-P-9 | spec |
| FR-30 | Breakdown | T5 | T-P-6, T-P-7 | spec |
| FR-31 | Breakdown в UI/CLI | T6, T10 | T-A-10 | spec |
| FR-32 | Безопасность промпта | T1, T5 | T-P-10 | spec |
| FR-33 | Инструменты/tokens/cost | T6 | T-REG-1…3 | spec |
| FR-34 | Ветки | T10 | T-B-1…4 | spec |
| FR-35 | Настройки Дня 10 | T10 | T-REG-5 | spec |
| FR-36 | Cargo в корне | T0 | build/run | spec |
| FR-37 | .gitignore | T0 | git status | spec |

## UX

| ID | Кратко | Задача | Проверка | Статус |
|---|---|---|---|---|
| UX-1 | Три слоя в TUI | T9 | T-UI-1, T-UI-4 | spec |
| UX-2 | Карточка записи | T9 | ручной | spec |
| UX-3 | Pending-экран | T9 | T-UI-2 | spec |
| UX-4 | Очистка с модалкой | T9 | T-UI-3 | spec |
| UX-5 | Проверка с прогрессом | T11 | ручной, T-E-7 | spec |
| UX-6 | Токены по слоям | T10 | ручной | spec |
| UX-7 | Клавиатура/футер | T9 | T-UI-1 | spec |
| UX-8 | Ошибки/пустые | T9, T10 | T-UI-6 | spec |

## NFR

| ID | Кратко | Задача | Тесты | Статус |
|---|---|---|---|---|
| NFR-1 | 0 доп. запросов Local | T4, T6 | T-A-7 | spec |
| NFR-2 | Сборка ≤ 50 мс | T5 | benchmark | spec |
| NFR-3 | Ключ не в файлах | T1, T2, T6 | T-R-1, T-S-9 | spec |
| NFR-4 | Битый JSON не блокирует | T2 | T-S-3 | spec |
| NFR-5 | Тесты без сети/терминала | T12 | cargo test | spec |
| NFR-6 | Build/clippy | все | CI | spec |
| NFR-7 | TUI ≤ 300/100 мс | T9 | ручной | spec |
| NFR-8 | Resize/80×24 | T9, T10 | T-UI-4/5 | spec |
| NFR-9 | Atomic/0600 | T2 | T-S-7/9 | spec |
| NFR-10 | Нет exit в контроллере | T6, T12 | тест | spec |
| NFR-11 | Логи не в stdout | T9 | ручной | spec |

## Эксперимент

| ID | Кратко | Задача | Тесты | Статус |
|---|---|---|---|---|
| EXP-1 | Фикстура 34/18 | T8 | T-F-1…5 | spec |
| EXP-2 | Probe/keywords | T8 | T-F-6 | spec |
| EXP-3 | 7 конфигураций | T8 | T-E-1/2 | spec |
| EXP-4 | Очистка | T8 | T-E-3 | spec |
| EXP-5 | Отчёт MD/JSON | T8, T11 | T-E-5/6 | spec |
| EXP-6 | Токены/дельты | T8 | T-E-4 | spec |
| EXP-7 | quick/full | T8, T11 | T-E-8 | spec |
| EXP-8 | --check | T8 | T-E-9 | spec |

## Артефакты

| Артефакт | Задача | Статус |
|---|---|---|
| Cargo-проект `11/` | T0…T12 | spec |
| Отчёт `docs/results/experiment-*.md` | T13 | spec |
| Пользовательский `README.md` | T13 | spec |
| Зелёные build/test/clippy | T14 | spec |

## Правило обновления

1. Взяв задачу — поставь `todo`.
2. Закрыв — `done`, добавь фактические пути тестов.
3. Отложено — `p2` + причина в `10-risks-adr.md`.
4. Строки не удалять.
