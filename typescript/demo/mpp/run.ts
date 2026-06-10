/**
 * Unified demo runner: spawns the seller server and buyer UI and opens the
 * browser. SA API calls target production (https://web3.okx.com) by default
 * and can be overridden via `SA_BASE_URL`.
 */
import { spawn, exec, type ChildProcess } from 'node:child_process'

const SERVER_PORT = process.env.SERVER_PORT ?? '3000'
const BUYER_PORT = process.env.BUYER_PORT ?? '3002'
const env = { ...process.env, SERVER_PORT, BUYER_PORT }

function startProcess(script: string): ChildProcess {
  const child = spawn('npx', ['tsx', script], {
    stdio: ['ignore', 'pipe', 'pipe'],
    env,
    cwd: import.meta.dirname,
  })
  child.stdout?.on('data', (d: Buffer) => process.stdout.write(d))
  child.stderr?.on('data', (d: Buffer) => process.stderr.write(d))
  return child
}

async function waitForServer(port: string, maxRetries = 20): Promise<void> {
  for (let i = 0; i < maxRetries; i++) {
    try {
      await fetch(`http://localhost:${port}`)
      return
    } catch {
      await new Promise((r) => setTimeout(r, 300))
    }
  }
  throw new Error(`Server on port ${port} did not start in time`)
}

async function main() {
  console.log('Starting Seller Server ...')
  const server = startProcess('server.ts')

  console.log('Starting Buyer UI ...')
  const buyer = startProcess('buyer.ts')

  const cleanup = () => { server.kill(); buyer.kill() }
  process.on('SIGINT', () => { cleanup(); process.exit(0) })
  process.on('SIGTERM', () => { cleanup(); process.exit(0) })

  try {
    await waitForServer(SERVER_PORT)
    await waitForServer(BUYER_PORT)

    const url = `http://localhost:${BUYER_PORT}`
    console.log(`\n  All servers ready.`)
    console.log(`  Open in browser: ${url}`)
    console.log(`  Press Ctrl+C to stop.\n`)

    // Auto-open browser (best effort)
    const openCmd = process.platform === 'darwin' ? 'open' : process.platform === 'win32' ? 'start' : 'xdg-open'
    exec(`${openCmd} ${url}`)

    // Keep running until Ctrl+C
    await new Promise(() => {})
  } catch (e) {
    cleanup()
    throw e
  }
}

main().catch((e) => {
  console.error('Demo failed:', e)
  process.exit(1)
})
