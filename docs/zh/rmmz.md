# RPG Maker MV/MZ

引擎 id：`rmmz`。输入为**游戏目录**（`data/`，通常还有 `js/`）。

## 流程

```bash
attx detect  --input /path/to/game
attx init    --input /path/to/game --src ja --dst zh
attx extract --workspace /path/to/game/.attx
attx translate --workspace /path/to/game/.attx
attx writeback --workspace /path/to/game/.attx --dry-run
attx writeback --workspace /path/to/game/.attx
```

写回为**原地修改**，并在被改文件旁生成 `*.attxbak`。

## 域（domain）

| domain | 来源 |
|--------|------|
| `dialogue` | 对话框正文（`101` 后的 `401`） |
| `namebox` | MZ 名牌 — `101` `parameters[4]`（0.7+） |
| `choices` | 选项（`102`） |
| `scroll` | 滚动文字（`405`） |
| `system` | `System.json` |
| `base` | 角色/物品/技能等数据库 |
| `plugins` | 仅 `js/plugins.js` 参数（不改 `js/plugins/*.js`） |

名牌值为 `\N[n]` 时**不提取**（运行时用 Actors 名）。

## 对白重排（0.7+）

模型常返回一整行长译文，而事件仍有 3 个 `401` 槽。写回按**显示宽度**（半角≈1，CJK≈2，控制符≈0）折成恰好 `n` 行；`\C[27]` 等不会被从中间切断。

默认最大宽度 **44**。行数已对齐且未超宽时保持原样。

## 插件参数

- 以 **JSON 字符串** 多层嵌套的参数：抽取时解码，写回时再编码
- 参数名可含 `/`（如 `行動-表示位置/味方`）；location 保留全名，payload 带 `plugin_index` / `param` / `json_path`
- 身份/路径字段（`key`、`FilePath` 等）跳过

## 建议

1. 先试译 20 条进游戏看效果，再全量
2. 首次写回前用 `--dry-run`
3. 插件异常时恢复 `js/plugins.js.attxbak`，并带上插件名与 location 提 issue
