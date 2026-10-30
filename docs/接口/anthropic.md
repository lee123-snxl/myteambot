# Anthropic

对话是 `POST /v1/messages`。请求头有 `anthropic-version: 2023-06-01`。有密钥时加 `x-api-key`。

必须带 `max_tokens: 4096`。系统提示词放在单独的 system 字段，不放进 messages。

模型列表在 `/v1/models`。
