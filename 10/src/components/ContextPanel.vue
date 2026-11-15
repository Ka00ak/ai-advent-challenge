<script setup lang="ts">
import { computed } from 'vue'
import type { CompressionConfig, ContextSummary } from '../agent/compression'
import {
  buildContextMessages,
  contextStrategyLabel,
  contextStrategyMeta,
  CONTEXT_STRATEGIES,
  normalizeContextConfig,
  type ContextPreview,
  type ContextStrategy,
} from '../agent/context'
import type { FactMode, StickyFactsState } from '../agent/facts'
import type {
  BenchmarkMode,
  MemoryBenchmarkReport,
  MemoryModeResult,
} from '../memory/benchmark'
import { messagesContainDetail, type MemoryDetail } from '../memory/dialog'
import { FACTS_STORAGE_KEY } from '../storage/facts'
import { SUMMARY_STORAGE_KEY } from '../storage/summary'
import {
  formatPercent,
  formatTokens,
  formatUsd,
  type ModelProfile,
} from '../tokens'
import type { ChatMessage } from '../types'

const props = defineProps<{
  messages: ChatMessage[]
  summary: ContextSummary | null
  facts: StickyFactsState | null
  strategy: ContextStrategy
  factsMode: FactMode
  compression: CompressionConfig
  profile: ModelProfile
  preview: ContextPreview
  benchmark: MemoryBenchmarkReport | null
  benchmarkRunning: boolean
  benchmarkStatus: string
  compressing: boolean
  factsBusy: boolean
  busy: boolean
  details: MemoryDetail[]
}>()

const emit = defineEmits<{
  'seed-dialog': []
  'compress-now': []
  'rebuild-facts': []
  'clear-summary': []
  'clear-facts': []
  'run-benchmark': []
  'cancel-benchmark': []
}>()

const config = computed(() =>
  normalizeContextConfig({
    strategy: props.strategy,
    keepRecent: props.compression.keepRecent,
    compression: props.compression,
    factsMode: props.factsMode,
  }),
)
const meta = computed(() => contextStrategyMeta(props.strategy))
const sentMessages = computed(() =>
  buildContextMessages(props.messages, config.value, props.summary, props.facts),
)

const detailRows = computed(() =>
  props.details.map(detail => ({
    detail,
    inFull: messagesContainDetail(props.messages, detail),
    inSent: messagesContainDetail(sentMessages.value, detail),
  })),
)

const summaryResult = computed(() => modeResult('summary'))
const slidingResult = computed(() => modeResult('sliding'))
const factsResult = computed(() => modeResult('facts'))
const fullResult = computed(() => modeResult('full'))

function modeResult(mode: BenchmarkMode): MemoryModeResult | null {
  return props.benchmark?.strategies.find(result => result.mode === mode) ?? null
}

const comparisonRows = computed(() =>
  props.details.map(detail => ({
    detail,
    full: findCheck(fullResult.value, detail.id),
    sliding: findCheck(slidingResult.value, detail.id),
    facts: findCheck(factsResult.value, detail.id),
    summary: findCheck(summaryResult.value, detail.id),
  })),
)

function findCheck(result: MemoryModeResult | null, detailId: string) {
  return result?.checks.find(check => check.detailId === detailId)
}

const complexityRows = computed(() => {
  const runtime = (mode: BenchmarkMode) => modeResult(mode)
  return CONTEXT_STRATEGIES.map(strategy => {
    const result = runtime(strategy.id)
    return {
      ...strategy,
      remembered: result ? `${result.remembered} / ${result.total}` : '—',
      requests: result ? result.extractionRequests : 0,
      contextTokens: result?.contextTokens ?? null,
      netPromptTokens: result?.netPromptTokens ?? null,
      code:
        strategy.id === 'sliding'
          ? '~20 строк: findRecentStart + slice'
          : strategy.id === 'facts'
            ? '~220 строк: facts.ts + LLM extractor'
            : '~300 строк: compression.ts + summarizer.ts',
    }
  })
})

const bestStrategy = computed(() => {
  const report = props.benchmark
  if (!report) return null
  const candidates = report.strategies.filter(result => result.mode !== 'full')
  return [...candidates].sort((a, b) => {
    if (b.remembered !== a.remembered) return b.remembered - a.remembered
    return a.netPromptTokens - b.netPromptTokens
  })[0] ?? null
})

function formatTime(iso?: string): string {
  if (!iso) return '—'
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleString('ru-RU')
}

function sign(value: number): string {
  return value >= 0 ? '−' : '+'
}

function checkClass(remembered?: boolean): string {
  if (remembered === undefined) return ''
  return remembered ? 'ok' : 'bad'
}

function checkMark(remembered?: boolean): string {
  if (remembered === undefined) return '—'
  return remembered ? '✓' : '✗'
}
</script>

<template>
  <div class="memory">
    <section class="field-group">
      <h2>Текущая стратегия</h2>

      <div class="strategy-hero" :class="`strategy-${props.strategy}`">
        <div class="strategy-head">
          <strong>{{ contextStrategyLabel(props.strategy) }}</strong>
          <span class="badge" :class="props.strategy === 'sliding' ? 'bad' : 'ok'">
            {{ meta.difficulty }}
          </span>
        </div>
        <p class="strategy-description">{{ meta.description }}</p>

        <div class="compress-grid">
          <div>
            <span class="muted">полная история</span>
            <strong>{{ formatTokens(props.preview.fullHistoryTokens) }}</strong>
          </div>
          <div>
            <span class="muted">в запросе</span>
            <strong>{{ formatTokens(props.preview.sentHistoryTokens) }}</strong>
          </div>
          <div>
            <span class="muted">facts</span>
            <strong>{{ formatTokens(props.preview.factsTokens) }}</strong>
          </div>
          <div>
            <span class="muted">summary</span>
            <strong>{{ formatTokens(props.preview.summaryTokens) }}</strong>
          </div>
          <div>
            <span class="muted">экономия</span>
            <strong :class="props.preview.savedTokens >= 0 ? 'good' : 'bad'">
              {{ sign(props.preview.savedTokens) }}{{ formatTokens(Math.abs(props.preview.savedTokens)) }}
            </strong>
          </div>
          <div>
            <span class="muted">facts в памяти</span>
            <strong>{{ props.facts?.facts.length ?? 0 }}</strong>
          </div>
        </div>

        <div class="gauge small">
          <span
            class="gauge-fill saved"
            :style="{ width: `${Math.min(100, Math.max(0, props.preview.savedRatio * 100))}%` }"
          />
        </div>

        <div class="context-meta">
          <span>экономия {{ formatPercent(Math.max(0, props.preview.savedRatio)) }}</span>
          <span>
            отброшено/сжато {{ props.preview.droppedMessages }} ·
            свежих {{ props.preview.recentMessages }}
          </span>
        </div>

        <p class="hint">
          {{ meta.extraRequest }} · {{ meta.bestFor }}. Слабое место: {{ meta.loses }}.
        </p>

        <div class="history-actions">
          <button
            v-if="props.strategy === 'facts'"
            type="button"
            class="btn primary small"
            :disabled="props.busy || !props.messages.length"
            @click="emit('rebuild-facts')"
          >
            {{ props.factsBusy ? 'Обновляю facts…' : 'Обновить facts' }}
          </button>
          <button
            v-if="props.strategy === 'facts'"
            type="button"
            class="btn ghost small"
            :disabled="props.busy || !props.facts?.facts.length"
            @click="emit('clear-facts')"
          >
            Очистить facts
          </button>
          <button
            v-if="props.strategy === 'summary'"
            type="button"
            class="btn primary small"
            :disabled="props.busy || !props.messages.length"
            @click="emit('compress-now')"
          >
            {{ props.compressing ? 'Сжимаю…' : 'Сжать сейчас' }}
          </button>
          <button
            v-if="props.strategy === 'summary'"
            type="button"
            class="btn ghost small"
            :disabled="props.busy || !props.summary"
            @click="emit('clear-summary')"
          >
            Очистить сводку
          </button>
          <button
            type="button"
            class="btn ghost small"
            :disabled="props.busy"
            @click="emit('seed-dialog')"
          >
            Загрузить контрольный диалог
          </button>
        </div>
      </div>
    </section>

    <section v-if="props.strategy === 'facts'" class="field-group">
      <h2>Sticky Facts · ключ → значение</h2>

      <div v-if="props.facts?.facts.length" class="facts-card">
        <div class="facts-meta">
          <span><strong>{{ props.facts.facts.length }}</strong> фактов</span>
          <span>{{ props.facts.updates }} обновл.</span>
          <span>{{ props.facts.mode === 'llm' ? 'LLM' : 'local' }}</span>
          <span>обработано {{ props.facts.processedMessages }} сообщений</span>
        </div>
        <div class="facts-table">
          <div class="facts-head">
            <span>ключ</span>
            <span>значение</span>
            <span>тип</span>
          </div>
          <div v-for="fact in props.facts.facts" :key="fact.key" class="facts-row">
            <code>{{ fact.key }}</code>
            <span>{{ fact.value }}</span>
            <span class="detail-category">{{ fact.category }}</span>
          </div>
        </div>
        <p class="hint">
          Facts лежат в <code>{{ FACTS_STORAGE_KEY }}</code> и не входят в массив
          <code>messages</code>. Именно этот блок уходит в модель вместе с
          последними {{ props.preview.keepRecent }} сообщениями.
        </p>
      </div>
      <p v-else class="hint">
        Facts пока пусты. Нажми «Обновить facts» или отправь сообщение — память
        обновится после каждой реплики пользователя.
      </p>
    </section>

    <section v-if="props.summary" class="field-group">
      <h2>Summary · хранится отдельно</h2>
      <div class="summary-card">
        <div class="summary-meta">
          <span><strong>{{ props.summary.coveredMessages }}</strong> сообщений в сводке</span>
          <span>{{ props.summary.chunks }} обновл.</span>
          <span>{{ props.summary.mode === 'llm' ? 'LLM' : 'local' }}</span>
        </div>
        <div class="muted summary-time">
          обновлена {{ formatTime(props.summary.updatedAt) }} ·
          <code>{{ SUMMARY_STORAGE_KEY }}</code>
        </div>
        <pre class="summary-text">{{ props.summary.text }}</pre>
      </div>
    </section>

    <section class="field-group">
      <h2>Детали для проверки</h2>
      <p class="hint">
        12 фактов из первых сообщений. «Полная» — есть ли деталь в истории,
        «в запросе» — осталась ли она в том, что реально уходит в модель при
        текущей стратегии. Это локальная проверка без API.
      </p>

      <div class="detail-table">
        <div class="detail-head">
          <span>деталь</span>
          <span>полная</span>
          <span>в запросе</span>
        </div>
        <div v-for="row in detailRows" :key="row.detail.id" class="detail-row">
          <div class="detail-fact">
            <span class="detail-category">{{ row.detail.category }}</span>
            <strong>{{ row.detail.fact }}</strong>
          </div>
          <span class="check" :class="row.inFull ? 'ok' : 'bad'">
            {{ row.inFull ? '✓' : '✗' }}
          </span>
          <span class="check" :class="row.inSent ? 'ok' : 'bad'">
            {{ row.inSent ? '✓' : '✗' }}
          </span>
        </div>
      </div>
    </section>

    <section class="field-group">
      <h2>Сравнение трёх стратегий</h2>

      <p class="hint">
        Один и тот же диалог и 12 вопросов прогоняются в четырёх режимах:
        полная история (baseline), Sliding Window, Sticky Facts и Summary. Это
        48 запросов к модели; при LLM-режиме facts добавляются запросы на
        извлечение памяти. Ответ считается верным, если в нём есть ожидаемая
        деталь. Нужен API-ключ.
      </p>

      <div class="history-actions">
        <button
          type="button"
          class="btn primary small"
          :disabled="props.busy"
          @click="emit('run-benchmark')"
        >
          {{ props.benchmarkRunning ? 'Проверяю…' : 'Проверить все стратегии' }}
        </button>
        <button
          v-if="props.benchmarkRunning"
          type="button"
          class="btn danger small"
          @click="emit('cancel-benchmark')"
        >
          Остановить
        </button>
      </div>

      <p v-if="props.benchmarkStatus" class="hint benchmark-status">
        {{ props.benchmarkStatus }}
      </p>

      <template v-if="props.benchmark">
        <div class="benchmark-cards strategy-cards">
          <div
            v-for="result in props.benchmark.strategies"
            :key="result.mode"
            class="benchmark-card"
          >
            <span class="muted">{{ result.label }}</span>
            <strong>{{ result.remembered }} / {{ result.total }}</strong>
            <span>{{ formatTokens(result.promptTokens) }} prompt</span>
            <span>
              память: {{ formatTokens(result.extractionPromptTokens) }} tok ·
              {{ result.extractionRequests }} запр.
            </span>
            <span>
              net: {{ formatTokens(result.netPromptTokens) }} tok ·
              {{ formatUsd(result.costUsd + result.extractionCostUsd) }}
            </span>
          </div>
        </div>

        <p class="hint">
          «prompt» — только вопросы. «память» — запросы на построение facts или
          summary. «net» — prompt + память: честная цена стратегии за прогон.
        </p>

        <div class="benchmark-table strategy-table">
          <div class="benchmark-head">
            <span>деталь</span>
            <span>full</span>
            <span>sliding</span>
            <span>facts</span>
            <span>summary</span>
          </div>
          <div v-for="row in comparisonRows" :key="row.detail.id" class="benchmark-row">
            <div class="benchmark-fact">
              <strong>{{ row.detail.fact }}</strong>
              <details class="benchmark-answer">
                <summary>ответы модели</summary>
                <p><span class="muted">full:</span> {{ row.full?.answer || '—' }}</p>
                <p><span class="muted">sliding:</span> {{ row.sliding?.answer || '—' }}</p>
                <p><span class="muted">facts:</span> {{ row.facts?.answer || '—' }}</p>
                <p><span class="muted">summary:</span> {{ row.summary?.answer || '—' }}</p>
              </details>
            </div>
            <span class="check" :class="checkClass(row.full?.remembered)">
              {{ checkMark(row.full?.remembered) }}
            </span>
            <span class="check" :class="checkClass(row.sliding?.remembered)">
              {{ checkMark(row.sliding?.remembered) }}
            </span>
            <span class="check" :class="checkClass(row.facts?.remembered)">
              {{ checkMark(row.facts?.remembered) }}
            </span>
            <span class="check" :class="checkClass(row.summary?.remembered)">
              {{ checkMark(row.summary?.remembered) }}
            </span>
          </div>
        </div>

        <p v-if="bestStrategy" class="hint">
          Лучший баланс в этом прогоне: <strong>{{ bestStrategy.label }}</strong> —
          {{ bestStrategy.remembered }} / {{ bestStrategy.total }} деталей,
          {{ formatTokens(bestStrategy.netPromptTokens) }} net prompt-токенов.
          Отчётов в сессии: старт {{ formatTime(props.benchmark.startedAt) }} ·
          финиш {{ formatTime(props.benchmark.finishedAt) }}.
        </p>
      </template>
      <p v-else-if="!props.benchmarkRunning" class="hint">
        Отчёта пока нет. Загрузи контрольный диалог и запусти проверку.
      </p>
    </section>

    <section class="field-group">
      <h2>Стратегия → детали / токены / сложность</h2>

      <div class="complexity-table">
        <div class="complexity-head">
          <span>стратегия</span>
          <span>детали</span>
          <span>запросы</span>
          <span>код</span>
        </div>
        <div v-for="row in complexityRows" :key="row.id" class="complexity-row">
          <div class="complexity-name">
            <strong>{{ row.label }}</strong>
            <span class="muted">{{ row.bestFor }}</span>
          </div>
          <span>{{ row.remembered }}</span>
          <span>
            {{ row.requests }}
            <span v-if="row.netPromptTokens !== null" class="muted">
              · {{ formatTokens(row.netPromptTokens) }} tok
            </span>
          </span>
          <span class="complexity-code">{{ row.code }}</span>
        </div>
      </div>

      <p class="hint">
        «Запросы» — реальные дополнительные вызовы модели в последнем прогоне.
        У Sliding Window их ноль. У Sticky Facts в LLM-режиме — по одному на
        каждую реплику пользователя. У Summary — по одному на порцию старых
        сообщений. «Код» — ориентир по модулям, которые нужно поддерживать.
      </p>
    </section>

    <section class="field-group">
      <h2>Вывод: что выбирать</h2>
      <div class="conclusion">
        <p>
          <strong>Sliding Window</strong> — когда важна только свежая переписка:
          короткая сессия, поддержка, FAQ. Нулевая сложность и цена, но детали
          старше окна исчезают полностью.
        </p>
        <p>
          <strong>Sticky Facts</strong> — когда в диалоге много конкретики:
          сроки, бюджеты, имена, ограничения, решения. Дороже Sliding Window на
          один запрос к модели за реплику (или бесплатно в local-режиме), зато
          ключевые детали не зависят от размера окна.
        </p>
        <p>
          <strong>Summary</strong> — когда важен общий смысл длинного свободного
          диалога, а не таблица фактов. Компромисс: один запрос на порцию
          сообщений и риск потерять число или имя при пересказе.
        </p>
        <p class="muted">
          На контрольном диалоге из 24 сообщений Sliding Window с окном 8 не
          видит ни одной из 12 деталей, а Facts и Summary — видят почти все.
          Facts дают более предсказуемую память, Summary — более связный контекст.
        </p>
      </div>
    </section>
  </div>
</template>
