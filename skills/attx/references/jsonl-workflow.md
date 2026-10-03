# 通用 JSONL 流程

用于外部提取器、离线审校、跨工具交换及二进制引擎的文本出口。attx 不会自动获得未知引擎的二进制写回能力。

## 记录格式

```json
{"id":"scene1:55","text":"改札でごった返す人混みの中、…","context":"00_op_000","role":"ヒロイン","item_type":"long_text"}
```

`id` 和 `text` 必填。ID 是稳定位置键；多行原文使用 `\n`。`context`、`role`、`item_type` 可选，类型包括 `long_text`、`array`、`short_text`。输出保留原文身份，增加 `translation`、`translation_lines`。

源文记录不能重复 ID、混入另一版本文本或用译文覆盖 `text`。控制符和变量保持不变，外部写回工具也必须校验 ID 与源文。

## 无工作区直译

```bash
attx translate-jsonl --input source.jsonl --output translated.jsonl --src ja --dst zh
```

该方式把结果交给已知、安全的外部写回器。不要自动执行模型生成的提取或写回脚本。若需要可恢复的缓存和 attx 的 review/repair 流程，使用 JSONL 工作区。

## JSONL 工作区

目录模式将 `source.jsonl` 放在输入目录，或直接使用 JSONL 文件：

```bash
attx run --input <输入> --engine jsonl --src ja --dst zh
```

目录输入输出 `translated.jsonl`；JSONL 文件输入输出 `<stem>.<目标语言>.jsonl` 副本。检查最终状态和 pending/passthrough，不把只输出一部分当成完整翻译。可接受部分输出时由用户明确授权 `--allow-partial`。

## 导出、校对和导入

```bash
attx export-jsonl --workspace <工作区> --output review.jsonl --filter all
# 保留 id 和 text，只修改 translation_lines 或 translation
attx import-jsonl --workspace <工作区> --input fix.jsonl
attx review --workspace <工作区>
attx writeback --workspace <工作区>
```

导出 filter 为 `pending|all|translated|passthrough`。工作区导出 `id` 对应单元 location，不要自行换成数据库内部键。

导入先验证全部记录，再统一保存。未知位置、源文不符、非空译文的结构/控制符错误或同一单元的重复记录使导入整体失败；location 和内部 unit ID 指向同一单元时也算重复。审校导出中的空/缺失译文跳过，不计入 imported。先重新导出当前记录再修正，不修改原文字段绕过检查。中文机械边缘残留清理后入库，mixed/untranslated 残留保留为后续修复项；默认写回仍拦截未解决项。RMMZ 布局处理也同步缓存，再次导出应基于同步后的数据。

正常 writeback 已包含在命名输入的翻译授权中。默认完整性要求仍然有效，不能因导入少量修正就擅自放行其他未解决条目。JSONL 或日志不得含 API Key。
