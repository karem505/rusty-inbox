# AI in Rusty Inbox: backlog

Target release: **0.2.0**. Every item below ships; nothing here is optional or "later".
Owner decisions (Karem, 2026-10-18): implement the whole list; providers Ollama, OpenRouter,
Anthropic, OpenAI and a ChatGPT/Codex subscription sign-in; the model is always chosen by the
user from the provider's live list, searchable; a thinking level can be chosen; backend in Rust;
the UI keeps the app's current language (English) and look.

Status legend: [ ] not started, [~] in progress, [x] done and verified.

## A. Connection and settings (Settings → AI)
- [ ] A1 Settings → AI page: master switch, provider picker, connection status, privacy notes
- [ ] A2 Providers: Ollama (local or ollama.com with a key), OpenRouter, Anthropic, OpenAI (API key),
      ChatGPT subscription (Codex sign-in, no key), Custom OpenAI-compatible endpoint (base URL + key)
- [ ] A3 Sign in with ChatGPT: browser login (PKCE, loopback 127.0.0.1:1455) with device-code fallback;
      tokens refreshed automatically; shows the account email and plan; Sign out
- [ ] A4 API keys stored in the app's local secret store (same place as mail passwords); never shown
      again after saving (only the last 4 characters); "Test connection"
- [ ] A5 Model picker: the full list fetched live from the provider, searchable (fuzzy), refresh button,
      shows name, id and context size; "use this id" for an id the list lacks
- [ ] A6 Thinking level: the levels the chosen model supports (from the provider where it says so),
      plus "Default"; hidden for models without reasoning control
- [ ] A7 Per-feature switches; background features are off by default and labelled as sending mail
      content to the provider
- [ ] A8 Accounts excluded from AI (nothing from them is ever sent)
- [ ] A9 Answer language: same as the email (default), English, Arabic

## B. While writing (composer)
- [ ] 1 Proofread: corrected text with the changes highlighted (word diff); Replace or Cancel
- [ ] 2 Rewrite: formal, friendly, shorter, longer, clearer, more persuasive; preview, Replace or Cancel
- [ ] 3 Translate the draft (English ⇄ Arabic, or any language typed in)
- [ ] 4 Write for me: a short instruction (or bullet points) becomes a full email; uses the thread when
      replying; output streams into a preview
- [ ] 5 Suggest a subject line (3 options to pick from)

## C. While reading (reader)
- [ ] 6 Summarize the conversation (short summary card at the top of the thread)
- [ ] 7 Action items and deadlines from the conversation (part of the same card, each with its due date)
- [ ] 8 Translate the message in place (toggle back to the original)
- [ ] 9 Smart replies: 3 short replies under the message; clicking one opens the reply prefilled
- [ ] 10 Summarize an attachment (PDF, DOCX, text, CSV, HTML) without opening it
- [ ] 16 Phishing check: risk level and the reasons (sender, Reply-To mismatch, link domains, urgency)

## D. Organizing (background, opt-in)
- [ ] 11 Smart filtering: new mail the rules are unsure about is classified by the model
      (People, Notifications, Newsletters, Invitations); never moves mail on the server
- [ ] 12 "Needs reply": new mail that expects an answer from you gets a mark and its own list
- [ ] 13 Follow-up reminders: mail you sent that asked for something and got no answer after N days
      shows in a "Follow up" list (and one notification)
- [ ] 14 Ask in plain words: the search field takes a question ("invoices from Zenit last month") and
      turns it into the app's search query, shown so it can be edited
- [ ] 15 Daily briefing: once a day at a chosen hour (or on demand), a digest of unread mail, people
      first, with what needs attention

## E. Release
- [ ] E1 Tests for every module (offline mock server for providers; prompt/parse unit tests)
- [ ] E2 Scripted UI check covers every AI screen against the mock server
- [ ] E3 README + release notes 0.2.0; privacy section updated
- [ ] E4 Karem connects his own accounts and tests live (then release)
