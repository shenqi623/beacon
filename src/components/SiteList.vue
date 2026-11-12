<script setup lang="ts">
import { reactive, watch } from 'vue'
import {
  SITE_STATUS_LABELS,
  STEP_KEYS,
  STEP_LABELS,
  STEP_STATUS_LABELS,
  type Site,
  type SiteStep,
} from '../../shared/sites'
import { errorMessage, fetchSite } from '@/api'
import { formatTime } from '@/format'

const props = defineProps<{ sites: Site[]; selectedId: number | null; loaded: boolean }>()
defineEmits<{ select: [id: number] }>()

const expanded = reactive<Record<number, boolean>>({})
const stepsById = reactive<Record<number, SiteStep[]>>({})
const loadingById = reactive<Record<number, boolean>>({})
const errorById = reactive<Record<number, string>>({})

const stepNumber = (site: Site) => STEP_KEYS.indexOf(site.currentStep) + 1

const progressLabel = (site: Site) => {
  if (site.status === 'done') return '全部完成'
  if (site.status === 'waiting_deploy') return '待部署 GTM'
  if (site.status === 'failed') return `失败 · ${STEP_LABELS[site.currentStep]}`
  return `${stepNumber(site)}/${STEP_KEYS.length} · ${STEP_LABELS[site.currentStep]}`
}

async function loadSteps(id: number) {
  loadingById[id] = true
  errorById[id] = ''
  try {
    const detail = await fetchSite(id)
    stepsById[id] = detail.steps
  } catch (err) {
    errorById[id] = errorMessage(err)
  } finally {
    loadingById[id] = false
  }
}

async function toggleProgress(id: number) {
  const next = !expanded[id]
  expanded[id] = next
  if (next) await loadSteps(id)
}

watch(
  () =>
    props.sites.map((site) => ({
      id: site.id,
      updatedAt: site.updatedAt,
      status: site.status,
    })),
  (rows) => {
    for (const row of rows) {
      if (expanded[row.id]) void loadSteps(row.id)
    }
  },
)
</script>

<template>
  <section class="card list-card">
    <header class="card-header">
      <div>
        <h2 class="card-title">站点列表</h2>
        <p class="card-subtitle">{{ loaded ? `共 ${sites.length} 个` : '加载中…' }}</p>
      </div>
    </header>

    <div v-if="loaded && sites.length === 0" class="empty">
      <p class="empty-title">还没有站点</p>
      <p class="text-muted">在上方填写域名后即可开始接入。</p>
    </div>
    <ul v-else class="list">
      <li
        v-for="site in sites"
        :key="site.id"
        class="row"
        :class="{ selected: site.id === selectedId, open: expanded[site.id] }"
      >
        <div class="row-main">
          <button type="button" class="item" @click="$emit('select', site.id)">
            <span class="item-top">
              <span class="domain">{{ site.domain }}</span>
              <span class="badge" :class="`badge-${site.status}`">
                {{ SITE_STATUS_LABELS[site.status] }}
              </span>
            </span>
            <span class="name">{{ site.name }}</span>
            <span class="meta">
              <span class="meta-left">
                <span
                  v-if="site.outputs.healthStatus"
                  class="health-dot"
                  :class="`health-dot-${site.outputs.healthStatus}`"
                  :title="
                    site.outputs.healthStatus === 'healthy'
                      ? '最近健康检查：健康'
                      : site.outputs.healthStatus === 'unhealthy'
                        ? '最近健康检查：异常'
                        : '最近健康检查：未知'
                  "
                ></span>
                <span>{{ progressLabel(site) }}</span>
              </span>
              <span>{{ formatTime(site.updatedAt) }}</span>
            </span>
          </button>

          <button
            type="button"
            class="progress-toggle"
            :aria-expanded="expanded[site.id] ? 'true' : 'false'"
            :title="expanded[site.id] ? '折叠进度' : '展开进度'"
            @click.stop="toggleProgress(site.id)"
          >
            <span class="chevron" :class="{ open: expanded[site.id] }" aria-hidden="true"></span>
            <span>{{ expanded[site.id] ? '折叠' : '进度' }}</span>
          </button>
        </div>

        <div v-if="expanded[site.id]" class="progress-panel">
          <div v-if="loadingById[site.id] && !stepsById[site.id]" class="progress-loading text-muted">
            <span class="spinner"></span> 加载进度…
          </div>
          <div v-else-if="errorById[site.id]" class="progress-error">{{ errorById[site.id] }}</div>
          <ol v-else class="timeline">
            <li
              v-for="(step, index) in stepsById[site.id] || []"
              :key="step.step"
              class="t-item"
              :class="`t-${step.status}`"
            >
              <span class="t-icon">
                <span v-if="step.status === 'running'" class="spinner"></span>
                <template v-else-if="step.status === 'done'">✓</template>
                <template v-else-if="step.status === 'failed'">!</template>
                <template v-else-if="step.status === 'waiting'">‖</template>
                <template v-else>{{ index + 1 }}</template>
              </span>
              <div class="t-body">
                <div class="t-head">
                  <span class="t-label">{{ STEP_LABELS[step.step] }}</span>
                  <span class="t-status">{{ STEP_STATUS_LABELS[step.status] }}</span>
                </div>
                <p v-if="step.message" class="t-message">{{ step.message }}</p>
              </div>
            </li>
          </ol>
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.list-card {
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
  background: var(--surface);
}

.list-card :deep(.card-header),
.list-card > .card-header {
  flex: 0 0 auto;
  padding: 10px 12px 8px;
}

.empty {
  flex: 1;
  display: grid;
  gap: 4px;
  place-content: center;
  padding: 20px 16px;
  text-align: center;
}

.empty-title {
  font-weight: 600;
}

.list {
  list-style: none;
  margin: 0;
  padding: 6px 8px 8px;
  display: block;
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.list > .row + .row {
  margin-top: 6px;
}

.list::-webkit-scrollbar {
  width: 8px;
}

.list::-webkit-scrollbar-thumb {
  background: #c5ccd8;
  border-radius: 4px;
}

.list::-webkit-scrollbar-track {
  background: transparent;
}

.row {
  display: flex;
  flex-direction: column;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  overflow: hidden;
}

.row.selected {
  border-color: #c8d9fb;
  background: var(--primary-soft);
}

.row-main {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  align-items: stretch;
  min-width: 0;
}

.item {
  min-width: 0;
  display: grid;
  gap: 2px;
  padding: 8px 8px 8px 10px;
  border: none;
  border-radius: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.item:hover {
  background: var(--surface-muted);
}

.row.selected .item:hover {
  background: transparent;
}

.item-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  min-width: 0;
}

.domain {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.name {
  color: var(--text-muted);
  font-size: 12.5px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  margin-top: 2px;
  color: var(--text-subtle);
  font-size: 11.5px;
  min-width: 0;
}

.meta-left {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.health-dot {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 50%;
}

.health-dot-healthy {
  background: var(--success);
}

.health-dot-unhealthy {
  background: var(--danger);
}

.health-dot-unknown {
  background: var(--text-subtle);
}

.progress-toggle {
  flex: none;
  display: inline-flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 2px;
  width: 44px;
  margin: 6px 6px 6px 0;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--primary);
  font: inherit;
  font-size: 11px;
  line-height: 1.1;
  cursor: pointer;
}

.row.selected .progress-toggle {
  background: #fff;
  border-color: #c8d9fb;
}

.progress-toggle:hover {
  background: var(--surface-muted);
  color: var(--text);
}

.chevron {
  width: 6px;
  height: 6px;
  border-right: 1.5px solid currentColor;
  border-bottom: 1.5px solid currentColor;
  transform: rotate(45deg);
  transition: transform 0.15s;
}

.chevron.open {
  transform: rotate(225deg);
  margin-top: 2px;
}

.progress-panel {
  flex: none;
  padding: 8px 10px 10px;
  border-top: 1px solid var(--border);
  background: color-mix(in srgb, var(--surface) 88%, var(--surface-muted));
}

.row.selected .progress-panel {
  border-top-color: #c8d9fb;
}

.progress-loading {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 2px;
  font-size: 12.5px;
}

.progress-error {
  padding: 6px 2px;
  color: var(--danger);
  font-size: 12.5px;
}

.timeline {
  list-style: none;
  margin: 0;
  padding: 0;
}

.t-item {
  position: relative;
  display: flex;
  gap: 10px;
  padding-bottom: 10px;
}

.t-item:last-child {
  padding-bottom: 0;
}

.t-item:not(:last-child)::before {
  content: '';
  position: absolute;
  left: 11px;
  top: 26px;
  bottom: 0;
  width: 2px;
  background: var(--border);
}

.t-done:not(:last-child)::before {
  background: #a7d8b7;
}

.t-icon {
  flex: none;
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  border: 2px solid var(--border);
  background: var(--surface);
  color: var(--text-subtle);
  font-size: 11px;
  font-weight: 700;
}

.t-done .t-icon {
  border-color: var(--success);
  background: var(--success);
  color: #fff;
}

.t-running .t-icon {
  border-color: var(--primary);
  color: var(--primary);
}

.t-running .t-icon .spinner {
  width: 11px;
  height: 11px;
}

.t-waiting .t-icon {
  border-color: var(--warning);
  background: var(--warning-soft);
  color: var(--warning);
}

.t-failed .t-icon {
  border-color: var(--danger);
  background: var(--danger);
  color: #fff;
}

.t-body {
  flex: 1;
  min-width: 0;
  padding-top: 2px;
}

.t-head {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 6px;
}

.t-label {
  font-weight: 600;
  font-size: 12.5px;
}

.t-pending .t-label {
  color: var(--text-muted);
  font-weight: 500;
}

.t-status {
  font-size: 11.5px;
  color: var(--text-subtle);
}

.t-running .t-status {
  color: var(--primary);
}

.t-waiting .t-status {
  color: var(--warning);
}

.t-done .t-status {
  color: var(--success);
}

.t-failed .t-status {
  color: var(--danger);
}

.t-message {
  margin: 3px 0 0;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.4;
  word-break: break-word;
}

.t-failed .t-message {
  color: var(--danger);
}
</style>
