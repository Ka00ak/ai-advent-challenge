import { buildEndpoint, createChatCompletion } from '../api/llm'
import { estimateCost, type ModelProfile } from '../tokens'
import type { ChatMessage, LlmExchange, LlmRequest, LlmSettings, TokenUsage } from '../types'
import {
  FACT_CATEGORIES,
  isFactCategory,
  type FactCategory,
  type FactExtractionOutput,
  type FactExtractor,
  type StickyFact,
} from './facts'

/**
 * LLM-извлечение facts. Один запрос на каждое новое сообщение пользователя.
 *
 * Это главный источник «сложности» стратегии Sticky Facts: sliding window
 * вообще не требует модели, summary вызывает её порциями, а facts — на каждую
 * реплику. Зато facts не размываются шумом и их можно показать пользователю.
 */

export const FACTS_SYSTEM_PROMPT = `Ты обновляешь долговременную память агента в формате ключ → значение.

Задача: из нового сообщения пользователя извлечь устойчивые факты и вернуть ТОЛЬКО их в JSON-массиве.

Правила:
1. Извлекай цель, имена, сроки, бюджеты, ограничения, предпочтения, принятые решения, договорённости и риски.
2. Ключ — короткий стабильный идентификатор на латинице: project.name, project.deadline, budget.total, constraint.infra, decision.database, agreement.meeting, preference.format.
3. Значение — дословная формулировка из сообщения. Числа, даты и имена не переписывай.
4. Если в сообщении нет новых устойчивых фактов, верни [].
5. Не добавляй факты, которых не было в сообщении, и не пересказывай весь диалог.
6. Ответ — только валидный JSON без markdown и пояснений:
[{"key":"project.deadline","value":"15 декабря 2026","category":"срок"}]
7. Допустимые category: ${FACT_CATEGORIES.join(', ')}.`

export function buildFactsUserPrompt(
  previousFacts: StickyFact[],
  message: ChatMessage,
): string {
  const previous = previousFacts.length
    ? previousFacts.map(fact => `- ${fact.key}: ${fact.value}`).join('\n')
    : '(пусто)'
  return [
    `Уже известные факты:\n${previous}`,
    `Новое сообщение пользователя:\n${message.content ?? ''}`,
    'Верни JSON-массив только с новыми или изменившимися фактами.',
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

interface RawFact {
  key?: unknown
  value?: unknown
  category?: unknown
}

/** Модель может вернуть ```json … ``` или текст вокруг массива — чиним. */
export function parseFactsJson(raw: string): StickyFact[] {
  const text = raw.trim()
  if (!text) return []

  let candidate = text
  const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i)
  if (fenced?.[1]) candidate = fenced[1].trim()

  let parsed: unknown
  try {
    parsed = JSON.parse(candidate)
  } catch {
    const start = candidate.indexOf('[')
    const end = candidate.lastIndexOf(']')
    if (start < 0 || end <= start) throw new Error('Модель вернула не JSON-массив фактов')
    parsed = JSON.parse(candidate.slice(start, end + 1))
  }

  if (!Array.isArray(parsed)) throw new Error('Ожидался JSON-массив фактов')

  const now = new Date().toISOString()
  return parsed
    .map((item): StickyFact | null => {
      if (!item || typeof item !== 'object') return null
      const rawFact = item as RawFact
      const key = typeof rawFact.key === 'string' ? rawFact.key.trim() : ''
      const value = typeof rawFact.value === 'string' ? rawFact.value.trim() : ''
      if (!key || !value) return null
      const category: FactCategory = isFactCategory(rawFact.category)
        ? rawFact.category
        : 'другое'
      return { key, value, category, updatedAt: now }
    })
    .filter((fact): fact is StickyFact => fact !== null)
}

export interface FactExtractorOptions {
  settings: LlmSettings
  profile: ModelProfile
  signal?: AbortSignal
  maxFactsTokens?: number
  onExchangeStart?: (exchange: LlmExchange) => void
  onExchangeEnd?: (exchange: LlmExchange) => void
}

export function createLlmFactExtractor(
  options: FactExtractorOptions,
): FactExtractor {
  return async ({ previousFacts, message }) => {
    const request: LlmRequest = {
      model: options.settings.model,
      messages: [
        { role: 'system', content: FACTS_SYSTEM_PROMPT },
        { role: 'user', content: buildFactsUserPrompt(previousFacts, message) },
      ],
      temperature: 0,
      max_tokens: options.maxFactsTokens ?? 380,
    }

    const exchange: LlmExchange = {
      id: makeId(),
      kind: 'facts',
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

      const rawText = contentToText(response.choices?.[0]?.message?.content)
      const facts = parseFactsJson(rawText)
      const usage: TokenUsage | undefined = response.usage
      exchange.usage = usage
      exchange.costUsd = estimateCost(
        options.profile,
        usage?.prompt_tokens ?? 0,
        usage?.completion_tokens ?? 0,
      ).totalUsd
      options.onExchangeEnd?.(exchange)

      const result: FactExtractionOutput = {
        facts,
        mode: 'llm',
        requests: 1,
        usageReported: typeof usage?.prompt_tokens === 'number',
      }
      if (typeof usage?.prompt_tokens === 'number') result.promptTokens = usage.prompt_tokens
      if (typeof usage?.completion_tokens === 'number') {
        result.completionTokens = usage.completion_tokens
      }
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
