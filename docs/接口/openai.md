# OpenAI 兼容

kind 为 openai。对话是 `POST .../chat/completions`。模型列表是 `GET .../models`。

有密钥时加上 `Authorization: Bearer`。请求体含 model 和 messages，不传 max_tokens。

通义、智谱、DeepSeek、xAI、Groq、Mistral、OpenRouter、LM Studio、llama.cpp、vLLM 都用这一类。地址里已经有路径时，只追加 `/chat/completions`，不再插入一层 `/v1`。
