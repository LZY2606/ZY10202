# 刃缘解像台 (Edge MTF Bench)

本地运行的倾斜黑白刃边（slanted-edge）MTF 分析台。对每个 ROI 完整保留从
**像素 → ESF → LSF → MTF** 的整条计算证据：旋转/裁切后的像素中心坐标、
逐行边缘检测与拟合残差、每个超采样箱的样本数（空箱保留为 `null`，绝不复制
邻箱）、LSF 重心、MTF50、Nyquist 附近行为，以及 0.5 电平的全部交点。

技术栈：Rust + Axum + SQLite（rusqlite bundled，无需系统 sqlite）+ 原生 Web UI。

## 安装与演示

```bash
cargo fetch --locked                 # 仅依赖，离线缓存可用
cargo test --locked                  # 20 个自动化测试
cargo run --locked -- --listen 127.0.0.1:5542
# 浏览器访问 http://127.0.0.1:5542 ，页首应显示“刃缘解像台”
```

可选参数：`--db edge_bench.db`（默认当前目录）。首次启动自动植入三个固定 fixture。

## 操作

1. 左上选择 fixture，在图像上**拖框**定义 ROI，或直接填 `x0,y0,w,h`。
2. 选择 ROI 旋转（0/90/180/270 顺时针）、超采样倍数 k、微分窗（3/5/7 点）、
   频谱窗（Hann / 矩形）、像素间距口径；在“排除污点行”里填局部行号。
3. “运行分析”后，右侧给出摘要卡、MTF/LSF/ESF 图、逐行拟合表与逐像素证据表。
4. 同一图像可保存任意多个方案；方案只用**参数指纹**（如
   `ss4xf5xw64vhpauto-2f9a1c8d3e`）区分，不使用“更平滑”这类主观描述。
5. 顶部“导出运行记录”下载单个 JSON；“清空数据库”后用“导入复核”即可
   重新导入，服务端会**用存储的像素与参数重新跑一遍分析**并比对 MTF50、
   箱数、空箱位置与指纹，报告每条记录是否逐位重放。

## 固定 fixture（确定性，字节级可复现）

固定随机种子 + 纯函数生成，任何机器重建得到同一图像；像素 FNV-1a64 校验和
显示在页面与 API：

| key | 尺寸 | 设计意图 |
|---|---|---|
| `bad_rows` | 96×72，pitch 5.0µm | 第 13 行全白、47/48 行全黑、60 行中灰无边缘；对比度不足自动排除，也可手工排除 |
| `dual_pitch` | 112×96 | 行 0..48 pitch=3.2µm，行 48..96 pitch=6.4µm；横跨两段时间距不确定，只显示 c/p |
| `ringing_edge` | 96×80，pitch 未知 | LSF 为主高斯瓣 + 偏置 4.6px 的弱负窄瓣（真实过冲），默认参数下 MTF 在约 0.397↓ / 0.417↑ / 0.469↓ 三次穿过 0.5 |

## 数据口径（重要）

**坐标 / 半像素约定**

- 像素 `(x,y)`（零基索引）的中心是 `(x+0.5, y+0.5)`，单位像素。
- ROI 是原图 `[x0,x0+w)×[y0,y0+h)` 的半开矩形。90/180/270 度旋转**只置换
  像素索引、不做任何重采样/插值**，因此每个局部像素中心仍精确落在某个
  `整数+0.5` 上（测试 `rotation_permutes_centers`、
  `rotated_square_roi_gives_same_mtf` 保证）。
- 边缘检测自动选择扫描轴：近竖直边缘（90/270 ROI）改沿列检测，再用极性
  校正让“有符号法向距离”恒为亮侧为正。方形 ROI 旋转前后 MTF50 一致。

**边缘拟合**

- 每行用 10%/90% 分位数估低/高电平，半幅电平线性插值得边缘中心；行
  对比度 < 0.15 判为污点行自动排除（页面给出原因）。
- 对参与行做 Theil–Sen 稳健直线拟合（中位斜率 + 中位截距），逐行给出
  残差、RMSE、最大绝对残差、倾角（`atan(slope)`，度）。

**ESF / 超采样箱**

- 像素按到拟合直线的**有符号垂直距离**投影；箱索引
  `k_idx = floor(d·k + 1/2)`，箱宽 `1/k` 像素，箱中心 `(k_idx+0.5)/k`。
- 箱值为落入像素的算术均值，并同时记录样本数。
- **没有像素落入的箱一律为 `null`（计数 0）**；LSF 微分模板只要触及空箱
  也产出 `null`，空箱在图中用红色缺口标记，绝不复制相邻箱。

**LSF**

- 三种微分核：3 点中心差分 `(f₊₁−f₋₁)/(2h)`；5 点
  `(−f₊₂+8f₊₁−8f₋₁+f₋₂)/(12h)`；7 点
  `(f₊₃−9f₊₂+45f₊₁−45f₋₁+9f₋₂−f₋₃)/(60h)`，其中 h=箱宽。
- 取 LSF 最大连续非空段，以峰为中心按 `window_half_bins` 截断，加 Hann
  窗（或矩形窗，保留真实旁瓣）后做 DFT。
- 重心按**带符号** LSF 计算（保留振铃负值信息），另给峰位与主瓣 FWHM。

**MTF / 频率单位**

- 内部频率单位恒为 **cycles/pixel**，Nyquist = 0.5；MTF 用 |DFT| 相对
  直流归一化。斜面投影距离已含 `1/sqrt(1+slope²)`，不需要再乘倾角因子。
- 仅当像素间距可确定时才换算 `lp/mm = f_cpp · 1000 / pitch_um`。
  pitch 未知（含手工选择“忽略”）或 ROI 横跨两段不同 pitch 时，
  **页面与 API 中所有 `f_lpmm` 都是 `null`**，绝不猜测。
- Nyquist 卡：给出 0.5 c/p 处插值 MTF、0.25 处值、0.4–0.5 与 0.5–0.6
  区间均值、过 Nyquist 后是否抬升及两区间比值（混叠提示）。

**MTF50 与多次穿过 0.5 的选取规则**

- 规则（页面在曲线下明示）：**MTF50 取主瓣沿频率升高方向第一次下穿
  0.5 的交点**，相邻频点间线性插值。
- 其余所有交点（包括后续“上穿/下穿”、所属旁瓣编号、方向、c/p 与
  lp/mm（若可用））全部按频率顺序列出。`ringing_edge` 的 0.417 c/p
  上穿点即明确标记为“旁瓣 1”，不会被误当作 MTF50。

## 参数指纹

`serde_json` 规范化序列化后取 FNV-1a64，前缀编码主要旋钮，形如
`ss{超采样}x{微分核}xw{窗半宽}v{窗}p{间距口径}-{哈希前10位}`。
相同参数指纹必定复现同一结果；任何旋钮变化都会产生不同指纹。

## HTTP 摘要

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/images` | 图像元数据（含 pitch 分段与校验和） |
| GET | `/api/images/:key/pixels` | 原始 u8 灰度像素（头里带宽高/校验和） |
| GET/POST | `/api/runs` | 列方案 / 建方案（body：image_key,label,params） |
| GET/DELETE | `/api/runs/:id` | 完整证据 JSON / 删除 |
| GET | `/api/logs` | 运行日志 |
| GET | `/api/export` · POST `/api/import` | 导出 / 清空并导入重放复核 |
| POST | `/api/clear` · `/api/reseed` | 清空库 / 重新植入 fixture |

## 清空后重新导入复核（验收路径）

```bash
curl -s localhost:5542/api/export -o run.json
curl -s -X POST localhost:5542/api/clear
curl -s -X POST localhost:5542/api/import --data-binary @run.json \
  -H 'Content-Type: application/json'
# 返回 all_ok=true 且每条 run 的 mtf50_delta=0、空箱位置一致
```

## 测试覆盖

`tests/geometry.rs`（半像素/旋转双射）、`tests/pipeline.rs`（坏行、空箱、
箱计数、双间距、未知 pitch、三次穿 0.5、超采样/微分核定量差异、旋转一致性、
指纹、Nyquist）、`tests/db.rs`（多方案指纹、导出/清空/导入重放）、
`tests/http.rs`（页面标题、建方案、参数 400、HTTP 级重放）。
