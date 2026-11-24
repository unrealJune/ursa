import { NativeTabs } from 'expo-router/unstable-native-tabs';

import { useTheme } from '@/theme';

/** Android's own Material 3 navigation bar. */
export default function Tabs() {
  const { c } = useTheme();
  return (
    <NativeTabs
      backgroundColor={c.surfaceContainer}
      indicatorColor={c.secondaryContainer}
      iconColor={{ default: c.onSurfaceVariant, selected: c.onSecondaryContainer }}
      labelStyle={{ default: { color: c.onSurfaceVariant }, selected: { color: c.onSurface } }}
      rippleColor={c.onSurfaceVariant}
    >
      <NativeTabs.Trigger name="index">
        <NativeTabs.Trigger.Label>Photos</NativeTabs.Trigger.Label>
        <NativeTabs.Trigger.Icon md="photo_library" />
      </NativeTabs.Trigger>
      <NativeTabs.Trigger name="files">
        <NativeTabs.Trigger.Label>Files</NativeTabs.Trigger.Label>
        <NativeTabs.Trigger.Icon md="folder" />
      </NativeTabs.Trigger>
      <NativeTabs.Trigger name="account">
        <NativeTabs.Trigger.Label>Account</NativeTabs.Trigger.Label>
        <NativeTabs.Trigger.Icon md="account_circle" />
      </NativeTabs.Trigger>
    </NativeTabs>
  );
}
