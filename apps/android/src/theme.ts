import { isDynamicColorAvailable, useMaterialColors, type MaterialColors } from '@expo/ui/jetpack-compose';
import { type TextStyle, useColorScheme } from 'react-native';

/** Ursa's own purple, for phones without wallpaper colours (Android 11 and older). */
export const SEED = '#5942E1';

export type Theme = { c: MaterialColors; dark: boolean };

/** Material 3 colours: the wallpaper's (Material You) where Android has them, else seeded from Ursa's purple. */
export function useTheme(): Theme {
  const scheme = useColorScheme();
  const colorScheme = scheme ?? undefined;
  const c = useMaterialColors(isDynamicColorAvailable ? { colorScheme } : { colorScheme, seedColor: SEED });
  return { c, dark: scheme === 'dark' };
}

/** The Material 3 type scale (Roboto, the system face). */
export const type = {
  displaySmall: { fontSize: 36, lineHeight: 44 },
  headlineLarge: { fontSize: 32, lineHeight: 40 },
  headlineMedium: { fontSize: 28, lineHeight: 36 },
  headlineSmall: { fontSize: 24, lineHeight: 32 },
  titleLarge: { fontSize: 22, lineHeight: 28 },
  titleMedium: { fontSize: 16, lineHeight: 24, fontWeight: '500', letterSpacing: 0.15 },
  titleSmall: { fontSize: 14, lineHeight: 20, fontWeight: '500', letterSpacing: 0.1 },
  bodyLarge: { fontSize: 16, lineHeight: 24, letterSpacing: 0.5 },
  bodyMedium: { fontSize: 14, lineHeight: 20, letterSpacing: 0.25 },
  bodySmall: { fontSize: 12, lineHeight: 16, letterSpacing: 0.4 },
  labelLarge: { fontSize: 14, lineHeight: 20, fontWeight: '500', letterSpacing: 0.1 },
  labelMedium: { fontSize: 12, lineHeight: 16, fontWeight: '500', letterSpacing: 0.5 },
  labelSmall: { fontSize: 11, lineHeight: 16, fontWeight: '500', letterSpacing: 0.5 },
} satisfies Record<string, TextStyle>;

export type TypeRole = keyof typeof type;
