# 试玩和审阅反馈

在用户命名的任务范围内修正译文并正常写回，不另设写回许可关卡。优先单条修正，避免无证据重译全部已通过条目。

## 先看已有证据

```bash
attx status --workspace <工作区>
attx review --workspace <工作区>
```

读取 `residual_source`、`kana_edge`、`kana_mixed`、`kana_untranslated`、`identical`、`control_loss`、`namebox_mismatch` 及 `glossary.violations`。每类 sample 有上限，不能把样例数当全量数量。

已有截图、原文、位置或审校样例够用就自行定位。只在缺少定位信息时问场景、地图、角色、原文片段或具体显示问题，不要求用户重新提供工具已有的数据。

## 定位与修复

1. `export-jsonl --filter all` 导出并查找原文或 location；自动修复适用时先 `repair`，遵守本次修复上限。
2. 人工修正只修改译文字段，保留原 `id`、`text`、控制符和角色关系。
3. `import-jsonl` 校验源文身份和译文，再 `review`、`writeback`。
4. 未命中时检查提取覆盖，不自动修改输入文件。图片字、二进制资源和不支持的插件结构应说明具体限制，必要时走外部提取器或 JSONL。

```bash
attx export-jsonl --workspace <工作区> --output review.jsonl --filter all
# 校对需要修改的 translation_lines，保留 id 和 text
attx import-jsonl --workspace <工作区> --input fix.jsonl
attx review --workspace <工作区>
attx writeback --workspace <工作区>
```

导入拒绝过期源文或未知 ID 时重新导出定位，不能改 `text` 冒充当前源文。默认完整性拦截仍生效；只有用户接受剩余未解决问题时才可 `writeback --allow-partial`。

RMMZ 中文规范化和固定对白槽位重排同步缓存后再写回。显示溢出和真实残留需要分别处理，不能直接改游戏输出而留下旧缓存，也不能删掉控制符或挤掉其他指令以强行适配。

## 闭环

报告修改范围、剩余审校命中、真实输出路径及是否写回。运行时显示需要用户在同一场景确认，机械审校不能替代试玩。若纠正形成具体、可复用且符合用户要求的规则，可写作品内 note：

```bash
attx learn note --workspace <工作区> --name <短名> --text "<一条具体要求>"
```

专有名词用 glossary。提取 skip 提案交用户审阅，不 `--approve-all`，不通过缩小提取范围隐藏漏翻。
