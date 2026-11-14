<script setup lang="ts">
import { ref } from 'vue'
import type { LlmExchange } from '../types'

const props = defineProps<{
  exchanges: LlmExchange[]
}>()

const emit = defineEmits<{
  clear: []
  export: []
}>()

const copiedId = ref('')

function stringify(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value)
  } catch {
    return String(value)
  }
}

function formatTime(iso: string): string {
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleTimeString('ru-RU')
}

async function copyExchange(entry: LlmExchange): Promise<void> {
  if (!navigator.clipboard) return
  const text = stringify({
    url: entry.url,
    request: entry.request,
    response: entry.response,
    error: entry.error,
  })
  try {
    await navigator.clipboard.writeText(text)
    copiedId.value = entry.id
    window.setTimeout(() => {
      if (copiedId.value === entry.id) copiedId.value = ''
    }, 1500)
  } catch {
    copiedId.value = ''
  }
}
</script>

<template>
  <div class="log">
    <div class="log-toolbar">
      <span class="muted">{{ props.exchanges.length }} запросов</span>
      <div class="row">
        <button class="btn ghost small" :disabled="!props.exchanges.length" @click="emit('export')">
          Экспорт
        </button>
        <button class="btn ghost small" :disabled="!props.exchanges.length" @click="emit('clear')">
          Очистить
        </button>
      </div>
    </div>

    <div v-if="!props.exchanges.length" class="log-empty">
      Здесь появятся реальные HTTP-запросы к LLM: полный `messages`, схемы инструментов,
      ответ модели, tool_calls и usage. Открой любой запрос, чтобы посмотреть JSON.
    </div>

    <details v-for="entry in props.exchanges" :key="entry.id" class="log-entry">
      <summary>
        <span class="method">POST</span>
        <span class="log-model">{{ entry.request.model }}</span>
        <span class="muted">{{ entry.durationMs }} ms</span>
        <span v-if="entry.usage?.total_tokens" class="muted">
          {{ entry.usage.total_tokens }} tok
        </span>
        <span v-if="entry.error" class="badge bad">error</span>
        <span v-else-if="entry.response" class="badge ok">ok</span>
        <span v-else class="badge">…</span>
      </summary>

      <div class="log-body">
        <div class="log-row">
          <span class="muted">{{ formatTime(entry.startedAt) }}</span>
          <button class="icon-btn" @click.prevent="copyExchange(entry)">
            {{ copiedId === entry.id ? 'Скопировано' : 'Копировать' }}
          </button>
        </div>

        <div class="log-label">URL</div>
        <code class="log-url">{{ entry.url }}</code>

        <div class="log-label">Request · messages + tools</div>
        <pre class="log-json">{{ stringify(entry.request) }}</pre>

        <template v-if="entry.response">
          <div class="log-label">Response</div>
          <pre class="log-json">{{ stringify(entry.response) }}</pre>
        </template>

        <div v-if="entry.error" class="log-error">{{ entry.error }}</div>
      </div>
    </details>
  </div>
</template>
