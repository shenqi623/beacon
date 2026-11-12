<script setup lang="ts">
import type { SitePlan } from '../../shared/sites'

defineProps<{ plan: SitePlan }>()
defineEmits<{ close: [] }>()
</script>

<template>
  <div class="plan">
    <div class="plan-header">
      <div>
        <h3 class="plan-title">预演：{{ plan.name }}</h3>
        <p class="text-muted plan-sub">
          {{ plan.domain }} · sitemap {{ plan.sitemapUrl }} · 以下操作仅为预览，不会创建任务或调用
          Google 接口
        </p>
      </div>
      <button type="button" class="btn btn-sm btn-ghost" @click="$emit('close')">收起</button>
    </div>

    <div v-if="plan.warnings.length" class="alert alert-warning">
      <span class="alert-title">实际执行前需要处理</span>
      <ul class="warnings">
        <li v-for="warning in plan.warnings" :key="warning">{{ warning }}</li>
      </ul>
    </div>

    <ol class="steps">
      <li v-for="(step, index) in plan.steps" :key="step.step" class="step">
        <span class="step-index">{{ index + 1 }}</span>
        <div class="step-body">
          <h4 class="step-title">{{ step.label }}</h4>
          <div v-for="op in step.operations" :key="op.api" class="operation">
            <div class="op-api mono">{{ op.api }}</div>
            <dl class="params">
              <template v-for="(value, key) in op.params" :key="key">
                <dt>{{ key }}</dt>
                <dd class="mono">{{ value }}</dd>
              </template>
            </dl>
          </div>
          <ul class="notes">
            <li v-for="note in step.notes" :key="note">{{ note }}</li>
          </ul>
        </div>
      </li>
    </ol>
  </div>
</template>

<style scoped>
.plan {
  display: grid;
  gap: 16px;
  padding: 20px;
  border-top: 1px solid var(--border);
  background: var(--surface-muted);
  border-radius: 0 0 var(--radius) var(--radius);
}

.plan-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 12px;
}

.plan-title {
  font-size: 15px;
  font-weight: 600;
}

.plan-sub {
  margin-top: 2px;
  font-size: 12px;
}

.warnings {
  margin: 0;
  padding-left: 18px;
}

.steps {
  list-style: none;
  margin: 0;
  padding: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 12px;
}

.step {
  display: flex;
  gap: 12px;
  padding: 14px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 8px;
}

.step-index {
  flex: none;
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: var(--primary-soft);
  color: var(--primary);
  font-size: 12px;
  font-weight: 600;
}

.step-body {
  display: grid;
  gap: 8px;
  min-width: 0;
}

.step-title {
  margin: 2px 0 0;
  font-size: 14px;
  font-weight: 600;
}

.operation {
  display: grid;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--surface-muted);
  border: 1px solid var(--border);
}

.op-api {
  font-size: 12px;
  font-weight: 600;
  color: var(--primary);
}

.params {
  display: grid;
  grid-template-columns: max-content minmax(0, 1fr);
  gap: 3px 12px;
  margin: 0;
  font-size: 12px;
}

.params dt {
  color: var(--text-muted);
}

.params dd {
  margin: 0;
  word-break: break-all;
}

.notes {
  margin: 0;
  padding-left: 16px;
  color: var(--text-muted);
  font-size: 12px;
}
</style>
