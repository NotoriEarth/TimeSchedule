// Claude Code hook adapter. Only versions and events observed in the M0 capture lab
// are listed; see docs/project-capture-support.md for the samples behind each line.
import { normalizeCapture, toolBody } from './contract.mjs';

export const adapter = {
  source: 'claude-code',
  // The label given to the probe when the lab hooks were written, checked against
  // `claude --version` and the `version` field of the lab session transcripts.
  verifiedVersions: ['claude-code-cli 2.1.325'],
  events: {
    SessionStart: { kind: 'session_start', body: payload => ({ source: payload.source }) },
    UserPromptSubmit: { kind: 'user_prompt', body: payload => ({ prompt: payload.prompt }) },
    // Every observed PreToolUse was followed by PostToolUse or PostToolUseFailure.
    PreToolUse: { skip: 'redundant_event' },
    PostToolUse: { kind: 'tool_result', body: payload => toolBody(payload) },
    PostToolUseFailure: { kind: 'tool_result', body: payload => toolBody(payload, { failed: true }) },
    Stop: { kind: 'turn_end', body: payload => ({ last_assistant_message: payload.last_assistant_message }) },
    SessionEnd: { kind: 'session_end', body: payload => ({ reason: payload.reason }) },
  },
};

export function normalizeEvent(input, context) {
  return normalizeCapture(input, context, adapter);
}
