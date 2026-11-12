<script setup lang="ts">
import { computed, ref } from 'vue'
import AuthCheckPanel from '@/components/AuthCheckPanel.vue'
import SettingsPanel from '@/components/SettingsPanel.vue'
import SitesView from '@/components/SitesView.vue'
import { getRuntimeInfo } from '@/api'

const tab = ref<'sites' | 'settings' | 'auth'>('sites')
getRuntimeInfo()
  .then((info) => {
    if (!info.sitesReady) tab.value = 'settings'
  })
  .catch(() => {})

const nav = [
  { key: 'sites' as const, label: '站点', hint: '任务与进度' },
  { key: 'settings' as const, label: '配置', hint: '账号 · 代理 · OAuth' },
  { key: 'auth' as const, label: '授权', hint: '检查 Google 权限' },
]

const pageTitle = computed(() => {
  if (tab.value === 'sites') return '站点'
  if (tab.value === 'settings') return '配置'
  return '授权'
})

const pageDesc = computed(() => {
  if (tab.value === 'sites') return '添加域名，自动开通并跟踪 GTM / GA4 / Search Console'
  if (tab.value === 'settings') return '账号、代理与 OAuth 都在这里，不用改配置文件'
  return '确认凭证有效，并能访问 GA / GTM 账号'
})
</script>

<template>
  <div class="shell">
    <aside class="sidebar">
      <div class="brand">
        <span class="brand-mark" aria-hidden="true"></span>
        <div class="brand-text">
          <span class="brand-name">Beacon</span>
          <span class="brand-sub">GTM · GA4 · GSC</span>
        </div>
      </div>

      <nav class="nav" aria-label="主导航">
        <button
          v-for="item in nav"
          :key="item.key"
          type="button"
          class="nav-item"
          :class="{ active: tab === item.key }"
          @click="tab = item.key"
        >
          <span class="nav-label">{{ item.label }}</span>
          <span class="nav-hint">{{ item.hint }}</span>
        </button>
      </nav>

      <div class="sidebar-foot">
        <span class="version">桌面版 0.1.0</span>
      </div>
    </aside>

    <div class="main">
      <header class="page-header">
        <div>
          <h1 class="page-title">{{ pageTitle }}</h1>
          <p class="page-desc">{{ pageDesc }}</p>
        </div>
      </header>

      <div class="page-body" :class="{ 'page-body-fill': tab === 'sites' }">
        <SitesView v-if="tab === 'sites'" />
        <SettingsPanel v-else-if="tab === 'settings'" />
        <AuthCheckPanel v-else />
      </div>
    </div>
  </div>
</template>

<style scoped>
.shell {
  display: grid;
  grid-template-columns: var(--sidebar-width) minmax(0, 1fr);
  height: 100%;
  background: transparent;
}

.sidebar {
  display: flex;
  flex-direction: column;
  background: color-mix(in srgb, var(--sidebar) 88%, #fff);
  border-right: 1px solid var(--sidebar-border);
  padding: 14px 10px 12px;
  min-height: 0;
  backdrop-filter: blur(10px);
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 8px 18px;
}

.brand-mark {
  width: 28px;
  height: 28px;
  border-radius: 8px;
  background:
    radial-gradient(circle at 50% 50%, #fff 0 2.5px, transparent 3.5px),
    linear-gradient(145deg, #4f8ff7 0%, #0b57d0 100%);
  box-shadow: 0 4px 10px rgb(11 87 208 / 22%);
  flex: none;
}

.brand-text {
  display: grid;
  gap: 1px;
  min-width: 0;
}

.brand-name {
  font-size: 15px;
  font-weight: 720;
  letter-spacing: -0.01em;
  line-height: 1.2;
}

.brand-sub {
  font-size: 11px;
  color: var(--text-subtle);
  letter-spacing: 0.04em;
}

.nav {
  display: grid;
  gap: 4px;
  flex: 1;
  min-height: 0;
  align-content: start;
}

.nav-item {
  position: relative;
  display: grid;
  gap: 1px;
  width: 100%;
  text-align: left;
  padding: 9px 10px 9px 12px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  transition:
    background 0.14s ease,
    color 0.14s ease,
    box-shadow 0.14s ease;
}

.nav-item:hover {
  background: rgb(21 27 40 / 4%);
}

.nav-item.active {
  background: linear-gradient(90deg, #edf3ff 0%, var(--accent-soft) 100%);
  color: var(--accent);
  box-shadow: inset 3px 0 0 var(--accent);
}

.nav-label {
  font-size: 13.5px;
  font-weight: 650;
}

.nav-hint {
  font-size: 11.5px;
  color: var(--text-subtle);
}

.nav-item.active .nav-hint {
  color: rgb(11 87 208 / 72%);
}

.sidebar-foot {
  padding: 10px 8px 2px;
  border-top: 1px solid var(--border);
  margin-top: 8px;
}

.version {
  font-size: 11px;
  color: var(--text-subtle);
}

.main {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  height: 100%;
  overflow: hidden;
}

.page-header {
  flex: none;
  padding: 14px 20px 10px;
  border-bottom: 1px solid rgb(219 225 234 / 55%);
}

.page-title {
  font-size: 18px;
  font-weight: 700;
  letter-spacing: -0.02em;
  line-height: 1.2;
}

.page-desc {
  margin-top: 3px;
  font-size: 12.5px;
  color: var(--text-muted);
  line-height: 1.4;
}

.page-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: auto;
  padding: 6px 20px 16px;
}

.page-body-fill {
  overflow: hidden;
}

@media (min-height: 900px) {
  .page-header {
    padding: 18px 24px 12px;
  }

  .page-title {
    font-size: 20px;
  }

  .page-desc {
    margin-top: 4px;
    font-size: 12.5px;
  }

  .page-body {
    padding: 8px 24px 20px;
  }
}

@media (max-height: 760px) {
  .page-header {
    padding: 10px 16px 4px;
  }

  .page-desc {
    display: none;
  }

  .page-body {
    padding: 4px 16px 10px;
  }
}

@media (max-width: 820px) {
  .shell {
    grid-template-columns: 1fr;
  }

  .sidebar {
    border-right: 0;
    border-bottom: 1px solid var(--sidebar-border);
    padding-bottom: 8px;
  }

  .nav {
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    gap: 4px;
  }

  .nav-hint,
  .sidebar-foot {
    display: none;
  }
}
</style>
