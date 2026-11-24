import { CameraView, useCameraPermissions } from 'expo-camera';
import * as Haptics from 'expo-haptics';
import { router } from 'expo-router';
import { useRef } from 'react';
import { Linking, StyleSheet, View } from 'react-native';

import { deliverScan } from '@/core/handoff';
import { useTheme } from '@/theme';
import { Button, Centered, Icon, Txt, withAlpha } from '@/ui/kit';

/** Reads one `ursa-…:` QR code and hands it back to the screen that asked. */
export default function Scan() {
  const { c } = useTheme();
  const [permission, request] = useCameraPermissions();
  const done = useRef(false);

  if (!permission) return <Centered>{null}</Centered>;
  if (!permission.granted) {
    return (
      <Centered>
        <Icon name="qr-code-scanner" size={48} color={c.primary} />
        <Txt role="titleLarge" style={{ textAlign: 'center' }}>
          Scan codes with the camera
        </Txt>
        <Txt color={c.onSurfaceVariant} style={{ textAlign: 'center' }}>
          Ursa reads link, node and invite codes from your other devices. Nothing is recorded.
        </Txt>
        {/* After a denial Android stops showing its prompt, and request() answers at once. */}
        {permission.canAskAgain ? (
          <Button label="Allow camera" onPress={request} />
        ) : (
          <Button label="Open settings" onPress={() => Linking.openSettings()} />
        )}
      </Centered>
    );
  }

  return (
    <View style={{ flex: 1, backgroundColor: 'black' }}>
      <CameraView
        style={StyleSheet.absoluteFill}
        facing="back"
        barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
        onBarcodeScanned={({ data }) => {
          if (done.current || !/^ursa-(link|claim|invite):/.test(data.trim())) return;
          done.current = true;
          Haptics.performAndroidHapticsAsync(Haptics.AndroidHaptics.Confirm).catch(() => {});
          router.back();
          deliverScan(data.trim());
        }}
      />
      <View style={styles.overlay} pointerEvents="none">
        <View style={[styles.frame, { borderColor: withAlpha('#FFFFFF', 0.9) }]} />
        <Txt role="bodyLarge" color="#FFFFFF" style={{ marginTop: 24, textAlign: 'center' }}>
          Point at the code on your other device
        </Txt>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  overlay: { position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, alignItems: 'center', justifyContent: 'center', padding: 32 },
  frame: { width: 260, height: 260, borderWidth: 3, borderRadius: 28 },
});
