// Codex hook adapter. Only the build and events observed in the M0 capture lab are
// listed; see docs/project-capture-support.md for the samples behind each line.
// Codex runs project hooks only after the user trusts the folder and reviews the hooks.
import { classifyTurns as classify, normalizeCapture, toolBody } from './contract.mjs';

export const adapter = {
  source: 'codex',
  // The executable bundled with the Codex desktop app, used both by `codex exec` runs
  // and by the app itself in the lab. The npm CLI build was not observed.
  verifiedVersions: ['codex-cli 0.169.0-alpha.11.1 app-bundled'],
  turnKey: payload => payload.turn_id,
  // `turn` marks events that belong to a turn: 'open' while it runs, 'completed' for the
  // normal end-of-turn hook, 'interrupted' when Codex reports that the user stopped it.
  // `requires` lists the string fields and `requiresObject` the object fields that every
  // observed sample of the event carried.
  events: {
    SessionStart: { kind: 'session_start', requires: ['source', 'model'], body: payload => ({ source: payload.source, model: payload.model }) },
    // Observed for `codex exec`; not observed when the desktop app delivered the prompt.
    UserPromptSubmit: { kind: 'user_prompt', turn: 'open', requires: ['prompt'], body: payload => ({ prompt: payload.prompt }) },
    // One pair per model call. They carry status and token counts of the coding session,
    // which is not the cost of record keeping, so they are not turned into activities.
    PreModelRequest: { skip: 'not_an_activity', turn: 'open' },
    PostModelRequest: { skip: 'not_an_activity', turn: 'open' },
    PreToolUse: { skip: 'redundant_event', turn: 'open', requires: ['tool_name', 'tool_use_id'], requiresObject: ['tool_input'] },
    // Codex has no failure event or flag: a failed command arrives here with its output
    // as plain text, so success cannot be read from the hook.
    PostToolUse: { kind: 'tool_result', turn: 'open', requires: ['tool_name', 'tool_use_id'], requiresObject: ['tool_input'], body: payload => toolBody(payload) },
    Stop: { kind: 'turn_end', turn: 'completed', requires: ['last_assistant_message'], body: payload => ({ last_assistant_message: payload.last_assistant_message }) },
    Interrupt: { kind: 'interrupted', turn: 'interrupted', body: () => ({}) },
    PreCompact: { skip: 'redundant_event', requires: ['trigger'] },
    PostCompact: { kind: 'compaction', requires: ['trigger'], body: payload => ({ trigger: payload.trigger }) },
    SessionEnd: { kind: 'session_end', requires: ['reason'], body: payload => ({ reason: payload.reason }) },
  },
};

export function normalizeEvent(input, context) {
  return normalizeCapture(input, context, adapter);
}

export function classifyTurns(records, context) {
  return classify(records, context, adapter);
}
