import { describe, expect, it } from 'vitest'
import { repairToolCallSequence } from '../src/agent/history'
import {
  CONVERSATION_STORAGE_KEY,
  CONVERSATION_VERSION,
  clearConversation,
  conversationStats,
  loadConversation,
  parseConversation,
  reconstructUiMessages,
  saveConversation,
  type KeyValueStorage,
  type StoredConversation,
} from '../src/storage/conversation'
import type { ChatMessage } from '../src/types'

class MemoryStorage implements KeyValueStorage {
  private readonly data = new Map<string, string>()

  getItem(key: string): string | null {
    return this.data.get(key) ?? null
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value)
  }

  removeItem(key: string): void {
    this.data.delete(key)
  }
}

const messages: ChatMessage[] = [
  { role: 'user', content: 'Сколько будет 1234 * 56 + 78?' },
  {
    role: 'assistant',
    content: null,
    tool_calls: [
      {
        id: 'call_1',
        type: 'function',
        function: { name: 'calculate', arguments: '{"expression":"1234*56+78"}' },
      },
    ],
  },
  {
    role: 'tool',
    tool_call_id: 'call_1',
    name: 'calculate',
    content: JSON.stringify({ ok: true, result: { expression: '1234*56+78', result: 69182 } }),
  },
  { role: 'assistant', content: 'Получается 69 182.' },
]

function conversation(items: ChatMessage[] = messages): StoredConversation {
  return {
    version: CONVERSATION_VERSION,
    id: 'conversation-test',
    createdAt: '2026-11-14T10:00:00.000Z',
    updatedAt: '2026-11-14T10:01:00.000Z',
    messages: items,
  }
}

describe('conversation storage', () => {
  it('сохраняет и загружает messages вместе с tool_calls и результатами', () => {
    const storage = new MemoryStorage()
    const saved = saveConversation(conversation(), storage)

    expect(saved.ok).toBe(true)
    expect(saved.bytes).toBeGreaterThan(0)
    expect(storage.getItem(CONVERSATION_STORAGE_KEY)).toBeTruthy()

    const loaded = loadConversation(storage)
    expect(loaded.error).toBeUndefined()
    expect(loaded.conversation?.messages).toEqual(messages)

    const assistant = loaded.conversation?.messages[1]
    expect(assistant?.role).toBe('assistant')
    expect(assistant?.tool_calls?.[0]?.function.name).toBe('calculate')
    expect(loaded.conversation?.messages[2]).toMatchObject({
      role: 'tool',
      tool_call_id: 'call_1',
    })
    expect(loaded.conversation?.messages[2]?.content).toContain('"result":69182')
  })

  it('удаляет сохранённую историю', () => {
    const storage = new MemoryStorage()
    saveConversation(conversation(), storage)
    clearConversation(storage)

    expect(storage.getItem(CONVERSATION_STORAGE_KEY)).toBeNull()
    expect(loadConversation(storage).conversation).toBeNull()
  })

  it('отбрасывает повреждённый JSON и неподдерживаемую версию', () => {
    expect(parseConversation('{oops').error).toContain('повреждена')
    expect(parseConversation('null').error).toContain('JSON-объектом')
    expect(parseConversation(JSON.stringify({ version: 99, messages })).error).toContain(
      'Неподдерживаемая версия',
    )
    expect(parseConversation(JSON.stringify({ version: 1, messages: [] })).error).toContain(
      'ни одного корректного',
    )
  })

  it('при импорте отбрасывает system и некорректные сообщения', () => {
    const result = parseConversation(
      JSON.stringify({
        version: 1,
        id: 'imported',
        messages: [
          { role: 'system', content: 'чужой system prompt' },
          { role: 'user', content: 'Привет' },
          { role: 'tool', content: 'без tool_call_id' },
          { role: 'assistant', content: 'Здравствуйте' },
        ],
      }),
    )

    expect(result.conversation?.messages).toEqual([
      { role: 'user', content: 'Привет' },
      { role: 'assistant', content: 'Здравствуйте' },
    ])
  })

  it('считает сообщения, tool_calls и результаты', () => {
    expect(conversationStats(messages)).toEqual({
      total: 4,
      user: 1,
      assistant: 2,
      tool: 1,
      toolCalls: 1,
      toolResults: 1,
    })
  })

  it('восстанавливает UI-ленту с trace инструментов', () => {
    const ui = reconstructUiMessages(messages)

    expect(ui).toHaveLength(2)
    expect(ui[0]).toMatchObject({ role: 'user', content: 'Сколько будет 1234 * 56 + 78?' })
    expect(ui[1]?.role).toBe('assistant')
    expect(ui[1]?.content).toBe('Получается 69 182.')
    expect(ui[1]?.steps).toHaveLength(1)
    expect(ui[1]?.steps[0]?.restored).toBe(true)
    expect(ui[1]?.steps[0]?.toolResults[0]).toMatchObject({
      id: 'call_1',
      name: 'calculate',
      ok: true,
      arguments: '{"expression":"1234*56+78"}',
      result: { expression: '1234*56+78', result: 69182 },
    })
  })

  it('восстанавливает ошибку инструмента из tool-сообщения', () => {
    const ui = reconstructUiMessages([
      { role: 'user', content: 'Посчитай 1 / 0' },
      {
        role: 'assistant',
        content: null,
        tool_calls: [
          {
            id: 'call_1',
            type: 'function',
            function: { name: 'calculate', arguments: '{"expression":"1 / 0"}' },
          },
        ],
      },
      {
        role: 'tool',
        tool_call_id: 'call_1',
        name: 'calculate',
        content: JSON.stringify({ ok: false, error: 'Деление на ноль' }),
      },
    ])

    expect(ui[1]?.steps[0]?.toolResults[0]).toMatchObject({
      ok: false,
      error: 'Деление на ноль',
    })
  })
})

describe('repairToolCallSequence', () => {
  it('не меняет корректную последовательность', () => {
    expect(repairToolCallSequence(messages)).toEqual(messages)
  })

  it('добавляет tool-ответ для прерванного вызова', () => {
    const incomplete: ChatMessage[] = [
      { role: 'user', content: 'Посчитай дважды' },
      {
        role: 'assistant',
        content: null,
        tool_calls: [
          {
            id: 'call_1',
            type: 'function',
            function: { name: 'calculate', arguments: '{"expression":"2+2"}' },
          },
          {
            id: 'call_2',
            type: 'function',
            function: { name: 'calculate', arguments: '{"expression":"3+3"}' },
          },
        ],
      },
      {
        role: 'tool',
        tool_call_id: 'call_1',
        name: 'calculate',
        content: JSON.stringify({ ok: true, result: 4 }),
      },
    ]

    const repaired = repairToolCallSequence(incomplete)
    expect(repaired).toHaveLength(4)
    expect(repaired[3]).toMatchObject({ role: 'tool', tool_call_id: 'call_2' })
    expect(repaired[3]?.content).toContain('"ok":false')
  })
})
