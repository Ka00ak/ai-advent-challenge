import { estimateMessageTokens, estimateMessagesTokens } from '../tokens'
import type { ChatMessage } from '../types'

/**
 * Управление контекстом: последние N сообщений уходят в модель как есть,
 * более старая часть истории заменяется сводкой (summary).
 *
 * Сводка хранится отдельно от полной истории и обновляется порциями: когда
 * количество ещё не сжатых старых сообщений достигает `summarizeEvery`.
 * Полная история при этом никуда не исчезает — она остаётся в localStorage и
 * в интерфейсе, а в запрос подставляется только summary + «хвост».
 */

export type SummaryMode = 'llm' | 'local'

export interface CompressionConfig {
  enabled: boolean
  /** Сколько последних сообщений всегда отправлять без изменений. */
  keepRecent: number
  /** Сколько старых сообщений накопить, прежде чем обновить сводку. */
  summarizeEvery: number
  /** llm — сжатие отдельным запросом к модели, local — локальная выжимка. */
  mode: SummaryMode
}

export const DEFAULT_COMPRESSION: CompressionConfig = {
  enabled: true,
  keepRecent: 8,
  summarizeEvery: 10,
  mode: 'llm',
}

export const DISABLED_COMPRESSION: CompressionConfig = {
  ...DEFAULT_COMPRESSION,
  enabled: false,
}

/** Состояние сводки. Хранится отдельно от массива messages. */
export interface ContextSummary {
  text: string
  /**
   * Сколько сообщений с начала истории уже вошло в сводку.
   * Всё, что идёт после этого индекса, уходит в запрос как есть.
   */
  coveredMessages: number
  updatedAt: string
  mode: SummaryMode
  /** Сколько раз сводка обновлялась (порций сжатия). */
  chunks: number
}

export interface SummaryGeneratorInput {
  previousSummary: string
  messages: ChatMessage[]
}

export interface SummaryGeneratorOutput {
  text: string
  mode: SummaryMode
  /** Сколько HTTP-запросов сделал генератор (локальному — 0). */
  requests?: number
  promptTokens?: number
  completionTokens?: number
  costUsd?: number
  usageReported?: boolean
}

export type SummaryGenerator = (
  input: SummaryGeneratorInput,
) => Promise<SummaryGeneratorOutput>

function clampInt(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min
  return Math.min(max, Math.max(min, Math.floor(value)))
}

export function normalizeCompression(
  config?: Partial<CompressionConfig> | null,
): CompressionConfig {
  // Без явной настройки сжатие выключено: так Agent сохраняет поведение 08,
  // пока приложение не передаст свои параметры.
  if (!config) return { ...DISABLED_COMPRESSION }

  return {
    enabled: config.enabled ?? DEFAULT_COMPRESSION.enabled,
    keepRecent: clampInt(config.keepRecent ?? DEFAULT_COMPRESSION.keepRecent, 1, 200),
    summarizeEvery: clampInt(
      config.summarizeEvery ?? DEFAULT_COMPRESSION.summarizeEvery,
      1,
      500,
    ),
    mode: config.mode === 'local' ? 'local' : 'llm',
  }
}

export function normalizeSummary(
  summary?: ContextSummary | null,
  messageCount?: number,
): ContextSummary | null {
  if (!summary?.text?.trim()) return null

  const maxCovered =
    typeof messageCount === 'number' ? Math.max(0, messageCount) : Number.MAX_SAFE_INTEGER

  return {
    ...summary,
    text: summary.text.trim(),
    coveredMessages: clampInt(summary.coveredMessages ?? 0, 0, maxCovered),
    chunks: Math.max(1, clampInt(summary.chunks ?? 1, 1, 1_000_000)),
  }
}

/**
 * Индекс начала «свежего» окна. Двигаемся от конца назад, но встаём только на
 * границу пользовательского сообщения: нельзя разрывать пару
 * assistant(tool_calls) → tool, иначе API отклонит запрос.
 */
export function findRecentStart(messages: ChatMessage[], keepRecent: number): number {
  if (keepRecent <= 0) return messages.length

  let start = Math.max(0, messages.length - Math.floor(keepRecent))
  while (start > 0 && messages[start]?.role !== 'user') start -= 1
  return start
}

export interface CompressionPlan {
  enabled: boolean
  fullHistoryTokens: number
  /** Индекс первого сообщения, которое остаётся в запросе без сжатия. */
  recentStart: number
  coveredMessages: number
  summarizedMessages: number
  pendingMessages: number
  recentMessages: number
  needsSummary: boolean
  /** Границы порции, которую нужно свернуть в сводку. */
  summarizeFrom: number
  summarizeTo: number
  pending: ChatMessage[]
  tail: ChatMessage[]
}

export function planCompression(
  messages: ChatMessage[],
  config: CompressionConfig,
  summary?: ContextSummary | null,
): CompressionPlan {
  const history = messages.filter(message => message.role !== 'system')
  const normalized = normalizeSummary(summary, history.length)
  const fullHistoryTokens = estimateMessagesTokens(history)

  if (!config.enabled) {
    return {
      enabled: false,
      fullHistoryTokens,
      recentStart: history.length,
      coveredMessages: 0,
      summarizedMessages: 0,
      pendingMessages: 0,
      recentMessages: history.length,
      needsSummary: false,
      summarizeFrom: history.length,
      summarizeTo: history.length,
      pending: [],
      tail: history,
    }
  }

  const coveredMessages = normalized?.coveredMessages ?? 0
  // Если пользователь уменьшил keepRecent после сжатия, сводка уже покрывает
  // часть «свежего» окна. Не дублируем эти сообщения: начинаем хвост после
  // покрытой части.
  const recentStart = Math.max(
    findRecentStart(history, config.keepRecent),
    Math.min(history.length, coveredMessages),
  )

  const pending = history.slice(coveredMessages, recentStart)
  const needsSummary = pending.length >= config.summarizeEvery

  return {
    enabled: true,
    fullHistoryTokens,
    recentStart,
    coveredMessages,
    summarizedMessages: coveredMessages,
    pendingMessages: pending.length,
    recentMessages: Math.max(0, history.length - recentStart),
    needsSummary,
    summarizeFrom: coveredMessages,
    summarizeTo: needsSummary ? recentStart : coveredMessages,
    pending,
    tail: history.slice(coveredMessages),
  }
}

function compact(text: string, limit: number): string {
  const normalized = text.replace(/\s+/g, ' ').trim()
  if (normalized.length <= limit) return normalized
  return `${normalized.slice(0, Math.max(0, limit - 1)).trimEnd()}…`
}

/** Компактная расшифровка истории для запроса-суммаризатора. */
export function renderTranscript(messages: ChatMessage[]): string {
  return messages
    .map(message => {
      if (message.role === 'user') {
        return `ПОЛЬЗОВАТЕЛЬ: ${message.content ?? ''}`
      }

      if (message.role === 'assistant') {
        const parts: string[] = []
        if (message.content?.trim()) parts.push(`АССИСТЕНТ: ${message.content}`)
        for (const call of message.tool_calls ?? []) {
          parts.push(
            `АССИСТЕНТ ВЫЗВАЛ: ${call.function.name}(${compact(call.function.arguments, 400)})`,
          )
        }
        return parts.join('\n')
      }

      if (message.role === 'tool') {
        return `РЕЗУЛЬТАТ ${message.name ?? 'tool'}: ${compact(message.content ?? '', 800)}`
      }

      return ''
    })
    .filter(Boolean)
    .join('\n\n')
}

/**
 * Локальная выжимка — запасной вариант без второго запроса к модели.
 * Она намеренно сохраняет пользовательские сообщения почти целиком: именно в
 * них обычно лежат сроки, бюджеты, имена и ограничения.
 */
export function summarizeLocally(
  messages: ChatMessage[],
  previousSummary = '',
): string {
  const lines: string[] = []

  for (const message of messages) {
    if (message.role === 'user') {
      lines.push(`- П: ${compact(message.content ?? '', 500)}`)
      continue
    }

    if (message.role === 'assistant') {
      const text = compact(message.content ?? '', 80)
      if (text) lines.push(`- А: ${text}`)
      for (const call of message.tool_calls ?? []) {
        lines.push(
          `- А вызвал ${call.function.name}(${compact(call.function.arguments, 140)})`,
        )
      }
      continue
    }

    if (message.role === 'tool') {
      lines.push(
        `- Результат ${message.name ?? 'tool'}: ${compact(message.content ?? '', 160)}`,
      )
    }
  }

  const previous = previousSummary.trim()
  const merged = [...(previous ? [previous] : []), ...lines].join('\n')
  return merged || '(в сжатой части не было содержательных сообщений)'
}

export function summaryMessage(summary: ContextSummary): ChatMessage {
  return {
    role: 'system',
    content: `Сводка предыдущей части диалога (${summary.coveredMessages} сообщений, обновлений: ${summary.chunks}):\n${summary.text}`,
  }
}

/**
 * История, которая реально уйдёт в модель (без основного system-промпта):
 * [сводка] + неохваченный остаток + последние N сообщений.
 */
export function buildCompressedContext(
  messages: ChatMessage[],
  config: CompressionConfig,
  summary?: ContextSummary | null,
): ChatMessage[] {
  const history = messages.filter(message => message.role !== 'system')

  if (!config.enabled) return [...history]
  if (!history.length) return []

  const normalized = normalizeSummary(summary, history.length)
  if (!normalized) return [...history]

  const start = Math.min(history.length, normalized.coveredMessages)
  return [summaryMessage(normalized), ...history.slice(start)]
}

export interface CompressionPreview {
  enabled: boolean
  fullHistoryTokens: number
  sentHistoryTokens: number
  summaryTokens: number
  tailTokens: number
  savedTokens: number
  savedRatio: number
  recentMessages: number
  summarizedMessages: number
  pendingMessages: number
  needsSummary: boolean
  keepRecent: number
  summarizeEvery: number
  usedSummary: boolean
}

export function previewCompression(
  messages: ChatMessage[],
  config: CompressionConfig,
  summary?: ContextSummary | null,
): CompressionPreview {
  const history = messages.filter(message => message.role !== 'system')
  const plan = planCompression(history, config, summary)
  const prepared = buildCompressedContext(history, config, summary)
  const normalized =
    config.enabled && history.length
      ? normalizeSummary(summary, history.length)
      : null
  const summaryTokens = normalized ? estimateMessageTokens(summaryMessage(normalized)) : 0
  const sentHistoryTokens = estimateMessagesTokens(prepared)
  const savedTokens = plan.fullHistoryTokens - sentHistoryTokens

  return {
    enabled: config.enabled,
    fullHistoryTokens: plan.fullHistoryTokens,
    sentHistoryTokens,
    summaryTokens,
    tailTokens: Math.max(0, sentHistoryTokens - summaryTokens),
    savedTokens,
    savedRatio: plan.fullHistoryTokens > 0 ? savedTokens / plan.fullHistoryTokens : 0,
    recentMessages: plan.recentMessages,
    summarizedMessages: plan.summarizedMessages,
    pendingMessages: plan.pendingMessages,
    needsSummary: plan.needsSummary,
    keepRecent: config.keepRecent,
    summarizeEvery: config.summarizeEvery,
    usedSummary: Boolean(config.enabled && normalized),
  }
}

export interface CompressionRunStats {
  enabled: boolean
  usedSummary: boolean
  summaryUpdated: boolean
  /** Сколько порций сводки было обновлено (включая локальные). */
  summaryUpdates: number
  /** Сколько HTTP-запросов на summarization реально ушло. */
  summaryRequests: number
  summaryPromptTokens: number
  summaryCompletionTokens: number
  summaryCostUsd: number
  summaryUsageReported: number
  summaryError?: string
  fullHistoryTokens: number
  sentHistoryTokens: number
  summaryTokens: number
  tailTokens: number
  savedTokens: number
  recentMessages: number
  summarizedMessages: number
}

export interface CompressionOutcome {
  summary: ContextSummary | null
  /** Готовая история для модели: [system-сводка] + хвост. */
  prepared: ChatMessage[]
  stats: CompressionRunStats
  plan: CompressionPlan
}

export interface CompressHistoryInput {
  messages: ChatMessage[]
  config: CompressionConfig
  summary?: ContextSummary | null
  generateSummary?: SummaryGenerator
  /** Для тестов и детерминированных снимков. */
  now?: () => Date
}

function isAbortError(error: unknown): boolean {
  return (
    (error instanceof DOMException && error.name === 'AbortError') ||
    (error instanceof Error && error.name === 'AbortError')
  )
}

/**
 * Основная операция: при необходимости обновляет сводку порциями и возвращает
 * подготовленный контекст. Полная история сохраняется вызывающей стороной.
 */
export async function compressHistory(
  input: CompressHistoryInput,
): Promise<CompressionOutcome> {
  const config = normalizeCompression(input.config)
  const history = input.messages.filter(message => message.role !== 'system')
  const now = input.now ?? (() => new Date())

  let summary = normalizeSummary(input.summary, history.length)
  let plan = planCompression(history, config, summary)

  let summaryUpdates = 0
  let summaryRequests = 0
  let summaryPromptTokens = 0
  let summaryCompletionTokens = 0
  let summaryCostUsd = 0
  let summaryUsageReported = 0
  let summaryError: string | undefined
  let guard = 0

  while (config.enabled && plan.needsSummary && guard < 8) {
    guard += 1
    const chunk = history.slice(plan.summarizeFrom, plan.summarizeTo)
    const previousSummary = summary?.text ?? ''

    let output: SummaryGeneratorOutput
    if (config.mode === 'llm' && input.generateSummary) {
      try {
        output = await input.generateSummary({ previousSummary, messages: chunk })
      } catch (error) {
        if (isAbortError(error)) throw error
        summaryError = error instanceof Error ? error.message : String(error)
        output = {
          text: summarizeLocally(chunk, previousSummary),
          mode: 'local',
        }
      }
    } else {
      output = {
        text: summarizeLocally(chunk, previousSummary),
        mode: 'local',
      }
    }

    const text = output.text.trim()
    summary = {
      text: text || previousSummary || '(сводка пуста)',
      coveredMessages: plan.summarizeTo,
      updatedAt: now().toISOString(),
      mode: output.mode,
      chunks: (summary?.chunks ?? 0) + 1,
    }

    summaryUpdates += 1
    summaryRequests += output.requests ?? (output.mode === 'llm' ? 1 : 0)
    summaryPromptTokens += output.promptTokens ?? 0
    summaryCompletionTokens += output.completionTokens ?? 0
    summaryCostUsd += output.costUsd ?? 0
    if (output.usageReported) summaryUsageReported += 1

    plan = planCompression(history, config, summary)
  }

  const prepared = buildCompressedContext(history, config, summary)
  const normalized =
    config.enabled && history.length
      ? normalizeSummary(summary, history.length)
      : null
  const summaryTokens = normalized ? estimateMessageTokens(summaryMessage(normalized)) : 0
  const sentHistoryTokens = estimateMessagesTokens(prepared)

  return {
    summary,
    prepared,
    plan,
    stats: {
      enabled: config.enabled,
      usedSummary: Boolean(config.enabled && normalized),
      summaryUpdated: summaryUpdates > 0,
      summaryUpdates,
      summaryRequests,
      summaryPromptTokens,
      summaryCompletionTokens,
      summaryCostUsd,
      summaryUsageReported,
      ...(summaryError ? { summaryError } : {}),
      fullHistoryTokens: plan.fullHistoryTokens,
      sentHistoryTokens,
      summaryTokens,
      tailTokens: Math.max(0, sentHistoryTokens - summaryTokens),
      savedTokens: plan.fullHistoryTokens - sentHistoryTokens,
      recentMessages: plan.recentMessages,
      summarizedMessages: normalized?.coveredMessages ?? 0,
    },
  }
}
