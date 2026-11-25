import { useFocusEffect } from 'expo-router';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Pressable, RefreshControl, ScrollView, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { errorText, Ursa } from 'ursa-core';

import {
  type Choice,
  choiceName,
  choiceShape,
  loadStorage,
  offlineText,
  same,
  onSwitchEnded,
  startSwitch,
  type StorageReport,
  useSwitch,
} from '@/core/storage';
import { bytes } from '@/format';
import { useTheme } from '@/theme';
import { Button, IconButton, Notice, Progress, SectionHeader, Txt, withAlpha } from '@/ui/kit';
import { Bar, Card, Legend } from '@/ui/meters';

type Shape = { k: number; n: number };

/** Who is in the pool, how files are protected, what each choice lets you store, and switching. */
export default function Pool() {
  const { c } = useTheme();
  const insets = useSafeAreaInsets();
  const [report, setReport] = useState<StorageReport | null>(null);
  const [selected, setSelected] = useState<Shape | null>(null);
  const [custom, setCustom] = useState<Shape | null>(null);
  const [draft, setDraft] = useState<Shape>({ k: 3, n: 5 });
  const [showCustom, setShowCustom] = useState(false);
  const [give, setGive] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const switching = useSwitch((s) => s.switching);
  const progress = useSwitch((s) => s.progress);
  const switchError = useSwitch((s) => s.error);
  const giveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(
    async (keep: boolean, extra: Shape | null) => {
      setLoading(true);
      try {
        const r = await loadStorage(extra);
        setReport(r);
        setGive(r.giveBytes);
        if (!keep) setSelected({ k: r.k, n: r.n });
        setError(null);
      } catch (e) {
        setError(errorText(e));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  useFocusEffect(
    useCallback(() => {
      load(false, null);
    }, [load]),
  );

  // A switch ended (here or at start): show where it left things.
  useEffect(() => onSwitchEnded(() => load(true, custom)), [load, custom]);

  if (!report || !selected) {
    return (
      <View style={{ flex: 1, backgroundColor: c.surface, padding: 16 }}>
        {error ? <Notice tone="error" text={error} /> : <Progress progress={null} />}
      </View>
    );
  }

  const sel = report.choices.find((x) => same(x, selected)) ?? report.choices.find((x) => same(x, report))!;
  const isCurrent = same(sel, report);
  const needed = !isCurrent || report.pendingFiles > 0;
  const groups = [...new Set(report.choices.map((x) => x.survives))].sort((a, b) => a - b);

  function changeGive(next: number) {
    setGive(next);
    if (giveTimer.current) clearTimeout(giveTimer.current);
    giveTimer.current = setTimeout(async () => {
      try {
        await Ursa.setGive(next);
        await load(true, custom);
      } catch (e) {
        setError(errorText(e));
      }
    }, 700);
  }

  async function onSwitch() {
    try {
      setError(null);
      await startSwitch(sel.k, sel.n);
      await load(true, custom);
    } catch (e) {
      setError(errorText(e));
    }
  }

  const giveNow = give ?? report.giveBytes;
  const step = Math.max(1e9, 10 ** Math.floor(Math.log10(Math.max(giveNow, 1e9))));
  const hasFriends = report.members.some((m) => !m.own);

  return (
    <ScrollView
      style={{ flex: 1, backgroundColor: c.surface }}
      contentContainerStyle={{ paddingBottom: insets.bottom + 24 }}
      refreshControl={<RefreshControl refreshing={loading} onRefresh={() => load(true, custom)} colors={[c.primary]} progressBackgroundColor={c.surfaceContainerHigh} />}
    >
      {error || switchError ? (
        <View style={{ paddingHorizontal: 16 }}>
          <Notice tone="error" text={(error ?? switchError)!} />
        </View>
      ) : null}

      <SectionHeader>Members</SectionHeader>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8, paddingHorizontal: 16 }}>
        {report.members.map((m) => (
          <View key={m.name + m.own} style={{ minWidth: '30%', flexGrow: 1, padding: 12, borderRadius: 16, backgroundColor: c.surfaceContainer }}>
            <Txt role="titleSmall">{m.own ? 'You' : m.name}</Txt>
            <Txt role="bodySmall" color={c.onSurfaceVariant}>
              {[m.up30d != null ? `${Number((m.up30d * 100).toFixed(1))}% up` : '—', m.givesYou != null ? bytes(m.givesYou) : null].filter(Boolean).join(' · ')}
            </Txt>
          </View>
        ))}
      </View>

      <SectionHeader>Protection</SectionHeader>
      {groups.map((g) => (
        <View key={g} style={{ paddingHorizontal: 16, marginBottom: 8 }}>
          <Txt role="labelLarge" style={{ marginBottom: 8 }}>{`Survives ${g} down`}</Txt>
          <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: 8 }}>
            {report.choices
              .filter((x) => x.survives === g)
              .sort((a, b) => a.n / a.k - b.n / b.k)
              .map((x) => (
                <ChoiceCard key={`${x.k}-${x.n}`} choice={x} selected={same(x, sel)} disabled={switching} onPress={() => setSelected({ k: x.k, n: x.n })} />
              ))}
          </View>
        </View>
      ))}
      <View style={{ paddingHorizontal: 16 }}>
        <Button label="Custom" kind="text" icon={showCustom ? 'expand-less' : 'expand-more'} onPress={() => setShowCustom(!showCustom)} style={{ alignSelf: 'flex-start' }} />
        {showCustom ? (
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 4, flexWrap: 'wrap' }}>
            <Stepper value={draft.k} min={1} max={31} onChange={(k) => setDraft({ ...draft, k })} label="pieces needed" />
            <Txt>of</Txt>
            <Stepper value={draft.n} min={2} max={32} onChange={(n) => setDraft({ ...draft, n })} label="pieces in all" />
            <Button
              label="Show"
              kind="tonal"
              disabled={draft.k >= draft.n}
              onPress={() => {
                setCustom(draft);
                setSelected(draft);
                load(true, draft);
              }}
            />
          </View>
        ) : null}
      </View>

      <SectionHeader>Capacity</SectionHeader>
      <Card>
        <Capacity report={report} sel={sel} moving={needed} />
      </Card>

      <Card style={{ marginTop: 8 }}>
        <View style={{ flexDirection: 'row', alignItems: 'center' }}>
          <Txt role="bodyLarge" style={{ flex: 1 }}>
            Give friends
          </Txt>
          <IconButton icon="remove" label="Give less" onPress={() => changeGive(Math.max(0, giveNow - step))} disabled={!hasFriends || giveNow <= 0} />
          <Txt role="titleMedium" style={{ minWidth: 72, textAlign: 'center' }}>
            {bytes(giveNow)}
          </Txt>
          <IconButton icon="add" label="Give more" onPress={() => changeGive(giveNow + step)} disabled={!hasFriends} />
        </View>
      </Card>

      <View style={{ paddingHorizontal: 16, marginTop: 16, gap: 12 }}>
        {switching ? <Progress progress={progress} /> : null}
        <Button
          label={switching ? 'Switching' : !needed ? 'In use' : !sel.switchFits ? 'Not enough space' : `Switch to ${choiceName(sel)}${choiceShape(sel) ? ` (${choiceShape(sel)})` : ''}`}
          disabled={switching || !needed || !sel.available || !sel.switchFits}
          onPress={onSwitch}
          style={{ alignSelf: 'flex-end' }}
        />
      </View>
    </ScrollView>
  );
}

function ChoiceCard({ choice, selected, disabled, onPress }: { choice: Choice; selected: boolean; disabled: boolean; onPress: () => void }) {
  const { c } = useTheme();
  const off = !choice.available;
  const dim = off ? c.onSurfaceVariant : c.onSurface;
  return (
    <View
      style={{
        width: '48%',
        flexGrow: 1,
        borderRadius: 16,
        overflow: 'hidden',
        borderWidth: selected ? 2 : 1,
        borderColor: selected ? c.primary : c.outlineVariant,
        backgroundColor: off ? c.surfaceContainerLow : c.surfaceContainer,
      }}
    >
      <Pressable
        onPress={onPress}
        disabled={off || disabled}
        android_ripple={{ color: withAlpha(c.onSurface, 0.12) }}
        accessibilityRole="radio"
        accessibilityState={{ checked: selected, disabled: off || disabled }}
        accessibilityLabel={`${choiceName(choice)} ${choiceShape(choice)}, store ${bytes(choice.limitBytes)}`}
        style={{ padding: 14, gap: 6 }}
      >
        <View style={{ flexDirection: 'row', alignItems: 'baseline', gap: 8 }}>
          <Txt role="headlineSmall" color={dim}>
            {choiceName(choice)}
          </Txt>
          {choiceShape(choice) ? (
            <Txt role="bodySmall" color={c.onSurfaceVariant}>
              {choiceShape(choice)}
            </Txt>
          ) : null}
        </View>
        <Fact label="Store" value={off ? '—' : bytes(choice.limitBytes)} strong />
        <Fact label="Offline" value={offlineText(choice)} />
        <View style={{ minHeight: 24, flexDirection: 'row' }}>
          {off ? (
            <Badge text={`Needs ${choice.n} nodes`} />
          ) : choice.recommended ? (
            <Badge text="Recommended" accent />
          ) : null}
        </View>
      </Pressable>
    </View>
  );
}

function Fact({ label, value, strong }: { label: string; value: string; strong?: boolean }) {
  const { c } = useTheme();
  return (
    <View style={{ flexDirection: 'row', gap: 8 }}>
      <Txt role="bodySmall" color={c.onSurfaceVariant} style={{ width: 52 }}>
        {label}
      </Txt>
      <Txt role={strong ? 'labelLarge' : 'bodySmall'}>{value}</Txt>
    </View>
  );
}

function Badge({ text, accent }: { text: string; accent?: boolean }) {
  const { c } = useTheme();
  return (
    <View
      style={{
        borderRadius: 8,
        paddingHorizontal: 8,
        paddingVertical: 2,
        backgroundColor: accent ? c.secondaryContainer : 'transparent',
        borderWidth: accent ? 0 : 1,
        borderColor: c.outlineVariant,
      }}
    >
      <Txt role="labelMedium" color={accent ? c.onSecondaryContainer : c.onSurfaceVariant}>
        {text}
      </Txt>
    </View>
  );
}

function Stepper({ value, min, max, onChange, label }: { value: number; min: number; max: number; onChange: (v: number) => void; label: string }) {
  return (
    <View style={{ flexDirection: 'row', alignItems: 'center' }}>
      <IconButton icon="remove" label={`Fewer ${label}`} onPress={() => onChange(Math.max(min, value - 1))} disabled={value <= min} />
      <Txt role="titleMedium" style={{ minWidth: 28, textAlign: 'center' }}>
        {String(value)}
      </Txt>
      <IconButton icon="add" label={`More ${label}`} onPress={() => onChange(Math.min(max, value + 1))} disabled={value >= max} />
    </View>
  );
}

/** The selected choice's limit, where its pieces go, the room a switch takes, and every choice's limit side by side. */
function Capacity({ report, sel, moving }: { report: StorageReport; sel: Choice; moving: boolean }) {
  const { c } = useTheme();
  const home = 1 / sel.k;
  const friends = (sel.n - 1) / sel.k;
  const peakFiles = sel.n > 0 ? (sel.switchExtraBytes * sel.k) / sel.n : 0;
  const options = report.choices.filter((x) => x.available);
  const max = Math.max(...options.map((x) => x.limitBytes), report.storedBytes, 1) * 1.08;
  const over = report.storedBytes > sel.limitBytes;
  return (
    <View style={{ gap: 14 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 16, flexWrap: 'wrap' }}>
        <Txt role="displaySmall" color={over ? c.error : c.onSurface}>
          {bytes(sel.limitBytes)}
        </Txt>
        <View style={{ flex: 1, minWidth: 140, gap: 6 }}>
          <Bar segments={[{ value: home, color: withAlpha(c.primary, 0.6) }, { value: friends, color: c.primary }]} total={home + friends} height={10} />
          <Legend
            items={[
              { label: `Yours ${Number(home.toFixed(2))}×`, color: withAlpha(c.primary, 0.6) },
              { label: `Friends ${Number(friends.toFixed(2))}×`, color: c.primary },
            ]}
          />
        </View>
      </View>
      {moving && sel.available ? (
        <View style={{ flexDirection: 'row' }}>
          <Txt color={c.onSurfaceVariant} style={{ flex: 1 }}>
            While switching
          </Txt>
          <Txt role="labelLarge" color={sel.switchFits ? c.onSurface : c.error}>{`+${bytes(sel.switchExtraBytes)}`}</Txt>
        </View>
      ) : null}
      <View style={{ gap: 8 }}>
        {options.map((x) => {
          const isSel = same(x, sel);
          return (
            <View key={`${x.k}-${x.n}`} style={{ flexDirection: 'row', alignItems: 'center', gap: 10 }}>
              <Txt role={isSel ? 'labelLarge' : 'bodySmall'} color={isSel ? c.onSurface : c.onSurfaceVariant} style={{ width: 64 }}>
                {choiceName(x)}
              </Txt>
              <View style={{ flex: 1 }}>
                <Bar
                  segments={[{ value: x.limitBytes, color: isSel ? c.primary : withAlpha(c.primary, 0.3) }]}
                  total={max}
                  height={14}
                  tick={report.storedBytes}
                  band={isSel && moving && peakFiles > 0 ? { from: report.storedBytes, to: report.storedBytes + peakFiles, color: c.tertiary } : undefined}
                />
              </View>
              <Txt role="bodySmall" style={{ width: 64, textAlign: 'right' }}>
                {bytes(x.limitBytes)}
              </Txt>
            </View>
          );
        })}
      </View>
    </View>
  );
}
