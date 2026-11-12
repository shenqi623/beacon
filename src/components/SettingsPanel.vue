<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import type {
  CredentialStatus,
  SaveConfigPayload,
  Settings,
  SettingsErrors,
} from '../../shared/types'
import {
  clearCredential,
  errorMessage,
  fetchSettings,
  openDataDir,
  startGoogleAuth,
  updateSettings,
} from '@/api'
import { isTauri } from '@/runtime'

const form = ref<Settings | null>(null)
const savedSnapshot = ref('')
const credentials = ref<CredentialStatus | null>(null)
const dataDir = ref('')
const clientIdInput = ref('')
const clientSecretInput = ref('')
const errors = ref<SettingsErrors>({})
const loadError = ref('')
const saving = ref(false)
const authorizing = ref(false)
const authPhase = ref('')
const saveResult = ref<{ ok: boolean; message: string } | null>(null)
const authMessage = ref<{ ok: boolean; message: string } | null>(null)

function snapshotOf(settings: Settings, clientId: string, clientSecret: string) {
  return JSON.stringify({
    settings,
    clientId: clientId.trim(),
    clientSecret: clientSecret.trim(),
  })
}

function markSaved(settings: Settings) {
  savedSnapshot.value = snapshotOf(settings, '', '')
  clientIdInput.value = ''
  clientSecretInput.value = ''
}

const dirty = computed(() => {
  if (!form.value || !savedSnapshot.value) return false
  return (
    snapshotOf(form.value, clientIdInput.value, clientSecretInput.value) !== savedSnapshot.value
  )
})

watch([clientIdInput, clientSecretInput], () => {
  if (saveResult.value?.ok) saveResult.value = null
})

onMounted(async () => {
  try {
    const data = await fetchSettings()
    form.value = data.settings
    credentials.value = data.credentials
    dataDir.value = data.dataDir ?? ''
    markSaved(data.settings)
  } catch (err) {
    loadError.value = `加载配置失败：${errorMessage(err)}`
  }
})

function setField<K extends keyof Settings>(key: K, value: Settings[K]) {
  if (!form.value) return
  form.value = { ...form.value, [key]: value }
  if (saveResult.value?.ok) saveResult.value = null
}

async function save(options?: { quiet?: boolean }): Promise<boolean> {
  if (!form.value) return false
  saving.value = true
  if (!options?.quiet) saveResult.value = null
  errors.value = {}
  try {
    const payload: SaveConfigPayload = {
      settings: form.value,
      clientId: clientIdInput.value.trim() || undefined,
      clientSecret: clientSecretInput.value.trim() || undefined,
    }
    const data = await updateSettings(payload)
    form.value = data.settings
    credentials.value = data.credentials
    dataDir.value = data.dataDir ?? dataDir.value
    markSaved(data.settings)
    if (!options?.quiet) {
      saveResult.value = { ok: true, message: '已保存' }
    }
    return true
  } catch (err) {
    saveResult.value = { ok: false, message: `保存失败：${errorMessage(err)}` }
    return false
  } finally {
    saving.value = false
  }
}

async function authorize() {
  authorizing.value = true
  authMessage.value = null
  authPhase.value = ''
  try {
    if (dirty.value) {
      authPhase.value = '正在保存配置…'
      const ok = await save({ quiet: true })
      if (!ok) {
        authMessage.value = {
          ok: false,
          message: '有未保存的更改，且自动保存失败。请先修正后点「保存配置」。',
        }
        return
      }
      saveResult.value = { ok: true, message: '已自动保存，正在授权…' }
    }
    authPhase.value = '等待浏览器授权…'
    const result = await startGoogleAuth()
    const data = await fetchSettings()
    credentials.value = data.credentials
    if (result.grantedScopesOk) {
      authMessage.value = { ok: true, message: result.message }
    } else {
      authMessage.value = {
        ok: false,
        message: `${result.message}；仍缺少权限：${result.missingScopes.join('、')}`,
      }
    }
  } catch (err) {
    authMessage.value = { ok: false, message: errorMessage(err) }
  } finally {
    authorizing.value = false
    authPhase.value = ''
  }
}

async function clearField(field: 'clientId' | 'clientSecret' | 'refreshToken') {
  try {
    const data = await clearCredential(field)
    credentials.value = data.credentials
    if (field === 'clientId') clientIdInput.value = ''
    if (field === 'clientSecret') clientSecretInput.value = ''
    if (form.value) markSaved(form.value)
    saveResult.value = { ok: true, message: '已清除' }
  } catch (err) {
    saveResult.value = { ok: false, message: errorMessage(err) }
  }
}

async function openDir() {
  try {
    await openDataDir()
  } catch (err) {
    saveResult.value = { ok: false, message: errorMessage(err) }
  }
}
</script>

<template>
  <div class="settings">
    <div v-if="loadError" class="alert alert-danger">{{ loadError }}</div>

    <div v-if="form && dirty" class="dirty-bar" role="status">
      <span>有未保存的更改。代理、账号等设置需保存后才会生效；点「授权登录」时会自动先保存。</span>
      <button type="button" class="btn btn-sm btn-primary" :disabled="saving" @click="save()">
        <span v-if="saving" class="spinner"></span>
        立即保存
      </button>
    </div>

    <form v-if="form" class="stack" @submit.prevent="save()">
      <section class="panel">
        <header class="panel-header">
          <div>
            <h2 class="panel-title">Google OAuth</h2>
            <p class="panel-subtitle">客户端类型请选「桌面应用」。Secret 与 Refresh Token 仅保存在本机。</p>
          </div>
        </header>
        <div class="panel-body stack-sm">
          <div class="grid-2">
            <label class="field">
              <span class="field-label">Client ID</span>
              <input
                v-model="clientIdInput"
                class="input selectable"
                :placeholder="
                  credentials?.clientId ? '已设置，留空表示不修改' : 'xxxx.apps.googleusercontent.com'
                "
                autocomplete="off"
              />
            </label>
            <label class="field">
              <span class="field-label">Client Secret</span>
              <input
                v-model="clientSecretInput"
                class="input selectable"
                type="password"
                :placeholder="
                  credentials?.clientSecret ? '已设置，留空表示不修改' : '输入 Client Secret'
                "
                autocomplete="new-password"
              />
            </label>
          </div>

          <ul v-if="credentials" class="status-list">
            <li>
              <span class="status-name">Client ID</span>
              <span class="status-right">
                <span class="badge" :class="credentials.clientId ? 'badge-done' : 'badge-failed'">
                  {{ credentials.clientId ? '已设置' : '未设置' }}
                </span>
                <button
                  v-if="isTauri() && credentials.clientId"
                  type="button"
                  class="btn btn-sm btn-ghost"
                  @click="clearField('clientId')"
                >
                  清除
                </button>
              </span>
            </li>
            <li>
              <span class="status-name">Client Secret</span>
              <span class="status-right">
                <span class="badge" :class="credentials.clientSecret ? 'badge-done' : 'badge-failed'">
                  {{ credentials.clientSecret ? '已设置' : '未设置' }}
                </span>
                <button
                  v-if="isTauri() && credentials.clientSecret"
                  type="button"
                  class="btn btn-sm btn-ghost"
                  @click="clearField('clientSecret')"
                >
                  清除
                </button>
              </span>
            </li>
            <li>
              <span class="status-name">Refresh Token</span>
              <span class="status-right">
                <span class="badge" :class="credentials.refreshToken ? 'badge-done' : 'badge-failed'">
                  {{ credentials.refreshToken ? '已设置' : '未设置' }}
                </span>
                <button
                  v-if="isTauri() && credentials.refreshToken"
                  type="button"
                  class="btn btn-sm btn-ghost"
                  @click="clearField('refreshToken')"
                >
                  清除
                </button>
              </span>
            </li>
          </ul>

          <div class="row">
            <button
              type="button"
              class="btn btn-primary"
              :disabled="!isTauri() || authorizing || saving"
              @click="authorize"
            >
              <span v-if="authorizing || (saving && authPhase)" class="spinner"></span>
              {{ authPhase || (authorizing ? '等待浏览器授权…' : '授权登录') }}
            </button>
            <span class="field-hint">
              {{
                isTauri()
                  ? dirty
                    ? '检测到未保存更改：点击后会先自动保存，再打开浏览器授权'
                    : '打开系统浏览器完成 Google 授权，Token 自动写入本机数据目录'
                  : '授权登录仅在桌面端可用，请运行 pnpm tauri:dev'
              }}
            </span>
          </div>
          <div
            v-if="authMessage"
            class="alert"
            :class="authMessage.ok ? 'alert-success' : 'alert-danger'"
          >
            {{ authMessage.message }}
          </div>
        </div>
      </section>

      <section class="panel">
        <header class="panel-header">
          <div>
            <h2 class="panel-title">网络代理</h2>
            <p class="panel-subtitle">访问 Google API 时使用本机 HTTP/HTTPS 代理。修改后请保存。</p>
          </div>
        </header>
        <div class="panel-body stack-sm">
          <label class="toggle">
            <input
              type="checkbox"
              :checked="form.proxyEnabled"
              @change="setField('proxyEnabled', ($event.target as HTMLInputElement).checked)"
            />
            <span>启用代理</span>
          </label>
          <label class="field">
            <span class="field-label">代理地址</span>
            <input
              class="input selectable"
              :class="{ invalid: errors.proxyUrl }"
              :disabled="!form.proxyEnabled"
              :value="form.proxyUrl"
              placeholder="http://127.0.0.1:7897"
              @input="setField('proxyUrl', ($event.target as HTMLInputElement).value)"
            />
            <span v-if="errors.proxyUrl" class="field-error">{{ errors.proxyUrl }}</span>
          </label>
        </div>
      </section>

      <section class="panel">
        <header class="panel-header">
          <div>
            <h2 class="panel-title">Google 账号</h2>
            <p class="panel-subtitle">预先在 Google 后台创建好的账号 ID。</p>
          </div>
        </header>
        <div class="panel-body">
          <div class="grid-2">
            <label class="field">
              <span class="field-label">GA 账号 ID</span>
              <input
                class="input selectable"
                :value="form.gaAccountId"
                placeholder="例如 123456789"
                @input="setField('gaAccountId', ($event.target as HTMLInputElement).value)"
              />
            </label>
            <label class="field">
              <span class="field-label">GTM 账号 ID</span>
              <input
                class="input selectable"
                :value="form.gtmAccountId"
                placeholder="例如 6001234567"
                @input="setField('gtmAccountId', ($event.target as HTMLInputElement).value)"
              />
            </label>
          </div>
        </div>
      </section>

      <section class="panel">
        <header class="panel-header">
          <div>
            <h2 class="panel-title">GA4 默认值</h2>
            <p class="panel-subtitle">新建属性时使用的时区与币种。</p>
          </div>
        </header>
        <div class="panel-body">
          <div class="grid-2">
            <label class="field">
              <span class="field-label">默认时区</span>
              <input
                class="input selectable"
                :value="form.timeZone"
                @input="setField('timeZone', ($event.target as HTMLInputElement).value)"
              />
            </label>
            <label class="field">
              <span class="field-label">默认币种</span>
              <input
                class="input selectable"
                :value="form.currencyCode"
                @input="setField('currencyCode', ($event.target as HTMLInputElement).value)"
              />
            </label>
          </div>
        </div>
      </section>

      <section class="panel">
        <header class="panel-header">
          <div>
            <h2 class="panel-title">执行节奏</h2>
            <p class="panel-subtitle">GTM 写操作限速与 GSC 验证重试策略。</p>
          </div>
        </header>
        <div class="panel-body">
          <div class="grid-3">
            <label class="field">
              <span class="field-label">GTM 写间隔（毫秒）</span>
              <input
                class="input selectable"
                type="number"
                :value="form.gtmWriteIntervalMs"
                @input="setField('gtmWriteIntervalMs', Number(($event.target as HTMLInputElement).value))"
              />
            </label>
            <label class="field">
              <span class="field-label">GSC 重试间隔（秒）</span>
              <input
                class="input selectable"
                type="number"
                :value="form.gscVerifyRetryIntervalSec"
                @input="
                  setField(
                    'gscVerifyRetryIntervalSec',
                    Number(($event.target as HTMLInputElement).value),
                  )
                "
              />
            </label>
            <label class="field">
              <span class="field-label">GSC 最多尝试次数</span>
              <input
                class="input selectable"
                type="number"
                :value="form.gscVerifyMaxAttempts"
                @input="
                  setField('gscVerifyMaxAttempts', Number(($event.target as HTMLInputElement).value))
                "
              />
            </label>
          </div>
        </div>
      </section>

      <div class="footer-bar" :class="{ dirty: dirty }">
        <div class="row">
          <button type="submit" class="btn btn-primary" :disabled="saving || !dirty">
            <span v-if="saving && !authorizing" class="spinner"></span>
            {{ dirty ? '保存配置' : '已是最新' }}
          </button>
          <button v-if="isTauri()" type="button" class="btn" @click="openDir">打开数据目录</button>
          <span v-if="dirty" class="dirty-hint">未保存</span>
          <span v-else-if="saveResult" :class="saveResult.ok ? 'ok-msg' : 'text-danger'">
            {{ saveResult.message }}
          </span>
        </div>
        <p v-if="dataDir" class="field-hint mono selectable path">{{ dataDir }}</p>
      </div>
    </form>
  </div>
</template>

<style scoped>
.settings {
  max-width: 860px;
}

.dirty-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
  padding: 10px 12px;
  border: 1px solid #efd7a0;
  border-radius: var(--radius-sm);
  background: var(--warning-soft);
  color: #6e4a00;
  font-size: 12.5px;
  line-height: 1.4;
  position: sticky;
  top: 0;
  z-index: 2;
}

.stack {
  display: grid;
  gap: 12px;
}

.stack-sm {
  display: grid;
  gap: 12px;
}

.grid-2 {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px 14px;
}

.grid-3 {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px 14px;
}

.toggle {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  width: fit-content;
  cursor: pointer;
}

.toggle input {
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
}

.status-list {
  list-style: none;
  margin: 0;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--surface-muted);
}

.status-list li {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  background: var(--surface);
}

.status-list li + li {
  border-top: 1px solid var(--border);
}

.status-name {
  font-size: 12.5px;
  font-weight: 500;
}

.status-right {
  display: flex;
  align-items: center;
  gap: 4px;
}

.row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
}

.footer-bar {
  display: grid;
  gap: 8px;
  padding: 4px 2px 0;
  position: sticky;
  bottom: 0;
  background: linear-gradient(to top, var(--bg) 70%, transparent);
  padding-top: 12px;
}

.footer-bar.dirty .btn-primary:not(:disabled) {
  box-shadow: 0 0 0 1px var(--accent);
}

.dirty-hint {
  color: var(--warning);
  font-size: 12.5px;
  font-weight: 600;
}

.ok-msg {
  color: var(--success);
  font-size: 12.5px;
  font-weight: 500;
}

.path {
  word-break: break-all;
}

@media (max-width: 720px) {
  .grid-2,
  .grid-3 {
    grid-template-columns: 1fr;
  }

  .dirty-bar {
    flex-direction: column;
    align-items: stretch;
  }
}
</style>
