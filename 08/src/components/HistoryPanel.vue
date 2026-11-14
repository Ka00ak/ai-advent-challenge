<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  CONVERSATION_STORAGE_KEY,
  conversationStats,
  formatBytes,
  type StoredConversation,
} from '../storage/conversation'
import type { ChatMessage } from '../types'
import type { SaveState } from '../ui-types'

const props = defineProps<{
  conversation: StoredConversation | null
  messages: ChatMessage[]
  saveState: SaveState
}>()

const emit = defineEmits<{
  export: []
  import: [payload: { filename: string; text: string }]
  clear: []
}>()

const fileInput = ref<HTMLInputElement | null>(null)

const stats = computed(() => conversationStats(props.messages))
const snapshot = computed(() =>
  JSON.stringify(
    {
      version: props.conversation?.version ?? 1,
      id: props.conversation?.id ?? 'not-saved-yet',
      createdAt: props.conversation?.createdAt ?? null,
      updatedAt: props.conversation?.updatedAt ?? null,
      messages: props.messages,
    },
    null,
    2,
  ),
)
const snapshotBytes = computed(() => new TextEncoder().encode(snapshot.value).length)

function formatTimestamp(iso?: string): string {
  if (!iso) return '—'
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleString('ru-RU')
}

function pickFile(): void {
  fileInput.value?.click()
}

async function onFileChange(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    const text = await file.text()
    emit('import', { filename: file.name, text })
  } finally {
    input.value = ''
  }
}
</script>

<template>
  <div class="history">
    <section class="field-group">
      <h2>Сохранение контекста</h2>

      <div class="storage-card" :class="props.saveState.state">
        <div class="storage-line">
          <span class="storage-dot" />
          <strong v-if="props.saveState.state === 'error'">Ошибка сохранения</strong>
          <strong v-else>Автосохранение включено</strong>
          <span class="muted">{{ formatBytes(snapshotBytes) }}</span>
        </div>
        <div class="storage-line muted">
          <span>JSON · localStorage</span>
          <code>{{ CONVERSATION_STORAGE_KEY }}</code>
        </div>
        <div class="storage-line muted">
          <span>Последнее сохранение</span>
          <span>{{ formatTimestamp(props.saveState.savedAt ?? props.conversation?.updatedAt) }}</span>
        </div>
        <p v-if="props.saveState.error" class="storage-error">{{ props.saveState.error }}</p>
      </div>

      <div class="history-stats">
        <div class="stat">
          <span class="stat-value">{{ stats.total }}</span>
          <span class="stat-label">messages</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ stats.user }}</span>
          <span class="stat-label">user</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ stats.assistant }}</span>
          <span class="stat-label">assistant</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ stats.tool }}</span>
          <span class="stat-label">tool</span>
        </div>
        <div class="stat">
          <span class="stat-value">{{ stats.toolCalls }}</span>
          <span class="stat-label">tool_calls</span>
        </div>
      </div>

      <p class="hint">
        Сохраняется весь массив <code>messages</code>: реплики, <code>tool_calls</code>
        и ответы инструментов. При следующем запуске он загружается обратно и уходит в
        модель как обычная история диалога.
      </p>

      <div class="history-actions">
        <button class="btn ghost small" :disabled="!props.messages.length" @click="emit('export')">
          Скачать JSON
        </button>
        <button class="btn ghost small" @click="pickFile">Загрузить JSON</button>
        <button class="btn danger small" :disabled="!props.messages.length" @click="emit('clear')">
          Очистить
        </button>
      </div>
      <input
        ref="fileInput"
        class="file-input"
        type="file"
        accept="application/json,.json"
        @change="onFileChange"
      />
    </section>

    <section class="field-group">
      <h2>messages · JSON</h2>
      <pre class="history-json">{{ snapshot }}</pre>
    </section>
  </div>
</template>
