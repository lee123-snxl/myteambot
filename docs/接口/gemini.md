# Gemini

对话是 `POST .../models/{模型}:generateContent`。密钥放在查询参数 `key`，不放进正文。

助手角色写成 `model`。系统提示词放在 `systemInstruction`。模型列表里的 `models/` 前缀会去掉。
