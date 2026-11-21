// Renders the hero's fallback posters from the live scene, so the still a visitor without WebGL
// sees is the same island, landmarks and sea the scene draws.
//
//   1. pnpm --filter @lanka-data-layer/web-v2 dev
//   2. start a Chrome with a debugging port, for example
//        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --remote-debugging-port=9333
//   3. pnpm --filter @lanka-data-layer/web-v2 poster [devtools endpoint] [dev server]
//        defaults: http://127.0.0.1:9333  http://127.0.0.1:5183
//      (the endpoint may also be a ws://…/devtools/browser/… address)
//
// Output (web-v2/public/hero): poster-{dark,light}-{wide,tall}.webp
// Needs nothing but Node 22+: the browser encodes the WebP itself.
import { mkdirSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const OUT = resolve(dirname(fileURLToPath(import.meta.url)), '../public/hero')
const [endpoint = 'http://127.0.0.1:9333', app = 'http://127.0.0.1:5183'] = process.argv.slice(2)
mkdirSync(OUT, { recursive: true })

// The camera at rest with no drift, the scene alone with nothing laid over it (see the review switches in Hero.tsx / HeroScene.tsx).
const PAGE = `${app}/?hero=poster&view=80.7,7.8,0`
/** [name, css width, css height, device scale]: the framing follows the aspect ratio, the scale sets the pixels */
const SHOTS = [
  ['wide', 1440, 900, 5 / 3], // 2400 × 1500
  ['tall', 500, 1084, 2], // 1000 × 2168
]
const QUALITY = 80

const wsUrl = endpoint.startsWith('ws') ? endpoint : (await (await fetch(`${endpoint}/json/version`)).json()).webSocketDebuggerUrl
const ws = new WebSocket(wsUrl)
await new Promise((ok, fail) => {
  ws.onopen = ok
  ws.onerror = () => fail(new Error(`no browser at ${endpoint}`))
})
let seq = 0
const waiting = new Map()
ws.onmessage = ({ data }) => {
  const m = JSON.parse(data)
  const w = waiting.get(m.id)
  if (!w) return
  waiting.delete(m.id)
  m.error ? w.fail(new Error(m.error.message)) : w.ok(m.result)
}
const send = (method, params = {}, sessionId) =>
  new Promise((ok, fail) => {
    waiting.set(++seq, { ok, fail })
    ws.send(JSON.stringify({ id: seq, method, params, sessionId }))
  })
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

const { targetId } = await send('Target.createTarget', { url: 'about:blank' })
const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true })
const page = (method, params) => send(method, params, sessionId)
const evaluate = async (expression) => (await page('Runtime.evaluate', { expression, returnByValue: true })).result.value
await page('Page.enable')

// the theme is pinned through localStorage for each shot: remember what was there and put it back afterwards
await page('Page.navigate', { url: `${app}/favicon.svg` })
await sleep(300)
const kept = await evaluate(`(() => { try { return localStorage.getItem('ldl-theme') } catch { return null } })()`)

try {
  for (const theme of ['dark', 'light']) {
    for (const [name, width, height, scale] of SHOTS) {
      await page('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: scale, mobile: false })
      await page('Page.navigate', { url: 'about:blank' })
      // the page reads its theme before first paint
      const { identifier } = await page('Page.addScriptToEvaluateOnNewDocument', { source: `try { localStorage.setItem('ldl-theme', '${theme}') } catch {}` })
      await page('Page.navigate', { url: PAGE })
      // the scene is on the glass once its canvas exists and the loader has gone
      for (let i = 0; i < 120; i++) {
        if (await evaluate(`!!document.querySelector('canvas') && !document.querySelector('.hero-loader')`)) break
        await sleep(250)
      }
      await evaluate(`document.head.append(Object.assign(document.createElement('style'), { textContent: 'header, footer, main > :not(:first-child) { display: none !important }' }))`)
      // the scene fades in over a second; give the shadow map and the fleet a moment as well
      await sleep(2600)
      const { data } = await page('Page.captureScreenshot', { format: 'webp', quality: QUALITY, clip: { x: 0, y: 0, width, height, scale: 1 } })
      const file = join(OUT, `poster-${theme}-${name}.webp`)
      writeFileSync(file, Buffer.from(data, 'base64'))
      console.log(`poster-${theme}-${name}.webp`, `${Math.round(width * scale)} × ${Math.round(height * scale)}`, `${(statSync(file).size / 1024).toFixed(0)} KB`)
      await page('Page.removeScriptToEvaluateOnNewDocument', { identifier })
    }
  }
} finally {
  await evaluate(`try { ${kept === null ? `localStorage.removeItem('ldl-theme')` : `localStorage.setItem('ldl-theme', ${JSON.stringify(kept)})`} } catch {}`).catch(() => {})
  await send('Target.closeTarget', { targetId })
  ws.close()
}
