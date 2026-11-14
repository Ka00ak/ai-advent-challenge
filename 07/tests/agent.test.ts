import { afterEach, describe, expect, it, vi } from 'vitest'
import { Agent } from '../src/agent/Agent'
import { ToolRegistry } from '../src/agent/ToolRegistry'
import { calculatorTool } from '../src/tools/calculator'
import { convertCurrencyTool } from '../src/tools/currency'
import type { AgentConfig } from '../src/agent/Agent'
import type { LlmRequest, ToolCall } from '../src/types'

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
})
