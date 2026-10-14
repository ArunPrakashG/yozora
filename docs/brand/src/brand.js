// Shared pieces for the brand images. The tile matches packaging/icons/*.svg. Also: the night sky, the mark, and the bar, popover and toast
// drawn with the Midnight tokens from crates/yozora-ui/src/theme.rs.

const T = {
  midnight: '#0A0E17',
  graphite: '#252E40',
  ink: '#E8F1FF',
  ink2: '#98A5BE',
  ink3: '#7A869F',
  starlight: '#8BBEFF',
  aurora: '#8FE3CF',
  nebula: '#BBA8FF',
  dawn: '#FFC58B',
};

function rng(seed) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Paints stars and faint nebulae into `canvas`, sized to its parent. */
function sky(canvas, { seed = 7, density = 1, glows = [], clear = [] } = {}) {
  const w = canvas.parentElement.clientWidth, h = canvas.parentElement.clientHeight;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = w * dpr; canvas.height = h * dpr;
  canvas.style.width = w + 'px'; canvas.style.height = h + 'px';
  const g = canvas.getContext('2d');
  g.scale(dpr, dpr);
  for (const n of glows) {
    const r = g.createRadialGradient(n.x * w, n.y * h, 0, n.x * w, n.y * h, n.r * Math.max(w, h));
    r.addColorStop(0, n.color);
    r.addColorStop(1, 'transparent');
    g.fillStyle = r;
    g.fillRect(0, 0, w, h);
  }
  const rand = rng(seed);
  const count = Math.round((w * h) / 1400 * density);
  const tints = ['#FFFFFF', '#E8F1FF', '#CFE2FF', '#BBD4FF', '#FFF1E0'];
  for (let i = 0; i < count; i++) {
    const x = rand() * w, y = rand() * h;
    const m = Math.pow(rand(), 3.2);
    // Keep text clean: no stars inside the `clear` boxes, given in CSS pixels.
    if (clear.some(([cx, cy, cw, ch]) => x > cx && x < cx + cw && y > cy && y < cy + ch)) continue;
    const r = 0.25 + m * 1.35;
    g.globalAlpha = 0.18 + m * 0.75;
    g.fillStyle = tints[Math.floor(rand() * tints.length)];
    g.beginPath(); g.arc(x, y, r, 0, Math.PI * 2); g.fill();
    if (m > 0.55) {
      const halo = g.createRadialGradient(x, y, 0, x, y, r * 7);
      halo.addColorStop(0, 'rgba(170,205,255,0.35)');
      halo.addColorStop(1, 'transparent');
      g.globalAlpha = 0.8; g.fillStyle = halo;
      g.beginPath(); g.arc(x, y, r * 7, 0, Math.PI * 2); g.fill();
    }
  }
  g.globalAlpha = 1;
}

/** The mark: a Y traced as a constellation. `tile` puts it on a glass app-icon tile. */
function mark({ size = 96, tile = false, id = 'm' } = {}) {
  const stars = [[27, 25, 2.6], [75, 29, 2.9], [50, 53, 4.4], [48, 81, 3.1]];
  const lines = [[0, 2], [1, 2], [2, 3]];
  const s = (x, y, r, i) => `
    <circle cx="${x}" cy="${y}" r="${r * 3.4}" fill="url(#${id}h)" opacity="${i === 2 ? 0.95 : 0.55}"/>
    <circle cx="${x}" cy="${y}" r="${r}" fill="${i === 2 ? T.starlight : T.ink}"/>`;
  const sparkle = (x, y, l) =>
    `<path d="M${x} ${y - l} Q${x} ${y} ${x + l} ${y} Q${x} ${y} ${x} ${y + l} Q${x} ${y} ${x - l} ${y} Q${x} ${y} ${x} ${y - l}Z" fill="${T.ink}" opacity=".9"/>`;
  return `<svg width="${size}" height="${size}" viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
    <defs>
      <radialGradient id="${id}h"><stop offset="0" stop-color="${T.starlight}" stop-opacity=".9"/><stop offset="1" stop-color="${T.starlight}" stop-opacity="0"/></radialGradient>
      <linearGradient id="${id}t" x1="0" y1="0" x2="0.6" y2="1"><stop offset="0" stop-color="#1A2747"/><stop offset="1" stop-color="#070B14"/></linearGradient>
      <linearGradient id="${id}e" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#C9DCFF" stop-opacity=".45"/><stop offset=".5" stop-color="#AAC8FF" stop-opacity=".12"/><stop offset="1" stop-color="#AAC8FF" stop-opacity=".22"/></linearGradient>
    </defs>
    ${tile ? `<rect x="1" y="1" width="98" height="98" rx="23" fill="url(#${id}t)" stroke="url(#${id}e)" stroke-width="1.2"/>` : ''}
    <g transform="${tile ? 'translate(13 12) scale(.74)' : ''}">
      ${lines.map(([a, b]) => `<line x1="${stars[a][0]}" y1="${stars[a][1]}" x2="${stars[b][0]}" y2="${stars[b][1]}" stroke="${T.ink}" stroke-opacity=".42" stroke-width="1.3" stroke-linecap="round"/>`).join('')}
      ${stars.map(([x, y, r], i) => s(x, y, r, i)).join('')}
      ${sparkle(84, 66, 4.2)}
      <circle cx="18" cy="60" r="1.1" fill="${T.ink}" opacity=".55"/>
      <circle cx="66" cy="12" r=".9" fill="${T.ink}" opacity=".45"/>
    </g>
  </svg>`;
}

const I = {
  wifi: '<path d="M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M2 9a15 15 0 0 1 20 0"/><circle cx="12" cy="19.5" r=".6" fill="currentColor"/>',
  vol: '<path d="M11 5 6 9H3v6h3l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13"/>',
  bat: '<rect x="2.5" y="7" width="17" height="10" rx="2.5"/><path d="M22 11v2"/><rect x="5" y="9.5" width="10" height="5" rx="1" fill="currentColor" stroke="none"/>',
  bt: '<path d="m7 7 10 10-5 5V2l5 5L7 17"/>',
  moon: '<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/>',
  gauge: '<path d="M12 14 16 9"/><path d="M3.5 18a9.5 9.5 0 1 1 17 0"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  bell: '<path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15z"/><path d="M10 20.5a2.2 2.2 0 0 0 4 0"/>',
  play: '<path d="M8 5.5v13l10-6.5z" fill="currentColor"/>',
  next: '<path d="M6 6v12l8.5-6zM17 6v12" fill="currentColor"/>',
  prev: '<path d="M18 6v12L9.5 12zM7 6v12" fill="currentColor"/>',
  check: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  term: '<path d="m5 8 4 4-4 4M12 16h7"/>',
};
const icon = (n, s = 16, extra = '') =>
  `<svg width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" ${extra}>${I[n]}</svg>`;

function bar({ title = 'notes.md — foot', clock = 'Wed 14 Oct  11:17' } = {}) {
  const ws = [1, 2, 3, 4, 5].map(n => `<span class="ws${n === 1 ? ' on' : n === 3 ? ' used' : ''}">${n}</span>`).join('');
  return `<div class="bar">
    <div class="side">${ws}</div>
    <div class="title">${title}</div>
    <div class="side right">
      <span class="item">${icon('bell', 15)}</span>
      <span class="item">${icon('wifi', 15)}</span>
      <span class="item">${icon('vol', 15)}</span>
      <span class="item bat">${icon('bat', 17)}<b>84%</b></span>
      <span class="item clock">${clock}</span>
    </div>
  </div>`;
}

function popover() {
  const tile = (ic, name, sub, on) => `<div class="tile${on ? ' lit' : ''}"><span class="ti">${icon(ic, 17)}</span><span><b>${name}</b><i>${sub}</i></span></div>`;
  const slider = (ic, pct) => `<div class="slider"><div class="fill" style="width:${pct}%"><span>${icon(ic, 15)}</span></div></div>`;
  return `<div class="glass pop">
    <div class="tiles">
      ${tile('wifi', 'Wi-Fi', 'Hoshi 5G', true)}
      ${tile('bt', 'Bluetooth', 'Buds Pro', true)}
      ${tile('moon', 'Do Not Disturb', 'Off', false)}
      ${tile('gauge', 'Performance', 'Power profile', false)}
    </div>
    ${slider('sun', 72)}
    ${slider('vol', 46)}
    <div class="media">
      <div class="art"></div>
      <div class="mt"><b>Midnight City</b><i>M83 · Spotify</i></div>
      <div class="mc">${icon('prev', 15)}${icon('play', 17)}${icon('next', 15)}</div>
    </div>
  </div>`;
}

function toast({ app = 'Claude Code', summary = 'Agent: tests passed', body = '212 passed, 0 failed', ic = 'check', progress = 0.72 } = {}) {
  return `<div class="glass toast">
    <span class="tic">${icon(ic, 17)}</span>
    <div class="tb"><div class="th"><span>${app}</span><span>now</span></div><b>${summary}</b><i>${body}</i></div>
    <div class="tp" style="width:${progress * 100}%"></div>
  </div>`;
}

const CSS = `
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body { background: ${T.midnight}; color: ${T.ink}; font-family: 'Inter', 'Inter Variable', sans-serif; -webkit-font-smoothing: antialiased; }
.stage { position: relative; overflow: hidden; background: radial-gradient(120% 90% at 70% 0%, #111A2C 0%, ${T.midnight} 60%); }
.stage > canvas { position: absolute; inset: 0; }
.jp { font-family: 'Noto Sans CJK JP', 'Noto Sans CJK TC', 'Source Han Sans CN', sans-serif; }
.mono { font-family: 'JetBrains Mono', monospace; }
.glass { position: absolute; border-radius: 16px; background: rgba(14,19,31,.82); backdrop-filter: blur(28px) saturate(140%);
  border: 1px solid rgba(170,200,255,.17); box-shadow: inset 0 1px 0 rgba(255,255,255,.07), 0 24px 50px -18px rgba(0,0,0,.72), 0 2px 8px rgba(0,0,0,.28); }
.bar { position: absolute; left: 0; right: 0; top: 0; height: 34px; display: flex; align-items: center; justify-content: space-between; padding: 0 8px;
  background: rgba(10,14,23,.55); backdrop-filter: blur(24px); border-bottom: 1px solid rgba(150,185,255,.10); font-size: 12.5px; font-weight: 500; }
.bar .side { display: flex; align-items: center; gap: 4px; min-width: 260px; }
.bar .right { justify-content: flex-end; }
.bar .title { color: ${T.ink2}; font-size: 12px; }
.ws { width: 22px; height: 22px; border-radius: 8px; display: grid; place-items: center; color: ${T.ink3}; font-size: 11.5px; }
.ws.used { color: ${T.ink}; }
.ws.on { width: 34px; background: ${T.starlight}; color: ${T.midnight}; font-weight: 600; box-shadow: 0 0 14px rgba(139,190,255,.45); }
.item { height: 24px; padding: 0 7px; border-radius: 8px; display: flex; align-items: center; gap: 5px; color: ${T.ink}; }
.item b { font-weight: 500; }
.item.clock { padding-right: 4px; letter-spacing: .1px; }
.pop { width: 360px; padding: 14px; display: flex; flex-direction: column; gap: 10px; }
.tiles { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
.tile { height: 58px; border-radius: 12px; background: rgba(232,241,255,.055); display: flex; align-items: center; gap: 10px; padding: 0 10px; }
.tile .ti { width: 34px; height: 34px; border-radius: 10px; display: grid; place-items: center; background: rgba(232,241,255,.09); color: ${T.ink}; flex: none; }
.tile.lit .ti { background: ${T.starlight}; color: ${T.midnight}; box-shadow: 0 0 16px rgba(139,190,255,.4); }
.tile b { display: block; white-space: nowrap; font-size: 12.5px; font-weight: 600; }
.tile i { display: block; font-style: normal; font-size: 11.5px; color: ${T.ink2}; margin-top: 1px; white-space: nowrap; }
.slider { height: 30px; border-radius: 10px; background: rgba(232,241,255,.08); overflow: hidden; }
.slider .fill { height: 100%; background: ${T.ink}; border-radius: 10px; display: flex; align-items: center; padding-left: 9px; color: ${T.midnight}; }
.media { display: flex; align-items: center; gap: 10px; padding: 8px; border-radius: 12px; background: rgba(232,241,255,.055); }
.art { width: 40px; height: 40px; border-radius: 8px; background: linear-gradient(140deg, #3B2A6E, #8BBEFF 55%, #FFC58B); flex: none; }
.mt { flex: 1; } .mt b { display: block; font-size: 12.5px; font-weight: 600; } .mt i { display: block; font-style: normal; font-size: 11.5px; color: ${T.ink2}; margin-top: 1px; }
.mc { display: flex; gap: 12px; align-items: center; color: ${T.ink}; padding-right: 4px; }
.toast { width: 336px; padding: 12px 12px 14px; display: flex; gap: 11px; overflow: hidden; }
.tic { width: 34px; height: 34px; border-radius: 10px; background: rgba(143,227,207,.16); color: ${T.aurora}; display: grid; place-items: center; flex: none; }
.tb { flex: 1; } .th { display: flex; justify-content: space-between; font-size: 11px; color: ${T.ink2}; }
.tb b { display: block; font-size: 13px; font-weight: 600; margin-top: 2px; } .tb i { display: block; font-style: normal; font-size: 12px; color: ${T.ink2}; margin-top: 2px; }
.tp { position: absolute; left: 12px; bottom: 6px; height: 2px; border-radius: 2px; background: ${T.starlight}; opacity: .8; max-width: calc(100% - 24px); }
.wordmark { font-weight: 600; letter-spacing: -0.035em; }
.horizon { position: absolute; border-radius: 50%; background: radial-gradient(closest-side, #0D1424 92%, #101a30 97%, transparent 100%);
  box-shadow: 0 -1px 0 rgba(170,205,255,.55), 0 -10px 40px rgba(139,190,255,.28), 0 -60px 160px rgba(139,190,255,.12); }
`;

function page(build) {
  document.head.insertAdjacentHTML('beforeend', `<style>${CSS}</style>`);
  build();
  for (const c of document.querySelectorAll('canvas[data-sky]')) sky(c, JSON.parse(c.dataset.sky));
  document.fonts.ready.then(() => document.body.setAttribute('data-ready', '1'));
}
