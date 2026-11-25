import { Stack } from 'expo-router';

import { useTheme } from '@/theme';

export default function MainLayout() {
  const { c } = useTheme();
  return (
    <Stack
      screenOptions={{
        headerStyle: { backgroundColor: c.surface },
        headerTintColor: c.onSurface,
        headerTitleStyle: { fontSize: 22, fontWeight: '400' },
        headerShadowVisible: false,
        contentStyle: { backgroundColor: c.surface },
      }}
    >
      <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
      <Stack.Screen name="viewer" options={{ headerShown: false, animation: 'fade', contentStyle: { backgroundColor: '#000000' } }} />
      <Stack.Screen name="backup" options={{ title: 'Backup' }} />
      <Stack.Screen name="devices" options={{ title: 'Devices' }} />
      <Stack.Screen name="add-device" options={{ title: 'Add a device' }} />
      <Stack.Screen name="nodes" options={{ title: 'Nodes' }} />
      <Stack.Screen name="add-node" options={{ title: 'Add a node' }} />
      <Stack.Screen name="friends" options={{ title: 'Friends' }} />
      <Stack.Screen name="pool" options={{ title: 'Pool' }} />
      <Stack.Screen name="usage" options={{ title: 'Usage' }} />
      <Stack.Screen name="recovery" options={{ title: 'Recovery' }} />
    </Stack>
  );
}
