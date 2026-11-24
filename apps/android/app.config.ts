import type { ConfigContext, ExpoConfig } from 'expo/config';

/**
 * app.json, plus the version CI stamps on a build: URSA_VERSION (the android-v* release, or a
 * CI version) and URSA_VERSION_CODE (the workflow run number, which only ever grows).
 */
export default ({ config }: ConfigContext): ExpoConfig => {
  const version = process.env.URSA_VERSION || config.version || '0.1.0';
  const versionCode = Number(process.env.URSA_VERSION_CODE) || config.android?.versionCode || 1;
  return {
    ...(config as ExpoConfig),
    version,
    android: { ...config.android, versionCode },
  };
};
