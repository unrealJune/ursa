import { type Href, Stack, useRootNavigationState, useRouter } from 'expo-router';
import * as SplashScreen from 'expo-splash-screen';
import { StatusBar } from 'expo-status-bar';
import * as SystemUI from 'expo-system-ui';
import { useEffect, useRef } from 'react';
import { GestureHandlerRootView } from 'react-native-gesture-handler';

import { type Phase, UrsaProvider, useUrsa } from '@/core/store';
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

/**
 * Where each phase starts. The guards below only hide screens; when the one on show is hidden,
 * expo-router falls back to whatever screen it may still show, and with no onboarding route at
 * `/` that was the unguarded scan screen: a fresh install opened on "Scan a code", with nothing
 * waiting for what it read. Welcome and needs-node share a start, so a step inside onboarding
 * (creating an account, which ends needs-node, then the recovery words) is never interrupted.
 */
const START: Record<Phase, Href> = {
  starting: '/starting',
  failed: '/problem',
  offline: '/problem',
  welcome: '/welcome',
  'needs-node': '/welcome',
  ready: '/',
};

function Nav() {
  const { phase } = useUrsa();
  const { c, dark } = useTheme();
  const router = useRouter();
  const ready = useRootNavigationState()?.key;
  const started = useRef<Href | null>(null);

  useEffect(() => {
    if (!ready || started.current === START[phase]) return;
    started.current = START[phase];
    router.replace(START[phase]);
  }, [phase, ready, router]);

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
