import { type Href, Stack, useRootNavigationState, useRouter } from 'expo-router';
import * as SplashScreen from 'expo-splash-screen';
import { StatusBar } from 'expo-status-bar';
import * as SystemUI from 'expo-system-ui';
import { useEffect, useRef } from 'react';
import { GestureHandlerRootView } from 'react-native-gesture-handler';

import { boot } from '@/core/lifecycle';
import { START } from '@/core/routes';
import { useSession } from '@/core/session';
import { useTheme } from '@/theme';

SplashScreen.preventAutoHideAsync();
boot();

export default function Root() {
  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <Nav />
    </GestureHandlerRootView>
  );
}

/**
 * The guards below only hide screens. When the screen on show is hidden, the stack falls back to
 * the first screen it may still show, before the effect below gets to say where to go. That first
 * screen must be `starting` (which forwards to the phase's start), never the scanner or the
 * network page: those are guarded too, and listed last. Without that, a phase change (a retry, a
 * relay change, the account closing) could land on "Scan a code" with nothing waiting for it.
 */
function Nav() {
  const phase = useSession((s) => s.phase);
  const { c, dark } = useTheme();
  const router = useRouter();
  const ready = useRootNavigationState()?.key;
  const shown = useRef<Href | null>(null);

  useEffect(() => {
    if (!ready || shown.current === START[phase]) return;
    shown.current = START[phase];
    router.replace(START[phase]);
  }, [phase, ready, router]);

  useEffect(() => {
    SystemUI.setBackgroundColorAsync(c.surface);
  }, [c.surface]);
  useEffect(() => {
    if (phase !== 'starting') SplashScreen.hideAsync();
  }, [phase]);

  const onboarding = phase === 'welcome' || phase === 'needs-node';
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
        <Stack.Screen name="starting" options={{ headerShown: false, animation: 'fade' }} />
        <Stack.Protected guard={phase === 'failed' || phase === 'offline'}>
          <Stack.Screen name="problem" options={{ headerShown: false, animation: 'fade' }} />
        </Stack.Protected>
        <Stack.Protected guard={onboarding}>
          <Stack.Screen name="(onboarding)" options={{ headerShown: false, animation: 'fade' }} />
        </Stack.Protected>
        <Stack.Protected guard={phase === 'ready'}>
          <Stack.Screen name="(main)" options={{ headerShown: false, animation: 'fade' }} />
        </Stack.Protected>
        <Stack.Protected guard={onboarding || phase === 'ready'}>
          <Stack.Screen name="scan" options={{ title: 'Scan a code', presentation: 'modal' }} />
        </Stack.Protected>
        <Stack.Protected guard={phase !== 'starting'}>
          <Stack.Screen name="network" options={{ title: 'Network' }} />
        </Stack.Protected>
      </Stack>
    </>
  );
}
