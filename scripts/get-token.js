// 通过本机回环地址完成 Google OAuth 授权，获取 Refresh Token 并写入 .env。
// 需要 OAuth 客户端类型为「桌面应用」，并在 .env 中提供 GOOGLE_CLIENT_ID、GOOGLE_CLIENT_SECRET。
// 用法：pnpm get-token

import { spawn } from 'node:child_process'
import { createHash, randomBytes } from 'node:crypto'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { resolve } from 'node:path'
import { GOOGLE_SCOPES } from '../shared/google-scopes.ts'

const AUTH_URL = 'https://accounts.google.com/o/oauth2/v2/auth'
const TOKEN_URL = 'https://oauth2.googleapis.com/token'
const TIMEOUT_MS = 5 * 60 * 1000
const ENV_PATH = resolve(import.meta.dirname, '../.env')

const clientId = process.env.GOOGLE_CLIENT_ID?.trim()
const clientSecret = process.env.GOOGLE_CLIENT_SECRET?.trim()
if (!clientId || !clientSecret) {
  console.error('请先在项目根目录的 .env 中填写 GOOGLE_CLIENT_ID 和 GOOGLE_CLIENT_SECRET。')
  process.exit(1)
}

const base64url = (buf) => buf.toString('base64url')
const state = base64url(randomBytes(16))
const codeVerifier = base64url(randomBytes(32))
const codeChallenge = base64url(createHash('sha256').update(codeVerifier).digest())

function openBrowser(url) {
  const [cmd, args] =
    process.platform === 'win32'
      ? ['rundll32', ['url.dll,FileProtocolHandler', url]]
      : process.platform === 'darwin'
        ? ['open', [url]]
        : ['xdg-open', [url]]
  spawn(cmd, args, { stdio: 'ignore', detached: true })
    .on('error', () => {})
    .unref()
}

function waitForCode(server) {
  return new Promise((resolvePromise, reject) => {
    const timer = setTimeout(() => reject(new Error('等待授权超时（5 分钟）')), TIMEOUT_MS)
    server.on('request', (req, res) => {
      const url = new URL(req.url ?? '/', 'http://127.0.0.1')
      if (url.pathname !== '/') {
        res.writeHead(404).end()
        return
      }
      const respond = (message) => {
        res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
        res.end(`<p>${message}</p><p>可以关闭此页面并返回终端。</p>`)
      }
      clearTimeout(timer)
      if (url.searchParams.get('state') !== state) {
        respond('授权失败：state 不匹配。')
        reject(new Error('state 不匹配，已中止'))
      } else if (url.searchParams.get('error')) {
        respond('授权被拒绝或失败。')
        reject(new Error(`授权失败：${url.searchParams.get('error')}`))
      } else {
        respond('授权成功。')
        resolvePromise(url.searchParams.get('code'))
      }
    })
  })
}

function writeRefreshToken(refreshToken) {
  const line = `GOOGLE_REFRESH_TOKEN=${refreshToken}`
  const content = existsSync(ENV_PATH) ? readFileSync(ENV_PATH, 'utf8') : ''
  const next = /^GOOGLE_REFRESH_TOKEN=.*$/m.test(content)
    ? content.replace(/^GOOGLE_REFRESH_TOKEN=.*$/m, line)
    : `${content}${content && !content.endsWith('\n') ? '\n' : ''}${line}\n`
  writeFileSync(ENV_PATH, next)
}

const server = createServer()
await new Promise((resolvePromise) => server.listen(0, '127.0.0.1', resolvePromise))
const redirectUri = `http://127.0.0.1:${server.address().port}`

const authUrl = new URL(AUTH_URL)
authUrl.search = new URLSearchParams({
  client_id: clientId,
  redirect_uri: redirectUri,
  response_type: 'code',
  scope: GOOGLE_SCOPES.join(' '),
  access_type: 'offline',
  prompt: 'consent',
  state,
  code_challenge: codeChallenge,
  code_challenge_method: 'S256',
}).toString()

console.log('请在浏览器中完成授权（如未自动打开，请手动访问以下地址）：\n')
console.log(authUrl.toString(), '\n')
openBrowser(authUrl.toString())

try {
  const code = await waitForCode(server)
  const res = await fetch(TOKEN_URL, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      code,
      client_id: clientId,
      client_secret: clientSecret,
      redirect_uri: redirectUri,
      grant_type: 'authorization_code',
      code_verifier: codeVerifier,
    }).toString(),
  })
  const data = await res.json()
  if (!res.ok) throw new Error(`换取 Token 失败：${data.error_description ?? data.error}`)
  if (!data.refresh_token) throw new Error('Google 没有返回 Refresh Token，请重试')

  writeRefreshToken(data.refresh_token)
  console.log('已将 GOOGLE_REFRESH_TOKEN 写入 .env，重启服务后生效。')

  const granted = new Set((data.scope ?? '').split(' '))
  const missing = GOOGLE_SCOPES.filter((scope) => !granted.has(scope))
  if (missing.length > 0) {
    console.warn('\n注意：以下权限未被授予，相关功能将无法使用，请重新运行并勾选全部权限：')
    for (const scope of missing) console.warn(`  - ${scope}`)
  }
} catch (err) {
  console.error(err.message)
  process.exitCode = 1
} finally {
  server.close()
}
