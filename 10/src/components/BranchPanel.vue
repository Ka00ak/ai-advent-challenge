<script setup lang="ts">
import { computed } from 'vue'
import {
  BRANCHES_STORAGE_KEY,
  type BranchState,
  type DialogBranch,
} from '../memory/branches'

const props = defineProps<{
  state: BranchState | null
  messageCount: number
  busy: boolean
}>()

const emit = defineEmits<{
  'save-checkpoint': []
  'create-branches': []
  switch: [branchId: string]
  clear: []
}>()

const activeId = computed(() => props.state?.activeBranchId ?? null)
const branches = computed(() => props.state?.branches ?? [])
const checkpoint = computed(() => props.state?.checkpoint ?? null)

function formatTime(iso?: string): string {
  if (!iso) return '—'
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleString('ru-RU')
}

function factsCount(branch: DialogBranch): number {
  return branch.facts?.facts.length ?? 0
}

function summaryCovered(branch: DialogBranch): number {
  return branch.summary?.coveredMessages ?? 0
}
</script>

<template>
  <div class="branches">
    <section class="field-group">
      <h2>Ветвление диалога</h2>

      <p class="hint">
        Checkpoint — снимок сообщений, сводки и facts в текущей точке. От него
        можно создать две ветки и продолжать каждую независимо: у ветки свои
        <code>messages</code>, своя память и свой активный контекст.
      </p>

      <div class="checkpoint-card" :class="{ empty: !checkpoint }">
        <div v-if="checkpoint" class="checkpoint-body">
          <div class="checkpoint-head">
            <strong>{{ checkpoint.label }}</strong>
            <span class="badge ok">checkpoint</span>
          </div>
          <div class="checkpoint-meta">
            <span>{{ checkpoint.messageIndex }} сообщений</span>
            <span>facts: {{ checkpoint.facts?.facts.length ?? 0 }}</span>
            <span>summary: {{ checkpoint.summary?.coveredMessages ?? 0 }}</span>
            <span>{{ formatTime(checkpoint.createdAt) }}</span>
          </div>
        </div>
        <div v-else>
          <strong>Checkpoint ещё не сохранён</strong>
          <p class="muted">
            Сейчас в диалоге {{ props.messageCount }} сообщений. Сохрани
            checkpoint — он станет общей точкой для двух веток.
          </p>
        </div>

        <div class="history-actions">
          <button
            type="button"
            class="btn primary small"
            :disabled="props.busy || !props.messageCount"
            @click="emit('save-checkpoint')"
          >
            Сохранить checkpoint
          </button>
          <button
            v-if="checkpoint"
            type="button"
            class="btn small"
            :disabled="props.busy"
            @click="emit('create-branches')"
          >
            Создать 2 ветки от checkpoint
          </button>
          <button
            v-if="props.state"
            type="button"
            class="btn danger small"
            :disabled="props.busy"
            @click="emit('clear')"
          >
            Удалить ветки
          </button>
        </div>
      </div>
    </section>

    <section class="field-group">
      <h2>Ветки · переключение</h2>

      <div v-if="branches.length" class="branch-list">
        <div
          v-for="branch in branches"
          :key="branch.id"
          class="branch-card"
          :class="{ active: branch.id === activeId }"
        >
          <div class="branch-head">
            <strong>{{ branch.name }}</strong>
            <span v-if="branch.id === activeId" class="badge ok">активна</span>
            <span v-else class="badge">ветка</span>
          </div>
          <div class="branch-meta">
            <span>{{ branch.messages.length }} сообщений</span>
            <span>facts: {{ factsCount(branch) }}</span>
            <span>summary: {{ summaryCovered(branch) }}</span>
            <span>обновлена {{ formatTime(branch.updatedAt) }}</span>
          </div>
          <div class="branch-preview">
            <span
              v-if="branch.messages.length"
              class="muted"
            >последняя реплика: «{{ branch.messages.at(-1)?.content?.slice(0, 120) }}»</span>
            <span v-else class="muted">пустая ветка</span>
          </div>
          <div class="history-actions">
            <button
              type="button"
              class="btn ghost small"
              :disabled="props.busy || branch.id === activeId"
              @click="emit('switch', branch.id)"
            >
              {{ branch.id === activeId ? 'Текущая' : 'Переключиться' }}
            </button>
          </div>
        </div>
      </div>
      <p v-else class="hint">
        Веток пока нет. Сохрани checkpoint и создай две ветки — они появятся
        здесь. Переключение меняет историю и память, с которыми работает агент.
      </p>

      <p class="hint">
        Ветки хранятся в <code>{{ BRANCHES_STORAGE_KEY }}</code>. В каждой ветке
        своя копия сообщений и памяти: продолжение одной не меняет другую.
      </p>
    </section>
  </div>
</template>
