import type { AgentStep, ChatMessage, ToolCall, ToolInvocation } from '../types'
import type { UiMessage } from '../ui-types'

/** Версия формата: при изменении структуры старую историю можно мигрировать. */
export const CONVERSATION_VERSION = 1 as const

/** Ключ, под которым JSON-снимок диалога лежит в localStorage. */
export const CONVERSATION_STORAGE_KEY = 'lab07.conversation.v1'

/**
 * Снимок диалога. `messages` — каноническая история для LLM:
 * пользовательские и ассистентские сообщения, tool_calls и tool-результаты.
 */
export interface StoredConversation {
  version: typeof CONVERSATION_VERSION
  id: string
  createdAt: string
  updatedAt: string
  messages: ChatMessage[]
}

export interface ConversationStats {
  total: number
  user: number
  assistant: number
  tool: number
  toolCalls: number
  toolResults: number
}

export interface ConversationLoadResult {
  conversation: StoredConversation | null
  error?: string
}

export interface ConversationSaveResult {
  ok: boolean
  savedAt?: string
  bytes?: number
  error?: string
}

export interface KeyValueStorage {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return null
  return value as Record<string, unknown>
}

function normalizeContent(value: unknown): string | null {
  if (typeof value === 'string') return value
  if (value === null || value === undefined) return null
  if (Array.isArray(value)) {
    return value
      .map(part => {
        if (typeof part === 'string') return part
        const record = asRecord(part)
        return typeof record?.text === 'string' ? record.text : ''
      })
      .join('')
  }
  return null
}

function normalizeToolCall(value: unknown, index: number): ToolCall | null {
  const record = asRecord(value)
  const fn = asRecord(record?.function)
  const name = typeof fn?.name === 'string' ? fn.name.trim() : ''
  if (!name) return null

  let args = '{}'
  const rawArgs = fn?.arguments
  if (typeof rawArgs === 'string') {
    args = rawArgs
  } else if (rawArgs !== undefined) {
    try {
      args = JSON.stringify(rawArgs)
    } catch {
      args = '{}'
    }
  }

  const id =
    typeof record?.id === 'string' && record.id.trim()
      ? record.id
      : `restored_call_${index}_${name}`

  return { id, type: 'function', function: { name, arguments: args } }
}

function normalizeMessage(value: unknown): ChatMessage | null {
  const record = asRecord(value)
  if (!record) return null

  const content = normalizeContent(record.content)

  if (record.role === 'user') {
    return { role: 'user', content: content ?? '' }
  }

  if (record.role === 'assistant') {
    const rawCalls = Array.isArray(record.tool_calls) ? record.tool_calls : []
    const toolCalls = rawCalls
      .map((call, index) => normalizeToolCall(call, index))
      .filter((call): call is ToolCall => call !== null)

    const message: ChatMessage = { role: 'assistant', content }
    if (toolCalls.length > 0) message.tool_calls = toolCalls
    return message
  }

  if (record.role === 'tool') {
    const toolCallId =
      typeof record.tool_call_id === 'string' ? record.tool_call_id.trim() : ''
    if (!toolCallId) return null

    const message: ChatMessage = {
      role: 'tool',
      content: content ?? '',
      tool_call_id: toolCallId,
    }
    if (typeof record.name === 'string' && record.name.trim()) message.name = record.name
    return message
  }

  // system не храним: системный промпт всегда подставляется заново при запуске.
  return null
}

function makeId(prefix: string): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `${prefix}-${crypto.randomUUID()}`
  }
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

export function createEmptyConversation(now = new Date()): StoredConversation {
  const timestamp = now.toISOString()
  return {
    version: CONVERSATION_VERSION,
    id: makeId('conversation'),
    createdAt: timestamp,
    updatedAt: timestamp,
    messages: [],
  }
}

export function cloneMessages(messages: ChatMessage[]): ChatMessage[] {
  // JSON-клонирование заодно снимает Vue-прокси с реактивных объектов.
  return JSON.parse(JSON.stringify(messages)) as ChatMessage[]
}

/** Разбирает и валидирует JSON-снимок из localStorage или импортированного файла. */
export function parseConversation(raw: string, now = new Date()): ConversationLoadResult {
  if (!raw.trim()) return { conversation: null, error: 'История пуста' }

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (error) {
    return {
      conversation: null,
      error: `История повреждена: ${error instanceof Error ? error.message : String(error)}`,
    }
  }

  const record = asRecord(parsed)
  if (!record) return { conversation: null, error: 'История должна быть JSON-объектом' }

  if (record.version !== CONVERSATION_VERSION) {
    return {
      conversation: null,
      error: `Неподдерживаемая версия истории: ${String(record.version ?? '—')}`,
    }
  }

  if (!Array.isArray(record.messages)) {
    return { conversation: null, error: 'В истории нет массива messages' }
  }

  const messages = record.messages
    .map(normalizeMessage)
    .filter((message): message is ChatMessage => message !== null)

  if (messages.length === 0) {
    return { conversation: null, error: 'В истории нет ни одного корректного сообщения' }
  }

  const timestamp = now.toISOString()
  return {
    conversation: {
      version: CONVERSATION_VERSION,
      id: typeof record.id === 'string' && record.id ? record.id : makeId('conversation'),
      createdAt: typeof record.createdAt === 'string' ? record.createdAt : timestamp,
      updatedAt: typeof record.updatedAt === 'string' ? record.updatedAt : timestamp,
      messages,
    },
  }
}

export function getBrowserStorage(): KeyValueStorage | null {
  try {
    if (typeof localStorage === 'undefined') return null
    return localStorage
  } catch {
    return null
  }
}

export function loadConversation(storage = getBrowserStorage()): ConversationLoadResult {
  if (!storage) return { conversation: null, error: 'localStorage недоступен в этом окружении' }

  let raw: string | null
  try {
    raw = storage.getItem(CONVERSATION_STORAGE_KEY)
  } catch (error) {
    return {
      conversation: null,
      error: `Не удалось прочитать историю: ${
        error instanceof Error ? error.message : String(error)
      }`,
    }
  }

  if (!raw) return { conversation: null }
  return parseConversation(raw)
}

export function saveConversation(
  conversation: StoredConversation,
  storage = getBrowserStorage(),
): ConversationSaveResult {
  if (!storage) return { ok: false, error: 'localStorage недоступен в этом окружении' }

  try {
    const serialized = JSON.stringify(conversation)
    storage.setItem(CONVERSATION_STORAGE_KEY, serialized)
    const bytes =
      typeof TextEncoder !== 'undefined'
        ? new TextEncoder().encode(serialized).length
        : serialized.length
    return { ok: true, savedAt: conversation.updatedAt, bytes }
  } catch (error) {
    return {
      ok: false,
      error: `Не удалось сохранить историю: ${
        error instanceof Error ? error.message : String(error)
      }`,
    }
  }
}

export function clearConversation(storage = getBrowserStorage()): void {
  if (!storage) return
  try {
    storage.removeItem(CONVERSATION_STORAGE_KEY)
  } catch {
    // Приватный режим или запрет на запись — очищать нечего.
  }
}

export function conversationStats(messages: ChatMessage[]): ConversationStats {
  const stats: ConversationStats = {
    total: messages.length,
    user: 0,
    assistant: 0,
    tool: 0,
    toolCalls: 0,
    toolResults: 0,
  }

  for (const message of messages) {
    if (message.role === 'user') stats.user += 1
    if (message.role === 'assistant') {
      stats.assistant += 1
      stats.toolCalls += message.tool_calls?.length ?? 0
    }
    if (message.role === 'tool') {
      stats.tool += 1
      stats.toolResults += 1
    }
  }

  return stats
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} Б`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} КБ`
  return `${(bytes / (1024 * 1024)).toFixed(2)} МБ`
}

function parseToolPayload(content: string | null): {
  ok: boolean
  result?: unknown
  error?: string
} {
  if (!content) return { ok: false, error: 'Пустой результат инструмента' }

  let parsed: unknown
  try {
    parsed = JSON.parse(content)
  } catch {
    return { ok: true, result: content }
  }

  const record = asRecord(parsed)
  if (!record) return { ok: true, result: parsed }

  if (record.ok === true) return { ok: true, result: record.result }
  if (record.ok === false) {
    return {
      ok: false,
      error: typeof record.error === 'string' ? record.error : 'Ошибка инструмента',
    }
  }
  return { ok: true, result: parsed }
}

/**
 * Восстанавливает UI-ленту из канонических messages.
 * Это позволяет после перезапуска показать не только текст, но и trace:
 * какой инструмент вызван, с какими аргументами и что он вернул.
 */
export function reconstructUiMessages(messages: ChatMessage[]): UiMessage[] {
  const result: UiMessage[] = []
  let current: UiMessage | null = null
  const stepsByCallId = new Map<string, AgentStep>()

  messages.forEach((message, index) => {
    if (message.role === 'user') {
      result.push({
        id: `restored-user-${index}`,
        role: 'user',
        content: message.content ?? '',
        pending: false,
        steps: [],
      })
      current = null
      return
    }

    if (message.role === 'assistant') {
      if (!current) {
        current = {
          id: `restored-assistant-${index}`,
          role: 'assistant',
          content: '',
          pending: false,
          steps: [],
        }
        result.push(current)
      }

      if (message.tool_calls?.length) {
        const step: AgentStep = {
          iteration: current.steps.length + 1,
          restored: true,
          exchange: {
            id: `restored-exchange-${index}`,
            url: '',
            startedAt: '',
            durationMs: 0,
            request: { model: '(restored)', messages: [] },
            message,
          },
          toolResults: [],
        }
        current.steps.push(step)
        for (const call of message.tool_calls) stepsByCallId.set(call.id, step)
      } else {
        const text = (message.content ?? '').trim()
        if (text) current.content = current.content ? `${current.content}\n\n${text}` : text
      }
      return
    }

    // tool-сообщение: привязываем результат к шагу с соответствующим tool_call_id.
    const step = message.tool_call_id ? stepsByCallId.get(message.tool_call_id) : undefined
    if (!step) return

    const call = step.exchange.message?.tool_calls?.find(
      candidate => candidate.id === message.tool_call_id,
    )
    const payload = parseToolPayload(message.content)
    const invocation: ToolInvocation = {
      id: message.tool_call_id ?? `restored-tool-${index}`,
      name: message.name ?? call?.function.name ?? 'tool',
      arguments: call?.function.arguments ?? '{}',
      startedAt: '',
      durationMs: 0,
      ok: payload.ok,
      ...(payload.ok ? { result: payload.result } : { error: payload.error }),
    }
    step.toolResults.push(invocation)
  })

  return result
}
