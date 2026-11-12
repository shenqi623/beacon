<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import type { Site, SiteDetail } from '../../shared/sites'
import { errorMessage, fetchSites, getRuntimeInfo } from '@/api'
import AddSiteForm from './AddSiteForm.vue'
import SiteDetailPanel from './SiteDetailPanel.vue'
import SiteList from './SiteList.vue'

const POLL_INTERVAL_MS = 3000

const sites = ref<Site[]>([])
const selectedId = ref<number | null>(null)
const loadError = ref('')
const loaded = ref(false)
const sitesReady = ref(true)
let pollTimer: ReturnType<typeof setTimeout> | undefined

async function loadSites() {
  clearTimeout(pollTimer)
  try {
    const runtime = await getRuntimeInfo()
    sitesReady.value = runtime.sitesReady
    if (!runtime.sitesReady) {
      sites.value = []
      loadError.value = ''
      loaded.value = true
      return
    }
    sites.value = await fetchSites()
    loadError.value = ''
    if (selectedId.value === null && sites.value[0]) selectedId.value = sites.value[0].id
  } catch (err) {
    loadError.value = `加载站点失败：${errorMessage(err)}`
  }
  loaded.value = true
  if (sites.value.some((site) => site.status === 'running')) {
    pollTimer = setTimeout(loadSites, POLL_INTERVAL_MS)
  }
}

function onCreated(site: SiteDetail) {
  selectedId.value = site.id
  loadSites()
}

async function onDeleted() {
  selectedId.value = null
  await loadSites()
  if (sites.value[0]) selectedId.value = sites.value[0].id
}

onMounted(loadSites)
onBeforeUnmount(() => clearTimeout(pollTimer))
</script>

<template>
  <div class="sites-view">
    <div v-if="!sitesReady" class="panel">
      <div class="panel-body notice">
        <h2 class="panel-title">站点功能未就绪</h2>
        <p class="text-muted">当前运行环境未启用站点自动化，请使用桌面端打开。</p>
      </div>
    </div>
    <template v-else>
      <AddSiteForm class="add-block" @created="onCreated" />
      <div v-if="loadError" class="alert alert-danger">{{ loadError }}</div>

      <div class="master-detail">
        <div class="list-pane">
          <SiteList
            :sites="sites"
            :selected-id="selectedId"
            :loaded="loaded"
            @select="selectedId = $event"
          />
        </div>
        <div class="detail-pane">
          <SiteDetailPanel
            v-if="selectedId !== null"
            :site-id="selectedId"
            @changed="loadSites"
            @deleted="onDeleted"
          />
          <div v-else class="card empty-detail">
            <p class="empty-title">选择左侧站点</p>
            <p class="text-muted">安装代码、产出 ID 与健康检查都会显示在这里；执行进度在左侧列表展开。</p>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.sites-view {
  flex: 1 1 0;
  min-height: 0;
  height: 100%;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: hidden;
}

.notice {
  display: grid;
  gap: 8px;
  padding: 28px 8px 20px;
  max-width: 520px;
}

.add-block {
  flex: none;
}

.master-detail {
  flex: 1 1 0;
  min-height: 0;
  display: grid;
  grid-template-columns: minmax(260px, 320px) minmax(0, 1fr);
  grid-template-rows: minmax(0, 1fr);
  gap: 12px;
  overflow: hidden;
}

.list-pane,
.detail-pane {
  position: relative;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.list-pane > :deep(.list-card),
.detail-pane > :deep(.detail),
.detail-pane > .empty-detail {
  position: absolute;
  inset: 0;
  width: auto;
  height: auto;
  max-height: none;
}

.empty-detail {
  display: grid;
  place-content: center;
  gap: 6px;
  padding: 28px 24px;
  text-align: center;
  overflow: auto;
  color: var(--text-muted);
  background:
    radial-gradient(420px 180px at 50% 30%, rgb(15 118 110 / 6%), transparent 70%),
    var(--surface);
}

.empty-title {
  font-weight: 650;
  color: var(--text);
  letter-spacing: -0.01em;
}

@media (min-height: 900px) {
  .sites-view {
    gap: 12px;
  }

  .master-detail {
    gap: 14px;
    grid-template-columns: minmax(280px, 340px) minmax(0, 1fr);
  }
}

@media (max-height: 760px) {
  .sites-view {
    gap: 8px;
  }

  .master-detail {
    gap: 8px;
  }
}

@media (max-width: 900px) {
  .master-detail {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(160px, 34vh) minmax(0, 1fr);
  }
}
</style>
