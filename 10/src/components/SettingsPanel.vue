<script setup lang="ts">
import { computed, ref } from 'vue'
import { buildEndpoint } from '../api/llm'
import { PROVIDERS, type AppSettings, type ProviderId } from '../settings'
import { resolveModelProfile } from '../tokens'
import { listDefaultTools } from '../tools'

defineProps<{
  disabled?: boolean
}>()

const settings = defineModel<AppSettings>({ required: true })
const showKey = ref(false)
const tools = listDefaultTools()

const currentProvider = computed(
  () => PROVIDERS.find(provider => provider.id === settings.value.providerId) ?? PROVIDERS[0],
)

const modelSuggestions = computed(() => currentProvider.value?.models ?? [])
const keyHint = computed(() => currentProvider.value?.keyHint ?? 'API-ключ')

const endpoint = computed(() => {
  try {
    return buildEndpoint(settings.value.baseUrl)
  } catch {
    return '—'
  }
})

const maxTokens = computed<number | string>({
  get: () => settings.value.maxTokens ?? '',
  set: value => {
    const numeric = typeof value === 'number' ? value : Number(value)
    settings.value.maxTokens =
      Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : null
  },
})

const modelProfile = computed(() => resolveModelProfile(settings.value.model))
const contextLimit = computed<number | string>({
  get: () => settings.value.contextLimit ?? '',
  set: value => {
    const numeric = typeof value === 'number' ? value : Number(value)
    settings.value.contextLimit =
      Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : null
  },
})

const keepRecentMessages = computed<number | string>({
  get: () => settings.value.keepRecentMessages,
  set: value => {
    const numeric = typeof value === 'number' ? value : Number(value)
    settings.value.keepRecentMessages =
      Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : 8
  },
})

const summarizeEveryMessages = computed<number | string>({
  get: () => settings.value.summarizeEveryMessages,
  set: value => {
    const numeric = typeof value === 'number' ? value : Number(value)
    settings.value.summarizeEveryMessages =
      Number.isFinite(numeric) && numeric > 0 ? Math.floor(numeric) : 10
  },
})

function onProviderChange(event: Event): void {
  const select = event.target as HTMLSelectElement
  const id = select.value as ProviderId
  const preset = PROVIDERS.find(provider => provider.id === id)
  if (!preset) return
  settings.value.providerId = id
  if (preset.baseUrl) settings.value.baseUrl = preset.baseUrl
  if (preset.model) settings.value.model = preset.model
}
</script>

<template>
  <div class="settings">
    <section class="field-group">
      <h2>Подключение</h2>

      <div class="field">
        <label for="provider">Провайдер</label>
        <select
          id="provider"
          :value="settings.providerId"
          :disabled="disabled"
          @change="onProviderChange"
        >
          <option v-for="provider in PROVIDERS" :key="provider.id" :value="provider.id">
            {{ provider.label }}
          </option>
        </select>
      </div>

      <div class="field">
        <label for="base-url">Base URL</label>
        <input
          id="base-url"
          v-model.trim="settings.baseUrl"
          :disabled="disabled"
          spellcheck="false"
          placeholder="https://api.example.com/v1"
        />
      </div>

      <div class="field">
        <label for="model">Модель</label>
        <input
          id="model"
          v-model.trim="settings.model"
          :disabled="disabled"
          spellcheck="false"
          list="model-suggestions"
          placeholder="например deepseek-flash"
        />
        <datalist id="model-suggestions">
          <option v-for="suggestion in modelSuggestions" :key="suggestion" :value="suggestion" />
        </datalist>
      </div>

      <div class="field">
        <label for="api-key">API-ключ</label>
        <div class="key-row">
          <input
            id="api-key"
            v-model="settings.apiKey"
            :type="showKey ? 'text' : 'password'"
            :disabled="disabled"
            :placeholder="keyHint"
            autocomplete="off"
            spellcheck="false"
          />
          <button type="button" class="icon-btn" @click="showKey = !showKey">
            {{ showKey ? 'Скрыть' : 'Показать' }}
          </button>
        </div>
        <p v-if="!settings.apiKey.trim()" class="warn">
          Ключ не задан — запросы к API не отправятся.
        </p>
        <p v-else class="hint">Ключ уходит напрямую провайдеру, минуя серверы проекта.</p>

        <label class="checkbox">
          <input v-model="settings.rememberKey" type="checkbox" :disabled="disabled" />
          <span>Запомнить ключ в этом браузере (localStorage)</span>
        </label>
        <p class="hint">
          Включай только на своём компьютере: ключ будет доступен любому скрипту на этом домене.
        </p>
      </div>

      <div class="endpoint-box">
        <span class="endpoint-label">Endpoint</span>
        <code class="endpoint">POST {{ endpoint }}</code>
      </div>
    </section>

    <section class="field-group">
      <h2>Параметры генерации</h2>

      <div class="field">
        <label for="temperature">
          <span>Temperature</span>
          <span class="value">{{ settings.temperature.toFixed(1) }}</span>
        </label>
        <input
          id="temperature"
          v-model.number="settings.temperature"
          type="range"
          min="0"
          max="2"
          step="0.1"
          :disabled="disabled"
        />
      </div>

      <div class="field">
        <label for="max-tokens">Max tokens</label>
        <input
          id="max-tokens"
          v-model.number="maxTokens"
          type="number"
          min="1"
          :disabled="disabled"
          placeholder="по умолчанию у модели"
        />
      </div>

      <div class="field">
        <label for="context-limit">Лимит контекста, токенов</label>
        <input
          id="context-limit"
          v-model.number="contextLimit"
          type="number"
          min="1"
          step="1000"
          :disabled="disabled"
          :placeholder="`из профиля модели: ${modelProfile.contextLimit.toLocaleString('ru-RU')}`"
        />
        <p class="hint">
          Пусто — лимит берётся из профиля модели. Меньшее значение удобно, чтобы
          воспроизвести переполнение контекста без огромного диалога.
        </p>
      </div>

      <div class="field">
        <label for="max-iterations">Максимум итераций агента</label>
        <input
          id="max-iterations"
          v-model.number="settings.maxIterations"
          type="number"
          min="1"
          max="12"
          :disabled="disabled"
        />
        <p class="hint">Одна итерация — один запрос к модели. Защита от бесконечного tool-loop.</p>
      </div>
    </section>

    <section class="field-group">
      <h2>Управление контекстом</h2>

      <label class="checkbox">
        <input v-model="settings.compressionEnabled" type="checkbox" :disabled="disabled" />
        <span>Сжимать старую часть истории в сводку</span>
      </label>
      <p class="hint">
        Полная история остаётся в localStorage. В модель уходят только последние
        сообщения и отдельно сохранённая сводка.
      </p>

      <div class="field">
        <label for="keep-recent">Последних сообщений как есть</label>
        <input
          id="keep-recent"
          v-model.number="keepRecentMessages"
          type="number"
          min="1"
          max="200"
          :disabled="disabled || !settings.compressionEnabled"
        />
        <p class="hint">
          Окно выравнивается по границе хода пользователя, чтобы не разрывать
          пару assistant(tool_calls) → tool.
        </p>
      </div>

      <div class="field">
        <label for="summarize-every">Сжимать каждые N старых сообщений</label>
        <input
          id="summarize-every"
          v-model.number="summarizeEveryMessages"
          type="number"
          min="1"
          max="500"
          :disabled="disabled || !settings.compressionEnabled"
        />
        <p class="hint">
          Когда сверх последних {{ keepRecentMessages }} накапливается столько
          сообщений, агент обновляет сводку. {{ summarizeEveryMessages }} —
          компромисс между точностью и числом запросов на сжатие.
        </p>
      </div>

      <div class="field">
        <label for="summary-mode">Как строить сводку</label>
        <select
          id="summary-mode"
          v-model="settings.summaryMode"
          :disabled="disabled || !settings.compressionEnabled"
        >
          <option value="llm">LLM — отдельный запрос к модели</option>
          <option value="local">local — локальная выжимка без API</option>
        </select>
        <p class="hint">
          LLM лучше переформулирует, но тратит токены на сам запрос сжатия.
          Локальная выжимка бесплатна и сохраняет пользовательские формулировки
          дословно.
        </p>
      </div>
    </section>

    <section class="field-group">
      <h2>Инструменты агента</h2>
      <div class="tool-list">
        <div v-for="tool in tools" :key="tool.name" class="tool-chip">
          <code>{{ tool.name }}</code>
          <p>{{ tool.description }}</p>
        </div>
      </div>
    </section>
  </div>
</template>
