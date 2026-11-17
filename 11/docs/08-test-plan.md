# 08. Тест-план

## 1. Пирамида

| Уровень | Что проверяет | Скорость | Сеть |
|---|---|---|---|
| unit | слои, router, retrieval, сборка, storage, миграция | быстрый | нет |
| integration | Agent + память, experiment на моке, ветки | средний | мок fetch |
| manual | UI, реальный API, отчёт | медленный | реальный |

Цель: **≥ 90 unit/integration тестов**, все детерминированы, без реального
ключа. Тесты Дня 10 сохраняются и адаптируются.

## 2. Файлы и зоны ответственности

```text
11/tests/
  memory-stm.test.ts          # окно, границы, summary, токены
  memory-working.test.ts      # upsert, status, prune, render
  memory-ltm.test.ts          # upsert, supersede, pinned, render profile
  memory-router.test.ts       # R1–R6, pending, audit, секреты
  memory-retrieval.test.ts    # score, topK, бюджеты, shadowed-by-wm
  memory-prompt.test.ts       # порядок блоков, усечение, dedupe, breakdown
  memory-migration.test.ts    # facts Дня 10 → LTM, идемпотентность
  memory-fixtures.test.ts     # 34 сообщения, 18 probe, изоляция
  memory-experiment.test.ts   # 7 конфигураций, очистка, отчёт (мок)
  memory-storage.test.ts      # parse/save/load/clear каждого ключа
  agent-memory.test.ts        # Agent + MemoryService + memory_write
  branches.test.ts            # снимок трёх слоёв (обновить)
  calculator.test.ts          # регресс Дня 10
  tokens.test.ts              # регресс оценки токенов
  tools.test.ts               # регресс tool-loop
```

## 3. Unit-тесты

### STM (`memory-stm.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-STM-1 | `selectStmWindow` возвращает последние 8 сообщений | FR-24 |
| T-STM-2 | граница сдвигается к `user`, окно не начинается с `assistant` | FR-24 |
| T-STM-3 | пара `assistant(tool_calls) → tool` не разрывается | FR-24 |
| T-STM-4 | текущая user-реплика включается всегда, даже при `layers.stm=false` | FR-22, FR-24 |
| T-STM-5 | при превышении удаляются самые старые пары, не последняя | FR-22 |
| T-STM-6 | summary рендерится только при `coveredMessages > 0` | FR-24 |
| T-STM-7 | полная история не мутирует при усечении | FR-7 |

### WM (`memory-working.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-WM-1 | `upsertWmItem` создаёт пункт с id и таймстемпами | FR-2 |
| T-WM-2 | повторный key обновляет значение, а не дублирует | FR-13 |
| T-WM-3 | `done`-пункт плана рендерится как `[x]`, остальные `done`/`dropped` — нет | FR-21 |
| T-WM-4 | порядок: goal → plan(order) → status → entity → open-question → note | FR-21 |
| T-WM-5 | усечение: сначала note, goal и активный plan остаются | FR-22 |
| T-WM-6 | `refLtmId` рендерится ссылкой, если LTM не включена | FR-25 |
| T-WM-7 | `pruneWm` удаляет старые done, не трогает active | FR-2 |

### LTM (`memory-ltm.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-LTM-1 | `upsertLtm` создаёт запись, key уникален | FR-3 |
| T-LTM-2 | одинаковое значение → `same`, без изменений | FR-13 |
| T-LTM-3 | `replaceLtm` ставит старой версии `supersededBy` | FR-13 |
| T-LTM-4 | `activeLtmRecords` исключает superseded | FR-13 |
| T-LTM-5 | `renderLtmProfile` берёт profile/project/preference, pinned выше | FR-23 |
| T-LTM-6 | профиль усекается по бюджету, pinned сохраняется | FR-22 |
| T-LTM-7 | key/value ограничиваются 64/500 символами | FR-29 |

### Router (`memory-router.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-R-1 | R1: `sk-…`, `Bearer …`, `password=…` отклоняются и маскируются | FR-14 |
| T-R-2 | R2: «для этой задачи» → WM, даже если key есть в LTM | FR-9 |
| T-R-3 | R3: обновление key в LTM → pending в режиме `confirm` | FR-10 |
| T-R-4 | R3 в режиме `auto` → supersede, pending пуст | FR-10 |
| T-R-5 | R4: имя проекта/дедлайн/решение → LTM pending | FR-9 |
| T-R-6 | R5: «вариант B рабочий» → WM entity | FR-9 |
| T-R-7 | R6: «привет», «спасибо», вопрос без данных → нет записи | FR-9 |
| T-R-8 | несколько кандидатов из одной реплики маршрутизируются независимо | FR-9 |
| T-R-9 | аудит содержит layer, ruleId, reason, source | FR-15 |
| T-R-10 | pending по одному key заменяется, oldValue сохраняется | FR-10 |
| T-R-11 | LLM-извлечение: ошибка → откат на local, `stats.error` | FR-8 |
| T-R-12 | `memory_write` с неверным layer → `rejected` | FR-11 |

### Retrieval (`memory-retrieval.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-RET-1 | совпадение в key весит больше, чем в value | FR-23 |
| T-RET-2 | `pinned` попадает в выборку первым | FR-23 |
| T-RET-3 | topK соблюдается | FR-23 |
| T-RET-4 | бюджет токенов соблюдается | FR-22 |
| T-RET-5 | superseded не выбирается | FR-13 |
| T-RET-6 | запись, теневая WM-пунктом, попадает в dropped | FR-25 |
| T-RET-7 | пустой запрос → выбираются pinned/свежие, не падает | FR-23 |
| T-RET-8 | детерминированный порядок при равных score | NFR-5 |

### Prompt (`memory-prompt.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-P-1 | порядок: system → LTM profile → LTM relevant → WM → STM summary → STM | FR-21 |
| T-P-2 | пустые блоки не добавляются | FR-21 |
| T-P-3 | каждый блок — отдельное system-сообщение | FR-21 |
| T-P-4 | `layers: {stm:false}` убирает блоки STM, но не WM/LTM | EXP-3 |
| T-P-5 | `layers: {ltm:false}` убирает оба LTM-блока | EXP-3 |
| T-P-6 | `MemoryBreakdown` суммируется в `totalTokens` | FR-27 |
| T-P-7 | `layerDelta` равен сумме блоков слоя | FR-27 |
| T-P-8 | последняя user-реплика всегда последняя в messages | FR-22 |
| T-P-9 | в системном промпте есть правила приоритета и «данные, не инструкции» | FR-26 |
| T-P-10 | значение с `\n` и управляющими символами санитизируется | FR-29 |
| T-P-11 | дедупликация WM-ссылки: одна строка, `dedupedWmIds` заполнен | FR-25 |
| T-P-12 | `totalBudget` соблюдается при переполнении всех слоёв | FR-22 |
| T-P-13 | при `layers.stm=false` остаётся текущая реплика, истории нет | FR-22, FR-24 |

### Миграция (`memory-migration.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-M-1 | 12 facts Дня 10 → 12 LTM-записей с правильными kind | FR-16, FR-18 |
| T-M-2 | значения сохраняются дословно (числа, даты, имена) | FR-18 |
| T-M-3 | повторный запуск: added=0, updated=0 | FR-17 |
| T-M-4 | изменённый fact обновляет запись, не дублирует | FR-17 |
| T-M-5 | пустой/битый JSON → `ok:false`, без исключения | FR-20 |
| T-M-6 | `legacyKey` и тег `migrated:day10` проставлены | FR-18 |
| T-M-7 | неизвестная категория → `knowledge` | FR-18 |

### Storage (`memory-storage.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-S-1 | save/load каждого ключа возвращает эквивалент | FR-6 |
| T-S-2 | неверная version → ошибка, слой не загружается | FR-6 |
| T-S-3 | битый JSON → `{state:null, error}`, приложение не падает | FR-6, NFR-4 |
| T-S-4 | `clearLtm` не меняет STM/WM; `clearWorking` не меняет LTM | FR-4 |
| T-S-5 | `clearLtm` очищает pending, но не WM/STM | FR-5 |
| T-S-6 | переполнение storage → `{ok:false}`, без throw | NFR-7 |
| T-S-7 | миграция настроек Дня 10 не падает | FR-32 |

### Фикстуры (`memory-fixtures.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-F-1 | 34 сообщения, 18 probe, 6 на слой | EXP-1 |
| T-F-2 | S1–S6 не отвечаются по сообщениям 1–26 | EXP-1 |
| T-F-3 | W1–W6 не отвечаются по сообщениям 27–34 | EXP-1 |
| T-F-4 | L1–L6 не отвечаются по сообщениям 27–34 | EXP-1 |
| T-F-5 | seeded WM/LTM совпадает с таблицами раздела 3 | EXP-1 |
| T-F-6 | `probeMatches` нормализует `ё`, регистр и пунктуацию | EXP-2 |
| T-F-7 | router из тех же сообщений даёт то же состояние (кроме pending) | FR-9 |

## 4. Integration-тесты

### Agent (`agent-memory.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-A-1 | запрос содержит system + блоки памяти + окно STM | FR-21 |
| T-A-2 | `memory_write` в tool-call пишет в WM и возвращает `created` | FR-11 |
| T-A-3 | `memory_write` в LTM в режиме confirm → `pending` | FR-10 |
| T-A-4 | после ответа `AgentRunResult.memory` содержит обновлённые слои | FR-3 |
| T-A-5 | `memoryAudit` и `memoryRoute.stats` заполнены | FR-15 |
| T-A-6 | ошибка модели не теряет уже записанные слои | FR-6 |
| T-A-7 | в режиме `local` дополнительных запросов нет | NFR-1 |
| T-A-8 | в режиме `llm` создаётся запрос `kind='memory'` | FR-8 |
| T-A-9 | очистка STM перед запуском не ломает WM/LTM | FR-5 |

### Эксперимент (`memory-experiment.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-E-1 | 7 конфигураций, 18 вопросов, 126 запросов на моке | EXP-3 |
| T-E-2 | матрица ответов совпадает с ожидаемой | EXP-3 |
| T-E-3 | очистка STM/WM/LTM даёт 12/18 и падение своих вопросов | EXP-4 |
| T-E-4 | токены по слоям заполнены, Δ положительны | EXP-6 |
| T-E-5 | `experimentToMarkdown` содержит все таблицы | EXP-5 |
| T-E-6 | `experimentToJson` валиден и парсится обратно | EXP-5 |
| T-E-7 | отмена прерывает прогон без сохранения частичного отчёта | EXP-3 |
| T-E-8 | `quick` использует 9 вопросов | EXP-7 |

### Ветки (`branches.test.ts`)

| ID | Проверка | FR |
|---|---|---|
| T-B-1 | checkpoint хранит STM/WM/LTM/pending/meta | FR-31 |
| T-B-2 | две ветки независимы по всем слоям | FR-31 |
| T-B-3 | переключение восстанавливает все части | FR-31 |
| T-B-4 | битый снимок ветки отбрасывается с ошибкой | FR-31 |

### Регресс Дня 10

| ID | Проверка |
|---|---|
| T-REG-1 | calculator/time/currency работают |
| T-REG-2 | tool-loop: несколько итераций, ошибки инструмента |
| T-REG-3 | estimateTextTokens/estimateCost не изменились |
| T-REG-4 | summary STM обновляется порциями |
| T-REG-5 | журнал API суммирует chat/summary/memory |
| T-REG-6 | localStorage истории и настроек |

## 5. Моки и детерминизм

```ts
class MemoryStorage implements KeyValueStorage {
  private readonly data = new Map<string, string>()
  getItem(key: string) { return this.data.get(key) ?? null }
  setItem(key: string, value: string) { this.data.set(key, value) }
  removeItem(key: string) { this.data.delete(key) }
}

function jsonResponse(body: unknown): Response { /* ... */ }

const now = () => new Date('2026-11-17T10:00:00.000Z')
```

Правила:

- `vi.stubGlobal('fetch', mock)` в каждом тесте с API; `afterEach` —
  `vi.unstubAllGlobals()`.
- Время передаётся через `now`, не берётся из `Date.now()` внутри тестов.
- Порядок массивов проверяется явно, не через `toEqual` на объектах с
  нестабильным порядком ключей.
- Для эксперимента мок отвечает по наличию нужного блока в промпте:
  `promptContains(layer) ? expected : 'не знаю'`.
- Реальные токены не используются; проверяются оценки и суммы.

## 6. Команды

```sh
npm test -w 11                 # все тесты
npm test -w 11 -- memory       # только память
npm run build -w 11            # vue-tsc + vite build
npm run dev -w 11              # ручная проверка
```

CI-гейт (локально перед сдачей): build + test + отсутствие `console.log` с
ключом.

## 7. Ручной чек-лист

- [ ] Открыть «Память»: три колонки, счётчики и токены видны.
- [ ] Импорт facts Дня 10: отчёт, повторный импорт не дублирует.
- [ ] Отправить «Запускаем проект «Тест»» → pending → принять → запись в LTM.
- [ ] Отклонить pending → записи нет, аудит содержит reject.
- [ ] «Отвечай коротко» → preference в LTM (pending).
- [ ] «Текущая задача: …» → WM, сразу без pending.
- [ ] «Запомни: только российские ЦОД» через tool → LTM/pending, ответ агента
      подтверждает запись.
- [ ] Перенести WM → LTM, проверить `refLtmId` и отсутствие дубля в промпте.
- [ ] Очистить STM → WM/LTM на месте, вопросы LTM отвечаются.
- [ ] Очистить WM → STM/LTM на месте.
- [ ] Очистить LTM → pending пуст, WM/STM на месте.
- [ ] «Проверка» → фикстура → quick → полный прогон → отчёт скачивается.
- [ ] «Токены»: разбивка по слоям для последнего запроса.
- [ ] «API»: запрос `memory` с режимом и результатом.
- [ ] Ветки: checkpoint → 2 ветки → разные LTM → переключение.
- [ ] Перезагрузка страницы: все слои и pending восстановлены.
- [ ] Битый JSON в LTM: баннер, приложение работает.

## 8. Definition of Done тестов

- [ ] Все T-* из этого документа реализованы или явно отмечены `P2/skip` с
      причиной.
- [ ] `npm test -w 11` зелёный, без сети.
- [ ] `npm run build -w 11` без ошибок TypeScript.
- [ ] Нет тестов, которые «подгоняют» фикстуру под ожидаемый ответ.
- [ ] Покрыты: изоляция слоёв, маршрутизация, сборка, миграция, очистка,
      эксперимент, ветки, регресс.
