import type { Href } from 'expo-router';

import type { Phase } from './session';

/**
 * Where each phase starts. Welcome and needs-node share a start, so a step inside onboarding
 * (creating an account, which ends needs-node, then the recovery words) is never interrupted.
 */
export const START: Record<Phase, Href> = {
  starting: '/starting',
  failed: '/problem',
  offline: '/problem',
  welcome: '/welcome',
  'needs-node': '/welcome',
  ready: '/',
};
