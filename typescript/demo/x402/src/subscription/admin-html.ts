/**
 * Admin UI HTML — vanilla-JS dashboard served at GET /admin. Loads
 * `/admin/api/state` on interval; per-row action buttons hit
 * `/admin/api/charge|cancel|sync/:subId`. No bundler; all CSS + JS inlined.
 */
export const ADMIN_HTML = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>x402 Subscription · Seller Admin</title>
<style>
  :root { color-scheme: light dark; --bg:#0b0d10; --panel:#15181d; --fg:#e6e6e6; --muted:#8a8f98; --accent:#5b8def; --ok:#3ecf8e; --warn:#f0883e; --bad:#f25c5c; --line:#262a31; }
  * { box-sizing:border-box }
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin:0; background:var(--bg); color:var(--fg); }
  header { padding:16px 24px; border-bottom:1px solid var(--line); display:flex; align-items:center; gap:16px; }
  header h1 { font-size:16px; margin:0; font-weight:600; }
  header .meta { color:var(--muted); font-size:12px; font-family: ui-monospace, monospace; }
  main { padding:24px; display:grid; gap:24px; }
  section { background:var(--panel); border:1px solid var(--line); border-radius:8px; padding:16px 20px; }
  section h2 { margin:0 0 12px; font-size:13px; text-transform:uppercase; letter-spacing:.05em; color:var(--muted); }
  table { width:100%; border-collapse:collapse; font-size:13px; }
  th, td { padding:10px 8px; text-align:left; border-bottom:1px solid var(--line); }
  th { color:var(--muted); font-weight:500; font-size:11px; text-transform:uppercase; letter-spacing:.04em; }
  tbody tr:last-child td { border-bottom:none; }
  code, .mono { font-family: ui-monospace, "SF Mono", monospace; font-size:12px; }
  .addr { color:var(--accent); }
  .state { padding:2px 8px; border-radius:99px; font-size:11px; font-weight:600; text-transform:uppercase; }
  .state.active { background:rgba(62,207,142,.15); color:var(--ok); }
  .state.canceled { background:rgba(242,92,92,.15); color:var(--bad); }
  .state.changed  { background:rgba(240,136,62,.15); color:var(--warn); }
  .state.completed{ background:rgba(91,141,239,.15); color:var(--accent); }
  button { background:var(--accent); color:white; border:none; border-radius:6px; padding:6px 12px; font-size:12px; cursor:pointer; font-weight:600; }
  button.secondary { background:transparent; color:var(--accent); border:1px solid var(--accent); }
  button.danger { background:var(--bad); }
  button:disabled { opacity:.4; cursor:not-allowed; }
  button + button { margin-left:6px; }
  .empty { text-align:center; color:var(--muted); padding:32px 0; }
  .toast { position:fixed; bottom:24px; right:24px; padding:12px 20px; border-radius:6px; font-size:13px; max-width:520px; word-wrap:break-word; }
  .toast.ok  { background:var(--ok); color:#0b0d10; }
  .toast.err { background:var(--bad); color:white; }
  .plans { display:grid; grid-template-columns: repeat(3, 1fr); gap:12px; }
  .plan { padding:12px; border:1px solid var(--line); border-radius:6px; }
  .plan h3 { margin:0 0 6px; font-size:14px; }
  .plan .tier { font-size:11px; color:var(--muted); }
  .plan .price { font-size:18px; font-weight:600; margin:8px 0 4px; }
  .plan .price .unit { font-size:11px; color:var(--muted); font-weight:400; }
  .plan .routes { font-size:11px; color:var(--muted); margin-top:6px; padding-top:6px; border-top:1px dashed var(--line); }
  .plan .routes code { background:rgba(91,141,239,.12); color:var(--accent); padding:1px 5px; border-radius:3px; }
  .refresh { float:right; }
</style>
</head>
<body>
<header>
  <h1>x402 Subscription · Seller Admin</h1>
  <div class="meta" id="meta">loading…</div>
</header>
<main>
  <section>
    <h2>Plan Catalog</h2>
    <div class="plans" id="plans"></div>
  </section>
  <section>
    <h2>Subscriptions <button class="secondary refresh" onclick="refresh()">↻ refresh</button></h2>
    <table>
      <thead>
        <tr>
          <th>subId</th><th>payer</th><th>plan</th><th>state</th>
          <th>period</th><th>next charge</th><th>totalPulled</th><th>actions</th>
        </tr>
      </thead>
      <tbody id="subs"><tr><td class="empty" colspan="8">loading…</td></tr></tbody>
    </table>
  </section>
</main>
<div id="toast"></div>
<script>
let DECIMALS = 6;
function shortAddr(a) { return a ? a.slice(0,6) + '…' + a.slice(-4) : '—'; }
function shortSub(s)  { return s ? s.slice(0,8) + '…' + s.slice(-6) : '—'; }
function fmtToken(raw) {
  if (raw === undefined || raw === null) return '—';
  const s = String(raw).padStart(DECIMALS + 1, '0');
  const int = s.slice(0, -DECIMALS), frac = s.slice(-DECIMALS).replace(/0+$/, '');
  return frac ? \`\${int}.\${frac}\` : int;
}
function toast(msg, ok) {
  const el = document.getElementById('toast');
  el.className = 'toast ' + (ok ? 'ok' : 'err');
  el.textContent = msg;
  setTimeout(() => { el.className = ''; el.textContent = ''; }, 4000);
}
async function api(path, opts) {
  const r = await fetch(path, opts);
  const j = await r.json().catch(() => ({}));
  if (!r.ok || j.ok === false) {
    toast(j.error || \`HTTP \${r.status}\`, false);
    return null;
  }
  return j;
}
async function refresh() {
  const s = await api('/admin/api/state');
  if (!s) return;
  DECIMALS = s.token.decimals;
  document.getElementById('meta').textContent =
    \`network=\${s.network} · merchant=\${shortAddr(s.merchant)} · token=\${shortAddr(s.token.address)}\`;

  const plansEl = document.getElementById('plans');
  plansEl.innerHTML = s.plans.map(p => \`
    <div class="plan">
      <h3>\${p.name}</h3>
      <div class="tier">tier \${p.tier} · id <code>\${p.id}</code></div>
      <div class="price">\${fmtToken(p.amountPerPeriod)} <span class="unit">/ period</span></div>
      <div class="tier">period \${p.periodSec}s · max \${p.maxPeriods}</div>
      <div class="routes">
        accessible:
        \${(p.accessibleRoutes || []).map(r => \`<code>\${r.replace('/api/protected/', '')}</code>\`).join(' · ')}
      </div>
    </div>\`).join('');

  const subsEl = document.getElementById('subs');
  if (s.subscriptions.length === 0) {
    subsEl.innerHTML = '<tr><td class="empty" colspan="8">no subscriptions in store yet</td></tr>';
    return;
  }
  subsEl.innerHTML = s.subscriptions.map(sub => {
    const state = sub.state || 'unknown';
    const canCharge = state === 'active';
    const canCancel = state === 'active';
    // Countdown target: prefer nextChargeableAt; fall back to
    // startAt + (lastChargedPeriod * periodSec) when the facilitator hasn't
    // populated nextChargeableAt yet.
    const fallback = sub.startAt
      ? Number(sub.startAt) + (Number(sub.lastChargedPeriod ?? 0)) * Number(sub.periodSec || 0)
      : null;
    const target = sub.nextChargeableAt ?? fallback ?? '';
    return \`<tr data-next="\${target}">
      <td><code>\${shortSub(sub.subId)}</code></td>
      <td><code class="addr">\${shortAddr(sub.payer)}</code></td>
      <td>
        \${sub.planId} <span class="tier">(tier \${sub.planTier})</span>
        \${sub.pendingPlanChange
          ? \`<div class="tier" style="color:var(--warn);margin-top:4px">↘ pending → <code>\${shortSub(sub.pendingPlanChange.newSubId)}</code> @ period \${sub.pendingPlanChange.effectiveFromPeriod}</div>\`
          : ''}
      </td>
      <td><span class="state \${state}">\${state}</span></td>
      <td>\${sub.lastChargedPeriod ?? 0} / \${sub.maxPeriods}</td>
      <td class="countdown mono">—</td>
      <td>\${fmtToken(sub.totalPulled)}</td>
      <td>
        <button onclick="act('charge','\${sub.subId}',this)" \${canCharge ? '' : 'disabled'}>charge</button>
        <button class="danger" onclick="act('cancel','\${sub.subId}',this)" \${canCancel ? '' : 'disabled'}>cancel</button>
        <button class="secondary" onclick="act('sync','\${sub.subId}',this)">sync</button>
      </td>
    </tr>\`;
  }).join('');
  tickCountdowns();
}
function fmtCountdown(seconds) {
  if (seconds <= 0) return '<span style="color:var(--ok)">due now</span>';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  const mm = String(m).padStart(2, '0');
  const ss = String(s).padStart(2, '0');
  return h > 0 ? \`\${h}h \${mm}m \${ss}s\` : \`\${mm}:\${ss}\`;
}
function tickCountdowns() {
  const now = Math.floor(Date.now() / 1000);
  document.querySelectorAll('tr[data-next]').forEach(tr => {
    const target = Number(tr.getAttribute('data-next'));
    const cell = tr.querySelector('.countdown');
    if (!cell) return;
    if (!Number.isFinite(target) || target <= 0) { cell.textContent = '—'; return; }
    cell.innerHTML = fmtCountdown(target - now);
  });
}
async function act(op, subId, btn) {
  btn.disabled = true;
  const r = await api(\`/admin/api/\${op}/\${subId}\`, { method: 'POST' });
  if (r) toast(\`\${op} → ok\`, true);
  btn.disabled = false;
  refresh();
}
refresh();
setInterval(refresh, 5000);
setInterval(tickCountdowns, 1000);
</script>
</body>
</html>`;
