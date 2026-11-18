// Claude Code hook adapter. Only versions and events observed in the M0 capture lab
// are listed; see docs/project-capture-support.md for the samples behind each line.
import { classifyTurns as classify, normalizeCapture, toolBody } from './contract.mjs';

export const adapter = {
  source: 'claude-code',
  // The label given to the probe when the lab hooks were written, checked against
  // `claude --version` and the `version` field of the lab session transcripts.
  verifiedVersions: ['claude-code-cli 2.1.325'],
  // Claude Code has no turn id; every hook of one turn shares the prompt id.
  turnKey: payload => payload.prompt_id,
  // `turn` marks events that belong to a turn: 'open' while it runs, 'completed' when the
  // tool's own end-of-turn hook fires. No user-interrupt hook was observed for Claude Code.
  events: {
    SessionStart: { kind: 'session_start', body: payload => ({ source: payload.source }) },
    UserPromptSubmit: { kind: 'user_prompt', turn: 'open', body: payload => ({ prompt: payload.prompt }) },
    // Every observed PreToolUse was followed by PostToolUse or PostToolUseFailure.
    PreToolUse: { skip: 'redundant_event', turn: 'open' },
    PostToolUse: { kind: 'tool_result', turn: 'open', body: payload => toolBody(payload) },
    PostToolUseFailure: { kind: 'tool_result', turn: 'open', body: payload => toolBody(payload, { failed: true }) },
    Stop: { kind: 'turn_end', turn: 'completed', body: payload => ({ last_assistant_message: payload.last_assistant_message }) },
    SessionEnd: { kind: 'session_end', body: payload => ({ reason: payload.reason }) },
  },
};

export function normalizeEvent(input, context) {
  return normalizeCapture(input, context, adapter);
}

export function classifyTurns(records) {
  return classify(records, adapter);
}
