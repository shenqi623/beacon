<script setup lang="ts">
import { ref } from 'vue'
import type { AuthCheckResult } from '../../shared/types'
import { checkAuth, errorMessage } from '@/api'
import { formatTime } from '@/format'

const result = ref<AuthCheckResult | null>(null)
const checking = ref(false)
const requestError = ref('')

async function runCheck() {
  checking.value = true
  requestError.value = ''
  try {
    result.value = await checkAuth()
  } catch (err) {
    requestError.value = `检查失败：${errorMessage(err)}`
  } finally {
    checking.value = false
  }
}
</script>

<template>
  <div class="auth">
    <section class="panel">
      <header class="panel-header">
        <div>
          <h2 class="panel-title">授权状态</h2>
          <p class="panel-subtitle">检查 OAuth 凭证、权限范围，以及 GA / GTM 账号是否可访问。</p>
        </div>
        <button type="button" class="btn btn-primary" :disabled="checking" @click="runCheck">
          <span v-if="checking" class="spinner"></span>
          {{ checking ? '检查中' : '检查授权' }}
        </button>
      </header>

      <div class="panel-body body">
        <div v-if="requestError" class="alert alert-danger">{{ requestError }}</div>

        <div v-if="!result && !requestError" class="empty">
          <p class="empty-title">尚未检查</p>
          <p class="text-muted">点击右上角「检查授权」。执行站点任务前也会自动检查一次。</p>
        </div>

        <template v-if="result">
          <div class="alert" :class="result.ok ? 'alert-success' : 'alert-danger'">
            <span class="alert-title">
              {{ result.ok ? '授权有效，可以执行任务' : '授权检查未通过' }}
            </span>
            <span>检查时间 {{ formatTime(result.checkedAt) }}</span>
          </div>

          <ul class="items">
            <li v-for="item in result.items" :key="item.key" :class="item.ok ? 'ok' : 'fail'">
              <span class="dot" aria-hidden="true"></span>
              <div class="content">
                <div class="label-row">
                  <span class="label">{{ item.label }}</span>
                  <span class="state">{{ item.ok ? '通过' : '失败' }}</span>
                </div>
                <div class="message selectable">{{ item.message }}</div>
              </div>
            </li>
          </ul>
        </template>
      </div>
    </section>
  </div>
</template>

<style scoped>
.auth {
  max-width: 720px;
}

.body {
  display: grid;
  gap: 12px;
}

.empty {
  display: grid;
  gap: 4px;
  padding: 28px 8px 20px;
  text-align: center;
}

.empty-title {
  font-weight: 600;
  font-size: 13.5px;
}

.items {
  list-style: none;
  margin: 0;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  overflow: hidden;
}

.items li {
  display: flex;
  gap: 12px;
  padding: 11px 12px;
  background: var(--surface);
}

.items li + li {
  border-top: 1px solid var(--border);
}

.dot {
  flex: none;
  width: 8px;
  height: 8px;
  margin-top: 6px;
  border-radius: 50%;
  background: var(--border-strong);
}

.ok .dot {
  background: var(--success);
}

.fail .dot {
  background: var(--danger);
}

.content {
  min-width: 0;
  flex: 1;
  display: grid;
  gap: 2px;
}

.label-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}

.label {
  font-weight: 600;
  font-size: 13px;
}

.state {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--text-subtle);
}

.ok .state {
  color: var(--success);
}

.fail .state {
  color: var(--danger);
}

.message {
  color: var(--text-muted);
  font-size: 12.5px;
  word-break: break-word;
  line-height: 1.4;
}

.fail .message {
  color: #8a1f18;
}
</style>
