import { useTheme } from '@/theme';
import { Centered, Loading, Txt } from '@/ui/kit';

export default function Starting() {
  const { c } = useTheme();
  return (
    <Centered>
      <Loading />
      <Txt color={c.onSurfaceVariant}>Connecting to your nodes…</Txt>
    </Centered>
  );
}
