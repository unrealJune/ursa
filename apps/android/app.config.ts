import type { ConfigContext, ExpoConfig } from 'expo/config';

/**
 * app.json, plus the version CI stamps on a build: URSA_VERSION (the android-v* release, or a
 * CI version) and URSA_VERSION_CODE (the workflow run number, which only ever grows).
 *
 * URSA_DEV=1 builds "Ursa dev" (com.unrealjune.ursa.dev), which installs next to a released Ursa:
 * a local build is signed with another key, so it cannot replace that one without uninstalling it,
 * and uninstalling forgets the account the phone holds.
 */
export default ({ config }: ConfigContext): ExpoConfig => {
  const version = process.env.URSA_VERSION || config.version || '0.1.0';
  const versionCode = Number(process.env.URSA_VERSION_CODE) || config.android?.versionCode || 1;
  const dev = process.env.URSA_DEV === '1';
  return {
    ...(config as ExpoConfig),
    ...(dev ? { name: 'Ursa dev' } : {}),
    version,
    android: { ...config.android, versionCode, ...(dev ? { package: 'com.unrealjune.ursa.dev' } : {}) },
  };
};
