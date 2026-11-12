export interface Settings {
  gaAccountId: string
  gtmAccountId: string
  timeZone: string
  currencyCode: string
  gtmWriteIntervalMs: number
  gscVerifyRetryIntervalSec: number
  gscVerifyMaxAttempts: number
  proxyEnabled: boolean
  proxyUrl: string
}

export type SettingsErrors = Partial<Record<keyof Settings, string>>

export interface CredentialStatus {
  clientId: boolean
  clientSecret: boolean
  refreshToken: boolean
}

export interface SettingsResponse {
  settings: Settings
  credentials: CredentialStatus
  dataDir?: string
}

export interface SaveConfigPayload {
  settings: Settings
  clientId?: string
  clientSecret?: string
}

export interface AuthCheckItem {
  key: 'credentials' | 'token' | 'scopes' | 'gaAccount' | 'gtmAccount' | string
  label: string
  ok: boolean
  message: string
}

export interface AuthCheckResult {
  ok: boolean
  checkedAt: string
  items: AuthCheckItem[]
}

export interface OAuthStartResult {
  message: string
  grantedScopesOk: boolean
  missingScopes: string[]
}

export interface RuntimeInfo {
  mode: 'tauri' | 'web'
  sitesReady: boolean
}
