import { afterEach, describe, expect, it, vi } from 'vitest'
import { Agent } from '../src/agent/Agent'
import { ToolRegistry } from '../src/agent/ToolRegistry'
import {
  CONVERSATION_VERSION,
  loadConversation,
  saveConversation,
  type KeyValueStorage,
  type StoredConversation,
} from '../src/storage/conversation'
import { calculatorTool } from '../src/tools/calculator'
import { convertCurrencyTool } from '../src/tools/currency'
import type { AgentConfig } from '../src/agent/Agent'
import type { ChatMessage, LlmRequest, ToolCall } from '../src/types'

const config: AgentConfig = {
  baseUrl: 'https://example.test/v1',
  apiKey: 'test-key',
  model: 'test-model',
  temperature: 0,
  maxTokens: null,
  maxIterations: 6,
  systemPrompt: 'Ты тестовый агент.',
}

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

function completion(message: unknown, finishReason: string): unknown {
  return {
    id: 'resp_test',
    choices: [{ index: 0, message, finish_reason: finishReason }],
    usage: { prompt_tokens: 5, completion_tokens: 5, total_tokens: 10 },
  }
}

function toolCall(id: string, name: string, args: Record<string, unknown>): ToolCall {
  return {
    id,
    type: 'function',
    function: { name, arguments: JSON.stringify(args) },
  }
}

function installFetchMock(responses: unknown[]): { requests: LlmRequest[]; fetchMock: ReturnType<typeof vi.fn> } {
  const requests: LlmRequest[] = []
  const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
    requests.push(JSON.parse(String(init?.body)) as LlmRequest)
    return jsonResponse(responses.shift())
  })
  vi.stubGlobal('fetch', fetchMock)
  return { requests, fetchMock }
}

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

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('Agent', () => {
  it('отвечает без инструментов, если они не нужны', async () => {
    const { requests, fetchMock } = installFetchMock([
      completion({ role: 'assistant', content: 'HTTP — без шифрования, HTTPS — с TLS.' }, 'stop'),
    ])

    const agent = new Agent(config, new ToolRegistry([calculatorTool]))
    const result = await agent.run('Чем HTTP отличается от HTTPS?', [])

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(result.stoppedReason).toBe('final')
    expect(result.finalText).toContain('HTTPS')
    expect(result.steps[0]?.toolResults).toHaveLength(0)
    expect(requests[0]?.tools?.[0]?.function.name).toBe('calculate')
  })

  it('вызывает один инструмент и возвращает результат в модель', async () => {
    const { requests } = installFetchMock([
      completion(
        {
          role: 'assistant',
          content: null,
          tool_calls: [toolCall('call_1', 'calculate', { expression: '2 + 2' })],
        },
        'tool_calls',
      ),
      completion({ role: 'assistant', content: 'Получается 4.' }, 'stop'),
    ])

    const agent = new Agent(config, new ToolRegistry([calculatorTool]))
    const result = await agent.run('Сколько будет 2 + 2?', [])

    expect(requests).toHaveLength(2)
    expect(result.stoppedReason).toBe('final')
    expect(result.finalText).toBe('Получается 4.')
    expect(result.steps).toHaveLength(2)

    const toolResult = result.steps[0]?.toolResults[0]
    expect(toolResult?.name).toBe('calculate')
    expect(toolResult?.ok).toBe(true)
    expect(toolResult?.result).toEqual({ expression: '2 + 2', result: 4 })

    const toolMessage = requests[1]?.messages.find(message => message.role === 'tool')
    expect(toolMessage?.tool_call_id).toBe('call_1')
    expect(toolMessage?.content).toContain('"result":4')
  })

  it('делает два вызова подряд и передаёт результат первого во второй', async () => {
    const { requests } = installFetchMock([
      completion(
        {
          role: 'assistant',
          content: null,
          tool_calls: [
            toolCall('call_1', 'convert_currency', { amount: 250, from: 'USD', to: 'EUR' }),
          ],
        },
        'tool_calls',
      ),
      completion(
        {
          role: 'assistant',
          content: null,
          tool_calls: [toolCall('call_2', 'calculate', { expression: '230 * 1.15' })],
        },
        'tool_calls',
      ),
      completion(
        { role: 'assistant', content: '250 USD ≈ 230 EUR, с наценкой 15% — 264.5 EUR.' },
        'stop',
      ),
    ])

    const agent = new Agent(
      config,
      new ToolRegistry([convertCurrencyTool, calculatorTool]),
    )
    const result = await agent.run('Переведи 250 USD в EUR и добавь 15%', [])

    expect(requests).toHaveLength(3)
    expect(result.stoppedReason).toBe('final')
    expect(result.steps).toHaveLength(3)
    expect(result.steps[0]?.toolResults[0]?.result).toMatchObject({ result: 230 })
    expect(result.steps[1]?.toolResults[0]?.result).toMatchObject({ result: 264.5 })

    const toolMessages = requests[2]?.messages.filter(message => message.role === 'tool')
    expect(toolMessages).toHaveLength(2)
    expect(toolMessages?.[0]?.tool_call_id).toBe('call_1')
    expect(toolMessages?.[1]?.tool_call_id).toBe('call_2')
  })

  it('возвращает ошибку инструмента модели, а не падает', async () => {
    const { requests } = installFetchMock([
      completion(
        {
          role: 'assistant',
          content: null,
          tool_calls: [toolCall('call_1', 'calculate', { expression: '1 / 0' })],
        },
        'tool_calls',
      ),
      completion({ role: 'assistant', content: 'На ноль делить нельзя.' }, 'stop'),
    ])

    const agent = new Agent(config, new ToolRegistry([calculatorTool]))
    const result = await agent.run('Сколько будет 1 / 0?', [])

    expect(result.steps[0]?.toolResults[0]?.ok).toBe(false)
    expect(result.steps[0]?.toolResults[0]?.error).toContain('Деление на ноль')
    const toolMessage = requests[1]?.messages.find(message => message.role === 'tool')
    expect(toolMessage?.content).toContain('"ok":false')
  })

  it('продолжает диалог из JSON-истории с tool_calls и результатами', async () => {
    const storage = new MemoryStorage()
    const restoredMessages: ChatMessage[] = [
      { role: 'user', content: 'Сколько будет 1234 * 56 + 78?' },
      {
        role: 'assistant',
        content: null,
        tool_calls: [toolCall('call_1', 'calculate', { expression: '1234*56+78' })],
      },
      {
        role: 'tool',
        tool_call_id: 'call_1',
        name: 'calculate',
        content: JSON.stringify({
          ok: true,
          result: { expression: '1234*56+78', result: 69182 },
        }),
      },
      { role: 'assistant', content: 'Получается 69 182.' },
    ]
    const snapshot: StoredConversation = {
      version: CONVERSATION_VERSION,
      id: 'conversation-restart',
      createdAt: '2026-11-14T10:00:00.000Z',
      updatedAt: '2026-11-14T10:01:00.000Z',
      messages: restoredMessages,
    }
    saveConversation(snapshot, storage)

    // Имитируем перезапуск: новый load из хранилища и новый экземпляр агента.
    const restored = loadConversation(storage).conversation
    expect(restored?.messages).toEqual(restoredMessages)

    const { requests } = installFetchMock([
      completion({ role: 'assistant', content: '69 182 × 2 = 138 364.' }, 'stop'),
    ])

    const agent = new Agent(config, new ToolRegistry([calculatorTool]))
    const result = await agent.run('Умножь прошлый результат на 2.', restored?.messages ?? [])

    expect(result.finalText).toBe('69 182 × 2 = 138 364.')

    const sent = requests[0]?.messages ?? []
    expect(sent.map(message => message.role)).toEqual([
      'system',
      'user',
      'assistant',
      'tool',
      'assistant',
      'user',
    ])
    expect(sent[2]?.tool_calls?.[0]?.function.name).toBe('calculate')
    expect(sent[3]).toMatchObject({ role: 'tool', tool_call_id: 'call_1' })
    expect(sent[3]?.content).toContain('"result":69182')
    expect(sent[4]?.content).toBe('Получается 69 182.')
    expect(sent[5]).toMatchObject({ role: 'user', content: 'Умножь прошлый результат на 2.' })
  })

  it('отдаёт каждое новое сообщение в onMessageAppended для автосохранения', async () => {
    installFetchMock([
      completion({ role: 'assistant', content: 'Привет! Чем помочь?' }, 'stop'),
    ])

    const appended: ChatMessage[] = []
    const agent = new Agent(config, new ToolRegistry([calculatorTool]))
    await agent.run('Привет', [], {
      onMessageAppended: message => appended.push(message),
    })

    expect(appended.map(message => message.role)).toEqual(['user', 'assistant'])
    expect(appended[0]?.content).toBe('Привет')
    expect(appended[1]?.content).toBe('Привет! Чем помочь?')
  })
})
