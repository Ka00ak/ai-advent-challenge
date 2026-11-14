import { afterEach, describe, expect, it, vi } from 'vitest'
import { Agent } from '../src/agent/Agent'
import { ToolRegistry } from '../src/agent/ToolRegistry'
import {
  ContextOverflowError,
  analyzeContext,
  buildPromptEstimate,
  buildTokenScenarios,
  estimateCost,
  estimateMessageTokens,
  estimateTextTokens,
  formatUsd,
  resolveModelProfile,
  summarizeExchanges,
} from '../src/tokens'
import { calculatorTool } from '../src/tools/calculator'
import type { AgentConfig } from '../src/agent/Agent'
import type { ChatMessage, LlmExchange, LlmRequest } from '../src/types'

const config: AgentConfig = {
  baseUrl: 'https://example.test/v1',
  apiKey: 'test-key',
  model: 'test-model',
  temperature: 0,
  maxTokens: null,
  maxIterations: 6,
  contextLimit: null,
  systemPrompt: 'Ты тестовый агент.',
}

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

function installFetchMock(responses: unknown[]): ReturnType<typeof vi.fn> {
  const requests: LlmRequest[] = []
  const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
    requests.push(JSON.parse(String(init?.body)) as LlmRequest)
    return jsonResponse(responses.shift())
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('оценка токенов', () => {
  it('пустая строка — ноль, непустая растёт вместе с длиной', () => {
    expect(estimateTextTokens('')).toBe(0)
    expect(estimateTextTokens('привет')).toBeGreaterThan(0)
    expect(estimateTextTokens('привет '.repeat(10))).toBeGreaterThan(
      estimateTextTokens('привет'),
    )
  })

  it('кириллица дороже латиницы той же длины', () => {
    expect(estimateTextTokens('приветствие')).toBeGreaterThan(
      estimateTextTokens('greetings'),
    )
  })

  it('учитывает tool_calls и служебные поля сообщения', () => {
    const plain: ChatMessage = { role: 'assistant', content: 'ok' }
    const withTool: ChatMessage = {
      role: 'assistant',
      content: null,
      tool_calls: [
        {
          id: 'call_1',
          type: 'function',
          function: { name: 'calculate', arguments: '{"expression":"1234*56+78"}' },
        },
      ],
    }

    expect(estimateMessageTokens(withTool)).toBeGreaterThan(estimateMessageTokens(plain))
  })

  it('buildPromptEstimate раскладывает запрос по частям без потерь', () => {
    const messages: ChatMessage[] = [
      { role: 'system', content: 'Ты тестовый агент.' },
      { role: 'user', content: 'Посчитай 2 + 2.' },
      { role: 'assistant', content: 'Сейчас посчитаю.' },
      { role: 'user', content: 'И добавь 10%.' },
    ]
    const tools = [
      {
        type: 'function' as const,
        function: {
          name: 'calculate',
          description: 'Считает выражения',
          parameters: { type: 'object', properties: { expression: { type: 'string' } } },
        },
      },
    ]

    const estimate = buildPromptEstimate(messages, tools, 100_000)
    expect(estimate.systemTokens).toBeGreaterThan(0)
    expect(estimate.historyTokens).toBeGreaterThan(0)
    expect(estimate.currentTokens).toBeGreaterThan(0)
    expect(estimate.toolsTokens).toBeGreaterThan(0)
    expect(estimate.totalTokens).toBe(
      estimate.systemTokens +
        estimate.historyTokens +
        estimate.currentTokens +
        estimate.toolsTokens,
    )
    expect(estimate.status).toBe('ok')
  })
})

describe('analyzeContext', () => {
  const messages: ChatMessage[] = [{ role: 'user', content: 'Привет, как дела?' }]

  it('различает ok, warning и overflow', () => {
    const base = analyzeContext({ messages, limit: 1_000_000 })
    expect(base.status).toBe('ok')

    const warning = analyzeContext({
      messages,
      limit: Math.floor(base.totalTokens / 0.9),
    })
    expect(warning.status).toBe('warning')

    const overflow = analyzeContext({ messages, limit: base.totalTokens - 1 })
    expect(overflow.status).toBe('overflow')
    expect(overflow.remaining).toBe(0)
  })

  it('включает system, черновик и схемы инструментов', () => {
    const result = analyzeContext({
      systemPrompt: 'Ты агент.',
      messages,
      draftText: 'Новый вопрос',
      tools: [
        {
          type: 'function',
          function: {
            name: 'calculate',
            description: 'Считает',
            parameters: { type: 'object', properties: {} },
          },
        },
      ],
      limit: 100_000,
    })

    expect(result.currentTokens).toBeGreaterThan(0)
    expect(result.toolsTokens).toBeGreaterThan(0)
    expect(result.totalTokens).toBeGreaterThan(result.historyTokens)
  })
})

describe('профиль модели и стоимость', () => {
  it('подставляет лимит по модели и позволяет переопределить его', () => {
    expect(resolveModelProfile('deepseek-chat').contextLimit).toBe(128_000)
    expect(resolveModelProfile('kimi-k3').contextLimit).toBe(256_000)
    expect(resolveModelProfile('deepseek-chat', 4_096).contextLimit).toBe(4_096)
  })

  it('считает стоимость по ценам за миллион токенов', () => {
    const profile = resolveModelProfile('deepseek-chat')
    const cost = estimateCost(profile, 1_000_000, 1_000_000)
    expect(cost.inputUsd).toBeCloseTo(profile.inputUsdPerMillion, 6)
    expect(cost.outputUsd).toBeCloseTo(profile.outputUsdPerMillion, 6)
    expect(formatUsd(cost.totalUsd)).toMatch(/^\$/)
  })

  it('суммирует usage и помечает запросы без точного usage', () => {
    const profile = resolveModelProfile('deepseek-chat')
    const exchanges: LlmExchange[] = [
      {
        id: '1',
        url: '',
        startedAt: '',
        durationMs: 0,
        request: { model: 'deepseek-chat', messages: [] },
        usage: { prompt_tokens: 100, completion_tokens: 20, total_tokens: 120 },
        costUsd: 0.001,
      },
      {
        id: '2',
        url: '',
        startedAt: '',
        durationMs: 0,
        request: { model: 'deepseek-chat', messages: [] },
        estimate: {
          systemTokens: 10,
          historyTokens: 50,
          currentTokens: 10,
          toolsTokens: 5,
          totalTokens: 75,
          limit: 1000,
          ratio: 0.075,
          remaining: 925,
          status: 'ok',
        },
      },
    ]

    const summary = summarizeExchanges(exchanges, profile)
    expect(summary.requests).toBe(2)
    expect(summary.promptTokens).toBe(175)
    expect(summary.completionTokens).toBe(20)
    expect(summary.totalTokens).toBe(195)
    expect(summary.reportedRequests).toBe(1)
    expect(summary.estimatedRequests).toBe(1)
  })
})

describe('сценарии', () => {
  it('короткий влезает, длинный близок к лимиту, overflow — переполнен', () => {
    const profile = resolveModelProfile('deepseek-chat', 10_000)
    const scenarios = buildTokenScenarios(profile)

    expect(scenarios.map(item => item.kind)).toEqual(['short', 'long', 'overflow'])
    expect(scenarios[0]?.status).toBe('ok')
    expect(scenarios[1]?.status).not.toBe('overflow')
    expect(scenarios[2]?.status).toBe('overflow')
    expect(scenarios[2]?.promptTokens).toBeGreaterThan(10_000)
  })
})

describe('Agent и токены', () => {
  it('блокирует запрос до отправки, если контекст переполнен', async () => {
    const fetchMock = installFetchMock([])
    const history: ChatMessage[] = [
      { role: 'user', content: 'Длинная история. '.repeat(200) },
      { role: 'assistant', content: 'Ответ. '.repeat(200) },
    ]

    const agent = new Agent({ ...config, contextLimit: 100 }, new ToolRegistry([calculatorTool]))

    await expect(agent.run('Ещё вопрос', history)).rejects.toBeInstanceOf(ContextOverflowError)
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('возвращает токены запроса, ответа и стоимость запуска', async () => {
    installFetchMock([
      {
        choices: [{ index: 0, message: { role: 'assistant', content: 'Готово.' }, finish_reason: 'stop' }],
        usage: { prompt_tokens: 80, completion_tokens: 12, total_tokens: 92 },
      },
    ])

    const agent = new Agent(config, new ToolRegistry([calculatorTool]))
    const result = await agent.run('Привет', [])

    expect(result.stoppedReason).toBe('final')
    expect(result.tokenStats.requests).toHaveLength(1)
    expect(result.tokenStats.promptTokens).toBe(80)
    expect(result.tokenStats.completionTokens).toBe(12)
    expect(result.tokenStats.totalTokens).toBe(92)
    expect(result.tokenStats.reportedRequests).toBe(1)
    expect(result.tokenStats.costUsd).toBeGreaterThan(0)
    expect(result.steps[0]?.exchange.estimate?.totalTokens).toBeGreaterThan(0)
    expect(result.steps[0]?.exchange.costUsd).toBeCloseTo(result.tokenStats.costUsd, 8)
  })
})
