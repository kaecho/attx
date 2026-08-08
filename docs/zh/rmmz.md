# RPG Maker MV/MZ

attx 是一个**通用**翻译框架 —— 本页只是 RMMZ 专属说明。主流程见 [用法](usage.md) 与 [Agent](agents.md)。

`rmmz` 适配器接收**游戏目录**。它会检测内容根目录 `./`、`www/` 或 `game/`（需要 `data/` 加上 `System.json` 或 `js/`）。

## 流水线

```bash
attx detect  --input /path/to/game
attx init    --input /path/to/game --src ja --dst zh      # workspace: /path/to/game/.attx
attx extract --workspace /path/to/game/.attx
attx translate --workspace /path/to/game/.attx
attx writeback --workspace /path/to/game/.attx --dry-run  # preview paths[]
attx writeback --workspace /path/to/game/.attx
```

**写回是原地进行**：`data/*.json` 与 `js/plugins.js` 在游戏目录中被重写，每个被覆盖的文件都会备份一次为 `*.attxbak`。务必先试运行并确认 —— agent 必须在真正写回前征得用户同意。

## 提取了什么

| domain | 来源 |
|--------|------|
| `dialogue` | 显示文字事件指令（`401`） |
| `namebox` | MZ 说话人名牌 —— 事件指令 `101` 的 `parameters[4]` |
| `choices` | 显示选项事件指令（`102`） |
| `scroll` | 滚动文字事件指令（`405`） |
| `system` | `System.json` —— 术语、消息、菜单 |
| `base` | 数据库名称 / 简介 / 描述（`Actors.json`、`Skills.json`、……） |
| `plugins` | `js/plugins.js` —— 只译插件 `@param` 值，**绝不**改插件源文件 |

`\N[n]` 名牌引用不会被提取。`data/*.json` 中被其他位置引用的名称会由学到的规则跳过（见内置经验中的 `plugins` 域：`attx learn defaults --format rmmz`）。

## 插件参数

- 只动 `js/plugins.js` —— `.js` 插件源文件永不修改。
- 值为嵌套 JSON 字符串的参数在提取时解码、写回时重新编码，因此嵌套结构可以完整往返。
- 包含 `/` 的参数名同样可以往返。
- 消息行会按显示宽度重新排版，回到原有的 `401` 槽位数量。

## 写回之后

本次运行学到的经验会自动应用到下一个项目（见 README 的 *自我改进的经验层*）。`*.attxbak` 文件可让你恢复翻译前的状态；它们已被 gitignore。
