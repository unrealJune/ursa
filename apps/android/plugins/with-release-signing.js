// Signs release builds with the key in URSA_KEYSTORE_PATH (+ _PASSWORD, URSA_KEY_ALIAS,
// URSA_KEY_PASSWORD) when CI provides one; without it a release build keeps the template's debug
// key, which is fine for a local build and nothing else. Every build that people install must be
// signed by the same key, or Android refuses the update.
const { withAppBuildGradle } = require('expo/config-plugins');

const MARK = '// ursa: release signing';

const BLOCK = `
${MARK}
def ursaKeystore = System.getenv("URSA_KEYSTORE_PATH")
if (ursaKeystore) {
    android.signingConfigs.create("ursaRelease") {
        storeFile file(ursaKeystore)
        storePassword System.getenv("URSA_KEYSTORE_PASSWORD")
        keyAlias System.getenv("URSA_KEY_ALIAS")
        keyPassword System.getenv("URSA_KEY_PASSWORD")
    }
    android.buildTypes.release.signingConfig android.signingConfigs.ursaRelease
}
`;

module.exports = function withReleaseSigning(config) {
  return withAppBuildGradle(config, (c) => {
    if (!c.modResults.contents.includes(MARK)) c.modResults.contents += BLOCK;
    return c;
  });
};
