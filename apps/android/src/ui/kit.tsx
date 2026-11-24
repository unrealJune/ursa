import MaterialIcons from '@expo/vector-icons/MaterialIcons';
import {
  Host,
  LinearWavyProgressIndicator,
  LoadingIndicator,
  Switch as ComposeSwitch,
} from '@expo/ui/jetpack-compose';
import { fillMaxWidth } from '@expo/ui/jetpack-compose/modifiers';
import { type ComponentProps, type ReactNode, useState } from 'react';
import {
  ActivityIndicator,
  Modal,
  Pressable,
  ScrollView,
  type StyleProp,
  StyleSheet,
  Text,
  TextInput,
  type TextInputProps,
  type TextStyle,
  View,
  type ViewStyle,
} from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { type TypeRole, type as typeScale, useTheme } from '@/theme';

export type IconName = ComponentProps<typeof MaterialIcons>['name'];

export function Icon({ name, size = 24, color }: { name: IconName | string; size?: number; color?: string }) {
  const { c } = useTheme();
  return <MaterialIcons name={name as IconName} size={size} color={color ?? c.onSurfaceVariant} />;
}

export function Txt({
  role = 'bodyMedium',
  color,
  style,
  children,
  numberOfLines,
  selectable,
}: {
  role?: TypeRole;
  color?: string;
  style?: StyleProp<TextStyle>;
  children: ReactNode;
  numberOfLines?: number;
  selectable?: boolean;
}) {
  const { c } = useTheme();
  return (
    <Text style={[typeScale[role], { color: color ?? c.onSurface }, style]} numberOfLines={numberOfLines} selectable={selectable}>
      {children}
    </Text>
  );
}

/** A scrolling page on the surface colour, padded clear of the system bars below. */
export function Page({ children, padded = true, style }: { children: ReactNode; padded?: boolean; style?: StyleProp<ViewStyle> }) {
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  return (
    <ScrollView
      style={{ flex: 1, backgroundColor: c.surface }}
      contentContainerStyle={[{ paddingBottom: insets.bottom + 24 }, padded && { paddingHorizontal: 16 }, style]}
      keyboardShouldPersistTaps="handled"
    >
      {children}
    </ScrollView>
  );
}

type ButtonKind = 'filled' | 'tonal' | 'outlined' | 'text' | 'danger';

/** A Material 3 common button: 40 dp, fully rounded, label-large, with the platform ripple. */
export function Button({
  label,
  onPress,
  kind = 'filled',
  icon,
  disabled,
  busy,
  style,
}: {
  label: string;
  onPress?: () => void;
  kind?: ButtonKind;
  icon?: IconName | string;
  disabled?: boolean;
  busy?: boolean;
  style?: StyleProp<ViewStyle>;
}) {
  const { c } = useTheme();
  const off = disabled || busy;
  const palette: Record<ButtonKind, { bg: string; fg: string; border?: string }> = {
    filled: { bg: c.primary, fg: c.onPrimary },
    tonal: { bg: c.secondaryContainer, fg: c.onSecondaryContainer },
    outlined: { bg: 'transparent', fg: c.primary, border: c.outlineVariant },
    text: { bg: 'transparent', fg: c.primary },
    danger: { bg: c.error, fg: c.onError },
  };
  const p = palette[kind];
  const fg = off ? withAlpha(c.onSurface, 0.38) : p.fg;
  const bg = off && kind !== 'text' && kind !== 'outlined' ? withAlpha(c.onSurface, 0.12) : p.bg;
  return (
    <View style={[styles.buttonWrap, style]}>
      <Pressable
        onPress={onPress}
        disabled={off}
        android_ripple={{ color: withAlpha(p.fg, 0.12) }}
        accessibilityRole="button"
        accessibilityLabel={label}
        style={[
          styles.button,
          { backgroundColor: bg, paddingHorizontal: kind === 'text' ? 12 : 24 },
          p.border && { borderWidth: 1, borderColor: off ? withAlpha(c.onSurface, 0.12) : p.border },
        ]}
      >
        {busy ? <ActivityIndicator size={18} color={fg} /> : icon ? <Icon name={icon} size={18} color={fg} /> : null}
        <Text style={[typeScale.labelLarge, { color: fg }]}>{label}</Text>
      </Pressable>
    </View>
  );
}

export function IconButton({
  icon,
  onPress,
  label,
  color,
  size = 24,
}: {
  icon: IconName | string;
  onPress: () => void;
  label: string;
  color?: string;
  size?: number;
}) {
  const { c } = useTheme();
  return (
    <View style={styles.iconButtonWrap}>
      <Pressable
        onPress={onPress}
        accessibilityRole="button"
        accessibilityLabel={label}
        android_ripple={{ color: withAlpha(c.onSurface, 0.12), borderless: true, radius: 20 }}
        style={styles.iconButton}
        hitSlop={4}
      >
        <Icon name={icon} size={size} color={color ?? c.onSurfaceVariant} />
      </Pressable>
    </View>
  );
}

/** A Material 3 filled text field: label inside the container, indicator line below. */
export function Field({
  label,
  supporting,
  error,
  trailing,
  style,
  ...input
}: TextInputProps & { label: string; supporting?: string; error?: string | null; trailing?: ReactNode }) {
  const { c } = useTheme();
  const [focused, setFocused] = useState(false);
  const accent = error ? c.error : focused ? c.primary : c.onSurfaceVariant;
  return (
    <View style={[{ marginVertical: 8 }, style]}>
      <View
        style={[
          styles.field,
          { backgroundColor: c.surfaceContainerHighest, borderBottomColor: error ? c.error : focused ? c.primary : c.onSurfaceVariant },
          { borderBottomWidth: focused || error ? 2 : 1 },
        ]}
      >
        <View style={{ flex: 1 }}>
          <Text style={[typeScale.bodySmall, { color: accent }]}>{label}</Text>
          <TextInput
            {...input}
            onFocus={(e) => {
              setFocused(true);
              input.onFocus?.(e);
            }}
            onBlur={(e) => {
              setFocused(false);
              input.onBlur?.(e);
            }}
            cursorColor={c.primary}
            selectionColor={withAlpha(c.primary, 0.3)}
            placeholderTextColor={c.onSurfaceVariant}
            style={[typeScale.bodyLarge, { color: c.onSurface, padding: 0, marginTop: 2, textAlignVertical: input.multiline ? 'top' : 'center' }]}
          />
        </View>
        {trailing}
      </View>
      {(error || supporting) && (
        <Text style={[typeScale.bodySmall, { color: error ? c.error : c.onSurfaceVariant, marginTop: 4, marginHorizontal: 16 }]}>
          {error || supporting}
        </Text>
      )}
    </View>
  );
}

/** A Material 3 list item: optional leading icon or element, one or two lines, optional trailing element. */
export function ListRow({
  icon,
  leading,
  title,
  subtitle,
  trailing,
  onPress,
  danger,
}: {
  icon?: IconName | string;
  leading?: ReactNode;
  title: string;
  subtitle?: string | null;
  trailing?: ReactNode;
  onPress?: () => void;
  danger?: boolean;
}) {
  const { c } = useTheme();
  const body = (
    <>
      {leading ?? (icon ? <View style={styles.rowLead}><Icon name={icon} color={danger ? c.error : c.onSurfaceVariant} /></View> : null)}
      <View style={{ flex: 1 }}>
        <Text style={[typeScale.bodyLarge, { color: danger ? c.error : c.onSurface }]} numberOfLines={1}>
          {title}
        </Text>
        {subtitle ? (
          <Text style={[typeScale.bodyMedium, { color: c.onSurfaceVariant }]} numberOfLines={2}>
            {subtitle}
          </Text>
        ) : null}
      </View>
      {trailing}
    </>
  );
  if (!onPress) return <View style={[styles.row, { minHeight: subtitle ? 72 : 56 }]}>{body}</View>;
  return (
    <Pressable onPress={onPress} android_ripple={{ color: withAlpha(c.onSurface, 0.12) }} style={[styles.row, { minHeight: subtitle ? 72 : 56 }]}>
      {body}
    </Pressable>
  );
}

export function SectionHeader({ children }: { children: string }) {
  const { c } = useTheme();
  return <Text style={[typeScale.titleSmall, { color: c.primary, paddingHorizontal: 16, paddingTop: 24, paddingBottom: 8 }]}>{children}</Text>;
}

/** A rounded group of rows on the container colour, the way Android's own settings draw them. */
export function Group({ children, style }: { children: ReactNode; style?: StyleProp<ViewStyle> }) {
  const { c } = useTheme();
  return <View style={[{ backgroundColor: c.surfaceContainer, borderRadius: 20, overflow: 'hidden', marginHorizontal: 16 }, style]}>{children}</View>;
}

export function Divider({ inset = 0 }: { inset?: number }) {
  const { c } = useTheme();
  return <View style={{ height: StyleSheet.hairlineWidth, backgroundColor: c.outlineVariant, marginLeft: inset }} />;
}

export function Switch({ value, onChange, disabled }: { value: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <Host matchContents>
      <ComposeSwitch value={value} onCheckedChange={onChange} enabled={!disabled} />
    </Host>
  );
}

/** Material 3 expressive wavy progress; `progress` null is indeterminate. */
export function Progress({ progress }: { progress: number | null }) {
  return (
    <Host style={{ height: 12, alignSelf: 'stretch' }}>
      <LinearWavyProgressIndicator progress={progress} modifiers={[fillMaxWidth()]} />
    </Host>
  );
}

/** Material 3 expressive loading indicator. */
export function Loading() {
  return (
    <Host matchContents>
      <LoadingIndicator />
    </Host>
  );
}

export function Centered({ children }: { children: ReactNode }) {
  const { c } = useTheme();
  return <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center', padding: 32, gap: 16, backgroundColor: c.surface }}>{children}</View>;
}

export function Empty({ icon, title, body, action }: { icon: IconName | string; title: string; body?: string; action?: ReactNode }) {
  const { c } = useTheme();
  return (
    <View style={{ alignItems: 'center', paddingHorizontal: 32, paddingVertical: 64, gap: 12 }}>
      <View style={{ width: 96, height: 96, borderRadius: 48, backgroundColor: c.secondaryContainer, alignItems: 'center', justifyContent: 'center', marginBottom: 8 }}>
        <Icon name={icon} size={44} color={c.onSecondaryContainer} />
      </View>
      <Txt role="titleLarge" style={{ textAlign: 'center' }}>
        {title}
      </Txt>
      {body ? (
        <Txt color={c.onSurfaceVariant} style={{ textAlign: 'center' }}>
          {body}
        </Txt>
      ) : null}
      {action}
    </View>
  );
}

/** A tonal notice: info, or an error with the core's message. */
export function Notice({ text, tone = 'info', action }: { text: string; tone?: 'info' | 'error'; action?: ReactNode }) {
  const { c } = useTheme();
  const bg = tone === 'error' ? c.errorContainer : c.secondaryContainer;
  const fg = tone === 'error' ? c.onErrorContainer : c.onSecondaryContainer;
  return (
    <View style={{ backgroundColor: bg, borderRadius: 16, padding: 16, flexDirection: 'row', gap: 12, alignItems: 'flex-start', marginVertical: 8 }}>
      <Icon name={tone === 'error' ? 'error-outline' : 'info-outline'} color={fg} size={20} />
      <View style={{ flex: 1, gap: 8 }}>
        <Txt color={fg}>{text}</Txt>
        {action}
      </View>
    </View>
  );
}

/** A Material 3 top app bar for screens without a stack header (the tabs). */
export function TopBar({ title, actions }: { title: string; actions?: ReactNode }) {
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  return (
    <View style={{ paddingTop: insets.top, backgroundColor: c.surface }}>
      <View style={{ height: 64, flexDirection: 'row', alignItems: 'center', paddingLeft: 16, paddingRight: 4 }}>
        <Text style={[typeScale.titleLarge, { color: c.onSurface, flex: 1 }]} numberOfLines={1}>
          {title}
        </Text>
        <View style={{ flexDirection: 'row', alignItems: 'center' }}>{actions}</View>
      </View>
    </View>
  );
}

/** A Material 3 floating action button, extended when it has a label. */
export function Fab({ icon, label, onPress, bottom = 16 }: { icon: IconName | string; label?: string; onPress: () => void; bottom?: number }) {
  const { c } = useTheme();
  return (
    <View style={{ position: 'absolute', right: 16, bottom, borderRadius: 16, overflow: 'hidden', elevation: 3, backgroundColor: c.primaryContainer }}>
      <Pressable
        onPress={onPress}
        accessibilityRole="button"
        accessibilityLabel={label}
        android_ripple={{ color: withAlpha(c.onPrimaryContainer, 0.12) }}
        style={{ height: 56, minWidth: 56, paddingHorizontal: label ? 16 : 0, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 12 }}
      >
        <Icon name={icon} color={c.onPrimaryContainer} />
        {label ? <Text style={[typeScale.labelLarge, { color: c.onPrimaryContainer }]}>{label}</Text> : null}
      </Pressable>
    </View>
  );
}

export type SheetAction = { icon: IconName | string; label: string; onPress: () => void; danger?: boolean };

/** A Material 3 modal bottom sheet of actions. */
export function Sheet({ visible, title, actions, onClose }: { visible: boolean; title?: string; actions: SheetAction[]; onClose: () => void }) {
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  return (
    <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose} statusBarTranslucent navigationBarTranslucent>
      <Pressable style={{ flex: 1, backgroundColor: withAlpha(c.scrim, 0.32) }} onPress={onClose} accessibilityLabel="Close" />
      <View style={{ backgroundColor: c.surfaceContainerLow, borderTopLeftRadius: 28, borderTopRightRadius: 28, paddingBottom: insets.bottom + 8 }}>
        <View style={{ alignSelf: 'center', width: 32, height: 4, borderRadius: 2, backgroundColor: c.onSurfaceVariant, opacity: 0.4, marginVertical: 16 }} />
        {title ? (
          <Text style={[typeScale.titleMedium, { color: c.onSurface, paddingHorizontal: 24, paddingBottom: 8 }]} numberOfLines={1}>
            {title}
          </Text>
        ) : null}
        {actions.map((a) => (
          <ListRow
            key={a.label}
            icon={a.icon}
            title={a.label}
            danger={a.danger}
            onPress={() => {
              onClose();
              a.onPress();
            }}
          />
        ))}
      </View>
    </Modal>
  );
}

/** `#RRGGBBAA` (Material colours) or `#RRGGBB` with a new alpha. */
export function withAlpha(color: string, alpha: number): string {
  const hex = color.replace('#', '').slice(0, 6);
  const a = Math.round(alpha * 255)
    .toString(16)
    .padStart(2, '0');
  return `#${hex}${a}`;
}

const styles = StyleSheet.create({
  buttonWrap: { borderRadius: 20, overflow: 'hidden' },
  button: { height: 40, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8 },
  iconButtonWrap: { width: 40, height: 40, borderRadius: 20, overflow: 'hidden' },
  iconButton: { width: 40, height: 40, alignItems: 'center', justifyContent: 'center' },
  field: { borderTopLeftRadius: 4, borderTopRightRadius: 4, paddingHorizontal: 16, paddingTop: 8, paddingBottom: 8, minHeight: 56, flexDirection: 'row', alignItems: 'center', gap: 8 },
  row: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 16, paddingVertical: 8, gap: 16 },
  rowLead: { width: 24, alignItems: 'center' },
});
