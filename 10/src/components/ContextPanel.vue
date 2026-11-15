<script setup lang="ts">
import { computed } from 'vue'
import {
  buildCompressedContext,
  previewCompression,
  type CompressionConfig,
  type ContextSummary,
} from '../agent/compression'
import type { MemoryBenchmarkReport } from '../memory/benchmark'
import { messagesContainDetail, type MemoryDetail } from '../memory/dialog'
import { SUMMARY_STORAGE_KEY } from '../storage/summary'
import {
  estimateCost,
  formatPercent,
  formatTokens,
  formatUsd,
  type ModelProfile,
} from '../tokens'
import type { ChatMessage } from '../types'

const props = defineProps<{
  messages: ChatMessage[]
  summary: ContextSummary | null
  compression: CompressionConfig
  profile: ModelProfile
  benchmark: MemoryBenchmarkReport | null
  benchmarkRunning: boolean
  benchmarkStatus: string
  compressing: boolean
  busy: boolean
  details: MemoryDetail[]
}>()

const emit = defineEmits<{
  'seed-dialog': []
  'compress-now': []
  'clear-summary': []
  'run-benchmark': []
  'cancel-benchmark': []
}>()

const preview = computed(() =>
  previewCompression(props.messages, props.compression, props.summary),
)
const sentMessages = computed(() =>
  buildCompressedContext(props.messages, props.compression, props.summary),
)

const detailRows = computed(() =>
  props.details.map(detail => ({
    detail,
    inFull: messagesContainDetail(props.messages, detail),
    inSent: messagesContainDetail(sentMessages.value, detail),
  })),
)

const fullCost = computed(
  () => estimateCost(props.profile, preview.value.fullHistoryTokens, 0).totalUsd,
)
const sentCost = computed(
  () => estimateCost(props.profile, preview.value.sentHistoryTokens, 0).totalUsd,
)

const comparisonRows = computed(() => {
  const report = props.benchmark
  if (!report) return []
  return props.details.map(detail => ({
    detail,
    full: report.full.checks.find(check => check.detailId === detail.id),
    compressed: report.compressed.checks.find(check => check.detailId === detail.id),
  }))
})

const savedPercent = computed(() => Math.max(0, preview.value.savedRatio))

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
      <h2>Сжатие истории</h2>

      <div class="compress-card" :class="{ disabled: !props.compression.enabled }">
        <div class="compress-head">
          <strong>
            {{ props.compression.enabled ? 'сжатие включено' : 'сжатие выключено' }}
          </strong>
          <span class="muted">
            последние {{ props.compression.keepRecent }} · каждые
            {{ props.compression.summarizeEvery }}
          </span>
        </div>

        <div class="compress-grid">
          <div>
            <span class="muted">полная история</span>
            <strong>{{ formatTokens(preview.fullHistoryTokens) }}</strong>
          </div>
          <div>
            <span class="muted">в запросе</span>
            <strong>{{ formatTokens(preview.sentHistoryTokens) }}</strong>
          </div>
          <div>
            <span class="muted">сводка</span>
            <strong>{{ formatTokens(preview.summaryTokens) }}</strong>
          </div>
          <div>
            <span class="muted">экономия</span>
            <strong :class="preview.savedTokens >= 0 ? 'good' : 'bad'">
              {{ sign(preview.savedTokens) }}{{ formatTokens(Math.abs(preview.savedTokens)) }}
            </strong>
          </div>
        </div>

        <div class="gauge small">
          <span
            class="gauge-fill saved"
            :style="{ width: `${Math.min(100, savedPercent * 100)}%` }"
          />
        </div>

        <div class="context-meta">
          <span>экономия {{ formatPercent(savedPercent) }}</span>
          <span>
            сжато {{ preview.summarizedMessages }} · свежих {{ preview.recentMessages }}
          </span>
        </div>

        <p class="hint">
          Полная история: {{ formatTokens(preview.fullHistoryTokens) }} токенов
          ({{ formatUsd(fullCost) }}). В запросе:
          {{ formatTokens(preview.sentHistoryTokens) }} токенов
          ({{ formatUsd(sentCost) }}). Сумма считается по ценам профиля
          {{ props.profile.label }}.
        </p>
      </div>
    </section>

    <section class="field-group">
      <h2>Сводка · хранится отдельно</h2>

      <div v-if="props.summary" class="summary-card">
        <div class="summary-meta">
          <span>
            <strong>{{ props.summary.coveredMessages }}</strong> сообщений в сводке
          </span>
          <span>{{ props.summary.chunks }} обновл.</span>
          <span>{{ props.summary.mode === 'llm' ? 'LLM' : 'local' }}</span>
        </div>
        <div class="muted summary-time">
          обновлена {{ formatTime(props.summary.updatedAt) }}
        </div>
        <pre class="summary-text">{{ props.summary.text }}</pre>
      </div>
      <p v-else class="hint">
        Сводки пока нет. Она появится после того, как сверх последних
        {{ props.compression.keepRecent }} сообщений накопится
        {{ props.compression.summarizeEvery }} старых, или по кнопке «Сжать сейчас».
      </p>

      <div class="history-actions">
        <button
          type="button"
          class="btn ghost small"
          :disabled="props.busy || !props.messages.length"
          @click="emit('compress-now')"
        >
          {{ props.compressing ? 'Сжимаю…' : 'Сжать сейчас' }}
        </button>
        <button
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

      <p class="hint">
        Сводка лежит в <code>{{ SUMMARY_STORAGE_KEY }}</code> и не входит в массив
        <code>messages</code>. При сбросе или импорте истории она очищается.
      </p>
    </section>

    <section class="field-group">
      <h2>Детали для проверки</h2>
      <p class="hint">
        Список составлен заранее: 12 фактов из первых сообщений. Для каждого
        видно, есть ли он в полной истории и остался ли в том, что реально уходит
        в модель после сжатия. Это локальная проверка без API: она не заменяет
        ответы модели, но показывает, какие детали потерялись на уровне контекста.
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
      <h2>Сравнение памяти</h2>

      <p class="hint">
        Один и тот же набор вопросов (12 деталей) задаётся модели дважды — 24
        запроса: с полной историей и с summary + последними сообщениями. Ответ
        считается верным, если в нём есть ожидаемая деталь. Нужен API-ключ;
        запросы видны во вкладке «API».
      </p>

      <div class="history-actions">
        <button
          type="button"
          class="btn primary small"
          :disabled="props.busy"
          @click="emit('run-benchmark')"
        >
          {{ props.benchmarkRunning ? 'Проверяю…' : 'Проверить оба режима' }}
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
        <div class="benchmark-cards">
          <div class="benchmark-card">
            <span class="muted">без сжатия</span>
            <strong>{{ props.benchmark.full.remembered }} / {{ props.benchmark.full.total }}</strong>
            <span>{{ formatTokens(props.benchmark.full.promptTokens) }} prompt</span>
            <span>{{ formatUsd(props.benchmark.full.costUsd) }}</span>
          </div>
          <div class="benchmark-card">
            <span class="muted">со сжатием</span>
            <strong>
              {{ props.benchmark.compressed.remembered }} /
              {{ props.benchmark.compressed.total }}
            </strong>
            <span>{{ formatTokens(props.benchmark.compressed.promptTokens) }} prompt</span>
            <span>{{ formatUsd(props.benchmark.compressed.costUsd) }}</span>
          </div>
          <div class="benchmark-card">
            <span class="muted">экономия prompt</span>
            <strong>{{ formatPercent(Math.max(0, props.benchmark.savedRatio)) }}</strong>
            <span>{{ formatTokens(props.benchmark.savedPromptTokens) }} токенов</span>
            <span>{{ formatUsd(props.benchmark.savedCostUsd) }}</span>
          </div>
          <div class="benchmark-card">
            <span class="muted">с учётом сжатия в прогоне</span>
            <strong>
              {{ props.benchmark.netSavedPromptTokens >= 0 ? '−' : '+' }}{{
                formatTokens(Math.abs(props.benchmark.netSavedPromptTokens))
              }}
            </strong>
            <span>
              {{ formatTokens(props.benchmark.compressed.summaryPromptTokens) }} на summary
            </span>
            <span>{{ props.benchmark.compression.summaryRequests }} запрос(ов)</span>
          </div>
        </div>

        <p class="hint">
          «Экономия prompt» — разница только по вопросам. «С учётом сжатия» —
          минус токены, потраченные на запросы построения сводки. Сводка
          обновлена: {{ props.benchmark.compression.summaryUpdated ? 'да' : 'нет' }},
          сжато {{ props.benchmark.compression.summarizedMessages }} сообщений.
        </p>

        <div class="benchmark-table">
          <div class="benchmark-head">
            <span>деталь</span>
            <span>без сжатия</span>
            <span>со сжатием</span>
          </div>
          <div v-for="row in comparisonRows" :key="row.detail.id" class="benchmark-row">
            <div class="benchmark-fact">
              <strong>{{ row.detail.fact }}</strong>
              <details class="benchmark-answer">
                <summary>ответы модели</summary>
                <p><span class="muted">без сжатия:</span> {{ row.full?.answer || '—' }}</p>
                <p><span class="muted">со сжатием:</span> {{ row.compressed?.answer || '—' }}</p>
              </details>
            </div>
            <span class="check" :class="checkClass(row.full?.remembered)">
              {{ checkMark(row.full?.remembered) }}
            </span>
            <span class="check" :class="checkClass(row.compressed?.remembered)">
              {{ checkMark(row.compressed?.remembered) }}
            </span>
          </div>
        </div>

        <p class="hint">
          Отчётов в сессии: старт {{ formatTime(props.benchmark.startedAt) }} ·
          финиш {{ formatTime(props.benchmark.finishedAt) }}. Полная история в
          этом прогоне: {{ formatTokens(props.benchmark.full.historyTokens) }} токенов,
          сжатая — {{ formatTokens(props.benchmark.compressed.historyTokens) }}.
        </p>
      </template>
      <p v-else-if="!props.benchmarkRunning" class="hint">
        Отчёта пока нет. Загрузи контрольный диалог и запусти проверку — результат
        покажет, сколько деталей помнит агент в каждом режиме и сколько токенов
        экономит сжатие.
      </p>
    </section>

    <section class="field-group">
      <h2>Что именно теряется</h2>
      <p class="hint">
        Сжатие — это компромисс. Сводка сохраняет детали, которые в неё явно
        попали, но может потерять нюансы, шутки, промежуточные рассуждения и
        точные формулировки. Локальная выжимка дословно оставляет сообщения
        пользователя и сильнее сокращает ответы ассистента. LLM-сводка лучше
        переформулирует, но может пропустить число или имя — поэтому в промпте
        сжатия перечислены обязательные категории деталей.
      </p>
    </section>
  </div>
</template>
