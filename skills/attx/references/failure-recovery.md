# 失败恢复

## 配置 / 鉴权

| 症状 | 处理 |
|------|------|
| `llm client not found` | 创建/修正 `setting.toml`，保证 `default_client` 与 `[[llm.clients]].name` 一致 |
| `HTTP 401` / Invalid token | 用户更新 api_key；Agent **停止重试**，不要循环刷接口 |
| `HTTP 404` model | 核对 `model` 名称与服务商文档 |
| 超时 | 增大 `timeout`；减小 `batch_chars`；降低 `rpm` |

## 引擎 / 提取

| 症状 | 处理 |
|------|------|
| detect 失败 | 确认目录含 `data/System.json`；或 `--engine rmmz` 强制；或走 JSONL |
| extracted = 0 | 检查是否译过已是中文、源语言是否选错（en/ja）、路径是否指到 www 子目录 |
| 编码乱码 | 确认 JSON 为 UTF-8 |

## 翻译

| 症状 | 处理 |
|------|------|
| 模型返回非 JSON | 自动会重试；持续失败则换模型或减小 batch |
| quality failed / 控制符丢失 | 导出该条 JSONL 人工修，`import-jsonl` |
| 部分成功 partial | `status` 看 pending；再 `translate` 续跑 |
| 全量太贵/太慢 | `--limit` 分批；调低 `rpm`/`worker` 相关配置 |

## 写回

| 症状 | 处理 |
|------|------|
| units_applied=0 | 没有已保存译文；先 translate/import |
| 游戏打不开 | 用同目录 `*.attxbak` 还原对应文件 |
| 只想撤销 | 从 `.attxbak` 拷回；或用原版游戏覆盖 data |

还原示例：

```bash
cp data/Map001.json.attxbak data/Map001.json
```

## 工作区损坏

```bash
# 用户明确同意后：删除工作区并重新 init/extract
rm -rf <工作区>
attx init --game <游戏目录> --src ja --dst zh --workspace <工作区>
attx extract --workspace <工作区>
```

已写回的游戏文件不会自动回滚；需要 bak 或原版。
