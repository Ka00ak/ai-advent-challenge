<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { Agent } from './agent/Agent'
import ApiLogPanel from './components/ApiLogPanel.vue'
import HistoryPanel from './components/HistoryPanel.vue'
import MessageBubble from './components/MessageBubble.vue'
import SettingsPanel from './components/SettingsPanel.vue'
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
import { createDefaultToolRegistry } from './tools'
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
const apiLog = ref<LlmExchange[]>([])
const input = ref('')
const running = ref(false)
const statusText = ref('')
const composerNotice = ref('')
const tab = ref<'settings' | 'history' | 'log'>('settings')
const chatRef = ref<HTMLElement | null>(null)
const restoreNotice = ref(Boolean(loaded.conversation))
const saveState = ref<SaveState>(
  loaded.conversation
    ? { state: 'saved', savedAt: loaded.conversation.updatedAt, bytes: initialJson }
    : loaded.error
      ? { state: 'error', error: loaded.error }
      : { state: 'idle' },
)

let controller: AbortController | null = null
let runGeneration = 0

const apiCount = computed(() => apiLog.value.length)
const stats = computed(() => conversationStats(historyMessages.value))
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
  messages.value = []
  historyMessages.value = []
  apiLog.value = []
  conversation.value = null
  input.value = ''
  statusText.value = ''
  composerNotice.value = ''
  restoreNotice.value = false
  clearConversation()
  saveState.value = { state: 'idle' }
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
    const logIndex = apiLog.value.findIndex(item => item.id === exchange.id)
    if (logIndex >= 0) apiLog.value.splice(logIndex, 1, { ...exchange })
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
      systemPrompt: SYSTEM_PROMPT,
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
          apiLog.value.push(exchange)
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
      },
      controller.signal,
    )

    if (isCurrentRun()) {
      // Синхронизируем канонический массив с тем, что реально ушло в модель:
      // сюда попадут и synthetic tool-ответы для прерванных вызовов.
      historyMessages.value = result.messages.filter(message => message.role !== 'system')
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
  const payload = currentSnapshot()
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `lab07-conversation-${new Date().toISOString().replace(/[:.]/g, '-')}.json`
  link.click()
  URL.revokeObjectURL(url)
}

function importHistory(payload: { filename: string; text: string }): void {
  if (running.value) {
    composerNotice.value = 'Дождись окончания текущего ответа, затем загрузи историю.'
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
    conversation: conversation.value,
    exchanges: apiLog.value,
  }
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `lab07-api-log-${new Date().toISOString().replace(/[:.]/g, '-')}.json`
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
})
</script>

<template>
  <div class="app">
    <header class="topbar">
      <div class="brand">
        <span class="lab-badge">07</span>
        <div>
          <h1>Агент с сохранением контекста</h1>
          <p>История messages, tool_calls и результатов переживает перезапуск приложения</p>
        </div>
      </div>
      <div class="topbar-actions">
        <span class="pill">{{ settings.model || 'модель не выбрана' }}</span>
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
            <div class="empty-badge">tool-calling + persistence</div>
            <h2>Агент помнит диалог после перезапуска</h2>
            <p>
              Начни с вопроса, который требует инструмента, перезагрузи страницу (F5) —
              и продолжи с того же места. В историю попадают не только реплики, но и
              вызовы инструментов с их результатами.
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

            <p class="hint">
              Сохранение автоматическое: JSON-снимок диалога лежит в
              <code>localStorage</code> под ключом
              <code>{{ CONVERSATION_STORAGE_KEY }}</code>. Сырой массив
              <code>messages</code> можно посмотреть и скачать во вкладке «История»
              справа.
            </p>
            <p v-if="!storageAvailable" class="warn">
              localStorage недоступен — история не сохранится после перезапуска.
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
          @export="exportHistory"
          @import="importHistory"
          @clear="resetConversation"
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
