import { Redirect } from 'expo-router';

import { START } from '@/core/routes';
import { useSession } from '@/core/session';
import { useTheme } from '@/theme';
import { Centered, Loading, Txt } from '@/ui/kit';

/**
 * The app's first screen, and the one the stack falls back to when a phase change hides the
 * screen on show: it waits while the core starts, then forwards to where the phase begins.
 */
export default function Starting() {
  const phase = useSession((s) => s.phase);
  const { c } = useTheme();
  if (phase !== 'starting') return <Redirect href={START[phase]} />;
  return (
    <Centered>
      <Loading />
      <Txt color={c.onSurfaceVariant}>Connecting to your nodes…</Txt>
    </Centered>
  );
}
