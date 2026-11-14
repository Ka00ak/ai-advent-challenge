export type ProviderId = 'deepseek' | 'moonshot' | 'custom'

export interface ProviderPreset {
  id: ProviderId
  label: string
  baseUrl: string
  model: string
  models: string[]
  keyHint: string
}

export const PROVIDERS: ProviderPreset[] = [
  {
    id: 'deepseek',
    label: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com',
    model: 'deepseek-flash',
    models: ['deepseek-flash', 'deepseek-chat', 'deepseek-reasoner'],
    keyHint: 'Ключ DeepSeek (DEEPSEEK_API_KEY)',
  },
  {
    id: 'moonshot',
    label: 'Moonshot / Kimi',
    baseUrl: 'https://api.moonshot.ai/v1',
    model: 'kimi-k3',
    models: ['kimi-k3', 'kimi-k3.1-pro'],
    keyHint: 'Ключ Moonshot (KIMI_API_KEY)',
  },
  {
    id: 'custom',
    label: 'Другой OpenAI-совместимый',
    baseUrl: '',
    model: '',
    models: [],
    keyHint: 'API-ключ провайдера',
  },
]

export interface AppSettings {
  providerId: ProviderId
  baseUrl: string
  model: string
  apiKey: string
  temperature: number
  maxTokens: number | null
  maxIterations: number
  rememberKey: boolean
}

export const DEFAULT_SETTINGS: AppSettings = {
  providerId: 'deepseek',
  baseUrl: 'https://api.deepseek.com',
  model: 'deepseek-flash',
  apiKey: '',
  temperature: 0.2,
  maxTokens: null,
  maxIterations: 6,
  rememberKey: false,
}

const SETTINGS_KEY = 'lab07.settings.v1'
const API_KEY_STORAGE = 'lab07.apiKey.v1'

function canUseStorage(): boolean {
  try {
    return typeof localStorage !== 'undefined'
  } catch {
    return false
  }
}

export function loadStoredApiKey(): string {
  if (!canUseStorage()) return ''
  try {
    return localStorage.getItem(API_KEY_STORAGE) ?? ''
  } catch {
    return ''
  }
}

export function saveStoredApiKey(apiKey: string): void {
  if (!canUseStorage()) return
  try {
    localStorage.setItem(API_KEY_STORAGE, apiKey)
  } catch {
    // Приватный режим или переполненное хранилище — ключ просто не сохранится.
  }
}

export function clearStoredApiKey(): void {
  if (!canUseStorage()) return
  try {
    localStorage.removeItem(API_KEY_STORAGE)
  } catch {
    // ignore
  }
}

export function loadSettings(): AppSettings {
  if (!canUseStorage()) return { ...DEFAULT_SETTINGS }
  try {
    const raw = localStorage.getItem(SETTINGS_KEY)
    if (!raw) return { ...DEFAULT_SETTINGS }
    const parsed = JSON.parse(raw) as Partial<AppSettings>
    const rememberKey = parsed.rememberKey === true
    return {
      ...DEFAULT_SETTINGS,
      ...parsed,
      rememberKey,
      apiKey: rememberKey ? loadStoredApiKey() : '',
    }
  } catch {
    return { ...DEFAULT_SETTINGS }
  }
}

export function saveSettings(settings: AppSettings): void {
  if (!canUseStorage()) return
  try {
    // Ключ никогда не попадает в это хранилище.
    const payload: AppSettings = { ...settings, apiKey: '' }
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(payload))
  } catch {
    // ignore
  }
}
