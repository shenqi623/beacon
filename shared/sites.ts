export const STEP_KEYS = [
  'gtm_container',
  'ga_property',
  'data_stream',
  'gtm_publish',
  'deploy_check',
  'gsc_verify',
  'gsc_add',
] as const

export type StepKey = (typeof STEP_KEYS)[number]

export const STEP_LABELS: Record<StepKey, string> = {
  gtm_container: '创建 GTM 容器',
  deploy_check: '等待部署',
  ga_property: '创建 GA4 属性',
  data_stream: '创建数据流',
  gtm_publish: '发布 GTM',
  gsc_verify: '验证 GSC',
  gsc_add: '加入 GSC',
}

export type SiteStatus = 'running' | 'waiting_deploy' | 'failed' | 'done'

export const SITE_STATUS_LABELS: Record<SiteStatus, string> = {
  running: '进行中',
  waiting_deploy: '等待部署',
  failed: '失败',
  done: '完成',
}

export type StepStatus = 'pending' | 'running' | 'waiting' | 'done' | 'failed'

export const STEP_STATUS_LABELS: Record<StepStatus, string> = {
  pending: '未开始',
  running: '执行中',
  waiting: '等待中',
  done: '完成',
  failed: '失败',
}

export type HealthItemStatus = 'pass' | 'fail' | 'skip'
export type HealthStatus = 'healthy' | 'unhealthy' | 'unknown'

export interface HealthItem {
  key: string
  label: string
  status: HealthItemStatus
  message: string
}

export interface SiteOutputs {
  gtmContainerPath?: string
  gtmContainerId?: string
  gtmPublicId?: string
  gaPropertyId?: string
  gaDataStreamId?: string
  measurementId?: string
  gtmVersionId?: string
  trackClick?: boolean
  trackOutbound?: boolean
  trackForm?: boolean
  healthStatus?: HealthStatus
  healthCheckedAt?: string
  healthItems?: HealthItem[]
}

export interface Site {
  id: number
  domain: string
  name: string
  sitemapUrl: string
  status: SiteStatus
  currentStep: StepKey
  error: string | null
  outputs: SiteOutputs
  createdAt: string
  updatedAt: string
}

export interface SiteStep {
  step: StepKey
  status: StepStatus
  message: string
  updatedAt: string | null
}

export interface SiteDetail extends Site {
  steps: SiteStep[]
}

export interface NewSiteInput {
  domain: string
  name: string
  sitemapUrl?: string
  trackClick?: boolean
  trackOutbound?: boolean
  trackForm?: boolean
}

export type NewSiteErrors = Partial<Record<keyof NewSiteInput, string>>

export interface PlanStep {
  step: StepKey
  label: string
  operations: { api: string; params: Record<string, string> }[]
  notes: string[]
}

export interface SitePlan {
  domain: string
  name: string
  sitemapUrl: string
  warnings: string[]
  steps: PlanStep[]
}

export function gtmSnippets(publicId: string): { head: string; body: string } {
  return {
    head: `<!-- Google Tag Manager -->
<script>(function(w,d,s,l,i){w[l]=w[l]||[];w[l].push({'gtm.start':
new Date().getTime(),event:'gtm.js'});var f=d.getElementsByTagName(s)[0],
j=d.createElement(s),dl=l!='dataLayer'?'&l='+l:'';j.async=true;j.src=
'https://www.googletagmanager.com/gtm.js?id='+i+dl;f.parentNode.insertBefore(j,f);
})(window,document,'script','dataLayer','${publicId}');</script>
<!-- End Google Tag Manager -->`,
    body: `<!-- Google Tag Manager (noscript) -->
<noscript><iframe src="https://www.googletagmanager.com/ns.html?id=${publicId}"
height="0" width="0" style="display:none;visibility:hidden"></iframe></noscript>
<!-- End Google Tag Manager (noscript) -->`,
  }
}
