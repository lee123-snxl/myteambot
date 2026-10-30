# LocalBot

After using group-chat bot tools, I felt constrained: I couldn’t use arbitrary APIs, and I couldn’t customize a bot’s role in a more transparent way. So I decided to build one myself.

LocalBot is a desktop window on this computer. It does not sign in, and it does not use a remote machine. Providers, bots, chats, and groups are JSON files on this machine. The window title is「本机 Bot」.

## What it does

1. Add a model provider. The API type is OpenAI-compatible, Anthropic, Gemini, Ollama, or Azure. You fill in the base URL and, for a cloud service, the API key. Presets fill in common cloud and local addresses and leave a saved key in place. A provider can be for chat, image, speech, or video. Anthropic and Ollama are chat only.
2. Create a bot: a name, a system prompt, one provider, an optional model, and an optional folder. The prompt is plain text, saved with the bot.
3. Chat with one saved bot from the sidebar. If the bot has a folder, it can read files there and write text back into that folder.
4. Put bots in a group, up to 12. They reply in the order they joined, each with its own prompt, provider, and model. A group can point at a folder too. Image, speech, and video bots use their own prompt plus your latest message, and the generated files stay with that chat or group.

The window follows the system proxy. Local addresses connect directly.

## Run

Double-click `dist\LocalBot.exe`, or run `LocalBot.bat`. If the exe is missing, the bat file builds it first. The release build has no console window.

Data directory, in order:

1. `LOCALBOT_DATA`, when it is set and not empty.
2. A `data` folder next to the exe, when the program is not under `target`. `dist\LocalBot.exe` uses `dist\data`.
3. Otherwise `%LOCALAPPDATA%\LocalBot`.

## Build

Windows, Rust, and the Visual Studio 2022 C++ toolchain. From the repository root:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File build.ps1
```

The script runs tests, then a release build, and copies the executable to `dist\LocalBot.exe`.

## Docs

Behavior that is already implemented is written in Chinese under [docs/概述.md](docs/概述.md).
