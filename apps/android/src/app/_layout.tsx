import { Stack } from 'expo-router';
import * as SplashScreen from 'expo-splash-screen';
import { StatusBar } from 'expo-status-bar';
import * as SystemUI from 'expo-system-ui';
import { useEffect } from 'react';
import { GestureHandlerRootView } from 'react-native-gesture-handler';

import { UrsaProvider, useUrsa } from '@/core/store';
import { useTheme } from '@/theme';

SplashScreen.preventAutoHideAsync();

export default function Root() {
  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <UrsaProvider>
        <Nav />
      </UrsaProvider>
    </GestureHandlerRootView>
  );
}

function Nav() {
  const { phase } = useUrsa();
  const { c, dark } = useTheme();

  useEffect(() => {
    SystemUI.setBackgroundColorAsync(c.surface);
  }, [c.surface]);
  useEffect(() => {
    if (phase !== 'starting') SplashScreen.hideAsync();
  }, [phase]);

  return (
    <>
      <StatusBar style={dark ? 'light' : 'dark'} />
      <Stack
        screenOptions={{
          headerStyle: { backgroundColor: c.surface },
          headerTintColor: c.onSurface,
          headerTitleStyle: { fontSize: 22, fontWeight: '400' },
          headerShadowVisible: false,
          contentStyle: { backgroundColor: c.surface },
        }}
      >
        <Stack.Protected guard={phase === 'starting'}>
          <Stack.Screen name="starting" options={{ headerShown: false }} />
        </Stack.Protected>
        <Stack.Protected guard={phase === 'failed' || phase === 'offline'}>
          <Stack.Screen name="problem" options={{ headerShown: false }} />
        </Stack.Protected>
        <Stack.Protected guard={phase === 'welcome' || phase === 'needs-node'}>
          <Stack.Screen name="(onboarding)" options={{ headerShown: false }} />
        </Stack.Protected>
        <Stack.Protected guard={phase === 'ready'}>
          <Stack.Screen name="(main)" options={{ headerShown: false }} />
        </Stack.Protected>
        <Stack.Screen name="scan" options={{ title: 'Scan a code', presentation: 'modal' }} />
        <Stack.Screen name="network" options={{ title: 'Network' }} />
      </Stack>
    </>
  );
}
