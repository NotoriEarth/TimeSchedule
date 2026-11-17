// Codex hook adapter. No Codex hook sample has been captured yet: Codex does not run
// project hooks until the user trusts the folder and reviews the hooks, so every
// Codex record is reported as unsupported until a version is verified here.
import { normalizeCapture } from './contract.mjs';

export const adapter = {
  source: 'codex',
  verifiedVersions: [],
  events: {},
};

export function normalizeEvent(input, context) {
  return normalizeCapture(input, context, adapter);
}
