<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from 'vue'
import { Agent } from './agent/Agent'
import ApiLogPanel from './components/ApiLogPanel.vue'
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
import { createDefaultToolRegistry } from './tools'
import type { ChatMessage, LlmExchange } from './types'
import type { UiMessage } from './ui-types'

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

const settings = ref<AppSettings>(loadSettings())
const messages = ref<UiMessage[]>([])
const apiLog = ref<LlmExchange[]>([])
const input = ref('')
const running = ref(false)
const statusText = ref('')
const composerNotice = ref('')
const tab = ref<'settings' | 'log'>('settings')
const chatRef = ref<HTMLElement | null>(null)

let controller: AbortController | null = null

const apiCount = computed(() => apiLog.value.length)

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

function toHistory(items: UiMessage[]): ChatMessage[] {
  return items
    .filter(item => !item.pending && !item.error && item.content.trim())
    .map(item => ({ role: item.role, content: item.content }))
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

  const history = toHistory(messages.value)
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
          statusText.value = message
          scrollSoon()
        },
        onExchangeStart: exchange => {
          apiLog.value.push(exchange)
          assistant.steps.push({
            iteration: assistant.steps.length + 1,
            exchange,
            toolResults: [],
          })
          scrollSoon()
        },
        onExchangeEnd: exchange => {
          updateExchange(exchange)
          scrollSoon()
        },
        onToolStart: invocation => {
          statusText.value = `Инструмент «${invocation.name}»: выполняется…`
          scrollSoon()
        },
        onToolEnd: invocation => {
          const step = assistant.steps[assistant.steps.length - 1]
          if (step) step.toolResults.push({ ...invocation })
          statusText.value = `Инструмент «${invocation.name}»: ${
            invocation.ok ? 'готово' : 'ошибка'
          }`
          scrollSoon()
        },
        onFinalText: finalText => {
          assistant.content = finalText
        },
      },
      controller.signal,
    )

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
    if (isAbortError(error)) {
      assistant.stopped = 'Остановлено пользователем'
    } else {
      assistant.error = toErrorMessage(error)
    }
  } finally {
    assistant.pending = false
    running.value = false
    controller = null
    statusText.value = ''
    scrollSoon()
  }
}

function stop(): void {
  controller?.abort()
}

function newDialog(): void {
  if (running.value) controller?.abort()
  messages.value = []
  apiLog.value = []
  input.value = ''
  statusText.value = ''
  composerNotice.value = ''
}

function clearLog(): void {
  apiLog.value = []
}

function exportLog(): void {
  const payload = {
    exportedAt: new Date().toISOString(),
    settings: { ...settings.value, apiKey: settings.value.apiKey ? '***' : '' },
    exchanges: apiLog.value,
  }
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = `lab06-api-log-${new Date().toISOString().replace(/[:.]/g, '-')}.json`
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
</script>

<template>
  <div class="app">
    <header class="topbar">
      <div class="brand">
        <span class="lab-badge">06</span>
        <div>
          <h1>Агент с инструментами</h1>
          <p>Модель сама выбирает, когда вызвать время, калькулятор или конвертер валют</p>
        </div>
      </div>
      <div class="topbar-actions">
        <span class="pill">{{ settings.model || 'модель не выбрана' }}</span>
        <span class="pill muted">{{ apiCount }} API</span>
        <button class="btn ghost small" @click="newDialog">Новый диалог</button>
      </div>
    </header>

    <main class="layout">
      <section class="chat-panel">
        <div ref="chatRef" class="chat">
          <div v-if="!messages.length" class="empty">
            <div class="empty-badge">tool-calling</div>
            <h2>Агент сам решает, какой инструмент вызвать</h2>
            <p>
              Проверь три сценария из задания: вопрос без инструментов, запрос с одним
              инструментом и цепочку из двух вызовов подряд.
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
              Все реальные запросы к API можно открыть во вкладке «API» справа: там видны
              messages, схемы инструментов, ответы модели и tool_calls.
            </p>
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
              placeholder="Например: переведи 250 USD в EUR и добавь 15%…"
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
          <button type="button" :class="{ active: tab === 'log' }" @click="tab = 'log'">
            API
            <span v-if="apiCount" class="count">{{ apiCount }}</span>
          </button>
        </div>

        <SettingsPanel v-if="tab === 'settings'" v-model="settings" :disabled="running" />
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
