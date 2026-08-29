# 试玩反馈补漏

先跑机械审校，再按用户线索定位：

```bash
attx review --workspace <WS>
```

`residual_source` / `identical` / `control_loss` / `namebox_mismatch` / `glossary.violations` 的 `sample` 足够时直接 export 那些 location。

## 收集信息

向用户要（能要多少要多少）：

- 场景 / 地图名 / 角色
- 原文或截图中的残留日文/英文
- 现显示译文（若有）
- 是否系统菜单 / 对话 / 选项

## 定位流程

1. `export-jsonl --filter all` 或 `pending`，在 JSONL 里搜原文片段。
2. 命中则改 `translation_lines` → `import-jsonl` → `writeback`（需写回许可）。
3. 未命中：
   - 可能未提取（插件 UI、图片字、未支持域）→ 如实说明当前 rmmz 范围限制
   - 或可走 JSONL 外挂补丁

## 禁止

- 无证据扩大改写大量已通过条目  
- 直接手改 `data/*.json`  
- 为个别漏翻触发无必要全量重译（优先单条 import）

## 闭环

```bash
attx import-jsonl --workspace <WS> --input fix.jsonl
attx writeback --workspace <WS> --dry-run
# 用户允许
attx writeback --workspace <WS>
```

若这次修正是**可复用的规则**（敬称、人称、某类 UI 不译），立刻写成 note，免得下一章再犯：

```bash
attx learn note --workspace <WS> --name <短名> --text "<一条具体指令>"
```

请用户再玩同一场景确认。
