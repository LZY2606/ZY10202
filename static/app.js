'use strict';
const $ = (id) => document.getElementById(id);
const state = {
  images: [],
  image: null,
  pixels: null,
  imgW: 0, imgH: 0,
  scale: 1,
  roi: { x0: 0, y0: 0, w: 0, h: 0, rotation: 0 },
  drag: null,
  runs: [],
  result: null,
  funit: 'cpp',
};

async function api(path, opts) {
  const res = await fetch('/api' + path, opts || {});
  if (res.status === 204) return null;
  const ct = res.headers.get('content-type') || '';
  const body = ct.includes('json') ? await res.json() : await res.text();
  if (!res.ok) throw new Error(body && body.error ? body.error : body);
  return body;
}
const fmt = (x, n = 4) => (x === null || x === undefined || Number.isNaN(x)) ? '—' : Number(x).toFixed(n);
const f3 = (x) => fmt(x, 3);

async function loadImages() {
  state.images = await api('/images');
  const sel = $('image-select');
  sel.innerHTML = '';
  for (const im of state.images) {
    const o = document.createElement('option');
    o.value = im.key;
    o.textContent = im.title;
    sel.appendChild(o);
  }
  if (state.images.length) sel.value = state.images[0].key;
  await selectImage();
}

async function selectImage() {
  const key = $('image-select').value;
  state.image = state.images.find(i => i.key === key);
  const res = await fetch('/api/images/' + encodeURIComponent(key) + '/pixels');
  state.imgW = parseInt(res.headers.get('X-Image-Width'), 10);
  state.imgH = parseInt(res.headers.get('X-Image-Height'), 10);
  state.pixels = new Uint8Array(await res.arrayBuffer());
  const im = state.image;
  const bands = im.pitch_bands.map((b, i) =>
    `第${i + 1}段 行${b.y0}..${b.y1}: ${b.pitch_um === null ? '未知' : b.pitch_um + 'µm'}`).join('；');
  $('image-meta').innerHTML =
    `<div>${state.imgW}×${state.imgH} · 校验和 <code>${im.pixel_checksum}</code></div>
     <div>${bands}</div><div>${im.note}</div>`;
  state.roi = { x0: 0, y0: 0, w: state.imgW, h: state.imgH, rotation: 0 };
  syncRoiInputs();
  drawImage();
  drawRoi();
  await loadRuns();
}

function pxVal(x, y) {
  return state.pixels[y * state.imgW + x];
}

function drawImage() {
  const cv = $('image-canvas');
  const pad = 8;
  const sx = (cv.width - 2 * pad) / state.imgW;
  const sy = (cv.height - 2 * pad) / state.imgH;
  state.scale = Math.min(sx, sy);
  state.offx = (cv.width - state.imgW * state.scale) / 2;
  state.offy = (cv.height - state.imgH * state.scale) / 2;
  const ctx = cv.getContext('2d');
  ctx.clearRect(0, 0, cv.width, cv.height);
  // blocky grayscale rects keep each pixel explicit
  for (let y = 0; y < state.imgH; y++) {
    for (let x = 0; x < state.imgW; x++) {
      const v = pxVal(x, y);
      ctx.fillStyle = `rgb(${v},${v},${v})`;
      ctx.fillRect(state.offx + x * state.scale, state.offy + y * state.scale,
        Math.ceil(state.scale), Math.ceil(state.scale));
    }
  }
}

function imgToCanvas(x, y) {
  return [state.offx + x * state.scale, state.offy + y * state.scale];
}
function canvasToImg(cx, cy) {
  return [Math.floor((cx - state.offx) / state.scale), Math.floor((cy - state.offy) / state.scale)];
}

function drawRoi() {
  const cv = $('roi-canvas');
  const ctx = cv.getContext('2d');
  ctx.clearRect(0, 0, cv.width, cv.height);
  const r = state.roi;
  if (r.w === 0 || r.h === 0) return;
  const [x, y] = imgToCanvas(r.x0, r.y0);
  const w = r.w * state.scale, h = r.h * state.scale;
  ctx.strokeStyle = '#5aa2ff';
  ctx.lineWidth = 2;
  ctx.setLineDash([6, 4]);
  ctx.strokeRect(x, y, w, h);
  ctx.setLineDash([]);
  // rotation marker: arrow showing local +u,+x axis after rotation
  ctx.fillStyle = '#5aa2ff';
  ctx.font = '11px sans-serif';
  ctx.fillText(`${r.rotation}° ROI ${r.w}×${r.h}`, x + 4, Math.max(12, y - 4));
  // excluded rows shading in local frame
  const ex = parseExcluded();
  ctx.fillStyle = 'rgba(239,107,107,0.28)';
  for (const v of ex) {
    const p = localRect(r, v);
    if (!p) continue;
    const [rx, ry] = imgToCanvas(p.x, p.y);
    ctx.fillRect(rx, ry, p.w * state.scale, p.h * state.scale);
  }
}

// For a local row v, return the original-image rectangle it occupies
// (only useful as a shading hint; rows are 1px lines pre-rotation).
function localRect(r, v) {
  const lw = (r.rotation === 90 || r.rotation === 270) ? r.h : r.w;
  const lh = (r.rotation === 90 || r.rotation === 270) ? r.w : r.h;
  if (v >= lh) return null;
  // sample both ends of the local row to get its bounding span
  const pts = [];
  for (const u of [0, lw - 1]) pts.push(mapLocal(r, u, v));
  const xs = pts.map(p => p[0]), ys = pts.map(p => p[1]);
  return { x: Math.min(...xs), y: Math.min(...ys), w: 1, h: 1 };
}

function mapLocal(r, u, v) {
  switch (r.rotation) {
    case 90: return [r.x0 + r.w - 1 - v, r.y0 + u];
    case 180: return [r.x0 + r.w - 1 - u, r.y0 + r.h - 1 - v];
    case 270: return [r.x0 + v, r.y0 + r.h - 1 - u];
    default: return [r.x0 + u, r.y0 + v];
  }
}

function syncRoiInputs() {
  $('p-x0').value = state.roi.x0;
  $('p-y0').value = state.roi.y0;
  $('p-w').value = state.roi.w;
  $('p-h').value = state.roi.h;
  $('p-rot').value = String(state.roi.rotation);
}
function readRoiInputs() {
  state.roi = {
    x0: clampInt($('p-x0').value, 0, state.imgW - 1),
    y0: clampInt($('p-y0').value, 0, state.imgH - 1),
    w: clampInt($('p-w').value, 1, state.imgW),
    h: clampInt($('p-h').value, 1, state.imgH),
    rotation: parseInt($('p-rot').value, 10),
  };
  // clamp into image
  state.roi.w = Math.min(state.roi.w, state.imgW - state.roi.x0);
  state.roi.h = Math.min(state.roi.h, state.imgH - state.roi.y0);
  syncRoiInputs();
}
function clampInt(v, lo, hi) {
  v = parseInt(v, 10);
  if (Number.isNaN(v)) v = lo;
  return Math.max(lo, Math.min(hi, v));
}
function parseExcluded() {
  return $('p-exclude').value.split(/[,\s]+/).map(s => parseInt(s, 10))
    .filter(n => !Number.isNaN(n));
}

function setupCanvasDrag() {
  const cv = $('roi-canvas');
  cv.addEventListener('pointerdown', (e) => {
    const rect = cv.getBoundingClientRect();
    const cx = (e.clientX - rect.left) * cv.width / rect.width;
    const cy = (e.clientY - rect.top) * cv.height / rect.height;
    const [x, y] = canvasToImg(cx, cy);
    if (x < 0 || y < 0 || x >= state.imgW || y >= state.imgH) return;
    state.drag = { x0: x, y0: y, x1: x, y1: y };
  });
  cv.addEventListener('pointermove', (e) => {
    if (!state.drag) return;
    const rect = cv.getBoundingClientRect();
    const cx = (e.clientX - rect.left) * cv.width / rect.width;
    const cy = (e.clientY - rect.top) * cv.height / rect.height;
    let [x, y] = canvasToImg(cx, cy);
    x = Math.max(0, Math.min(state.imgW - 1, x));
    y = Math.max(0, Math.min(state.imgH - 1, y));
    state.drag.x1 = x; state.drag.y1 = y;
    const d = state.drag;
    state.roi = {
      x0: Math.min(d.x0, d.x1), y0: Math.min(d.y0, d.y1),
      w: Math.abs(d.x1 - d.x0) + 1, h: Math.abs(d.y1 - d.y0) + 1,
      rotation: state.roi.rotation,
    };
    syncRoiInputs();
    drawRoi();
  });
  window.addEventListener('pointerup', () => { state.drag = null; });
}

function gatherParams() {
  readRoiInputs();
  const mode = $('p-pitchmode').value;
  return {
    roi: {
      x0: state.roi.x0, y0: state.roi.y0, w: state.roi.w, h: state.roi.h,
      rotation: parseInt($('p-rot').value, 10),
    },
    excluded_rows: parseExcluded(),
    supersample: parseInt($('p-ss').value, 10),
    derivative: $('p-deriv').value,
    pitch_mode: mode,
    manual_pitch_um: mode === 'manual' ? parseFloat($('p-pitch').value) : null,
    window_half_bins: parseInt($('p-half').value, 10),
    spectral_window: $('p-window').value,
  };
}

async function runAnalysis() {
  $('param-error').textContent = '';
  let params;
  try { params = gatherParams(); } catch (e) { $('param-error').textContent = e.message; return; }
  try {
    const body = {
      image_key: $('image-select').value,
      label: $('run-label').value || '未命名方案',
      params,
    };
    const created = await api('/runs', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    $('run-label').value = '';
    state.result = created.result;
    renderResult(created.result);
    await loadRuns();
    await loadLogs();
  } catch (e) {
    $('param-error').textContent = e.message;
  }
}

async function loadRuns() {
  const key = $('image-select').value;
  state.runs = await api('/runs?image_key=' + encodeURIComponent(key));
  const tb = document.querySelector('#runs-table tbody');
  tb.innerHTML = '';
  for (const r of state.runs) {
    const tr = document.createElement('tr');
    tr.innerHTML = `<td>${r.id}</td><td>${escapeHtml(r.label)}</td>
      <td><code>${r.fingerprint}</code></td><td class="run-m50">…</td>
      <td><button data-id="${r.id}" class="sel">查看</button>
          <button data-id="${r.id}" class="del danger">删</button></td>`;
    tr.querySelector('.sel').onclick = () => openRun(r.id);
    tr.querySelector('.del').onclick = async () => {
      await api('/runs/' + r.id, { method: 'DELETE' });
      await loadRuns(); await loadLogs();
    };
    tb.appendChild(tr);
    // fill m50 lazily
    api('/runs/' + r.id).then(detail => {
      const m50 = detail.result.mtf50;
      tr.querySelector('.run-m50').textContent = m50 ? f3(m50.f_cpp) + ' c/p' : '无交点';
    });
  }
}

async function openRun(id) {
  const detail = await api('/runs/' + id);
  state.result = detail.result;
  renderResult(detail.result);
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
}

function renderResult(r) {
  const pitchTxt = r.pitch.pitch_um === null ? '未知（只显示 c/p）' : r.pitch.pitch_um + ' µm';
  const m50 = r.mtf50;
  $('summary').innerHTML = `
    <div class="card"><div class="k">参数指纹</div><div class="v" style="font-size:12px"><code>${r.params_fingerprint}</code></div></div>
    <div class="card"><div class="k">MTF50（首个下穿）</div>
      <div class="v">${m50 ? f3(m50.f_cpp) + ' <small>c/p</small>' : '—'}</div>
      <div class="k">${m50 && m50.f_lpmm !== null && m50.f_lpmm !== undefined ? f3(m50.f_lpmm) + ' lp/mm' : 'lp/mm 不可用'}</div></div>
    <div class="card"><div class="k">边缘倾角 / RMSE</div>
      <div class="v">${f3(r.fit.angle_deg)}°</div><div class="k">残差 RMSE ${f3(r.fit.rmse_px)}px · 最大 ${f3(r.fit.max_abs_residual_px)}px</div></div>
    <div class="card"><div class="k">LSF 重心</div>
      <div class="v">${f3(r.centroid.centroid_px)} <small>px</small></div>
      <div class="k">峰位 ${f3(r.centroid.peak_px)}px · FWHM ${r.centroid.fwhm_px === null ? '—' : f3(r.centroid.fwhm_px) + 'px'}</div></div>
    <div class="card"><div class="k">Nyquist 行为</div>
      <div class="v">${f3(r.nyquist.mtf_at_nyquist)}</div>
      <div class="k">0.5c/p；0.4–0.5 均值 ${f3(r.nyquist.mean_04_05)}；0.5–0.6 ${f3(r.nyquist.mean_05_06)}${r.nyquist.rises_after_nyquist ? ' · 过 Nyquist 后抬升⚠' : ''}</div></div>
    <div class="card"><div class="k">ESF 箱 / 空箱</div>
      <div class="v">${r.bins.length} <small>箱</small></div><div class="k">空箱 ${r.n_empty_bins} 保留为 null · 像素 ${r.n_pixels}</div></div>
    <div class="card"><div class="k">像素间距</div><div class="v" style="font-size:13px">${pitchTxt}</div><div class="k">${escapeHtml(r.pitch.note)}</div></div>
    <div class="card"><div class="k">DFT 窗口</div><div class="v" style="font-size:12px">${escapeHtml(r.df_window)}</div></div>`;

  const canLpmm = r.mtf.length && r.mtf[0].f_lpmm !== null && r.mtf[0].f_lpmm !== undefined;
  $('funit-lpmm-wrap').classList.toggle('hidden', !canLpmm);
  if (!canLpmm && state.funit === 'lpmm') { state.funit = 'cpp'; }
  document.querySelector('input[name=funit][value=cpp]').checked = state.funit === 'cpp';
  $('funit-note').textContent = canLpmm
    ? '' : '该方案像素间距未知/横跨双段，仅显示 cycles/pixel，绝不猜测 lp/mm';
  drawMtf(r);
  drawLsf(r);
  drawEsf(r);

  // crossings list with explicit rule
  const other = r.crossings.filter((_, i) => !(i === 0 && r.mtf50 && r.crossings[0] === r.mtf50));
  const lines = r.crossings.map((c, i) => {
    const isSel = r.mtf50 && c === r.mtf50;
    const unit = (v) => v === null || v === undefined ? '' : ` / ${f3(v)} lp/mm`;
    const arrow = c.direction === 'down' ? '↓下穿' : '↑上穿';
    return `<span class="${c.direction === 'down' ? 'cross-down' : 'cross-up'}">${isSel ? '★选定 ' : ''}${arrow} @ ${f3(c.f_cpp)} c/p${unit(c.f_lpmm)}（波瓣${c.lobe}）</span>`;
  });
  $('mtf-crossings').innerHTML =
    `<b>0.5 交点选取规则：</b>${escapeHtml(r.crossing_rule)}<br>全部交点（共 ${r.crossings.length} 个）：${lines.join('　') || '无'}`;

  // fit info
  $('fit-info').innerHTML = `edge_along(line) = ${f3(r.fit.intercept)} ${r.fit.slope >= 0 ? '+' : '−'} ${f3(Math.abs(r.fit.slope))}·line；参与拟合行 ${r.fit.n_rows}；坐标均为像素中心（整数+0.5）。`;

  // rows table
  const rt = document.querySelector('#rows-table tbody');
  rt.innerHTML = '';
  const residMap = {};
  for (const x of r.fit.residuals) residMap[x.row] = x.residual_px;
  for (const row of r.rows) {
    const tr = document.createElement('tr');
    const res = residMap[row.row];
    tr.innerHTML = `<td>${row.row}</td>
      <td>${row.detected_edge_u === null ? '—' : f3(row.detected_edge_u)}</td>
      <td>${f3(row.low)}</td><td>${f3(row.high)}</td><td>${f3(row.contrast)}</td>
      <td>${res === undefined ? '—' : f3(res)}</td>
      <td>${row.included ? '<span class="pill in">参与</span>' : '<span class="pill out">排除：' + escapeHtml(row.reason) + '</span>'}</td>`;
    rt.appendChild(tr);
  }
  // pixels table (cap 400 rows for rendering; full data available via /runs/:id)
  const pt = document.querySelector('#pixels-table tbody');
  pt.innerHTML = '';
  const cap = 400;
  for (const s of r.pixels.slice(0, cap)) {
    const tr = document.createElement('tr');
    const fracOk = Math.abs((s.center_x % 1) - 0.5) < 1e-9 && Math.abs((s.center_y % 1) - 0.5) < 1e-9;
    tr.innerHTML = `<td>(${s.u},${s.v})</td><td>(${s.ox},${s.oy})</td>
      <td>${fmt(s.center_x, 1)}, ${fmt(s.center_y, 1)} ${fracOk ? '✓' : '✗'}</td>
      <td>${f3(s.value01)}</td><td>${f3(s.signed_distance_px)}</td><td>${s.bin_index}</td>`;
    pt.appendChild(tr);
  }
  if (r.pixels.length > cap) {
    const tr = document.createElement('tr');
    tr.innerHTML = `<td colspan="6">…其余 ${r.pixels.length - cap} 个像素见 <code>GET /api/runs/:id</code> 完整证据…</td>`;
    pt.appendChild(tr);
  }
}

function chartFrame(cv, xmax, ymax, xlabel, ylabel) {
  const ctx = cv.getContext('2d');
  const W = cv.width, H = cv.height;
  ctx.clearRect(0, 0, W, H);
  const pad = { l: 46, r: 12, t: 12, b: 30 };
  const plotW = W - pad.l - pad.r, plotH = H - pad.t - pad.b;
  ctx.strokeStyle = '#28324a'; ctx.fillStyle = '#93a0b8';
  ctx.lineWidth = 1; ctx.font = '10px sans-serif';
  for (let i = 0; i <= 5; i++) {
    const gy = pad.t + plotH * i / 5;
    ctx.beginPath(); ctx.moveTo(pad.l, gy); ctx.lineTo(W - pad.r, gy); ctx.stroke();
    ctx.fillText((ymax * (1 - i / 5)).toFixed(2), 6, gy + 3);
  }
  for (let i = 0; i <= 6; i++) {
    const gx = pad.l + plotW * i / 6;
    ctx.fillText((xmax * i / 6).toFixed(2), gx - 10, H - 10);
  }
  ctx.fillText(xlabel, W / 2 - 30, H - 0);
  ctx.save();
  ctx.translate(12, H / 2); ctx.rotate(-Math.PI / 2); ctx.fillText(ylabel, -20, 0);
  ctx.restore();
  return { ctx, pad, plotW, plotH, W, H, xmax, ymax };
}

function xy(f, pt) {
  return [f.pad.l + (pt.x / f.xmax) * f.plotW,
          f.pad.t + (1 - Math.min(1, pt.y / f.ymax)) * f.plotH];
}

function freqX(r, p) {
  return state.funit === 'lpmm' && p.f_lpmm !== null && p.f_lpmm !== undefined ? p.f_lpmm : p.f_cpp;
}
function freqLabel() {
  return state.funit === 'lpmm' ? 'lp/mm' : 'cycles/pixel';
}

function drawMtf(r) {
  const cv = $('chart-mtf');
  if (!r.mtf || !r.mtf.length) return;
  const canLpmm = r.mtf[0].f_lpmm !== null && r.mtf[0].f_lpmm !== undefined;
  let xmax;
  if (state.funit === 'lpmm' && canLpmm) {
    xmax = Math.max(...r.mtf.map(p => p.f_lpmm)) * 1.02;
  } else {
    xmax = 0.6;
  }
  const f = chartFrame(cv, xmax, 1.05, freqLabel(), 'MTF');
  // 0.5 line
  const [, hy] = xy(f, { x: 0, y: 0.5 });
  f.ctx.strokeStyle = '#e0b341'; f.ctx.setLineDash([5, 4]);
  f.ctx.beginPath(); f.ctx.moveTo(f.pad.l, hy); f.ctx.lineTo(f.W - f.pad.r, hy); f.ctx.stroke();
  f.ctx.setLineDash([]);
  // Nyquist marker only in c/p mode
  if (state.funit === 'cpp') {
    const nx = f.pad.l + (0.5 / 0.6) * f.plotW;
    f.ctx.strokeStyle = '#ef6b6b';
    f.ctx.beginPath(); f.ctx.moveTo(nx, f.pad.t); f.ctx.lineTo(nx, f.pad.t + f.plotH); f.ctx.stroke();
    f.ctx.fillStyle = '#ef6b6b';
    f.ctx.fillText('Nyquist 0.5', nx - 30, f.pad.t + 10);
  }
  // curve
  f.ctx.strokeStyle = '#5aa2ff'; f.ctx.lineWidth = 2;
  f.ctx.beginPath();
  r.mtf.forEach((p, i) => {
    const x = freqX(r, p);
    const [cx, cy] = xy(f, { x, y: p.mtf });
    i === 0 ? f.ctx.moveTo(cx, cy) : f.ctx.lineTo(cx, cy);
  });
  f.ctx.stroke();
  // crossings
  for (const c of r.crossings) {
    const x = state.funit === 'lpmm' && c.f_lpmm !== null && c.f_lpmm !== undefined ? c.f_lpmm : c.f_cpp;
    const [cx, cy] = xy(f, { x, y: 0.5 });
    f.ctx.fillStyle = c.direction === 'down' ? '#e0b341' : '#43c08a';
    f.ctx.beginPath(); f.ctx.arc(cx, cy, 4, 0, Math.PI * 2); f.ctx.fill();
  }
}

function drawLsf(r) {
  const cv = $('chart-lsf');
  const vals = r.lsf.map(p => p.value).filter(v => v !== null);
  const maxAbs = Math.max(...vals.map(v => Math.abs(v)), 0.02);
  const xmin = r.lsf[0].center_px, xmax = r.lsf[r.lsf.length - 1].center_px;
  const f = chartFrame(cv, xmax, maxAbs * 2, '相对边缘距离 (px)', 'LSF');
  const xx = (c) => f.pad.l + ((c - xmin) / (xmax - xmin)) * f.plotW;
  const zeroY = f.pad.t + f.plotH / 2;
  f.ctx.strokeStyle = '#28324a';
  f.ctx.beginPath(); f.ctx.moveTo(f.pad.l, zeroY); f.ctx.lineTo(f.W - f.pad.r, zeroY); f.ctx.stroke();
  // vertical gap markers where value is null
  f.ctx.strokeStyle = '#5aa2ff'; f.ctx.lineWidth = 1.6;
  f.ctx.beginPath();
  let started = false;
  for (const p of r.lsf) {
    const cx = xx(p.center_px);
    const cy = zeroY - (p.value || 0) / (2 * maxAbs) * f.plotH;
    if (p.value === null) { started = false; continue; }
    if (!started) { f.ctx.moveTo(cx, cy); started = true; } else f.ctx.lineTo(cx, cy);
  }
  f.ctx.stroke();
  f.ctx.fillStyle = 'rgba(239,107,107,0.35)';
  for (const p of r.lsf) {
    if (p.value === null) f.ctx.fillRect(xx(p.center_px) - 1, f.pad.t, 2, f.plotH);
  }
  // centroid
  const cxx = xx(r.centroid.centroid_px);
  f.ctx.strokeStyle = '#43c08a';
  f.ctx.beginPath(); f.ctx.moveTo(cxx, f.pad.t); f.ctx.lineTo(cxx, f.pad.t + f.plotH); f.ctx.stroke();
  f.ctx.fillStyle = '#43c08a';
  f.ctx.fillText('重心 ' + r.centroid.centroid_px.toFixed(3), cxx + 3, f.pad.t + 12);
}

function drawEsf(r) {
  const cv = $('chart-esf');
  const present = r.bins.filter(b => typeof b.value === 'number' && Number.isFinite(b.value));
  const vmin = Math.min(...present.map(b => b.value));
  const vmax = Math.max(...present.map(b => b.value));
  const xmin = r.bins[0].center_px, xmax = r.bins[r.bins.length - 1].center_px;
  const cmax = Math.max(...r.bins.map(b => b.count), 1);
  if (!Number.isFinite(vmin) || !Number.isFinite(xmax - xmin)) return;
  const f = chartFrame(cv, xmax, 1.0, '相对边缘距离 (px)', 'ESF (归一码值)');
  const xx = (c) => f.pad.l + ((c - xmin) / (xmax - xmin)) * f.plotW;
  const yy = (v) => f.pad.t + (1 - (v - 0) / 1.0) * f.plotH;
  // bars: sample count encoded by opacity
  for (const b of r.bins) {
    const cx = xx(b.center_px);
    if (b.value === null) {
      f.ctx.fillStyle = 'rgba(239,107,107,0.22)';
      f.ctx.fillRect(cx - 1, f.pad.t, 2, f.plotH);
    } else {
      const alpha = 0.25 + 0.55 * (b.count / cmax);
      f.ctx.fillStyle = `rgba(90,162,255,${alpha})`;
      const cy = yy(b.value);
      f.ctx.fillRect(cx - 1, cy, 2, f.pad.t + f.plotH - cy);
    }
  }
  f.ctx.strokeStyle = '#e8edf7'; f.ctx.lineWidth = 1.2;
  f.ctx.beginPath();
  let started = false;
  for (const b of r.bins) {
    if (b.value === null) { started = false; continue; }
    const cx = xx(b.center_px), cy = yy(b.value);
    if (!started) { f.ctx.moveTo(cx, cy); started = true; } else f.ctx.lineTo(cx, cy);
  }
  f.ctx.stroke();
  f.ctx.fillStyle = '#93a0b8';
  f.ctx.fillText(`数值范围 ${vmin.toFixed(3)}–${vmax.toFixed(3)}（原始码值 0..1）；颜色越亮样本越多`, f.pad.l, f.H - 2);
}

async function loadLogs() {
  const logs = await api('/logs');
  const tb = document.querySelector('#log-table tbody');
  tb.innerHTML = logs.map(l =>
    `<tr><td>${l.created_at}</td><td>${escapeHtml(l.action)}</td><td>${escapeHtml(l.detail)}</td></tr>`
  ).join('');
}

function bind() {
  $('image-select').addEventListener('change', selectImage);
  $('btn-refresh').onclick = loadRuns;
  $('btn-analyze').onclick = runAnalysis;
  $('p-rot').addEventListener('change', () => { readRoiInputs(); drawRoi(); });
  for (const id of ['p-x0', 'p-y0', 'p-w', 'p-h']) {
    $(id).addEventListener('change', () => { readRoiInputs(); drawRoi(); });
  }
  $('p-exclude').addEventListener('input', drawRoi);
  $('p-pitchmode').addEventListener('change', () => {
    $('manual-pitch-wrap').classList.toggle('hidden', $('p-pitchmode').value !== 'manual');
  });
  document.querySelectorAll('input[name=funit]').forEach(el =>
    el.addEventListener('change', () => {
      state.funit = document.querySelector('input[name=funit]:checked').value;
      if (state.result) drawMtf(state.result);
    }));
  $('btn-export').onclick = () => { window.location.href = '/api/export'; };
  $('file-import').addEventListener('change', async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    const data = await file.arrayBuffer();
    const rep = await api('/import', { method: 'POST', body: data });
    alert(rep.all_ok
      ? `导入并重放复核完成：图像 ${rep.imported_images}，方案 ${rep.imported_runs}，全部一致 ✓`
      : `复核存在问题：\n` + JSON.stringify(rep, null, 2));
    await loadImages(); await loadLogs();
    e.target.value = '';
  });
  $('btn-clear').onclick = async () => {
    if (!confirm('确定清空所有图像、方案与日志？之后可用“重新植入 fixture”或导入文件恢复。')) return;
    await api('/clear', { method: 'POST' });
    await loadImages(); await loadLogs();
  };
  $('btn-reseed').onclick = async () => {
    await api('/reseed', { method: 'POST' });
    await loadImages(); await loadLogs();
  };
}

(async function init() {
  bind();
  setupCanvasDrag();
  await loadImages();
  await loadLogs();
})();
