<script setup lang="ts">
import type { AgentStep, ToolInvocation } from '../types'
import type { UiMessage } from '../ui-types'

const props = defineProps<{
  message: UiMessage
  status?: string
}>()

function stringify(value: unknown): string {
  if (typeof value === 'string') return value
  try {
    return JSON.stringify(value, null, 2) ?? String(value)
  } catch {
    return String(value)
  }
}

function prettyArguments(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2)
  } catch {
    return raw
  }
}

function tokenLabel(step: AgentStep): string | null {
  const total = step.exchange.usage?.total_tokens
  return typeof total === 'number' ? `${total} токенов` : null
}

function toolStatus(tool: ToolInvocation): string {
  return tool.ok ? 'ok' : 'error'
}
</script>

<template>
  <article class="message" :class="props.message.role">
    <div class="avatar" aria-hidden="true">{{ props.message.role === 'user' ? 'Вы' : 'AI' }}</div>
    <div class="bubble">
      <div v-if="props.message.steps.length" class="steps">
        <div v-for="step in props.message.steps" :key="step.exchange.id" class="step">
          <div class="step-head">
            <span class="step-num">Итерация {{ step.iteration }}</span>
            <span class="muted">{{ step.exchange.durationMs }} ms</span>
            <span v-if="tokenLabel(step)" class="muted">{{ tokenLabel(step) }}</span>
            <span v-if="step.toolResults.length" class="step-tools">
              {{ step.toolResults.length }} tool{{ step.toolResults.length > 1 ? 's' : '' }}
            </span>
          </div>

          <div v-if="step.exchange.message?.content" class="step-text">
            {{ step.exchange.message.content }}
          </div>

          <div v-for="tool in step.toolResults" :key="tool.id" class="tool" :class="{ failed: !tool.ok }">
            <div class="tool-head">
              <code>{{ tool.name }}</code>
              <span class="tool-status" :class="tool.ok ? 'ok' : 'bad'">{{ toolStatus(tool) }}</span>
              <span class="muted">{{ tool.durationMs }} ms</span>
            </div>
            <div class="tool-label">arguments</div>
            <pre class="tool-json">{{ prettyArguments(tool.arguments) }}</pre>
            <template v-if="tool.ok">
              <div class="tool-label">result</div>
              <pre class="tool-json">{{ stringify(tool.result) }}</pre>
            </template>
            <div v-else class="tool-error">{{ tool.error }}</div>
          </div>

          <div v-if="step.exchange.error" class="step-error">
            {{ step.exchange.error }}
          </div>
        </div>
      </div>

      <div v-if="props.message.content" class="content">{{ props.message.content }}</div>

      <div v-else-if="props.message.pending" class="typing" aria-live="polite">
        <span class="dot" />
        <span class="dot" />
        <span class="dot" />
        <span>{{ props.status || 'Модель думает…' }}</span>
      </div>

      <div v-if="props.message.error" class="error">{{ props.message.error }}</div>
      <div v-if="props.message.stopped" class="stopped">{{ props.message.stopped }}</div>
    </div>
  </article>
</template>
