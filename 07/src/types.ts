/** Общие типы для агента, инструментов и журнала API-запросов. */

export type ChatRole = 'system' | 'user' | 'assistant' | 'tool'

export interface ToolCall {
  id: string
  type: 'function'
  function: {
    name: string
    arguments: string
  }
}

export interface ChatMessage {
  role: ChatRole
  content: string | null
  tool_calls?: ToolCall[]
  tool_call_id?: string
  name?: string
}

export interface JsonSchema {
  type: string
  description?: string
  properties?: Record<string, JsonSchema>
  required?: string[]
  enum?: string[]
  items?: JsonSchema
  default?: unknown
  minimum?: number
  maximum?: number
  additionalProperties?: boolean
}

export interface ToolSchema {
  type: 'function'
  function: {
    name: string
    description: string
    parameters: JsonSchema
  }
}

export interface ToolSummary {
  name: string
  description: string
}

/** Инструмент агента: схема для модели + обычная функция-исполнитель. */
export interface AgentTool<Args = Record<string, unknown>, Result = unknown> {
  name: string
  description: string
  parameters: JsonSchema
  execute(args: Args): Result | Promise<Result>
}

export interface TokenUsage {
  prompt_tokens?: number
  completion_tokens?: number
  total_tokens?: number
}

/** Один запрос к LLM и полученный на него ответ (для журнала и отладки). */
export interface LlmExchange {
  id: string
  url: string
  startedAt: string
  durationMs: number
  request: LlmRequest
  response?: unknown
  message?: ChatMessage
  usage?: TokenUsage
  error?: string
}

export interface LlmRequest {
  model: string
  messages: ChatMessage[]
  tools?: ToolSchema[]
  tool_choice?: 'auto' | 'none'
  temperature?: number
  max_tokens?: number
}

/** Результат выполнения одного вызова инструмента. */
export interface ToolInvocation {
  id: string
  name: string
  arguments: string
  startedAt: string
  durationMs: number
  ok: boolean
  result?: unknown
  error?: string
}

export interface AgentStep {
  iteration: number
  exchange: LlmExchange
  toolResults: ToolInvocation[]
}

export type AgentStopReason = 'final' | 'max-iterations' | 'cancelled'

export interface AgentRunResult {
  finalText: string
  steps: AgentStep[]
  messages: ChatMessage[]
  stoppedReason: AgentStopReason
}

export interface LlmSettings {
  baseUrl: string
  apiKey: string
  model: string
  temperature: number
  maxTokens: number | null
  maxIterations: number
}
