import type { ChatMessage, LlmRequest, TokenUsage } from '../types'

export interface ChatCompletionChoice {
  index?: number
  finish_reason?: string | null
  message?: ChatMessage
}

export interface ChatCompletionResponse {
  id?: string
  object?: string
  model?: string
  choices?: ChatCompletionChoice[]
  usage?: TokenUsage
  error?: {
    message?: string
    type?: string
    code?: string
  }
  [key: string]: unknown
}

export class LlmApiError extends Error {
  readonly status: number
  readonly body: unknown

  constructor(message: string, status: number, body: unknown) {
    super(message)
    this.name = 'LlmApiError'
    this.status = status
    this.body = body
  }
}

/** Убирает лишние слэши в конце, чтобы не получить двойной `//` в URL. */
export function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/+$/, '')
}

/** Поддерживает и `https://api.deepseek.com`, и полный URL до `/chat/completions`. */
export function buildEndpoint(baseUrl: string): string {
  const base = normalizeBaseUrl(baseUrl)
  if (!base) throw new Error('Не задан Base URL')
  if (/\/chat\/completions$/.test(base)) return base
  return `${base}/chat/completions`
}

function extractErrorMessage(payload: unknown, fallback: string): string {
  if (payload && typeof payload === 'object') {
    const error = (payload as { error?: unknown }).error
    if (error && typeof error === 'object') {
      const message = (error as { message?: unknown }).message
      if (typeof message === 'string' && message.trim()) return message
    }
    if (typeof error === 'string' && error.trim()) return error
    const message = (payload as { message?: unknown }).message
    if (typeof message === 'string' && message.trim()) return message
  }
  if (typeof payload === 'string' && payload.trim()) return payload.slice(0, 500)
  return fallback
}

/**
 * Минимальный клиент OpenAI-совместимого Chat Completions API.
 * Используется и DeepSeek, и Moonshot/Kimi, и любой другой совместимый провайдер.
 */
export async function createChatCompletion(
  baseUrl: string,
  apiKey: string,
  request: LlmRequest,
  signal?: AbortSignal,
): Promise<ChatCompletionResponse> {
  const key = apiKey.trim()
  if (!key) throw new Error('Не задан API-ключ')

  const response = await fetch(buildEndpoint(baseUrl), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${key}`,
    },
    body: JSON.stringify(request),
    signal,
  })

  const rawText = await response.text()
  let payload: unknown = null
  try {
    payload = rawText ? JSON.parse(rawText) : null
  } catch {
    payload = rawText
  }

  if (!response.ok) {
    const detail = extractErrorMessage(payload, response.statusText || 'без описания')
    throw new LlmApiError(`HTTP ${response.status}: ${detail}`, response.status, payload)
  }

  if (!payload || typeof payload !== 'object') {
    throw new Error('API вернул пустой или не-JSON ответ')
  }

  const result = payload as ChatCompletionResponse
  if (result.error) {
    throw new LlmApiError(
      extractErrorMessage(result, 'API вернул ошибку'),
      response.status,
      result,
    )
  }
  return result
}
