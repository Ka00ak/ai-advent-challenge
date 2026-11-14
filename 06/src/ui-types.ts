import type { AgentStep } from './types'

export interface UiMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
  pending: boolean
  error?: string
  stopped?: string
  steps: AgentStep[]
}
