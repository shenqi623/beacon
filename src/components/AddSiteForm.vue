<script setup lang="ts">
import { computed, ref } from 'vue'
import type { NewSiteErrors, SiteDetail, SitePlan } from '../../shared/sites'
import { ApiError, createSite, errorMessage, previewSite } from '@/api'
import PlanPreview from './PlanPreview.vue'

const emit = defineEmits<{ created: [site: SiteDetail] }>()

const domain = ref('')
const name = ref('')
const sitemapUrl = ref('')
const trackClick = ref(true)
const trackOutbound = ref(true)
const trackForm = ref(true)
const errors = ref<NewSiteErrors>({})
const submitError = ref('')
const pending = ref<'create' | 'preview' | null>(null)
const plan = ref<SitePlan | null>(null)

const sitemapPlaceholder = computed(() => {
  const host = domain.value
    .trim()
    .replace(/^https?:\/\//i, '')
    .replace(/\/+$/, '')
  return `默认 https://${host || '{域名}'}/sitemap.xml`
})

async function run(action: 'create' | 'preview') {
  pending.value = action
  errors.value = {}
  submitError.value = ''
  const input = {
    domain: domain.value,
    name: name.value,
    sitemapUrl: sitemapUrl.value || undefined,
    trackClick: trackClick.value,
    trackOutbound: trackOutbound.value,
    trackForm: trackForm.value,
  }
  try {
    if (action === 'preview') {
      plan.value = await previewSite(input)
      return
    }
    const site = await createSite(input)
    domain.value = ''
    name.value = ''
    sitemapUrl.value = ''
    trackClick.value = true
    trackOutbound.value = true
    trackForm.value = true
    plan.value = null
    emit('created', site)
  } catch (err) {
    const body = err instanceof ApiError ? (err.body as { errors?: NewSiteErrors } | null) : null
    if (body?.errors) errors.value = body.errors
    else submitError.value = `${action === 'preview' ? '预演' : '添加'}失败：${errorMessage(err)}`
  } finally {
    pending.value = null
  }
}
</script>

<template>
  <section class="card">
    <header class="card-header">
      <div>
        <h2 class="card-title">添加站点</h2>
        <p class="card-subtitle">
          一键开通 GTM、GA4、Search Console。你只需稍后把 GTM 代码贴到网站。
        </p>
      </div>
    </header>

    <form class="card-body form" @submit.prevent="run('create')">
      <div class="fields">
        <label class="field">
          <span class="field-label">域名<span class="required">*</span></span>
          <input
            v-model="domain"
            class="input"
            :class="{ invalid: errors.domain }"
            placeholder="example.com"
            autocomplete="off"
          />
          <span v-if="errors.domain" class="field-error">{{ errors.domain }}</span>
        </label>
        <label class="field">
          <span class="field-label">站点名称<span class="required">*</span></span>
          <input
            v-model="name"
            class="input"
            :class="{ invalid: errors.name }"
            placeholder="同时用作 GTM / GA4 名称"
          />
          <span v-if="errors.name" class="field-error">{{ errors.name }}</span>
        </label>
        <label class="field">
          <span class="field-label">sitemap（选填）</span>
          <input
            v-model="sitemapUrl"
            class="input"
            :class="{ invalid: errors.sitemapUrl }"
            :placeholder="sitemapPlaceholder"
          />
          <span v-if="errors.sitemapUrl" class="field-error">{{ errors.sitemapUrl }}</span>
        </label>
      </div>

      <div class="tracking">
        <div class="tracking-head">
          <span class="field-label">要自动监听什么？</span>
          <span class="text-muted tracking-hint">默认全开；会写入 GTM 并发送到 GA4</span>
        </div>
        <div class="choice-grid">
          <label class="choice" :class="{ 'choice-on': trackClick }">
            <span class="choice-top">
              <span class="choice-title">点击</span>
              <input v-model="trackClick" type="checkbox" />
            </span>
            <span class="choice-desc">所有元素点击 → <code>click</code></span>
          </label>
          <label class="choice" :class="{ 'choice-on': trackOutbound }">
            <span class="choice-top">
              <span class="choice-title">出站链接</span>
              <input v-model="trackOutbound" type="checkbox" />
            </span>
            <span class="choice-desc">点到外站 → <code>outbound_click</code></span>
          </label>
          <label class="choice" :class="{ 'choice-on': trackForm }">
            <span class="choice-top">
              <span class="choice-title">表单提交</span>
              <input v-model="trackForm" type="checkbox" />
            </span>
            <span class="choice-desc">提交表单 → <code>form_submit</code></span>
          </label>
        </div>
      </div>

      <div class="actions">
        <button type="submit" class="btn btn-primary" :disabled="pending !== null">
          <span v-if="pending === 'create'" class="spinner"></span>
          添加并开始
        </button>
        <button type="button" class="btn" :disabled="pending !== null" @click="run('preview')">
          <span v-if="pending === 'preview'" class="spinner"></span>
          先预演
        </button>
        <span class="text-muted hint">预演只看将执行的操作，不创建资源</span>
      </div>

      <div v-if="submitError" class="alert alert-danger">{{ submitError }}</div>
    </form>

    <PlanPreview v-if="plan" :plan="plan" @close="plan = null" />
  </section>
</template>

<style scoped>
.card :deep(.card-header) {
  padding: 10px 14px 8px;
}

.card :deep(.card-body) {
  padding: 10px 14px 12px;
}

.form {
  display: grid;
  gap: 10px;
}

.fields {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px 12px;
}

.actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}

.hint {
  font-size: 12px;
}

.tracking {
  display: grid;
  gap: 6px;
}

.tracking-head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 6px 10px;
}

.tracking-hint {
  font-size: 12px;
}

@media (min-height: 900px) {
  .card :deep(.card-header) {
    padding: 12px 16px 10px;
  }

  .card :deep(.card-body) {
    padding: 12px 16px 14px;
  }

  .form {
    gap: 12px;
  }
}

@media (max-height: 760px) {
  .card :deep(.card-subtitle) {
    display: none;
  }

  .form {
    gap: 8px;
  }

  .tracking-hint {
    display: none;
  }
}

@media (max-width: 900px) {
  .fields {
    grid-template-columns: 1fr;
  }
}
</style>
