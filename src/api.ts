import { invoke } from '@tauri-apps/api/core'
import type { NewSiteInput, Site, SiteDetail, SitePlan } from '../shared/sites'
import type {
  AuthCheckResult,
  OAuthStartResult,
  RuntimeInfo,
  SaveConfigPayload,
  Settings,
  SettingsResponse,
} from '../shared/types'
import { isTauri } from './runtime'

export class ApiError extends Error {
  readonly status: number
  readonly body: unknown

  constructor(status: number, body: unknown) {
    super(`请求失败（HTTP ${status}）`)
    this.name = 'ApiError'
    this.status = status
    this.body = body
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const res = await fetch(path, {
    ...init,
    headers: init.body ? { 'content-type': 'application/json', ...init.headers } : init.headers,
  })
  const body: unknown = await res.json().catch(() => null)
  if (!res.ok) throw new ApiError(res.status, body)
  return body as T
}

function throwInvokeError(err: unknown): never {
  const raw = typeof err === 'string' ? err : err instanceof Error ? err.message : String(err)
  try {
    const body = JSON.parse(raw) as { status?: number; message?: string; errors?: unknown }
    if (body && (body.errors || body.message)) {
      throw new ApiError(body.status ?? 400, body)
    }
  } catch (e) {
    if (e instanceof ApiError) throw e
  }
  throw new Error(raw)
}

async function invokeSite<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args)
  } catch (err) {
    throwInvokeError(err)
  }
}

export async function getRuntimeInfo(): Promise<RuntimeInfo> {
  if (isTauri()) return invoke<RuntimeInfo>('runtime_info')
  return { mode: 'web', sitesReady: true }
}

export async function fetchSettings(): Promise<SettingsResponse> {
  if (isTauri()) return invoke<SettingsResponse>('get_config')
  return request<SettingsResponse>('/api/settings')
}

export async function updateSettings(payload: SaveConfigPayload | Settings): Promise<SettingsResponse> {
  if (isTauri()) {
    const body: SaveConfigPayload =
      'settings' in payload ? payload : { settings: payload as Settings }
    return invoke<SettingsResponse>('save_config', { request: body })
  }
  const settings = 'settings' in payload ? payload.settings : (payload as Settings)
  return request<SettingsResponse>('/api/settings', {
    method: 'PUT',
    body: JSON.stringify(settings),
  })
}

export async function clearCredential(
  field: 'clientId' | 'clientSecret' | 'refreshToken',
): Promise<SettingsResponse> {
  if (!isTauri()) throw new Error('清除凭证仅在桌面端可用')
  return invoke<SettingsResponse>('clear_credential', { field })
}

export async function startGoogleAuth(): Promise<OAuthStartResult> {
  if (!isTauri()) throw new Error('授权登录仅在桌面端可用，请运行 pnpm tauri:dev')
  return invoke<OAuthStartResult>('start_google_auth')
}

export async function openDataDir(): Promise<void> {
  if (!isTauri()) throw new Error('打开数据目录仅在桌面端可用')
  await invoke('open_data_dir')
}

export const checkAuth = () =>
  isTauri()
    ? invoke<AuthCheckResult>('check_auth')
    : request<AuthCheckResult>('/api/auth/check', { method: 'POST' })

export const fetchSites = () =>
  isTauri() ? invokeSite<Site[]>('list_sites') : request<Site[]>('/api/sites')

export const fetchSite = (id: number) =>
  isTauri()
    ? invokeSite<SiteDetail>('get_site', { id })
    : request<SiteDetail>(`/api/sites/${id}`)

export const createSite = (input: NewSiteInput) =>
  isTauri()
    ? invokeSite<SiteDetail>('create_site', { input })
    : request<SiteDetail>('/api/sites', { method: 'POST', body: JSON.stringify(input) })

export const previewSite = (input: NewSiteInput) =>
  isTauri()
    ? invokeSite<SitePlan>('preview_site_cmd', { input })
    : request<SitePlan>('/api/sites/preview', { method: 'POST', body: JSON.stringify(input) })

export const checkDeploy = (id: number) =>
  isTauri()
    ? invokeSite<SiteDetail>('check_site_deploy', { id })
    : request<SiteDetail>(`/api/sites/${id}/deploy-check`, { method: 'POST' })

export const resumeSite = (id: number) =>
  isTauri()
    ? invokeSite<SiteDetail>('resume_site_cmd', { id })
    : request<SiteDetail>(`/api/sites/${id}/resume`, { method: 'POST' })

export const deleteSite = (id: number) =>
  isTauri()
    ? invokeSite<void>('delete_site_cmd', { id })
    : request<void>(`/api/sites/${id}`, { method: 'DELETE' })

export const healthCheckSite = (id: number) =>
  isTauri()
    ? invokeSite<SiteDetail>('health_check_site_cmd', { id })
    : Promise.reject(new Error('健康检查仅支持桌面端'))

export function errorMessage(err: unknown): string {
  if (typeof err === 'string') return err
  if (err instanceof ApiError) {
    const message = (err.body as { message?: unknown } | null)?.message
    if (typeof message === 'string') return message
  }
  return err instanceof Error ? err.message : String(err)
}
