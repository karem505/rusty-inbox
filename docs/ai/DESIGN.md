# AI in Rusty Inbox: design and worker contract

Read this whole file before writing code. The backlog is `docs/ai/BACKLOG.md`; tick items there
(`[x]`) when they are done **and verified**.

## Owner decisions (settled, do not re-ask)
- Implement every backlog item (1–16 plus the A section). Target version 0.2.0.
- Providers: ChatGPT subscription (Codex sign-in), OpenAI (API key), Anthropic (API key), OpenRouter
  (API key), Ollama (local, or ollama.com with a key), Custom OpenAI-compatible endpoint.
- The model is **always** picked by the user from the provider's **live** list, with search. There is
  no hard-coded model catalog and no silent default model.
- A thinking level can be chosen (the levels the model supports, plus "Default").
- Backend in Rust. The UI keeps the app's language (English strings, `qsTr`) and its look: Theme
  colours, `Option`/`Toggle`/`Choice`/`Button2` building blocks in Settings, IconImage icons, short
  OutCubic motion (110–170 ms), no emoji, no new fonts.
- Privacy: nothing is sent unless the user turned AI on and picked a provider. Background features
  (11, 12, 13, 15, auto summary/replies/phishing) are off by default and their switch says that mail
  content is sent to the provider. Excluded accounts (`aiExcluded`) are never sent, by any feature.
  API keys never appear in logs, JSON sent to QML, or error messages (only the last 4 characters).

## Layout and ownership
| Worker | Owns (may create/edit) | Must not touch |
|---|---|---|
| **A** AI core | `crates/rusty-inbox-ai/src/{lib,http,provider,models,chat,codex}.rs`, `crates/rusty-inbox-ai/examples/`, `crates/rusty-inbox-ai/tests/` | everything else |
| **B** AI settings | `app/src/ai.rs`, `app/src/ai_settings.rs`, `app/qml/AiSettingsPage.qml`, the AI page entry + mount in `app/qml/Settings.qml`; its autotest block in `app/qml/Main.qml` marked `// AI(B)` | composer, reader, sidebar, core crate |
| **C1** write + read tools | `crates/rusty-inbox-ai/src/features/{compose,read}.rs`, `crates/rusty-inbox-ai/src/extract.rs`, `app/src/assistant.rs`, `app/qml/AiComposerTools.qml`, `app/qml/AiReaderPanel.qml`, `app/qml/Composer.qml`, `app/qml/Reader.qml`; up to 10 lines in `app/qml/Main.qml` marked `// AI(C1)` | Settings, sidebar, thread list, core crate |
| **C2** organize | `crates/rusty-inbox-ai/src/features/organize.rs`, `app/src/ai_inbox.rs`, `app/qml/AiBriefing.qml`, `app/qml/Main.qml` (except C1's lines), `app/qml/Sidebar.qml`, `app/qml/ThreadList.qml`, `crates/rusty-inbox-core/**` (store tables, list filters), hook lines in `app/src/bridge.rs` / `app/src/engine.rs` | Settings, composer, reader |

Shared files every worker may **append** to (one line per item, keep both sides at merge):
`app/build.rs` (ICONS list, qml_files), `app/icons/*.svg` (new icons only), `tools/ai-mock/replies/<worker>-*.json`.
`crates/rusty-inbox-ai/src/features/mod.rs` helpers (`MailForAi`, `Lang`, `render_thread`,
`extract_json`, `clean_text`, `clip`) exist already: use them; add new shared helpers only in your own
file. Worker A owns `lib.rs`; ask in your report instead of editing it.

Already in place (the parent's skeleton, commit on `main`): the crate with these signatures, the
OpenAI-compatible chat + model list (OpenRouter, Custom) working and streaming, `app/src/ai.rs`
(`StoreSecrets`, `connection`, `active`, `available`, `feature_on`, `excluded`, `lang`, `run`), the
settings keys below, three registered QML objects (`AiSettings`, `Assistant`, `AiInbox`, each with
`available()`), four placeholder QML files, the `sparkle` icon, and `tools/ai-mock/mock_server.py`.

## Settings keys (already in `app/src/settings.rs`)
`aiEnabled`, `aiProvider` (codex|openai|anthropic|openrouter|ollama|custom), `aiModel`, `aiThinking`
(`""` = default), `aiOllamaUrl`, `aiCustomUrl`, `aiLanguage` (auto|en|ar), `aiExcluded` (comma list),
`aiComposeTools`, `aiReadTools`, `aiAutoSummary`, `aiAutoReplies`, `aiAutoPhishing`, `aiSmartFilter`,
`aiNeedsReply`, `aiFollowUps`, `aiFollowUpDays` (2|3|5|7|14), `aiSearch`, `aiBriefing`,
`aiBriefingHour` (5..12). Secrets: `ai:key:<provider id>` (API keys), `ai:codex` (sign-in tokens).

## Threading rule (all app workers)
AI calls are blocking (ureq). Run each on `std::thread::spawn`, never on the Qt thread and never on
the engine's tokio runtime (two workers only: a long AI call there would stall mail sync). Report
back with `qt_thread().queue(...)` + a signal, the same way `bridge.rs` does. Every request gets an
id (i64, increasing) so QML can drop results of a request it no longer shows; keep an
`Arc<AtomicBool>` per id for Cancel. Stream: emit the accumulated text at most every 50 ms.

## Wire facts (verified 2026-10-18 from this PC)
**ChatGPT subscription (Codex)**: verified with a real Plus account.
- OAuth client `app_EMoamEEZ73f0CkXaXp7hrann`, issuer `https://auth.openai.com`.
- Browser: `GET https://auth.openai.com/oauth/authorize?response_type=code&client_id=…&redirect_uri=http://localhost:1455/auth/callback&scope=openid%20profile%20email%20offline_access&code_challenge=<S256>&code_challenge_method=S256&id_token_add_organizations=true&codex_cli_simplified_flow=true&state=<random>`.
  The listener binds **127.0.0.1:1455** (the redirect URI string says `localhost`). Exchange:
  `POST https://auth.openai.com/oauth/token` form `grant_type=authorization_code, code, redirect_uri, client_id, code_verifier`.
- Device code: `POST {issuer}/api/accounts/deviceauth/usercode` JSON `{"client_id"}` →
  `{device_auth_id, user_code, interval}` (interval ≥ 3 s; 429 = retry with backoff). The user opens
  `https://auth.openai.com/codex/device` and types the code. Poll `POST {issuer}/api/accounts/deviceauth/token`
  JSON `{device_auth_id, user_code}`: 403/404 = pending, 200 → `{authorization_code, code_verifier}`;
  exchange at the token URL with `redirect_uri=https://auth.openai.com/deviceauth/callback`. 15 min max.
- Refresh: form `grant_type=refresh_token, refresh_token, client_id`. Refresh tokens rotate and are
  single-use: one refresh at a time (process-wide mutex), save the new pair before using it; refresh
  when the access token expires within 120 s and once after a 401.
- Claims (access-token JWT payload): `exp`; `https://api.openai.com/auth` → `chatgpt_account_id`,
  `chatgpt_plan_type` (e.g. `plus`), `chatgpt_data_residency` / `chatgpt_compute_residency`;
  `https://api.openai.com/profile` → `email`.
- Backend `https://chatgpt.com/backend-api/codex`, headers: `Authorization: Bearer <access>`,
  `ChatGPT-Account-ID: <account id>` (without it the model list is empty), `x-openai-internal-codex-residency`
  when the claim is set, `originator: rusty-inbox`, `User-Agent: RustyInbox/<version>`.
- Models: `GET /models?client_version=99.0.0` (fallback `0.0.0` if empty/non-200) →
  `{"models":[{slug, display_name, description, visibility ("list"|"hide"), priority, context_window,
  supported_reasoning_levels:[{effort, description}], default_reasoning_level, ...}]}`. Hide
  `visibility=hide`; sort by `priority`. Live list on the test account: gpt-6.1-sol, gpt-6-astra,
  gpt-6.1-terra, gpt-6-sol, gpt-6-luna, … with levels low…ultra.
- Chat: `POST /responses` with `{"model", "instructions": <system>, "input": [{"type":"message","role":"user","content":[{"type":"input_text","text":…}]}], "store": false, "stream": true, "reasoning": {"effort": <level>}}`
  (assistant turns: `{"type":"message","role":"assistant","content":[{"type":"output_text","text":…}]}`).
  SSE events seen: `response.created`, `response.in_progress`, `response.output_item.added`,
  `response.content_part.added`, `response.output_text.delta` (`delta`), `response.metadata`,
  `response.output_text.done`, `response.content_part.done`, `response.output_item.done`,
  `response.completed` (`response.usage.{input_tokens,output_tokens}`); errors as `response.failed` /
  `error`. Answered "pong" in < 2 s at effort low.
**OpenAI (API key)**: same Responses API at `https://api.openai.com/v1/responses` (`Authorization:
Bearer <key>`), `GET /v1/models` lists ids only: keep chat-capable ones (drop embedding, tts, whisper,
dall-e, gpt-image, moderation, transcribe, realtime, audio, search-only, babbage/davinci) and offer
reasoning levels (`minimal low medium high`) for o1/o3/o4/gpt-5/gpt-6 families; JSON mode =
`"text": {"format": {"type": "json_object"}}` (the prompt must mention JSON).
**Anthropic**: `GET https://api.anthropic.com/v1/models?limit=1000` and `POST /v1/messages`, headers
`x-api-key`, `anthropic-version: 2023-06-01`. Stream SSE: `content_block_delta` with
`delta.type=text_delta` (`text`); `thinking_delta` is not shown; `message_delta.usage`; `error`.
Thinking: `{"thinking":{"type":"enabled","budget_tokens":N}}` with low=2048, medium=8192, high=24576,
and `max_tokens` = N + answer budget; if the API answers 400 mentioning `adaptive`, retry with
`{"thinking":{"type":"adaptive"}}` plus `"output_config":{"effort":<level>}`. Levels offered: off, low,
medium, high (none for claude-3-* / *-haiku-3*). Not probed live (no key on this PC).
**OpenRouter**: `GET https://openrouter.ai/api/v1/models` is public (457 models today) with
`reasoning: {mandatory, supported_efforts:[max,xhigh,high,medium,low,minimal,none], default_effort}`
and `supported_parameters`; chat = Chat Completions with `reasoning: {effort}` (`none` = off) and the
`HTTP-Referer` / `X-Title` headers. Implemented and tested in the skeleton.
**Ollama**: local `http://localhost:11434` (no key) or `https://ollama.com` (`Authorization: Bearer <key>`).
`GET /api/tags` → `{models:[{name, model, size, details, capabilities:["completion","tools","thinking"]}]}`
(older local servers have no `capabilities`: then ask `POST /api/show {"model"}` →
`{capabilities, …}` for up to 40 models). Chat: `POST /api/chat {"model","messages":[{role,content}],"stream":true,"think":…}`
streams NDJSON lines `{"message":{"content","thinking"},"done":bool, prompt_eval_count, eval_count}`;
`think` = false/true, or "low"/"medium"/"high" for gpt-oss models; JSON mode `"format":"json"`.
The test key on this PC is over its weekly quota (HTTP 429 `{"error":"you … reached your weekly
usage limit…"}`): show such messages to the user as they are.

## Thinking levels
Universal names, lightest first: `off on minimal low medium high xhigh max ultra`.
`ModelInfo.thinking` lists what a model accepts; `""` (Default) is always allowed and sends nothing.
Mapping per provider is in `chat.rs` (Worker A): Codex/OpenAI `reasoning.effort` (off → omit),
OpenRouter `reasoning.effort` (`off` → `none`), Anthropic budgets (above), Ollama `think`,
Custom `reasoning_effort`.

## UX rules (B, C1, C2)
- Every AI action shows progress (streamed text or a spinner + Cancel), an error state with the
  provider's message and a Retry, and never fabricates a result: no fallback text when the call fails.
- Results that change the user's text (proofread, rewrite, translate, write for me, subject) are a
  **preview with Replace / Insert / Cancel**; nothing overwrites the draft without a click.
- AI buttons appear only when `available()` is true and the feature switch is on; when AI is off,
  the composer and reader look exactly as in 0.1.4.
- Arabic text renders right-to-left (`horizontalAlignment: Text.AlignRight` / `dir="auto"` in HTML).
- Keyboard: Ctrl+J opens the composer AI menu; Shift+S in the reader summarizes. The Shortcuts page
  lives in Settings.qml (B's file): C1 and C2 list their shortcuts in their report and the parent adds
  them.

## Tests and checks
- Rust: unit tests for every prompt builder (asserts on the system prompt rules, clipping, language
  instruction), every parser (good answer, fenced answer, garbage → None/empty, truncated JSON → None),
  word diff, extraction (generate a small PDF/DOCX in the test), Codex JWT claims, PKCE, the token
  refresh rotation (mock token endpoint), each provider's request body and stream parser against a
  local `TcpListener` mock (no network in `cargo test`).
- UI: the scripted check `--autotest=<dir>` runs offline. Add an AI pass: with
  `RUSTY_INBOX_AI_URL=http://127.0.0.1:18765/v1` (mock running) and the extra argument `--autotest-ai`,
  every AI screen you own must be opened, waited on until the mock answer is shown, and screenshotted
  (`<dir>/ai-<screen>.png`). C2 owns the step list in Main.qml: it adds steps for its own screens after
  the existing ones (only when `--autotest-ai` is given). B and C1 may add their steps there too, as one
  contiguous block each marked `// AI(B)` / `// AI(C1)` placed right before C2's block; the parent
  resolves the merge. Check every screenshot with vision before reporting.
- Mock replies: put yours in `tools/ai-mock/replies/<worker>-<feature>.json` (`match` = a unique
  phrase from your system prompt).
- Commands (from the repo root of your worktree; Qt app builds are heavy, serialize them):
  `cargo fmt --all`, `flock ~/.hermes/cache/scratch/rusty-build.lock cargo clippy -j 6 --workspace --all-targets --release -- -D warnings`,
  `flock ~/.hermes/cache/scratch/rusty-build.lock cargo test -j 6 --workspace --release`.
  rustfmt `max_width=120` (rustfmt.toml). clippy must be clean.
- Never run the installed app (`~/.local/bin/rusty-inbox`) or touch `~/.local/share/rusty-inbox`;
  run your build with `RUSTY_INBOX_DATA_DIR=<a copy in your scratch dir>` and `QT_QPA_PLATFORM=offscreen`.
- Never print or commit a key or token. Never send real mail.
