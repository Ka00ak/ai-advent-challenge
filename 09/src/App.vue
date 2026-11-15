<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { Agent } from './agent/Agent'
import {
  buildCompressedContext,
  compressHistory,
  previewCompression,
  type CompressionConfig,
  type ContextSummary,
} from './agent/compression'
import { createLlmSummaryGenerator } from './agent/summarizer'
import ApiLogPanel from './components/ApiLogPanel.vue'
import ContextPanel from './components/ContextPanel.vue'
import HistoryPanel from './components/HistoryPanel.vue'
import MessageBubble from './components/MessageBubble.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import TokenPanel from './components/TokenPanel.vue'
import {
  MEMORY_DETAILS,
  buildMemoryDialog,
} from './memory/dialog'
import {
  runMemoryBenchmark as runMemoryBenchmarkCheck,
  type MemoryBenchmarkReport,
} from './memory/benchmark'
import { SYSTEM_PROMPT } from './prompt'
import {
  clearStoredApiKey,
  loadSettings,
  saveSettings,
  saveStoredApiKey,
  type AppSettings,
} from './settings'
import {
  CONVERSATION_STORAGE_KEY,
  CONVERSATION_VERSION,
  clearConversation,
  cloneMessages,
  conversationStats,
  createEmptyConversation,
  formatBytes,
  getBrowserStorage,
  loadConversation,
  parseConversation,
  reconstructUiMessages,
  saveConversation,
  type StoredConversation,
} from './storage/conversation'
import {
  SUMMARY_STORAGE_KEY,
  clearSummary as clearStoredSummary,
  createStoredSummary,
  loadSummary,
  saveSummary,
  type StoredSummary,
} from './storage/summary'
import { createDefaultToolRegistry } from './tools'
import {
  analyzeContext,
  formatPercent,
  formatTokens,
  resolveModelProfile,
  summarizeExchanges,
} from './tokens'
import type { ChatMessage, LlmExchange } from './types'
import type { SaveState, UiMessage } from './ui-types'

const TEST_PROMPTS = [
  {
    tag: 'без инструментов',
    title: 'Общий вопрос',
    prompt: 'Объясни в двух предложениях, чем HTTP отличается от HTTPS.',
  },
  {
    tag: 'один инструмент',
    title: 'Вычисление',
    prompt: 'Сколько будет 1234 * 56 + 78?',
  },
  {
    tag: 'два инструмента подряд',
    title: 'Валюта → проценты',
    prompt:
      'Сначала переведи 250 USD в EUR инструментом convert_currency, затем увеличь результат на 15% инструментом calculate. Покажи оба результата.',
  },
]

const loaded = loadConversation()
const loadedSummary = loadSummary()
const initialMessages = loaded.conversation?.messages ?? []
const initialJson =
  loaded.conversation && typeof TextEncoder !== 'undefined'
    ? new TextEncoder().encode(JSON.stringify(loaded.conversation)).length
    : undefined

const settings = ref<AppSettings>(loadSettings())
const conversation = ref<StoredConversation | null>(loaded.conversation)
const historyMessages = ref<ChatMessage[]>(initialMessages)
const messages = ref<UiMessage[]>(
  initialMessages.length > 0 ? reconstructUiMessages(initialMessages) : [],
)
const summary = ref<StoredSummary | null>(
  loaded.conversation &&
    loadedSummary.summary &&
    (!loadedSummary.summary.conversationId ||
      loadedSummary.summary.conversationId === loaded.conversation.id)
    ? loadedSummary.summary
    : null,
)
const apiLog = ref<LlmExchange[]>([])
const input = ref('')
const running = ref(false)
const compressing = ref(false)
const statusText = ref('')
const composerNotice = ref('')
const tab = ref<'settings' | 'history' | 'tokens' | 'memory' | 'log'>('settings')
const chatRef = ref<HTMLElement | null>(null)
const restoreNotice = ref(Boolean(loaded.conversation))
const benchmark = ref<MemoryBenchmarkReport | null>(null)
const benchmarking = ref(false)
const benchmarkStatus = ref('')
const saveState = ref<SaveState>(
  loaded.conversation
    ? { state: 'saved', savedAt: loaded.conversation.updatedAt, bytes: initialJson }
    : loaded.error
      ? { state: 'error', error: loaded.error }
      : { state: 'idle' },
)

let controller: AbortController | null = null
let benchmarkController: AbortController | null = null
let runGeneration = 0

const toolSchemas = createDefaultToolRegistry().schemas()
const apiCount = computed(() => apiLog.value.length)
const stats = computed(() => conversationStats(historyMessages.value))
const modelProfile = computed(() =>
  resolveModelProfile(settings.value.model, settings.value.contextLimit),
)
const compressionConfig = computed<CompressionConfig>(() => ({
  enabled: settings.value.compressionEnabled,
  keepRecent: settings.value.keepRecentMessages,
  summarizeEvery: settings.value.summarizeEveryMessages,
  mode: settings.value.summaryMode,
}))
/** То, что реально уйдёт в модель: summary + последние N сообщений. */
const requestMessages = computed(() =>
  buildCompressedContext(
    historyMessages.value,
    compressionConfig.value,
    summary.value,
  ),
)
const contextEstimate = computed(() =>
  analyzeContext({
    systemPrompt: SYSTEM_PROMPT,
    messages: requestMessages.value,
    draftText: input.value,
    tools: toolSchemas,
    limit: modelProfile.value.contextLimit,
  }),
)
const compressionPreview = computed(() =>
  previewCompression(
    historyMessages.value,
    compressionConfig.value,
    summary.value,
  ),
)
const usageSummary = computed(() => summarizeExchanges(apiLog.value, modelProfile.value))
const storageAvailable = getBrowserStorage() !== null
const storageLabel = computed(() => {
  if (saveState.value.state === 'error') return 'история не сохранена'
  if (!historyMessages.value.length) return 'история пуста'
  return saveState.value.bytes ? `сохранено · ${formatBytes(saveState.value.bytes)}` : 'сохранено'
})

function makeId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `id-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

function toErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function isAbortError(error: unknown): boolean {
  return (
    (error instanceof DOMException && error.name === 'AbortError') ||
    (error instanceof Error && error.name === 'AbortError')
  )
}

function toStoredSummary(value: ContextSummary | null): StoredSummary | null {
  if (!value?.text.trim()) return null
  return createStoredSummary(value, conversation.value?.id ?? null)
}

function upsertExchange(exchange: LlmExchange): void {
  const index = apiLog.value.findIndex(item => item.id === exchange.id)
  if (index >= 0) apiLog.value.splice(index, 1, { ...exchange })
  else apiLog.value.push({ ...exchange })
}

function persistSummary(): void {
  if (!summary.value) {
    clearStoredSummary()
    return
  }

  const stored = createStoredSummary(summary.value, conversation.value?.id ?? null)
  summary.value = stored
  const result = saveSummary(stored)
  if (!result.ok) {
    composerNotice.value = result.error ?? 'Не удалось сохранить сводку.'
  }
}

function clearSummaryState(): void {
  benchmarkController?.abort()
  benchmarkController = null
  benchmarking.value = false
  summary.value = null
  clearStoredSummary()
  benchmark.value = null
  benchmarkStatus.value = ''
}

function currentSnapshot(): StoredConversation {
  const now = new Date().toISOString()
  const fallback = createEmptyConversation(new Date(now))
  return {
    version: CONVERSATION_VERSION,
    id: conversation.value?.id ?? fallback.id,
    createdAt: conversation.value?.createdAt ?? fallback.createdAt,
    updatedAt: now,
    messages: cloneMessages(historyMessages.value),
  }
}

function persistNow(): void {
  if (!historyMessages.value.length) {
    clearConversation()
    conversation.value = null
    saveState.value = { state: 'idle' }
    return
  }

  const snapshot = currentSnapshot()
  const result = saveConversation(snapshot)
  if (result.ok) {
    conversation.value = snapshot
    saveState.value = { state: 'saved', savedAt: result.savedAt, bytes: result.bytes }
  } else {
    saveState.value = { state: 'error', error: result.error }
  }
}

function resetConversation(): void {
  runGeneration += 1
  if (running.value) controller?.abort()
  controller = null
  running.value = false
  compressing.value = false
  messages.value = []
  historyMessages.value = []
  apiLog.value = []
  conversation.value = null
  input.value = ''
  statusText.value = ''
  composerNotice.value = ''
  restoreNotice.value = false
  clearSummaryState()
  clearConversation()
  saveState.value = { state: 'idle' }
}

function loadMemoryDialog(): void {
  if (running.value || compressing.value || benchmarking.value) {
    composerNotice.value = 'Дождись окончания текущей операции, затем загрузи диалог.'
    return
  }

  const dialog = buildMemoryDialog()
  historyMessages.value = cloneMessages(dialog)
  messages.value = reconstructUiMessages(dialog)
  const snapshot = createEmptyConversation()
  snapshot.messages = cloneMessages(dialog)
  conversation.value = snapshot
  apiLog.value = []
  clearSummaryState()
  persistNow()
  restoreNotice.value = false
  composerNotice.value = `Загружен контрольный диалог: ${dialog.length} сообщений и ${MEMORY_DETAILS.length} деталей.`
  tab.value = 'memory'
  scrollSoon()
}

async function compressNow(): Promise<void> {
  if (running.value || compressing.value || benchmarking.value) return

  const source = cloneMessages(historyMessages.value)
  if (!source.length) {
    composerNotice.value = 'История пуста: сначала загрузи контрольный диалог.'
    tab.value = 'memory'
    return
  }

  const generation = runGeneration
  compressing.value = true
  composerNotice.value = ''
  try {
    const generator =
      compressionConfig.value.mode === 'llm' && settings.value.apiKey.trim()
        ? createLlmSummaryGenerator({
            settings: settings.value,
            profile: modelProfile.value,
            onExchangeStart: upsertExchange,
            onExchangeEnd: upsertExchange,
          })
        : undefined

    const outcome = await compressHistory({
      messages: source,
      config: compressionConfig.value,
      summary: summary.value,
      generateSummary: generator,
    })

    // Пока шёл запрос, могли нажать «Новый диалог»: результат уже не нужен.
    if (generation !== runGeneration) return

    if (outcome.stats.summaryUpdated) {
      summary.value = toStoredSummary(outcome.summary)
      persistSummary()
      const modeLabel = outcome.summary?.mode === 'llm' ? 'LLM' : 'локально'
      composerNotice.value = `Сводка обновлена (${modeLabel}): сжато ${outcome.stats.summarizedMessages} сообщений, экономия ~${formatTokens(
        Math.max(0, outcome.stats.savedTokens),
      )} токенов.`
    } else {
      composerNotice.value = `Сжимать пока нечего: нужно минимум ${compressionConfig.value.summarizeEvery} старых сообщений сверх последних ${compressionConfig.value.keepRecent}.`
    }
    tab.value = 'memory'
  } catch (error) {
    if (generation !== runGeneration) return
    if (!isAbortError(error)) {
      composerNotice.value = `Не удалось обновить сводку: ${toErrorMessage(error)}`
    }
    tab.value = 'memory'
  } finally {
    if (generation === runGeneration) compressing.value = false
  }
}

async function runMemoryBenchmarkAction(): Promise<void> {
  if (benchmarking.value || running.value || compressing.value) return

  if (!settings.value.apiKey.trim()) {
    composerNotice.value = 'Для проверки памяти нужен API-ключ в настройках.'
    tab.value = 'memory'
    return
  }

  if (!historyMessages.value.length) loadMemoryDialog()
  const source = cloneMessages(historyMessages.value)
  if (!source.length) return

  benchmarkController = new AbortController()
  const generation = runGeneration
  benchmarking.value = true
  benchmark.value = null
  benchmarkStatus.value = 'Подготовка истории…'
  composerNotice.value = ''
  tab.value = 'memory'

  try {
    const generator =
      compressionConfig.value.mode === 'llm'
        ? createLlmSummaryGenerator({
            settings: settings.value,
            profile: modelProfile.value,
            signal: benchmarkController.signal,
            onExchangeStart: upsertExchange,
            onExchangeEnd: upsertExchange,
          })
        : undefined

    const report = await runMemoryBenchmarkCheck({
      messages: source,
      details: MEMORY_DETAILS,
      settings: settings.value,
      profile: modelProfile.value,
      compression: compressionConfig.value,
      summary: summary.value,
      generateSummary: generator,
      signal: benchmarkController.signal,
      onProgress: text => {
        benchmarkStatus.value = text
      },
      onExchangeStart: upsertExchange,
      onExchangeEnd: upsertExchange,
    })

    if (generation !== runGeneration) return

    benchmark.value = report
    if (report.summary) {
      summary.value = toStoredSummary(report.summary)
      persistSummary()
    }
    benchmarkStatus.value = 'Готово'
  } catch (error) {
    if (generation !== runGeneration) return
    benchmarkStatus.value = isAbortError(error)
      ? 'Остановлено пользователем'
      : `Ошибка: ${toErrorMessage(error)}`
  } finally {
    if (generation === runGeneration) {
      benchmarking.value = false
      benchmarkController = null
    }
  }
}

function stopBenchmark(): void {
  benchmarkController?.abort()
}

function clearSummaryAction(): void {
  if (running.value || compressing.value) return
  clearSummaryState()
  composerNotice.value =
    'Сводка очищена. Следующий запрос снова отправит полную историю, пока не накопится новая порция для сжатия.'
}

async function scrollToBottom(): Promise<void> {
  await nextTick()
  const element = chatRef.value
  if (element) element.scrollTop = element.scrollHeight
}

function scrollSoon(): void {
  void scrollToBottom()
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return
  event.preventDefault()
  void send()
}

async function send(preset?: string): Promise<void> {
  const text = (preset ?? input.value).trim()
  if (!text || running.value) return
  if (benchmarking.value || compressing.value) {
    composerNotice.value = 'Дождись окончания проверки памяти или сжатия.'
    return
  }

  if (!settings.value.apiKey.trim()) {
    composerNotice.value = 'Сначала укажи API-ключ в настройках справа.'
    tab.value = 'settings'
    return
  }
  if (!settings.value.baseUrl.trim() || !settings.value.model.trim()) {
    composerNotice.value = 'Заполни Base URL и модель в настройках.'
    tab.value = 'settings'
    return
  }

  // Снимок истории до новой реплики: агент добавит user-сообщение сам
  // и вернёт каждое новое сообщение через onMessageAppended.
  const history = cloneMessages(historyMessages.value)
  messages.value.push({ id: makeId(), role: 'user', content: text, pending: false, steps: [] })

  const assistant = reactive<UiMessage>({
    id: makeId(),
    role: 'assistant',
    content: '',
    pending: true,
    steps: [],
  })
  messages.value.push(assistant)

  input.value = ''
  composerNotice.value = ''
  running.value = true
  statusText.value = 'Подготовка запроса…'
  controller = new AbortController()
  const generation = ++runGeneration
  const isCurrentRun = (): boolean => generation === runGeneration
  scrollSoon()

  function updateExchange(exchange: LlmExchange): void {
    upsertExchange(exchange)
    const step = assistant.steps.find(item => item.exchange.id === exchange.id)
    if (step) step.exchange = { ...exchange }
  }

  const agent = new Agent(
    {
      baseUrl: settings.value.baseUrl,
      apiKey: settings.value.apiKey,
      model: settings.value.model,
      temperature: settings.value.temperature,
      maxTokens: settings.value.maxTokens,
      maxIterations: settings.value.maxIterations,
      contextLimit: settings.value.contextLimit,
      systemPrompt: SYSTEM_PROMPT,
      compression: compressionConfig.value,
      summary: summary.value,
    },
    createDefaultToolRegistry(),
  )

  try {
    const result = await agent.run(
      text,
      history,
      {
        onStatus: message => {
          if (!isCurrentRun()) return
          statusText.value = message
          scrollSoon()
        },
        onExchangeStart: exchange => {
          if (!isCurrentRun()) return
          upsertExchange(exchange)
          if (exchange.kind === 'summary') {
            scrollSoon()
            return
          }
          assistant.steps.push({
            iteration: assistant.steps.length + 1,
            exchange,
            toolResults: [],
          })
          scrollSoon()
        },
        onExchangeEnd: exchange => {
          if (!isCurrentRun()) return
          updateExchange(exchange)
          scrollSoon()
        },
        onToolStart: invocation => {
          if (!isCurrentRun()) return
          statusText.value = `Инструмент «${invocation.name}»: выполняется…`
          scrollSoon()
        },
        onToolEnd: invocation => {
          if (!isCurrentRun()) return
          const step = assistant.steps[assistant.steps.length - 1]
          if (step) step.toolResults.push({ ...invocation })
          statusText.value = `Инструмент «${invocation.name}»: ${
            invocation.ok ? 'готово' : 'ошибка'
          }`
          scrollSoon()
        },
        onFinalText: finalText => {
          if (!isCurrentRun()) return
          assistant.content = finalText
        },
        onMessageAppended: message => {
          if (!isCurrentRun()) return
          historyMessages.value.push(message)
          persistNow()
        },
        onSummaryUpdate: nextSummary => {
          if (!isCurrentRun()) return
          summary.value = toStoredSummary(nextSummary)
          persistSummary()
        },
      },
      controller.signal,
    )

    if (isCurrentRun()) {
      // Синхронизируем канонический массив с полной историей. Сжатая версия
      // живёт только в запросе; в localStorage остаются все сообщения.
      historyMessages.value = result.messages.filter(message => message.role !== 'system')
      summary.value = toStoredSummary(result.summary)
      persistSummary()
      persistNow()
    }

    assistant.content = result.finalText || assistant.content
    if (result.stoppedReason === 'cancelled') {
      assistant.stopped = 'Остановлено пользователем'
    } else if (!assistant.content.trim()) {
      assistant.content =
        result.stoppedReason === 'max-iterations'
          ? `Модель достигла лимита ${settings.value.maxIterations} итераций и не дала финальный ответ. Увеличь лимит или упрости запрос.`
          : 'Модель вернула пустой ответ.'
    }
  } catch (error) {
    if (!isCurrentRun()) return
    if (isAbortError(error)) {
      assistant.stopped = 'Остановлено пользователем'
    } else {
      assistant.error = toErrorMessage(error)
    }
  } finally {
    if (generation === runGeneration) {
      assistant.pending = false
      running.value = false
      controller = null
      statusText.value = ''
    }
    persistNow()
    scrollSoon()
  }
}

function stop(): void {
  controller?.abort()
}

function newDialog(): void {
  resetConversation()
}

function exportHistory(): void {
  if (!historyMessages.value.length) return
  const payload = {
    ...currentSnapshot(),
    summary: summary.value,
  }
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `lab09-conversation-${new Date().toISOString().replace(/[:.]/g, '-')}.json`
  link.click()
  URL.revokeObjectURL(url)
}

function importHistory(payload: { filename: string; text: string }): void {
  if (running.value || benchmarking.value || compressing.value) {
    composerNotice.value = 'Дождись окончания текущей операции, затем загрузи историю.'
    return
  }

  const result = parseConversation(payload.text)
  if (!result.conversation) {
    composerNotice.value = `Не удалось загрузить ${payload.filename}: ${result.error ?? 'неизвестная ошибка'}`
    tab.value = 'history'
    return
  }

  conversation.value = result.conversation
  historyMessages.value = result.conversation.messages
  messages.value = reconstructUiMessages(result.conversation.messages)
  apiLog.value = []
  clearSummaryState()
  restoreNotice.value = true
  composerNotice.value = `Загружена история из ${payload.filename}.`
  persistNow()
  tab.value = 'history'
  scrollSoon()
}

function clearLog(): void {
  apiLog.value = []
}

function exportLog(): void {
  const payload = {
    exportedAt: new Date().toISOString(),
    settings: { ...settings.value, apiKey: settings.value.apiKey ? '***' : '' },
    tokens: {
      profile: modelProfile.value,
      context: contextEstimate.value,
      summary: usageSummary.value,
    },
    compression: {
      config: compressionConfig.value,
      preview: compressionPreview.value,
      summary: summary.value,
      storageKey: SUMMARY_STORAGE_KEY,
    },
    conversation: conversation.value,
    exchanges: apiLog.value,
  }
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `lab09-api-log-${new Date().toISOString().replace(/[:.]/g, '-')}.json`
  link.click()
  URL.revokeObjectURL(url)
}

watch(
  settings,
  value => {
    saveSettings(value)
    if (value.rememberKey && value.apiKey.trim()) saveStoredApiKey(value.apiKey)
    else clearStoredApiKey()
  },
  { deep: true },
)

watch(input, () => {
  if (composerNotice.value) composerNotice.value = ''
})

onBeforeUnmount(() => {
  if (historyMessages.value.length) persistNow()
  if (summary.value) persistSummary()
})
</script>

<template>
  <div class="app">
    <header class="topbar">
      <div class="brand">
        <span class="lab-badge">09</span>
        <div>
          <h1>Агент со сжатием истории</h1>
          <p>Последние N сообщений как есть · сводка остального · экономия токенов</p>
        </div>
      </div>
      <div class="topbar-actions">
        <span class="pill">{{ settings.model || 'модель не выбрана' }}</span>
        <span
          class="pill token-pill"
          :class="contextEstimate.status"
          :title="`Оценка запроса с учётом сжатия: ${formatTokens(contextEstimate.totalTokens)} из ${formatTokens(contextEstimate.limit)} токенов. Полная история: ${formatTokens(compressionPreview.fullHistoryTokens)} токенов. Открыть вкладку «Токены»`"
          @click="tab = 'tokens'"
        >
          ~{{ formatTokens(contextEstimate.totalTokens) }} tok ·
          {{ formatPercent(contextEstimate.ratio) }}
        </span>
        <span
          v-if="summary"
          class="pill summary-pill"
          :title="`Сводка покрывает ${summary.coveredMessages} сообщений и хранится отдельно: ${SUMMARY_STORAGE_KEY}`"
          @click="tab = 'memory'"
        >
          память · {{ summary.coveredMessages }}
        </span>
        <span
          class="pill save-pill"
          :class="saveState.state"
          :title="`localStorage: ${CONVERSATION_STORAGE_KEY}`"
        >
          <span class="storage-dot" />
          {{ storageLabel }}
        </span>
        <span class="pill muted">{{ apiCount }} API</span>
        <button class="btn ghost small" @click="newDialog">Новый диалог</button>
      </div>
    </header>

    <main class="layout">
      <section class="chat-panel">
        <div ref="chatRef" class="chat">
          <div v-if="!messages.length" class="empty">
            <div class="empty-badge">summary + last N messages</div>
            <h2>Длинный диалог без переполнения контекста</h2>
            <p>
              Агент хранит последние {{ settings.keepRecentMessages }} сообщений как
              есть, а более старую часть заменяет сводкой. Полная история остаётся
              в localStorage — в модель уходит только summary и свежий хвост.
              Загрузи контрольный диалог и проверь, какие детали пережили сжатие.
            </p>

            <div class="presets">
              <button
                v-for="preset in TEST_PROMPTS"
                :key="preset.tag"
                type="button"
                class="preset"
                :disabled="running"
                @click="send(preset.prompt)"
              >
                <span class="preset-tag">{{ preset.tag }}</span>
                <strong>{{ preset.title }}</strong>
                <span class="preset-prompt">{{ preset.prompt }}</span>
              </button>
            </div>

            <div class="memory-dialog-card">
              <div>
                <strong>Контрольный диалог для проверки памяти</strong>
                <p class="muted">
                  24 сообщения · 12 конкретных деталей (имена, сроки, бюджеты,
                  ограничения, решения). Детали спрятаны в начале, последние
                  8 сообщений — без фактов.
                </p>
              </div>
              <button
                type="button"
                class="btn primary"
                :disabled="running"
                @click="loadMemoryDialog"
              >
                Загрузить 24 сообщения
              </button>
            </div>

            <p class="hint">
              Сохранение автоматическое: полная история лежит в
              <code>localStorage</code> под ключом
              <code>{{ CONVERSATION_STORAGE_KEY }}</code>, сводка — отдельно под
              <code>{{ SUMMARY_STORAGE_KEY }}</code>. Оба JSON можно посмотреть
              во вкладках «История» и «Память».
            </p>
            <p v-if="!storageAvailable" class="warn">
              localStorage недоступен — история и сводка не сохранятся после перезапуска.
            </p>
          </div>

          <div v-if="restoreNotice && messages.length" class="restore-banner">
            <div>
              <strong>История восстановлена из JSON</strong>
              <p class="muted">
                {{ stats.total }} messages · {{ stats.user }} user ·
                {{ stats.assistant }} assistant · {{ stats.tool }} tool · из них
                {{ stats.toolCalls }} tool_calls и {{ stats.toolResults }} результатов
                инструментов
              </p>
            </div>
            <button type="button" class="icon-btn" @click="restoreNotice = false">
              Скрыть
            </button>
          </div>

          <MessageBubble
            v-for="message in messages"
            :key="message.id"
            :message="message"
            :status="statusText"
          />
        </div>

        <footer class="composer">
          <div class="composer-box">
            <textarea
              v-model="input"
              rows="2"
              :disabled="running"
              placeholder="Продолжи диалог: «А если умножить прошлый результат на 2?»…"
              @keydown="onKeydown"
            />
            <div class="composer-actions">
              <span class="composer-hint" :class="{ warn: composerNotice }">
                <template v-if="composerNotice">{{ composerNotice }}</template>
                <template v-else-if="running">{{ statusText || 'Работаю…' }}</template>
                <template v-else-if="!settings.apiKey.trim()">
                  Ключ не задан — открой «Настройки»
                </template>
                <template v-else>Enter — отправить, Shift+Enter — перенос строки</template>
              </span>
              <button v-if="running" type="button" class="btn danger" @click="stop">
                Остановить
              </button>
              <button
                v-else
                type="button"
                class="btn primary"
                :disabled="!input.trim()"
                @click="send()"
              >
                Отправить
              </button>
            </div>
          </div>
        </footer>
      </section>

      <aside class="sidebar">
        <div class="tabs">
          <button
            type="button"
            :class="{ active: tab === 'settings' }"
            @click="tab = 'settings'"
          >
            Настройки
          </button>
          <button
            type="button"
            :class="{ active: tab === 'history' }"
            @click="tab = 'history'"
          >
            История
            <span v-if="stats.total" class="count">{{ stats.total }}</span>
          </button>
          <button
            type="button"
            :class="{ active: tab === 'tokens' }"
            @click="tab = 'tokens'"
          >
            Токены
            <span v-if="apiCount" class="count">{{ apiCount }}</span>
          </button>
          <button
            type="button"
            :class="{ active: tab === 'memory' }"
            @click="tab = 'memory'"
          >
            Память
            <span v-if="summary" class="count">{{ summary.coveredMessages }}</span>
          </button>
          <button type="button" :class="{ active: tab === 'log' }" @click="tab = 'log'">
            API
            <span v-if="apiCount" class="count">{{ apiCount }}</span>
          </button>
        </div>

        <SettingsPanel v-if="tab === 'settings'" v-model="settings" :disabled="running" />
        <HistoryPanel
          v-else-if="tab === 'history'"
          :conversation="conversation"
          :messages="historyMessages"
          :save-state="saveState"
          :summary="summary"
          @export="exportHistory"
          @import="importHistory"
          @clear="resetConversation"
        />
        <TokenPanel
          v-else-if="tab === 'tokens'"
          :messages="historyMessages"
          :exchanges="apiLog"
          :context="contextEstimate"
          :profile="modelProfile"
          :compression="compressionConfig"
          :summary="summary"
        />
        <ContextPanel
          v-else-if="tab === 'memory'"
          :messages="historyMessages"
          :summary="summary"
          :compression="compressionConfig"
          :profile="modelProfile"
          :benchmark="benchmark"
          :benchmark-running="benchmarking"
          :benchmark-status="benchmarkStatus"
          :compressing="compressing"
          :busy="running || compressing || benchmarking"
          :details="MEMORY_DETAILS"
          @seed-dialog="loadMemoryDialog"
          @compress-now="compressNow"
          @clear-summary="clearSummaryAction"
          @run-benchmark="runMemoryBenchmarkAction"
          @cancel-benchmark="stopBenchmark"
        />
        <ApiLogPanel
          v-else
          :exchanges="apiLog"
          @clear="clearLog"
          @export="exportLog"
        />
      </aside>
    </main>
  </div>
</template>
