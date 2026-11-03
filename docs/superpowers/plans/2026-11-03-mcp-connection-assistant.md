# MCP connection assistant implementation plan

> **For agentic workers:** Use the existing session for implementation and request an independent final code review. The user has approved implementing the connection-assistant flow discussed in chat.

**Goal:** Register and check TimeSchedule from Settings without manually copying credentials or editing client configuration.

**Architecture:** Keep the existing app-owned loopback HTTP server. Add an on-demand settings assistant and a short-lived native authentication-header command. Client configuration points to that command, not a plaintext token. Run filesystem and network operations off the UI thread; do not poll clients.

**Tech stack:** Rust/Iced, existing Hyper/Tokio/DPAPI, TOML editing and JSON serialization.

**Spec:** The approved chat flow: choose AI client, preview the configuration change, register, check connection. User selected Codex, Claude Code and Claude Desktop. Desktop uses a same-EXE stdio bridge to the app-owned HTTP server. This is local Windows integration, not cloud synchronization or data migration.

## Global constraints

- Preserve unrelated client configuration and create a backup before changing an existing file.
- Reject malformed files and changes made after preview. Never log or show existing credentials.
- Never write the bearer token into config, a command argument, or persistent environment.
- Use the running app's executable/data paths; accept paths containing spaces and Korean.
- Report server reachability separately from client registration and actual client adoption.
- Keep settings controls mounted and status height stable while background actions run.
- No client config mutations during development outside isolated fixtures; user registers through the delivered UI.
- Version 0.10.0; prepare representative EXE, commit/push/PR, hold merge.

## Review focus

- Existing unrelated servers and user edits survive; preview cannot overwrite concurrent changes.
- Missing/corrupt credentials do not create new credentials in helper mode.
- A tampered endpoint cannot send credentials off localhost or follow redirects.
- Quoted/unicode paths work through the real Windows helper process.
- Busy, error and success UI states retain layout and cannot apply stale previews.

## Tasks

- [x] Add failing behavior tests for configuration preservation, idempotence, conflict detection, malformed data and helper path quoting; implement isolated configuration planning/apply.
- [x] Add failing tests for loopback validation, missing credentials and real authenticated initialize/tools/list; implement bounded probe and native header CLI.
- [x] Add settings client selection, path detection/editing, preview, registration and check using asynchronous tasks; test state and layout transitions.
- [x] Run workspace tests and release verification, inspect UI, measure lightweight changes and obtain independent review.
- [x] Update usage/version docs, build representative EXE, commit/push/PR and automatic development record according to AGENTS.md. 0.10.0 delivered in PR #13; diary saved, Space verification pending.

## Follow-up: ChatGPT Desktop (0.10.1)

User requested ChatGPT Desktop as an additional selectable client. Official OpenAI documentation confirms that it shares the same local host's Codex configuration. Reuse the existing TOML registration and authentication helper; do not create a second server, separate credential store, or resident process. Show shared-configuration scope before applying and explain desktop restart versus web/cloud use. Keep the existing preview/apply/check workflow. Verify registration in both directions without duplicate entries or rewriting, preserve tool restrictions, and exercise the real settings layout with ChatGPT Desktop selected. Continue this branch/PR and hold merge for user testing.

Source checked 2026-11-03: https://learn.chatgpt.com/docs/extend/mcp?surface=desktop
