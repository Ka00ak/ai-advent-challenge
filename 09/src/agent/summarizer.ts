import { buildEndpoint, createChatCompletion } from '../api/llm'
import { estimateCost, type ModelProfile } from '../tokens'
import type { ChatMessage, LlmExchange, LlmRequest, LlmSettings, TokenUsage } from '../types'
import {
  renderTranscript,
  type SummaryGenerator,
  type SummaryGeneratorOutput,
} from './compression'

/**
 * Промпт summarization. Главное требование дня 09 — не потерять конкретные
 * детали: сроки, бюджеты, имена, ограничения и принятые решения.
 */
export const SUMMARY_SYSTEM_PROMPT = `Ты сжимаешь историю диалога в долговременную память агента.

Правила:
1. Сохрани все конкретные детали: имена людей и проектов, даты и сроки, бюджеты и суммы, ограничения, принятые решения, договорённости, риски и открытые вопросы.
2. Не теряй числа и названия: их нужно переносить в сводку дословно.
3. Не добавляй ничего от себя и не делай выводов, которых не было в диалоге.
4. Пиши по-русски, короткими пунктами. Без вступления и без вопросов пользователю.
5. Если дана предыдущая сводка — объедини её с новыми сообщениями и верни обновлённую сводку целиком.`

export function buildSummaryUserPrompt(
  previousSummary: string,
  messages: ChatMessage[],
): string {
  const previous = previousSummary.trim()
  const transcript = renderTranscript(messages)
  return [
    previous ? `Предыдущая сводка:\n${previous}` : 'Предыдущей сводки нет.',
    `Новые сообщения, которые нужно добавить в память:\n${transcript}`,
    'Верни обновлённую сводку целиком, сохранив все конкретные детали.',
  ].join('\n\n')
}

function makeId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `id-${Date.now()}-${Math.random().toString(16).slice(2)}`
}

function contentToText(content: unknown): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .map(part => {
        if (typeof part === 'string') return part
        if (part && typeof part === 'object' && 'text' in part) {
          const text = (part as { text?: unknown }).text
          if (typeof text === 'string') return text
        }
        return ''
      })
      .join('')
  }
  return ''
}

export interface SummaryGeneratorOptions {
  settings: LlmSettings
  profile: ModelProfile
  signal?: AbortSignal
  maxSummaryTokens?: number
  onExchangeStart?: (exchange: LlmExchange) => void
  onExchangeEnd?: (exchange: LlmExchange) => void
}

/**
 * Генератор сводки через отдельный запрос к LLM. Возвращает ещё и usage,
 * чтобы интерфейс мог честно показать расход токенов на само сжатие.
 */
export function createLlmSummaryGenerator(
  options: SummaryGeneratorOptions,
): SummaryGenerator {
  return async ({ previousSummary, messages }) => {
    const request: LlmRequest = {
      model: options.settings.model,
      messages: [
        { role: 'system', content: SUMMARY_SYSTEM_PROMPT },
        {
          role: 'user',
          content: buildSummaryUserPrompt(previousSummary, messages),
        },
      ],
      temperature: 0,
      max_tokens: options.maxSummaryTokens ?? 600,
    }

    const exchange: LlmExchange = {
      id: makeId(),
      kind: 'summary',
      url: buildEndpoint(options.settings.baseUrl),
      startedAt: new Date().toISOString(),
      durationMs: 0,
      request,
    }
    options.onExchangeStart?.(exchange)
    const started = performance.now()

    try {
      const response = await createChatCompletion(
        options.settings.baseUrl,
        options.settings.apiKey,
        request,
        options.signal,
      )
      exchange.durationMs = Math.round(performance.now() - started)
      exchange.response = response

      const text = contentToText(response.choices?.[0]?.message?.content).trim()
      if (!text) throw new Error('Модель вернула пустую сводку')

      const usage: TokenUsage | undefined = response.usage
      const promptTokens = usage?.prompt_tokens
      const completionTokens = usage?.completion_tokens
      exchange.usage = usage
      exchange.costUsd = estimateCost(
        options.profile,
        promptTokens ?? 0,
        completionTokens ?? 0,
      ).totalUsd

      options.onExchangeEnd?.(exchange)

      const result: SummaryGeneratorOutput = {
        text,
        mode: 'llm',
        requests: 1,
        usageReported: typeof promptTokens === 'number',
      }
      if (typeof promptTokens === 'number') result.promptTokens = promptTokens
      if (typeof completionTokens === 'number') result.completionTokens = completionTokens
      if (typeof exchange.costUsd === 'number') result.costUsd = exchange.costUsd
      return result
    } catch (error) {
      exchange.durationMs = Math.round(performance.now() - started)
      exchange.error = error instanceof Error ? error.message : String(error)
      options.onExchangeEnd?.(exchange)
      throw error
    }
  }
}
