<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  SITE_STATUS_LABELS,
  STEP_KEYS,
  STEP_LABELS,
  gtmSnippets,
  type HealthStatus,
  type SiteDetail,
} from '../../shared/sites'
import {
  checkDeploy,
  deleteSite,
  errorMessage,
  fetchSite,
  healthCheckSite,
  resumeSite,
} from '@/api'
import { formatTime } from '@/format'

const POLL_INTERVAL_MS = 2000

const props = defineProps<{ siteId: number }>()
const emit = defineEmits<{ changed: []; deleted: [] }>()

const site = ref<SiteDetail | null>(null)
const loadError = ref('')
const actionError = ref('')
const acting = ref(false)
const deleting = ref(false)
const healthChecking = ref(false)
const copied = ref<string | null>(null)
let pollTimer: ReturnType<typeof setTimeout> | undefined

const HEALTH_STATUS_LABELS: Record<HealthStatus, string> = {
  healthy: '健康',
  unhealthy: '异常',
  unknown: '未知',
}

const snippets = computed(() =>
  site.value?.outputs.gtmPublicId ? gtmSnippets(site.value.outputs.gtmPublicId) : null,
)

const outputTiles = computed(() => {
  const outputs = site.value?.outputs ?? {}
  return [
    { label: 'GTM ID', value: outputs.gtmPublicId },
    { label: 'GA4 衡量 ID', value: outputs.measurementId },
    { label: 'GA4 属性 ID', value: outputs.gaPropertyId },
    { label: 'GA4 数据流 ID', value: outputs.gaDataStreamId },
    { label: 'GTM 容器 ID', value: outputs.gtmContainerId },
    { label: 'GTM 发布版本', value: outputs.gtmVersionId },
  ].filter((tile): tile is { label: string; value: string } => Boolean(tile.value))
})

const healthStatus = computed(() => site.value?.outputs.healthStatus)
const healthItems = computed(() => site.value?.outputs.healthItems ?? [])
const healthCheckedAt = computed(() => site.value?.outputs.healthCheckedAt)

const doneCount = computed(
  () => site.value?.steps.filter((step) => step.status === 'done').length ?? 0,
)

async function load() {
  clearTimeout(pollTimer)
  const id = props.siteId
  try {
    const data = await fetchSite(id)
    if (id !== props.siteId) return
    const statusChanged = site.value?.status !== data.status
    site.value = data
    loadError.value = ''
    if (statusChanged) emit('changed')
  } catch (err) {
    loadError.value = `加载失败：${errorMessage(err)}`
  }
  if (site.value?.status === 'running') pollTimer = setTimeout(load, POLL_INTERVAL_MS)
}

watch(
  () => props.siteId,
  () => {
    site.value = null
    actionError.value = ''
    load()
  },
  { immediate: true },
)

onBeforeUnmount(() => clearTimeout(pollTimer))

async function runAction(action: (id: number) => Promise<SiteDetail>, label: string) {
  actionError.value = ''
  acting.value = true
  try {
    site.value = await action(props.siteId)
    emit('changed')
    load()
  } catch (err) {
    actionError.value = `${label}失败：${errorMessage(err)}`
  } finally {
    acting.value = false
  }
}

async function copy(text: string, key: string) {
  await navigator.clipboard.writeText(text)
  copied.value = key
  setTimeout(() => {
    if (copied.value === key) copied.value = null
  }, 1500)
}

async function removeSite() {
  if (!site.value) return
  const ok = window.confirm(
    `确定删除站点「${site.value.name}」（${site.value.domain}）？\n仅删除本机任务记录，不会删除 Google 上已创建的资源。`,
  )
  if (!ok) return
  actionError.value = ''
  deleting.value = true
  try {
    await deleteSite(props.siteId)
    emit('deleted')
  } catch (err) {
    actionError.value = `删除失败：${errorMessage(err)}`
  } finally {
    deleting.value = false
  }
}

async function runHealthCheck() {
  actionError.value = ''
  healthChecking.value = true
  try {
    site.value = await healthCheckSite(props.siteId)
    emit('changed')
  } catch (err) {
    actionError.value = `健康检查失败：${errorMessage(err)}`
  } finally {
    healthChecking.value = false
  }
}
</script>

<template>
  <section class="card detail">
    <div v-if="loadError" class="card-body">
      <div class="alert alert-danger">{{ loadError }}</div>
    </div>

    <div v-if="!site && !loadError" class="card-body loading text-muted">
      <span class="spinner"></span> 加载中…
    </div>

    <template v-if="site">
      <header class="card-header head">
        <div class="head-main">
          <div class="head-title">
            <h2>{{ site.name }}</h2>
            <span class="badge" :class="`badge-${site.status}`">
              {{ SITE_STATUS_LABELS[site.status] }}
            </span>
          </div>
          <p class="card-subtitle">
            <a :href="`https://${site.domain}/`" target="_blank" rel="noopener">
              https://{{ site.domain }}/
            </a>
            · sitemap {{ site.sitemapUrl }}
          </p>
        </div>
        <div class="head-aside">
          <div class="progress" :title="`已完成 ${doneCount}/${STEP_KEYS.length} 步`">
            <span class="progress-text">{{ doneCount }}/{{ STEP_KEYS.length }}</span>
            <span class="progress-bar">
              <span
                class="progress-fill"
                :class="`fill-${site.status}`"
                :style="{ width: `${(doneCount / STEP_KEYS.length) * 100}%` }"
              ></span>
            </span>
          </div>
          <div class="head-actions">
            <button
              type="button"
              class="btn btn-sm"
              :disabled="acting || deleting || healthChecking || site.status === 'running'"
              :title="site.status === 'running' ? '执行中无法检查' : '复核 GTM / GA4 / GSC'"
              @click="runHealthCheck"
            >
              <span v-if="healthChecking" class="spinner"></span>
              健康检查
            </button>
            <button
              type="button"
              class="btn btn-sm btn-danger"
              :disabled="acting || deleting || healthChecking || site.status === 'running'"
              :title="site.status === 'running' ? '执行中无法删除' : '删除本机任务'"
              @click="removeSite"
            >
              <span v-if="deleting" class="spinner"></span>
              删除站点
            </button>
          </div>
        </div>
      </header>

      <div class="card-body body">
        <div v-if="site.status === 'failed'" class="alert alert-danger">
          <span class="alert-title">执行失败：{{ STEP_LABELS[site.currentStep] }}</span>
          <span>{{ site.error }}</span>
          <div class="alert-actions">
            <button
              type="button"
              class="btn btn-primary btn-sm"
              :disabled="acting"
              @click="runAction(resumeSite, '继续')"
            >
              <span v-if="acting" class="spinner"></span>
              从失败处继续
            </button>
            <span class="text-muted">已完成的步骤不会重复执行</span>
          </div>
          <span v-if="actionError">{{ actionError }}</span>
        </div>

        <div v-if="site.status === 'waiting_deploy'" class="alert alert-warning">
          <span class="alert-title">还差一步：把 GTM 装到网站</span>
          <span>
            Google 侧已就绪。复制下方两段代码贴到每个页面，再点「检查部署」。检测
            <code>{{ site.outputs.gtmPublicId }}</code>
            是否出现在首页原始 HTML 中（前端动态插入检测不到）。
          </span>
          <div class="alert-actions">
            <button
              type="button"
              class="btn btn-primary btn-sm"
              :disabled="acting"
              @click="runAction(checkDeploy, '检查')"
            >
              <span v-if="acting" class="spinner"></span>
              检查部署
            </button>
          </div>
          <span v-if="actionError">{{ actionError }}</span>
        </div>

        <div v-if="site.status === 'done'" class="alert alert-success">
          <span class="alert-title">接入完成</span>
          <span>GTM、GA4、Search Console 已打通。可随时做健康检查确认是否仍正常。</span>
        </div>

        <section v-if="snippets" class="install" :class="{ 'install-focus': site.status === 'waiting_deploy' }">
          <div class="install-head">
            <div>
              <h3 class="section-title">网站只需安装 GTM</h3>
              <p class="install-lead text-muted">
                只要部署
                <strong class="mono">{{ site.outputs.gtmPublicId }}</strong>
                。GA4 与 GSC 验证会随 GTM 自动完成，不必另贴代码。
              </p>
            </div>
          </div>
          <div class="snippets">
            <div class="snippet">
              <div class="snippet-bar">
                <span class="snippet-step">1</span>
                <span>放到 <code>&lt;head&gt;</code> 尽量靠前</span>
                <button type="button" class="btn btn-sm" @click="copy(snippets.head, 'head')">
                  {{ copied === 'head' ? '已复制' : '复制' }}
                </button>
              </div>
              <pre class="selectable">{{ snippets.head }}</pre>
            </div>
            <div class="snippet">
              <div class="snippet-bar">
                <span class="snippet-step">2</span>
                <span>放到 <code>&lt;body&gt;</code> 开始标签后</span>
                <button type="button" class="btn btn-sm" @click="copy(snippets.body, 'body')">
                  {{ copied === 'body' ? '已复制' : '复制' }}
                </button>
              </div>
              <pre class="selectable">{{ snippets.body }}</pre>
            </div>
          </div>
        </section>

        <section v-if="outputTiles.length" class="ids">
          <h3 class="section-title">产出 ID</h3>
          <div class="tiles">
            <button
              v-for="tile in outputTiles"
              :key="tile.label"
              type="button"
              class="tile"
              title="点击复制"
              @click="copy(tile.value, tile.label)"
            >
              <span class="tile-label">{{ tile.label }}</span>
              <span class="tile-value mono">{{ tile.value }}</span>
              <span class="tile-copy">{{ copied === tile.label ? '已复制' : '复制' }}</span>
            </button>
          </div>
        </section>

        <section class="tracking-summary">
          <h3 class="section-title">监听预设</h3>
          <div class="chip-row">
            <span class="chip" :class="site.outputs.trackClick ? 'chip-on' : 'chip-off'">点击</span>
            <span class="chip" :class="site.outputs.trackOutbound ? 'chip-on' : 'chip-off'">出站</span>
            <span class="chip" :class="site.outputs.trackForm ? 'chip-on' : 'chip-off'">表单</span>
          </div>
        </section>

        <section class="health">
          <div class="health-head">
            <div>
              <h3 class="section-title">健康检查</h3>
              <p class="text-muted health-meta">
                <template v-if="healthCheckedAt">上次 {{ formatTime(healthCheckedAt) }}</template>
                <template v-else>开通后可随时复核页面、GTM、GA4、GSC</template>
              </p>
            </div>
            <span
              v-if="healthStatus"
              class="badge"
              :class="`badge-health-${healthStatus}`"
            >
              {{ HEALTH_STATUS_LABELS[healthStatus] }}
            </span>
          </div>
          <div v-if="healthItems.length" class="health-grid">
            <div
              v-for="item in healthItems"
              :key="item.key"
              class="health-card"
              :class="`health-${item.status}`"
            >
              <div class="health-card-top">
                <span class="health-item-label">{{ item.label }}</span>
                <span class="health-item-status">
                  {{ item.status === 'pass' ? '通过' : item.status === 'fail' ? '失败' : '跳过' }}
                </span>
              </div>
              <p class="health-item-msg">{{ item.message }}</p>
            </div>
          </div>
          <div
            v-if="actionError && site.status !== 'failed' && site.status !== 'waiting_deploy'"
            class="alert alert-danger"
          >
            {{ actionError }}
          </div>
        </section>
      </div>
    </template>
  </section>
</template>

<style scoped>
.detail {
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
  background: var(--surface);
}

.detail :deep(.card-header) {
  flex: none;
  padding: 10px 14px 8px;
}

.loading {
  display: flex;
  align-items: center;
  gap: 8px;
}

.head {
  align-items: flex-start;
}

.head-main {
  min-width: 0;
}

.head-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.head-title h2 {
  font-size: 16px;
  font-weight: 600;
}

.head-aside {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
  flex: none;
}

.head-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 6px;
}

.card-subtitle a {
  color: var(--primary);
  text-decoration: none;
}

.card-subtitle a:hover {
  text-decoration: underline;
}

.progress {
  flex: none;
  display: grid;
  gap: 4px;
  justify-items: end;
  width: 120px;
}

.progress-text {
  font-size: 11.5px;
  color: var(--text-muted);
}

.progress-bar {
  width: 100%;
  height: 5px;
  border-radius: 999px;
  background: var(--border);
  overflow: hidden;
}

.progress-fill {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--primary);
  transition: width 0.3s;
}

.fill-done {
  background: var(--success);
}

.fill-failed {
  background: var(--danger);
}

.fill-waiting_deploy {
  background: var(--warning);
}

.body {
  flex: 1;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  display: grid;
  gap: 12px;
  align-content: start;
  padding: 10px 14px 12px;
}

.install-head {
  margin-bottom: 8px;
}

.install-lead {
  margin-top: 2px;
  font-size: 12.5px;
  line-height: 1.4;
}

.install-lead strong {
  color: var(--text);
  font-weight: 650;
}

.install-focus {
  padding: 10px 12px;
  border: 1px solid #efd7a0;
  border-radius: var(--radius);
  background: linear-gradient(180deg, #fffaf0 0%, #fff 48%);
}

.install-focus .section-title {
  color: var(--warning);
}

.snippet-step {
  display: inline-grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 11px;
  font-weight: 700;
}

.chip-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.tiles {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px;
}

.tile {
  position: relative;
  display: grid;
  gap: 1px;
  padding: 7px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface-muted);
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
  transition: border-color 0.15s;
}

.tile:hover {
  border-color: var(--primary);
}

.tile-label {
  font-size: 12px;
  color: var(--text-muted);
}

.tile-value {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tile-copy {
  position: absolute;
  top: 8px;
  right: 10px;
  font-size: 11px;
  color: var(--primary);
  opacity: 0;
  transition: opacity 0.15s;
}

.tile:hover .tile-copy {
  opacity: 1;
}

.section-title {
  margin-bottom: 6px;
  font-size: 11.5px;
  font-weight: 650;
  color: var(--text-subtle);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.health-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  margin-bottom: 6px;
}

.health-head .section-title {
  margin-bottom: 1px;
}

.health-meta {
  margin: 0;
  font-size: 12px;
}

.health-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.health-card {
  display: grid;
  gap: 4px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--surface);
}

.health-card-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.health-item-label {
  font-weight: 600;
  font-size: 13px;
}

.health-item-status {
  font-size: 12px;
  font-weight: 600;
}

.health-pass {
  border-color: #b8dfc8;
  background: #f6fbf8;
}

.health-pass .health-item-status {
  color: var(--success);
}

.health-fail {
  border-color: #f0c4c0;
  background: #fff8f7;
}

.health-fail .health-item-status {
  color: var(--danger);
}

.health-skip .health-item-status {
  color: var(--text-muted);
}

.health-item-msg {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.35;
  word-break: break-word;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

@media (max-width: 720px) {
  .health-grid {
    grid-template-columns: 1fr;
  }
}

.snippets {
  display: grid;
  gap: 8px;
}

.snippet {
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.snippet-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  background: var(--surface-muted);
  border-bottom: 1px solid var(--border);
  font-size: 12.5px;
}

.snippet-bar .btn {
  margin-left: auto;
}

pre {
  margin: 0;
  padding: 8px 10px;
  max-height: clamp(64px, 11vh, 120px);
  background: #111827;
  color: #e5e7eb;
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.45;
  overflow: auto;
  user-select: text;
}

@media (min-height: 900px) {
  .detail :deep(.card-header) {
    padding: 12px 16px 10px;
  }

  .head-title h2 {
    font-size: 17px;
  }

  .body {
    gap: 14px;
    padding: 12px 16px 14px;
  }

  pre {
    max-height: clamp(80px, 14vh, 160px);
    padding: 10px 12px;
  }
}

@media (max-height: 760px) {
  .body {
    gap: 8px;
    padding: 8px 12px 10px;
  }

  .install-lead {
    display: none;
  }

  .health-meta {
    display: none;
  }

  pre {
    max-height: 56px;
  }
}
</style>
