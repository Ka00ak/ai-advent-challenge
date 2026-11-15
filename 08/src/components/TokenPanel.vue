<script setup lang="ts">
import { computed } from 'vue'
import {
  buildTokenScenarios,
  estimateMessagesTokens,
  formatPercent,
  formatTokens,
  formatUsd,
  summarizeExchanges,
  type ContextBreakdown,
  type ModelProfile,
} from '../tokens'
import type { ChatMessage, LlmExchange } from '../types'

const props = defineProps<{
  messages: ChatMessage[]
  exchanges: LlmExchange[]
  context: ContextBreakdown
  profile: ModelProfile
}>()

const summary = computed(() => summarizeExchanges(props.exchanges, props.profile))
const historyTokens = computed(() => estimateMessagesTokens(props.messages))
const lastExchange = computed(() => props.exchanges.at(-1))
const scenarios = computed(() => buildTokenScenarios(props.profile))

const current = computed(() => {
  const exchange = lastExchange.value
  if (!exchange) {
    return {
      promptTokens: props.context.totalTokens,
      completionTokens: 0,
      totalTokens: props.context.totalTokens,
      estimatedPromptTokens: props.context.totalTokens,
      costUsd: 0,
      reported: false,
      hasRequest: false,
    }
  }

  const promptTokens = exchange.usage?.prompt_tokens ?? exchange.estimate?.totalTokens ?? 0
  const completionTokens = exchange.usage?.completion_tokens ?? 0
  return {
    promptTokens,
    completionTokens,
    totalTokens: exchange.usage?.total_tokens ?? promptTokens + completionTokens,
    estimatedPromptTokens: exchange.estimate?.totalTokens ?? promptTokens,
    costUsd: exchange.costUsd ?? 0,
    reported: typeof exchange.usage?.prompt_tokens === 'number',
    hasRequest: true,
  }
})

const requestRows = computed(() =>
  props.exchanges.map((exchange, index) => {
    const promptTokens = exchange.usage?.prompt_tokens ?? exchange.estimate?.totalTokens ?? 0
    const completionTokens = exchange.usage?.completion_tokens ?? 0
    return {
      id: exchange.id,
      index: index + 1,
      promptTokens,
      completionTokens,
      totalTokens: exchange.usage?.total_tokens ?? promptTokens + completionTokens,
      costUsd: exchange.costUsd ?? 0,
      reported: typeof exchange.usage?.prompt_tokens === 'number',
      estimate: exchange.estimate,
    }
  }),
)

const maxRowTokens = computed(() =>
  Math.max(1, ...requestRows.value.map(row => row.totalTokens)),
)

const growth = computed(() => {
  const rows = requestRows.value.filter(row => row.promptTokens > 0)
  if (rows.length < 2) return null
  const first = rows[0]!
  const last = rows[rows.length - 1]!
  return {
    from: first.promptTokens,
    to: last.promptTokens,
    delta: last.promptTokens - first.promptTokens,
    factor: first.promptTokens > 0 ? last.promptTokens / first.promptTokens : 1,
  }
})

const gaugeWidth = computed(() => `${Math.min(100, props.context.ratio * 100)}%`)

const statusLabel = computed(() => {
  if (props.context.status === 'overflow') return 'переполнено'
  if (props.context.status === 'warning') return 'близко к лимиту'
  return 'влезает'
})

const statusIcon = computed(() => {
  if (props.context.status === 'overflow') return '⛔'
  if (props.context.status === 'warning') return '⚠️'
  return '✓'
})

const scenarioStatus = computed(() => ({
  short: 'ok' as const,
  long: 'warning' as const,
  overflow: 'overflow' as const,
}))

function scenarioLabel(kind: 'short' | 'long' | 'overflow'): string {
  if (kind === 'short') return 'проходит'
  if (kind === 'long') return 'дорого, но проходит'
  return 'переполнено, но отправляется'
}
</script>

<template>
  <div class="tokens">
    <section class="field-group">
      <h2>Контекстное окно</h2>

      <div class="context-card" :class="props.context.status">
        <div class="context-head">
          <span class="context-status">{{ statusIcon }} {{ statusLabel }}</span>
          <span class="muted">
            {{ formatTokens(props.context.totalTokens) }} /
            {{ formatTokens(props.context.limit) }} токенов
          </span>
        </div>

        <div class="gauge" :class="props.context.status">
          <span class="gauge-fill" :style="{ width: gaugeWidth }" />
        </div>

        <div class="context-meta">
          <span>Занято {{ formatPercent(props.context.ratio) }}</span>
          <span>Свободно ~{{ formatTokens(props.context.remaining) }}</span>
        </div>

        <div class="context-breakdown">
          <div>
            <span class="muted">system</span>
            <strong>{{ formatTokens(props.context.systemTokens) }}</strong>
          </div>
          <div>
            <span class="muted">история</span>
            <strong>{{ formatTokens(props.context.historyTokens) }}</strong>
          </div>
          <div>
            <span class="muted">новое</span>
            <strong>{{ formatTokens(props.context.currentTokens) }}</strong>
          </div>
          <div>
            <span class="muted">инструменты</span>
            <strong>{{ formatTokens(props.context.toolsTokens) }}</strong>
          </div>
        </div>

        <p class="hint">
          Это оценка по эвристике (кириллица / латиница / JSON) плюс схемы
          инструментов. Точные значения приходят от провайдера в
          <code>usage</code> после ответа. Лимит:
          <strong>{{ props.profile.label }}</strong> ·
          {{ formatTokens(props.profile.contextLimit) }} токенов.
        </p>
      </div>
    </section>

    <section class="field-group">
      <h2>Текущий запрос и ответ</h2>

      <div class="token-cards">
        <div class="token-card">
          <span class="token-label">prompt</span>
          <strong>{{ formatTokens(current.promptTokens) }}</strong>
          <span class="token-note">
            {{ current.reported ? 'usage от API' : 'оценка' }}
          </span>
        </div>
        <div class="token-card">
          <span class="token-label">ответ</span>
          <strong>{{ formatTokens(current.completionTokens) }}</strong>
          <span class="token-note">completion</span>
        </div>
        <div class="token-card">
          <span class="token-label">всего</span>
          <strong>{{ formatTokens(current.totalTokens) }}</strong>
          <span class="token-note">запрос + ответ</span>
        </div>
        <div class="token-card">
          <span class="token-label">стоимость</span>
          <strong>{{ formatUsd(current.costUsd) }}</strong>
          <span class="token-note">оценка</span>
        </div>
      </div>

      <p v-if="current.hasRequest && !current.reported" class="hint">
        Провайдер не вернул <code>usage</code> — показана локальная оценка.
      </p>
      <p v-else-if="!current.hasRequest" class="hint">
        Запросов ещё не было. В prompt входит system-промпт, вся история, черновик
        в поле ввода и схемы инструментов.
      </p>
    </section>

    <section class="field-group">
      <h2>Вся история и стоимость сессии</h2>

      <div class="history-stats">
        <div class="stat">
          <span class="stat-value">{{ formatTokens(historyTokens) }}</span>
          <span class="stat-label">history</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ summary.requests }}</span>
          <span class="stat-label">requests</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ formatTokens(summary.promptTokens) }}</span>
          <span class="stat-label">prompt sum</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ formatTokens(summary.completionTokens) }}</span>
          <span class="stat-label">completion</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ formatUsd(summary.costUsd) }}</span>
          <span class="stat-label">cost</span>
        </div>
      </div>

      <p class="hint">
        «history» — оценка текущего массива <code>messages</code> без system и
        схем инструментов. «prompt sum» — сумма токенов всех запросов сессии:
        каждый шаг tool-loop отправляет историю заново, поэтому сумма растёт
        быстрее, чем сама история. Цены профиля:
        {{ formatUsd(props.profile.inputUsdPerMillion) }}/1M вход ·
        {{ formatUsd(props.profile.outputUsdPerMillion) }}/1M выход.
      </p>

      <div v-if="requestRows.length" class="growth">
        <div class="growth-head">
          <strong>Рост промпта по запросам</strong>
          <span v-if="growth" class="muted">
            {{ formatTokens(growth.from) }} → {{ formatTokens(growth.to) }}
            ({{ growth.delta >= 0 ? '+' : '' }}{{ formatTokens(growth.delta) }},
            ×{{ growth.factor.toFixed(1) }})
          </span>
        </div>

        <div v-for="row in requestRows" :key="row.id" class="growth-row">
          <span class="growth-index">#{{ row.index }}</span>
          <div class="growth-track">
            <span
              class="growth-bar"
              :style="{ width: `${Math.max(2, (row.totalTokens / maxRowTokens) * 100)}%` }"
            />
          </div>
          <span class="growth-value">
            {{ formatTokens(row.promptTokens) }}↑ {{ formatTokens(row.completionTokens) }}↓
          </span>
          <span class="growth-cost">{{ formatUsd(row.costUsd) }}</span>
        </div>
      </div>
      <p v-else class="hint">
        После первого запроса здесь появится таблица: сколько токенов ушло на
        каждый шаг и как менялась стоимость.
      </p>
    </section>

    <section class="field-group">
      <h2>Сравнение сценариев</h2>

      <p class="hint">
        Сценарии считаются локально, без обращения к API. Один и тот же лимит,
        разная длина истории.
      </p>

      <div class="scenario-list">
        <div
          v-for="scenario in scenarios"
          :key="scenario.kind"
          class="scenario"
          :class="scenarioStatus[scenario.kind]"
        >
          <div class="scenario-head">
            <strong>{{ scenario.title }}</strong>
            <span class="badge" :class="scenario.kind === 'overflow' ? 'bad' : 'ok'">
              {{ scenarioLabel(scenario.kind) }}
            </span>
          </div>
          <div class="scenario-meta">
            <span>{{ scenario.messageCount }} сообщений</span>
            <span>~{{ formatTokens(scenario.promptTokens) }} токенов</span>
            <span>{{ formatPercent(scenario.ratio) }} лимита</span>
            <span>{{ formatUsd(scenario.costUsd) }}</span>
          </div>
          <div class="gauge small" :class="scenario.status">
            <span
              class="gauge-fill"
              :style="{ width: `${Math.min(100, scenario.ratio * 100)}%` }"
            />
          </div>
          <p class="scenario-description">{{ scenario.description }}</p>
          <p class="scenario-expected">{{ scenario.expected }}</p>
        </div>
      </div>

      <div class="overflow-note">
        <strong>Что ломается при переполнении</strong>
        <p>
          Chat Completions API принимает один промпт целиком: system + вся история +
          tools. Если сумма больше контекстного окна, агент всё равно отправляет
          запрос, чтобы показать реальный ответ провайдера: он либо ответит, либо
          вернёт HTTP 400 <code>context_length_exceeded</code> — тогда ответа не
          будет, токены на отклонённый запрос обычно не списываются.
        </p>
        <p>
          Лечение: начать новый диалог, удалить старые реплики, сократить
          tool-результаты или выбрать модель с большим окном. Увеличение
          <code>max_tokens</code> не помогает: это лимит ответа, а не входа.
        </p>
      </div>
    </section>
  </div>
</template>
