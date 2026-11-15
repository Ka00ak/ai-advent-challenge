import type { ChatMessage, LlmExchange, ToolSchema, TokenUsage } from './types'

/**
 * Подсчёт токенов.
 *
 * В Chat Completions API точное число токенов считает токенизатор модели.
 * В браузере мы не тащим BPE-словари, поэтому используем оценку по составу
 * текста: кириллица в BPE обычно дороже латиницы, JSON и пунктуация — что-то
 * среднее. Оценка нужна, чтобы увидеть порядок величин и заранее поймать
 * переполнение контекста. После ответа API точные значения приходят в `usage`
 * (`prompt_tokens` / `completion_tokens`) — они имеют приоритет в интерфейсе.
 */

const CYRILLIC = /[\u0400-\u052f]/u
const LATIN_OR_DIGIT = /[A-Za-z0-9]/u
const WHITESPACE = /\s/u

/** Оценка токенов для произвольной строки. Ноль для пустой строки. */
export function estimateTextTokens(text: string): number {
  if (!text) return 0

  let cyrillic = 0
  let latin = 0
  let punctuation = 0
  let whitespace = 0
  let wide = 0

  for (const char of text) {
    if (WHITESPACE.test(char)) {
      whitespace += 1
    } else if (CYRILLIC.test(char)) {
      cyrillic += 1
    } else if (LATIN_OR_DIGIT.test(char)) {
      latin += 1
    } else if ((char.codePointAt(0) ?? 0) > 0xffff) {
      wide += 1
    } else {
      punctuation += 1
    }
  }

  const tokens =
    cyrillic / 2 +
    latin / 3.8 +
    punctuation / 2.5 +
    whitespace / 14 +
    wide * 2

  return Math.max(1, Math.ceil(tokens))
}

/** Оценка одного сообщения вместе со служебными полями формата API. */
export function estimateMessageTokens(message: ChatMessage): number {
  const content = typeof message.content === 'string' ? message.content : ''
  let tokens = 4 + estimateTextTokens(content)

  if (message.name) tokens += estimateTextTokens(message.name) + 1
  if (message.tool_call_id) tokens += estimateTextTokens(message.tool_call_id) + 3

  for (const call of message.tool_calls ?? []) {
    tokens += 8
    tokens += estimateTextTokens(call.id)
    tokens += estimateTextTokens(call.function.name)
    tokens += estimateTextTokens(call.function.arguments)
  }

  return tokens
}

export function estimateMessagesTokens(messages: ChatMessage[]): number {
  return messages.reduce((total, message) => total + estimateMessageTokens(message), 0)
}

/** Схемы инструментов тоже уходят в каждый запрос и занимают контекст. */
export function estimateToolsTokens(tools: ToolSchema[]): number {
  if (!tools.length) return 0
  try {
    return estimateTextTokens(JSON.stringify(tools))
  } catch {
    return 0
  }
}

export type ContextStatus = 'ok' | 'warning' | 'overflow'

/**
 * Разбивка промпта по частям. `currentTokens` — то, что добавляется прямо
 * сейчас: черновик в поле ввода (для живой оценки) или последнее сообщение
 * запроса (для оценки перед отправкой).
 */
export interface ContextBreakdown {
  systemTokens: number
  historyTokens: number
  currentTokens: number
  toolsTokens: number
  totalTokens: number
  limit: number
  ratio: number
  remaining: number
  status: ContextStatus
}

export interface AnalyzeContextInput {
  systemPrompt?: string
  messages: ChatMessage[]
  draftText?: string
  tools?: ToolSchema[]
  limit: number
}

function statusFor(total: number, limit: number): ContextStatus {
  if (limit <= 0) return 'ok'
  if (total > limit) return 'overflow'
  if (total / limit >= 0.8) return 'warning'
  return 'ok'
}

export function analyzeContext(input: AnalyzeContextInput): ContextBreakdown {
  const systemTokens = input.systemPrompt
    ? estimateMessageTokens({ role: 'system', content: input.systemPrompt })
    : 0
  const historyTokens = estimateMessagesTokens(input.messages)
  const draft = input.draftText?.trim() ?? ''
  const currentTokens = draft ? estimateMessageTokens({ role: 'user', content: draft }) : 0
  const toolsTokens = estimateToolsTokens(input.tools ?? [])
  const totalTokens = systemTokens + historyTokens + currentTokens + toolsTokens
  const limit = Math.max(0, Math.floor(input.limit))

  return {
    systemTokens,
    historyTokens,
    currentTokens,
    toolsTokens,
    totalTokens,
    limit,
    ratio: limit > 0 ? totalTokens / limit : 0,
    remaining: Math.max(0, limit - totalTokens),
    status: statusFor(totalTokens, limit),
  }
}

/**
 * Оценка реального запроса агента. Разбивка:
 * system = первый system-промпт, current = последнее добавленное сообщение,
 * history = всё, что было до него.
 */
export function buildPromptEstimate(
  messages: ChatMessage[],
  tools: ToolSchema[],
  limit: number,
): ContextBreakdown {
  const systemMessages = messages.filter(message => message.role === 'system')
  const rest = messages.filter(message => message.role !== 'system')
  const current = rest.at(-1)
  const history = current ? rest.slice(0, -1) : rest

  const systemTokens = estimateMessagesTokens(systemMessages)
  const historyTokens = estimateMessagesTokens(history)
  const currentTokens = current ? estimateMessageTokens(current) : 0
  const toolsTokens = estimateToolsTokens(tools)
  const totalTokens = systemTokens + historyTokens + currentTokens + toolsTokens
  const safeLimit = Math.max(0, Math.floor(limit))

  return {
    systemTokens,
    historyTokens,
    currentTokens,
    toolsTokens,
    totalTokens,
    limit: safeLimit,
    ratio: safeLimit > 0 ? totalTokens / safeLimit : 0,
    remaining: Math.max(0, safeLimit - totalTokens),
    status: statusFor(totalTokens, safeLimit),
  }
}

export function formatTokens(tokens: number): string {
  return Math.max(0, Math.round(tokens)).toLocaleString('ru-RU')
}

export function formatPercent(ratio: number): string {
  const percent = Math.max(0, ratio * 100)
  return `${percent < 10 ? percent.toFixed(1) : Math.round(percent)}%`
}

/* ------------------------------------------------------------------ */
/* Модели: контекстное окно и цены                                     */
/* ------------------------------------------------------------------ */

export interface ModelProfile {
  /** Идентификатор модели, для которой подобран профиль. */
  id: string
  label: string
  /** Сколько токенов влезает в один запрос: system + history + tools + ответ. */
  contextLimit: number
  maxOutputTokens: number
  /** Цена за 1M входных токенов, USD. Ориентировочно. */
  inputUsdPerMillion: number
  /** Цена за 1M выходных токенов, USD. Ориентировочно. */
  outputUsdPerMillion: number
}

/**
 * Ориентировочные профили. Цены и лимиты меняются у провайдеров, поэтому их
 * можно переопределить в настройках (лимит контекста). Точную стоимость всегда
 * считает биллинг провайдера; здесь — оценка для сравнения сценариев.
 */
export const MODEL_PROFILES: ModelProfile[] = [
  {
    id: 'deepseek',
    label: 'DeepSeek',
    contextLimit: 128_000,
    maxOutputTokens: 8_192,
    inputUsdPerMillion: 0.28,
    outputUsdPerMillion: 0.42,
  },
  {
    id: 'moonshot',
    label: 'Moonshot / Kimi',
    contextLimit: 256_000,
    maxOutputTokens: 32_768,
    inputUsdPerMillion: 0.6,
    outputUsdPerMillion: 2.5,
  },
  {
    id: 'generic',
    label: 'OpenAI-совместимая модель',
    contextLimit: 128_000,
    maxOutputTokens: 8_192,
    inputUsdPerMillion: 0.5,
    outputUsdPerMillion: 1.5,
  },
]

function profileForModel(model: string): ModelProfile {
  const id = model.trim().toLowerCase()
  if (id.includes('deepseek')) return MODEL_PROFILES[0]!
  if (id.includes('kimi') || id.includes('moonshot')) return MODEL_PROFILES[1]!
  return MODEL_PROFILES[2]!
}

/** Профиль модели с учётом ручного переопределения лимита контекста. */
export function resolveModelProfile(model: string, contextLimitOverride?: number | null): ModelProfile {
  const base = profileForModel(model)
  const override =
    typeof contextLimitOverride === 'number' && contextLimitOverride > 0
      ? Math.floor(contextLimitOverride)
      : null

  return {
    ...base,
    id: model.trim() || base.id,
    contextLimit: override ?? base.contextLimit,
  }
}

/* ------------------------------------------------------------------ */
/* Стоимость                                                           */
/* ------------------------------------------------------------------ */

export interface CostEstimate {
  inputUsd: number
  outputUsd: number
  totalUsd: number
}

export function estimateCost(
  profile: ModelProfile,
  promptTokens: number,
  completionTokens: number,
): CostEstimate {
  const inputUsd = (Math.max(0, promptTokens) / 1_000_000) * profile.inputUsdPerMillion
  const outputUsd = (Math.max(0, completionTokens) / 1_000_000) * profile.outputUsdPerMillion
  return { inputUsd, outputUsd, totalUsd: inputUsd + outputUsd }
}

export function formatUsd(usd: number): string {
  if (!Number.isFinite(usd) || usd <= 0) return '$0'
  if (usd < 0.000001) return '<$0.000001'
  if (usd < 0.01) return `$${usd.toFixed(6)}`
  if (usd < 1) return `$${usd.toFixed(4)}`
  return `$${usd.toFixed(2)}`
}

/* ------------------------------------------------------------------ */
/* Сводка по журналу запросов                                          */
/* ------------------------------------------------------------------ */

export interface UsageSummary {
  requests: number
  promptTokens: number
  completionTokens: number
  totalTokens: number
  costUsd: number
  reportedRequests: number
  estimatedRequests: number
}

/** Суммирует usage всех запросов сессии. Если usage не пришёл — берёт оценку. */
export function summarizeExchanges(
  exchanges: LlmExchange[],
  profile: ModelProfile,
): UsageSummary {
  const summary: UsageSummary = {
    requests: exchanges.length,
    promptTokens: 0,
    completionTokens: 0,
    totalTokens: 0,
    costUsd: 0,
    reportedRequests: 0,
    estimatedRequests: 0,
  }

  for (const exchange of exchanges) {
    const usage = exchange.usage
    const promptTokens = usage?.prompt_tokens ?? exchange.estimate?.totalTokens ?? 0
    const completionTokens = usage?.completion_tokens ?? 0
    const totalTokens = usage?.total_tokens ?? promptTokens + completionTokens
    const cost =
      typeof exchange.costUsd === 'number'
        ? exchange.costUsd
        : estimateCost(profile, promptTokens, completionTokens).totalUsd

    summary.promptTokens += promptTokens
    summary.completionTokens += completionTokens
    summary.totalTokens += totalTokens
    summary.costUsd += cost
    if (usage && typeof usage.prompt_tokens === 'number') summary.reportedRequests += 1
    else summary.estimatedRequests += 1
  }

  return summary
}

export function usageCost(profile: ModelProfile, usage?: TokenUsage): number {
  if (!usage) return 0
  return estimateCost(
    profile,
    usage.prompt_tokens ?? 0,
    usage.completion_tokens ?? 0,
  ).totalUsd
}

/* ------------------------------------------------------------------ */
/* Демонстрационные сценарии                                           */
/* ------------------------------------------------------------------ */

export type ScenarioKind = 'short' | 'long' | 'overflow'

export interface TokenScenario {
  kind: ScenarioKind
  title: string
  description: string
  messageCount: number
  promptTokens: number
  limit: number
  ratio: number
  status: ContextStatus
  costUsd: number
  expected: string
}

const SCENARIO_SENTENCE =
  'Агент обсуждает контекстное окно, историю диалога, стоимость токенов и вызовы инструментов. '
const SCENARIO_ANSWER =
  'Модель отвечает кратко, опираясь на историю и результаты инструментов. '

function textWithTokens(source: string, targetTokens: number): string {
  const perRepeat = Math.max(1, estimateTextTokens(source))
  const repeats = Math.max(1, Math.ceil(targetTokens / perRepeat))
  return source.repeat(repeats)
}

function dialogWithTokens(totalTokens: number, pairs: number): ChatMessage[] {
  const perMessage = Math.max(16, Math.floor(totalTokens / Math.max(2, pairs * 2)))
  const messages: ChatMessage[] = []
  for (let index = 0; index < pairs; index += 1) {
    messages.push({
      role: 'user',
      content: textWithTokens(SCENARIO_SENTENCE, perMessage),
    })
    messages.push({
      role: 'assistant',
      content: textWithTokens(SCENARIO_ANSWER, perMessage),
    })
  }
  return messages
}

const scenarioCache = new Map<string, TokenScenario[]>()

/**
 * Три сценария для сравнения: короткий, длинный и превышающий контекст.
 * Строятся локально, без обращения к API, поэтому работают без ключа.
 */
export function buildTokenScenarios(profile: ModelProfile): TokenScenario[] {
  const limit = Math.max(1, profile.contextLimit)
  const cacheKey = `${profile.id}:${limit}`
  const cached = scenarioCache.get(cacheKey)
  if (cached) return cached

  const shortTarget = Math.min(300, Math.max(40, Math.round(limit * 0.01)))
  const longTarget = Math.min(30_000, Math.max(shortTarget * 8, Math.round(limit * 0.55)))
  const overflowTarget = Math.max(limit + 1, Math.round(limit * 1.25))

  const definitions: Array<{
    kind: ScenarioKind
    title: string
    description: string
    messages: ChatMessage[]
    expected: string
  }> = [
    {
      kind: 'short',
      title: 'Короткий диалог',
      description: 'Пара реплик: приветствие и один уточняющий вопрос.',
      messages: dialogWithTokens(shortTarget, 2),
      expected: 'Запрос проходит, контекст почти не расходуется.',
    },
    {
      kind: 'long',
      title: 'Длинный диалог',
      description: `Около ${Math.round(limit * 0.55).toLocaleString('ru-RU')} токенов истории: ещё влезает, но стоимость и задержка растут.`,
      messages: dialogWithTokens(longTarget, 24),
      expected: 'Запрос проходит, но каждый следующий шаг дороже и медленнее.',
    },
    {
      kind: 'overflow',
      title: 'Переполнение контекста',
      description: 'История больше контекстного окна модели.',
      messages: dialogWithTokens(overflowTarget, 40),
      expected:
        'Агент всё равно отправляет запрос: провайдер либо ответит, либо вернёт 400 context_length_exceeded.',
    },
  ]

  const scenarios = definitions.map(definition => {
    const promptTokens = estimateMessagesTokens(definition.messages)
    const ratio = promptTokens / limit
    return {
      kind: definition.kind,
      title: definition.title,
      description: definition.description,
      messageCount: definition.messages.length,
      promptTokens,
      limit,
      ratio,
      status: statusFor(promptTokens, limit),
      costUsd: estimateCost(profile, promptTokens, 0).totalUsd,
      expected: definition.expected,
    }
  })

  scenarioCache.set(cacheKey, scenarios)
  return scenarios
}
